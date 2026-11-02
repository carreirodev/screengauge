//! Settings' "Start with Windows" row (v1 spec WPC-296, Settings; WPC-326): its switch shows what Windows holds, read
//! when Settings opens, each time it comes back to the front, as the user may have changed it in Windows meanwhile,
//! and after each click, here or in the tray menu. Windows turned it off: the switch shows off, and a click on it, or
//! on the tray menu's item, shows the amber message under the label until Settings closes, or until the user turns it
//! on again in Windows.
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

/// The amber message, the prototype's words (WPC-279).
pub const TURNED_OFF_IN_WINDOWS: &str =
    "Turned off in Windows settings. Turn it on in Settings > Apps > Startup.";

thread_local! {
    /// What Windows held when last read.
    static STATE: Cell<StartWithWindows> = const { Cell::new(StartWithWindows::Off) };
    /// A click asked for the message, and Settings hasn't closed since.
    static SAID: Cell<bool> = const { Cell::new(false) };
    /// How much the message grows the row, while it shows.
    static GROWN: Cell<Option<Grown>> = const { Cell::new(None) };
}

/// Whether the switch shows on.
pub fn is_on() -> bool {
    STATE.get().is_on()
}

/// The message under the label, while it shows.
pub fn message() -> Option<Message> {
    showing(STATE.get(), SAID.get()).then(turned_off_message)
}

/// How much the message makes the row grow, while it shows.
pub fn grown() -> Option<Grown> {
    GROWN.get()
}

/// Settings is opening: what Windows holds is read, and the message, should a click from the tray menu have asked for
/// it, measured before the window is sized.
pub fn opening() {
    read();
    measure();
}

/// Settings is closing: the message goes with it.
pub fn close() {
    SAID.set(false);
    GROWN.set(None);
}

/// Settings came back to the front: the user may have changed it in Windows meanwhile.
pub fn reread(hwnd: HWND) {
    let before = STATE.get();
    read();
    refresh(hwnd, STATE.get() != before);
}

/// A click on the switch.
pub fn click(hwnd: HWND) {
    match start_with_windows::click() {
        Ok(Clicked::Turned) => {}
        Ok(Clicked::TurnedOffInWindows) => SAID.set(true),
        Err(e) => log::failure("Settings: turning Start with Windows", &e),
    }
    read();
    refresh(hwnd, true);
}

/// The tray menu's item was clicked while Windows has it turned off: Settings is about to open, or come to the front,
/// with the message. `hwnd` is Settings, should it be open already.
pub fn say_turned_off_in_windows(hwnd: Option<HWND>) {
    SAID.set(true);
    if let Some(hwnd) = hwnd {
        read();
        refresh(hwnd, true);
    }
}

/// The tray menu turned it on or off while Settings is open: the switch follows.
pub fn follow(hwnd: HWND) {
    read();
    refresh(hwnd, true);
}

/// Whether the message shows: asked for by a click, and Windows still has it turned off.
fn showing(state: StartWithWindows, said: bool) -> bool {
    said && state == StartWithWindows::TurnedOffInWindows
}

fn read() {
    match start_with_windows::state() {
        Ok(state) => STATE.set(state),
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
    let grown = if showing(STATE.get(), SAID.get()) {
        match graphics::fonts().and_then(|fonts| turned_off_message().growth(&fonts)) {
            Ok(grown) => Some(grown),
            Err(e) => {
                log::failure("Settings: laying out Start with Windows' message", &e);
                None
            }
        }
    } else {
        None
    };
    GROWN.replace(grown) != grown
}

pub(super) fn turned_off_message() -> Message {
    Message {
        row: Row::StartWithWindows,
        text: Text::Fixed(TURNED_OFF_IN_WINDOWS),
        band: false,
        width: message_room(),
    }
}

/// The room the message has, in logical pixels: from the label's left to the switch, less the row's gap.
fn message_room() -> f32 {
    let inner = layout::SIDE + layout::CARD_BORDER + layout::ROW_PADDING;
    let room = layout::WIDTH - 2 * inner - SWITCH_WIDTH - layout::ROW_GAP;
    // A few hundred pixels, which a float holds exactly.
    f32::from(u16::try_from(room).unwrap_or(u16::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graphics::Fonts;

    #[test]
    fn the_message_shows_only_after_a_click_while_windows_has_it_turned_off() {
        assert!(showing(StartWithWindows::TurnedOffInWindows, true));
        assert!(!showing(StartWithWindows::TurnedOffInWindows, false));
        // Turned on again in Windows meanwhile: the switch is on, and nothing is left to say.
        assert!(!showing(StartWithWindows::On, true));
        assert!(!showing(StartWithWindows::Off, true));
    }

    #[test]
    fn the_message_runs_from_the_label_to_the_switch_less_the_row_s_gap() {
        // 560 wide, 29 in on each side, the switch's 42 and the gap's 10.
        assert!((message_room() - 450.0).abs() < f32::EPSILON);
    }

    #[test]
    fn the_message_takes_one_line_and_grows_its_row() {
        let fonts = Fonts::new().expect("the fonts");
        let message = turned_off_message();
        let lines = message.lines(&fonts).expect("the lines");
        let grown = message.growth(&fonts).expect("the growth");
        assert_eq!(lines, 1);
        assert_eq!(grown.row, Row::StartWithWindows);
        assert!(grown.by > 0, "{grown:?}");
    }
}
