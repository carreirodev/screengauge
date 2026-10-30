//! Layer 1 tests of the settings file (v1 spec WPC-296, Testing Decisions): the defaults, a wrong value, a damaged
//! file and the remembered state, through the text ScreenGauge reads and writes.

use std::path::Path;

use screengauge_core::settings::{
    self, ColorFormat, Damaged, DefaultFolder, DimensionsMode, Folder, Key, Magnification,
    Modifiers, Options, Problem, Remembered, Reset, Shortcut, ShortcutError, SnipDestination,
    Stored, Theme, Tolerance, Tool, ToolbarPlace,
};

fn folder(path: &str) -> Folder {
    Folder::new(Path::new(path)).expect("an absolute path")
}

/// Every option and every remembered value away from its default.
fn changed_everywhere() -> Stored {
    let ctrl_alt_win = Modifiers {
        win: true,
        ctrl: true,
        alt: true,
        shift: false,
    };
    Stored {
        options: Options {
            global_shortcut: Shortcut::new(ctrl_alt_win, Key::Function(12)).expect("allowed"),
            start_with_windows: true,
            theme: Theme::Dark,
            default_tolerance: Tolerance::new(17).expect("within the limits"),
            also_show_logical_pixels: false,
            color_format: ColorFormat::Hex,
            snip_destination: SnipDestination::Both,
            // "=" and "#" in a folder name are still part of the value.
            default_folder: DefaultFolder::On(folder(r"D:\Snips = #1")),
        },
        remembered: Remembered {
            last_tool: Tool::Inspect,
            dimensions_mode: DimensionsMode::Vertical,
            magnification: Magnification::X32,
            toolbar_place: ToolbarPlace::new(0.0, 0.875).expect("shares from 0 to 1"),
            last_save_as_folder: Some(folder(r"\\server\share\Design Reviews")),
            warned_about_other_copy: true,
        },
    }
}

/// Reads a file that must not be damaged.
fn read(text: &str) -> settings::Read {
    settings::read(text.as_bytes()).expect("the file isn't damaged")
}

/// The `name = value` lines of a settings file, without its notes and blank lines.
fn value_lines(text: &str) -> Vec<&str> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect()
}

#[test]
fn a_new_file_holds_every_default_and_nothing_else() {
    assert_eq!(
        value_lines(&settings::write(&Stored::default())),
        [
            "global_shortcut = Win + Shift + X",
            "start_with_windows = false",
            "theme = Automatic",
            "default_tolerance = 2",
            "also_show_logical_pixels = true",
            "color_format = Both",
            "snip_destination = Clipboard",
            "save_to_default_folder = false",
            "default_folder =",
            "last_tool = Dimensions",
            "dimensions_mode = Cross",
            "magnification = 8",
            "toolbar_x = 0.5",
            "toolbar_y = 0",
            "last_save_as_folder =",
            "warned_about_other_copy = false",
        ]
    );
}

#[test]
fn every_option_and_the_remembered_state_are_read_back() {
    let stored = changed_everywhere();
    let read = read(&settings::write(&stored));
    assert_eq!(read.stored, stored);
    assert_eq!(read.resets, []);
}

#[test]
fn turning_save_to_default_folder_off_keeps_the_folder() {
    let mut stored = Stored::default();
    stored.options.default_folder = DefaultFolder::Off(Some(folder(r"C:\Users\Ana\Pictures")));
    assert_eq!(read(&settings::write(&stored)).stored, stored);
}

// A wrong value

fn unusable(key: &'static str, value: &str) -> Reset {
    Reset {
        key,
        problem: Problem::Unusable(value.to_owned()),
    }
}

#[test]
fn a_wrong_value_goes_back_to_its_default_and_the_rest_stays() {
    let text = settings::write(&changed_everywhere())
        .replace("theme = Dark", "theme = Purple")
        .replace("default_tolerance = 17", "default_tolerance = 51")
        .replace("magnification = 32", "magnification = 3");
    let read = read(&text);

    let mut expected = changed_everywhere();
    expected.options.theme = Theme::Automatic;
    expected.options.default_tolerance = Tolerance::default();
    expected.remembered.magnification = Magnification::X8;
    assert_eq!(read.stored, expected);
    assert_eq!(
        read.resets,
        [
            unusable("theme", "Purple"),
            unusable("default_tolerance", "51"),
            unusable("magnification", "3"),
        ]
    );
}

#[test]
fn an_option_that_doesnt_exist_goes_back_to_its_default() {
    for line in [
        "start_with_windows = yes",
        "theme = Purple",
        "also_show_logical_pixels = 1",
        "color_format = CMYK",
        "snip_destination = Printer",
        "save_to_default_folder = maybe",
        "last_tool = Ruler",
        "dimensions_mode = Diagonal",
        "warned_about_other_copy = no",
    ] {
        let read = read(line);
        assert_eq!(read.stored, Stored::default(), "{line}");
        assert_eq!(read.resets.len(), 1, "{line}");
    }
}

#[test]
fn a_number_outside_its_limits_goes_back_to_its_default() {
    for line in [
        "default_tolerance = 51",
        "default_tolerance = -1",
        "default_tolerance = 2.5",
        "magnification = 1",
        "magnification = 64",
        "toolbar_x = 1.5",
        "toolbar_y = -0.1",
        "toolbar_x = NaN",
        "toolbar_y = inf",
    ] {
        let read = read(line);
        assert_eq!(read.stored, Stored::default(), "{line}");
        assert_eq!(read.resets.len(), 1, "{line}");
    }
}

#[test]
fn the_limits_themselves_are_kept() {
    let read = read("default_tolerance = 0\nmagnification = 2\ntoolbar_x = 1\ntoolbar_y = 0");
    assert_eq!(read.resets, []);
    assert_eq!(read.stored.options.default_tolerance, Tolerance::MIN);
    assert_eq!(read.stored.remembered.magnification, Magnification::X2);
    assert_eq!(
        read.stored.remembered.toolbar_place,
        ToolbarPlace::new(1.0, 0.0).expect("shares")
    );
    assert_eq!(
        settings::read(b"default_tolerance = 50").map(|read| read.stored.options.default_tolerance),
        Ok(Tolerance::MAX)
    );
}

// A damaged file

#[test]
fn a_file_that_isnt_utf8_text_is_damaged() {
    assert_eq!(
        settings::read(&[0xFF, 0xFE, b't', 0, b'h', 0]),
        Err(Damaged::NotUtf8)
    );
}

#[test]
fn a_line_that_isnt_name_value_damages_the_whole_file() {
    let text = "theme = Dark\n\n# a note\nlast_tool Inspect\nmagnification = 4\n";
    assert_eq!(
        settings::read(text.as_bytes()),
        Err(Damaged::NotNameValue { line: 4 })
    );
    assert_eq!(
        settings::read(b"theme = Dark\n= Light"),
        Err(Damaged::NotNameValue { line: 2 })
    );
}

#[test]
fn a_file_without_any_setting_is_damaged() {
    for text in ["", "\n\n", "# only a note\n", "language = pt\n"] {
        assert_eq!(
            settings::read(text.as_bytes()),
            Err(Damaged::NoSettings),
            "{text:?}"
        );
    }
}

// Edited by hand

#[test]
fn a_file_edited_in_notepad_is_read() {
    let text = "\u{feff}; edited by hand\r\n  Theme=dark  \r\nGLOBAL_SHORTCUT = shift+win+f5\r\nColor_Format = rgb\r\n";
    let read = read(text);
    assert_eq!(read.resets, []);
    assert_eq!(read.stored.options.theme, Theme::Dark);
    assert_eq!(read.stored.options.color_format, ColorFormat::Rgb);
    assert_eq!(
        read.stored.options.global_shortcut.to_string(),
        "Win + Shift + F5"
    );
}

#[test]
fn a_missing_name_keeps_its_default_without_a_reset() {
    let read = read("theme = Light");
    let mut expected = Stored::default();
    expected.options.theme = Theme::Light;
    assert_eq!(read.stored, expected);
    assert_eq!(read.resets, []);
}

#[test]
fn a_name_screengauge_doesnt_know_is_left_out() {
    let read = read("theme = Light\nlanguage = pt");
    assert_eq!(read.resets, []);
    assert_eq!(read.stored.options.theme, Theme::Light);
    assert!(!settings::write(&read.stored).contains("language"));
}

#[test]
fn a_name_written_twice_goes_back_to_its_default() {
    let read = read("theme = Dark\nmagnification = 4\ntheme = Light");
    assert_eq!(read.stored.options.theme, Theme::Automatic);
    assert_eq!(read.stored.remembered.magnification, Magnification::X4);
    assert_eq!(
        read.resets,
        [Reset {
            key: "theme",
            problem: Problem::Twice
        }]
    );
}

// The global shortcut

fn modifiers(names: &[&str]) -> Modifiers {
    Modifiers {
        win: names.contains(&"Win"),
        ctrl: names.contains(&"Ctrl"),
        alt: names.contains(&"Alt"),
        shift: names.contains(&"Shift"),
    }
}

#[test]
fn the_shortcut_needs_win_ctrl_or_alt() {
    assert_eq!(
        Shortcut::new(modifiers(&["Shift"]), Key::Letter('X')),
        Err(ShortcutError::Incomplete)
    );
    for held in [["Win"], ["Ctrl"], ["Alt"]] {
        assert!(
            Shortcut::new(modifiers(&held), Key::Letter('X')).is_ok(),
            "{held:?}"
        );
    }
}

#[test]
fn ctrl_alt_without_win_is_refused() {
    assert_eq!(
        Shortcut::new(modifiers(&["Ctrl", "Alt"]), Key::Letter('X')),
        Err(ShortcutError::CtrlAltWithoutWin)
    );
    assert_eq!(
        Shortcut::new(modifiers(&["Ctrl", "Alt", "Shift"]), Key::Digit(1)),
        Err(ShortcutError::CtrlAltWithoutWin)
    );
    assert!(Shortcut::new(modifiers(&["Win", "Ctrl", "Alt"]), Key::Letter('X')).is_ok());
}

#[test]
fn the_main_key_is_a_letter_a_number_or_f1_to_f12() {
    let win = modifiers(&["Win"]);
    for key in [
        Key::Letter('A'),
        Key::Letter('Z'),
        Key::Digit(0),
        Key::Digit(9),
        Key::Function(1),
        Key::Function(12),
    ] {
        assert!(Shortcut::new(win, key).is_ok(), "{key:?}");
    }
    for key in [
        Key::Letter('a'),
        Key::Letter('?'),
        Key::Digit(10),
        Key::Function(0),
        Key::Function(13),
    ] {
        assert_eq!(
            Shortcut::new(win, key),
            Err(ShortcutError::Incomplete),
            "{key:?}"
        );
    }
}

#[test]
fn a_shortcut_the_rules_refuse_goes_back_to_its_default() {
    for value in [
        "Ctrl + Alt + X",
        "Shift + X",
        "Win + Shift",
        "Win",
        "",
        "Win + F13",
        "Win + Space",
        "Win + X + Y",
        "Win + Win + X",
        "Win + + X",
    ] {
        let read = read(&format!("global_shortcut = {value}"));
        assert_eq!(
            read.stored.options.global_shortcut,
            Shortcut::default(),
            "{value:?}"
        );
        assert_eq!(
            read.resets,
            [unusable("global_shortcut", value)],
            "{value:?}"
        );
    }
}

#[test]
fn a_shortcut_is_written_in_the_order_settings_shows() {
    let read = read("global_shortcut = shift + alt + win + ctrl + 7");
    let written = settings::write(&read.stored);
    assert!(
        written.contains("\nglobal_shortcut = Win + Ctrl + Alt + Shift + 7\n"),
        "{written}"
    );
}

// Folders

#[test]
fn save_to_default_folder_needs_a_folder() {
    let read = read("save_to_default_folder = true\ndefault_folder =");
    assert_eq!(read.stored.options.default_folder, DefaultFolder::Off(None));
    assert_eq!(
        read.resets,
        [Reset {
            key: "save_to_default_folder",
            problem: Problem::NoFolder
        }]
    );
}

#[test]
fn a_folder_that_isnt_a_full_path_goes_back_to_its_default() {
    for key in ["default_folder", "last_save_as_folder"] {
        for value in ["Pictures", r"C:Snips", r"\Snips", "C:\\Snips\tnew"] {
            let read = read(&format!("{key} = {value}"));
            assert_eq!(read.stored, Stored::default(), "{key} = {value}");
            assert_eq!(read.resets, [unusable(key, value)], "{key} = {value}");
        }
    }
}
