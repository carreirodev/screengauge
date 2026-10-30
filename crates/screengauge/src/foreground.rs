//! Another program coming to the front while the tools are open: Alt+Tab, the Windows key, or a Windows notification
//! that opens a program. Windows reports every change of the window in front through a WinEvent hook, set only while
//! the tools are open, so nothing watches at rest. The hook leaves out ScreenGauge's own windows, such as Save As and
//! the window for choosing a folder, which never count as another program.

use std::cell::Cell;

use windows::Win32::Foundation::{E_FAIL, HWND, LPARAM, WPARAM};
use windows::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent};
use windows::Win32::UI::WindowsAndMessaging::{
    EVENT_SYSTEM_FOREGROUND, PostMessageW, WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS, WM_APP,
};
use windows::core::Error;

use crate::log;

/// What a [`Watch`] posts to the window it tells when another program came to the front.
pub const OTHER_PROGRAM_IN_FRONT: u32 = WM_APP + 3;

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
    /// Starts telling `window`, with [`OTHER_PROGRAM_IN_FRONT`]. Only changes from now on reach it: none made before,
    /// such as the Start menu closing on its way to calling ScreenGauge. Drop the watch before closing `window`, since
    /// closing the window in front hands the front to another program.
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
                WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
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

/// Windows' call for every window that comes to the front, but ScreenGauge's own. Posted on, never handled here: it
/// arrives while this thread waits for messages, possibly inside one of the frozen screen's window procedures, and
/// closing the tools must not happen there.
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
    // SAFETY: a window of this thread; should it be closed already, PostMessageW fails and says so. The message
    // carries nothing.
    let posted =
        unsafe { PostMessageW(Some(window), OTHER_PROGRAM_IN_FRONT, WPARAM(0), LPARAM(0)) };
    if let Err(e) = posted {
        log::failure("foreground: telling the frozen screen", &e);
    }
}
