//! The global shortcut as a value: which combinations are allowed (v1 spec WPC-296, Settings, "Gravador do global
//! shortcut"). The settings file keeps it as text, and the shortcut recorder checks a new combination with
//! [`Shortcut::new`].

use std::fmt;

/// The modifier keys held with the main key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Modifiers {
    pub win: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}

/// The main key of a shortcut. Only a letter, a number on the main keys or F1 to F12 can be one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    /// `'A'` to `'Z'`.
    Letter(char),
    /// 0 to 9.
    Digit(u8),
    /// F1 to F12.
    Function(u8),
}

impl Key {
    fn is_allowed(self) -> bool {
        match self {
            Key::Letter(letter) => letter.is_ascii_uppercase(),
            Key::Digit(digit) => digit <= 9,
            Key::Function(number) => (1..=12).contains(&number),
        }
    }

    /// Reads `X`, `5` or `F5`, in either case.
    fn parse(text: &str) -> Option<Key> {
        let mut chars = text.chars();
        let key = match (chars.next(), chars.next()) {
            (Some(letter), None) if letter.is_ascii_alphabetic() => {
                Key::Letter(letter.to_ascii_uppercase())
            }
            (Some(digit), None) => Key::Digit(u8::try_from(digit.to_digit(10)?).ok()?),
            _ => Key::Function(text.strip_prefix(['F', 'f'])?.parse().ok()?),
        };
        key.is_allowed().then_some(key)
    }
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Key::Letter(letter) => write!(f, "{letter}"),
            Key::Digit(digit) => write!(f, "{digit}"),
            Key::Function(number) => write!(f, "F{number}"),
        }
    }
}

/// A combination ScreenGauge accepts as the global shortcut: Win, Ctrl or Alt, optionally Shift, and one main key.
/// Ctrl + Alt needs Win too, because on Brazilian keyboards it is AltGr and would take characters like / and ?.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shortcut {
    modifiers: Modifiers,
    key: Key,
}

/// Why a combination can't be the global shortcut.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShortcutError {
    /// No Win, Ctrl or Alt, or a main key that isn't a letter, a number or F1 to F12.
    Incomplete,
    /// Ctrl + Alt without Win.
    CtrlAltWithoutWin,
}

impl Shortcut {
    pub fn new(modifiers: Modifiers, key: Key) -> Result<Shortcut, ShortcutError> {
        if !key.is_allowed() || !(modifiers.win || modifiers.ctrl || modifiers.alt) {
            return Err(ShortcutError::Incomplete);
        }
        if modifiers.ctrl && modifiers.alt && !modifiers.win {
            return Err(ShortcutError::CtrlAltWithoutWin);
        }
        Ok(Shortcut { modifiers, key })
    }

    pub fn modifiers(self) -> Modifiers {
        self.modifiers
    }

    pub fn key(self) -> Key {
        self.key
    }

    /// Reads the text [`Display`](fmt::Display) writes, in any order and case: `win + shift + x` is `Win + Shift + X`.
    pub(super) fn parse(text: &str) -> Option<Shortcut> {
        let mut modifiers = Modifiers::default();
        let mut key = None;
        for part in text.split('+').map(str::trim) {
            let held = match part.to_ascii_lowercase().as_str() {
                "win" => &mut modifiers.win,
                "ctrl" => &mut modifiers.ctrl,
                "alt" => &mut modifiers.alt,
                "shift" => &mut modifiers.shift,
                _ => {
                    if key.replace(Key::parse(part)?).is_some() {
                        return None;
                    }
                    continue;
                }
            };
            if std::mem::replace(held, true) {
                return None;
            }
        }
        Shortcut::new(modifiers, key?).ok()
    }
}

/// `Win + Shift + X`, the default (decided in WPC-290).
impl Default for Shortcut {
    fn default() -> Shortcut {
        Shortcut {
            modifiers: Modifiers {
                win: true,
                shift: true,
                ..Modifiers::default()
            },
            key: Key::Letter('X'),
        }
    }
}

/// As Settings and the notifications show it: `Win + Ctrl + Alt + Shift + X`, in that order.
impl fmt::Display for Shortcut {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Modifiers {
            win,
            ctrl,
            alt,
            shift,
        } = self.modifiers;
        for (held, name) in [(win, "Win"), (ctrl, "Ctrl"), (alt, "Alt"), (shift, "Shift")] {
            if held {
                write!(f, "{name} + ")?;
            }
        }
        write!(f, "{}", self.key)
    }
}
