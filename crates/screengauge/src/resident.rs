//! The resident process: one window that is never shown, so ScreenGauge has no taskbar button while idle.
//! It receives what Windows tells ScreenGauge (for now, the tray icon's clicks). Nothing is prepared at
//! startup and there is no timer: the thread sleeps in GetMessageW until Windows sends something.

use std::sync::atomic::{AtomicU32, Ordering};

use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW, MSG,
    PostQuitMessage, RegisterClassExW, RegisterWindowMessageW, TranslateMessage, WINDOW_EX_STYLE,
    WM_CONTEXTMENU, WM_DESTROY, WNDCLASSEXW, WS_OVERLAPPED,
};
use windows::core::{Error, PCWSTR, w};

use crate::win32::{low_word, struct_size};
use crate::{log, tray};

const CLASS_NAME: PCWSTR = w!("ScreenGauge.Resident");

/// Explorer broadcasts this message when it restarts and has dropped every tray icon.
static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);

/// Runs until the user exits from the tray icon.
pub fn run() -> windows::core::Result<()> {
    // SAFETY: registers a constant message name.
    let taskbar_created = unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) };
    if taskbar_created == 0 {
        return Err(Error::from_thread());
    }
    TASKBAR_CREATED.store(taskbar_created, Ordering::Relaxed);

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
    tray::add(hwnd)?;

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
            // Provisional exit, until the tray icon's menu exists (WPC-325).
            if low_word(lparam) == WM_CONTEXTMENU {
                // SAFETY: our own window, on the thread that created it.
                if let Err(e) = unsafe { DestroyWindow(hwnd) } {
                    log::line(&format!("exit: DestroyWindow: {e}"));
                }
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            if let Err(e) = tray::remove(hwnd) {
                log::line(&format!("exit: removing the tray icon: {e}"));
            }
            // SAFETY: ends this thread's message loop.
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        _ if message == TASKBAR_CREATED.load(Ordering::Relaxed) => {
            if let Err(e) = tray::add(hwnd) {
                log::line(&format!(
                    "tray icon: adding it again after Explorer restarted: {e}"
                ));
            }
            LRESULT(0)
        }
        // SAFETY: the arguments are the ones Windows passed to this window procedure.
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}
