//! The tray icon next to the clock. Provisional: Windows' generic app icon, until the app icon (WPC-304).

use windows::Win32::Foundation::{E_FAIL, HWND};
use windows::Win32::UI::Shell::{
    NIF_ICON, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_SETVERSION,
    NOTIFY_ICON_MESSAGE, NOTIFYICON_VERSION_4, NOTIFYICONDATAW, Shell_NotifyIconW,
};
use windows::Win32::UI::WindowsAndMessaging::{IDI_APPLICATION, LoadIconW, WM_APP};
use windows::core::Error;

use crate::win32::struct_size;

/// What Windows sends the resident window for the icon's clicks. With version 4, the event (such as
/// `WM_CONTEXTMENU` for a right click) is the low word of lParam.
pub const CALLBACK_MESSAGE: u32 = WM_APP + 1;

/// One icon per process, so its ID is a constant.
const ICON_ID: u32 = 1;

/// Adds the icon. Also called again when the taskbar is created (see `resident`).
pub fn add(hwnd: HWND) -> windows::core::Result<()> {
    let mut data = icon_data(hwnd);
    data.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP | NIF_SHOWTIP;
    data.uCallbackMessage = CALLBACK_MESSAGE;
    // SAFETY: loads a shared system icon, which is never freed.
    data.hIcon = unsafe { LoadIconW(None, IDI_APPLICATION) }?;
    for (slot, unit) in data.szTip.iter_mut().zip("ScreenGauge".encode_utf16()) {
        *slot = unit;
    }
    notify(NIM_ADD, &data, "NIM_ADD")?;
    data.Anonymous.uVersion = NOTIFYICON_VERSION_4;
    notify(NIM_SETVERSION, &data, "NIM_SETVERSION")
}

pub fn remove(hwnd: HWND) -> windows::core::Result<()> {
    notify(NIM_DELETE, &icon_data(hwnd), "NIM_DELETE")
}

/// What names the icon to Windows: its window and ID.
fn icon_data(hwnd: HWND) -> NOTIFYICONDATAW {
    NOTIFYICONDATAW {
        cbSize: const { struct_size::<NOTIFYICONDATAW>() },
        hWnd: hwnd,
        uID: ICON_ID,
        ..Default::default()
    }
}

/// Shell_NotifyIconW returns only FALSE and documents no error code, so the error names the message.
fn notify(
    message: NOTIFY_ICON_MESSAGE,
    data: &NOTIFYICONDATAW,
    message_name: &str,
) -> windows::core::Result<()> {
    // SAFETY: `data` starts from `icon_data`, so it is initialized and its cbSize matches its type.
    if unsafe { Shell_NotifyIconW(message, data) }.as_bool() {
        Ok(())
    } else {
        Err(Error::new(
            E_FAIL,
            format!("Shell_NotifyIconW({message_name}) returned FALSE"),
        ))
    }
}
