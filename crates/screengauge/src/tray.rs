//! The tray icon next to the clock. Provisional: Windows' generic app icon, until the app icon (WPC-304).

use windows::Win32::Foundation::{E_FAIL, HWND, LPARAM};
use windows::Win32::UI::Shell::{
    NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_REALTIME, NIF_SHOWTIP, NIF_TIP, NIM_ADD, NIM_DELETE,
    NIM_MODIFY, NIM_SETVERSION, NIN_SELECT, NINF_KEY, NOTIFY_ICON_MESSAGE, NOTIFYICON_VERSION_4,
    NOTIFYICONDATAW, Shell_NotifyIconW,
};
use windows::Win32::UI::WindowsAndMessaging::{IDI_APPLICATION, LoadIconW, WM_APP};
use windows::core::Error;

use crate::win32::{low_word, struct_size};

/// What Windows sends the resident window for the icon's clicks. With version 4, the event (such as
/// `WM_CONTEXTMENU` for a right click) is the low word of lParam.
pub const CALLBACK_MESSAGE: u32 = WM_APP + 1;

/// One icon per process, so its ID is a constant.
const ICON_ID: u32 = 1;

/// The windows crate has NIN_SELECT and NINF_KEY but not their combination, which Windows' headers define this way.
const NIN_KEYSELECT: u32 = NIN_SELECT | NINF_KEY;

/// Adds the icon. Also called again when the taskbar is created (see `resident`).
pub fn add(hwnd: HWND) -> windows::core::Result<()> {
    let mut data = icon_data(hwnd);
    data.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP | NIF_SHOWTIP;
    data.uCallbackMessage = CALLBACK_MESSAGE;
    // SAFETY: loads a shared system icon, which is never freed.
    data.hIcon = unsafe { LoadIconW(None, IDI_APPLICATION) }?;
    if !fill(&mut data.szTip, "ScreenGauge") {
        return Err(Error::new(E_FAIL, "the tooltip is too long"));
    }
    notify(NIM_ADD, &data, "NIM_ADD")?;
    data.Anonymous.uVersion = NOTIFYICON_VERSION_4;
    notify(NIM_SETVERSION, &data, "NIM_SETVERSION")
}

pub fn remove(hwnd: HWND) -> windows::core::Result<()> {
    notify(NIM_DELETE, &icon_data(hwnd), "NIM_DELETE")
}

/// Shows `text` as a Windows notification from the icon: Windows 11 shows a tray icon's balloon as one of its own
/// notifications (ADR 0003), and its events come back as `CALLBACK_MESSAGE`. With `right_away_only`, Windows drops it
/// when it can't show it at once. Fails when the icon isn't in the notification area, or when `text` is too long.
pub fn balloon(hwnd: HWND, text: &str, right_away_only: bool) -> windows::core::Result<()> {
    let mut data = icon_data(hwnd);
    data.uFlags = NIF_INFO;
    if right_away_only {
        data.uFlags |= NIF_REALTIME;
    }
    if !fill(&mut data.szInfo, text) {
        return Err(Error::new(
            E_FAIL,
            format!("the notification \"{text}\" is too long"),
        ));
    }
    notify(NIM_MODIFY, &data, "NIM_MODIFY")
}

/// Whether `text` fits in a balloon notification. The notifications' tests check their texts with it.
#[cfg(test)]
pub fn fits_in_a_balloon(text: &str) -> bool {
    fill(&mut icon_data(HWND::default()).szInfo, text)
}

/// Copies `text` into a null-terminated buffer, or returns false, leaving the buffer as it was, when it doesn't fit.
fn fill(buffer: &mut [u16], text: &str) -> bool {
    let units: Vec<u16> = text.encode_utf16().chain([0]).collect();
    let Some(start) = buffer.get_mut(..units.len()) else {
        return false;
    };
    start.copy_from_slice(&units);
    true
}

/// Whether a `CALLBACK_MESSAGE` calls ScreenGauge: a left click, which Windows reports once as NIN_SELECT when the
/// button is released, or Enter or Space while the keyboard is on the icon. A double click is a first click that calls
/// and a second one that lands on the frozen screen, which by then covers the taskbar.
pub fn is_call(lparam: LPARAM) -> bool {
    matches!(low_word(lparam), NIN_SELECT | NIN_KEYSELECT)
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

#[cfg(test)]
mod tests {
    use windows::Win32::UI::WindowsAndMessaging::{
        WM_CONTEXTMENU, WM_LBUTTONDBLCLK, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE,
    };

    use super::*;

    /// The lParam Windows sends with version 4: the event in the low word, the icon's ID in the high word.
    fn event(event: u32) -> LPARAM {
        LPARAM(isize::try_from(ICON_ID << 16 | event).expect("fits in an isize"))
    }

    #[test]
    fn a_left_click_calls() {
        // NIN_SELECT, which Windows sends once the button is released.
        assert!(is_call(event(0x400)));
    }

    #[test]
    fn selecting_the_icon_from_the_keyboard_calls() {
        // NIN_KEYSELECT: Enter or Space while the keyboard is on the icon.
        assert!(is_call(event(0x401)));
    }

    #[test]
    fn the_button_messages_around_a_click_dont_call_again() {
        for button in [WM_LBUTTONDOWN, WM_LBUTTONUP, WM_LBUTTONDBLCLK] {
            assert!(!is_call(event(button)), "{button:#x}");
        }
    }

    #[test]
    fn a_balloon_holds_255_characters_and_no_more() {
        assert!(fits_in_a_balloon(&"x".repeat(255)));
        assert!(!fits_in_a_balloon(&"x".repeat(256)));
    }

    #[test]
    fn text_that_doesnt_fit_leaves_the_buffer_as_it_was() {
        let mut buffer = [7; 4];
        assert!(!fill(&mut buffer, "abcd"));
        assert_eq!(buffer, [7; 4]);
        assert!(fill(&mut buffer, "abc"));
        assert_eq!(buffer, [97, 98, 99, 0]);
    }

    #[test]
    fn a_right_click_or_the_mouse_passing_over_doesnt_call() {
        assert!(!is_call(event(WM_CONTEXTMENU)));
        assert!(!is_call(event(WM_MOUSEMOVE)));
    }
}
