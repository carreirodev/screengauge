//! Layer 1 tests of the cursor over the toolbar, the measure through it and minimizing it (v1 spec WPC-296, Toolbar;
//! ticket WPC-311): the arrow on the toolbar and the thin cross off it, the tool pausing there with its measurement as it
//! was (the owner's choice in WPC-311), the wheel and the press doing nothing there, the button under the cursor,
//! Dimensions measuring through the toolbar and the pinned measurements, and the small button the minimized toolbar
//! becomes, on frozen screens drawn here, without a screen. They look only at the scene the session would draw, the
//! effects it returns, and where the core says the toolbar is.

use std::time::Duration;

use screengauge_core::session::toolbar::{self, Hit, Layout};
use screengauge_core::session::{
    Colors, Cursor, Effect, Event, Held, Image, Input, Key, LocalTime, Measurement, Opening, Photo,
    Point, Rect, Scale, Session, Span, ToolbarButton,
};
use screengauge_core::settings::{DimensionsMode, Options, Remembered, Tool, ToolbarPlace};

/// The owner's 4K monitor at 175%, Windows' main monitor.
const AOC: Rect = Rect {
    left: 0,
    top: 0,
    width: 3840,
    height: 2160,
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
/// 1200 to 1400, with a white strip of 24 pixels between them, and a big card from (1300, 60) to (2800, 1000) whose top
/// side runs under the toolbar, which sits at the top center, from column 1455 to 2384 and row 25 to 141. On the Full
/// HD one, a blue card from (4000, 300) to (4099, 399).
fn owners_monitors() -> Vec<Photo> {
    vec![
        monitor(
            AOC,
            168,
            &[
                [800, 1200, 999, 1400],
                [1024, 1200, 1223, 1400],
                [1300, 60, 2800, 1000],
            ],
        ),
        monitor(ARZOPA, 120, &[[4000, 300, 4099, 399]]),
    ]
}

/// Between the two small cards on the 4K monitor: 24 across, 2160 down the white strip.
const BETWEEN_CARDS: Point = Point { x: 1010, y: 1300 };
/// Inside the card on the Full HD monitor: 100 × 100.
const IN_SMALL_CARD: Point = Point { x: 4050, y: 350 };

/// Where the core puts the toolbar on the 4K monitor, where the cursors of these tests open it.
fn toolbar_layout() -> Layout {
    let scale = Scale::from_dpi(168).expect("a real scale");
    toolbar::layout_on(AOC, scale, ToolbarPlace::default())
}

fn middle(rect: Rect) -> Point {
    Point {
        x: rect.left + i32::try_from(rect.width / 2).expect("a small rectangle"),
        y: rect.top + i32::try_from(rect.height / 2).expect("a small rectangle"),
    }
}

/// The middle of `button` on the whole toolbar.
fn on(button: ToolbarButton) -> Point {
    let (_, place) = toolbar_layout()
        .buttons
        .into_iter()
        .find(|(each, _)| *each == button)
        .expect("every button has a place");
    middle(place)
}

/// The toolbar's background: the room between the tools' group and the modes'.
fn on_background() -> Point {
    let layout = toolbar_layout();
    Point {
        x: layout.tools.right() + 2,
        y: middle(layout.toolbar).y,
    }
}

fn open_with(mode: DimensionsMode, cursor: Point, second_click_until: Option<Duration>) -> Session {
    Session::open(Opening {
        frozen_screen: owners_monitors().into_iter().map(Some).collect(),
        cursor,
        options: Options::default(),
        remembered: Remembered {
            last_tool: Tool::Dimensions,
            dimensions_mode: mode,
            ..Remembered::default()
        },
        second_click_until,
    })
    .expect("a frozen screen the session takes")
}

/// The tools open in Dimensions in `mode`, the cursor at `cursor`, called by the global shortcut.
fn open(mode: DimensionsMode, cursor: Point) -> Session {
    open_with(mode, cursor, None)
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
            hour: 11,
            minute: 30,
            second: 5,
        },
        uptime: Duration::from_millis(milliseconds),
        input,
    }
}

fn move_to(session: &mut Session, to: Point) -> Vec<Effect> {
    session.handle(event(to, 1000, Input::Move))
}

/// A click on `button`, which the Windows layer reports when the mouse comes up on it, with the cursor there.
fn click(session: &mut Session, button: ToolbarButton, at: Point) -> Vec<Effect> {
    session.handle(event(at, 1000, Input::Toolbar(button)))
}

fn live(session: &Session) -> Option<Measurement> {
    session.scene().live.cloned()
}

fn label(session: &Session) -> Option<String> {
    session
        .scene()
        .live
        .map(|measurement| measurement.label.physical.clone())
}

#[test]
fn on_the_toolbar_the_cursor_is_the_arrow_and_off_it_the_thin_cross() {
    let mut session = open(DimensionsMode::Horizontal, BETWEEN_CARDS);
    assert_eq!(session.scene().cursor, Cursor::Cross);
    for point in [
        on_background(),
        on(ToolbarButton::Tool(Tool::Inspect)),
        on(ToolbarButton::Close),
    ] {
        move_to(&mut session, point);
        assert_eq!(session.scene().cursor, Cursor::Arrow, "{point:?}");
    }
    let layout = toolbar_layout();
    for off in [
        Point {
            x: layout.toolbar.left - 1,
            y: layout.toolbar.top,
        },
        Point {
            x: layout.toolbar.left,
            y: layout.toolbar.bottom() + 1,
        },
        IN_SMALL_CARD,
    ] {
        move_to(&mut session, off);
        assert_eq!(session.scene().cursor, Cursor::Cross, "{off:?}");
    }
}

#[test]
fn on_the_toolbar_the_measurement_stays_as_it_was_until_the_cursor_leaves() {
    let mut session = open(DimensionsMode::Horizontal, BETWEEN_CARDS);
    let before = live(&session);
    assert_eq!(label(&session).as_deref(), Some("24"));
    for point in [
        on(ToolbarButton::Tool(Tool::Bounds)),
        on_background(),
        on(ToolbarButton::Settings),
    ] {
        assert_eq!(move_to(&mut session, point), []);
        assert_eq!(live(&session), before, "{point:?}");
    }
    // Off the toolbar, it measures where the cursor is again, on either monitor.
    move_to(&mut session, IN_SMALL_CARD);
    let after = live(&session).expect("Dimensions measures");
    assert_eq!((after.monitor, after.label.physical.as_str()), (1, "100"));
}

#[test]
fn a_mode_clicked_on_the_toolbar_measures_again_where_the_tool_paused() {
    let mut session = open(DimensionsMode::Cross, BETWEEN_CARDS);
    let horizontal = on(ToolbarButton::Mode(DimensionsMode::Horizontal));
    move_to(&mut session, horizontal);
    click(
        &mut session,
        ToolbarButton::Mode(DimensionsMode::Horizontal),
        horizontal,
    );
    let measured = live(&session).expect("Dimensions measures");
    assert_eq!(
        measured.horizontal().map(|span| span.first.y),
        Some(BETWEEN_CARDS.y)
    );
    assert_eq!(measured.label.physical, "24");
    let vertical = on(ToolbarButton::Mode(DimensionsMode::Vertical));
    move_to(&mut session, vertical);
    click(
        &mut session,
        ToolbarButton::Mode(DimensionsMode::Vertical),
        vertical,
    );
    assert_eq!(label(&session).as_deref(), Some("2160"));
    // Another tool and back, from the toolbar: Dimensions comes back where it paused.
    let bounds = on(ToolbarButton::Tool(Tool::Bounds));
    move_to(&mut session, bounds);
    click(&mut session, ToolbarButton::Tool(Tool::Bounds), bounds);
    assert_eq!(live(&session), None);
    let dimensions = on(ToolbarButton::Tool(Tool::Dimensions));
    move_to(&mut session, dimensions);
    click(
        &mut session,
        ToolbarButton::Tool(Tool::Dimensions),
        dimensions,
    );
    let back = live(&session).expect("Dimensions measures");
    assert_eq!(
        back.vertical().map(|span| span.first.x),
        Some(BETWEEN_CARDS.x)
    );
}

#[test]
fn opened_with_the_cursor_on_the_toolbar_nothing_is_measured_until_it_leaves() {
    let mut session = open(DimensionsMode::Horizontal, on_background());
    assert_eq!(session.scene().cursor, Cursor::Arrow);
    assert_eq!(live(&session), None);
    move_to(&mut session, on(ToolbarButton::Close));
    assert_eq!(live(&session), None);
    move_to(&mut session, BETWEEN_CARDS);
    assert_eq!(label(&session).as_deref(), Some("24"));
}

#[test]
fn the_wheel_on_the_toolbar_changes_nothing() {
    let mut session = open(DimensionsMode::Horizontal, BETWEEN_CARDS);
    move_to(&mut session, on_background());
    assert_eq!(
        session.handle(event(on_background(), 1000, Input::Wheel(3 * 120))),
        []
    );
    assert_eq!(session.scene().tolerance_indicator, None);
    // Off the toolbar, one notch moves the tolerance one step from the default: the three on the toolbar didn't count.
    move_to(&mut session, BETWEEN_CARDS);
    session.handle(event(BETWEEN_CARDS, 1200, Input::Wheel(120)));
    let indicator = session
        .scene()
        .tolerance_indicator
        .expect("the wheel turned off the toolbar");
    assert_eq!(
        indicator.tolerance,
        Options::default().default_tolerance.up()
    );
}

#[test]
fn a_press_on_the_toolbar_neither_pins_nor_copies() {
    let mut session = open(DimensionsMode::Horizontal, BETWEEN_CARDS);
    for point in [on_background(), on(ToolbarButton::Tool(Tool::Snip))] {
        move_to(&mut session, point);
        assert_eq!(session.handle(event(point, 1000, Input::Press)), []);
        assert_eq!(session.scene().pinned, [], "{point:?}");
        assert_eq!(session.scene().notice, None, "{point:?}");
    }
}

#[test]
fn an_arrow_key_that_steps_onto_the_toolbar_pauses_the_tool_too() {
    let layout = toolbar_layout();
    let below = Point {
        x: middle(layout.toolbar).x,
        y: layout.toolbar.bottom() + 1,
    };
    let mut session = open(DimensionsMode::Horizontal, below);
    let before = live(&session);
    assert!(before.is_some());
    let up = session.handle(event(below, 1000, Input::KeyDown(Key::Up)));
    let onto = Point {
        y: below.y - 1,
        ..below
    };
    assert_eq!(up, [Effect::MoveCursor(onto)]);
    assert_eq!(session.scene().cursor, Cursor::Arrow);
    assert_eq!(live(&session), before);
    // Two rows down, off the toolbar and past where it paused: it measures where the keys put the cursor.
    session.handle(event(onto, 1000, Input::KeyDown(Key::Down)));
    session.handle(event(below, 1000, Input::KeyDown(Key::Down)));
    assert_eq!(session.scene().cursor, Cursor::Cross);
    let row = live(&session)
        .and_then(|measurement| measurement.horizontal())
        .map(|span| span.first.y);
    assert_eq!(row, Some(below.y + 1));
}

#[test]
fn the_button_under_the_cursor_is_the_hovered_one() {
    let mut session = open(DimensionsMode::Horizontal, BETWEEN_CARDS);
    assert_eq!(session.scene().toolbar.hovered, None);
    for (button, place) in toolbar_layout().buttons {
        move_to(&mut session, middle(place));
        assert_eq!(session.scene().toolbar.hovered, Some(button));
    }
    move_to(&mut session, on_background());
    assert_eq!(session.scene().toolbar.hovered, None);
    // With another tool active, H, V and C don't show, and their room is the background.
    let horizontal = on(ToolbarButton::Mode(DimensionsMode::Horizontal));
    move_to(&mut session, horizontal);
    session.handle(event(horizontal, 1000, Input::KeyDown(Key::Digit2)));
    assert_eq!(session.scene().toolbar.hovered, None);
    assert_eq!(session.scene().cursor, Cursor::Arrow);
    move_to(&mut session, BETWEEN_CARDS);
    assert_eq!(session.scene().toolbar.hovered, None);
}

#[test]
fn a_line_that_runs_under_the_toolbar_stops_at_the_real_edge() {
    // Below the toolbar, inside the tall card, whose top side at row 60 is under the toolbar.
    let cursor = Point { x: 1920, y: 600 };
    let session = open(DimensionsMode::Vertical, cursor);
    let span = session
        .scene()
        .live
        .and_then(|measurement| measurement.vertical())
        .expect("Dimensions measures the column");
    assert_eq!(
        span,
        Span {
            first: Point { x: 1920, y: 60 },
            last: Point { x: 1920, y: 1000 },
        }
    );
    assert_eq!(span.length(), 941);
    // The edge it stops at is under the toolbar, which the photo never had in it.
    assert!(session.toolbar_hit(span.first).is_some());
}

#[test]
fn a_line_that_crosses_a_pinned_measurement_measures_as_if_it_werent_there() {
    let mut session = open(DimensionsMode::Horizontal, BETWEEN_CARDS);
    session.handle(event(BETWEEN_CARDS, 1000, Input::Press));
    assert_eq!(session.scene().pinned.len(), 1);
    let pinned_row = BETWEEN_CARDS.y;
    session.handle(event(BETWEEN_CARDS, 1000, Input::KeyDown(Key::V)));
    // Down the white strip between the small cards, across the pinned line.
    let above = Point {
        x: BETWEEN_CARDS.x,
        y: 1250,
    };
    move_to(&mut session, above);
    let span = live(&session)
        .and_then(|measurement| measurement.vertical())
        .expect("Dimensions measures the column");
    assert!((span.first.y..=span.last.y).contains(&pinned_row));
    let unpinned = open(DimensionsMode::Vertical, above);
    assert_eq!(
        Some(span),
        unpinned
            .scene()
            .live
            .and_then(|measurement| measurement.vertical())
    );
    assert_eq!(span.length(), 2160);
}

#[test]
fn minimizing_leaves_the_small_button_and_the_tool_measures_where_the_toolbar_was() {
    let mut session = open(DimensionsMode::Vertical, BETWEEN_CARDS);
    let minimize = on(ToolbarButton::Minimize);
    move_to(&mut session, minimize);
    assert_eq!(click(&mut session, ToolbarButton::Minimize, minimize), []);
    let scene = session.scene();
    assert!(scene.toolbar.minimized);
    // Where the minimize button was is the tool's now: there it measures down the big card, from its top side under
    // where the toolbar was.
    assert_eq!(scene.cursor, Cursor::Cross);
    assert_eq!(scene.toolbar.hovered, None);
    let measured = scene.live.expect("the tool goes on measuring");
    assert_eq!(
        measured.vertical(),
        Some(Span {
            first: Point {
                x: minimize.x,
                y: 60
            },
            last: Point {
                x: minimize.x,
                y: 1000
            },
        })
    );
}

#[test]
fn on_the_minimized_toolbar_only_the_small_button_pauses_the_tool() {
    let mut session = open(DimensionsMode::Horizontal, BETWEEN_CARDS);
    click(&mut session, ToolbarButton::Minimize, BETWEEN_CARDS);
    let small = middle(toolbar_layout().minimized);
    let before = live(&session);
    move_to(&mut session, small);
    assert_eq!(session.scene().cursor, Cursor::Arrow);
    assert_eq!(
        session.scene().toolbar.hovered,
        Some(ToolbarButton::Restore)
    );
    assert_eq!(live(&session), before);
    assert_eq!(
        session.toolbar_hit(small),
        Some(Hit::Button(ToolbarButton::Restore))
    );
    // Where the Inspect button was, the tool measures, and a click pins and copies.
    let inspect = on(ToolbarButton::Tool(Tool::Inspect));
    move_to(&mut session, inspect);
    assert_eq!(session.scene().cursor, Cursor::Cross);
    assert_eq!(session.toolbar_hit(inspect), None);
    let effects = session.handle(event(inspect, 1000, Input::Press));
    assert!(matches!(effects.first(), Some(Effect::CopyText(_))));
    assert_eq!(session.scene().pinned.len(), 1);
}

#[test]
fn the_small_button_brings_the_toolbar_back_whole() {
    let mut session = open(DimensionsMode::Cross, BETWEEN_CARDS);
    click(&mut session, ToolbarButton::Minimize, BETWEEN_CARDS);
    let small = middle(toolbar_layout().minimized);
    move_to(&mut session, small);
    let paused = live(&session);
    assert_eq!(click(&mut session, ToolbarButton::Restore, small), []);
    let scene = session.scene();
    assert!(!scene.toolbar.minimized);
    assert_eq!(scene.toolbar.mode, Some(DimensionsMode::Cross));
    // The toolbar is back under the cursor, so the tool stays paused.
    assert_eq!(scene.cursor, Cursor::Arrow);
    assert_eq!(scene.live.cloned(), paused);
    assert_eq!(
        session.toolbar_hit(on(ToolbarButton::Close)),
        Some(Hit::Button(ToolbarButton::Close))
    );
}

#[test]
fn the_keys_work_while_minimized_and_the_small_button_shows_the_active_tool() {
    let mut session = open(DimensionsMode::Cross, BETWEEN_CARDS);
    click(&mut session, ToolbarButton::Minimize, BETWEEN_CARDS);
    session.handle(event(BETWEEN_CARDS, 1000, Input::KeyDown(Key::Digit3)));
    let toolbar = session.scene().toolbar;
    assert_eq!((toolbar.active, toolbar.minimized), (Tool::Inspect, true));
    session.handle(event(BETWEEN_CARDS, 1000, Input::KeyDown(Key::Digit1)));
    session.handle(event(BETWEEN_CARDS, 1000, Input::KeyDown(Key::H)));
    assert_eq!(label(&session).as_deref(), Some("24"));
    assert!(session.scene().toolbar.minimized);
}

#[test]
fn every_call_opens_the_toolbar_whole_and_minimizing_is_never_kept() {
    let mut session = open(DimensionsMode::Cross, BETWEEN_CARDS);
    click(&mut session, ToolbarButton::Minimize, BETWEEN_CARDS);
    assert!(session.scene().toolbar.minimized);
    // Minimizing alone changes nothing remembered, so nothing is written.
    assert_eq!(
        session.handle(event(BETWEEN_CARDS, 1000, Input::KeyDown(Key::Esc))),
        [Effect::Close]
    );
    // Minimized with another mode, what is written is the mode, and the next call opens the toolbar whole.
    let mut session = open(DimensionsMode::Cross, BETWEEN_CARDS);
    click(&mut session, ToolbarButton::Minimize, BETWEEN_CARDS);
    session.handle(event(BETWEEN_CARDS, 1000, Input::KeyDown(Key::V)));
    let effects = session.handle(event(BETWEEN_CARDS, 1000, Input::KeyDown(Key::Esc)));
    let kept = effects
        .iter()
        .find_map(|effect| match effect {
            Effect::SaveRemembered(remembered) => Some(remembered.clone()),
            _ => None,
        })
        .expect("the mode changed");
    let next = Session::open(Opening {
        frozen_screen: owners_monitors().into_iter().map(Some).collect(),
        cursor: BETWEEN_CARDS,
        options: Options::default(),
        remembered: kept,
        second_click_until: None,
    })
    .expect("a frozen screen the session takes");
    let toolbar = next.scene().toolbar;
    assert_eq!(
        (toolbar.minimized, toolbar.mode),
        (false, Some(DimensionsMode::Vertical))
    );
}

#[test]
fn minimize_and_restore_clicked_as_the_second_click_of_the_calling_double_click_do_nothing() {
    let mut session = open_with(
        DimensionsMode::Cross,
        BETWEEN_CARDS,
        Some(Duration::from_millis(500)),
    );
    let minimize = on(ToolbarButton::Minimize);
    let early = |button| event(minimize, 300, Input::Toolbar(button));
    assert_eq!(session.handle(early(ToolbarButton::Minimize)), []);
    assert!(!session.scene().toolbar.minimized);
    session.handle(event(
        minimize,
        600,
        Input::Toolbar(ToolbarButton::Minimize),
    ));
    assert!(session.scene().toolbar.minimized);
    let small = middle(toolbar_layout().minimized);
    session.handle(event(small, 600, Input::Move));
    assert_eq!(
        session.handle(event(small, 700, Input::Toolbar(ToolbarButton::Restore))),
        []
    );
    assert!(!session.scene().toolbar.minimized);
}
