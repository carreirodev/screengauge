//! Windows and message pumping shared by the hotkey and recorder modes.

use windows::Win32::Foundation::{HINSTANCE, HWND, WAIT_FAILED};
use windows::Win32::Graphics::Gdi::{COLOR_WINDOW, HBRUSH};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::GetDpiForSystem;
use windows::Win32::UI::WindowsAndMessaging::{
    CW_USEDEFAULT, CreateWindowExW, DestroyWindow, DispatchMessageW, HWND_MESSAGE, IDC_ARROW, IsWindow, LoadCursorW,
    MsgWaitForMultipleObjects, MSG, PM_REMOVE, PeekMessageW, QS_ALLINPUT, RegisterClassExW, TranslateMessage,
    WINDOW_EX_STYLE, WINDOW_STYLE, WM_QUIT, WNDCLASSEXW, WNDPROC, WS_OVERLAPPEDWINDOW, WS_VISIBLE,
};
use windows::core::PCWSTR;

use crate::common::Ctx;
use crate::log;

/// A window destroyed on drop.
pub struct OwnedWindow {
    hwnd: HWND,
}

impl OwnedWindow {
    pub fn hwnd(&self) -> HWND {
        self.hwnd
    }
}

impl Drop for OwnedWindow {
    fn drop(&mut self) {
        // SAFETY: plain query. The user can close a visible window first, which destroys it.
        if !unsafe { IsWindow(Some(self.hwnd)) }.as_bool() {
            return;
        }
        // SAFETY: the window was created by this thread and is destroyed once.
        if let Err(e) = unsafe { DestroyWindow(self.hwnd) } {
            log::line(format!("ERROR: DestroyWindow on cleanup: {e}"));
        }
    }
}

pub enum Kind {
    /// A message-only window: never shown, receives messages such as WM_HOTKEY.
    Hidden,
    /// A normal window, `width` x `height` in pixels at 96 dpi (scaled to the system dpi).
    Visible { width: i32, height: i32 },
}

/// Registers the window class (once per process) and creates one window of it.
pub fn create(class: PCWSTR, title: PCWSTR, proc: WNDPROC, kind: Kind) -> Result<OwnedWindow, String> {
    // SAFETY: asks for the handle of the running executable.
    let module = unsafe { GetModuleHandleW(None) }.ctx("GetModuleHandleW")?;
    let instance = HINSTANCE(module.0);
    // SAFETY: loads a stock cursor.
    let cursor = unsafe { LoadCursorW(None, IDC_ARROW) }.ctx("LoadCursorW")?;
    let class_info = WNDCLASSEXW {
        cbSize: u32::try_from(size_of::<WNDCLASSEXW>()).ctx("WNDCLASSEXW size")?,
        lpfnWndProc: proc,
        hInstance: instance,
        hCursor: cursor,
        // The classic "system color index + 1" brush; GDI treats it as a color, not a pointer.
        hbrBackground: HBRUSH(std::ptr::without_provenance_mut(usize::try_from(COLOR_WINDOW.0 + 1).ctx("brush")?)),
        lpszClassName: class,
        ..Default::default()
    };
    // SAFETY: `class_info` points to valid data for the duration of the call.
    if unsafe { RegisterClassExW(&class_info) } == 0 {
        return Err(format!("RegisterClassExW: {}", windows::core::Error::from_thread()));
    }

    let (style, x, y, w, h, parent) = match kind {
        Kind::Hidden => (WINDOW_STYLE(0), 0, 0, 0, 0, Some(HWND_MESSAGE)),
        Kind::Visible { width, height } => {
            // SAFETY: plain query.
            let dpi = i32::try_from(unsafe { GetDpiForSystem() }).ctx("dpi")?;
            (
                WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                width * dpi / 96,
                height * dpi / 96,
                None,
            )
        }
    };
    // SAFETY: class registered above; all pointers are valid for the call.
    let hwnd = unsafe {
        CreateWindowExW(WINDOW_EX_STYLE(0), class, title, style, x, y, w, h, parent, None, Some(instance), None)
    }
    .ctx("CreateWindowExW")?;
    Ok(OwnedWindow { hwnd })
}

/// Waits up to `wait_ms` for messages, then dispatches everything queued. Returns true once WM_QUIT was seen.
/// Out-of-context WinEvent callbacks and window procedures run inside this call.
pub fn pump(wait_ms: u32) -> Result<bool, String> {
    // SAFETY: waits on this thread's own message queue only (no handles).
    let waited = unsafe { MsgWaitForMultipleObjects(None, false, wait_ms, QS_ALLINPUT) };
    if waited == WAIT_FAILED {
        return Err(format!("MsgWaitForMultipleObjects: {}", windows::core::Error::from_thread()));
    }
    let mut msg = MSG::default();
    // SAFETY: `msg` is a valid out-parameter; messages belong to this thread.
    while unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE) }.as_bool() {
        if msg.message == WM_QUIT {
            return Ok(true);
        }
        // SAFETY: `msg` came from PeekMessageW on this thread.
        unsafe {
            // The return value only says whether a character message was generated; it is not an error code.
            _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    Ok(false)
}
