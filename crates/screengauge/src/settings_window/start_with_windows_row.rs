//! Settings' "Start with Windows" row (v1 spec WPC-296, Settings; WPC-326): its switch shows what Windows holds, read
//! when Settings opens, each time it comes back to the front, as the user may have changed it in Windows meanwhile,
//! and after each click, here or in the tray menu. Windows turned it off: the switch shows off, and a click on it, or
//! on the tray menu's item, shows the amber message under the label until Settings closes, or until the user turns it
//! on again in Windows. A click Windows refused says so the same way, as a refused global shortcut does (WPC-324).
//!
//! Only the resident thread touches it, from Settings' window procedure and from the resident window's.

use std::cell::Cell;

use screengauge_core::settings::StartWithWindows;
use windows::Win32::Foundation::HWND;

use super::controls::SWITCH_WIDTH;
use super::layout::{self, Grown, Row};
use super::row_message::{Message, Text};
use crate::start_with_windows::{self, Clicked};
use crate::{graphics, log};

/// The amber message for Windows' own choice, the prototype's words (WPC-279).
pub const TURNED_OFF_IN_WINDOWS: &str =
    "Turned off in Windows settings. Turn it on in Settings > Apps > Startup.";

/// The amber message for a click Windows refused, as when the registry can't be read or written.
pub const NOT_TAKEN: &str = "Windows didn't take the change. Try again.";

/// What the last click asked the row to say.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Asked {
    /// Windows has it turned off: said while Windows keeps it so.
    TurnedOffInWindows,
    /// Windows refused the click: said until the next click.
    NotTaken,
}

thread_local! {
    /// What Windows held when last read.
    static STATE: Cell<StartWithWindows> = const { Cell::new(StartWithWindows::Off) };
    /// What the last click asked to say, until Settings closes.
    static ASKED: Cell<Option<Asked>> = const { Cell::new(None) };
    /// How much the message grows the row, while it shows.
    static GROWN: Cell<Option<Grown>> = const { Cell::new(None) };
}

/// Whether the switch shows on.
pub fn is_on() -> bool {
    STATE.get().is_on()
}

/// The message under the label, while it shows.
pub fn message() -> Option<Message> {
    said(STATE.get(), ASKED.get()).map(message_saying)
}

/// How much the message makes the row grow, while it shows.
pub fn grown() -> Option<Grown> {
    GROWN.get()
}

/// Settings is opening: what Windows holds is read, and the message, should a click on the tray menu have asked for
/// one, measured before the window is sized.
pub fn opening() {
    read();
    measure();
}

/// Settings is closing, or couldn't open: the message goes with it.
pub fn close() {
    ASKED.set(None);
    GROWN.set(None);
}

/// Settings came back to the front: the user may have changed it in Windows meanwhile.
pub fn reread(hwnd: HWND) {
    let before = STATE.get();
    read();
    refresh(hwnd, STATE.get() != before);
}

/// A click on the switch, or on the tray menu's item with Settings open (`hwnd`) or about to open (`None`).
pub fn click(hwnd: Option<HWND>) {
    ASKED.set(match start_with_windows::click() {
        Ok(Clicked::Turned) => None,
        Ok(Clicked::TurnedOffInWindows) => Some(Asked::TurnedOffInWindows),
        Err(e) => {
            log::failure("Start with Windows: turning it", &e);
            Some(Asked::NotTaken)
        }
    });
    if let Some(hwnd) = hwnd {
        read();
        refresh(hwnd, true);
    }
}

/// Whether a click asked for a message that Settings shows: the tray menu then opens it.
pub fn is_saying() -> bool {
    ASKED.get().is_some()
}

/// What the row says, with Windows holding `state` and the last click having asked for `asked`: Windows' choice only
/// while Windows keeps it off, and a refused click until the next one.
fn said(state: StartWithWindows, asked: Option<Asked>) -> Option<Asked> {
    match asked? {
        Asked::TurnedOffInWindows if state != StartWithWindows::TurnedOffInWindows => None,
        asked => Some(asked),
    }
}

fn read() {
    match start_with_windows::state() {
        Ok(state) => {
            STATE.set(state);
            // Turned on again in Windows: the message is about nothing, and doesn't come back should the user turn it
            // off there again, until a click asks.
            ASKED.set(ASKED.get().and_then(|asked| said(state, Some(asked))));
        }
        // The switch keeps what it showed: a click reads again, and says so should it fail too.
        Err(e) => log::failure("Settings: reading Start with Windows", &e),
    }
}

/// Draws the page again, `changed` saying the switch may have, and resizes the window should the message come or go.
fn refresh(hwnd: HWND, changed: bool) {
    if measure() {
        super::follow_page(hwnd);
    } else if changed {
        super::invalidate(hwnd);
    }
}

/// Keeps how much the message grows its row, should it show; returns whether that changed.
fn measure() -> bool {
    let grown = said(STATE.get(), ASKED.get()).and_then(|asked| {
        match graphics::fonts().and_then(|fonts| message_saying(asked).growth(&fonts)) {
            Ok(grown) => Some(grown),
            Err(e) => {
                log::failure("Settings: laying out Start with Windows' message", &e);
                None
            }
        }
    });
    GROWN.replace(grown) != grown
}

pub(super) fn message_saying(asked: Asked) -> Message {
    Message {
        row: Row::StartWithWindows,
        text: Text::Fixed(match asked {
            Asked::TurnedOffInWindows => TURNED_OFF_IN_WINDOWS,
            Asked::NotTaken => NOT_TAKEN,
        }),
        band: false,
        width: message_room(),
    }
}

/// The room the message has, in logical pixels: from the label's left to the switch, less the row's gap.
fn message_room() -> f32 {
    let room = layout::WIDTH - 2 * layout::ROW_INSET - SWITCH_WIDTH - layout::ROW_GAP;
    // A few hundred pixels, which a u16 holds and a float exactly.
    f32::from(u16::try_from(room).unwrap_or(u16::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graphics::Fonts;

    #[test]
    fn windows_choice_is_said_only_while_windows_keeps_it_off() {
        let asked = Some(Asked::TurnedOffInWindows);
        assert_eq!(said(StartWithWindows::TurnedOffInWindows, asked), asked);
        assert_eq!(said(StartWithWindows::TurnedOffInWindows, None), None);
        // Turned on again in Windows meanwhile: the switch is on, and nothing is left to say.
        assert_eq!(said(StartWithWindows::On, asked), None);
        assert_eq!(said(StartWithWindows::Off, asked), None);
    }

    #[test]
    fn a_refused_click_is_said_whatever_windows_holds() {
        for state in [
            StartWithWindows::Off,
            StartWithWindows::On,
            StartWithWindows::TurnedOffInWindows,
        ] {
            assert_eq!(said(state, Some(Asked::NotTaken)), Some(Asked::NotTaken));
        }
    }

    #[test]
    fn the_message_runs_from_the_label_to_the_switch_less_the_row_s_gap() {
        // 560 wide, 29 in on each side, the switch's 42 and the gap's 10.
        assert!((message_room() - 450.0).abs() < f32::EPSILON);
    }

    #[test]
    fn each_message_takes_one_line_and_grows_its_row() {
        let fonts = Fonts::new().expect("the fonts");
        for asked in [Asked::TurnedOffInWindows, Asked::NotTaken] {
            let message = message_saying(asked);
            assert_eq!(message.lines(&fonts).expect("the lines"), 1, "{asked:?}");
            let grown = message.growth(&fonts).expect("the growth");
            assert_eq!(grown.row, Row::StartWithWindows);
            assert!(grown.by > 0, "{grown:?}");
        }
    }
}
