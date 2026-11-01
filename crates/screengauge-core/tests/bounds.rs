//! Layer 1 tests of Bounds drawing a rectangle and shrinking it to the content (v1 spec WPC-296, Bounds; ticket
//! WPC-313): what a drag shows while the button is held, what releasing it pins and copies, the monitor's limit, the
//! stack Bounds shares with Dimensions, and switching between the two, on frozen screens drawn here, without a screen.
//! They look only at the scene the session would draw and at the effects it returns.

use std::time::Duration;

use screengauge_core::session::toolbar;
use screengauge_core::session::{
    Colors, Cursor, DrawnRectangle, Effect, Event, Held, Image, Input, Key, Label, LocalTime,
    Measurement, Message, Notice, Opening, Photo, Point, Rect, Scale, Session, Sign, ToolbarButton,
};
use screengauge_core::settings::{DimensionsMode, Options, Remembered, Tool, ToolbarPlace};

/// The owner's 4K monitor at 175%, Windows' main monitor.
const AOC: Rect = Rect {
    left: 0,
    top: 0,
    width: 3840,
    height: 2160,
};

const WHITE: [u8; 3] = [255, 255, 255];
const BUTTON_BLUE: [u8; 3] = [37, 99, 235];

/// A monitor at `rect` and `dpi` (96 is 100%), white, with rectangles of the given colors from (`left`, `top`) to
/// (`right`, `bottom`), both included, in physical pixels of the virtual screen. Later rectangles cover earlier ones.
fn monitor(rect: Rect, dpi: u32, shapes: &[([u8; 3], [i32; 4])]) -> Photo {
    let width = usize::try_from(rect.width).expect("a small monitor");
    let height = usize::try_from(rect.height).expect("a small monitor");
    let mut bgra = [WHITE[2], WHITE[1], WHITE[0], 255].repeat(width * height);
    for &(color, [left, top, right, bottom]) in shapes {
        for y in top..=bottom {
            for x in left..=right {
                let column = usize::try_from(x - rect.left).expect("a pixel on the monitor");
                let row = usize::try_from(y - rect.top).expect("a pixel on the monitor");
                let at = (row * width + column) * 4;
                bgra[at..at + 3].copy_from_slice(&[color[2], color[1], color[0]]);
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

/// A blue button of 100 × 40 on the 4K monitor at 175%, columns 1000 to 1099 and rows 600 to 639.
fn button() -> Photo {
    monitor(AOC, 168, &[(BUTTON_BLUE, [1000, 600, 1099, 639])])
}

fn at(x: i32, y: i32) -> Point {
    Point { x, y }
}

/// Bounds active, the cursor at `cursor`, the default options, called by the global shortcut.
fn open(frozen_screen: Vec<Photo>, cursor: Point) -> Session {
    Session::open(Opening {
        frozen_screen: frozen_screen.into_iter().map(Some).collect(),
        cursor,
        options: Options::default(),
        remembered: Remembered {
            last_tool: Tool::Bounds,
            ..Remembered::default()
        },
        second_click_until: None,
    })
    .expect("a frozen screen the session takes")
}

/// `input` with the cursor at `cursor`, `held` down, `milliseconds` after Windows started, at 20:41:12 on 2026-10-26.
fn event_holding(cursor: Point, held: Held, milliseconds: u64, input: Input) -> Event {
    Event {
        cursor,
        held,
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

fn event(cursor: Point, milliseconds: u64, input: Input) -> Event {
    event_holding(cursor, Held::default(), milliseconds, input)
}

/// Moves the cursor to `from`, presses the button there and drags to `to`, without letting go.
fn hold(session: &mut Session, from: Point, to: Point, milliseconds: u64) {
    session.handle(event(from, milliseconds, Input::Move));
    session.handle(event(from, milliseconds + 10, Input::Press));
    session.handle(event(to, milliseconds + 20, Input::Move));
}

/// Drags from `from` to `to` and lets go there, with nothing held; what the release returns.
fn drag(session: &mut Session, from: Point, to: Point, milliseconds: u64) -> Vec<Effect> {
    hold(session, from, to, milliseconds);
    session.handle(event(to, milliseconds + 30, Input::Release))
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
fn a_drag_around_a_100_by_40_button_copies_100_x_40() {
    let mut session = open(vec![button()], at(900, 500));
    let effects = drag(&mut session, at(900, 500), at(1200, 700), 1000);
    assert_eq!(copied(&effects), ["100 x 40"]);
}

fn pinned(session: &Session) -> Vec<Measurement> {
    session.scene().pinned.to_vec()
}

/// What Bounds measures for the 100 × 40 button on the 4K monitor at 175%, where it is 57.1 × 22.9 logical pixels.
fn the_button() -> Measurement {
    Measurement {
        monitor: 0,
        horizontal: None,
        vertical: None,
        content: Some(Rect {
            left: 1000,
            top: 600,
            width: 100,
            height: 40,
        }),
        label: Label {
            physical: "100 × 40".to_owned(),
            logical: Some("57.1 × 22.9".to_owned()),
        },
    }
}

#[test]
fn letting_go_pins_the_content_and_shows_copied_where_the_cursor_is() {
    let mut session = open(vec![button()], at(900, 500));
    drag(&mut session, at(900, 500), at(1200, 700), 1000);
    assert_eq!(pinned(&session), [the_button()]);
    assert_eq!(
        session.scene().notice.copied(),
        Some(Notice {
            message: Message::Copied,
            at: at(1200, 700),
            monitor: 0,
        })
    );
}

#[test]
fn after_letting_go_nothing_is_live_and_the_next_drag_pins_another() {
    let mut session = open(vec![button()], at(900, 500));
    drag(&mut session, at(900, 500), at(1200, 700), 1000);
    assert_eq!(session.scene().live, None);
    // A second drag, from the bottom right corner up to the top left one this time, pins the same button again.
    let effects = drag(&mut session, at(1150, 680), at(950, 550), 2000);
    assert_eq!(copied(&effects), ["100 x 40"]);
    assert_eq!(pinned(&session), [the_button(), the_button()]);
}

fn rect(left: i32, top: i32, right: i32, bottom: i32) -> Rect {
    Rect {
        left,
        top,
        width: left.abs_diff(right) + 1,
        height: top.abs_diff(bottom) + 1,
    }
}

/// The drawn rectangle the scene shows, on its monitor.
fn drawn(session: &Session) -> Option<DrawnRectangle> {
    session.scene().drawn_rectangle
}

/// The content the live preview shows.
fn live_content(session: &Session) -> Option<Rect> {
    session.scene().live.and_then(|live| live.content)
}

#[test]
fn while_the_button_is_held_the_preview_shows_the_drawn_rectangle_and_the_content_inside_it() {
    let mut session = open(vec![button()], at(900, 500));
    hold(&mut session, at(900, 500), at(1200, 700), 1000);
    assert_eq!(
        drawn(&session),
        Some(DrawnRectangle {
            monitor: 0,
            rect: rect(900, 500, 1200, 700),
        })
    );
    assert_eq!(session.scene().live, Some(&the_button()));
    // Nothing is pinned or copied until the button comes up.
    assert_eq!(pinned(&session), []);
}

#[test]
fn the_preview_follows_each_move_of_the_drag() {
    let mut session = open(vec![button()], at(900, 500));
    hold(&mut session, at(900, 500), at(1050, 620), 1000);
    assert_eq!(
        drawn(&session).map(|drawn| drawn.rect),
        Some(rect(900, 500, 1050, 620))
    );
    assert_eq!(live_content(&session), Some(rect(1000, 600, 1050, 620)));
    session.handle(event(at(1200, 700), 1100, Input::Move));
    assert_eq!(live_content(&session), Some(rect(1000, 600, 1099, 639)));
}

#[test]
fn letting_go_takes_the_drawn_rectangle_away() {
    let mut session = open(vec![button()], at(900, 500));
    drag(&mut session, at(900, 500), at(1200, 700), 1000);
    assert_eq!(drawn(&session), None);
}

#[test]
fn before_the_press_bounds_shows_nothing() {
    let mut session = open(vec![button()], at(900, 500));
    session.handle(event(at(1050, 620), 1000, Input::Move));
    assert_eq!(drawn(&session), None);
    assert_eq!(session.scene().live, None);
}

const ICON_GRAY: [u8; 3] = [90, 90, 90];
const TEXT_BLACK: [u8; 3] = [20, 20, 20];

#[test]
fn content_in_several_places_shrinks_to_one_rectangle_with_the_gaps_inside() {
    // Two icons of 20 × 20 a hundred pixels apart, the second lower than the first.
    let icons = monitor(
        AOC,
        168,
        &[
            (ICON_GRAY, [1000, 600, 1019, 619]),
            (ICON_GRAY, [1120, 630, 1139, 649]),
        ],
    );
    let mut session = open(vec![icons], at(950, 550));
    let effects = drag(&mut session, at(950, 550), at(1200, 700), 1000);
    assert_eq!(copied(&effects), ["140 x 50"]);
}

#[test]
fn an_icon_and_its_text_in_the_same_rectangle_shrink_to_one_rectangle() {
    // An icon of 24 × 24, then 8 pixels further a word of three letters 10 pixels tall, its strokes 2 pixels wide,
    // sitting on the icon's bottom.
    let item = monitor(
        AOC,
        168,
        &[
            (ICON_GRAY, [1000, 600, 1023, 623]),
            (TEXT_BLACK, [1032, 614, 1033, 623]),
            (TEXT_BLACK, [1038, 614, 1043, 615]),
            (TEXT_BLACK, [1048, 618, 1049, 623]),
        ],
    );
    let mut session = open(vec![item], at(950, 550));
    let effects = drag(&mut session, at(950, 550), at(1100, 700), 1000);
    assert_eq!(copied(&effects), ["50 x 24"]);
}

#[test]
fn a_soft_shadow_or_an_antialiased_edge_counts_as_content_once_it_differs_by_more_than_the_tolerance()
 {
    // The button with a faint shadow row under it, 15 levels off the white, and a fainter row under that, 3 levels off:
    // at the default tolerance, a level of difference more than 6 is content.
    let shadowed = monitor(
        AOC,
        168,
        &[
            (BUTTON_BLUE, [1000, 600, 1099, 639]),
            ([240, 240, 240], [1002, 640, 1101, 640]),
            ([252, 252, 252], [1002, 641, 1101, 641]),
        ],
    );
    let mut session = open(vec![shadowed], at(900, 500));
    let effects = drag(&mut session, at(900, 500), at(1200, 700), 1000);
    assert_eq!(copied(&effects), ["102 x 41"]);
}

#[test]
fn a_rectangle_that_cuts_through_the_button_measures_only_the_part_inside_it() {
    let mut session = open(vec![button()], at(900, 500));
    let effects = drag(&mut session, at(900, 500), at(1049, 619), 1000);
    assert_eq!(copied(&effects), ["50 x 20"]);
}

#[test]
fn starting_on_the_button_makes_its_blue_the_reference_color_and_the_rectangle_hardly_shrinks() {
    // Begun inside the button, the blue is the reference color, and the white around the button is the content: it
    // reaches every side of what was drawn.
    let mut session = open(vec![button()], at(1010, 610));
    let effects = drag(&mut session, at(1010, 610), at(1200, 700), 1000);
    assert_eq!(copied(&effects), ["191 x 91"]);
}

#[test]
fn a_drag_in_any_direction_draws_the_same_rectangle() {
    for (from, to) in [
        (at(900, 500), at(1200, 700)),
        (at(1200, 700), at(900, 500)),
        (at(1200, 500), at(900, 700)),
        (at(900, 700), at(1200, 500)),
    ] {
        let mut session = open(vec![button()], from);
        hold(&mut session, from, to, 1000);
        assert_eq!(
            drawn(&session).map(|drawn| drawn.rect),
            Some(rect(900, 500, 1200, 700)),
            "{from:?} to {to:?}"
        );
        assert_eq!(
            session.scene().live,
            Some(&the_button()),
            "{from:?} to {to:?}"
        );
    }
}

/// The owner's Full HD monitor at 125%, to the right of the 4K, with a gray square on it right of the 4K's side.
const ARZOPA: Rect = Rect {
    left: 3840,
    top: 0,
    width: 1920,
    height: 1080,
};

/// A blue square of 40 × 40 against the 4K monitor's right side, rows 600 to 639, and a gray one on the Full HD
/// monitor to its right.
fn across_two_monitors() -> Vec<Photo> {
    vec![
        monitor(AOC, 168, &[(BUTTON_BLUE, [3800, 600, 3839, 639])]),
        monitor(ARZOPA, 120, &[(ICON_GRAY, [3860, 600, 3899, 639])]),
    ]
}

#[test]
fn the_rectangle_stops_at_the_limit_of_the_monitor_the_drag_began_on() {
    let mut session = open(across_two_monitors(), at(3700, 500));
    hold(&mut session, at(3700, 500), at(4000, 700), 1000);
    assert_eq!(
        drawn(&session),
        Some(DrawnRectangle {
            monitor: 0,
            rect: rect(3700, 500, 3839, 700),
        })
    );
    // The gray square on the other monitor isn't measured, and the logical pixels are the 4K's at 175%.
    let effects = session.handle(event_holding(
        at(4000, 700),
        Held {
            shift: true,
            ..Held::default()
        },
        1100,
        Input::Release,
    ));
    assert_eq!(copied(&effects), ["22.9 x 22.9"]);
    assert_eq!(
        pinned(&session)[0].label,
        Label {
            physical: "40 × 40".to_owned(),
            logical: Some("22.9 × 22.9".to_owned()),
        }
    );
    // The notice is where the cursor is, on the monitor the cursor went on to, as every notice is.
    assert_eq!(
        session
            .scene()
            .notice
            .map(|notice| (notice.at, notice.monitor)),
        Some((at(4000, 700), 1))
    );
}

#[test]
fn begun_on_the_other_monitor_the_rectangle_stops_at_that_monitors_limit() {
    let mut session = open(across_two_monitors(), at(3950, 700));
    hold(&mut session, at(3950, 700), at(3700, 500), 1000);
    assert_eq!(
        drawn(&session),
        Some(DrawnRectangle {
            monitor: 1,
            rect: rect(3840, 500, 3950, 700),
        })
    );
    let effects = session.handle(event(at(3700, 500), 1100, Input::Release));
    assert_eq!(copied(&effects), ["40 x 40"]);
    assert_eq!(pinned(&session)[0].monitor, 1);
    assert_eq!(
        pinned(&session)[0].label.logical.as_deref(),
        Some("32 × 32")
    );
}

const SHIFT: Held = Held {
    shift: true,
    ctrl: false,
    alt: false,
};

/// How far the mouse wheel turns in a notch, in Windows' units.
const NOTCH: i32 = 120;

#[test]
fn the_wheel_in_the_middle_of_the_drag_changes_the_preview_at_once() {
    // A faint panel above the button, 10 levels off the white: content at the default tolerance, whose limit is 6
    // levels, and like the reference color two notches up, at 12.
    let faint = monitor(
        AOC,
        168,
        &[
            ([245, 245, 245], [950, 560, 1150, 580]),
            (BUTTON_BLUE, [1000, 600, 1099, 639]),
        ],
    );
    let mut session = open(vec![faint], at(900, 500));
    hold(&mut session, at(900, 500), at(1200, 700), 1000);
    assert_eq!(live_content(&session), Some(rect(950, 560, 1150, 639)));
    session.handle(event(at(1200, 700), 1100, Input::Wheel(NOTCH)));
    session.handle(event(at(1200, 700), 1150, Input::Wheel(NOTCH)));
    assert_eq!(live_content(&session), Some(rect(1000, 600, 1099, 639)));
    // The tolerance indicator shows the new tolerance where the cursor is.
    let indicator = session.scene().tolerance_indicator.expect("the indicator");
    assert_eq!(
        (indicator.tolerance.get(), indicator.at),
        (4, at(1200, 700))
    );
    // Letting go pins what the preview shows.
    let effects = session.handle(event(at(1200, 700), 1200, Input::Release));
    assert_eq!(copied(&effects), ["100 x 40"]);
}

#[test]
fn with_the_button_held_the_arrows_move_the_rectangles_corner_one_pixel_or_ten_with_shift() {
    let mut session = open(vec![button()], at(900, 500));
    hold(&mut session, at(900, 500), at(1050, 620), 1000);
    let effects = session.handle(event(at(1050, 620), 1100, Input::KeyDown(Key::Right)));
    assert_eq!(effects, [Effect::MoveCursor(at(1051, 620))]);
    assert_eq!(
        drawn(&session).map(|drawn| drawn.rect),
        Some(rect(900, 500, 1051, 620))
    );
    assert_eq!(live_content(&session), Some(rect(1000, 600, 1051, 620)));
    session.handle(event_holding(
        at(1051, 620),
        SHIFT,
        1200,
        Input::KeyDown(Key::Down),
    ));
    assert_eq!(live_content(&session), Some(rect(1000, 600, 1051, 630)));
}

#[test]
fn without_the_button_the_arrows_move_where_the_next_drag_begins() {
    let mut session = open(vec![button()], at(1010, 610));
    // Begun on the button, a drag would make the blue the reference color; ten pixels up and left it begins on the
    // white.
    session.handle(event_holding(
        at(1010, 610),
        SHIFT,
        1000,
        Input::KeyDown(Key::Up),
    ));
    session.handle(event_holding(
        at(1010, 600),
        SHIFT,
        1100,
        Input::KeyDown(Key::Left),
    ));
    session.handle(event(at(1000, 600), 1200, Input::KeyDown(Key::Up)));
    session.handle(event(at(1000, 599), 1300, Input::KeyDown(Key::Left)));
    session.handle(event(at(999, 599), 1400, Input::Press));
    session.handle(event(at(1200, 700), 1500, Input::Move));
    assert_eq!(
        drawn(&session).map(|drawn| drawn.rect),
        Some(rect(999, 599, 1200, 700))
    );
    assert_eq!(live_content(&session), Some(rect(1000, 600, 1099, 639)));
}

#[test]
fn shift_held_as_the_button_comes_up_copies_the_logical_pixels() {
    let mut session = open(vec![button()], at(900, 500));
    hold(&mut session, at(900, 500), at(1200, 700), 1000);
    let effects = session.handle(event_holding(at(1200, 700), SHIFT, 1100, Input::Release));
    assert_eq!(copied(&effects), ["57.1 x 22.9"]);
}

#[test]
fn shift_held_only_for_the_arrows_copies_the_physical_pixels() {
    // Shift counts as it is when the button comes up, even when it was held to move with the arrows.
    let mut session = open(vec![button()], at(900, 500));
    hold(&mut session, at(900, 500), at(1190, 700), 1000);
    session.handle(event_holding(
        at(1190, 700),
        SHIFT,
        1100,
        Input::KeyDown(Key::Right),
    ));
    session.handle(event_holding(
        at(1200, 700),
        SHIFT,
        1200,
        Input::KeyUp(Key::Shift),
    ));
    let effects = session.handle(event(at(1200, 700), 1300, Input::Release));
    assert_eq!(copied(&effects), ["100 x 40"]);
}

/// `tool` active, Dimensions in its horizontal mode, the cursor at `cursor`, the default options, called by the global
/// shortcut.
fn open_as(tool: Tool, frozen_screen: Vec<Photo>, cursor: Point) -> Session {
    Session::open(Opening {
        frozen_screen: frozen_screen.into_iter().map(Some).collect(),
        cursor,
        options: Options::default(),
        remembered: Remembered {
            last_tool: tool,
            dimensions_mode: DimensionsMode::Horizontal,
            ..Remembered::default()
        },
        second_click_until: None,
    })
    .expect("a frozen screen the session takes")
}

fn key(cursor: Point, milliseconds: u64, key: Key) -> Event {
    event(cursor, milliseconds, Input::KeyDown(key))
}

/// What a click in Dimensions pins with the cursor at (1050, 620), inside the button: its row, 100 pixels.
fn dimensions_pin(session: &mut Session, milliseconds: u64) -> Measurement {
    session.handle(event(at(1050, 620), milliseconds, Input::Move));
    let live = session.scene().live.cloned().expect("Dimensions measures");
    assert_eq!(live.label.physical, "100");
    session.handle(event(at(1050, 620), milliseconds + 10, Input::Press));
    session.handle(event(at(1050, 620), milliseconds + 20, Input::Release));
    live
}

#[test]
fn backspace_in_bounds_takes_away_the_newest_pin_whichever_tool_pinned_it() {
    let mut session = open_as(Tool::Dimensions, vec![button()], at(1050, 620));
    let row = dimensions_pin(&mut session, 1000);
    session.handle(key(at(1050, 620), 2000, Key::Digit2));
    drag(&mut session, at(900, 500), at(1200, 700), 3000);
    assert_eq!(pinned(&session), [row.clone(), the_button()]);
    assert_eq!(session.handle(key(at(1200, 700), 4000, Key::Backspace)), []);
    assert_eq!(pinned(&session), [row]);
    session.handle(key(at(1200, 700), 4100, Key::Delete));
    assert_eq!(pinned(&session), []);
}

#[test]
fn backspace_in_dimensions_takes_away_a_pin_bounds_made() {
    let mut session = open_as(Tool::Bounds, vec![button()], at(900, 500));
    drag(&mut session, at(900, 500), at(1200, 700), 1000);
    session.handle(key(at(1200, 700), 2000, Key::Digit1));
    let row = dimensions_pin(&mut session, 3000);
    assert_eq!(pinned(&session), [the_button(), row]);
    // With Bounds active again, the newest is Dimensions' row.
    session.handle(key(at(1050, 620), 4000, Key::Digit2));
    session.handle(key(at(1050, 620), 4100, Key::Backspace));
    assert_eq!(pinned(&session), [the_button()]);
    session.handle(key(at(1050, 620), 4200, Key::Digit1));
    session.handle(key(at(1050, 620), 4300, Key::Backspace));
    assert_eq!(pinned(&session), []);
    assert!(session.scene().live.is_some(), "Dimensions still measures");
}

#[test]
fn key_2_and_the_toolbar_button_make_bounds_the_active_tool() {
    for switch in [
        Input::KeyDown(Key::Digit2),
        Input::Toolbar(ToolbarButton::Tool(Tool::Bounds)),
    ] {
        let mut session = open_as(Tool::Dimensions, vec![button()], at(900, 500));
        session.handle(event(at(900, 500), 1000, switch.clone()));
        assert_eq!(session.scene().toolbar.active, Tool::Bounds, "{switch:?}");
        // Dimensions no longer measures around the cursor, and a drag shrinks to the button.
        assert_eq!(session.scene().live, None, "{switch:?}");
        let effects = drag(&mut session, at(900, 500), at(1200, 700), 2000);
        assert_eq!(copied(&effects), ["100 x 40"], "{switch:?}");
    }
}

#[test]
fn switching_between_dimensions_and_bounds_keeps_the_photo_both_tools_pins_and_the_tolerance() {
    for (to_bounds, to_dimensions) in [
        (Input::KeyDown(Key::Digit2), Input::KeyDown(Key::Digit1)),
        (
            Input::Toolbar(ToolbarButton::Tool(Tool::Bounds)),
            Input::Toolbar(ToolbarButton::Tool(Tool::Dimensions)),
        ),
    ] {
        let faint = monitor(
            AOC,
            168,
            &[
                ([245, 245, 245], [950, 560, 1150, 580]),
                (BUTTON_BLUE, [1000, 600, 1099, 639]),
            ],
        );
        let photo = faint.clone();
        let mut session = open_as(Tool::Dimensions, vec![faint], at(1050, 620));
        let row = dimensions_pin(&mut session, 1000);
        // Two notches in Dimensions raise the tolerance to 4, past the faint panel's 10 levels.
        session.handle(event(at(1050, 620), 1100, Input::Wheel(NOTCH)));
        session.handle(event(at(1050, 620), 1150, Input::Wheel(NOTCH)));
        session.handle(event(at(900, 500), 1200, to_bounds.clone()));
        assert_eq!(session.scene().frozen_screen, std::slice::from_ref(&photo));
        assert_eq!(pinned(&session), std::slice::from_ref(&row));
        // The preview shrinks with the tolerance the wheel set, past the faint panel to the button, and the indicator
        // shows the same number.
        hold(&mut session, at(900, 500), at(1200, 700), 1300);
        assert_eq!(live_content(&session), Some(rect(1000, 600, 1099, 639)));
        assert_eq!(
            session
                .scene()
                .tolerance_indicator
                .map(|shown| shown.tolerance.get()),
            Some(4)
        );
        session.handle(event(at(1200, 700), 1400, Input::Release));
        session.handle(event(at(1050, 620), 1500, to_dimensions.clone()));
        assert_eq!(pinned(&session), [row.clone(), the_button()]);
        assert_eq!(session.scene().frozen_screen, std::slice::from_ref(&photo));
        // Dimensions measures with that tolerance too.
        session.handle(event(at(1050, 620), 1600, Input::Wheel(NOTCH)));
        assert_eq!(
            session
                .scene()
                .tolerance_indicator
                .map(|shown| shown.tolerance.get()),
            Some(5)
        );
    }
}

#[test]
fn a_pin_is_a_record_of_the_release_that_a_later_turn_of_the_wheel_doesnt_change() {
    let mut session = open(vec![button()], at(900, 500));
    drag(&mut session, at(900, 500), at(1200, 700), 1000);
    for notch in 0..20 {
        session.handle(event(at(1200, 700), 2000 + notch, Input::Wheel(NOTCH)));
    }
    assert_eq!(pinned(&session), [the_button()]);
}

#[test]
fn a_drag_over_nothing_but_the_reference_color_shows_no_content_found_and_pins_and_copies_nothing()
{
    let mut session = open(vec![button()], at(100, 100));
    hold(&mut session, at(100, 100), at(400, 300), 1000);
    // The preview shows the drawn rectangle alone.
    assert_eq!(session.scene().live, None);
    assert_eq!(
        drawn(&session).map(|drawn| drawn.rect),
        Some(rect(100, 100, 400, 300))
    );
    let effects = session.handle(event(at(400, 300), 1100, Input::Release));
    // Nothing reaches the clipboard, which stays as it was: the notice only asks to be woken when it goes.
    assert_eq!(effects, [Effect::WakeAt(Duration::from_millis(2400))]);
    assert_eq!(pinned(&session), []);
    assert_eq!(drawn(&session), None);
    assert_eq!(
        session.scene().notice.copied(),
        Some(Notice {
            message: Message::NoContentFound,
            at: at(400, 300),
            monitor: 0,
        })
    );
}

#[test]
fn no_content_found_has_an_alert_sign() {
    assert_eq!(Message::NoContentFound.text(), "No content found");
    assert_eq!(Message::NoContentFound.sign(), Sign::Alert);
    assert_eq!(Message::Copied.sign(), Sign::Ok);
}

#[test]
fn the_second_click_of_the_double_click_that_called_screengauge_starts_no_drag() {
    // Called from the tray icon at 10 s of uptime: a press before 10.5 s is the double-click's second click.
    let mut session = Session::open(Opening {
        second_click_until: Some(Duration::from_millis(10_500)),
        ..Opening {
            frozen_screen: vec![Some(button())],
            cursor: at(900, 500),
            options: Options::default(),
            remembered: Remembered {
                last_tool: Tool::Bounds,
                ..Remembered::default()
            },
            second_click_until: None,
        }
    })
    .expect("a frozen screen the session takes");
    session.handle(event(at(900, 500), 10_140, Input::Press));
    session.handle(event(at(1200, 700), 10_300, Input::Move));
    assert_eq!(drawn(&session), None);
    assert_eq!(
        session.handle(event(at(1200, 700), 10_400, Input::Release)),
        []
    );
    // A drag after the double-click time counts.
    let effects = drag(&mut session, at(900, 500), at(1200, 700), 10_500);
    assert_eq!(copied(&effects), ["100 x 40"]);
}

#[test]
fn switching_tools_in_the_middle_of_the_drag_drops_it() {
    for switch in [Key::Digit1, Key::Digit3, Key::Digit4] {
        let mut session = open(vec![button()], at(900, 500));
        hold(&mut session, at(900, 500), at(1200, 700), 1000);
        session.handle(key(at(1200, 700), 1100, switch));
        assert_eq!(drawn(&session), None, "{switch:?}");
        assert_eq!(
            session.handle(event(at(1200, 700), 1200, Input::Release)),
            [],
            "{switch:?}"
        );
        assert_eq!(pinned(&session), [], "{switch:?}");
    }
}

#[test]
fn switching_to_bounds_in_the_middle_of_its_own_drag_keeps_it() {
    let mut session = open(vec![button()], at(900, 500));
    hold(&mut session, at(900, 500), at(1200, 700), 1000);
    session.handle(key(at(1200, 700), 1100, Key::Digit2));
    let effects = session.handle(event(at(1200, 700), 1200, Input::Release));
    assert_eq!(copied(&effects), ["100 x 40"]);
}

/// Where the core puts the toolbar on the 4K monitor at 175%, at its default place.
fn toolbar_layout() -> toolbar::Layout {
    toolbar::layout_on(
        AOC,
        Scale::from_dpi(168).expect("a real scale"),
        ToolbarPlace::default(),
    )
}

/// The middle of Inspect's button on the toolbar.
fn on_inspect() -> Point {
    let (_, place) = toolbar_layout()
        .buttons
        .into_iter()
        .find(|(button, _)| *button == ToolbarButton::Tool(Tool::Inspect))
        .expect("every button has a place");
    at(
        place.left + i32::try_from(place.width / 2).expect("a small button"),
        place.top + i32::try_from(place.height / 2).expect("a small button"),
    )
}

#[test]
fn a_drag_goes_on_over_the_toolbar_which_neither_pauses_it_nor_reacts() {
    // A button under the toolbar, which the photo has without it.
    let inspect = on_inspect();
    let under = monitor(
        AOC,
        168,
        &[(
            BUTTON_BLUE,
            [
                inspect.x - 50,
                inspect.y + 200,
                inspect.x + 49,
                inspect.y + 239,
            ],
        )],
    );
    let start = at(inspect.x - 100, inspect.y + 400);
    let mut session = open(vec![under], start);
    hold(&mut session, start, inspect, 1000);
    assert_eq!(
        drawn(&session).map(|drawn| drawn.rect),
        Some(rect(start.x, inspect.y, inspect.x, start.y))
    );
    assert_eq!(session.scene().cursor, Cursor::Cross);
    assert_eq!(session.scene().toolbar.hovered, None);
    // The wheel still changes the tolerance there, as anywhere during the drag.
    session.handle(event(inspect, 1100, Input::Wheel(NOTCH)));
    assert_eq!(
        session
            .scene()
            .tolerance_indicator
            .map(|shown| shown.tolerance.get()),
        Some(3)
    );
    // Let go over the toolbar, the drag pins and copies, and the button under the cursor isn't clicked.
    let effects = session.handle(event(inspect, 1200, Input::Release));
    assert_eq!(copied(&effects), ["51 x 40"]);
    assert_eq!(session.scene().toolbar.active, Tool::Bounds);
    // Once the button is up, the toolbar reacts again.
    session.handle(event(inspect, 1300, Input::Move));
    assert_eq!(session.scene().cursor, Cursor::Arrow);
}

#[test]
fn a_press_on_the_toolbar_starts_no_drag() {
    let inspect = on_inspect();
    let mut session = open(vec![button()], inspect);
    session.handle(event(inspect, 1000, Input::Press));
    session.handle(event(at(1200, 700), 1100, Input::Move));
    assert_eq!(drawn(&session), None);
}

const ALT: Held = Held {
    shift: false,
    ctrl: false,
    alt: true,
};

/// With Alt held from before the press: presses at `from`, drags to `to` and lets go there; what the release returns.
fn drag_holding_alt(
    session: &mut Session,
    from: Point,
    to: Point,
    milliseconds: u64,
) -> Vec<Effect> {
    session.handle(event(from, milliseconds, Input::Move));
    session.handle(event_holding(
        from,
        ALT,
        milliseconds + 5,
        Input::KeyDown(Key::Alt),
    ));
    session.handle(event_holding(from, ALT, milliseconds + 10, Input::Press));
    session.handle(event_holding(to, ALT, milliseconds + 20, Input::Move));
    session.handle(event_holding(to, ALT, milliseconds + 30, Input::Release))
}

/// What Bounds with Alt measures for the drawn rectangle from (900, 500) to (1200, 700) on the 4K monitor at 175%,
/// where 301 × 201 physical pixels are 172 × 114.9 logical pixels.
fn the_drawn_rectangle() -> Measurement {
    Measurement {
        monitor: 0,
        horizontal: None,
        vertical: None,
        content: Some(rect(900, 500, 1200, 700)),
        label: Label {
            physical: "301 × 201".to_owned(),
            logical: Some("172 × 114.9".to_owned()),
        },
    }
}

#[test]
fn with_alt_held_from_before_the_press_the_drag_measures_exactly_the_drawn_rectangle() {
    let mut session = open(vec![button()], at(900, 500));
    session.handle(event(at(900, 500), 1000, Input::Move));
    session.handle(event_holding(
        at(900, 500),
        ALT,
        1005,
        Input::KeyDown(Key::Alt),
    ));
    session.handle(event_holding(at(900, 500), ALT, 1010, Input::Press));
    session.handle(event_holding(at(1200, 700), ALT, 1020, Input::Move));
    // Only the drawn rectangle shows, already in the result's style: as the live measurement, with no dashes.
    assert_eq!(session.scene().live, Some(&the_drawn_rectangle()));
    assert_eq!(drawn(&session), None);
    // Letting go with Alt pins the drawn rectangle and copies its size.
    let effects = session.handle(event_holding(at(1200, 700), ALT, 1030, Input::Release));
    assert_eq!(copied(&effects), ["301 x 201"]);
    assert_eq!(pinned(&session), [the_drawn_rectangle()]);
    assert_eq!(
        session.scene().notice.map(|notice| notice.message),
        Some(Message::Copied)
    );
}

#[test]
fn with_alt_an_area_of_nothing_but_the_reference_color_is_measured_like_any_other() {
    let mut session = open(vec![button()], at(100, 100));
    let effects = drag_holding_alt(&mut session, at(100, 100), at(400, 300), 1000);
    assert_eq!(copied(&effects), ["301 x 201"]);
    assert_eq!(
        pinned(&session)
            .iter()
            .map(|pin| pin.content)
            .collect::<Vec<_>>(),
        [Some(rect(100, 100, 400, 300))]
    );
    assert_eq!(
        session.scene().notice.map(|notice| notice.message),
        Some(Message::Copied)
    );
}

#[test]
fn with_alt_a_gradient_is_measured_as_drawn_where_the_shrink_would_stop_on_its_first_step() {
    // A gradient across the monitor's columns from 800 on, a level darker every four pixels. Begun at column 810, level
    // 253, the first step more than 6 levels off it is level 246, from column 836: without Alt, the rectangle shrinks to
    // it, 836 to 1100.
    let steps: Vec<([u8; 3], [i32; 4])> = (0..100u8)
        .map(|step| {
            let level = 255 - step;
            let left = 800 + 4 * i32::from(step);
            ([level, level, level], [left, 0, left + 3, 1000])
        })
        .collect();
    let gradient = monitor(AOC, 168, &steps);
    let mut session = open(vec![gradient.clone()], at(810, 500));
    let shrunk = drag(&mut session, at(810, 500), at(1100, 700), 1000);
    assert_eq!(copied(&shrunk), ["265 x 201"]);
    let mut session = open(vec![gradient], at(810, 500));
    let exact = drag_holding_alt(&mut session, at(810, 500), at(1100, 700), 1000);
    assert_eq!(copied(&exact), ["291 x 201"]);
}

#[test]
fn pressing_and_letting_go_of_alt_in_the_middle_of_the_drag_changes_the_preview_at_once() {
    let mut session = open(vec![button()], at(900, 500));
    hold(&mut session, at(900, 500), at(1200, 700), 1000);
    assert_eq!(session.scene().live, Some(&the_button()));
    session.handle(event_holding(
        at(1200, 700),
        ALT,
        1100,
        Input::KeyDown(Key::Alt),
    ));
    assert_eq!(session.scene().live, Some(&the_drawn_rectangle()));
    assert_eq!(drawn(&session), None);
    // Held down, Alt repeats, which changes nothing.
    session.handle(event_holding(
        at(1200, 700),
        ALT,
        1130,
        Input::KeyDown(Key::Alt),
    ));
    assert_eq!(session.scene().live, Some(&the_drawn_rectangle()));
    session.handle(event(at(1200, 700), 1200, Input::KeyUp(Key::Alt)));
    assert_eq!(session.scene().live, Some(&the_button()));
    assert_eq!(
        drawn(&session).map(|drawn| drawn.rect),
        Some(rect(900, 500, 1200, 700))
    );
}

#[test]
fn alt_counts_as_it_is_when_the_button_comes_up() {
    // Pressed in the middle of the drag and still held at the release: the drawn rectangle.
    let mut session = open(vec![button()], at(900, 500));
    hold(&mut session, at(900, 500), at(1200, 700), 1000);
    session.handle(event_holding(
        at(1200, 700),
        ALT,
        1100,
        Input::KeyDown(Key::Alt),
    ));
    let effects = session.handle(event_holding(at(1200, 700), ALT, 1200, Input::Release));
    assert_eq!(copied(&effects), ["301 x 201"]);
    // Held from before the press and let go before the release: the content.
    let mut session = open(vec![button()], at(900, 500));
    session.handle(event_holding(
        at(900, 500),
        ALT,
        1000,
        Input::KeyDown(Key::Alt),
    ));
    session.handle(event_holding(at(900, 500), ALT, 1010, Input::Press));
    session.handle(event_holding(at(1200, 700), ALT, 1020, Input::Move));
    session.handle(event(at(1200, 700), 1100, Input::KeyUp(Key::Alt)));
    let effects = session.handle(event(at(1200, 700), 1200, Input::Release));
    assert_eq!(copied(&effects), ["100 x 40"]);
    assert_eq!(pinned(&session), [the_button()]);
}

#[test]
fn alt_and_shift_held_as_the_button_comes_up_copy_the_drawn_rectangles_logical_pixels() {
    let mut session = open(vec![button()], at(900, 500));
    hold(&mut session, at(900, 500), at(1200, 700), 1000);
    let effects = session.handle(event_holding(
        at(1200, 700),
        Held {
            shift: true,
            alt: true,
            ..Held::default()
        },
        1100,
        Input::Release,
    ));
    assert_eq!(copied(&effects), ["172 x 114.9"]);
    assert_eq!(pinned(&session), [the_drawn_rectangle()]);
}

/// Presses at `from`, moves through each of `path` and lets go at the last, with `held` down the whole time; what the
/// release returns.
fn drag_through(
    session: &mut Session,
    from: Point,
    path: &[Point],
    held: Held,
    milliseconds: u64,
) -> Vec<Effect> {
    session.handle(event_holding(from, held, milliseconds, Input::Move));
    session.handle(event_holding(from, held, milliseconds + 10, Input::Press));
    let mut last = from;
    for (step, &to) in (1..).zip(path) {
        session.handle(event_holding(
            to,
            held,
            milliseconds + 10 + step * 10,
            Input::Move,
        ));
        last = to;
    }
    session.handle(event_holding(
        last,
        held,
        milliseconds + 500,
        Input::Release,
    ))
}

#[test]
fn a_click_or_a_tremble_does_nothing_with_or_without_alt_and_shows_no_notice() {
    // Begun on the white three pixels up and left of the button, a tremble of up to 4 pixels each way reaches into its
    // corner: without the limit, it would pin 2 × 2 of it, or the 5 × 5 drawn with Alt.
    for held in [Held::default(), ALT] {
        for path in [
            vec![],
            vec![at(997, 597)],
            vec![at(1001, 601), at(999, 599), at(1000, 600)],
        ] {
            let mut session = open(vec![button()], at(997, 597));
            let effects = drag_through(&mut session, at(997, 597), &path, held, 1000);
            assert_eq!(effects, [], "{held:?} {path:?}");
            assert_eq!(pinned(&session), [], "{held:?} {path:?}");
            assert_eq!(session.scene().notice, None, "{held:?} {path:?}");
        }
    }
}

#[test]
fn a_drag_counts_once_the_cursor_goes_more_than_4_pixels_from_the_press_along_either_axis() {
    // 4 pixels right and 4 down of the press is still a tremble.
    let mut session = open(vec![button()], at(997, 597));
    assert_eq!(
        drag_through(
            &mut session,
            at(997, 597),
            &[at(1001, 601)],
            Held::default(),
            1000
        ),
        []
    );
    // 5 pixels along one axis counts, whichever way.
    for (to, size) in [
        (at(1002, 601), "3 x 2"),
        (at(1001, 602), "2 x 3"),
        (at(992, 597), "No content found"),
        (at(997, 592), "No content found"),
    ] {
        let mut session = open(vec![button()], at(997, 597));
        let effects = drag_through(&mut session, at(997, 597), &[to], Held::default(), 1000);
        let shown = copied(&effects)
            .first()
            .map(|copied| (*copied).to_owned())
            .or_else(|| {
                session
                    .scene()
                    .notice
                    .map(|notice| notice.message.text().to_owned())
            });
        assert_eq!(shown.as_deref(), Some(size), "{to:?}");
    }
}

#[test]
fn the_limit_is_4_physical_pixels_at_every_scale() {
    // On the Full HD monitor at 125%, as on the 4K at 175%, as Windows' own drag distance is.
    for (moved, counts) in [(4, false), (5, true)] {
        let mut session = open(across_two_monitors(), at(3855, 595));
        let effects = drag_through(
            &mut session,
            at(3855, 595),
            &[at(3855 + moved, 595)],
            ALT,
            1000,
        );
        assert_eq!(!copied(&effects).is_empty(), counts, "{moved}");
    }
}

#[test]
fn a_drag_that_went_past_the_limit_counts_even_when_it_comes_back_near_the_press() {
    let mut session = open(vec![button()], at(997, 597));
    let effects = drag_through(
        &mut session,
        at(997, 597),
        &[at(1200, 700), at(1001, 601)],
        Held::default(),
        1000,
    );
    assert_eq!(copied(&effects), ["2 x 2"]);
}

#[test]
fn the_arrows_with_the_button_held_count_as_the_mouse_going_that_far() {
    let mut session = open(vec![button()], at(997, 597));
    session.handle(event(at(997, 597), 1000, Input::Move));
    session.handle(event(at(997, 597), 1010, Input::Press));
    for step in 0..5 {
        session.handle(key(
            at(997 + step, 597),
            1100 + u64::try_from(step).expect("a small step"),
            Key::Right,
        ));
    }
    let effects = session.handle(event_holding(at(1002, 597), ALT, 1200, Input::Release));
    assert_eq!(copied(&effects), ["6 x 1"]);
}

#[test]
fn until_the_drag_counts_the_preview_shows_nothing() {
    for held in [Held::default(), ALT] {
        let mut session = open(vec![button()], at(997, 597));
        session.handle(event_holding(at(997, 597), held, 1000, Input::Move));
        session.handle(event_holding(at(997, 597), held, 1010, Input::Press));
        session.handle(event_holding(at(1001, 601), held, 1020, Input::Move));
        assert_eq!(drawn(&session), None, "{held:?}");
        assert_eq!(session.scene().live, None, "{held:?}");
        // One pixel further, the drawn rectangle shows, or with Alt the measurement in its place.
        session.handle(event_holding(at(1002, 601), held, 1030, Input::Move));
        let shown = drawn(&session)
            .map(|drawn| drawn.rect)
            .or_else(|| live_content(&session));
        assert_eq!(shown, Some(rect(997, 597, 1002, 601)), "{held:?}");
    }
}

#[test]
fn a_thin_drag_along_one_axis_counts() {
    // A gray line one pixel tall along row 800; the drag begins on the white just above it and draws 200 × 2 around it.
    let line = monitor(AOC, 168, &[(ICON_GRAY, [950, 800, 1250, 800])]);
    let mut session = open(vec![line], at(1000, 799));
    let effects = drag(&mut session, at(1000, 799), at(1199, 800), 1000);
    assert_eq!(copied(&effects), ["200 x 1"]);
    // With Alt, it is the 200 × 2 drawn.
    let line = monitor(AOC, 168, &[(ICON_GRAY, [950, 800, 1250, 800])]);
    let mut session = open(vec![line], at(1000, 799));
    let effects = drag_holding_alt(&mut session, at(1000, 799), at(1199, 800), 1000);
    assert_eq!(copied(&effects), ["200 x 2"]);
}

#[test]
fn esc_with_the_button_held_cancels_a_drag_that_doesnt_count_yet() {
    let mut session = open(vec![button()], at(997, 597));
    session.handle(event(at(997, 597), 1000, Input::Move));
    session.handle(event(at(997, 597), 1010, Input::Press));
    assert_eq!(session.handle(key(at(997, 597), 1100, Key::Esc)), []);
    session.handle(event(at(1200, 700), 1200, Input::Move));
    assert_eq!(drawn(&session), None);
    assert_eq!(
        session.handle(event(at(1200, 700), 1300, Input::Release)),
        []
    );
}

#[test]
fn esc_with_the_button_held_cancels_only_the_drag() {
    let mut session = open(vec![button()], at(900, 500));
    drag(&mut session, at(900, 500), at(1200, 700), 1000);
    hold(&mut session, at(900, 500), at(1200, 700), 2000);
    // The tools stay open, the pin stays, and the drawn rectangle and its preview go.
    assert_eq!(session.handle(key(at(1200, 700), 2100, Key::Esc)), []);
    assert_eq!(drawn(&session), None);
    assert_eq!(session.scene().live, None);
    assert_eq!(pinned(&session), [the_button()]);
    // Moving on with the button still held draws nothing, and letting it go pins and copies nothing.
    session.handle(event(at(1150, 680), 2200, Input::Move));
    assert_eq!(drawn(&session), None);
    assert_eq!(session.scene().live, None);
    assert_eq!(
        session.handle(event(at(1150, 680), 2300, Input::Release)),
        []
    );
    assert_eq!(pinned(&session), [the_button()]);
    assert_eq!(session.scene().toolbar.active, Tool::Bounds);
    // The next drag works as ever.
    let effects = drag(&mut session, at(900, 500), at(1200, 700), 3000);
    assert_eq!(copied(&effects), ["100 x 40"]);
    assert_eq!(pinned(&session), [the_button(), the_button()]);
}

#[test]
fn esc_without_a_drag_closes_the_tools_and_takes_the_pins_away() {
    let mut session = open(vec![button()], at(900, 500));
    drag(&mut session, at(900, 500), at(1200, 700), 1000);
    let effects = session.handle(key(at(1200, 700), 2000, Key::Esc));
    assert_eq!(effects.first(), Some(&Effect::Close));
    assert_eq!(pinned(&session), []);
}

#[test]
fn esc_with_the_button_held_in_dimensions_closes_the_tools() {
    // Dimensions has no drag: its press pins at once, so Esc with the button still down closes as ever.
    let mut session = open_as(Tool::Dimensions, vec![button()], at(1050, 620));
    session.handle(event(at(1050, 620), 1000, Input::Move));
    session.handle(event(at(1050, 620), 1010, Input::Press));
    let effects = session.handle(key(at(1050, 620), 1100, Key::Esc));
    assert_eq!(effects.first(), Some(&Effect::Close));
}

#[test]
fn closing_another_way_in_the_middle_of_the_drag_takes_it_away_with_the_pins() {
    for close in [
        Input::GlobalShortcut,
        Input::Toolbar(ToolbarButton::Close),
        Input::CloseAsked,
        Input::OtherProgramInFront,
        Input::MonitorsChanged,
    ] {
        let mut session = open(vec![button()], at(900, 500));
        drag(&mut session, at(900, 500), at(1200, 700), 1000);
        hold(&mut session, at(900, 500), at(1200, 700), 2000);
        let effects = session.handle(event(at(1200, 700), 2100, close.clone()));
        assert_eq!(effects.first(), Some(&Effect::Close), "{close:?}");
        assert_eq!(drawn(&session), None, "{close:?}");
        assert_eq!(pinned(&session), [], "{close:?}");
        assert_eq!(
            session.handle(event(at(1200, 700), 2200, Input::Release)),
            [],
            "{close:?}"
        );
    }
}
