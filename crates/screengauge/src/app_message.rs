//! The numbers of ScreenGauge's own messages, from WM_APP up: every one is chosen here, in one list, so two parts
//! never take the same number (WPC-302 and WPC-303, built in parallel, once did). A test holds them apart. The numbers
//! never change: the live checkers post some of them by number.

use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_APP};

/// Declares each message as a `pub const` at WM_APP plus its offset, and lists them all in `EVERY`, the tests' list,
/// so no message is left out of the test.
macro_rules! app_messages {
    ($($(#[$doc:meta])* $name:ident = $offset:literal;)+) => {
        $(
            $(#[$doc])*
            pub const $name: u32 = WM_APP + $offset;
        )+

        /// Every message above, with its name.
        #[cfg(test)]
        const EVERY: &[(&str, u32)] = &[$((stringify!($name), $name)),+];
    };
}

app_messages! {
    /// What Windows sends the resident window for the tray icon's clicks. With version 4, the event (such as
    /// `WM_CONTEXTMENU` for a right click) is the low word of lParam.
    TRAY_ICON = 1;

    /// What a frozen screen window posts to the resident window when the session core returned effects for it to
    /// carry out. Posted, never sent: closing the tools destroys these windows, which must not happen inside one of
    /// their own window procedures.
    EFFECTS_READY = 2;

    /// What a foreground [`Watch`](crate::foreground::Watch) posts to the window it tells when another program came
    /// to the front.
    OTHER_PROGRAM_IN_FRONT = 3;

    /// Posted to the resident window to show the notifications waiting, once the messages posted before it are
    /// handled.
    SHOW_WAITING_NOTIFICATIONS = 4;

    /// What a frozen screen window posts to itself when Windows says the monitors changed.
    MONITORS_CHANGED = 5;

    /// What a foreground [`Watch`](crate::foreground::Watch) posts to the window it tells when one of ScreenGauge's
    /// own windows came to the front.
    OWN_WINDOW_IN_FRONT = 6;

    /// What the keyboard hook posts to the Settings window for a key that went down or up while the field records:
    /// the wParam is its virtual-key code, and the lParam whether it went down and the modifiers held after it
    /// ([`KeyEvent::of`](crate::keyboard_hook::KeyEvent::of)).
    HOOK_KEY = 7;

    /// What the keyboard hook's thread posts to the Settings window once the hook is removed; the wParam is the number
    /// `start` was given, so a hook stopped before can't be taken for the one running.
    HOOK_ENDED = 8;

    /// What the Settings window asks of the keyboard hook's thread: record again.
    HOOK_RECORD = 9;

    /// What the Settings window asks of the keyboard hook's thread: let the keys go once they are up.
    HOOK_DRAIN = 10;

    /// What the keyboard hook's thread posts itself when the last key it kept is up.
    HOOK_DONE = 11;

    /// Posted to the resident window when Windows told something about a notification: its click, or that it went
    /// away.
    NOTIFICATION_EVENTS_READY = 12;

    /// What the tray menu posts the resident window once an item is chosen, with the item in wParam.
    TRAY_MENU_CHOSEN = 13;

    /// What the update check's thread posts to the Settings window once GitHub answered, or didn't: the answer waits
    /// in the channel `settings_window::updates` keeps.
    UPDATE_CHECKED = 14;

    /// What the tray menu posts the resident window when it opens or closes, so the tray icon's tip follows: hidden
    /// while the menu is open, which it would otherwise cover (WPC-373). Posted rather than done at once: the menu
    /// opens while Explorer waits for the icon's message to be answered, and closes on the way to the photo.
    TRAY_MENU_OPENED_OR_CLOSED = 15;
}

/// Posts `message`, carrying nothing, to `window`, which handles it once the messages before it in its queue are. A
/// window already closed refuses it, and the error says so.
pub fn post(window: HWND, message: u32) -> windows::core::Result<()> {
    // SAFETY: the message carries no pointers, so any window, even one gone meanwhile, is sound to post to: Windows
    // checks the handle and reports one that is no longer a window.
    unsafe { PostMessageW(Some(window), message, WPARAM(0), LPARAM(0)) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_two_messages_share_a_number() {
        for (i, (name, number)) in EVERY.iter().enumerate() {
            for (other, other_number) in &EVERY[i + 1..] {
                assert_ne!(number, other_number, "{name} and {other} share a number");
            }
        }
    }

    #[test]
    fn a_message_posted_arrives_carrying_nothing_and_a_closed_window_refuses_it() {
        use windows::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, DestroyWindow, HWND_MESSAGE, MSG, PM_REMOVE, PeekMessageW,
            WINDOW_EX_STYLE, WINDOW_STYLE,
        };
        use windows::core::w;

        // SAFETY: "STATIC" is a window class of Windows' own; a message-only window is never shown, and destroyed below.
        let window = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("STATIC"),
                w!("ScreenGauge app message test"),
                WINDOW_STYLE(0),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                None,
                None,
            )
        }
        .expect("a message-only window");

        post(window, EFFECTS_READY).expect("posted to a live window");
        let mut message = MSG::default();
        // SAFETY: fills a local, from this thread's queue, for the window made above.
        let found = unsafe {
            PeekMessageW(
                &mut message,
                Some(window),
                EFFECTS_READY,
                EFFECTS_READY,
                PM_REMOVE,
            )
        };
        assert!(found.as_bool(), "the message waits in the queue");
        assert_eq!(
            (
                message.hwnd,
                message.message,
                message.wParam.0,
                message.lParam.0
            ),
            (window, EFFECTS_READY, 0, 0)
        );

        // SAFETY: the window made above, on this thread.
        unsafe { DestroyWindow(window) }.expect("the window destroyed");
        assert!(
            post(window, EFFECTS_READY).is_err(),
            "a closed window refuses it"
        );
    }

    #[test]
    fn every_message_is_in_the_application_range() {
        // WM_APP to 0xBFFF is the application's own; from 0xC000 the numbers are RegisterWindowMessage's.
        for (name, number) in EVERY {
            assert!((WM_APP..0xC000).contains(number), "{name} is {number:#x}");
        }
    }
}
