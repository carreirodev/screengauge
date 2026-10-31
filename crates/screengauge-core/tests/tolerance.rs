//! Layer 1 tests of the tolerance adjusted with the mouse wheel (v1 spec WPC-296, Tolerance; ticket WPC-309): turning
//! the wheel forward raises it and back lowers it, one step a notch, within its limits; what Dimensions measures follows
//! at once, the pinned measurements don't; the tolerance indicator shows the new value and goes a second after the
//! wheel stops; and closing the tools, by any way out, keeps nothing, so the next call starts at the default tolerance.
//! They look only at the scene the session would draw and at the effects it returns.

use std::time::Duration;

use screengauge_core::session::{
    Colors, Effect, Event, Held, Image, Input, Key, LocalTime, Opening, Photo, Point, Rect, Scale,
    Session, Span, ToleranceIndicator,
};
use screengauge_core::settings::{DimensionsMode, Options, Remembered, Tolerance, Tool};

/// A Full HD monitor at 100%, Windows' main monitor, so labels have no logical part.
const FULL_HD: Rect = Rect {
    left: 0,
    top: 0,
    width: 1920,
    height: 1080,
};

/// The row the gray ramp is on.
const ROW: i32 = 500;

/// One notch of the mouse wheel, in Windows' units, turned forward: away from the user.
const NOTCH: i32 = 120;

/// A white monitor at `rect` with a gray ramp on row `ROW`: column `rect.left + 100 + n` has the gray 100 + n, for n from
/// 0 to 99. Each pixel differs from its neighbor by 1, so how far a span runs along the ramp depends on the tolerance
/// alone.
fn ramp_on(rect: Rect) -> Photo {
    let width = usize::try_from(rect.width).expect("a small monitor");
    let height = usize::try_from(rect.height).expect("a small monitor");
    let mut bgra = [255, 255, 255, 255].repeat(width * height);
    let row = usize::try_from(ROW - rect.top).expect("the row is on the monitor");
    for n in 0..100_u8 {
        let at = (row * width + 100 + usize::from(n)) * 4;
        bgra[at..at + 3].copy_from_slice(&[100 + n; 3]);
    }
    Photo {
        rect,
        scale: Scale::from_dpi(96).expect("a real scale"),
        colors: Colors::Exact,
        image: Image::new(rect.width, rect.height, bgra).expect("the pixels fill the image"),
    }
}

fn at(x: i32, y: i32) -> Point {
    Point { x, y }
}

/// Where the cursor starts: over the gray 150 of the ramp.
const GRAY_150: Point = Point { x: 150, y: ROW };

fn options() -> Options {
    Options::default()
}

/// Dimensions in the horizontal mode over the ramp, with `options`, the cursor on gray 150.
fn opening(options: Options) -> Opening {
    Opening {
        frozen_screen: vec![Some(ramp_on(FULL_HD))],
        cursor: GRAY_150,
        options,
        remembered: Remembered {
            dimensions_mode: DimensionsMode::Horizontal,
            ..Remembered::default()
        },
        second_click_until: None,
    }
}

fn open(options: Options) -> Session {
    Session::open(opening(options)).expect("a frozen screen the session takes")
}

/// `input` with the cursor at `cursor`, no modifier held, `milliseconds` after Windows started.
fn event(cursor: Point, milliseconds: u64, input: Input) -> Event {
    Event {
        cursor,
        held: Held::default(),
        time: LocalTime {
            year: 2026,
            month: 10,
            day: 31,
            hour: 21,
            minute: 5,
            second: 0,
        },
        uptime: Duration::from_millis(milliseconds),
        input,
    }
}

/// The wheel turned by `delta` with the cursor on gray 150, `milliseconds` after Windows started.
fn wheel(delta: i32, milliseconds: u64) -> Event {
    event(GRAY_150, milliseconds, Input::Wheel(delta))
}

/// The span along the ramp's row from `first` to `last`, both included.
fn row(first: i32, last: i32) -> Span {
    Span {
        first: at(first, ROW),
        last: at(last, ROW),
    }
}

/// The span Dimensions measures now.
fn live_span(session: &Session) -> Option<Span> {
    session.scene().live.and_then(|live| live.horizontal)
}

// Spans from gray 150, by the provisional rule of WPC-332: a pixel counts as the same area while none of its red, green
// or blue differs from the reference color's by more than three times the tolerance.

/// At the default tolerance, 2: gray 144 to gray 156.
fn at_two() -> Span {
    row(144, 156)
}

/// At 3: gray 141 to gray 159.
fn at_three() -> Span {
    row(141, 159)
}

#[test]
fn turning_the_wheel_forward_one_notch_raises_the_tolerance_and_dimensions_measures_again_at_once()
{
    let mut session = open(options());
    assert_eq!(live_span(&session), Some(at_two()));
    session.handle(wheel(NOTCH, 1000));
    assert_eq!(live_span(&session), Some(at_three()));
}

/// At 1: gray 147 to gray 153.
fn at_one() -> Span {
    row(147, 153)
}

#[test]
fn turning_the_wheel_back_one_notch_lowers_the_tolerance_and_dimensions_measures_again_at_once() {
    let mut session = open(options());
    session.handle(wheel(-NOTCH, 1000));
    assert_eq!(live_span(&session), Some(at_one()));
}

fn tolerance(value: u8) -> Tolerance {
    Tolerance::new(value).expect("within the limits")
}

fn indicator(session: &Session) -> Option<ToleranceIndicator> {
    session.scene().tolerance_indicator
}

#[test]
fn the_indicator_shows_the_tolerance_the_wheel_set_where_the_cursor_is() {
    let mut session = open(options());
    assert_eq!(indicator(&session), None);
    session.handle(wheel(NOTCH, 1000));
    assert_eq!(
        indicator(&session),
        Some(ToleranceIndicator {
            tolerance: tolerance(3),
            at: GRAY_150,
            monitor: 0,
        })
    );
}

/// How long the indicator stays after the wheel's last notch (v1 spec WPC-296, Tolerance).
const INDICATOR: u64 = 1000;

fn wakes(effects: &[Effect]) -> Vec<Duration> {
    effects
        .iter()
        .filter_map(|effect| match effect {
            Effect::WakeAt(uptime) => Some(*uptime),
            _ => None,
        })
        .collect()
}

fn ms(milliseconds: u64) -> Duration {
    Duration::from_millis(milliseconds)
}

#[test]
fn the_indicator_goes_a_second_after_the_wheels_last_notch() {
    let mut session = open(options());
    assert_eq!(
        wakes(&session.handle(wheel(NOTCH, 1000))),
        [ms(1000 + INDICATOR)]
    );
    // Another notch before then keeps it a second longer, from that notch.
    assert_eq!(
        wakes(&session.handle(wheel(NOTCH, 1600))),
        [ms(1600 + INDICATOR)]
    );
    session.handle(event(GRAY_150, 1600 + INDICATOR - 1, Input::Wake));
    assert!(indicator(&session).is_some());
    session.handle(event(GRAY_150, 1600 + INDICATOR, Input::Wake));
    assert_eq!(indicator(&session), None);
}

#[test]
fn a_wake_that_comes_early_keeps_the_indicator_and_asks_again() {
    let mut session = open(options());
    session.handle(wheel(NOTCH, 1000));
    let effects = session.handle(event(GRAY_150, 1990, Input::Wake));
    assert!(indicator(&session).is_some());
    assert_eq!(wakes(&effects), [ms(2000)]);
}

#[test]
fn any_event_after_its_time_takes_the_indicator_away_should_a_wake_be_lost() {
    let mut session = open(options());
    session.handle(wheel(NOTCH, 1000));
    session.handle(event(at(151, ROW), 2000, Input::Move));
    assert_eq!(indicator(&session), None);
}

/// How long the notice "Copied" stays (v1 spec WPC-296, Como as medidas aparecem).
const NOTICE: u64 = 1300;

/// A click with the cursor on gray 150: it pins what Dimensions measures and shows "Copied".
fn click(milliseconds: u64) -> Event {
    event(GRAY_150, milliseconds, Input::Press)
}

#[test]
fn with_a_notice_up_the_wheel_asks_to_be_woken_when_the_notice_goes_then_for_the_indicator() {
    let mut session = open(options());
    session.handle(click(1000));
    assert_eq!(
        wakes(&session.handle(wheel(NOTCH, 1500))),
        [ms(1000 + NOTICE)]
    );
    let effects = session.handle(event(GRAY_150, 1000 + NOTICE, Input::Wake));
    assert_eq!(session.scene().notice, None);
    assert!(indicator(&session).is_some());
    assert_eq!(wakes(&effects), [ms(1500 + INDICATOR)]);
    let effects = session.handle(event(GRAY_150, 1500 + INDICATOR, Input::Wake));
    assert_eq!(indicator(&session), None);
    assert_eq!(wakes(&effects), []);
}

#[test]
fn with_the_indicator_up_a_copy_asks_to_be_woken_when_the_indicator_goes_then_for_the_notice() {
    let mut session = open(options());
    session.handle(wheel(NOTCH, 1000));
    assert_eq!(wakes(&session.handle(click(1100))), [ms(1000 + INDICATOR)]);
    let effects = session.handle(event(GRAY_150, 1000 + INDICATOR, Input::Wake));
    assert_eq!(indicator(&session), None);
    assert!(session.scene().notice.is_some());
    assert_eq!(wakes(&effects), [ms(1100 + NOTICE)]);
}

#[test]
fn a_precision_touchpads_small_turns_add_up_to_one_step_a_notch() {
    let mut session = open(options());
    for milliseconds in [1000, 1010, 1020] {
        session.handle(wheel(30, milliseconds));
        assert_eq!(live_span(&session), Some(at_two()));
    }
    session.handle(wheel(30, 1030));
    assert_eq!(live_span(&session), Some(at_three()));
}

#[test]
fn two_notches_in_one_turn_are_two_steps() {
    let mut session = open(options());
    session.handle(wheel(2 * NOTCH, 1000));
    assert_eq!(
        indicator(&session).map(|shown| shown.tolerance),
        Some(tolerance(4))
    );
}

#[test]
fn turning_the_other_way_starts_counting_again_so_its_first_notch_is_a_step() {
    let mut session = open(options());
    session.handle(wheel(60, 1000));
    session.handle(wheel(-NOTCH, 1010));
    assert_eq!(live_span(&session), Some(at_one()));
}

fn starting_at(default_tolerance: Tolerance) -> Options {
    Options {
        default_tolerance,
        ..Options::default()
    }
}

fn shown_tolerance(session: &Session) -> Option<Tolerance> {
    indicator(session).map(|shown| shown.tolerance)
}

#[test]
fn at_the_highest_tolerance_turning_forward_keeps_it_there_and_still_shows_it() {
    let mut session = open(starting_at(Tolerance::MAX));
    // At the highest tolerance, the white beside the ramp counts as the same area as gray 150: the whole row.
    assert_eq!(live_span(&session), Some(row(0, 1919)));
    session.handle(wheel(NOTCH, 1000));
    assert_eq!(shown_tolerance(&session), Some(Tolerance::MAX));
    assert_eq!(live_span(&session), Some(row(0, 1919)));
}

#[test]
fn at_the_lowest_tolerance_turning_back_keeps_it_there_and_still_shows_it() {
    let mut session = open(starting_at(Tolerance::MIN));
    session.handle(wheel(-NOTCH, 1000));
    assert_eq!(shown_tolerance(&session), Some(Tolerance::MIN));
    assert_eq!(live_span(&session), Some(row(150, 150)));
}

#[test]
fn notches_past_a_limit_are_not_owed_so_the_first_notch_back_is_a_step() {
    let mut session = open(starting_at(Tolerance::MAX));
    session.handle(wheel(3 * NOTCH, 1000));
    session.handle(wheel(-NOTCH, 1010));
    assert_eq!(
        shown_tolerance(&session),
        Tolerance::new(Tolerance::MAX.get() - 1)
    );
}

/// The session opened with `tool` active.
fn open_with(tool: Tool) -> Session {
    let opening = opening(options());
    Session::open(Opening {
        remembered: Remembered {
            last_tool: tool,
            ..opening.remembered.clone()
        },
        ..opening
    })
    .expect("a frozen screen the session takes")
}

#[test]
fn with_inspect_active_the_wheel_leaves_the_tolerance_alone() {
    // Inspect uses no tolerance: its wheel changes the magnification (WPC-296, Inspect).
    let mut session = open_with(Tool::Inspect);
    let effects = session.handle(wheel(NOTCH, 1000));
    assert_eq!(effects, []);
    assert_eq!(indicator(&session), None);
}

#[test]
fn with_bounds_or_snip_active_the_wheel_changes_the_same_tolerance() {
    for tool in [Tool::Bounds, Tool::Snip] {
        let mut session = open_with(tool);
        session.handle(wheel(NOTCH, 1000));
        assert_eq!(shown_tolerance(&session), Some(tolerance(3)), "{tool:?}");
    }
}

#[test]
fn a_pinned_measurement_keeps_what_it_measured_when_the_wheel_changes_the_tolerance() {
    let mut session = open(options());
    session.handle(click(1000));
    session.handle(wheel(NOTCH, 2000));
    let pinned: Vec<Option<Span>> = session
        .scene()
        .pinned
        .iter()
        .map(|measurement| measurement.horizontal)
        .collect();
    assert_eq!(pinned, [Some(at_two())]);
    assert_eq!(live_span(&session), Some(at_three()));
}

/// A second Full HD monitor at 100%, right of the first, with its own ramp.
const RIGHT: Rect = Rect {
    left: 1920,
    top: 0,
    width: 1920,
    height: 1080,
};

#[test]
fn the_indicator_follows_the_cursor_onto_another_monitor() {
    let mut session = Session::open(Opening {
        frozen_screen: vec![Some(ramp_on(FULL_HD)), Some(ramp_on(RIGHT))],
        ..opening(options())
    })
    .expect("a frozen screen the session takes");
    session.handle(wheel(NOTCH, 1000));
    session.handle(event(at(2100, 700), 1100, Input::Move));
    assert_eq!(
        indicator(&session),
        Some(ToleranceIndicator {
            tolerance: tolerance(3),
            at: at(2100, 700),
            monitor: 1,
        })
    );
}

#[test]
fn closing_the_tools_takes_the_indicator_with_them() {
    let mut session = open(options());
    session.handle(wheel(NOTCH, 1000));
    session.handle(event(GRAY_150, 1100, Input::KeyDown(Key::Esc)));
    assert_eq!(indicator(&session), None);
}

/// Every way the tools close (v1 spec WPC-296, Fechar as ferramentas) that the session takes yet: Esc, the global
/// shortcut again, Alt+F4, another program in front and a change of monitors.
fn ways_out() -> [Input; 5] {
    [
        Input::KeyDown(Key::Esc),
        Input::GlobalShortcut,
        Input::CloseAsked,
        Input::OtherProgramInFront,
        Input::MonitorsChanged,
    ]
}

#[test]
fn closing_the_tools_any_way_keeps_nothing_of_the_tolerance_the_wheel_set() {
    for way_out in ways_out() {
        let mut session = open(options());
        session.handle(wheel(NOTCH, 1000));
        let effects = session.handle(event(GRAY_150, 1100, way_out.clone()));
        assert_eq!(effects, [Effect::Close], "{way_out:?}");
    }
}

#[test]
fn the_next_call_starts_at_the_default_tolerance_in_settings() {
    // A default other than the built-in one, as the settings file may hold: 5, so from gray 150 the span runs from gray
    // 135 to gray 165, and after a notch forward, at 6, from gray 132 to gray 168.
    let settings = starting_at(tolerance(5));
    let mut session = open(settings.clone());
    assert_eq!(live_span(&session), Some(row(135, 165)));
    session.handle(wheel(NOTCH, 1000));
    assert_eq!(live_span(&session), Some(row(132, 168)));
    session.handle(event(GRAY_150, 1100, Input::KeyDown(Key::Esc)));
    let next = open(settings);
    assert_eq!(live_span(&next), Some(row(135, 165)));
    assert_eq!(indicator(&next), None);
}
