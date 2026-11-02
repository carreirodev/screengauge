//! The session core's events, as the Windows layer makes them: what happened, with where the cursor was, the modifier
//! keys held, the local time and how long Windows has been running, read here so the core never reads them itself.

use std::time::Duration;

use screengauge_core::session::{Event, Held, Input, Key, LocalTime, Point};
use windows::Win32::Foundation::{POINT, SYSTEMTIME, WPARAM};
use windows::Win32::System::SystemInformation::{GetLocalTime, GetTickCount64};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, VIRTUAL_KEY, VK_1, VK_2, VK_3, VK_4, VK_BACK, VK_C, VK_CONTROL, VK_DELETE,
    VK_DOWN, VK_ESCAPE, VK_H, VK_LEFT, VK_MENU, VK_RIGHT, VK_SHIFT, VK_UP, VK_V,
};
use windows::Win32::UI::WindowsAndMessaging::{GetCursorPos, GetMessagePos};

use crate::log;
use crate::win32::signed_words;

/// `input`, as part of the message this thread is handling: the cursor's place when Windows sent it, the modifier keys
/// held as of that message, and the local time and Windows' running time now.
pub fn of_message(input: Input) -> Event {
    // SAFETY: no arguments.
    let time = unsafe { GetLocalTime() };
    Event {
        cursor: message_cursor(),
        held: held(),
        time: local_time(time),
        uptime: uptime(),
        input,
    }
}

/// `input`, as something the Windows layer did for the session, such as closing Save As or writing a file: the cursor
/// where it is now, since a window's own message loop may have run meanwhile, and the rest as [`of_message`] reads it.
pub fn now(input: Input) -> Event {
    let mut event = of_message(input);
    match cursor_now() {
        Ok(cursor) => event.cursor = cursor,
        // The last message's place, then: a notice may show a little off.
        Err(e) => log::failure("event: finding the cursor", &e),
    }
    event
}

/// Where the cursor is now, in physical pixels of the virtual screen: the process is Per-Monitor V2.
pub fn cursor_now() -> windows::core::Result<Point> {
    let mut cursor = POINT::default();
    // SAFETY: fills a local.
    unsafe { GetCursorPos(&mut cursor) }?;
    Ok(Point {
        x: cursor.x,
        y: cursor.y,
    })
}

/// Where the cursor was when Windows sent the message this thread is handling, in physical pixels of the virtual
/// screen: the place an event made of that message carries.
pub fn message_cursor() -> Point {
    // SAFETY: no arguments; reads the cursor's place for the message this thread last retrieved.
    point(unsafe { GetMessagePos() })
}

/// How long Windows has been running, to the millisecond, on the clock its timers count on.
pub fn uptime() -> Duration {
    // SAFETY: no arguments.
    Duration::from_millis(unsafe { GetTickCount64() })
}

fn held() -> Held {
    let down = |key: VIRTUAL_KEY| {
        // SAFETY: reads a key's state as of the message being handled.
        let state = unsafe { GetKeyState(i32::from(key.0)) };
        // A set high bit, so a negative value, is a key held down.
        state < 0
    };
    Held {
        shift: down(VK_SHIFT),
        ctrl: down(VK_CONTROL),
        alt: down(VK_MENU),
    }
}

/// GetMessagePos's place: x in the low 16 bits and y in the high ones, both signed. With Per-Monitor V2, they are
/// physical pixels of the virtual screen.
fn point(position: u32) -> Point {
    // u32 to usize is lossless on x64, the only target ScreenGauge builds for (main.rs).
    let (x, y) = signed_words(position as usize);
    Point {
        x: i32::from(x),
        y: i32::from(y),
    }
}

fn local_time(time: SYSTEMTIME) -> LocalTime {
    LocalTime {
        year: time.wYear,
        month: time.wMonth,
        day: time.wDay,
        hour: time.wHour,
        minute: time.wMinute,
        second: time.wSecond,
    }
}

/// How far a WM_MOUSEWHEEL's wheel turned: the signed high word of its wParam.
pub fn wheel(wparam: WPARAM) -> i32 {
    i32::from(signed_words(wparam.0).1)
}

/// The key a key message's wParam names, when it is one the tools use. 1 to 4 are the main keys' digits.
pub fn key(wparam: WPARAM) -> Option<Key> {
    let code = VIRTUAL_KEY(u16::try_from(wparam.0).ok()?);
    Some(match code {
        VK_H => Key::H,
        VK_V => Key::V,
        VK_C => Key::C,
        VK_1 => Key::Digit1,
        VK_2 => Key::Digit2,
        VK_3 => Key::Digit3,
        VK_4 => Key::Digit4,
        VK_LEFT => Key::Left,
        VK_RIGHT => Key::Right,
        VK_UP => Key::Up,
        VK_DOWN => Key::Down,
        VK_ESCAPE => Key::Esc,
        VK_BACK => Key::Backspace,
        VK_DELETE => Key::Delete,
        VK_SHIFT => Key::Shift,
        VK_MENU => Key::Alt,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::UI::Input::KeyboardAndMouse::{VK_5, VK_F4, VK_NUMPAD1, VK_X};

    fn wparam(key: VIRTUAL_KEY) -> WPARAM {
        WPARAM(usize::from(key.0))
    }

    #[test]
    fn the_keys_the_tools_use_are_told_apart() {
        let keys: Vec<_> = [VK_ESCAPE, VK_H, VK_1, VK_4, VK_LEFT, VK_DELETE, VK_MENU]
            .into_iter()
            .map(|code| key(wparam(code)))
            .collect();
        assert_eq!(
            keys,
            [
                Some(Key::Esc),
                Some(Key::H),
                Some(Key::Digit1),
                Some(Key::Digit4),
                Some(Key::Left),
                Some(Key::Delete),
                Some(Key::Alt),
            ]
        );
    }

    #[test]
    fn other_keys_are_left_to_windows() {
        for code in [VK_X, VK_5, VK_F4, VK_NUMPAD1] {
            assert_eq!(key(wparam(code)), None, "{code:?}");
        }
    }

    #[test]
    fn a_place_left_of_the_main_monitor_keeps_its_sign() {
        // x = -1920 and y = 1079, packed the way GetMessagePos packs them.
        let position = (1079 << 16) | u32::from((-1920_i16).cast_unsigned());
        assert_eq!(point(position), Point { x: -1920, y: 1079 });
    }

    #[test]
    fn the_wheel_turned_toward_the_user_is_negative() {
        let toward = WPARAM(usize::from((-120_i16).cast_unsigned()) << 16);
        let away = WPARAM(120 << 16);
        assert_eq!((wheel(toward), wheel(away)), (-120, 120));
    }
}
