//! Layer 1 tests of which side of the cursor a notice goes (ticket WPC-367): above it, as the spec says, except when
//! Inspect's magnified view went above the cursor near the monitor's bottom, where the notice goes below it, in the
//! place the view would have taken, so it never hides the color just copied. The side is decided when the copy is asked
//! for, where the click was, as the notice's place is. They run on a white frozen screen made up here, without a
//! screen or a clipboard, and look only at the scene the session would draw.

use std::time::Duration;

use screengauge_core::session::{
    Colors, Event, Held, Image, Input, LocalTime, Message, NoticeSide, Opening, Photo, Point, Rect,
    Scale, Session,
};
use screengauge_core::settings::{Options, Remembered, Tool};

/// A white Full HD monitor at 100%.
fn white() -> Photo {
    Photo {
        rect: Rect {
            left: 0,
            top: 0,
            width: 1920,
            height: 1080,
        },
        scale: Scale::from_dpi(96).expect("100%"),
        colors: Colors::Exact,
        image: Image::new(1920, 1080, [255; 4].repeat(1920 * 1080))
            .expect("the pixels fill the image"),
    }
}

fn at(x: i32, y: i32) -> Point {
    Point { x, y }
}

/// `tool` active, the cursor in the middle of the monitor.
fn open(tool: Tool) -> Session {
    Session::open(Opening {
        frozen_screen: vec![Some(white())],
        cursor: at(960, 540),
        options: Options::default(),
        remembered: Remembered {
            last_tool: tool,
            ..Remembered::default()
        },
        second_click_until: None,
    })
    .expect("a frozen screen the session takes")
}

/// `input` with the cursor at `cursor`, nothing held, `milliseconds` after Windows started.
fn event(cursor: Point, milliseconds: u64, input: Input) -> Event {
    Event {
        cursor,
        held: Held::default(),
        time: LocalTime {
            year: 2026,
            month: 11,
            day: 5,
            hour: 14,
            minute: 2,
            second: 0,
        },
        uptime: Duration::from_millis(milliseconds),
        input,
    }
}

/// Clicks at `click`, then moves the cursor to `then` before the clipboard answers `made`. Which side the notice went
/// to, and whether the magnified view was above the cursor at the click.
fn click_and_answer(
    session: &mut Session,
    click: Point,
    then: Point,
    made: bool,
) -> (NoticeSide, bool) {
    session.handle(event(click, 1000, Input::Move));
    let view_above = session
        .scene()
        .magnified_view
        .is_some_and(|view| view.panel.top < click.y);
    session.handle(event(click, 1010, Input::Press));
    session.handle(event(click, 1020, Input::Release));
    session.handle(event(then, 1030, Input::Move));
    let answer = if made {
        Input::Copied
    } else {
        Input::NotCopied
    };
    session.handle(event(then, 1040, answer));
    let notice = session.scene().notice.copied().expect("a notice");
    assert_eq!(notice.at, click, "the notice stays where the click was");
    (notice.side, view_above)
}

#[test]
fn inspects_notice_goes_below_the_cursor_exactly_where_the_view_is_above_it() {
    // Down the whole lower half of the monitor, the last row included: the notice is below exactly where the view
    // went above, and above everywhere else.
    let mut flips = None;
    for y in 540..1080 {
        let mut session = open(Tool::Inspect);
        let (side, view_above) = click_and_answer(&mut session, at(700, y), at(700, y), true);
        let wanted = if view_above {
            NoticeSide::Below
        } else {
            NoticeSide::Above
        };
        assert_eq!(side, wanted, "y = {y}");
        if view_above && flips.is_none() {
            flips = Some(y);
        }
    }
    // The view does go above near the bottom, in the monitor's bottom quarter, so the case is covered.
    let flips = flips.expect("the view goes above the cursor near the bottom");
    assert!((800..=830).contains(&flips), "flips at y = {flips}");
}

#[test]
fn couldnt_copy_goes_below_as_copied_does() {
    let mut session = open(Tool::Inspect);
    let (side, _) = click_and_answer(&mut session, at(700, 1030), at(700, 1030), false);
    assert_eq!(side, NoticeSide::Below);
    assert_eq!(
        session.scene().notice.map(|notice| notice.message),
        Some(Message::CouldntCopy)
    );
}

#[test]
fn the_side_is_the_one_at_the_click_whatever_the_cursor_does_before_the_answer() {
    // Clicked near the bottom, answered with the cursor in the middle: still below.
    let mut session = open(Tool::Inspect);
    let (side, _) = click_and_answer(&mut session, at(700, 1030), at(700, 300), true);
    assert_eq!(side, NoticeSide::Below);
    // Clicked in the middle, answered with the cursor near the bottom: still above.
    let mut session = open(Tool::Inspect);
    let (side, _) = click_and_answer(&mut session, at(700, 300), at(700, 1030), true);
    assert_eq!(side, NoticeSide::Above);
}

#[test]
fn notices_without_the_magnified_view_stay_above_near_the_bottom() {
    // Dimensions pins and copies where it clicks; Bounds and Snip, with nothing but white, say "No content found" where
    // the drag ends. None of them has a magnified view, so near the bottom their notices are where they always were.
    let mut session = open(Tool::Dimensions);
    let (side, _) = click_and_answer(&mut session, at(700, 1030), at(700, 1030), true);
    assert_eq!(side, NoticeSide::Above);
    for tool in [Tool::Bounds, Tool::Snip] {
        let mut session = open(tool);
        session.handle(event(at(600, 900), 1000, Input::Move));
        session.handle(event(at(600, 900), 1010, Input::Press));
        session.handle(event(at(700, 1030), 1020, Input::Move));
        session.handle(event(at(700, 1030), 1030, Input::Release));
        let notice = session.scene().notice.copied().expect("a notice");
        assert_eq!(notice.side, NoticeSide::Above, "{tool:?}");
    }
}
