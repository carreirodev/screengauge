//! Layer 1 tests of the global shortcut recorder's rules in Settings (v1 spec WPC-296, Settings; WPC-323): the
//! modifiers alone record nothing, an incomplete combination and Ctrl + Alt without Win are refused and the field goes on
//! recording, and Esc, a click outside or Windows refusing the combination keep the shortcut there was. Esc takes the
//! message with it, a click outside leaves it.

use screengauge_core::settings::{
    Key, Modifiers, Pressed, Recorder, Refusal, Shortcut, Step, Tried, is_common,
};

fn held(names: &[&str]) -> Modifiers {
    Modifiers {
        win: names.contains(&"Win"),
        ctrl: names.contains(&"Ctrl"),
        alt: names.contains(&"Alt"),
        shift: names.contains(&"Shift"),
    }
}

fn recording() -> Recorder {
    let mut recorder = Recorder::default();
    recorder.start();
    recorder
}

fn shortcut(names: &[&str], key: Key) -> Shortcut {
    Shortcut::new(held(names), key).expect("an allowed combination")
}

#[test]
fn the_field_rests_until_it_is_clicked() {
    let mut recorder = Recorder::default();
    assert!(!recorder.is_recording());
    // A key reaching a field that isn't recording, as one posted just before it stopped, does nothing.
    assert_eq!(
        recorder.key_down(held(&["Win"]), Pressed::Main(Key::Letter('J'))),
        Step::Show
    );
    assert!(!recorder.is_recording());
    assert_eq!(recorder.cancel(), Step::Show);

    recorder.start();
    assert!(recorder.is_recording());
    assert_eq!(recorder.held(), Modifiers::default());
    assert_eq!(recorder.refusal(), None);
}

#[test]
fn modifiers_alone_record_nothing_and_the_field_waits_for_the_main_key() {
    let mut recorder = recording();
    for down in [
        &["Win"][..],
        &["Win", "Ctrl"],
        &["Win", "Ctrl", "Alt", "Shift"],
    ] {
        assert_eq!(recorder.key_down(held(down), Pressed::Modifier), Step::Show);
        assert!(recorder.is_recording());
        assert_eq!(recorder.held(), held(down));
        assert_eq!(recorder.refusal(), None);
    }
    // All of them come up again: still recording, still nothing recorded.
    recorder.key_up(held(&["Win"]));
    assert_eq!(recorder.held(), held(&["Win"]));
    recorder.key_up(Modifiers::default());
    assert!(recorder.is_recording());
    assert_eq!(recorder.held(), Modifiers::default());
    assert_eq!(recorder.refusal(), None);
}

#[test]
fn a_combination_the_rules_accept_is_tried_with_windows() {
    let mut recorder = recording();
    recorder.key_down(held(&["Win"]), Pressed::Modifier);
    recorder.key_down(held(&["Win", "Alt"]), Pressed::Modifier);
    let step = recorder.key_down(held(&["Win", "Alt"]), Pressed::Main(Key::Letter('J')));
    assert_eq!(step, Step::Try(shortcut(&["Win", "Alt"], Key::Letter('J'))));
    // Until Windows answers, the field still records.
    assert!(recorder.is_recording());

    recorder.tried(Tried::Taken);
    assert!(!recorder.is_recording());
    assert_eq!(recorder.refusal(), None);
}

#[test]
fn numbers_and_f1_to_f12_are_main_keys_and_shift_is_optional() {
    for (names, key) in [
        (&["Ctrl"][..], Key::Digit(0)),
        (&["Alt", "Shift"], Key::Digit(9)),
        (&["Win"], Key::Function(1)),
        (&["Win", "Ctrl", "Alt", "Shift"], Key::Function(12)),
        (&["Win", "Shift"], Key::Letter('X')),
    ] {
        let mut recorder = recording();
        assert_eq!(
            recorder.key_down(held(names), Pressed::Main(key)),
            Step::Try(shortcut(names, key)),
            "{names:?} {key:?}"
        );
    }
}

#[test]
fn an_incomplete_combination_is_refused_and_the_field_goes_on_recording() {
    // A main key with Shift only, or alone, and a key that can't be a main key, even with Win.
    for (names, pressed) in [
        (&["Shift"][..], Pressed::Main(Key::Letter('J'))),
        (&[][..], Pressed::Main(Key::Function(5))),
        (&["Win"], Pressed::Other),
        (&["Ctrl", "Shift"], Pressed::Other),
    ] {
        let mut recorder = recording();
        assert_eq!(
            recorder.key_down(held(names), pressed),
            Step::Show,
            "{names:?}"
        );
        assert!(recorder.is_recording(), "{names:?}");
        assert_eq!(recorder.refusal(), Some(Refusal::Incomplete), "{names:?}");
    }
}

#[test]
fn ctrl_alt_without_win_is_refused_and_with_win_it_is_tried() {
    let mut recorder = recording();
    let step = recorder.key_down(held(&["Ctrl", "Alt"]), Pressed::Main(Key::Letter('K')));
    assert_eq!(step, Step::Show);
    assert!(recorder.is_recording());
    assert_eq!(recorder.refusal(), Some(Refusal::CtrlAltWithoutWin));
    // AltGr + K on a Brazilian keyboard comes as Ctrl + Alt too, Shift or not.
    let step = recorder.key_down(
        held(&["Ctrl", "Alt", "Shift"]),
        Pressed::Main(Key::Digit(7)),
    );
    assert_eq!(step, Step::Show);
    assert_eq!(recorder.refusal(), Some(Refusal::CtrlAltWithoutWin));
    // The refusal shows until a combination the rules accept, which is tried.
    let step = recorder.key_down(
        held(&["Win", "Ctrl", "Alt"]),
        Pressed::Main(Key::Letter('K')),
    );
    assert_eq!(
        step,
        Step::Try(shortcut(&["Win", "Ctrl", "Alt"], Key::Letter('K')))
    );
    recorder.tried(Tried::Taken);
    assert_eq!(recorder.refusal(), None);
}

#[test]
fn a_combination_another_program_holds_is_refused_and_recording_stops() {
    let mut recorder = recording();
    let step = recorder.key_down(held(&["Ctrl", "Shift"]), Pressed::Main(Key::Letter('S')));
    assert_eq!(
        step,
        Step::Try(shortcut(&["Ctrl", "Shift"], Key::Letter('S')))
    );
    recorder.tried(Tried::AlreadyUsed);
    assert!(!recorder.is_recording());
    assert_eq!(recorder.refusal(), Some(Refusal::AlreadyUsed));
    // The message stays until the field is clicked again.
    recorder.start();
    assert_eq!(recorder.refusal(), None);
}

#[test]
fn esc_cancels_and_takes_the_refusal_of_this_recording_with_it() {
    let mut recorder = recording();
    recorder.key_down(held(&["Shift"]), Pressed::Main(Key::Letter('J')));
    assert_eq!(recorder.refusal(), Some(Refusal::Incomplete));
    // Esc with a modifier held still cancels.
    assert_eq!(
        recorder.key_down(held(&["Win"]), Pressed::Escape),
        Step::Stop
    );
    assert!(!recorder.is_recording());
    assert_eq!(recorder.refusal(), None);
    assert_eq!(recorder.held(), Modifiers::default());
}

#[test]
fn a_click_outside_or_losing_the_front_cancels_and_leaves_the_message() {
    let mut recorder = recording();
    recorder.key_down(held(&["Ctrl", "Alt"]), Pressed::Main(Key::Letter('K')));
    assert_eq!(recorder.cancel(), Step::Stop);
    assert!(!recorder.is_recording());
    assert_eq!(recorder.held(), Modifiers::default());
    assert_eq!(recorder.refusal(), Some(Refusal::CtrlAltWithoutWin));
    // Once stopped, cancelling again does nothing.
    assert_eq!(recorder.cancel(), Step::Show);
}

#[test]
fn a_click_on_the_field_while_it_records_keeps_recording_and_clears_the_message() {
    let mut recorder = recording();
    recorder.key_down(held(&["Win"]), Pressed::Modifier);
    recorder.key_down(held(&["Win"]), Pressed::Other);
    assert_eq!(recorder.refusal(), Some(Refusal::Incomplete));
    recorder.start();
    assert!(recorder.is_recording());
    assert_eq!(recorder.refusal(), None);
    // Win is still held: the field still shows it.
    assert_eq!(recorder.held(), held(&["Win"]));
}

#[test]
fn reset_while_recording_stops_it_and_says_how_registering_went() {
    let mut recorder = recording();
    recorder.key_down(held(&["Win"]), Pressed::Other);
    assert_eq!(recorder.refusal(), Some(Refusal::Incomplete));
    // Reset: recording is cancelled first, then the default is tried.
    assert_eq!(recorder.cancel(), Step::Stop);
    assert_eq!(recorder.refusal(), Some(Refusal::Incomplete));
    recorder.tried(Tried::AlreadyUsed);
    assert_eq!(recorder.refusal(), Some(Refusal::AlreadyUsed));
    recorder.tried(Tried::Taken);
    assert_eq!(recorder.refusal(), None);
}

#[test]
fn the_combinations_windows_and_most_programs_use_are_refused_and_the_field_goes_on_recording() {
    let mut combinations: Vec<(&[&str], Key)> = "CVXZYASFPNOWT"
        .chars()
        .map(|letter| (&["Ctrl"][..], Key::Letter(letter)))
        .collect();
    combinations.push((&["Alt"], Key::Function(4)));
    assert_eq!(combinations.len(), 14);
    for (names, key) in combinations {
        assert!(is_common(shortcut(names, key)), "{names:?} {key:?}");
        let mut recorder = recording();
        assert_eq!(
            recorder.key_down(held(names), Pressed::Main(key)),
            Step::Show,
            "{names:?} {key:?}"
        );
        assert!(recorder.is_recording(), "{names:?} {key:?}");
        assert_eq!(
            recorder.refusal(),
            Some(Refusal::Common),
            "{names:?} {key:?}"
        );
    }
}

#[test]
fn with_shift_or_win_too_they_are_other_combinations_and_are_tried() {
    for (names, key) in [
        (&["Ctrl", "Shift"][..], Key::Letter('C')),
        (&["Win", "Ctrl"], Key::Letter('V')),
        (&["Alt", "Shift"], Key::Function(4)),
        (&["Ctrl"], Key::Function(4)),
        (&["Alt"], Key::Letter('C')),
        (&["Ctrl"], Key::Letter('B')),
    ] {
        assert!(!is_common(shortcut(names, key)), "{names:?} {key:?}");
        let mut recorder = recording();
        assert_eq!(
            recorder.key_down(held(names), Pressed::Main(key)),
            Step::Try(shortcut(names, key)),
            "{names:?} {key:?}"
        );
    }
}
