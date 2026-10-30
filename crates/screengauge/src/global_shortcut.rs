//! The global shortcut, registered with Windows for the resident window, which then receives WM_HOTKEY whenever the
//! user presses it, in any program. The combination comes from the settings file (`Win + Shift + X` by default).

use screengauge_core::settings::{Key, Shortcut};
use windows::Win32::Foundation::{ERROR_HOTKEY_NOT_REGISTERED, HWND, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN, RegisterHotKey,
    UnregisterHotKey, VK_F1,
};

/// ScreenGauge has one global shortcut, so its ID is a constant.
const ID: i32 = 1;

/// Registers `shortcut` for `hwnd`. It fails when another program already holds the combination; telling the user
/// comes with WPC-324.
pub fn register(hwnd: HWND, shortcut: Shortcut) -> windows::core::Result<()> {
    let (modifiers, key) = combination(shortcut);
    // SAFETY: our own window, on the thread that created it.
    unsafe { RegisterHotKey(Some(hwnd), ID, modifiers, key) }
}

/// Undoes `register`. When registering had failed, there is nothing to undo.
pub fn unregister(hwnd: HWND) -> windows::core::Result<()> {
    // SAFETY: as in `register`.
    match unsafe { UnregisterHotKey(Some(hwnd), ID) } {
        Err(e) if e.code() == ERROR_HOTKEY_NOT_REGISTERED.to_hresult() => Ok(()),
        result => result,
    }
}

/// Whether a WM_HOTKEY's wParam names the global shortcut.
pub fn is_global_shortcut(wparam: WPARAM) -> bool {
    i32::try_from(wparam.0) == Ok(ID)
}

/// The modifiers and the virtual-key code RegisterHotKey takes for `shortcut`. With MOD_NOREPEAT, holding the keys
/// down calls ScreenGauge once: the same keys also close the tools, so a repeat would open and close them in turn.
fn combination(shortcut: Shortcut) -> (HOT_KEY_MODIFIERS, u32) {
    let held = shortcut.modifiers();
    let mut modifiers = MOD_NOREPEAT;
    for (is_held, modifier) in [
        (held.win, MOD_WIN),
        (held.ctrl, MOD_CONTROL),
        (held.alt, MOD_ALT),
        (held.shift, MOD_SHIFT),
    ] {
        if is_held {
            modifiers |= modifier;
        }
    }
    // The virtual-key codes of A to Z and 0 to 9 are their ASCII codes.
    let key = match shortcut.key() {
        Key::Letter(letter) => u32::from(letter),
        Key::Digit(digit) => u32::from(b'0') + u32::from(digit),
        // F1 to F12 are consecutive codes.
        Key::Function(number) => u32::from(VK_F1.0) + u32::from(number) - 1,
    };
    (modifiers, key)
}

#[cfg(test)]
mod tests {
    use screengauge_core::settings::Modifiers;

    use super::*;

    fn shortcut(modifiers: Modifiers, key: Key) -> Shortcut {
        Shortcut::new(modifiers, key).expect("an allowed combination")
    }

    #[test]
    fn the_default_is_win_shift_x() {
        assert_eq!(
            combination(Shortcut::default()),
            (MOD_WIN | MOD_SHIFT | MOD_NOREPEAT, 0x58)
        );
    }

    #[test]
    fn every_modifier_and_a_number() {
        let all = Modifiers {
            win: true,
            ctrl: true,
            alt: true,
            shift: true,
        };
        assert_eq!(
            combination(shortcut(all, Key::Digit(7))),
            (
                MOD_WIN | MOD_CONTROL | MOD_ALT | MOD_SHIFT | MOD_NOREPEAT,
                0x37
            )
        );
    }

    #[test]
    fn function_keys_from_f1_to_f12() {
        let alt = Modifiers {
            alt: true,
            ..Modifiers::default()
        };
        assert_eq!(
            combination(shortcut(alt, Key::Function(1))),
            (MOD_ALT | MOD_NOREPEAT, 0x70)
        );
        assert_eq!(
            combination(shortcut(alt, Key::Function(12))),
            (MOD_ALT | MOD_NOREPEAT, 0x7B)
        );
    }

    #[test]
    fn digit_zero_and_letter_a() {
        let ctrl = Modifiers {
            ctrl: true,
            ..Modifiers::default()
        };
        assert_eq!(
            combination(shortcut(ctrl, Key::Digit(0))),
            (MOD_CONTROL | MOD_NOREPEAT, 0x30)
        );
        assert_eq!(
            combination(shortcut(ctrl, Key::Letter('A'))),
            (MOD_CONTROL | MOD_NOREPEAT, 0x41)
        );
    }
}
