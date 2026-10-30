//! The settings file: ScreenGauge's options and its remembered state, as text people can read and edit (v1 spec
//! WPC-296, "Arquivo de configurações e estado lembrado"; the format is ADR 0002).
//!
//! The Windows layer reads the file and hands its bytes to [`read`], and saves the text [`write`] returns. Not kept
//! anywhere: the tolerance adjusted with the mouse wheel, the pinned measurements and the frozen screen.

mod shortcut;

use std::collections::HashMap;
use std::fmt;
use std::path::Path;

pub use shortcut::{Key, Modifiers, Shortcut, ShortcutError};

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
}

/// A folder ScreenGauge keeps, as an absolute path. It may have been deleted since: whoever uses it checks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Folder(String);

impl Folder {
    /// `None` for a path the settings file can't keep: relative, not Unicode, with control characters, or with
    /// spaces at either end.
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

/// A tolerance. The limits and the default are the prototype's, provisional until the calibration (WPC-332).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Tolerance(u8);

impl Tolerance {
    pub const MIN: Tolerance = Tolerance(0);
    pub const MAX: Tolerance = Tolerance(50);

    pub fn new(value: u8) -> Option<Tolerance> {
        (Tolerance::MIN.0..=Tolerance::MAX.0)
            .contains(&value)
            .then_some(Tolerance(value))
    }

    pub fn get(self) -> u8 {
        self.0
    }
}

impl Default for Tolerance {
    fn default() -> Tolerance {
        Tolerance(2)
    }
}

/// Where the toolbar sits in its monitor, as a share of the room it has to move on each axis: 0 is against the left
/// (or top) edge, 1 against the right (or bottom) edge. The toolbar (WPC-312) places it on the current monitor.
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

/// Gives a choice its word in the file, and the list of all of them for the file's notes.
macro_rules! words {
    ($choice:ident { $($variant:ident => $word:literal),+ $(,)? }) => {
        impl $choice {
            const WORDS: &[&str] = &[$($word),+];

            fn word(self) -> &'static str {
                match self {
                    $($choice::$variant => $word),+
                }
            }

            /// Reads a word in any case.
            fn from_word(text: &str) -> Option<$choice> {
                [$($choice::$variant),+]
                    .into_iter()
                    .find(|choice| choice.word().eq_ignore_ascii_case(text))
            }
        }
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Theme {
    /// Follows the Windows app color mode.
    #[default]
    Automatic,
    Light,
    Dark,
}

words!(Theme { Automatic => "Automatic", Light => "Light", Dark => "Dark" });

/// How Inspect shows and copies a color.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ColorFormat {
    Hex,
    Rgb,
    #[default]
    Both,
}

words!(ColorFormat { Hex => "HEX", Rgb => "RGB", Both => "Both" });

/// Where Snip's image goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SnipDestination {
    #[default]
    Clipboard,
    File,
    Both,
}

words!(SnipDestination { Clipboard => "Clipboard", File => "File", Both => "Both" });

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tool {
    #[default]
    Dimensions,
    Bounds,
    Inspect,
    Snip,
}

words!(Tool { Dimensions => "Dimensions", Bounds => "Bounds", Inspect => "Inspect", Snip => "Snip" });

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DimensionsMode {
    Horizontal,
    Vertical,
    #[default]
    Cross,
}

words!(DimensionsMode { Horizontal => "Horizontal", Vertical => "Vertical", Cross => "Cross" });

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum Magnification {
    X2,
    X4,
    #[default]
    X8,
    X16,
    X32,
}

words!(Magnification { X2 => "2", X4 => "4", X8 => "8", X16 => "16", X32 => "32" });

impl Magnification {
    /// How many times larger each physical pixel appears.
    pub fn factor(self) -> u32 {
        match self {
            Magnification::X2 => 2,
            Magnification::X4 => 4,
            Magnification::X8 => 8,
            Magnification::X16 => 16,
            Magnification::X32 => 32,
        }
    }
}

/// The names in the file, one per line.
mod key {
    pub const GLOBAL_SHORTCUT: &str = "global_shortcut";
    pub const START_WITH_WINDOWS: &str = "start_with_windows";
    pub const THEME: &str = "theme";
    pub const DEFAULT_TOLERANCE: &str = "default_tolerance";
    pub const ALSO_SHOW_LOGICAL_PIXELS: &str = "also_show_logical_pixels";
    pub const COLOR_FORMAT: &str = "color_format";
    pub const SNIP_DESTINATION: &str = "snip_destination";
    pub const SAVE_TO_DEFAULT_FOLDER: &str = "save_to_default_folder";
    pub const DEFAULT_FOLDER: &str = "default_folder";
    pub const LAST_TOOL: &str = "last_tool";
    pub const DIMENSIONS_MODE: &str = "dimensions_mode";
    pub const MAGNIFICATION: &str = "magnification";
    pub const TOOLBAR_X: &str = "toolbar_x";
    pub const TOOLBAR_Y: &str = "toolbar_y";
    pub const LAST_SAVE_AS_FOLDER: &str = "last_save_as_folder";
    pub const WARNED_ABOUT_OTHER_COPY: &str = "warned_about_other_copy";
}

/// A settings file that could be read: what it keeps, and each value that was wrong and went back to its default.
/// Everything else in the file was kept.
#[derive(Debug, Clone, PartialEq)]
pub struct Read {
    pub stored: Stored,
    pub resets: Vec<Reset>,
}

/// A value that went back to its default.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reset {
    /// Its name in the file.
    pub key: &'static str,
    pub problem: Problem,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Problem {
    /// The value, as found, isn't one this setting takes.
    Unusable(String),
    /// The name appears more than once, so which value was meant is unknown.
    Twice,
    /// "Save to default folder" was on without a usable folder.
    NoFolder,
}

impl fmt::Display for Reset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} went back to its default: ", self.key)?;
        match &self.problem {
            Problem::Unusable(value) => write!(f, "\"{value}\" isn't a value it takes"),
            Problem::Twice => write!(f, "it appears more than once"),
            Problem::NoFolder => write!(f, "turning it on needs {}", key::DEFAULT_FOLDER),
        }
    }
}

/// Why a whole file went back to the defaults, the remembered state included.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Damaged {
    NotUtf8,
    /// This line, counted from 1, is neither blank, a note, nor `name = value`.
    NotNameValue {
        line: usize,
    },
    /// Not one setting ScreenGauge knows, as in an empty file.
    NoSettings,
}

impl fmt::Display for Damaged {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Damaged::NotUtf8 => write!(f, "the file isn't UTF-8 text"),
            Damaged::NotNameValue { line } => write!(f, "line {line} isn't \"name = value\""),
            Damaged::NoSettings => write!(f, "the file holds no settings"),
        }
    }
}

/// Reads a settings file. A missing name keeps its default without a reset, as in a file from an older version, and a
/// name ScreenGauge doesn't know is left out.
pub fn read(bytes: &[u8]) -> Result<Read, Damaged> {
    let text = std::str::from_utf8(bytes).map_err(|_| Damaged::NotUtf8)?;
    // Notepad can save UTF-8 with a byte order mark.
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut values = Values::parse(text)?;
    let mut stored = Stored::default();

    let options = &mut stored.options;
    values.take(
        key::GLOBAL_SHORTCUT,
        Shortcut::parse,
        &mut options.global_shortcut,
    );
    values.take(
        key::START_WITH_WINDOWS,
        parse_switch,
        &mut options.start_with_windows,
    );
    values.take(key::THEME, Theme::from_word, &mut options.theme);
    values.take(
        key::DEFAULT_TOLERANCE,
        |text| Tolerance::new(text.parse().ok()?),
        &mut options.default_tolerance,
    );
    values.take(
        key::ALSO_SHOW_LOGICAL_PIXELS,
        parse_switch,
        &mut options.also_show_logical_pixels,
    );
    values.take(
        key::COLOR_FORMAT,
        ColorFormat::from_word,
        &mut options.color_format,
    );
    values.take(
        key::SNIP_DESTINATION,
        SnipDestination::from_word,
        &mut options.snip_destination,
    );
    let mut save_to_default_folder = false;
    values.take(
        key::SAVE_TO_DEFAULT_FOLDER,
        parse_switch,
        &mut save_to_default_folder,
    );
    let mut default_folder = None;
    values.take(key::DEFAULT_FOLDER, parse_folder, &mut default_folder);
    options.default_folder = match (save_to_default_folder, default_folder) {
        (true, Some(folder)) => DefaultFolder::On(folder),
        (true, None) => {
            values.reset(key::SAVE_TO_DEFAULT_FOLDER, Problem::NoFolder);
            DefaultFolder::Off(None)
        }
        (false, folder) => DefaultFolder::Off(folder),
    };

    let remembered = &mut stored.remembered;
    values.take(key::LAST_TOOL, Tool::from_word, &mut remembered.last_tool);
    values.take(
        key::DIMENSIONS_MODE,
        DimensionsMode::from_word,
        &mut remembered.dimensions_mode,
    );
    values.take(
        key::MAGNIFICATION,
        Magnification::from_word,
        &mut remembered.magnification,
    );
    values.take(key::TOOLBAR_X, parse_share, &mut remembered.toolbar_place.x);
    values.take(key::TOOLBAR_Y, parse_share, &mut remembered.toolbar_place.y);
    values.take(
        key::LAST_SAVE_AS_FOLDER,
        parse_folder,
        &mut remembered.last_save_as_folder,
    );
    values.take(
        key::WARNED_ABOUT_OTHER_COPY,
        parse_switch,
        &mut remembered.warned_about_other_copy,
    );

    if values.known == 0 {
        return Err(Damaged::NoSettings);
    }
    Ok(Read {
        stored,
        resets: values.resets,
    })
}

/// The values of a file being read, by name.
struct Values<'a> {
    found: HashMap<String, Found<'a>>,
    /// How many names ScreenGauge knows were found.
    known: usize,
    resets: Vec<Reset>,
}

#[derive(Clone, Copy)]
enum Found<'a> {
    Once(&'a str),
    Twice,
}

impl<'a> Values<'a> {
    fn parse(text: &'a str) -> Result<Values<'a>, Damaged> {
        let mut found = HashMap::new();
        for (index, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with(['#', ';']) {
                continue;
            }
            let (name, value) = line
                .split_once('=')
                .filter(|(name, _)| !name.trim().is_empty())
                .ok_or(Damaged::NotNameValue { line: index + 1 })?;
            found
                .entry(name.trim().to_ascii_lowercase())
                .and_modify(|found| *found = Found::Twice)
                .or_insert(Found::Once(value.trim()));
        }
        Ok(Values {
            found,
            known: 0,
            resets: Vec::new(),
        })
    }

    /// Puts the value named `key` in `into` when `parse` accepts it. Otherwise `into` keeps its default, and the
    /// reset is noted unless the name is missing.
    fn take<T>(&mut self, key: &'static str, parse: impl FnOnce(&str) -> Option<T>, into: &mut T) {
        let Some(&found) = self.found.get(key) else {
            return;
        };
        self.known += 1;
        match found {
            Found::Twice => self.reset(key, Problem::Twice),
            Found::Once(value) => match parse(value) {
                Some(value) => *into = value,
                None => self.reset(key, Problem::Unusable(value.to_owned())),
            },
        }
    }

    fn reset(&mut self, key: &'static str, problem: Problem) {
        self.resets.push(Reset { key, problem });
    }
}

fn parse_switch(text: &str) -> Option<bool> {
    ["false", "true"]
        .into_iter()
        .position(|word| word.eq_ignore_ascii_case(text))
        .map(|position| position == 1)
}

/// An empty value is no folder.
fn parse_folder(text: &str) -> Option<Option<Folder>> {
    if text.is_empty() {
        return Some(None);
    }
    Folder::new(Path::new(text)).map(Some)
}

fn parse_share(text: &str) -> Option<f64> {
    text.parse().ok().filter(|share| is_share(*share))
}

const HEADER: &str = "\
# ScreenGauge settings
#
# Exit ScreenGauge before editing this file: it rewrites the file whenever something changes.
# Each line is \"name = value\", and switches are true or false. A value ScreenGauge can't use goes
# back to its default, and the rest is kept.
";

/// The text of a settings file that keeps `stored`.
pub fn write(stored: &Stored) -> String {
    let Stored {
        options,
        remembered,
    } = stored;
    let mut file = File(String::from(HEADER));

    file.heading("The options in Settings");
    file.note(
        "Win, Ctrl or Alt, optionally Shift, and a letter, a number or F1 to F12. Ctrl + Alt also needs Win.",
    );
    file.value(key::GLOBAL_SHORTCUT, &options.global_shortcut.to_string());
    file.value(key::START_WITH_WINDOWS, switch(options.start_with_windows));
    file.choice(key::THEME, Theme::WORDS, options.theme.word());
    file.note(&format!(
        "{} to {}",
        Tolerance::MIN.get(),
        Tolerance::MAX.get()
    ));
    file.value(
        key::DEFAULT_TOLERANCE,
        &options.default_tolerance.get().to_string(),
    );
    file.value(
        key::ALSO_SHOW_LOGICAL_PIXELS,
        switch(options.also_show_logical_pixels),
    );
    file.choice(
        key::COLOR_FORMAT,
        ColorFormat::WORDS,
        options.color_format.word(),
    );
    file.choice(
        key::SNIP_DESTINATION,
        SnipDestination::WORDS,
        options.snip_destination.word(),
    );
    file.note("Turning it on needs default_folder.");
    file.value(
        key::SAVE_TO_DEFAULT_FOLDER,
        switch(options.default_folder.is_on()),
    );
    file.value(key::DEFAULT_FOLDER, folder(options.default_folder.folder()));

    file.heading("Remembered between calls, not in Settings");
    file.choice(key::LAST_TOOL, Tool::WORDS, remembered.last_tool.word());
    file.choice(
        key::DIMENSIONS_MODE,
        DimensionsMode::WORDS,
        remembered.dimensions_mode.word(),
    );
    file.choice(
        key::MAGNIFICATION,
        Magnification::WORDS,
        remembered.magnification.word(),
    );
    file.note("The toolbar's place in its monitor, from 0 (left or top) to 1 (right or bottom).");
    file.value(key::TOOLBAR_X, &remembered.toolbar_place.x().to_string());
    file.value(key::TOOLBAR_Y, &remembered.toolbar_place.y().to_string());
    file.value(
        key::LAST_SAVE_AS_FOLDER,
        folder(remembered.last_save_as_folder.as_ref()),
    );
    file.value(
        key::WARNED_ABOUT_OTHER_COPY,
        switch(remembered.warned_about_other_copy),
    );
    file.0
}

/// The settings file's text as [`write`] builds it.
struct File(String);

impl File {
    fn heading(&mut self, text: &str) {
        self.0.push('\n');
        self.note(text);
        self.0.push('\n');
    }

    fn note(&mut self, text: &str) {
        self.0.push_str("# ");
        self.0.push_str(text);
        self.0.push('\n');
    }

    fn value(&mut self, key: &str, value: &str) {
        self.0.push_str(key);
        self.0.push_str(" =");
        if !value.is_empty() {
            self.0.push(' ');
            self.0.push_str(value);
        }
        self.0.push('\n');
    }

    /// A value from a fixed list, after a note that lists them all: "Automatic, Light or Dark".
    fn choice(&mut self, key: &str, words: &[&str], word: &str) {
        let note = match words {
            [first @ .., last] => format!("{} or {last}", first.join(", ")),
            [] => String::new(),
        };
        self.note(&note);
        self.value(key, word);
    }
}

fn switch(on: bool) -> &'static str {
    if on { "true" } else { "false" }
}

/// No folder is an empty value.
fn folder(folder: Option<&Folder>) -> &str {
    folder.map_or("", |folder| folder.0.as_str())
}
