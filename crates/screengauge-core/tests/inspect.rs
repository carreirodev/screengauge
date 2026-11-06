//! Layer 1 tests of Inspect's magnified view and magnification (v1 spec WPC-296, Inspect; ticket WPC-315): the square
//! of pixels around the cursor and its size at each magnification and scale, the panel below and to the right of the
//! cursor that changes side near the monitor's limits and never leaves the current monitor, the part past that limit
//! left empty, the magnification the mouse wheel changes and the magnification indicator that says it, the cursor's
//! position on the readout, and the keys. They look only at the scene the session would draw and at the effects it
//! returns, on frozen screens made up here, without a screen.

use std::time::Duration;

use screengauge_core::session::toolbar;
use screengauge_core::session::{
    Colors, Effect, Event, Held, Image, Input, Key, LocalTime, MagnifiedView, Opening, Photo,
    Point, Rect, Scale, Session, ToolbarButton,
};
use screengauge_core::settings::{
    DimensionsMode, Magnification, Options, Remembered, Tool, ToolbarPlace,
};

/// A Full HD monitor at 100%, Windows' main monitor.
const FULL_HD: Rect = Rect {
    left: 0,
    top: 0,
    width: 1920,
    height: 1080,
};

/// Where the cursor is when a test doesn't say: far from every side of the Full HD monitor and below the toolbar.
const MIDDLE: Point = Point { x: 900, y: 600 };

/// A white monitor at `rect` and `dpi` (96 is 100%).
fn monitor(rect: Rect, dpi: u32) -> Photo {
    let width = usize::try_from(rect.width).expect("a small monitor");
    let height = usize::try_from(rect.height).expect("a small monitor");
    Photo {
        rect,
        scale: Scale::from_dpi(dpi).expect("a real scale"),
        colors: Colors::Exact,
        image: Image::new(rect.width, rect.height, [255; 4].repeat(width * height))
            .expect("the pixels fill the image"),
    }
}

/// Inspect on `monitors`, with the cursor at `cursor` and `remembered` otherwise.
fn open_on(monitors: Vec<Photo>, cursor: Point, remembered: Remembered) -> Session {
    Session::open(Opening {
        frozen_screen: monitors.into_iter().map(Some).collect(),
        cursor,
        options: Options::default(),
        remembered: Remembered {
            last_tool: Tool::Inspect,
            ..remembered
        },
        second_click_until: None,
    })
    .expect("a frozen screen the session takes")
}

/// Inspect on the Full HD monitor with the cursor at `cursor`, for the first time.
fn open_at(cursor: Point) -> Session {
    open_on(vec![monitor(FULL_HD, 96)], cursor, Remembered::default())
}

/// The magnified view the session shows now.
fn view(session: &Session) -> MagnifiedView {
    session
        .scene()
        .magnified_view
        .expect("Inspect shows its magnified view")
}

fn square(left: i32, top: i32, side: u32) -> Rect {
    Rect {
        left,
        top,
        width: side,
        height: side,
    }
}

#[test]
fn opened_for_the_first_time_inspect_shows_21_pixels_a_side_around_the_cursor_at_8x() {
    let view = view(&open_at(MIDDLE));
    assert_eq!(view.monitor, 0);
    assert_eq!(view.center, MIDDLE);
    assert_eq!(view.magnification, Magnification::X8);
    // 176 logical pixels at 100% hold 22 pixels at 8×, and the odd number below it puts the cursor's pixel in the
    // middle: 10 on each side of it, 168 physical pixels a side.
    assert_eq!(view.source, square(890, 590, 21));
    assert_eq!(view.shown, view.source);
    assert_eq!(
        (view.enlargement.width, view.enlargement.height),
        (168, 168)
    );
    assert!(view.grid);
    assert!(
        !view.magnification_indicator,
        "the magnification indicator shows only once the wheel turns"
    );
}

/// `input` with the cursor at `cursor`, no modifier held, `milliseconds` after Windows started.
fn event(cursor: Point, milliseconds: u64, input: Input) -> Event {
    Event {
        cursor,
        held: Held::default(),
        time: LocalTime {
            year: 2026,
            month: 11,
            day: 1,
            hour: 18,
            minute: 0,
            second: 0,
        },
        uptime: Duration::from_millis(milliseconds),
        input,
    }
}

/// Every magnification, from 2× to 32×.
const MAGNIFICATIONS: [Magnification; 5] = [
    Magnification::X2,
    Magnification::X4,
    Magnification::X8,
    Magnification::X16,
    Magnification::X32,
];

/// Inspect on a Full HD monitor at `dpi`, opened at the remembered `magnification`, with the cursor at `cursor`.
fn open_with(dpi: u32, magnification: Magnification, cursor: Point) -> Session {
    open_on(
        vec![monitor(FULL_HD, dpi)],
        cursor,
        Remembered {
            magnification,
            ..Remembered::default()
        },
    )
}

#[test]
fn the_view_holds_the_most_whole_pixels_that_fit_176_logical_pixels_an_odd_number_of_them() {
    // For each scale, the pixels a side and the enlargement's side in physical pixels at 2×, 4×, 8×, 16× and 32×,
    // worked out by hand from 176 logical pixels: at 150% they are 264 physical pixels, which hold 132 pixels at 2×,
    // made 131 to be odd, and 8.25 at 32×, so 7.
    let expected: [(u32, [(u32, u32); 5]); 5] = [
        (96, [(87, 174), (43, 172), (21, 168), (11, 176), (5, 160)]),
        (120, [(109, 218), (55, 220), (27, 216), (13, 208), (5, 160)]),
        (144, [(131, 262), (65, 260), (33, 264), (15, 240), (7, 224)]),
        (168, [(153, 306), (77, 308), (37, 296), (19, 304), (9, 288)]),
        (
            192,
            [(175, 350), (87, 348), (43, 344), (21, 336), (11, 352)],
        ),
    ];
    for (dpi, sizes) in expected {
        for (magnification, (across, side)) in MAGNIFICATIONS.into_iter().zip(sizes) {
            let view = view(&open_with(dpi, magnification, MIDDLE));
            let half = i32::try_from(across / 2).expect("a few pixels");
            assert_eq!(
                view.source,
                square(MIDDLE.x - half, MIDDLE.y - half, across),
                "{dpi} dpi, {magnification:?}"
            );
            assert_eq!(
                (view.enlargement.width, view.enlargement.height),
                (side, side),
                "{dpi} dpi, {magnification:?}"
            );
        }
    }
}

#[test]
fn the_grid_parts_the_pixels_from_8x_up() {
    let grids =
        MAGNIFICATIONS.map(|magnification| view(&open_with(96, magnification, MIDDLE)).grid);
    assert_eq!(grids, [false, false, true, true, true]);
}

#[test]
fn the_panel_is_22_logical_pixels_below_and_right_of_the_cursor_around_the_enlargement_and_the_readout()
 {
    // At 100%: a border of 1 and a padding of 6 around the enlargement of 168, which the readout follows: 7 above its
    // line of 16 (12px text at a line height of 1.35, 16.2), 5 above the color's row, as tall as HEX above RGB in Color
    // format's default, Both, two lines of 16, and 2 below it.
    let view = view(&open_at(MIDDLE));
    assert_eq!(
        view.panel,
        Rect {
            left: 922,
            top: 622,
            width: 7 + 168 + 7,
            height: 7 + 168 + 7 + 16 + 5 + 2 * 16 + 2 + 7,
        }
    );
    assert_eq!(view.enlargement, square(929, 629, 168));
}

#[test]
fn at_150_percent_every_size_of_the_panel_is_scaled_on_its_own() {
    // Each size halves up on its own: a border of 2 (1.5) and a padding of 9, 33 above and right of the cursor, and the
    // readout's 11 above its two lines of 24 (24.3), 8 (7.5) above the color's two lines of 24, and 3 below them. The
    // logical pixels show, so X and Y take a line each.
    let view = view(&open_with(144, Magnification::X8, MIDDLE));
    assert_eq!(
        view.panel,
        Rect {
            left: 933,
            top: 633,
            width: 11 + 264 + 11,
            height: 11 + 264 + 11 + 2 * 24 + 8 + 2 * 24 + 3 + 11,
        }
    );
    assert_eq!(view.enlargement, square(944, 644, 264));
}

/// Where the panel's top left corner is.
fn corner(view: &MagnifiedView) -> Point {
    Point {
        x: view.panel.left,
        y: view.panel.top,
    }
}

fn at(x: i32, y: i32) -> Point {
    Point { x, y }
}

// At 100% and 8× the panel is 182 wide and 244 tall, and starts 22 right of and below the cursor.

#[test]
fn near_the_monitors_right_side_the_panel_goes_left_of_the_cursor_and_stays_below_it() {
    // At 1712 the panel ends 4 short of the monitor's right side, as near as it comes; a pixel further it would come
    // nearer, so it goes left of the cursor, its right side 22 before it.
    assert_eq!(corner(&view(&open_at(at(1712, 600)))), at(1734, 622));
    assert_eq!(
        corner(&view(&open_at(at(1713, 600)))),
        at(1713 - 22 - 182, 622)
    );
}

#[test]
fn near_the_monitors_bottom_the_panel_goes_above_the_cursor_and_stays_right_of_it() {
    // At 810 the panel ends 4 short of the monitor's bottom, as near as it comes; a pixel further it goes above.
    assert_eq!(corner(&view(&open_at(at(900, 810)))), at(922, 832));
    assert_eq!(
        corner(&view(&open_at(at(900, 811)))),
        at(922, 811 - 22 - 244)
    );
}

#[test]
fn in_the_bottom_right_corner_the_panel_goes_above_and_left_of_the_cursor() {
    assert_eq!(
        corner(&view(&open_at(at(1900, 1070)))),
        at(1900 - 22 - 182, 1070 - 22 - 244)
    );
}

#[test]
fn near_the_monitors_left_side_and_top_the_panel_stays_below_and_right_of_the_cursor() {
    assert_eq!(corner(&view(&open_at(at(0, 0)))), at(22, 22));
}

/// A monitor 300 pixels wide and 400 tall at 100%, right of the Full HD one: too narrow and too short for the panel on
/// either side of the cursor in its middle.
const NARROW: Rect = Rect {
    left: 1920,
    top: 0,
    width: 300,
    height: 400,
};

/// The cursor moved to `to`, `milliseconds` after Windows started.
fn move_to(to: Point, milliseconds: u64) -> Event {
    event(to, milliseconds, Input::Move)
}

#[test]
fn on_a_monitor_too_small_for_either_side_the_panel_stays_2_inside_its_left_side_and_top() {
    // Opened on the Full HD monitor, where the toolbar goes, and moved to the narrow one.
    let mut session = open_on(
        vec![monitor(FULL_HD, 96), monitor(NARROW, 96)],
        MIDDLE,
        Remembered::default(),
    );
    session.handle(move_to(at(2070, 200), 1000));
    let view = view(&session);
    assert_eq!(view.monitor, 1);
    assert_eq!(corner(&view), at(1922, 2));
}

#[test]
fn the_panel_never_leaves_the_current_monitor() {
    // Every 9th pixel of both monitors, at every magnification: the panel is on the cursor's monitor, 2 or more inside
    // each of its sides, as both monitors have room for it.
    for magnification in MAGNIFICATIONS {
        let mut session = open_on(
            vec![monitor(FULL_HD, 96), monitor(NARROW, 96)],
            MIDDLE,
            Remembered {
                magnification,
                ..Remembered::default()
            },
        );
        let mut milliseconds = 1000;
        for (index, rect) in [FULL_HD, NARROW].into_iter().enumerate() {
            let (right, bottom) = (
                rect.left + rect.width.cast_signed(),
                rect.top + rect.height.cast_signed(),
            );
            for y in (rect.top..bottom).step_by(9) {
                for x in (rect.left..right).step_by(9) {
                    milliseconds += 1;
                    session.handle(move_to(at(x, y), milliseconds));
                    let view = view(&session);
                    if view.center != at(x, y) {
                        // On the toolbar, where Inspect pauses with the view where the cursor last was off it.
                        continue;
                    }
                    let panel = view.panel;
                    assert!(
                        view.monitor == index
                            && panel.left >= rect.left + 2
                            && panel.top >= rect.top + 2
                            && panel.left + panel.width.cast_signed() <= right - 2
                            && panel.top + panel.height.cast_signed() <= bottom - 2,
                        "{magnification:?} at ({x}, {y}): {panel:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn in_the_monitors_corner_only_the_pixels_on_it_are_shown() {
    // The cursor on the top left pixel: of the 21 × 21 around it, the 11 × 11 from it right and down are on the
    // monitor, and the rest of the enlargement stays empty.
    let view = view(&open_at(at(0, 0)));
    assert_eq!(view.source, square(-10, -10, 21));
    assert_eq!(view.shown, square(0, 0, 11));
    let view = self::view(&open_at(at(1919, 1079)));
    assert_eq!(view.shown, square(1909, 1069, 11));
}

/// A Full HD monitor at 100% right of the main one, touching it.
const BESIDE: Rect = Rect {
    left: 1920,
    top: 0,
    width: 1920,
    height: 1080,
};

#[test]
fn at_the_limit_between_two_monitors_the_view_shows_no_pixel_of_the_other_one() {
    let mut session = open_on(
        vec![monitor(FULL_HD, 96), monitor(BESIDE, 96)],
        at(1919, 600),
        Remembered::default(),
    );
    let view = view(&session);
    assert_eq!(
        (view.monitor, view.shown),
        (
            0,
            Rect {
                left: 1909,
                top: 590,
                width: 11,
                height: 21
            }
        )
    );
    // A pixel right, the cursor is on the other monitor, and so is the view, with nothing of the first one.
    session.handle(move_to(at(1920, 600), 1000));
    let view = self::view(&session);
    assert_eq!(
        (view.monitor, view.shown),
        (
            1,
            Rect {
                left: 1920,
                top: 590,
                width: 11,
                height: 21
            }
        )
    );
    assert!(view.panel.left >= 1920 + 2, "{:?}", view.panel);
}

/// A 4K monitor at 150% left of the main one and higher, so its coordinates are below zero.
const LEFT_4K: Rect = Rect {
    left: -3840,
    top: -500,
    width: 3840,
    height: 2160,
};

/// What a coordinate on the readout reads: its physical pixels, and its logical pixels when they show.
fn reads(coordinate: &screengauge_core::session::Coordinate) -> (&str, Option<&str>) {
    (
        coordinate.label.physical.as_str(),
        coordinate.label.logical.as_deref(),
    )
}

#[test]
fn the_position_counts_from_the_current_monitors_top_left_corner_and_is_never_below_zero() {
    let mut session = open_on(
        vec![monitor(FULL_HD, 96), monitor(LEFT_4K, 144)],
        MIDDLE,
        Remembered::default(),
    );
    // On the main monitor at 100%, the virtual screen's own numbers, with no logical pixels.
    let view = view(&session);
    assert_eq!(
        (reads(&view.x), reads(&view.y)),
        (("900", None), ("600", None))
    );
    // The 4K monitor's top left pixel is (0, 0), and its last one (3839, 2159), 2559.3 and 1439.3 logical pixels at
    // 150%.
    session.handle(move_to(at(-3840, -500), 1000));
    let view = self::view(&session);
    assert_eq!(
        (reads(&view.x), reads(&view.y)),
        (("0", Some("0")), ("0", Some("0")))
    );
    session.handle(move_to(at(-1, 1659), 2000));
    let view = self::view(&session);
    assert_eq!(
        (reads(&view.x), reads(&view.y)),
        (("3839", Some("2559.3")), ("2159", Some("1439.3")))
    );
}

#[test]
fn with_also_show_logical_pixels_off_the_position_has_no_logical_part() {
    let session = Session::open(Opening {
        frozen_screen: vec![Some(monitor(FULL_HD, 144))],
        cursor: MIDDLE,
        options: Options {
            also_show_logical_pixels: false,
            ..Options::default()
        },
        remembered: Remembered {
            last_tool: Tool::Inspect,
            ..Remembered::default()
        },
        second_click_until: None,
    })
    .expect("a frozen screen the session takes");
    let view = view(&session);
    assert_eq!(
        (reads(&view.x), reads(&view.y)),
        (("900", None), ("600", None))
    );
}

#[test]
fn without_logical_pixels_x_and_y_share_a_line_in_two_columns() {
    // At 100% the readout's text is 168 less 4 on each side, 160 wide: two columns of 76, 8 apart.
    let view = view(&open_at(MIDDLE));
    assert_eq!(
        view.x.line,
        Rect {
            left: 933,
            top: 629 + 168 + 7,
            width: 76,
            height: 16
        }
    );
    assert_eq!(
        view.y.line,
        Rect {
            left: 933 + 76 + 8,
            ..view.x.line
        }
    );
}

#[test]
fn with_logical_pixels_x_and_y_take_a_line_each_x_above() {
    // At 150% the readout's text is 264 less 6 on each side, 252 wide, on lines of 24 (24.3) below the enlargement's 11
    // above.
    let view = view(&open_with(144, Magnification::X8, MIDDLE));
    assert_eq!(
        view.x.line,
        Rect {
            left: 950,
            top: 644 + 264 + 11,
            width: 252,
            height: 24
        }
    );
    assert_eq!(
        view.y.line,
        Rect {
            top: 644 + 264 + 11 + 24,
            ..view.x.line
        }
    );
}

/// One notch of the mouse wheel, in Windows' units, turned forward: away from the user.
const NOTCH: i32 = 120;

/// The wheel turned by `delta` with the cursor in the middle of the Full HD monitor, `milliseconds` after Windows
/// started.
fn wheel(delta: i32, milliseconds: u64) -> Event {
    event(MIDDLE, milliseconds, Input::Wheel(delta))
}

fn magnification(session: &Session) -> Magnification {
    view(session).magnification
}

#[test]
fn a_notch_forward_raises_the_magnification_a_level_and_a_notch_back_lowers_it() {
    let mut session = open_at(MIDDLE);
    session.handle(wheel(NOTCH, 1000));
    assert_eq!(magnification(&session), Magnification::X16);
    session.handle(wheel(-NOTCH, 2000));
    session.handle(wheel(-NOTCH, 3000));
    assert_eq!(magnification(&session), Magnification::X4);
    // The view holds the pixels that fit at the new level: 43 a side at 4×.
    assert_eq!(view(&session).source.width, 43);
}

#[test]
fn each_notch_of_a_fast_turn_is_a_level_and_parts_of_a_notch_add_up() {
    let mut session = open_at(MIDDLE);
    session.handle(wheel(2 * NOTCH, 1000));
    assert_eq!(magnification(&session), Magnification::X32);
    // A precision touchpad turns half a notch at a time.
    session.handle(wheel(-NOTCH / 2, 2000));
    assert_eq!(magnification(&session), Magnification::X32);
    session.handle(wheel(-NOTCH / 2, 2100));
    assert_eq!(magnification(&session), Magnification::X16);
}

#[test]
fn turning_past_a_limit_does_nothing_and_the_first_notch_back_is_a_level() {
    let mut session = open_with(96, Magnification::X32, MIDDLE);
    session.handle(wheel(3 * NOTCH, 1000));
    assert_eq!(magnification(&session), Magnification::X32);
    session.handle(wheel(-NOTCH, 2000));
    assert_eq!(magnification(&session), Magnification::X16);

    let mut session = open_with(96, Magnification::X2, MIDDLE);
    session.handle(wheel(-NOTCH, 1000));
    assert_eq!(magnification(&session), Magnification::X2);
    session.handle(wheel(NOTCH, 2000));
    assert_eq!(magnification(&session), Magnification::X4);
}

fn magnification_indicator(session: &Session) -> bool {
    view(session).magnification_indicator
}

#[test]
fn the_magnification_indicator_shows_when_the_wheel_turns_and_goes_a_second_after_it_stops() {
    let mut session = open_at(MIDDLE);
    let effects = session.handle(wheel(NOTCH, 1000));
    assert!(magnification_indicator(&session));
    assert_eq!(effects, [Effect::WakeAt(Duration::from_millis(2000))]);
    // A timer that comes a little early leaves it, and asks again for the moment it goes.
    let effects = session.handle(event(MIDDLE, 1990, Input::Wake));
    assert!(magnification_indicator(&session));
    assert_eq!(effects, [Effect::WakeAt(Duration::from_millis(2000))]);
    let effects = session.handle(event(MIDDLE, 2000, Input::Wake));
    assert!(!magnification_indicator(&session));
    assert_eq!(effects, []);
}

#[test]
fn another_notch_keeps_the_magnification_indicator_a_second_more() {
    let mut session = open_at(MIDDLE);
    session.handle(wheel(NOTCH, 1000));
    let effects = session.handle(wheel(NOTCH, 1600));
    assert_eq!(effects, [Effect::WakeAt(Duration::from_millis(2600))]);
    session.handle(event(MIDDLE, 2000, Input::Wake));
    assert!(magnification_indicator(&session));
}

#[test]
fn turning_past_a_limit_shows_the_magnification_indicator_all_the_same() {
    // The owner's choice in WPC-315: the magnification indicator tells the magnification is already at its highest.
    let mut session = open_with(96, Magnification::X32, MIDDLE);
    let effects = session.handle(wheel(NOTCH, 1000));
    assert!(magnification_indicator(&session));
    assert_eq!(effects, [Effect::WakeAt(Duration::from_millis(2000))]);
}

#[test]
fn the_magnification_indicator_goes_with_whatever_event_comes_after_its_second() {
    // Should the wake be lost, the next move takes the magnification indicator away.
    let mut session = open_at(MIDDLE);
    session.handle(wheel(NOTCH, 1000));
    session.handle(move_to(at(901, 600), 2500));
    assert!(!magnification_indicator(&session));
}

/// The effects of closing the tools with Esc, `milliseconds` after Windows started.
fn close(session: &mut Session, milliseconds: u64) -> Vec<Effect> {
    session.handle(event(MIDDLE, milliseconds, Input::KeyDown(Key::Esc)))
}

#[test]
fn closing_keeps_the_magnification_the_wheel_left_for_the_next_call() {
    let mut session = open_at(MIDDLE);
    session.handle(wheel(NOTCH, 1000));
    let effects = close(&mut session, 2000);
    assert_eq!(
        effects,
        [
            Effect::Close,
            Effect::SaveRemembered(Remembered {
                last_tool: Tool::Inspect,
                magnification: Magnification::X16,
                ..Remembered::default()
            })
        ]
    );
}

#[test]
fn a_magnification_turned_and_turned_back_changes_nothing_to_keep() {
    let mut session = open_at(MIDDLE);
    session.handle(wheel(NOTCH, 1000));
    session.handle(wheel(-NOTCH, 1100));
    assert_eq!(close(&mut session, 2000), [Effect::Close]);
}

#[test]
fn the_wheel_on_the_toolbar_changes_no_magnification() {
    // Inspect pauses on the toolbar like every tool, and the wheel does nothing there (the owner's choice in WPC-311).
    let mut session = open_at(MIDDLE);
    let on_inspect = on(ToolbarButton::Tool(Tool::Inspect));
    session.handle(move_to(on_inspect, 1000));
    let effects = session.handle(event(on_inspect, 1100, Input::Wheel(NOTCH)));
    assert_eq!(effects, []);
    assert_eq!(magnification(&session), Magnification::X8);
    assert!(!magnification_indicator(&session));
}

/// Where the core puts the toolbar on the Full HD monitor the first time.
fn on(button: ToolbarButton) -> Point {
    let scale = Scale::from_dpi(96).expect("a real scale");
    let (_, place) = toolbar::layout_on(FULL_HD, scale, ToolbarPlace::default())
        .buttons
        .into_iter()
        .find(|(each, _)| *each == button)
        .expect("every button has a place");
    at(
        place.left + place.width.cast_signed() / 2,
        place.top + place.height.cast_signed() / 2,
    )
}

#[test]
fn on_the_toolbar_the_view_stays_where_the_cursor_left_it() {
    let mut session = open_at(MIDDLE);
    session.handle(move_to(at(950, 300), 1000));
    let before = view(&session);
    session.handle(move_to(on(ToolbarButton::Tool(Tool::Snip)), 1100));
    assert_eq!(view(&session), before);
}

#[test]
fn opened_with_the_cursor_on_the_toolbar_inspect_shows_no_view_until_it_leaves() {
    let on_close = on(ToolbarButton::Close);
    let mut session = open_at(on_close);
    assert_eq!(session.scene().magnified_view, None);
    session.handle(move_to(MIDDLE, 1000));
    assert_eq!(view(&session).center, MIDDLE);
}

/// `key` pressed with the cursor in the middle of the Full HD monitor, Shift held when `shift`.
fn key(key: Key, shift: bool, milliseconds: u64) -> Event {
    Event {
        held: Held {
            shift,
            ..Held::default()
        },
        ..event(MIDDLE, milliseconds, Input::KeyDown(key))
    }
}

#[test]
fn the_arrows_move_the_cursor_a_pixel_or_ten_with_shift_and_the_view_follows() {
    let mut session = open_at(MIDDLE);
    let effects = session.handle(key(Key::Right, false, 1000));
    assert_eq!(effects, [Effect::MoveCursor(at(901, 600))]);
    assert_eq!(view(&session).center, at(901, 600));
    assert_eq!(view(&session).source.left, 891);
    assert_eq!(reads(&view(&session).x), ("901", None));
    let effects = session.handle(key(Key::Down, true, 1100));
    assert_eq!(effects, [Effect::MoveCursor(at(901, 610))]);
    assert_eq!(view(&session).center, at(901, 610));
    assert_eq!(corner(&view(&session)), at(923, 632));
    session.handle(key(Key::Left, true, 1200));
    session.handle(key(Key::Up, false, 1300));
    assert_eq!(view(&session).center, at(891, 609));
}

/// A white Full HD monitor at 100% with a gray card from (300, 400) to (500, 499), for Dimensions to measure.
fn with_card() -> Photo {
    let mut photo = monitor(FULL_HD, 96);
    let mut bgra = photo.image.bgra().to_vec();
    for y in 400..500_usize {
        for x in 300..=500_usize {
            let at = (y * 1920 + x) * 4;
            bgra[at..at + 3].copy_from_slice(&[128, 128, 128]);
        }
    }
    photo.image = Image::new(1920, 1080, bgra).expect("the pixels fill the image");
    photo
}

#[test]
fn backspace_and_delete_do_nothing_in_inspect_and_the_pinned_measurements_stay() {
    // A width pinned in Dimensions, then Inspect with its key.
    let card = at(400, 450);
    let mut session = Session::open(Opening {
        frozen_screen: vec![Some(with_card())],
        cursor: card,
        options: Options::default(),
        remembered: Remembered {
            last_tool: Tool::Dimensions,
            dimensions_mode: DimensionsMode::Horizontal,
            ..Remembered::default()
        },
        second_click_until: None,
    })
    .expect("a frozen screen the session takes");
    session.handle(event(card, 1000, Input::Press));
    session.handle(event(card, 1050, Input::Release));
    session.handle(event(card, 1100, Input::KeyDown(Key::Digit3)));
    let pinned = session.scene().pinned.to_vec();
    assert_eq!(pinned.len(), 1);
    for (index, key) in [Key::Backspace, Key::Delete].into_iter().enumerate() {
        let effects = session.handle(event(card, 2000 + 100 * index as u64, Input::KeyDown(key)));
        assert_eq!(effects, []);
        assert_eq!(session.scene().pinned, pinned.as_slice(), "{key:?}");
    }
    assert_eq!(view(&session).center, card);
}

#[test]
fn key_3_and_inspects_button_turn_inspect_on_and_another_tool_takes_the_view_away() {
    let dimensions = || {
        Session::open(Opening {
            frozen_screen: vec![Some(monitor(FULL_HD, 96))],
            cursor: MIDDLE,
            options: Options::default(),
            remembered: Remembered::default(),
            second_click_until: None,
        })
        .expect("a frozen screen the session takes")
    };
    let mut session = dimensions();
    assert_eq!(session.scene().magnified_view, None);
    session.handle(key(Key::Digit3, false, 1000));
    assert_eq!(view(&session).center, MIDDLE);
    session.handle(key(Key::Digit1, false, 1100));
    assert_eq!(session.scene().magnified_view, None);

    let mut session = dimensions();
    let button = on(ToolbarButton::Tool(Tool::Inspect));
    session.handle(move_to(button, 1000));
    session.handle(event(
        button,
        1100,
        Input::Toolbar(ToolbarButton::Tool(Tool::Inspect)),
    ));
    assert_eq!(session.scene().toolbar.active, Tool::Inspect);
    // The cursor is on the toolbar, where the tool pauses: the view is where it was before, at the middle.
    assert_eq!(view(&session).center, MIDDLE);
}
