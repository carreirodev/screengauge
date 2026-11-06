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
fn read(text: &str) -> settings::Loaded {
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
            "default_tolerance = 6",
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

#[test]
fn every_destination_and_every_state_of_the_default_folder_is_read_back() {
    let folders = [
        DefaultFolder::Off(None),
        DefaultFolder::Off(Some(folder(r"C:\Users\Ana\Pictures\Snips"))),
        DefaultFolder::On(folder(r"\\server\share\Design Reviews")),
    ];
    for destination in [
        SnipDestination::Clipboard,
        SnipDestination::File,
        SnipDestination::Both,
    ] {
        for default_folder in &folders {
            let mut stored = Stored::default();
            stored.options.snip_destination = destination;
            stored.options.default_folder = default_folder.clone();
            let read = read(&settings::write(&stored));
            assert_eq!(
                read.stored, stored,
                "{destination:?} with {default_folder:?}"
            );
            assert_eq!(read.resets, []);
        }
    }
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
        .replace("default_tolerance = 17", "default_tolerance = 101")
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
            unusable("default_tolerance", "101"),
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
        "default_tolerance = 101",
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
        settings::read(b"default_tolerance = 100")
            .map(|read| read.stored.options.default_tolerance),
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
    let text = "theme = Dark\n\n# a note\n\u{1}\u{2}garbage\nmagnification = 4\n";
    assert_eq!(
        settings::read(text.as_bytes()),
        Err(Damaged::NotNameValue { line: 4 })
    );
    assert_eq!(
        settings::read(b"theme = Dark\n= Light"),
        Err(Damaged::NotNameValue { line: 2 })
    );
    assert_eq!(
        settings::read(b"theme = Dark\nlast_tol Inspect"),
        Err(Damaged::NotNameValue { line: 2 })
    );
}

#[test]
fn a_known_name_missing_its_equals_sign_goes_back_to_its_default_alone() {
    let read = read("theme = Dark\nmagnification 32\nLAST_TOOL");
    let mut expected = Stored::default();
    expected.options.theme = Theme::Dark;
    assert_eq!(read.stored, expected);
    assert_eq!(
        read.resets,
        [
            Reset {
                key: "last_tool",
                problem: Problem::MissingEquals
            },
            Reset {
                key: "magnification",
                problem: Problem::MissingEquals
            },
        ]
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

/// Every tolerance a notch of the mouse wheel or Settings' − / + steps through, from the lowest up (WPC-332): one by one
/// up to 10, then five by five up to 100.
fn steps() -> Vec<u8> {
    (0..=10).chain((15..=100).step_by(5)).collect()
}

#[test]
fn the_tolerance_runs_from_0_to_100_and_starts_at_6() {
    // Calibrated with the test page samples and approved by the owner (WPC-332): the number is the difference of
    // red, green or blue that Inspect shows.
    assert_eq!(Tolerance::MIN.get(), 0);
    assert_eq!(Tolerance::MAX.get(), 100);
    assert_eq!(Tolerance::default().get(), 6);
    assert_eq!(Tolerance::new(101), None);
}

#[test]
fn a_step_up_is_one_up_to_10_then_five_and_stops_at_the_highest() {
    let steps = steps();
    for pair in steps.windows(2) {
        let (from, to) = (tolerance(pair[0]), tolerance(pair[1]));
        assert_eq!(from.up(), to, "up from {}", pair[0]);
        assert_eq!(to.down(), from, "down from {}", pair[1]);
    }
    assert_eq!(Tolerance::MAX.up(), Tolerance::MAX);
    assert_eq!(Tolerance::MIN.down(), Tolerance::MIN);
}

#[test]
fn from_the_default_a_step_is_one_either_way() {
    assert_eq!(Tolerance::default().up().get(), 7);
    assert_eq!(Tolerance::default().down().get(), 5);
}

#[test]
fn a_tolerance_between_two_steps_goes_to_the_next_step_either_way() {
    // The settings file may hold any whole number from 0 to 100, such as 13: + takes it to 15 and − to 10, so every
    // later step is on the same steps as the wheel's.
    assert_eq!(tolerance(13).up().get(), 15);
    assert_eq!(tolerance(13).down().get(), 10);
    assert_eq!(tolerance(99).up(), Tolerance::MAX);
    assert_eq!(tolerance(99).down().get(), 95);
}

fn tolerance(value: u8) -> Tolerance {
    Tolerance::new(value).expect("within the limits")
}

#[test]
fn each_magnification_enlarges_a_pixel_to_its_number_of_pixels_a_side() {
    // 2×, 4×, 8×, 16× and 32× (v1 spec WPC-296, Inspect).
    let factors = [
        Magnification::X2,
        Magnification::X4,
        Magnification::X8,
        Magnification::X16,
        Magnification::X32,
    ]
    .map(Magnification::factor);
    assert_eq!(factors, [2, 4, 8, 16, 32]);
}

/// What Settings' controls do to the options (WPC-320): each click changes one option, and says whether it changed.
mod changes {
    use std::path::Path;

    use screengauge_core::settings::{
        Change, ColorFormat, DefaultFolder, Folder, Options, SnipDestination, Theme, Tolerance,
    };

    fn tolerance(value: u8) -> Tolerance {
        Tolerance::new(value).expect("within the limits")
    }

    #[test]
    fn minus_and_plus_step_the_default_tolerance_as_one_notch_of_the_wheel_does() {
        let mut options = Options::default();
        assert_eq!(options.default_tolerance, tolerance(6));
        assert!(options.apply(Change::DefaultToleranceUp));
        assert_eq!(options.default_tolerance, tolerance(6).up());
        assert!(options.apply(Change::DefaultToleranceDown));
        assert!(options.apply(Change::DefaultToleranceDown));
        assert_eq!(options.default_tolerance, tolerance(6).down());
    }

    #[test]
    fn minus_at_the_lowest_and_plus_at_the_highest_tolerance_change_nothing() {
        let mut options = Options {
            default_tolerance: Tolerance::MIN,
            ..Options::default()
        };
        assert!(!options.apply(Change::DefaultToleranceDown));
        assert_eq!(options.default_tolerance, Tolerance::MIN);

        options.default_tolerance = Tolerance::MAX;
        assert!(!options.apply(Change::DefaultToleranceUp));
        assert_eq!(options.default_tolerance, Tolerance::MAX);
    }

    #[test]
    fn the_switch_turns_logical_pixels_off_and_on_again() {
        let mut options = Options::default();
        assert!(options.also_show_logical_pixels, "on by default");
        assert!(options.apply(Change::AlsoShowLogicalPixels(false)));
        assert!(!options.also_show_logical_pixels);
        assert!(!options.apply(Change::AlsoShowLogicalPixels(false)));
        assert!(options.apply(Change::AlsoShowLogicalPixels(true)));
        assert!(options.also_show_logical_pixels);
    }

    #[test]
    fn a_recorded_shortcut_is_kept_and_written_and_read_back_as_it_was() {
        use screengauge_core::settings::{self, Key, Modifiers, Shortcut, Stored};

        let win_alt = Modifiers {
            win: true,
            alt: true,
            ..Modifiers::default()
        };
        let recorded = Shortcut::new(win_alt, Key::Letter('J')).expect("allowed");
        let mut stored = Stored::default();
        assert!(stored.options.apply(Change::GlobalShortcut(recorded)));
        assert!(!stored.options.apply(Change::GlobalShortcut(recorded)));
        let text = settings::write(&stored);
        assert!(
            text.contains("\nglobal_shortcut = Win + Alt + J\n"),
            "{text}"
        );
        let read = settings::read(text.as_bytes()).expect("the file isn't damaged");
        assert_eq!(read.stored.options.global_shortcut, recorded);
        assert!(read.resets.is_empty(), "{:?}", read.resets);
        // Reset is the default again.
        assert!(
            stored
                .options
                .apply(Change::GlobalShortcut(Shortcut::default()))
        );
        assert_eq!(
            stored.options.global_shortcut.to_string(),
            "Win + Shift + X"
        );
    }

    #[test]
    fn a_color_format_is_chosen_and_choosing_it_again_changes_nothing() {
        let mut options = Options::default();
        assert_eq!(options.color_format, ColorFormat::Both);
        assert!(options.apply(Change::ColorFormat(ColorFormat::Hex)));
        assert_eq!(options.color_format, ColorFormat::Hex);
        assert!(!options.apply(Change::ColorFormat(ColorFormat::Hex)));
        assert!(options.apply(Change::ColorFormat(ColorFormat::Rgb)));
        assert_eq!(options.color_format, ColorFormat::Rgb);
    }

    #[test]
    fn the_theme_starts_automatic_and_takes_light_or_dark() {
        let mut options = Options::default();
        assert_eq!(options.theme, Theme::Automatic);
        assert!(options.apply(Change::Theme(Theme::Dark)));
        assert_eq!(options.theme, Theme::Dark);
        assert!(!options.apply(Change::Theme(Theme::Dark)));
        assert!(options.apply(Change::Theme(Theme::Light)));
        assert!(options.apply(Change::Theme(Theme::Automatic)));
        assert_eq!(options.theme, Theme::Automatic);
    }

    #[test]
    fn a_change_touches_only_its_own_option() {
        let mut options = Options::default();
        options.apply(Change::ColorFormat(ColorFormat::Rgb));
        options.apply(Change::AlsoShowLogicalPixels(false));
        options.apply(Change::DefaultToleranceUp);
        let wanted = Options {
            default_tolerance: tolerance(6).up(),
            also_show_logical_pixels: false,
            color_format: ColorFormat::Rgb,
            ..Options::default()
        };
        assert_eq!(options, wanted);
    }

    fn folder(path: &str) -> Folder {
        Folder::new(Path::new(path)).expect("an absolute path")
    }

    #[test]
    fn the_destination_starts_at_the_clipboard_and_takes_file_or_both() {
        let mut options = Options::default();
        assert_eq!(options.snip_destination, SnipDestination::Clipboard);
        assert!(options.apply(Change::SnipDestination(SnipDestination::File)));
        assert_eq!(options.snip_destination, SnipDestination::File);
        assert!(!options.apply(Change::SnipDestination(SnipDestination::File)));
        assert!(options.apply(Change::SnipDestination(SnipDestination::Both)));
        assert!(options.apply(Change::SnipDestination(SnipDestination::Clipboard)));
        assert_eq!(options.snip_destination, SnipDestination::Clipboard);
    }

    #[test]
    fn save_to_default_folder_starts_off_with_no_folder() {
        assert_eq!(Options::default().default_folder, DefaultFolder::Off(None));
    }

    #[test]
    fn turning_it_on_with_no_folder_kept_changes_nothing_until_a_folder_is_chosen() {
        let mut options = Options::default();
        assert!(!options.apply(Change::SaveToDefaultFolder(true)));
        assert_eq!(options.default_folder, DefaultFolder::Off(None));
        // Choosing the folder is part of turning it on.
        assert!(options.choose_default_folder(folder(r"C:\Snips"), true));
        assert_eq!(
            options.default_folder,
            DefaultFolder::On(folder(r"C:\Snips"))
        );
    }

    #[test]
    fn turning_it_off_keeps_the_folder_and_turning_it_on_again_uses_the_same_one() {
        let mut options = Options::default();
        options.choose_default_folder(folder(r"C:\Snips"), true);
        assert!(options.apply(Change::SaveToDefaultFolder(false)));
        assert_eq!(
            options.default_folder,
            DefaultFolder::Off(Some(folder(r"C:\Snips")))
        );
        assert!(!options.apply(Change::SaveToDefaultFolder(false)));
        assert!(options.apply(Change::SaveToDefaultFolder(true)));
        assert_eq!(
            options.default_folder,
            DefaultFolder::On(folder(r"C:\Snips"))
        );
        assert!(!options.apply(Change::SaveToDefaultFolder(true)));
    }

    #[test]
    fn change_stores_a_new_folder_and_leaves_the_switch_as_it_was() {
        let mut options = Options::default();
        // Off with none kept: it stores the folder and stays off.
        assert!(options.choose_default_folder(folder(r"C:\One"), false));
        assert_eq!(
            options.default_folder,
            DefaultFolder::Off(Some(folder(r"C:\One")))
        );
        // Off with one kept.
        assert!(options.choose_default_folder(folder(r"C:\Two"), false));
        assert_eq!(
            options.default_folder,
            DefaultFolder::Off(Some(folder(r"C:\Two")))
        );
        // On: it stays on, with the new folder.
        options.apply(Change::SaveToDefaultFolder(true));
        assert!(options.choose_default_folder(folder(r"C:\Three"), false));
        assert_eq!(
            options.default_folder,
            DefaultFolder::On(folder(r"C:\Three"))
        );
    }

    #[test]
    fn choosing_the_folder_already_kept_changes_nothing() {
        let mut options = Options::default();
        options.choose_default_folder(folder(r"C:\Snips"), true);
        assert!(!options.choose_default_folder(folder(r"C:\Snips"), false));
        assert!(!options.choose_default_folder(folder(r"C:\Snips"), true));
        options.apply(Change::SaveToDefaultFolder(false));
        // Turning it on by choosing the same folder again does change the switch.
        assert!(options.choose_default_folder(folder(r"C:\Snips"), true));
        assert!(options.default_folder.is_on());
    }

    #[test]
    fn the_destination_and_the_folder_never_touch_each_other() {
        // With the clipboard as the destination the switch still only keeps the folder (v1 spec WPC-296, Snip).
        let mut options = Options::default();
        options.choose_default_folder(folder(r"C:\Snips"), true);
        options.apply(Change::SnipDestination(SnipDestination::File));
        options.apply(Change::SnipDestination(SnipDestination::Clipboard));
        assert_eq!(
            options.default_folder,
            DefaultFolder::On(folder(r"C:\Snips"))
        );
        options.apply(Change::SaveToDefaultFolder(false));
        assert_eq!(options.snip_destination, SnipDestination::Clipboard);
    }
}
