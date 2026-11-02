//! The global shortcut recorder in Settings, as rules (v1 spec WPC-296, Settings, the global shortcut recorder;
//! WPC-323): a click on the field starts recording, the modifiers held show as they go down and up, and the first other
//! key decides. A combination the rules accept is tried with Windows, which may say another program holds it; one they
//! refuse shows why and the field goes on recording; Esc, a click outside the field or Settings losing the front stop
//! recording and keep the shortcut there was.
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
            Pressed::Escape => self.cancel(),
            Pressed::Other => {
                self.refusal = Some(Refusal::Incomplete);
                Step::Show
            }
            Pressed::Main(key) => match Shortcut::new(held, key) {
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

    /// Esc, a click outside the field, or Settings losing the front: recording stops, the shortcut there was stays,
    /// and a refusal of this recording goes with it. `Step::Show` when the field wasn't recording.
    pub fn cancel(&mut self) -> Step {
        if !self.recording {
            return Step::Show;
        }
        self.recording = false;
        self.held = Modifiers::default();
        self.refusal = None;
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
