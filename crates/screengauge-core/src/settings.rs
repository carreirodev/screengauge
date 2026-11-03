//! What the settings file keeps: ScreenGauge's options and its remembered state (v1 spec WPC-296, Settings file and
//! remembered state). The file's text, ADR 0002, is the `text` module's.
//!
//! The Windows layer reads the file and hands its bytes to [`read`], and saves the text [`write`] returns. Not kept
//! anywhere: the tolerance adjusted with the mouse wheel, the pinned measurements and the frozen screen.

mod recorder;
mod shortcut;
mod start_with_windows;
mod text;

use std::path::Path;

pub use recorder::{Pressed, Recorder, Refusal, Step, Tried, is_common};
pub use shortcut::{Key, Modifiers, Shortcut, ShortcutError};
pub use start_with_windows::{Click, StartWithWindows};
pub use text::{Damaged, Loaded, Problem, Reset, read, write};

/// Everything the settings file keeps.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Stored {
    pub options: Options,
    pub remembered: Remembered,
}

/// The options the user changes in Settings.
#[derive(Debug, Clone, PartialEq)]
pub struct Options {
    pub global_shortcut: Shortcut,
    /// What the user last chose for "Start with Windows" in ScreenGauge. Windows holds what happens at sign-in, and the
    /// switch shows Windows' answer ([`StartWithWindows`]); this follows the clicks, and is never written to Windows.
    pub start_with_windows: bool,
    pub theme: Theme,
    pub default_tolerance: Tolerance,
    pub also_show_logical_pixels: bool,
    pub color_format: ColorFormat,
    pub snip_destination: SnipDestination,
    pub default_folder: DefaultFolder,
}

impl Default for Options {
    fn default() -> Options {
        Options {
            global_shortcut: Shortcut::default(),
            start_with_windows: false,
            theme: Theme::default(),
            default_tolerance: Tolerance::default(),
            also_show_logical_pixels: true,
            color_format: ColorFormat::default(),
            snip_destination: SnipDestination::default(),
            default_folder: DefaultFolder::default(),
        }
    }
}

/// One change the user makes in Settings. Each takes effect, and is saved, at once, with no Save or Cancel (v1 spec
/// WPC-296, Settings): the next call opens with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    /// "Global shortcut": a combination the recorder took, or Reset's `Win + Shift + X`, once Windows has registered it.
    GlobalShortcut(Shortcut),
    /// "Start with Windows" turned on or off, once Windows has it.
    StartWithWindows(bool),
    /// "Theme": Automatic, Light or Dark.
    Theme(Theme),
    /// "Default tolerance"'s − and +: the same step as a notch of the mouse wheel, stopping at the limits.
    DefaultToleranceDown,
    DefaultToleranceUp,
    /// "Also show logical pixels" turned on or off.
    AlsoShowLogicalPixels(bool),
    ColorFormat(ColorFormat),
    /// "Destination": Clipboard, File or Both.
    SnipDestination(SnipDestination),
    /// "Save to default folder" turned on or off, with the folder already kept. Turning it on with none kept changes
    /// nothing: the folder is chosen first, and [`Options::choose_default_folder`] turns it on with it.
    SaveToDefaultFolder(bool),
}

impl Options {
    /// Applies `change`. False when it left the options as they were, as − does at the lowest tolerance or a click on
    /// the format already chosen, so there is nothing to save.
    pub fn apply(&mut self, change: Change) -> bool {
        let before = self.clone();
        match change {
            Change::GlobalShortcut(shortcut) => self.global_shortcut = shortcut,
            Change::StartWithWindows(on) => self.start_with_windows = on,
            Change::Theme(theme) => self.theme = theme,
            Change::DefaultToleranceDown => self.default_tolerance = self.default_tolerance.down(),
            Change::DefaultToleranceUp => self.default_tolerance = self.default_tolerance.up(),
            Change::AlsoShowLogicalPixels(on) => self.also_show_logical_pixels = on,
            Change::ColorFormat(format) => self.color_format = format,
            Change::SnipDestination(destination) => self.snip_destination = destination,
            Change::SaveToDefaultFolder(on) => self.default_folder = self.default_folder.turned(on),
        }
        *self != before
    }

    /// Keeps `folder` as the default folder, which Windows' folder window returned. "Save to default folder" stays as it
    /// is, on or off, and `turn_on` turns it on too, as choosing the folder is part of turning it on. False when
    /// nothing changed.
    pub fn choose_default_folder(&mut self, folder: Folder, turn_on: bool) -> bool {
        let before = self.default_folder.clone();
        let on = turn_on || before.is_on();
        self.default_folder = if on {
            DefaultFolder::On(folder)
        } else {
            DefaultFolder::Off(Some(folder))
        };
        self.default_folder != before
    }
}

/// What ScreenGauge remembers between calls without showing it in Settings.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Remembered {
    /// The active tool the next call starts with.
    pub last_tool: Tool,
    pub dimensions_mode: DimensionsMode,
    pub magnification: Magnification,
    pub toolbar_place: ToolbarPlace,
    /// Where the next Save As opens; `None` means Pictures.
    pub last_save_as_folder: Option<Folder>,
    /// This copy has already warned that the other copy (Store or installer) is installed too.
    pub warned_about_other_copy: bool,
}

/// "Save to default folder" and the folder it saves to. Turning it on needs a folder; turning it off keeps the
/// folder, so turning it on again uses the same one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DefaultFolder {
    Off(Option<Folder>),
    On(Folder),
}

impl Default for DefaultFolder {
    fn default() -> DefaultFolder {
        DefaultFolder::Off(None)
    }
}

impl DefaultFolder {
    pub fn is_on(&self) -> bool {
        matches!(self, DefaultFolder::On(_))
    }

    pub fn folder(&self) -> Option<&Folder> {
        match self {
            DefaultFolder::Off(folder) => folder.as_ref(),
            DefaultFolder::On(folder) => Some(folder),
        }
    }

    /// Turned `on` or off with the folder it keeps. Turning it on with no folder kept leaves it off.
    fn turned(&self, on: bool) -> DefaultFolder {
        match (self, on) {
            (DefaultFolder::Off(Some(folder)), true) => DefaultFolder::On(folder.clone()),
            (DefaultFolder::On(folder), false) => DefaultFolder::Off(Some(folder.clone())),
            _ => self.clone(),
        }
    }
}

/// A folder ScreenGauge keeps, as an absolute path. It may have been deleted since: whoever uses it checks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Folder(String);

impl Folder {
    /// `None` for a path the settings file can't keep: relative, not Unicode, with control characters, or with
    /// spaces at either end. Absolute follows Windows' rules (`C:\…` or `\\server\share\…`), the only system
    /// ScreenGauge is built for.
    pub fn new(path: &Path) -> Option<Folder> {
        let text = path.to_str()?;
        let keepable =
            path.is_absolute() && text.trim() == text && !text.chars().any(char::is_control);
        keepable.then(|| Folder(text.to_owned()))
    }

    pub fn as_path(&self) -> &Path {
        Path::new(&self.0)
    }
}

/// A tolerance: how far a pixel's red, green or blue may be from the reference color's, as Inspect shows them, with
/// the pixel still the same area. The limits, the default and the steps were calibrated with the test page's captures
/// and approved by the owner (WPC-332; ADR 0005).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Tolerance(u8);

impl Tolerance {
    pub const MIN: Tolerance = Tolerance(0);
    /// Past the strongest gradient the test page draws, whose colors are 79 apart; above it, only colors far apart,
    /// such as the page's blue and white, 225 apart, would count as one area.
    pub const MAX: Tolerance = Tolerance(100);

    pub fn new(value: u8) -> Option<Tolerance> {
        (Tolerance::MIN.0..=Tolerance::MAX.0)
            .contains(&value)
            .then_some(Tolerance(value))
    }

    pub const fn get(self) -> u8 {
        self.0
    }

    /// One step higher, as a notch of the mouse wheel turned forward and Settings' + make it: the next of the steps
    /// above it. At `MAX` it stays.
    pub fn up(self) -> Tolerance {
        let next = if self.0 < LAST_FINE_STEP {
            self.0 + 1
        } else {
            // The next multiple of the coarse step above it.
            (self.0 / COARSE_STEP + 1) * COARSE_STEP
        };
        Tolerance::new(next).unwrap_or(Tolerance::MAX)
    }

    /// One step lower, as a notch of the mouse wheel turned back and Settings' − make it: the next of the steps below
    /// it. At `MIN` it stays.
    pub fn down(self) -> Tolerance {
        let next = if self.0 <= LAST_FINE_STEP {
            self.0.saturating_sub(1)
        } else {
            // The next multiple of the coarse step below it, and never below the last fine step.
            ((self.0 - 1) / COARSE_STEP * COARSE_STEP).max(LAST_FINE_STEP)
        };
        Tolerance::new(next).unwrap_or(Tolerance::MIN)
    }
}

/// The last of the steps of 1: from 0 to it, a notch of the mouse wheel and Settings' − / + move the tolerance by 1,
/// where the captures show light cards, faint shadows and subtle gradients being told apart (WPC-332). From it, + goes
/// to 15.
const LAST_FINE_STEP: u8 = 10;

/// Above [`LAST_FINE_STEP`], a notch moves it by 5, to the next multiple of 5, so crossing a strong gradient takes few
/// notches: 13 from the default across the test page's blue-to-purple gradient.
const COARSE_STEP: u8 = 5;

/// 6: the test page's answers are sound at every scale from 5 to 9. Below 5, faint pixels around a text's shadow
/// count; from 10, a light gradient into white is lost, and from 12, a light gray card on white (WPC-332).
impl Default for Tolerance {
    fn default() -> Tolerance {
        Tolerance(6)
    }
}

/// Where the toolbar sits in its monitor, as a share of the room it has to move inside its margins on each axis: 0 is
/// against the left (or top) margin, 1 against the right (or bottom) one. Dragging the toolbar sets it, and the next call
/// puts the toolbar at the same place of the current monitor, whatever its size and scale
/// ([`session::toolbar::corner`](crate::session::toolbar::corner), WPC-312).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ToolbarPlace {
    x: f64,
    y: f64,
}

impl ToolbarPlace {
    pub fn new(x: f64, y: f64) -> Option<ToolbarPlace> {
        (is_share(x) && is_share(y)).then_some(ToolbarPlace { x, y })
    }

    pub fn x(self) -> f64 {
        self.x
    }

    pub fn y(self) -> f64 {
        self.y
    }
}

/// Top center, where the toolbar appears the first time.
impl Default for ToolbarPlace {
    fn default() -> ToolbarPlace {
        ToolbarPlace { x: 0.5, y: 0.0 }
    }
}

/// From 0 to 1; false for NaN too.
fn is_share(value: f64) -> bool {
    (0.0..=1.0).contains(&value)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Theme {
    /// Follows the Windows app color mode.
    #[default]
    Automatic,
    Light,
    Dark,
}

/// How Inspect shows and copies a color.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ColorFormat {
    Hex,
    Rgb,
    #[default]
    Both,
}

/// Where Snip's image goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SnipDestination {
    #[default]
    Clipboard,
    File,
    Both,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tool {
    #[default]
    Dimensions,
    Bounds,
    Inspect,
    Snip,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DimensionsMode {
    Horizontal,
    Vertical,
    #[default]
    Cross,
}

/// How many times larger each physical pixel appears in the magnified view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum Magnification {
    X2,
    X4,
    #[default]
    X8,
    X16,
    X32,
}

impl Magnification {
    /// How many physical pixels a side each enlarged pixel takes: 2, 4, 8, 16 or 32.
    pub const fn factor(self) -> u32 {
        match self {
            Magnification::X2 => 2,
            Magnification::X4 => 4,
            Magnification::X8 => 8,
            Magnification::X16 => 16,
            Magnification::X32 => 32,
        }
    }

    /// One level higher, as a notch of the mouse wheel turned forward makes it. At 32× it stays.
    pub fn up(self) -> Magnification {
        match self {
            Magnification::X2 => Magnification::X4,
            Magnification::X4 => Magnification::X8,
            Magnification::X8 => Magnification::X16,
            Magnification::X16 | Magnification::X32 => Magnification::X32,
        }
    }

    /// One level lower, as a notch of the mouse wheel turned back makes it. At 2× it stays.
    pub fn down(self) -> Magnification {
        match self {
            Magnification::X2 | Magnification::X4 => Magnification::X2,
            Magnification::X8 => Magnification::X4,
            Magnification::X16 => Magnification::X8,
            Magnification::X32 => Magnification::X16,
        }
    }
}
