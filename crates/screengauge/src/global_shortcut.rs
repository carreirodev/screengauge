//! The global shortcut, registered with Windows for the resident window, which then receives WM_HOTKEY whenever the
//! user presses it, in any program. The combination comes from the settings file (`Win + Shift + X` by default), and
//! Settings' recorder changes it while ScreenGauge runs (WPC-323).
//!
//! Only the resident thread touches it: Windows ties a hotkey to the thread of the window it is registered for.

use std::cell::Cell;

use screengauge_core::settings::{Key, Shortcut};
use windows::Win32::Foundation::{E_FAIL, ERROR_HOTKEY_NOT_REGISTERED, HWND, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN, RegisterHotKey,
    UnregisterHotKey, VK_F1,
};
use windows::core::Error;

use crate::log;

/// The two IDs the global shortcut takes in turn. A new combination is registered under the ID the current one doesn't
/// use, and the current one is released only once Windows took the new one, so a combination another program holds
/// leaves the current one working.
const IDS: [i32; 2] = [1, 2];

/// The global shortcut as Windows holds it for the resident window: its ID and its combination.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Registered {
    id: i32,
    shortcut: Shortcut,
}

thread_local! {
    /// The resident window, kept from startup even when registering failed then, so a later change can register.
    static RESIDENT: Cell<Option<HWND>> = const { Cell::new(None) };
    /// What Windows holds now; `None` before startup, after exit, and while another program holds the combination.
    static REGISTERED: Cell<Option<Registered>> = const { Cell::new(None) };
}

/// Registers `shortcut` for `hwnd`, the resident window, at startup. It fails when another program already holds the
/// combination; telling the user comes with WPC-324.
pub fn register(hwnd: HWND, shortcut: Shortcut) -> windows::core::Result<()> {
    RESIDENT.set(Some(hwnd));
    change(shortcut)
}

/// Makes `shortcut` the global shortcut. It fails when another program holds it, and then the shortcut there was still
/// works. Nothing is done when it already is the global shortcut.
pub fn change(shortcut: Shortcut) -> windows::core::Result<()> {
    let Some(hwnd) = RESIDENT.get() else {
        return Err(Error::new(
            E_FAIL,
            "the global shortcut has no resident window",
        ));
    };
    let now = REGISTERED.get();
    let Plan::Register { id, release } = plan(now, shortcut) else {
        return Ok(());
    };
    let (modifiers, key) = combination(shortcut);
    // SAFETY: our own window, on the thread that created it.
    unsafe { RegisterHotKey(Some(hwnd), id, modifiers, key) }?;
    REGISTERED.set(Some(Registered { id, shortcut }));
    if let Some(old) = release
        && let Err(e) = release_id(hwnd, old)
    {
        // The new one works: a WM_HOTKEY of the old ID is no longer the global shortcut (`is_global_shortcut`).
        log::failure("global shortcut: releasing the one before", &e);
    }
    Ok(())
}

/// Undoes `register` as ScreenGauge exits. When registering had failed, there is nothing to undo.
pub fn unregister() -> windows::core::Result<()> {
    let (Some(hwnd), Some(registered)) = (RESIDENT.get(), REGISTERED.take()) else {
        return Ok(());
    };
    release_id(hwnd, registered.id)
}

/// Whether a WM_HOTKEY's wParam names the global shortcut as it is now.
pub fn is_global_shortcut(wparam: WPARAM) -> bool {
    REGISTERED
        .get()
        .is_some_and(|registered| i32::try_from(wparam.0) == Ok(registered.id))
}

/// What `change` asks of Windows, given what it holds `now`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Plan {
    /// `shortcut` already is the global shortcut: registering it again would fail, since the resident window holds it.
    Keep,
    /// Register it under `id`, then release the ID `release` names, should there be one.
    Register { id: i32, release: Option<i32> },
}

fn plan(now: Option<Registered>, shortcut: Shortcut) -> Plan {
    match now {
        Some(registered) if registered.shortcut == shortcut => Plan::Keep,
        Some(registered) => Plan::Register {
            id: if registered.id == IDS[0] {
                IDS[1]
            } else {
                IDS[0]
            },
            release: Some(registered.id),
        },
        None => Plan::Register {
            id: IDS[0],
            release: None,
        },
    }
}

fn release_id(hwnd: HWND, id: i32) -> windows::core::Result<()> {
    // SAFETY: our own window, on the thread that created it.
    match unsafe { UnregisterHotKey(Some(hwnd), id) } {
        Err(e) if e.code() == ERROR_HOTKEY_NOT_REGISTERED.to_hresult() => Ok(()),
        result => result,
    }
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

    fn win_alt(key: char) -> Shortcut {
        let win_alt = Modifiers {
            win: true,
            alt: true,
            ..Modifiers::default()
        };
        shortcut(win_alt, Key::Letter(key))
    }

    #[test]
    fn the_first_registration_takes_the_first_id_and_releases_nothing() {
        assert_eq!(
            plan(None, Shortcut::default()),
            Plan::Register {
                id: 1,
                release: None
            }
        );
    }

    #[test]
    fn a_new_combination_takes_the_other_id_and_then_releases_the_one_before() {
        let now = Registered {
            id: 1,
            shortcut: Shortcut::default(),
        };
        assert_eq!(
            plan(Some(now), win_alt('J')),
            Plan::Register {
                id: 2,
                release: Some(1)
            }
        );
        let now = Registered {
            id: 2,
            shortcut: win_alt('J'),
        };
        assert_eq!(
            plan(Some(now), Shortcut::default()),
            Plan::Register {
                id: 1,
                release: Some(2)
            }
        );
    }

    #[test]
    fn the_combination_already_registered_is_kept_as_it_is() {
        let now = Registered {
            id: 2,
            shortcut: win_alt('J'),
        };
        assert_eq!(plan(Some(now), win_alt('J')), Plan::Keep);
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
