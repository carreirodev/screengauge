//! Layer 1 tests of dragging the toolbar (v1 spec WPC-296, Toolbar; ticket WPC-312): it moves by its background with
//! the cursor, inside its own monitor's margins (the owner's choice in WPC-312), the tool pausing all the while; the
//! place it is left at is the remembered state's, and the next call puts it at the same place of the cursor's monitor.
//! On the owner's two monitors, drawn here, without a screen.

use std::time::Duration;

use screengauge_core::session::toolbar::{self, Hit, Layout};
use screengauge_core::session::{
    Colors, Cursor, Effect, Event, Held, Image, Input, Key, LocalTime, Opening, Photo, Point, Rect,
    Scale, Session, ToolbarButton,
};
use screengauge_core::settings::{DimensionsMode, Options, Remembered, Tool, ToolbarPlace};

/// The owner's 4K monitor at 175%, Windows' main monitor. The toolbar there is 930 × 117, its margin 25, and the top
/// center puts its corner at (1455, 25).
const AOC: Rect = Rect {
    left: 0,
    top: 0,
    width: 3840,
    height: 2160,
};

/// The owner's Full HD monitor at 125%, right of the 4K one. The toolbar there is 663 × 82, its margin 18.
const ARZOPA: Rect = Rect {
    left: 3840,
    top: 0,
    width: 1920,
    height: 1080,
};

/// A white monitor at `rect` and `dpi`, with a blue card from (`left`, `top`) to (`right`, `bottom`), both included.
fn monitor(rect: Rect, dpi: u32, card: [i32; 4]) -> Photo {
    let width = usize::try_from(rect.width).expect("a small monitor");
    let height = usize::try_from(rect.height).expect("a small monitor");
    let mut bgra = [255, 255, 255, 255].repeat(width * height);
    let [left, top, right, bottom] = card;
    for y in top..=bottom {
        for x in left..=right {
            let column = usize::try_from(x - rect.left).expect("a pixel on the monitor");
            let row = usize::try_from(y - rect.top).expect("a pixel on the monitor");
            let at = (row * width + column) * 4;
            bgra[at..at + 3].copy_from_slice(&[235, 99, 37]);
        }
    }
    Photo {
        rect,
        scale: Scale::from_dpi(dpi).expect("a real scale"),
        colors: Colors::Exact,
        image: Image::new(rect.width, rect.height, bgra).expect("the pixels fill the image"),
    }
}

/// A blue card of 200 × 201 on the 4K monitor, from (800, 1200) to (999, 1400), and one of 100 × 100 on the Full HD
/// one, from (4000, 300) to (4099, 399).
fn owners_monitors() -> Vec<Option<Photo>> {
    vec![
        Some(monitor(AOC, 168, [800, 1200, 999, 1400])),
        Some(monitor(ARZOPA, 120, [4000, 300, 4099, 399])),
    ]
}

/// Inside the card on the 4K monitor: 200 across.
const IN_CARD: Point = Point { x: 900, y: 1300 };
/// Inside the card on the Full HD monitor.
const IN_SMALL_CARD: Point = Point { x: 4050, y: 350 };

fn place(x: f64, y: f64) -> ToolbarPlace {
    ToolbarPlace::new(x, y).expect("shares from 0 to 1")
}

fn remembered(place: ToolbarPlace) -> Remembered {
    Remembered {
        last_tool: Tool::Dimensions,
        dimensions_mode: DimensionsMode::Horizontal,
        toolbar_place: place,
        ..Remembered::default()
    }
}

/// The tools open in Dimensions' horizontal mode with the toolbar remembered at `place`, the cursor at `cursor`.
fn open_at(place: ToolbarPlace, cursor: Point, second_click_until: Option<Duration>) -> Session {
    Session::open(Opening {
        frozen_screen: owners_monitors(),
        cursor,
        options: Options::default(),
        remembered: remembered(place),
        second_click_until,
    })
    .expect("a frozen screen the session takes")
}

/// The tools open with the toolbar at the top center, the cursor at `cursor`, called by the global shortcut.
fn open(cursor: Point) -> Session {
    open_at(ToolbarPlace::default(), cursor, None)
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
            hour: 14,
            minute: 5,
            second: 0,
        },
        uptime: Duration::from_millis(milliseconds),
        input,
    }
}

fn handle(session: &mut Session, at: Point, input: Input) -> Vec<Effect> {
    session.handle(event(at, 1000, input))
}

/// Where the toolbar's top left corner is in the scene.
fn corner(session: &Session) -> Point {
    session.scene().toolbar.corner
}

/// Where the core puts the toolbar's parts with its corner at `corner`, on the 4K monitor.
fn layout_at(corner: Point) -> Layout {
    toolbar::layout(corner, Scale::from_dpi(168).expect("a real scale"))
}

fn middle(rect: Rect) -> Point {
    Point {
        x: rect.left + i32::try_from(rect.width / 2).expect("a small rectangle"),
        y: rect.top + i32::try_from(rect.height / 2).expect("a small rectangle"),
    }
}

/// The toolbar's top center corner on the 4K monitor.
const TOP_CENTER: Point = Point { x: 1455, y: 25 };

/// The toolbar's background at the top center: the room between the tools' group and the modes', at (1870, 83).
fn grip() -> Point {
    let layout = layout_at(TOP_CENTER);
    Point {
        x: layout.tools.right() + 2,
        y: middle(layout.toolbar).y,
    }
}

/// `point` moved `right` and `down`.
fn by(point: Point, right: i32, down: i32) -> Point {
    Point {
        x: point.x + right,
        y: point.y + down,
    }
}

/// The middle of `button` with the toolbar's corner at `corner`.
fn on(corner: Point, button: ToolbarButton) -> Point {
    let (_, place) = layout_at(corner)
        .buttons
        .into_iter()
        .find(|(each, _)| *each == button)
        .expect("every button has a place");
    middle(place)
}

#[test]
fn dragged_by_its_background_the_toolbar_moves_with_the_cursor() {
    let mut session = open(IN_CARD);
    assert_eq!(corner(&session), TOP_CENTER);
    assert_eq!(handle(&mut session, grip(), Input::Move), []);
    assert_eq!(handle(&mut session, grip(), Input::Press), []);
    let to = by(grip(), -400, 600);
    assert_eq!(handle(&mut session, to, Input::Move), []);
    assert_eq!(corner(&session), Point { x: 1055, y: 625 });
    assert_eq!(handle(&mut session, to, Input::Release), []);
    // What the cursor is on follows the toolbar: the grip is on its background, and where it was is the tool's again.
    assert_eq!(session.toolbar_hit(to), Some(Hit::Background));
    assert_eq!(session.toolbar_hit(grip()), None);
    assert_eq!(
        session.toolbar_hit(on(Point { x: 1055, y: 625 }, ToolbarButton::Close)),
        Some(Hit::Button(ToolbarButton::Close))
    );
}

#[test]
fn the_next_call_puts_it_at_the_same_place_of_the_cursors_monitor() {
    let mut session = open(IN_CARD);
    handle(&mut session, grip(), Input::Move);
    handle(&mut session, grip(), Input::Press);
    // Far past the bottom left: it stops at the margins, 25 from the left side and the bottom one.
    let far = by(grip(), -5000, 5000);
    handle(&mut session, far, Input::Move);
    assert_eq!(
        corner(&session),
        Point {
            x: 25,
            y: 2160 - 117 - 25
        }
    );
    handle(&mut session, far, Input::Release);
    let bottom_left = place(0.0, 1.0);
    assert_eq!(
        handle(&mut session, far, Input::KeyDown(Key::Esc)),
        [
            Effect::Close,
            Effect::SaveRemembered(remembered(bottom_left))
        ]
    );
    // Called again with the cursor on the Full HD monitor: its bottom left, 18 from the left side and the bottom one.
    let next = open_at(bottom_left, IN_SMALL_CARD, None);
    assert_eq!(next.scene().toolbar.monitor, 1);
    assert_eq!(
        corner(&next),
        Point {
            x: 3840 + 18,
            y: 1080 - 82 - 18
        }
    );
    // And back on the 4K monitor, exactly where it was left.
    assert_eq!(
        corner(&open_at(bottom_left, IN_CARD, None)),
        Point {
            x: 25,
            y: 2160 - 117 - 25
        }
    );
}

#[test]
fn it_stays_inside_its_own_monitor_while_the_cursor_goes_to_the_other() {
    let mut session = open(IN_CARD);
    handle(&mut session, grip(), Input::Move);
    let paused = session.scene().live.cloned();
    assert!(paused.is_some());
    handle(&mut session, grip(), Input::Press);
    // Onto the Full HD monitor, 217 lower than the grip: the toolbar stops 25 inside the 4K monitor's right side.
    let other = Point { x: 5000, y: 300 };
    assert_eq!(handle(&mut session, other, Input::Move), []);
    assert_eq!(session.scene().toolbar.monitor, 0);
    assert_eq!(
        corner(&session),
        Point {
            x: 3840 - 930 - 25,
            y: 25 + 217
        }
    );
    // The tool stays paused with the arrow, off the toolbar as the cursor is, until the button comes up.
    assert_eq!(session.scene().cursor, Cursor::Arrow);
    assert_eq!(session.scene().live.cloned(), paused);
    // Closing in the middle of the drag keeps the place the toolbar has: the right side, 217 of the 1993 down.
    assert_eq!(
        handle(&mut session, other, Input::KeyDown(Key::Esc)),
        [
            Effect::Close,
            Effect::SaveRemembered(remembered(place(1.0, 217.0 / 1993.0)))
        ]
    );
}

#[test]
fn a_drag_pins_and_copies_nothing_and_no_button_reacts_while_the_toolbar_is_held() {
    let mut session = open(IN_CARD);
    handle(&mut session, grip(), Input::Move);
    handle(&mut session, grip(), Input::Press);
    // Along the row, far enough right that the toolbar stops at the margin with the cursor over its close button.
    let stopped = Point {
        x: 3840 - 930 - 25,
        y: 25,
    };
    let over_close = Point {
        y: grip().y,
        ..on(stopped, ToolbarButton::Close)
    };
    assert_eq!(handle(&mut session, over_close, Input::Move), []);
    assert_eq!(corner(&session), stopped);
    assert_eq!(
        session.toolbar_hit(over_close),
        Some(Hit::Button(ToolbarButton::Close))
    );
    assert_eq!(session.scene().toolbar.hovered, None);
    // The button comes up there: the drag ends, and the close button isn't clicked.
    assert_eq!(handle(&mut session, over_close, Input::Release), []);
    assert_eq!(session.scene().pinned, []);
    assert_eq!(session.scene().notice, None);
    assert_eq!(session.scene().toolbar.hovered, Some(ToolbarButton::Close));
}

#[test]
fn a_press_on_a_button_or_on_the_minimized_toolbar_drags_nothing() {
    let mut session = open(IN_CARD);
    let inspect = on(TOP_CENTER, ToolbarButton::Tool(Tool::Inspect));
    handle(&mut session, inspect, Input::Move);
    handle(&mut session, inspect, Input::Press);
    handle(&mut session, by(inspect, 300, 300), Input::Move);
    assert_eq!(corner(&session), TOP_CENTER);
    handle(&mut session, by(inspect, 300, 300), Input::Release);

    let minimize = on(TOP_CENTER, ToolbarButton::Minimize);
    handle(
        &mut session,
        minimize,
        Input::Toolbar(ToolbarButton::Minimize),
    );
    let restore = middle(layout_at(TOP_CENTER).minimized);
    handle(&mut session, restore, Input::Move);
    assert_eq!(
        session.toolbar_hit(restore),
        Some(Hit::Button(ToolbarButton::Restore))
    );
    handle(&mut session, restore, Input::Press);
    handle(&mut session, by(restore, 300, 300), Input::Move);
    assert_eq!(corner(&session), TOP_CENTER);
    assert_eq!(
        handle(
            &mut session,
            by(restore, 300, 300),
            Input::KeyDown(Key::Esc)
        ),
        [Effect::Close]
    );
}

#[test]
fn a_toolbar_let_go_where_it_was_picked_up_leaves_the_remembered_state_as_it_was() {
    // A place the toolbar's pixels don't land on exactly: a third across the 4K monitor's room.
    let third = place(1.0 / 3.0, 0.5);
    let mut session = open_at(third, IN_CARD, None);
    let start = corner(&session);
    let grip = by(start, 415, 40);
    handle(&mut session, grip, Input::Move);
    handle(&mut session, grip, Input::Press);
    handle(&mut session, by(grip, 5, -3), Input::Move);
    assert_eq!(corner(&session), by(start, 5, -3));
    handle(&mut session, grip, Input::Move);
    assert_eq!(corner(&session), start);
    handle(&mut session, grip, Input::Release);
    assert_eq!(
        handle(&mut session, grip, Input::KeyDown(Key::Esc)),
        [Effect::Close]
    );
}

#[test]
fn the_second_click_of_the_double_click_that_called_it_drags_nothing() {
    let mut session = open_at(
        ToolbarPlace::default(),
        IN_CARD,
        Some(Duration::from_millis(500)),
    );
    session.handle(event(grip(), 300, Input::Move));
    session.handle(event(grip(), 350, Input::Press));
    session.handle(event(by(grip(), 0, 400), 400, Input::Move));
    assert_eq!(corner(&session), TOP_CENTER);
}

#[test]
fn the_arrow_keys_move_the_held_toolbar_with_the_cursor() {
    let mut session = open(IN_CARD);
    handle(&mut session, grip(), Input::Move);
    handle(&mut session, grip(), Input::Press);
    let shifted = Event {
        held: Held {
            shift: true,
            ..Held::default()
        },
        ..event(grip(), 1000, Input::KeyDown(Key::Down))
    };
    assert_eq!(
        session.handle(shifted),
        [Effect::MoveCursor(by(grip(), 0, 10))]
    );
    assert_eq!(corner(&session), by(TOP_CENTER, 0, 10));
}

#[test]
fn once_let_go_the_toolbar_stays_and_the_tool_measures_where_the_cursor_goes() {
    let mut session = open(IN_CARD);
    handle(&mut session, grip(), Input::Move);
    handle(&mut session, grip(), Input::Press);
    let to = by(grip(), 0, 300);
    handle(&mut session, to, Input::Move);
    handle(&mut session, to, Input::Release);
    handle(&mut session, IN_SMALL_CARD, Input::Move);
    assert_eq!(corner(&session), by(TOP_CENTER, 0, 300));
    assert_eq!(session.scene().cursor, Cursor::Cross);
    let live = session.scene().live.expect("Dimensions measures");
    assert_eq!((live.monitor, live.label.physical.as_str()), (1, "100"));
}
