//! Layer 1 tests of pinned measurements and the copy in Dimensions (v1 spec WPC-296, Dimensions; ticket WPC-308): what a
//! click pins and copies, the notice it shows and when the notice goes, the stack that Backspace and Delete undo, the
//! guard against a double-click's second click, and closing, on frozen screens drawn here, without a screen. They look
//! only at the scene the session would draw and at the effects it returns.

use std::time::Duration;

use screengauge_core::session::{
    Colors, Effect, Event, Held, Image, Input, Key, LocalTime, Measurement, Message, Notice,
    Opening, Photo, Point, Rect, Scale, Session, Sign,
};
use screengauge_core::settings::{DimensionsMode, Options, Remembered};

/// The owner's 4K monitor at 175%, Windows' main monitor.
const AOC: Rect = Rect {
    left: 0,
    top: 0,
    width: 3840,
    height: 2160,
};

const WHITE: [u8; 3] = [255, 255, 255];
const CARD_BLUE: [u8; 3] = [37, 99, 235];

/// A monitor at `rect` and `dpi` (96 is 100%), white, with blue rectangles from (`left`, `top`) to (`right`, `bottom`),
/// both included, in physical pixels of the virtual screen.
fn monitor(rect: Rect, dpi: u32, blue: &[[i32; 4]]) -> Photo {
    let width = usize::try_from(rect.width).expect("a small monitor");
    let height = usize::try_from(rect.height).expect("a small monitor");
    let mut bgra = [WHITE[2], WHITE[1], WHITE[0], 255].repeat(width * height);
    for &[left, top, right, bottom] in blue {
        for y in top..=bottom {
            for x in left..=right {
                let column = usize::try_from(x - rect.left).expect("a pixel on the monitor");
                let row = usize::try_from(y - rect.top).expect("a pixel on the monitor");
                let at = (row * width + column) * 4;
                bgra[at..at + 3].copy_from_slice(&[CARD_BLUE[2], CARD_BLUE[1], CARD_BLUE[0]]);
            }
        }
    }
    Photo {
        rect,
        scale: Scale::from_dpi(dpi).expect("a real scale"),
        colors: Colors::Exact,
        image: Image::new(rect.width, rect.height, bgra).expect("the pixels fill the image"),
    }
}

/// Two blue cards of 200 × 201 on the 4K monitor at 175%, columns 800 to 999 and 1024 to 1223, rows 500 to 700, with a
/// white strip of 24 pixels between them: from (1010, 600) the horizontal span is 24, and from (900, 600), inside the
/// left card, the cross is 200 × 201.
fn two_cards() -> Photo {
    monitor(AOC, 168, &[[800, 500, 999, 700], [1024, 500, 1223, 700]])
}

fn at(x: i32, y: i32) -> Point {
    Point { x, y }
}

/// Dimensions in `mode`, the cursor at `cursor`, the default options, called by the global shortcut.
fn open(frozen_screen: Vec<Photo>, mode: DimensionsMode, cursor: Point) -> Session {
    Session::open(opening(frozen_screen, mode, cursor)).expect("a frozen screen the session takes")
}

fn opening(frozen_screen: Vec<Photo>, mode: DimensionsMode, cursor: Point) -> Opening {
    Opening {
        frozen_screen: frozen_screen.into_iter().map(Some).collect(),
        cursor,
        options: Options::default(),
        remembered: Remembered {
            dimensions_mode: mode,
            ..Remembered::default()
        },
        second_click_until: None,
    }
}

/// `input` with the cursor at `cursor`, no modifier held, `milliseconds` after Windows started, at 20:41:12 on
/// 2026-10-26.
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

/// A click at `cursor`, `milliseconds` after Windows started: the press is what counts.
fn click(cursor: Point, milliseconds: u64) -> Event {
    event(cursor, milliseconds, Input::Press)
}

fn shift_click(cursor: Point, milliseconds: u64) -> Event {
    Event {
        held: Held {
            shift: true,
            ..Held::default()
        },
        ..click(cursor, milliseconds)
    }
}

fn pinned(session: &Session) -> Vec<Measurement> {
    session.scene().pinned.to_vec()
}

fn copied(effects: &[Effect]) -> Vec<&str> {
    effects
        .iter()
        .filter_map(|effect| match effect {
            Effect::CopyText(text) => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

#[test]
fn a_click_pins_what_dimensions_measures_and_copies_its_physical_pixels() {
    let mut session = open(vec![two_cards()], DimensionsMode::Horizontal, at(1010, 600));
    let measured = session.scene().live.cloned().expect("a live measurement");
    let effects = session.handle(click(at(1010, 600), 1000));
    assert_eq!(pinned(&session), std::slice::from_ref(&measured));
    assert_eq!(copied(&effects), ["24"]);
    // It keeps measuring: the live measurement is still there, the same one.
    assert_eq!(session.scene().live, Some(&measured));
}

#[test]
fn a_cross_copies_width_then_height_with_the_letter_x_between_them() {
    // A white panel of 240 × 180, columns 2000 to 2239 and rows 1200 to 1379, on blue.
    let page = monitor(
        AOC,
        168,
        &[
            [0, 0, 3839, 1199],
            [0, 1380, 3839, 2159],
            [0, 1200, 1999, 1379],
            [2240, 1200, 3839, 1379],
        ],
    );
    let mut session = open(vec![page], DimensionsMode::Cross, at(2010, 1250));
    let effects = session.handle(click(at(2010, 1250), 1000));
    assert_eq!(copied(&effects), ["240 x 180"]);
}

#[test]
fn shift_and_a_click_copies_the_logical_pixels_with_a_point_before_the_tenths() {
    // 200 × 201 physical pixels at 175% are 114.3 × 114.9 logical pixels.
    let mut session = open(vec![two_cards()], DimensionsMode::Cross, at(900, 600));
    let effects = session.handle(shift_click(at(900, 600), 1000));
    assert_eq!(copied(&effects), ["114.3 x 114.9"]);
    assert_eq!(pinned(&session).len(), 1);
}

#[test]
fn shift_copies_the_logical_pixels_even_when_settings_hides_them() {
    let mut session = Session::open(Opening {
        options: Options {
            also_show_logical_pixels: false,
            ..Options::default()
        },
        ..opening(vec![two_cards()], DimensionsMode::Horizontal, at(1010, 600))
    })
    .expect("a frozen screen the session takes");
    let effects = session.handle(shift_click(at(1010, 600), 1000));
    assert_eq!(copied(&effects), ["13.7"]);
    // The option only hides them on the label, the pinned one's too.
    assert_eq!(pinned(&session)[0].label.logical, None);
}

#[test]
fn at_100_percent_shift_copies_the_same_whole_number() {
    let cards = monitor(AOC, 96, &[[800, 500, 999, 700], [1024, 500, 1223, 700]]);
    let mut session = open(vec![cards], DimensionsMode::Cross, at(900, 600));
    let physical = session.handle(click(at(900, 600), 1000));
    let logical = session.handle(shift_click(at(900, 600), 2000));
    assert_eq!(copied(&physical), ["200 x 201"]);
    assert_eq!(copied(&logical), ["200 x 201"]);
}

/// How long a notice stays: a little over a second, the approved prototype's 1.3 s (v1 spec WPC-296, Como as medidas
/// aparecem).
const NOTICE: u64 = 1300;

fn wakes(effects: &[Effect]) -> Vec<Duration> {
    effects
        .iter()
        .filter_map(|effect| match effect {
            Effect::WakeAt(uptime) => Some(*uptime),
            _ => None,
        })
        .collect()
}

#[test]
fn each_copy_shows_copied_where_the_click_was_with_an_ok_sign() {
    let mut session = open(vec![two_cards()], DimensionsMode::Horizontal, at(1010, 600));
    assert_eq!(session.scene().notice, None);
    session.handle(click(at(1010, 600), 1000));
    let notice = session.scene().notice.copied().expect("a notice");
    assert_eq!(
        notice,
        Notice {
            message: Message::Copied,
            at: at(1010, 600),
            monitor: 0,
        }
    );
    assert_eq!(notice.message.text(), "Copied");
    assert_eq!(notice.message.sign(), Sign::Ok);
}

#[test]
fn the_notice_stays_where_it_appeared_while_the_cursor_moves_on() {
    let mut session = open(vec![two_cards()], DimensionsMode::Horizontal, at(1010, 600));
    session.handle(click(at(1010, 600), 1000));
    session.handle(event(at(1500, 900), 1100, Input::Move));
    assert_eq!(
        session.scene().notice.map(|notice| notice.at),
        Some(at(1010, 600))
    );
}

#[test]
fn a_copy_asks_to_be_woken_when_its_notice_is_due_to_go() {
    let mut session = open(vec![two_cards()], DimensionsMode::Horizontal, at(1010, 600));
    let effects = session.handle(click(at(1010, 600), 1000));
    assert_eq!(
        effects,
        [
            Effect::CopyText("24".to_owned()),
            Effect::WakeAt(Duration::from_millis(1000 + NOTICE))
        ]
    );
}

#[test]
fn the_notice_goes_once_its_time_is_up_and_not_a_millisecond_before() {
    let mut session = open(vec![two_cards()], DimensionsMode::Horizontal, at(1010, 600));
    session.handle(click(at(1010, 600), 1000));
    // A timer may wake the session a little early: the notice stays, and the session asks again for the rest.
    let early = session.handle(event(at(1010, 600), 1000 + NOTICE - 1, Input::Wake));
    assert!(session.scene().notice.is_some());
    assert_eq!(wakes(&early), [Duration::from_millis(1000 + NOTICE)]);
    let due = session.handle(event(at(1010, 600), 1000 + NOTICE, Input::Wake));
    assert_eq!(session.scene().notice, None);
    assert_eq!(due, []);
}

#[test]
fn any_event_after_the_notice_is_due_takes_it_away() {
    // Should Windows never deliver the wake, the next move still takes the notice away.
    let mut session = open(vec![two_cards()], DimensionsMode::Horizontal, at(1010, 600));
    session.handle(click(at(1010, 600), 1000));
    session.handle(event(at(1011, 600), 1000 + NOTICE + 500, Input::Move));
    assert_eq!(session.scene().notice, None);
}

#[test]
fn an_earlier_notices_time_never_takes_away_a_newer_one() {
    let mut session = open(vec![two_cards()], DimensionsMode::Horizontal, at(1010, 600));
    session.handle(click(at(1010, 600), 1000));
    session.handle(event(at(900, 600), 1900, Input::Move));
    let second = session.handle(click(at(900, 600), 2000));
    assert_eq!(wakes(&second), [Duration::from_millis(2000 + NOTICE)]);
    // The first notice's wake comes: the second, shown a second later, stays its whole time.
    session.handle(event(at(900, 600), 1000 + NOTICE, Input::Wake));
    assert_eq!(
        session.scene().notice.map(|notice| notice.at),
        Some(at(900, 600))
    );
    session.handle(event(at(900, 600), 2000 + NOTICE, Input::Wake));
    assert_eq!(session.scene().notice, None);
}

#[test]
fn a_wake_with_no_notice_changes_nothing() {
    let mut session = open(vec![two_cards()], DimensionsMode::Horizontal, at(1010, 600));
    let live = session.scene().live.cloned();
    assert_eq!(session.handle(event(at(1010, 600), 5000, Input::Wake)), []);
    assert_eq!(session.scene().live.cloned(), live);
    assert_eq!(pinned(&session), []);
}

/// The owner's Full HD monitor at 125%, to the left of the 4K, all white.
const ARZOPA: Rect = Rect {
    left: -1920,
    top: 0,
    width: 1920,
    height: 1080,
};

fn key(cursor: Point, milliseconds: u64, key: Key) -> Event {
    event(cursor, milliseconds, Input::KeyDown(key))
}

#[test]
fn several_clicks_pin_several_measurements_oldest_first() {
    let mut session = open(vec![two_cards()], DimensionsMode::Horizontal, at(1010, 600));
    let gap = session.scene().live.cloned().expect("the gap");
    session.handle(click(at(1010, 600), 1000));
    session.handle(event(at(900, 600), 1500, Input::Move));
    let card = session.scene().live.cloned().expect("the card");
    session.handle(click(at(900, 600), 2000));
    assert_eq!(pinned(&session), [gap, card]);
}

#[test]
fn backspace_and_delete_each_take_away_the_newest_pin() {
    let mut session = open(vec![two_cards()], DimensionsMode::Horizontal, at(1010, 600));
    let gap = session.scene().live.cloned().expect("the gap");
    session.handle(click(at(1010, 600), 1000));
    session.handle(event(at(900, 600), 1500, Input::Move));
    session.handle(click(at(900, 600), 2000));
    session.handle(event(at(1100, 600), 2500, Input::Move));
    session.handle(click(at(1100, 600), 3000));
    assert_eq!(pinned(&session).len(), 3);
    assert_eq!(session.handle(key(at(1100, 600), 4000, Key::Backspace)), []);
    assert_eq!(pinned(&session).len(), 2);
    assert_eq!(session.handle(key(at(1100, 600), 4100, Key::Delete)), []);
    assert_eq!(pinned(&session), [gap]);
    // Dimensions goes on measuring, and the live measurement isn't taken away.
    assert!(session.scene().live.is_some());
}

#[test]
fn backspace_with_nothing_pinned_does_nothing() {
    let mut session = open(vec![two_cards()], DimensionsMode::Horizontal, at(1010, 600));
    let live = session.scene().live.cloned();
    assert_eq!(session.handle(key(at(1010, 600), 1000, Key::Backspace)), []);
    assert_eq!(pinned(&session), []);
    assert_eq!(session.scene().live.cloned(), live);
}

#[test]
fn a_pin_is_a_record_of_the_click_that_a_later_mode_doesnt_change() {
    let mut session = open(vec![two_cards()], DimensionsMode::Horizontal, at(1010, 600));
    let horizontal = session.scene().live.cloned().expect("the gap");
    session.handle(click(at(1010, 600), 1000));
    session.handle(key(at(1010, 600), 2000, Key::V));
    assert_ne!(session.scene().live.cloned(), Some(horizontal.clone()));
    assert_eq!(pinned(&session), [horizontal]);
}

#[test]
fn a_pin_stays_on_its_monitor_while_the_cursor_measures_on_another() {
    let mut session = open(
        vec![two_cards(), monitor(ARZOPA, 120, &[])],
        DimensionsMode::Horizontal,
        at(1010, 600),
    );
    let gap = session.scene().live.cloned().expect("the gap");
    session.handle(click(at(1010, 600), 1000));
    session.handle(event(at(-900, 500), 2000, Input::Move));
    assert_eq!(session.scene().live.map(|live| live.monitor), Some(1));
    assert_eq!(pinned(&session), [gap]);
    assert_eq!(pinned(&session)[0].monitor, 0);
    // Pinned on the other monitor, the copy and its notice are that monitor's.
    let effects = session.handle(click(at(-900, 500), 3000));
    assert_eq!(copied(&effects), ["1920"]);
    assert_eq!(session.scene().notice.map(|notice| notice.monitor), Some(1));
    assert_eq!(pinned(&session)[1].monitor, 1);
}

#[test]
fn every_way_of_closing_erases_the_pins_and_the_notice() {
    for exit in [
        Input::KeyDown(Key::Esc),
        Input::GlobalShortcut,
        Input::CloseAsked,
        Input::OtherProgramInFront,
        Input::MonitorsChanged,
    ] {
        let mut session = open(vec![two_cards()], DimensionsMode::Horizontal, at(1010, 600));
        session.handle(click(at(1010, 600), 1000));
        let effects = session.handle(event(at(1010, 600), 1100, exit.clone()));
        assert_eq!(effects.first(), Some(&Effect::Close), "{exit:?}");
        assert_eq!(pinned(&session), [], "{exit:?}");
        assert_eq!(session.scene().notice, None, "{exit:?}");
    }
}

#[test]
fn the_next_call_opens_with_nothing_pinned() {
    let mut session = open(vec![two_cards()], DimensionsMode::Horizontal, at(1010, 600));
    session.handle(click(at(1010, 600), 1000));
    session.handle(key(at(1010, 600), 1100, Key::Esc));
    let next = open(vec![two_cards()], DimensionsMode::Horizontal, at(1010, 600));
    assert_eq!(pinned(&next), []);
    assert_eq!(next.scene().notice, None);
}

#[test]
fn a_click_with_another_tool_active_pins_and_copies_nothing() {
    // Only Dimensions measures yet; Inspect's copy comes with WPC-316.
    let mut session = Session::open(Opening {
        remembered: Remembered {
            last_tool: screengauge_core::settings::Tool::Inspect,
            ..Remembered::default()
        },
        ..opening(vec![two_cards()], DimensionsMode::Horizontal, at(1010, 600))
    })
    .expect("a frozen screen the session takes");
    assert_eq!(session.handle(click(at(1010, 600), 1000)), []);
    assert_eq!(pinned(&session), []);
    assert_eq!(session.scene().notice, None);
}

/// Called from the tray icon at 10 s of uptime, with Windows' default double-click time of 500 ms: a press before
/// 10.5 s is the double-click's second click.
fn called_by_a_click() -> Session {
    Session::open(Opening {
        second_click_until: Some(Duration::from_millis(10_500)),
        ..opening(vec![two_cards()], DimensionsMode::Horizontal, at(1010, 600))
    })
    .expect("a frozen screen the session takes")
}

#[test]
fn the_second_click_of_the_double_click_that_called_screengauge_does_nothing() {
    let mut session = called_by_a_click();
    // The photo shows about 140 ms after the first click, and the second click lands on it.
    assert_eq!(session.handle(click(at(1010, 600), 10_140)), []);
    assert_eq!(session.handle(click(at(1010, 600), 10_499)), []);
    assert_eq!(pinned(&session), []);
    assert_eq!(session.scene().notice, None);
}

#[test]
fn a_click_after_the_double_click_time_counts() {
    let mut session = called_by_a_click();
    let effects = session.handle(click(at(1010, 600), 10_500));
    assert_eq!(copied(&effects), ["24"]);
    assert_eq!(pinned(&session).len(), 1);
}

#[test]
fn after_the_global_shortcut_a_click_counts_at_once() {
    // \`open\` opens as the global shortcut does, with no second click to wait for.
    let mut session = open(vec![two_cards()], DimensionsMode::Horizontal, at(1010, 600));
    let effects = session.handle(click(at(1010, 600), 10));
    assert_eq!(copied(&effects), ["24"]);
}

#[test]
fn the_guard_holds_back_only_clicks_not_the_keys_or_the_mouse() {
    let mut session = called_by_a_click();
    session.handle(event(at(900, 600), 10_200, Input::Move));
    assert_eq!(
        session.scene().live.map(|live| live.label.physical.clone()),
        Some("200".to_owned())
    );
    session.handle(key(at(900, 600), 10_300, Key::V));
    assert_eq!(
        session.scene().live.map(|live| live.label.physical.clone()),
        Some("201".to_owned())
    );
}
