//! The resident process: one window that is never shown, so ScreenGauge has no taskbar button while idle.
//! It receives what Windows tells ScreenGauge: the global shortcut, the tray icon's clicks and the calls of a copy
//! opened again. Nothing is prepared at startup and there is no timer: the thread sleeps in GetMessageW until
//! Windows sends something. While the tools are open, it carries out the effects the session core returns.

use std::cell::RefCell;
use std::sync::atomic::{AtomicU32, Ordering};

use screengauge_core::session::{Effect, Input, Opening, Session};
use screengauge_core::settings::{Remembered, Stored};
use windows::Win32::Foundation::{
    E_FAIL, ERROR_SUCCESS, HINSTANCE, HWND, LPARAM, LRESULT, SetLastError, WPARAM,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
    FindWindowW, GetMessageW, GetWindowThreadProcessId, MSG, PostMessageW, PostQuitMessage,
    RegisterClassExW, RegisterWindowMessageW, TranslateMessage, WINDOW_EX_STYLE, WM_CONTEXTMENU,
    WM_DESTROY, WM_HOTKEY, WNDCLASSEXW, WS_OVERLAPPED,
};
use windows::core::{Error, PCWSTR, w};

use crate::frozen_screen::{self, FrozenScreen};
use crate::notification::{self, Notification};
use crate::settings_file::SettingsFile;
use crate::win32::{low_word, struct_size};
use crate::{capture, event, global_shortcut, log, tray};

/// The resident window's class, which a copy opened again looks for to call the running copy.
const CLASS_NAME: PCWSTR = w!("ScreenGauge.Resident");

/// Explorer broadcasts this message when the taskbar is created: at sign-in, and when Explorer restarts
/// and has dropped every tray icon.
static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);

/// The call that a copy opened again sends the running copy; the running copy also sends it to itself, from the tray
/// icon and at startup, so every call but the global shortcut's goes the same way. Registered by name, so each copy
/// gets the same number, whichever installed it.
const CALL_NAME: PCWSTR = w!("ScreenGauge.Call");
static CALL: AtomicU32 = AtomicU32::new(0);

thread_local! {
    /// The frozen screen, with its session, while the tools are open. Only the resident window's procedure touches it.
    /// Opening and closing set it and take it out, never while it is borrowed, because they show and destroy windows;
    /// what runs while it is borrowed dispatches no message.
    static FROZEN_SCREEN: RefCell<Option<FrozenScreen>> = const { RefCell::new(None) };
    /// What the settings file holds, and the file to save it to. Set once, as the process starts.
    static SETTINGS_FILE: RefCell<Option<LoadedSettingsFile>> = const { RefCell::new(None) };
}

/// The settings file as ScreenGauge read it at startup, kept up to date as it saves.
struct LoadedSettingsFile {
    /// `None` when the file couldn't be opened: ScreenGauge then runs with the defaults and saves nothing.
    file: Option<SettingsFile>,
    stored: Stored,
}

/// Calls ScreenGauge right away, since opening it is a call, then runs until the user exits from the tray icon, with
/// what the settings file holds and the file to save it to.
pub fn run(file: Option<SettingsFile>, stored: Stored) -> windows::core::Result<()> {
    let shortcut = stored.options.global_shortcut;
    SETTINGS_FILE.set(Some(LoadedSettingsFile { file, stored }));
    TASKBAR_CREATED.store(register_message(w!("TaskbarCreated"))?, Ordering::Relaxed);
    CALL.store(register_message(CALL_NAME)?, Ordering::Relaxed);

    // SAFETY: no module name, so this is the handle of ScreenGauge.exe itself.
    let instance: HINSTANCE = unsafe { GetModuleHandleW(None) }?.into();
    let class = WNDCLASSEXW {
        cbSize: const { struct_size::<WNDCLASSEXW>() },
        lpfnWndProc: Some(window_proc),
        hInstance: instance,
        lpszClassName: CLASS_NAME,
        ..Default::default()
    };
    // SAFETY: `class` is fully initialized, and its name is a static string.
    if unsafe { RegisterClassExW(&class) } == 0 {
        return Err(Error::from_thread());
    }
    frozen_screen::register_class(instance)?;
    // SAFETY: the class was just registered in this module. The window is never shown.
    let hwnd = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            CLASS_NAME,
            w!("ScreenGauge"),
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(instance),
            None,
        )
    }?;
    // At sign-in the taskbar may not exist yet; the icon then comes with TaskbarCreated, so a failure
    // here leaves ScreenGauge running.
    if let Err(e) = tray::add(hwnd) {
        log::failure("tray icon: adding it at startup", &e);
    }
    // Another program may hold the combination; ScreenGauge keeps running, and telling the user comes with WPC-324.
    if let Err(e) = global_shortcut::register(hwnd, shortcut) {
        log::failure(&format!("global shortcut: registering {shortcut}"), &e);
    }
    post_call(hwnd);
    // After the call, so a notification from startup, such as the damaged settings file, never lands in its photo.
    notification::post_show_waiting(hwnd);

    let mut message = MSG::default();
    loop {
        // SAFETY: `message` is a valid MSG for the call to fill.
        match unsafe { GetMessageW(&mut message, None, 0, 0) }.0 {
            -1 => return Err(Error::from_thread()),
            0 => return Ok(()),
            _ => {
                // SAFETY: a message GetMessageW just returned. TranslateMessage's result only says whether
                // a character message was posted, so there is nothing to check.
                unsafe {
                    let _ = TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
            }
        }
    }
}

extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        tray::CALLBACK_MESSAGE => {
            // Explorer sends this message and waits for the answer, so the call is posted and answered at once: the
            // photo is taken once the click is over, with only the icon's highlight left of it.
            if tray::is_call(lparam) {
                post_call(hwnd);
            }
            notification::on_tray_event(low_word(lparam));
            // Provisional exit, until the tray icon's menu exists (WPC-325).
            if low_word(lparam) == WM_CONTEXTMENU {
                // SAFETY: our own window, on the thread that created it.
                if let Err(e) = unsafe { DestroyWindow(hwnd) } {
                    log::failure("exit: DestroyWindow", &e);
                }
            }
            LRESULT(0)
        }
        WM_HOTKEY if global_shortcut::is_global_shortcut(wparam) => {
            on_call(hwnd, Input::GlobalShortcut);
            LRESULT(0)
        }
        frozen_screen::EFFECTS_READY => {
            let effects = FROZEN_SCREEN.with_borrow(|open| {
                open.as_ref()
                    .map(FrozenScreen::take_effects)
                    .unwrap_or_default()
            });
            carry_out(effects);
            LRESULT(0)
        }
        notification::SHOW_WAITING => {
            notification::show_waiting(hwnd);
            LRESULT(0)
        }
        WM_DESTROY => {
            close();
            frozen_screen::release_factory();
            if let Err(e) = global_shortcut::unregister(hwnd) {
                log::failure("exit: unregistering the global shortcut", &e);
            }
            if let Err(e) = tray::remove(hwnd) {
                log::failure("exit: removing the tray icon", &e);
            }
            // SAFETY: ends this thread's message loop.
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        _ if message == TASKBAR_CREATED.load(Ordering::Relaxed) => {
            match tray::add(hwnd) {
                // The notifications waiting for the icon can show now.
                Ok(()) => notification::show_waiting(hwnd),
                Err(e) => log::failure("tray icon: adding it when the taskbar was created", &e),
            }
            LRESULT(0)
        }
        _ if message == CALL.load(Ordering::Relaxed) => {
            on_call(hwnd, Input::Called);
            LRESULT(0)
        }
        // SAFETY: the arguments are the ones Windows passed to this window procedure.
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}

/// A call opens the tools, or, when they are open, goes to the session core as `input`: the global shortcut closes
/// them, and the tray icon or opening ScreenGauge again (`Input::Called`) leaves them as they are.
fn on_call(resident: HWND, input: Input) {
    let told = FROZEN_SCREEN.with_borrow(|open| {
        open.as_ref()
            .map(|frozen| frozen.handle(event::of_message(input)))
    });
    match told {
        Some(effects) => carry_out(effects),
        None => match open(resident) {
            Ok(frozen) => FROZEN_SCREEN.set(Some(frozen)),
            // Whatever kept the tools closed, the user hears of it, never a failure in silence (v1 spec WPC-296,
            // Frozen screen). Most often it is a monitor neither capture path could photograph.
            Err(e) => {
                log::failure("frozen screen: opening it", &e);
                notification::show(resident, Notification::CaptureFailed);
            }
        },
    }
}

/// Opens the tools: the photo comes first, before any window of ScreenGauge appears or takes the keyboard, so whatever
/// was on screen at the call (a hover, an open menu, a tooltip) is in it.
fn open(resident: HWND) -> windows::core::Result<FrozenScreen> {
    let frozen_screen = capture::every_monitor()?;
    let Stored {
        options,
        remembered,
    } = SETTINGS_FILE
        .with_borrow(|loaded| loaded.as_ref().map(|loaded| loaded.stored.clone()))
        .ok_or_else(|| Error::new(E_FAIL, "the settings file wasn't read at startup"))?;
    let session = Session::open(Opening {
        frozen_screen,
        options,
        remembered,
    })
    .map_err(|refused| Error::new(E_FAIL, refused.to_string()))?;
    FrozenScreen::show(session, resident)
}

/// Does what the session core asked, in its order.
fn carry_out(effects: Vec<Effect>) {
    for effect in effects {
        let not_built = match effect {
            Effect::Close => {
                close();
                continue;
            }
            Effect::SaveRemembered(remembered) => {
                save_remembered(remembered);
                continue;
            }
            // No rule asks for these yet: the tools that do bring them (Dimensions' copy with WPC-308, Snip with
            // WPC-317 and WPC-318, the gear with WPC-319). The log never shows their content, which may hold a path.
            Effect::CopyText(_) => "copying text",
            Effect::CopyImage(_) => "copying an image",
            Effect::WritePng { .. } => "writing a PNG file",
            Effect::OpenSaveAs { .. } => "opening Save As",
            Effect::OpenSettings => "opening Settings",
        };
        log::failure("session", &format!("{not_built} isn't built yet"));
    }
}

/// Dropping the frozen screen closes its windows, drops the session and erases the photos.
fn close() {
    drop(FROZEN_SCREEN.with_borrow_mut(Option::take));
}

/// Keeps `remembered` for the next call, and in the settings file when there is one.
fn save_remembered(remembered: Remembered) {
    SETTINGS_FILE.with_borrow_mut(|loaded| {
        let Some(loaded) = loaded else {
            log::failure(
                "settings: saving the remembered state",
                &"the settings file wasn't read at startup",
            );
            return;
        };
        loaded.stored.remembered = remembered;
        if let Some(file) = &loaded.file
            && let Err(e) = file.save(&loaded.stored)
        {
            log::failure("settings: saving the remembered state", &e);
        }
    });
}

/// Posts the call to this copy's own resident window, which handles it once the message being handled is answered.
fn post_call(resident: HWND) {
    // SAFETY: our own window; the message carries nothing.
    if let Err(e) = unsafe {
        PostMessageW(
            Some(resident),
            CALL.load(Ordering::Relaxed),
            WPARAM(0),
            LPARAM(0),
        )
    } {
        log::failure("call: posting it", &e);
    }
}

/// For a copy opened while another runs: calls the running copy. Only the program the user just opened may bring a
/// window to the front, so it passes that right on to the running copy, whose frozen screen can then take the keyboard.
pub fn call_running_copy() -> windows::core::Result<()> {
    call_window_of_class(CLASS_NAME)
}

/// `call_running_copy` for a resident window of class `class`, a null-terminated name that outlives the call; the
/// tests give a class of their own.
fn call_window_of_class(class: PCWSTR) -> windows::core::Result<()> {
    let call_message = register_message(CALL_NAME)?;
    // FindWindowW sets no error code when no window has the class, and claiming the single instance has just left
    // one, so it is cleared first.
    // SAFETY: sets this thread's last error.
    unsafe { SetLastError(ERROR_SUCCESS) };
    // SAFETY: a valid class name, as this function requires, and no window name.
    let running = unsafe { FindWindowW(class, PCWSTR::null()) }.map_err(|e| {
        // The running copy may have no window yet: opened twice in quick succession, the first copy still calls itself
        // once it has one.
        if e.code().is_ok() {
            Error::new(E_FAIL, "the running copy has no resident window")
        } else {
            e
        }
    })?;
    let mut process = 0;
    // SAFETY: fills a local.
    if unsafe { GetWindowThreadProcessId(running, Some(&mut process)) } == 0 {
        // The window closed since FindWindowW: the running copy is exiting.
        return Err(Error::from_thread());
    }
    // Without the right, the frozen screen still opens, and the keyboard reaches it once it is clicked.
    // SAFETY: a process ID; Windows checks that this process may give the right.
    if let Err(e) = unsafe { AllowSetForegroundWindow(process) } {
        log::failure("call: letting the running copy come to the front", &e);
    }
    // SAFETY: the message carries nothing.
    unsafe { PostMessageW(Some(running), call_message, WPARAM(0), LPARAM(0)) }
}

/// A message number every program gets for the same `name`, a null-terminated name that outlives the call.
fn register_message(name: PCWSTR) -> windows::core::Result<u32> {
    // SAFETY: a valid, null-terminated message name.
    match unsafe { RegisterWindowMessageW(name) } {
        0 => Err(Error::from_thread()),
        message => Ok(message),
    }
}

#[cfg(test)]
mod tests {
    use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, SetLastError};
    use windows::Win32::UI::WindowsAndMessaging::{PM_REMOVE, PeekMessageW};
    use windows::core::HSTRING;

    use super::*;

    /// A window class of its own per test, so a ScreenGauge running on this computer never receives a test's call.
    fn test_class(test: &str) -> HSTRING {
        HSTRING::from(format!("ScreenGauge.Test.{}.{test}", std::process::id()))
    }

    extern "system" fn default_proc(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        // SAFETY: the arguments are the ones Windows passed to this window procedure.
        unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
    }

    /// A hidden top-level window of `class`, standing in for the running copy's resident window.
    fn running_copy(class: &HSTRING) -> HWND {
        // SAFETY: no module name, so this is the handle of the test executable.
        let instance: HINSTANCE = unsafe { GetModuleHandleW(None) }
            .expect("GetModuleHandleW")
            .into();
        let window_class = WNDCLASSEXW {
            cbSize: const { struct_size::<WNDCLASSEXW>() },
            lpfnWndProc: Some(default_proc),
            hInstance: instance,
            lpszClassName: PCWSTR(class.as_ptr()),
            ..Default::default()
        };
        // SAFETY: `window_class` is fully initialized, and the class name outlives the test.
        assert_ne!(unsafe { RegisterClassExW(&window_class) }, 0);
        // SAFETY: the class was just registered. The window is never shown.
        unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                PCWSTR(class.as_ptr()),
                None,
                WS_OVERLAPPED,
                0,
                0,
                0,
                0,
                None,
                None,
                Some(instance),
                None,
            )
        }
        .expect("CreateWindowExW")
    }

    #[test]
    fn a_copy_opened_again_calls_the_running_copy() {
        let class = test_class("running");
        let running = running_copy(&class);

        call_window_of_class(PCWSTR(class.as_ptr())).expect("calling the running copy");

        let call_message = register_message(CALL_NAME).expect("the call message");
        let mut message = MSG::default();
        // SAFETY: `message` is a valid MSG for the call to fill; the window is this thread's.
        let received = unsafe {
            PeekMessageW(
                &mut message,
                Some(running),
                call_message,
                call_message,
                PM_REMOVE,
            )
        }
        .as_bool();
        assert!(received, "the running copy received no call");
        // SAFETY: the test's own window, on the thread that created it.
        unsafe { DestroyWindow(running) }.expect("DestroyWindow");
    }

    #[test]
    fn with_no_copy_running_the_call_fails_and_says_so() {
        let class = test_class("none");
        // What claiming the single instance leaves behind in a copy opened again, so the failure can't be read from it.
        // SAFETY: sets this thread's last error.
        unsafe { SetLastError(ERROR_ALREADY_EXISTS) };
        let error = call_window_of_class(PCWSTR(class.as_ptr()))
            .expect_err("there is no running copy to call");
        assert_eq!(error.code(), E_FAIL, "{error}");
    }
}
