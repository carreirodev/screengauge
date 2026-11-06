//! Layer 1 tests of the cases around Bounds' drag (v1 spec WPC-296, Bounds and Fechar as ferramentas; ticket WPC-314):
//! Alt held measures exactly the drawn rectangle, an area of nothing but the reference color shows "No content found",
//! a click does nothing, and Esc with the button held cancels only the drag. On frozen screens drawn here, without a
//! screen; they look only at the scene the session would draw and at the effects it returns.

use std::time::Duration;

use screengauge_core::session::toolbar;
use screengauge_core::session::{
    Colors, Cursor, DrawnRectangle, Effect, Event, Held, Image, Input, Key, Label, LocalTime,
    Measurement, Message, Notice, NoticeSide, Opening, Photo, Point, Rect, Scale, Session, Sign,
    ToolbarButton,
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

const ICON_GRAY: [u8; 3] = [90, 90, 90];

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
            side: NoticeSide::Above,
        })
    );
    // What the user asked for wasn't done, so the notice has the alert sign, not Copied's check mark, and it tells of Alt,
    // which measures the drawn rectangle itself (the owner's choice in WPC-314).
    assert_eq!(
        (
            Message::NoContentFound.text(),
            Message::NoContentFound.sign()
        ),
        (
            "No content found · Hold Alt to measure the rectangle",
            Sign::Alert
        )
    );
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
    session.handle(event_holding(
        from,
        ALT,
        milliseconds,
        Input::KeyDown(Key::Alt),
    ));
    drag_through(session, from, &[to], ALT, milliseconds + 5)
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
    session.handle(event(at(1200, 700), 1040, Input::Copied));
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
    session.handle(event(at(400, 300), 1100, Input::Copied));
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
fn a_click_within_the_drag_distance_does_nothing_with_or_without_alt_and_shows_no_notice() {
    // Begun on the white three pixels up and left of the button, a press that moves up to 4 pixels each way reaches
    // into its corner: without the drag distance, it would pin 2 × 2 of it, or the 5 × 5 drawn with Alt.
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
    // 4 pixels right and 4 down of the press is still within the drag distance.
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
    // 5 pixels along one axis counts, whichever way: what is copied, or the notice's message when nothing is.
    for (to, shown_after) in [
        (at(1002, 601), "3 x 2"),
        (at(1001, 602), "2 x 3"),
        (at(992, 597), "NoContentFound"),
        (at(997, 592), "NoContentFound"),
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
                    .map(|notice| format!("{:?}", notice.message))
            });
        assert_eq!(shown.as_deref(), Some(shown_after), "{to:?}");
    }
}

#[test]
fn the_limit_is_4_physical_pixels_at_every_scale() {
    // On the 4K monitor at 175% and on the Full HD one at 125% alike, as Windows' own drag distance is. With Alt, any
    // drag that counts copies.
    for start in [at(100, 595), at(3855, 595)] {
        for (moved, counts) in [(4, false), (5, true)] {
            let mut session = open(across_two_monitors(), start);
            let to = at(start.x + moved, start.y);
            let effects = drag_through(&mut session, start, &[to], ALT, 1000);
            assert_eq!(!copied(&effects).is_empty(), counts, "{start:?} {moved}");
        }
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
fn esc_held_down_or_pressed_again_with_the_button_still_held_closes_nothing() {
    let mut session = open(vec![button()], at(900, 500));
    drag(&mut session, at(900, 500), at(1200, 700), 1000);
    hold(&mut session, at(900, 500), at(1200, 700), 2000);
    // The first Esc cancels the drag. Held down, it repeats, as Windows sends it again after a moment and then every few
    // hundredths of a second, and a second press comes as well, all with the button still down.
    for milliseconds in [2100, 2600, 2633, 3000] {
        assert_eq!(
            session.handle(key(at(1200, 700), milliseconds, Key::Esc)),
            [],
            "{milliseconds}"
        );
    }
    assert_eq!(pinned(&session), [the_button()]);
    assert_eq!(
        session.handle(event(at(1200, 700), 3100, Input::Release)),
        []
    );
    // Once the button is up, Esc closes the tools.
    let effects = session.handle(key(at(1200, 700), 3200, Key::Esc));
    assert_eq!(effects.first(), Some(&Effect::Close));
}

#[test]
fn while_the_button_stays_down_after_esc_the_toolbar_doesnt_react() {
    let inspect = on_inspect();
    let start = at(inspect.x - 100, inspect.y + 400);
    let mut session = open(vec![button()], start);
    hold(&mut session, start, at(start.x + 50, start.y + 50), 1000);
    session.handle(key(at(start.x + 50, start.y + 50), 1100, Key::Esc));
    // The press is still the tool's: over the toolbar, no button reacts and the cursor stays the cross, as the button
    // coming up there clicks nothing.
    session.handle(event(inspect, 1200, Input::Move));
    assert_eq!(session.scene().toolbar.hovered, None);
    assert_eq!(session.scene().cursor, Cursor::Cross);
    assert_eq!(session.handle(event(inspect, 1300, Input::Release)), []);
    assert_eq!(session.scene().toolbar.active, Tool::Bounds);
    // Once the button is up, the toolbar reacts again.
    session.handle(event(inspect, 1400, Input::Move));
    assert_eq!(session.scene().cursor, Cursor::Arrow);
    assert_eq!(
        session.scene().toolbar.hovered,
        Some(ToolbarButton::Tool(Tool::Inspect))
    );
}

#[test]
fn after_esc_cancels_the_drag_switching_to_dimensions_makes_esc_close_again() {
    // The button still down is Bounds' no more once Dimensions is the active tool, and there Esc closes the tools.
    let mut session = open(vec![button()], at(900, 500));
    hold(&mut session, at(900, 500), at(1200, 700), 1000);
    session.handle(key(at(1200, 700), 1100, Key::Esc));
    session.handle(key(at(1200, 700), 1200, Key::Digit1));
    let effects = session.handle(key(at(1200, 700), 1300, Key::Esc));
    assert_eq!(effects.first(), Some(&Effect::Close));
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
