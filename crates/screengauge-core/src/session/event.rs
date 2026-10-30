//! What the Windows layer tells the session core while the tools are open.

use std::path::PathBuf;

use super::Point;
use crate::settings::{DimensionsMode, Tool};

/// Something that happened while the tools are open.
#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    /// Where the cursor was, in physical pixels of the virtual screen.
    pub cursor: Point,
    /// The modifier keys held at that moment.
    pub held: Held,
    /// When it happened, by the computer's clock. The session never reads the clock itself; the Snip's file name
    /// comes from this.
    pub time: LocalTime,
    pub input: Input,
}

/// The modifier keys held during an event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Held {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
}

/// A local date and time, to the second, each part as wide as Windows gives it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalTime {
    pub year: u16,
    /// 1 to 12.
    pub month: u16,
    /// 1 to 31.
    pub day: u16,
    /// 0 to 23.
    pub hour: u16,
    pub minute: u16,
    pub second: u16,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Input {
    /// The mouse moved.
    Move,
    /// The left mouse button went down; the tools use no other button.
    Press,
    /// The left mouse button came up.
    Release,
    /// The mouse wheel turned, in Windows' units: 120 a notch, positive away from the user.
    Wheel(i32),
    /// A key the tools use went down. A key held down repeats this.
    KeyDown(Key),
    /// A key the tools use came up.
    KeyUp(Key),
    /// A button on the toolbar was clicked.
    Toolbar(ToolbarButton),
    /// The global shortcut was pressed again.
    GlobalShortcut,
    /// ScreenGauge was called again another way than the global shortcut: from its tray icon, or by opening it again
    /// from the Start menu or another program.
    Called,
    /// Windows asked the tools to close, as with Alt+F4.
    CloseAsked,
    /// Another program came to the front, as with Alt+Tab or the Windows key. ScreenGauge's own windows, such as Save
    /// As, never count.
    OtherProgramInFront,
    /// Windows said the monitors changed: one connected or disconnected, or a scale, resolution, arrangement or HDR
    /// change.
    MonitorsChanged,
    /// The Save As window closed, with the path the user chose, or `None` when it was cancelled.
    SaveAsClosed(Option<PathBuf>),
}

/// The keys the tools use. None of them can be changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    /// Dimensions' horizontal mode.
    H,
    /// Dimensions' vertical mode.
    V,
    /// Dimensions' cross mode.
    C,
    /// 1 to 4, on the main keys: the tools in the toolbar's order.
    Digit1,
    Digit2,
    Digit3,
    Digit4,
    Left,
    Right,
    Up,
    Down,
    Esc,
    Backspace,
    Delete,
    Shift,
    Alt,
}

/// The toolbar's buttons.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolbarButton {
    Tool(Tool),
    /// H, V or C, shown while Dimensions is the active tool.
    Mode(DimensionsMode),
    /// The gear.
    Settings,
    Minimize,
    /// The small button the minimized toolbar becomes.
    Restore,
    Close,
}
