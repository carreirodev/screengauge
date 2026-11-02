//! Another program coming to the front while the tools are open: Alt+Tab, the Windows key, or a Windows notification
//! that opens a program. Windows reports every change of the window in front through a WinEvent hook, set only while
//! the tools are open, so nothing watches at rest. ScreenGauge's own windows, such as Save As, never count as another
//! program: one of them coming to the front is told apart, as the way back to Save As after it stepped aside behind
//! another program (WPC-318). Settings is the exception: it is a window of its own, apart from the tools, so coming to
//! the front by Alt+Tab or its taskbar button it closes them as another program would, rather than taking the keyboard
//! behind the frozen screen, where Esc would no longer reach the tools (WPC-319).

use std::cell::Cell;

use windows::Win32::Foundation::{E_FAIL, HWND, LPARAM, WPARAM};
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent};
use windows::Win32::UI::WindowsAndMessaging::{
    EVENT_SYSTEM_FOREGROUND, GetForegroundWindow, GetWindowThreadProcessId, PostMessageW,
    WINEVENT_OUTOFCONTEXT, WM_APP,
};
use windows::core::Error;

use crate::{log, settings_window};

/// What a [`Watch`] posts to the window it tells when another program came to the front.
pub const OTHER_PROGRAM_IN_FRONT: u32 = WM_APP + 3;

/// What a [`Watch`] posts to the window it tells when one of ScreenGauge's own windows came to the front.
pub const OWN_WINDOW_IN_FRONT: u32 = WM_APP + 6;

thread_local! {
    /// The live [`Watch`]'s hook and the window it tells. The hook's callback gets no context of its own, so it finds
    /// the window here, and only for that hook: a call for a hook already removed tells nobody.
    static WATCHING: Cell<Option<(HWINEVENTHOOK, HWND)>> = const { Cell::new(None) };
}

/// Tells a window, from the moment it starts until it is dropped, each time another program comes to the front. One at
/// a time, on the thread whose message loop runs the tools: a watch started while another lives takes its place.
pub struct Watch {
    hook: HWINEVENTHOOK,
}

impl Watch {
    /// Starts telling `window`, with [`OTHER_PROGRAM_IN_FRONT`] or [`OWN_WINDOW_IN_FRONT`]. Only changes from now on
    /// reach it: none made before, such as the Start menu closing on its way to calling ScreenGauge. Drop the watch
    /// before closing `window`, since closing the window in front hands the front to another program.
    pub fn start(window: HWND) -> windows::core::Result<Watch> {
        // SAFETY: an out-of-context hook, so the callback is a function of this program, which Windows calls on this
        // thread while it waits for messages; no module is needed.
        let hook = unsafe {
            SetWinEventHook(
                EVENT_SYSTEM_FOREGROUND,
                EVENT_SYSTEM_FOREGROUND,
                None,
                Some(on_foreground),
                0,
                0,
                WINEVENT_OUTOFCONTEXT,
            )
        };
        // SetWinEventHook sets no error code to read.
        if hook.is_invalid() {
            return Err(Error::new(
                E_FAIL,
                "SetWinEventHook refused to watch the window in front",
            ));
        }
        WATCHING.set(Some((hook, window)));
        Ok(Watch { hook })
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        if WATCHING.get().is_some_and(|(hook, _)| hook == self.hook) {
            WATCHING.set(None);
        }
        // SAFETY: the hook this watch set, on the thread that set it (a hook handle can't leave its thread), removed
        // once.
        if !unsafe { UnhookWinEvent(self.hook) }.as_bool() {
            log::failure(
                "foreground",
                &"UnhookWinEvent failed, so Windows keeps calling a hook that tells nobody",
            );
        }
    }
}

/// Windows' call for every change of the window in front. Posted on, never handled here: it arrives while this thread
/// waits for messages, possibly inside one of the frozen screen's window procedures, and closing the tools must not
/// happen there.
///
/// Whose window is in front is read now, not taken from the window the call names: Windows' calls come late, and out of
/// order with the change itself. Coming back to Save As by Alt+Tab, the call for Alt+Tab's own window came after the
/// one for Save As, at the same millisecond, and would have sent Save As aside again (seen in WPC-318).
extern "system" fn on_foreground(
    hook: HWINEVENTHOOK,
    _event: u32,
    _hwnd: HWND,
    _object: i32,
    _child: i32,
    _thread: u32,
    _time: u32,
) {
    let Some((watched, window)) = WATCHING.get() else {
        return;
    };
    if hook != watched {
        return;
    }
    // SAFETY: no arguments.
    let front = unsafe { GetForegroundWindow() };
    let mut process = 0;
    // SAFETY: fills a local. No window in front, or one gone meanwhile, leaves it 0, which is another program's.
    unsafe { GetWindowThreadProcessId(front, Some(&mut process)) };
    // SAFETY: no arguments.
    let message = if process == unsafe { GetCurrentProcessId() } && !settings_window::is(front) {
        OWN_WINDOW_IN_FRONT
    } else {
        OTHER_PROGRAM_IN_FRONT
    };
    // SAFETY: a window of this thread; should it be closed already, PostMessageW fails and says so. The message
    // carries nothing.
    let posted = unsafe { PostMessageW(Some(window), message, WPARAM(0), LPARAM(0)) };
    if let Err(e) = posted {
        log::failure("foreground: telling the frozen screen", &e);
    }
}
