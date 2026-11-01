//! Layer 1 tests of the toolbar and of switching tools (v1 spec WPC-296, Toolbar and Trocar de ferramenta; ticket
//! WPC-310): the toolbar the scene shows, keys 1 to 4 and the tools' buttons, H, V and C, the close button, the buttons
//! that do nothing yet, the last tool remembered, and what a switch keeps, on frozen screens drawn here, without a
//! screen. They look only at the scene the session would draw and at the effects it returns.
//!
//! Inspect and Snip can be made active, and then their button is lit and nothing is measured (the owner's choice in
//! WPC-310); Inspect shows its magnified view (WPC-315), and Snip comes with its own ticket. Bounds measures only while a
//! rectangle is drawn (WPC-313).

use std::time::Duration;

use screengauge_core::session::{
    Colors, Effect, Event, Held, Image, Input, Key, LocalTime, Opening, Photo, Point, Rect, Scale,
    Session, Toolbar, ToolbarButton,
};
use screengauge_core::settings::{DimensionsMode, Options, Remembered, Tool};

/// The owner's 4K monitor at 175%, Windows' main monitor.
const AOC: Rect = Rect {
    left: 0,
    top: 0,
    width: 3840,
    height: 2160,
};

/// Where the top center puts the toolbar's corner on the 4K monitor: 930 × 117 at 175%, 25 inside the top side, and half
/// of the 3840 - 930 - 50 = 2860 across past the margin.
const AOC_TOP_CENTER: Point = Point {
    x: 25 + 1430,
    y: 25,
};

/// On the Full HD monitor: 663 × 82 at 125%, 18 inside the top side, and half of the 1920 - 663 - 36 = 1221 across,
/// rounded down, past the margin.
const ARZOPA_TOP_CENTER: Point = Point {
    x: 3840 + 18 + 610,
    y: 18,
};

/// The owner's Full HD monitor at 125%, right of the 4K one.
const ARZOPA: Rect = Rect {
    left: 3840,
    top: 0,
    width: 1920,
    height: 1080,
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

/// Both of the owner's monitors. On the 4K one, two blue cards of 200 × 201, columns 800 to 999 and 1024 to 1223, rows
/// 500 to 700, with a white strip of 24 pixels between them: from (1010, 600) the horizontal span is 24, and the cross
/// 24 × 2160. On the Full HD one, a blue card from (4000, 300) to (4099, 399): from (4050, 350) the cross is 100 × 100.
fn owners_monitors() -> Vec<Photo> {
    vec![
        monitor(AOC, 168, &[[800, 500, 999, 700], [1024, 500, 1223, 700]]),
        monitor(ARZOPA, 120, &[[4000, 300, 4099, 399]]),
    ]
}

/// Between the two cards on the 4K monitor.
const BETWEEN_CARDS: Point = Point { x: 1010, y: 600 };
/// Inside the card on the Full HD monitor.
const IN_SMALL_CARD: Point = Point { x: 4050, y: 350 };

/// The tools open with `last_tool` active and Dimensions in `mode`, the cursor at `cursor`, the default options, called
/// by the global shortcut.
fn open(last_tool: Tool, mode: DimensionsMode, cursor: Point) -> Session {
    Session::open(Opening {
        frozen_screen: owners_monitors().into_iter().map(Some).collect(),
        cursor,
        options: Options::default(),
        remembered: Remembered {
            last_tool,
            dimensions_mode: mode,
            ..Remembered::default()
        },
        second_click_until: None,
    })
    .expect("a frozen screen the session takes")
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

fn key(cursor: Point, key: Key) -> Event {
    event(cursor, 1000, Input::KeyDown(key))
}

fn toolbar(session: &Session) -> Toolbar {
    session.scene().toolbar
}

#[test]
fn key_2_makes_bounds_the_active_tool_and_dimensions_stops_measuring() {
    let mut session = open(Tool::Dimensions, DimensionsMode::Horizontal, BETWEEN_CARDS);
    assert!(session.scene().live.is_some());
    let effects = session.handle(key(BETWEEN_CARDS, Key::Digit2));
    assert_eq!(effects, []);
    assert_eq!(toolbar(&session).active, Tool::Bounds);
    assert_eq!(toolbar(&session).mode, None);
    assert_eq!(session.scene().live, None);
}

#[test]
fn keys_1_to_4_are_the_tools_in_the_toolbars_order_from_either_monitor() {
    for cursor in [BETWEEN_CARDS, IN_SMALL_CARD] {
        let mut session = open(Tool::Dimensions, DimensionsMode::Cross, cursor);
        for (digit, tool) in [
            (Key::Digit4, Tool::Snip),
            (Key::Digit3, Tool::Inspect),
            (Key::Digit1, Tool::Dimensions),
            (Key::Digit2, Tool::Bounds),
        ] {
            assert_eq!(session.handle(key(cursor, digit)), [], "{digit:?}");
            assert_eq!(toolbar(&session).active, tool, "{cursor:?} {digit:?}");
        }
    }
}

/// A click on `button`, which the Windows layer reports when the mouse comes up on it.
fn press(button: ToolbarButton) -> Event {
    // The toolbar is at the top of the 4K monitor; the session goes by the button, not by where the click was.
    event(Point { x: 1920, y: 40 }, 1000, Input::Toolbar(button))
}

#[test]
fn each_tools_button_makes_it_the_active_tool() {
    let mut session = open(Tool::Dimensions, DimensionsMode::Horizontal, BETWEEN_CARDS);
    for tool in [Tool::Inspect, Tool::Bounds, Tool::Snip, Tool::Dimensions] {
        assert_eq!(session.handle(press(ToolbarButton::Tool(tool))), []);
        assert_eq!(toolbar(&session).active, tool);
    }
}

#[test]
fn the_active_tools_own_button_changes_nothing() {
    let mut session = open(Tool::Dimensions, DimensionsMode::Horizontal, BETWEEN_CARDS);
    let live = session.scene().live.cloned();
    assert_eq!(
        session.handle(press(ToolbarButton::Tool(Tool::Dimensions))),
        []
    );
    assert_eq!(toolbar(&session).active, Tool::Dimensions);
    assert_eq!(session.scene().live.cloned(), live);
}

#[test]
fn h_v_and_c_buttons_switch_dimensions_mode_and_light_up() {
    let mut session = open(Tool::Dimensions, DimensionsMode::Cross, BETWEEN_CARDS);
    for (mode, label) in [
        (DimensionsMode::Horizontal, "24"),
        (DimensionsMode::Vertical, "2160"),
        (DimensionsMode::Cross, "24 × 2160"),
    ] {
        assert_eq!(session.handle(press(ToolbarButton::Mode(mode))), []);
        assert_eq!(toolbar(&session).mode, Some(mode));
        let live = session.scene().live.expect("Dimensions measures");
        assert_eq!(live.label.physical, label, "{mode:?}");
    }
}

#[test]
fn with_another_tool_active_h_v_and_c_change_nothing() {
    let mut session = open(Tool::Bounds, DimensionsMode::Cross, BETWEEN_CARDS);
    for mode in [DimensionsMode::Horizontal, DimensionsMode::Vertical] {
        assert_eq!(session.handle(press(ToolbarButton::Mode(mode))), []);
        assert_eq!(toolbar(&session).mode, None);
    }
    // Dimensions comes back in the mode it had.
    session.handle(press(ToolbarButton::Tool(Tool::Dimensions)));
    assert_eq!(toolbar(&session).mode, Some(DimensionsMode::Cross));
}

#[test]
fn the_close_button_closes_the_tools_and_erases_the_pinned_measurements() {
    let mut session = open(Tool::Dimensions, DimensionsMode::Horizontal, BETWEEN_CARDS);
    session.handle(event(BETWEEN_CARDS, 1000, Input::Press));
    assert_eq!(session.scene().pinned.len(), 1);
    // Nothing remembered changed, so nothing is written.
    assert_eq!(session.handle(press(ToolbarButton::Close)), [Effect::Close]);
    assert_eq!(session.scene().pinned, []);
    // Closed: the session takes nothing more.
    assert_eq!(session.handle(key(BETWEEN_CARDS, Key::Digit3)), []);
}

fn saved(effects: &[Effect]) -> Option<&Remembered> {
    effects.iter().find_map(|effect| match effect {
        Effect::SaveRemembered(remembered) => Some(remembered),
        _ => None,
    })
}

#[test]
fn the_last_tool_used_is_kept_and_the_next_call_starts_with_it() {
    let mut session = open(Tool::Dimensions, DimensionsMode::Cross, BETWEEN_CARDS);
    session.handle(key(BETWEEN_CARDS, Key::Digit3));
    session.handle(press(ToolbarButton::Tool(Tool::Snip)));
    let effects = session.handle(key(BETWEEN_CARDS, Key::Esc));
    // The screen comes back first, then the file is written.
    assert_eq!(effects.first(), Some(&Effect::Close));
    let kept = saved(&effects).expect("the last tool changed").clone();
    assert_eq!(kept.last_tool, Tool::Snip);
    // Dimensions' mode is kept as it was.
    assert_eq!(kept.dimensions_mode, DimensionsMode::Cross);

    let next = Session::open(Opening {
        frozen_screen: owners_monitors().into_iter().map(Some).collect(),
        cursor: BETWEEN_CARDS,
        options: Options::default(),
        remembered: kept,
        second_click_until: None,
    })
    .expect("a frozen screen the session takes");
    assert_eq!(
        toolbar(&next),
        Toolbar {
            monitor: 0,
            corner: AOC_TOP_CENTER,
            active: Tool::Snip,
            mode: None,
            minimized: false,
            hovered: None,
        }
    );
    assert_eq!(next.scene().live, None);
}

#[test]
fn ending_on_the_tool_it_opened_with_writes_nothing() {
    let mut session = open(Tool::Inspect, DimensionsMode::Vertical, BETWEEN_CARDS);
    session.handle(key(BETWEEN_CARDS, Key::Digit1));
    session.handle(press(ToolbarButton::Tool(Tool::Bounds)));
    session.handle(key(BETWEEN_CARDS, Key::Digit3));
    assert_eq!(session.handle(press(ToolbarButton::Close)), [Effect::Close]);
}

#[test]
fn a_toolbar_button_clicked_as_the_second_click_of_the_calling_double_click_does_nothing() {
    // Called with a double-click on the tray icon: until Windows has run 500 ms, a click is that double-click's second
    // click (WPC-308's guard), on the toolbar as anywhere else.
    let mut session = Session::open(Opening {
        frozen_screen: owners_monitors().into_iter().map(Some).collect(),
        cursor: BETWEEN_CARDS,
        options: Options::default(),
        remembered: Remembered::default(),
        second_click_until: Some(Duration::from_millis(500)),
    })
    .expect("a frozen screen the session takes");
    let early = |button| event(Point { x: 1920, y: 40 }, 300, Input::Toolbar(button));
    assert_eq!(session.handle(early(ToolbarButton::Close)), []);
    assert_eq!(session.handle(early(ToolbarButton::Tool(Tool::Bounds))), []);
    assert_eq!(toolbar(&session).active, Tool::Dimensions);
    // A key is no click: 2 switches at once.
    session.handle(event(BETWEEN_CARDS, 300, Input::KeyDown(Key::Digit2)));
    assert_eq!(toolbar(&session).active, Tool::Bounds);
    // Once the double-click's time is over, the buttons act.
    let later = event(
        Point { x: 1920, y: 40 },
        500,
        Input::Toolbar(ToolbarButton::Close),
    );
    assert_eq!(session.handle(later).first(), Some(&Effect::Close));
}

#[test]
fn the_gear_does_nothing_yet() {
    let mut session = open(Tool::Dimensions, DimensionsMode::Horizontal, BETWEEN_CARDS);
    session.handle(event(BETWEEN_CARDS, 1000, Input::Press));
    let before = format!("{:?}", session.scene());
    assert_eq!(session.handle(press(ToolbarButton::Settings)), []);
    assert_eq!(format!("{:?}", session.scene()), before);
}

fn pin_count(session: &Session) -> usize {
    session.scene().pinned.len()
}

#[test]
fn a_pin_made_in_dimensions_stays_through_inspect_and_snip_where_backspace_and_delete_do_nothing() {
    let mut session = open(Tool::Dimensions, DimensionsMode::Horizontal, BETWEEN_CARDS);
    session.handle(event(BETWEEN_CARDS, 1000, Input::Press));
    let pin = session.scene().pinned.to_vec();
    assert_eq!(pin.len(), 1);
    for (tool, undo) in [(Key::Digit3, Key::Backspace), (Key::Digit4, Key::Delete)] {
        session.handle(key(BETWEEN_CARDS, tool));
        assert_eq!(session.scene().pinned, pin, "{tool:?}");
        assert_eq!(session.handle(key(BETWEEN_CARDS, undo)), []);
        assert_eq!(session.scene().pinned, pin, "{tool:?} {undo:?}");
    }
    // With Bounds active, Backspace takes off Dimensions' pin: the two tools' pins are one stack.
    session.handle(press(ToolbarButton::Tool(Tool::Bounds)));
    session.handle(key(BETWEEN_CARDS, Key::Backspace));
    assert_eq!(pin_count(&session), 0);
}

#[test]
fn switching_keeps_the_frozen_screen() {
    let mut session = open(Tool::Dimensions, DimensionsMode::Horizontal, BETWEEN_CARDS);
    for digit in [Key::Digit2, Key::Digit3, Key::Digit4, Key::Digit1] {
        session.handle(key(BETWEEN_CARDS, digit));
        assert_eq!(
            session.scene().frozen_screen,
            owners_monitors(),
            "{digit:?}"
        );
    }
}

/// The owner's monitors with a faint gray column at x = 4200 down the whole Full HD monitor, 15 below white on every
/// channel: an edge at the default tolerance (2, so more than 6 counts), none from tolerance 5 up.
fn with_a_faint_line() -> Vec<Photo> {
    let mut photos = owners_monitors();
    let full_hd = &photos[1];
    let width = usize::try_from(ARZOPA.width).expect("a small monitor");
    let mut bgra = full_hd.image.bgra().to_vec();
    for row in 0..usize::try_from(ARZOPA.height).expect("a small monitor") {
        let at = (row * width + (4200 - 3840)) * 4;
        bgra[at..at + 3].copy_from_slice(&[240, 240, 240]);
    }
    photos[1] = Photo {
        image: Image::new(ARZOPA.width, ARZOPA.height, bgra).expect("the pixels fill the image"),
        ..full_hd.clone()
    };
    photos
}

#[test]
fn switching_keeps_the_tolerance_the_wheel_set() {
    let cursor = Point { x: 4150, y: 700 };
    let mut session = Session::open(Opening {
        frozen_screen: with_a_faint_line().into_iter().map(Some).collect(),
        cursor,
        options: Options::default(),
        remembered: Remembered {
            dimensions_mode: DimensionsMode::Horizontal,
            ..Remembered::default()
        },
        second_click_until: None,
    })
    .expect("a frozen screen the session takes");
    let length = |session: &Session| session.scene().live.map(|live| live.label.physical.clone());
    // From the monitor's left side to just before the faint line.
    assert_eq!(length(&session).as_deref(), Some("360"));
    // Four notches up: tolerance 6, and the faint line is no edge any more.
    session.handle(event(cursor, 1000, Input::Wheel(4 * 120)));
    assert_eq!(length(&session).as_deref(), Some("1920"));
    for away in [Key::Digit2, Key::Digit3, Key::Digit4] {
        session.handle(key(cursor, away));
        session.handle(key(cursor, Key::Digit1));
        assert_eq!(length(&session).as_deref(), Some("1920"), "{away:?}");
    }
    // The tolerance the wheel set is never kept: closing writes nothing.
    assert_eq!(session.handle(key(cursor, Key::Esc)), [Effect::Close]);
}

#[test]
fn the_toolbar_opens_on_the_cursors_monitor_with_the_last_tool_lit() {
    let session = open(Tool::Dimensions, DimensionsMode::Vertical, IN_SMALL_CARD);
    assert_eq!(
        toolbar(&session),
        Toolbar {
            monitor: 1,
            corner: ARZOPA_TOP_CENTER,
            active: Tool::Dimensions,
            mode: Some(DimensionsMode::Vertical),
            minimized: false,
            hovered: None,
        }
    );
    let session = open(Tool::Bounds, DimensionsMode::Vertical, BETWEEN_CARDS);
    assert_eq!(
        toolbar(&session),
        Toolbar {
            monitor: 0,
            corner: AOC_TOP_CENTER,
            active: Tool::Bounds,
            mode: None,
            minimized: false,
            hovered: None,
        }
    );
}

#[test]
fn the_toolbar_stays_on_its_monitor_while_the_tool_measures_on_the_other() {
    let mut session = open(Tool::Dimensions, DimensionsMode::Horizontal, IN_SMALL_CARD);
    session.handle(event(BETWEEN_CARDS, 1000, Input::Move));
    assert_eq!(session.scene().live.map(|live| live.monitor), Some(0));
    assert_eq!(toolbar(&session).monitor, 1);
    // The keys work from the other monitor too, and the toolbar there shows it.
    session.handle(key(BETWEEN_CARDS, Key::Digit3));
    assert_eq!(
        toolbar(&session),
        Toolbar {
            monitor: 1,
            corner: ARZOPA_TOP_CENTER,
            active: Tool::Inspect,
            mode: None,
            minimized: false,
            hovered: None,
        }
    );
}

#[test]
fn back_to_dimensions_it_measures_again_where_the_cursor_is_in_its_mode() {
    let mut session = open(Tool::Inspect, DimensionsMode::Cross, IN_SMALL_CARD);
    assert_eq!(session.scene().live, None);
    assert_eq!(toolbar(&session).mode, None);
    session.handle(key(IN_SMALL_CARD, Key::Digit1));
    assert_eq!(toolbar(&session).mode, Some(DimensionsMode::Cross));
    let live = session.scene().live.expect("Dimensions measures");
    assert_eq!(
        (live.monitor, live.label.physical.as_str()),
        (1, "100 × 100")
    );
}
