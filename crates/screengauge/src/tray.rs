//! The tray icon next to the clock. Provisional: Windows' generic app icon, until the app icon (WPC-304).

use windows::Win32::Foundation::{E_FAIL, HWND};
use windows::Win32::UI::Shell::{
    NIF_ICON, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_SETVERSION,
    NOTIFYICON_VERSION_4, NOTIFYICONDATAW, Shell_NotifyIconW,
};
use windows::Win32::UI::WindowsAndMessaging::{IDI_APPLICATION, LoadIconW, WM_APP};
use windows::core::Error;

use crate::win32::struct_size;

/// What Windows sends the resident window for the icon's clicks. With version 4, the event (such as
/// `WM_CONTEXTMENU` for a right click) is the low word of lParam.
pub const CALLBACK_MESSAGE: u32 = WM_APP + 1;

/// One icon per process, so its ID is a constant.
const ICON_ID: u32 = 1;

/// Adds the icon. Also called again when Explorer restarts, which drops every tray icon.
pub fn add(hwnd: HWND) -> windows::core::Result<()> {
    let mut data = data(hwnd);
    data.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP | NIF_SHOWTIP;
    data.uCallbackMessage = CALLBACK_MESSAGE;
    // SAFETY: loads a shared system icon, which is never freed.
    data.hIcon = unsafe { LoadIconW(None, IDI_APPLICATION) }?;
    for (slot, unit) in data.szTip.iter_mut().zip("ScreenGauge".encode_utf16()) {
        *slot = unit;
    }
    // SAFETY: `data` is fully initialized and its cbSize matches its type.
    if !unsafe { Shell_NotifyIconW(NIM_ADD, &data) }.as_bool() {
        return Err(failure("Shell_NotifyIconW(NIM_ADD)"));
    }
    data.Anonymous.uVersion = NOTIFYICON_VERSION_4;
    // SAFETY: as above.
    if !unsafe { Shell_NotifyIconW(NIM_SETVERSION, &data) }.as_bool() {
        return Err(failure("Shell_NotifyIconW(NIM_SETVERSION)"));
    }
    Ok(())
}

pub fn remove(hwnd: HWND) -> windows::core::Result<()> {
    // SAFETY: `data` names the icon by its window and ID, and its cbSize matches its type.
    if !unsafe { Shell_NotifyIconW(NIM_DELETE, &data(hwnd)) }.as_bool() {
        return Err(failure("Shell_NotifyIconW(NIM_DELETE)"));
    }
    Ok(())
}

fn data(hwnd: HWND) -> NOTIFYICONDATAW {
    NOTIFYICONDATAW {
        cbSize: const { struct_size::<NOTIFYICONDATAW>() },
        hWnd: hwnd,
        uID: ICON_ID,
        ..Default::default()
    }
}

/// Shell_NotifyIconW returns only FALSE and documents no error code, so the message names the call.
fn failure(call: &str) -> Error {
    Error::new(E_FAIL, format!("{call} returned FALSE"))
}
