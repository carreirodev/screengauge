//! "Start with Windows" (v1 spec WPC-296, Settings; WPC-326): what the switch and the tray menu show, from what Windows
//! holds, and what a click on them does. Windows' own choice, made in Task Manager or in Settings > Apps > Startup, is
//! never overridden.

use screengauge_core::settings::{Change, Click, Options, StartWithWindows};

/// What Windows keeps for an entry the user turned off in Task Manager: 03, then the time it was turned off. Read from
/// the owner's computer, Windows 11 build 26300.
const TURNED_OFF: [u8; 12] = [3, 0, 0, 0, 0xd8, 0x4d, 0xca, 0xb1, 0x59, 0x53, 0xdd, 0x01];
/// And for one left on, or turned on again: 02 and no time.
const TURNED_ON: [u8; 12] = [2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];

#[test]
fn listed_and_never_touched_in_windows_it_is_on() {
    assert_eq!(StartWithWindows::of(true, None), StartWithWindows::On);
    assert_eq!(
        StartWithWindows::of(true, Some(&TURNED_ON)),
        StartWithWindows::On
    );
}

#[test]
fn not_listed_it_is_off() {
    assert_eq!(StartWithWindows::of(false, None), StartWithWindows::Off);
    assert_eq!(
        StartWithWindows::of(false, Some(&TURNED_ON)),
        StartWithWindows::Off
    );
}

#[test]
fn listed_and_turned_off_in_windows_it_shows_off_and_says_so() {
    let state = StartWithWindows::of(true, Some(&TURNED_OFF));
    assert_eq!(state, StartWithWindows::TurnedOffInWindows);
    assert!(!state.is_on());
}

#[test]
fn not_listed_a_choice_left_in_windows_is_off_so_turning_it_on_lists_it_where_windows_shows_it() {
    // Windows lists ScreenGauge in Settings > Apps > Startup only while it is listed to start: with its entry gone, as
    // an uninstall leaves it, the user could never turn it on again there, so ScreenGauge lists it first.
    assert_eq!(
        StartWithWindows::of(false, Some(&TURNED_OFF)),
        StartWithWindows::Off
    );
}

#[test]
fn any_odd_first_byte_is_turned_off_and_any_even_one_on() {
    // Windows writes 02 and 03; other builds are reported to write 06 and 07, with the same low bit.
    for (first, on) in [(2, true), (3, false), (6, true), (7, false), (0, true)] {
        let mut approved = TURNED_ON;
        approved[0] = first;
        assert_eq!(
            StartWithWindows::of(true, Some(&approved)).is_on(),
            on,
            "{first:02x}"
        );
    }
    // An empty value says nothing: Windows hasn't turned it off.
    assert!(StartWithWindows::of(true, Some(&[])).is_on());
}

#[test]
fn a_click_turns_it_on_or_off_unless_windows_turned_it_off() {
    assert_eq!(StartWithWindows::Off.click(), Click::TurnOn);
    assert_eq!(StartWithWindows::On.click(), Click::TurnOff);
    assert_eq!(
        StartWithWindows::TurnedOffInWindows.click(),
        Click::SayTurnedOffInWindows
    );
}

#[test]
fn only_on_shows_the_switch_on_and_the_check_mark() {
    assert!(StartWithWindows::On.is_on());
    assert!(!StartWithWindows::Off.is_on());
    assert!(!StartWithWindows::TurnedOffInWindows.is_on());
}

#[test]
fn the_settings_file_follows_the_clicks_off_by_default() {
    let mut options = Options::default();
    assert!(!options.start_with_windows);
    assert!(options.apply(Change::StartWithWindows(true)));
    assert!(options.start_with_windows);
    assert!(!options.apply(Change::StartWithWindows(true)));
    assert!(options.apply(Change::StartWithWindows(false)));
    assert!(!options.start_with_windows);
}
