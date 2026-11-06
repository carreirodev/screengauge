//! Layer 1 tests of what the notice says once the clipboard answers (ticket WPC-352): the session asks for a copy, the
//! Windows layer hands back whether Windows took it, and only that answer shows "Copied" or "Couldn't copy.", for every
//! copy the tools make: the measurement pinned in Dimensions or Bounds, Inspect's color and Snip's image, with each
//! Destination that copies. They run on a frozen screen drawn here, without a screen or a clipboard, and look only at
//! the scene the session would draw and at the effects it returns.

use std::path::{Path, PathBuf};
use std::time::Duration;

use screengauge_core::session::{
    Colors, Effect, Event, Held, Image, Input, LocalTime, Message, Notice, NoticeSide, Opening,
    Photo, Point, Rect, Scale, Session, Sign,
};
use screengauge_core::settings::{
    DefaultFolder, Folder, Options, Remembered, SnipDestination, Tool,
};

/// A 1080p monitor at 100%, white, with a blue button of 100 × 40 at columns 1000 to 1099 and rows 600 to 639.
fn button() -> Photo {
    let mut bgra = [255, 255, 255, 255].repeat(1920 * 1080);
    for y in 600..640 {
        for x in 1000..1100 {
            let at = (y * 1920 + x) * 4;
            bgra[at..at + 3].copy_from_slice(&[235, 99, 37]);
        }
    }
    Photo {
        rect: Rect {
            left: 0,
            top: 0,
            width: 1920,
            height: 1080,
        },
        scale: Scale::from_dpi(96).expect("100%"),
        colors: Colors::Exact,
        image: Image::new(1920, 1080, bgra).expect("the pixels fill the image"),
    }
}

fn at(x: i32, y: i32) -> Point {
    Point { x, y }
}

/// `tool` active, Snip's image going to `destination` and the file to `default_folder`, the cursor at (900, 500),
/// called by the global shortcut.
fn open_with(tool: Tool, destination: SnipDestination, default_folder: DefaultFolder) -> Session {
    Session::open(Opening {
        frozen_screen: vec![Some(button())],
        cursor: at(900, 500),
        options: Options {
            snip_destination: destination,
            default_folder,
            ..Options::default()
        },
        remembered: Remembered {
            last_tool: tool,
            ..Remembered::default()
        },
        second_click_until: None,
    })
    .expect("a frozen screen the session takes")
}

fn open(tool: Tool) -> Session {
    open_with(tool, SnipDestination::Clipboard, DefaultFolder::Off(None))
}

/// `input` with the cursor at `cursor`, nothing held, `milliseconds` after Windows started, at 20:41:12 on 2026-10-26.
fn event(cursor: Point, milliseconds: u64, input: Input) -> Event {
    Event {
        cursor,
        held: Held::default(),
        time: LocalTime {
            year: 2026,
            month: 10,
            day: 26,
            hour: 20,
            minute: 41,
            second: 12,
        },
        uptime: Duration::from_millis(milliseconds),
        input,
    }
}

/// Clicks inside the button at (1050, 620): Dimensions pins the span there, and Inspect copies the blue. What the
/// press, which pins and copies, returns.
fn click(session: &mut Session) -> Vec<Effect> {
    session.handle(event(at(1050, 620), 1000, Input::Move));
    let pressed = session.handle(event(at(1050, 620), 1010, Input::Press));
    session.handle(event(at(1050, 620), 1020, Input::Release));
    pressed
}

/// Drags around the button and lets go at (1200, 700): Bounds pins the button, and Snip takes its image.
fn drag(session: &mut Session) -> Vec<Effect> {
    session.handle(event(at(900, 500), 1000, Input::Move));
    session.handle(event(at(900, 500), 1010, Input::Press));
    session.handle(event(at(1200, 700), 1020, Input::Move));
    session.handle(event(at(1200, 700), 1030, Input::Release))
}

/// The clipboard's answer to the copy: Windows took it when `made`, and refused it after the retries otherwise.
fn answer(session: &mut Session, made: bool) -> Vec<Effect> {
    let input = if made {
        Input::Copied
    } else {
        Input::NotCopied
    };
    session.handle(event(at(1200, 700), 1040, input))
}

fn notice(session: &Session) -> Option<Message> {
    session.scene().notice.map(|notice| notice.message)
}

#[test]
fn a_refused_copy_of_a_pinned_measurement_says_couldnt_copy() {
    let mut session = open(Tool::Dimensions);
    click(&mut session);
    answer(&mut session, false);
    assert_eq!(notice(&session), Some(Message::CouldntCopy));
    assert_eq!(Message::CouldntCopy.text(), "Couldn't copy.");
    assert_eq!(Message::CouldntCopy.sign(), Sign::Alert);
}

#[test]
fn a_copy_of_a_pinned_measurement_says_copied_once_the_clipboard_took_it() {
    let mut session = open(Tool::Dimensions);
    click(&mut session);
    answer(&mut session, true);
    assert_eq!(notice(&session), Some(Message::Copied));
}

#[test]
fn nothing_says_copied_before_the_clipboard_answers() {
    for tool in [Tool::Dimensions, Tool::Inspect] {
        let mut session = open(tool);
        let effects = click(&mut session);
        assert!(
            effects
                .iter()
                .any(|effect| matches!(effect, Effect::CopyText(_))),
            "{tool:?}: {effects:?}"
        );
        // No notice, and so no wake for one: the scene the windows draw has no "Copied" to show for a moment.
        assert!(
            !effects
                .iter()
                .any(|effect| matches!(effect, Effect::WakeAt(_))),
            "{tool:?}: {effects:?}"
        );
        assert_eq!(notice(&session), None, "{tool:?}");
    }
    for tool in [Tool::Bounds, Tool::Snip] {
        let mut session = open(tool);
        let effects = drag(&mut session);
        assert!(
            effects
                .iter()
                .any(|effect| matches!(effect, Effect::CopyText(_) | Effect::CopyImage(_))),
            "{tool:?}: {effects:?}"
        );
        assert_eq!(notice(&session), None, "{tool:?}");
    }
}

#[test]
fn two_copies_waiting_at_once_are_each_answered_in_their_order() {
    // Two clicks before the Windows layer gets to the first copy: the first answer is the first copy's.
    let mut session = open(Tool::Dimensions);
    click(&mut session);
    session.handle(event(at(1300, 300), 1100, Input::Move));
    session.handle(event(at(1300, 300), 1110, Input::Press));
    session.handle(event(at(1300, 300), 1120, Input::Release));
    assert_eq!(session.scene().pinned.len(), 2);
    session.handle(event(at(1300, 300), 1130, Input::NotCopied));
    assert_eq!(
        session.scene().notice.copied(),
        Some(Notice {
            message: Message::CouldntCopy,
            at: at(1050, 620),
            monitor: 0,
            side: NoticeSide::Above,
        })
    );
    // The second answer is the second copy's, where its click was, in place of the first notice.
    session.handle(event(at(1300, 300), 1140, Input::Copied));
    assert_eq!(
        session.scene().notice.copied(),
        Some(Notice {
            message: Message::Copied,
            at: at(1300, 300),
            monitor: 0,
            side: NoticeSide::Above,
        })
    );
}

#[test]
fn the_measurement_stays_pinned_when_the_copy_is_refused() {
    for (tool, pin) in [
        (Tool::Dimensions, click as fn(&mut Session) -> Vec<Effect>),
        (Tool::Bounds, drag),
    ] {
        let mut session = open(tool);
        pin(&mut session);
        assert_eq!(session.scene().pinned.len(), 1, "{tool:?}");
        answer(&mut session, false);
        assert_eq!(session.scene().pinned.len(), 1, "{tool:?}");
        assert_eq!(notice(&session), Some(Message::CouldntCopy), "{tool:?}");
    }
}

#[test]
fn the_notice_shows_where_the_click_was_for_as_long_as_copied_does() {
    let mut session = open(Tool::Dimensions);
    click(&mut session);
    // The cursor moves on before the answer: the notice still goes where the copy was asked for.
    let effects = session.handle(event(at(1500, 900), 1040, Input::NotCopied));
    assert_eq!(
        session.scene().notice.copied(),
        Some(Notice {
            message: Message::CouldntCopy,
            at: at(1050, 620),
            monitor: 0,
            side: NoticeSide::Above,
        })
    );
    // 1.3 s, as "Copied" (v1 spec WPC-296, Como as medidas aparecem).
    assert_eq!(
        effects,
        [Effect::WakeAt(Duration::from_millis(1040 + 1300))]
    );
}

#[test]
fn bounds_says_copied_or_couldnt_copy_as_the_clipboard_answers() {
    for (made, message) in [(true, Message::Copied), (false, Message::CouldntCopy)] {
        let mut session = open(Tool::Bounds);
        drag(&mut session);
        answer(&mut session, made);
        assert_eq!(notice(&session), Some(message));
    }
}

#[test]
fn inspect_says_copied_or_couldnt_copy_as_the_clipboard_answers() {
    for (made, message) in [(true, Message::Copied), (false, Message::CouldntCopy)] {
        let mut session = open(Tool::Inspect);
        click(&mut session);
        answer(&mut session, made);
        assert_eq!(notice(&session), Some(message));
    }
}

#[test]
fn snip_to_the_clipboard_says_copied_or_couldnt_copy_as_the_clipboard_answers() {
    for (made, message) in [(true, Message::Copied), (false, Message::CouldntCopy)] {
        let mut session = open(Tool::Snip);
        let effects = drag(&mut session);
        assert!(
            matches!(effects.as_slice(), [Effect::CopyImage(_)]),
            "{effects:?}"
        );
        answer(&mut session, made);
        assert_eq!(notice(&session), Some(message));
    }
}

// Snip with Destination at Both: the copy is asked for first and answered before the file, and one notice says both.

fn folder(path: &str) -> Folder {
    Folder::new(Path::new(path)).expect("an absolute path")
}

fn written(session: &mut Session, saved: bool) {
    let input = if saved { Input::Saved } else { Input::NotSaved };
    session.handle(event(at(1200, 700), 1050, input));
}

#[test]
fn both_says_what_became_of_the_copy_and_the_file() {
    for (made, saved, message) in [
        (true, true, Message::CopiedAndSaved),
        (false, true, Message::CouldntCopyButSaved),
        (true, false, Message::CouldntSaveButCopied),
        (false, false, Message::CouldntSaveOrCopy),
    ] {
        let mut session = open_with(
            Tool::Snip,
            SnipDestination::Both,
            DefaultFolder::On(folder(r"D:\Snips")),
        );
        drag(&mut session);
        answer(&mut session, made);
        // The file is still being written: nothing shows yet.
        assert_eq!(notice(&session), None, "copied {made}, saved {saved}");
        written(&mut session, saved);
        assert_eq!(
            notice(&session),
            Some(message),
            "copied {made}, saved {saved}"
        );
        assert_eq!(message.sign() == Sign::Ok, made && saved);
    }
    assert_eq!(
        Message::CouldntCopyButSaved.text(),
        "Couldn't copy. The image was saved."
    );
    assert_eq!(Message::CouldntSaveOrCopy.text(), "Couldn't save or copy.");
}

#[test]
fn both_through_save_as_says_what_became_of_the_copy_and_the_file() {
    for (made, saved, message) in [
        (true, true, Message::CopiedAndSaved),
        (false, true, Message::CouldntCopyButSaved),
        (true, false, Message::CouldntSaveButCopied),
        (false, false, Message::CouldntSaveOrCopy),
    ] {
        let mut session = open_with(Tool::Snip, SnipDestination::Both, DefaultFolder::Off(None));
        drag(&mut session);
        answer(&mut session, made);
        let chosen = Some(PathBuf::from(r"E:\Work\button.png"));
        session.handle(event(at(960, 540), 1045, Input::SaveAsClosed(chosen)));
        written(&mut session, saved);
        assert_eq!(
            notice(&session),
            Some(message),
            "copied {made}, saved {saved}"
        );
    }
}

#[test]
fn both_with_save_as_cancelled_says_copied_or_couldnt_copy() {
    for (made, message) in [(true, Message::Copied), (false, Message::CouldntCopy)] {
        let mut session = open_with(Tool::Snip, SnipDestination::Both, DefaultFolder::Off(None));
        drag(&mut session);
        answer(&mut session, made);
        session.handle(event(at(960, 540), 1045, Input::SaveAsClosed(None)));
        assert_eq!(notice(&session), Some(message));
    }
}

#[test]
fn both_with_save_as_failing_says_what_became_of_the_copy() {
    for (made, message) in [
        (true, Message::CouldntSaveButCopied),
        (false, Message::CouldntSaveOrCopy),
    ] {
        let mut session = open_with(Tool::Snip, SnipDestination::Both, DefaultFolder::Off(None));
        drag(&mut session);
        answer(&mut session, made);
        session.handle(event(at(960, 540), 1045, Input::SaveAsFailed));
        assert_eq!(notice(&session), Some(message));
    }
}

#[test]
fn snip_to_a_file_copies_nothing_and_says_only_what_became_of_the_file() {
    for (saved, message) in [(true, Message::Saved), (false, Message::CouldntSave)] {
        let mut session = open_with(
            Tool::Snip,
            SnipDestination::File,
            DefaultFolder::On(folder(r"D:\Snips")),
        );
        let effects = drag(&mut session);
        assert!(
            !effects
                .iter()
                .any(|effect| matches!(effect, Effect::CopyImage(_))),
            "{effects:?}"
        );
        written(&mut session, saved);
        assert_eq!(notice(&session), Some(message));
    }
}
