//! The global shortcut recorder in Settings, as rules (v1 spec WPC-296, Settings, the global shortcut recorder;
//! WPC-323): a click on the field starts recording, the modifiers held show as they go down and up, and the first other
//! key decides. A combination the rules accept is tried with Windows, which may say another program holds it; one they
//! refuse shows why and the field goes on recording; Esc, a click outside the field or Settings losing the front stop
//! recording and keep the shortcut there was. Esc takes the message with it; a click outside leaves it, as the prototype
//! does, so what the user clicks doesn't move from under the cursor.
//!
//! The Windows layer says which key went down, as [`Pressed`], with the modifiers held, and does what [`Step`] asks:
//! try a combination, or let the keyboard go.

use super::shortcut::{Key, Modifiers, Shortcut, ShortcutError};

/// A key that went down while the field records, as the recorder tells keys apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pressed {
    /// Win, Ctrl, Alt or Shift, either side.
    Modifier,
    /// A key that can be a shortcut's main key: a letter, a number on the main keys, or F1 to F12.
    Main(Key),
    Escape,
    /// Any other key, such as Space, Tab, an arrow or a number on the numeric keypad.
    Other,
}

/// Why the field shows a message under its label.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// No Win, Ctrl or Alt, or a main key that isn't a letter, a number or F1 to F12.
    Incomplete,
    /// Ctrl + Alt without Win, which is AltGr on Brazilian keyboards.
    CtrlAltWithoutWin,
    /// One of the combinations Windows and most programs use, such as Ctrl + C or Alt + F4 ([`is_common`]).
    Common,
    /// Windows refused the combination: another program holds it. The shortcut there was still works.
    AlreadyUsed,
}

impl From<ShortcutError> for Refusal {
    fn from(error: ShortcutError) -> Refusal {
        match error {
            ShortcutError::Incomplete => Refusal::Incomplete,
            ShortcutError::CtrlAltWithoutWin => Refusal::CtrlAltWithoutWin,
        }
    }
}

/// What the Windows layer does after a key went down.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// Nothing but drawing the field again: the modifiers held changed, or a refusal shows.
    Show,
    /// The rules accept this combination: register it with Windows, then say how that went with
    /// [`Recorder::tried`]. Until then the field still records.
    Try(Shortcut),
    /// Recording stopped and the shortcut there was stays: let the keyboard go.
    Stop,
}

/// How registering a combination with Windows went.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tried {
    /// It is the global shortcut now.
    Taken,
    /// Another program holds it.
    AlreadyUsed,
}

/// The field and the message under its label.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Recorder {
    recording: bool,
    held: Modifiers,
    refusal: Option<Refusal>,
}

impl Recorder {
    /// Whether the field is recording, and so holds the keyboard.
    pub fn is_recording(&self) -> bool {
        self.recording
    }

    /// The modifiers held while recording, which the field shows before the main key.
    pub fn held(&self) -> Modifiers {
        self.held
    }

    /// Why the last combination was refused, while it shows.
    pub fn refusal(&self) -> Option<Refusal> {
        self.refusal
    }

    /// A click on the field: it records from nothing held, and a message there was goes. A click while it records
    /// changes nothing but the message.
    pub fn start(&mut self) {
        if !self.recording {
            self.held = Modifiers::default();
        }
        self.recording = true;
        self.refusal = None;
    }

    /// A key went down with `held` down after it. Only a main key, Esc or another key decides: the modifiers alone
    /// record nothing, and the field waits for the main key.
    pub fn key_down(&mut self, held: Modifiers, pressed: Pressed) -> Step {
        if !self.recording {
            return Step::Show;
        }
        self.held = held;
        match pressed {
            Pressed::Modifier => Step::Show,
            Pressed::Escape => {
                self.refusal = None;
                self.cancel()
            }
            Pressed::Other => {
                self.refusal = Some(Refusal::Incomplete);
                Step::Show
            }
            Pressed::Main(key) => match Shortcut::new(held, key) {
                Ok(shortcut) if is_common(shortcut) => {
                    self.refusal = Some(Refusal::Common);
                    Step::Show
                }
                Ok(shortcut) => Step::Try(shortcut),
                Err(error) => {
                    self.refusal = Some(error.into());
                    Step::Show
                }
            },
        }
    }

    /// A key came up, leaving `held` down.
    pub fn key_up(&mut self, held: Modifiers) {
        if self.recording {
            self.held = held;
        }
    }

    /// A click outside the field, or Settings losing the front: recording stops, the shortcut there was stays, and a
    /// refusal shown stays too. `Step::Show` when the field wasn't recording.
    pub fn cancel(&mut self) -> Step {
        if !self.recording {
            return Step::Show;
        }
        self.recording = false;
        self.held = Modifiers::default();
        Step::Stop
    }

    /// Registering the combination [`Step::Try`] gave, or Reset's, went as `tried`. Either way the field stops
    /// recording: it shows the global shortcut, which is the new one when Windows took it, and the old one, with the
    /// message, when another program holds the new one.
    pub fn tried(&mut self, tried: Tried) {
        self.recording = false;
        self.held = Modifiers::default();
        self.refusal = match tried {
            Tried::Taken => None,
            Tried::AlreadyUsed => Some(Refusal::AlreadyUsed),
        };
    }
}

/// The letters that, with Ctrl alone, Windows and most programs use: copy, paste, cut, undo, redo, select all, save, find,
/// print, new, open, close and new tab (the owner's list, WPC-323).
const WITH_CTRL: [char; 13] = [
    'C', 'V', 'X', 'Z', 'Y', 'A', 'S', 'F', 'P', 'N', 'O', 'W', 'T',
];

/// Whether `shortcut` is one of the combinations Windows and most programs use, which the recorder refuses: Ctrl and
/// one of [`WITH_CTRL`], or Alt + F4, exactly. With Shift or Win too it is another combination, and is allowed.
pub fn is_common(shortcut: Shortcut) -> bool {
    let only = |modifiers: Modifiers| shortcut.modifiers() == modifiers;
    let ctrl = Modifiers {
        ctrl: true,
        ..Modifiers::default()
    };
    let alt = Modifiers {
        alt: true,
        ..Modifiers::default()
    };
    match shortcut.key() {
        Key::Letter(letter) => only(ctrl) && WITH_CTRL.contains(&letter),
        Key::Function(4) => only(alt),
        Key::Digit(_) | Key::Function(_) => false,
    }
}
