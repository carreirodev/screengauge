//! Layer 1 tests of Snip taking an image to the clipboard (v1 spec WPC-296, Snip; ticket WPC-317): the area it takes,
//! which is Bounds' gesture, Alt, an empty area, a tiny drag, Esc in the middle of a drag, the wheel, the monitor's
//! limit, what the image holds, and what the scene shows while the button is held, on frozen screens drawn here, without
//! a screen. They look only at the scene the session would draw and at the effects it returns.

use std::time::Duration;

use screengauge_core::session::{
    Colors, DrawnRectangle, Effect, Event, Held, Image, Input, Key, Label, LocalTime, Message,
    Notice, NoticeSide, Opening, Photo, Point, Rect, Scale, Session, SnipArea, ToolbarButton,
};
use screengauge_core::settings::{Options, Remembered, Tool};

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
/// The fourth byte of every pixel is 0, as a photo may leave it: it says nothing.
fn monitor(rect: Rect, dpi: u32, shapes: &[([u8; 3], [i32; 4])]) -> Photo {
    let width = usize::try_from(rect.width).expect("a small monitor");
    let height = usize::try_from(rect.height).expect("a small monitor");
    let mut bgra = [WHITE[2], WHITE[1], WHITE[0], 0].repeat(width * height);
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

/// Snip active, the cursor at `cursor`, the default options, called by the global shortcut.
fn open(frozen_screen: Vec<Photo>, cursor: Point) -> Session {
    Session::open(Opening {
        frozen_screen: frozen_screen.into_iter().map(Some).collect(),
        cursor,
        options: Options::default(),
        remembered: Remembered {
            last_tool: Tool::Snip,
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

/// The images the effects put on the clipboard.
fn images(effects: &[Effect]) -> Vec<&Image> {
    effects
        .iter()
        .filter_map(|effect| match effect {
            Effect::CopyImage(image) => Some(image),
            _ => None,
        })
        .collect()
}

/// Every pixel of `image` as red, green and blue, row by row from the top.
fn colors(image: &Image) -> Vec<[u8; 3]> {
    image
        .pixels()
        .iter()
        .map(|&[blue, green, red, _]| [red, green, blue])
        .collect()
}

#[test]
fn a_drag_around_a_100_by_40_button_copies_an_image_of_100_by_40_of_the_button() {
    let mut session = open(vec![button()], at(900, 500));
    let effects = drag(&mut session, at(900, 500), at(1200, 700), 1000);
    let images = images(&effects);
    assert_eq!(images.len(), 1, "{effects:?}");
    assert_eq!((images[0].width(), images[0].height()), (100, 40));
    assert_eq!(colors(images[0]), vec![BUTTON_BLUE; 100 * 40]);
}

#[test]
fn letting_go_shows_copied_where_the_cursor_is_and_pins_nothing() {
    let mut session = open(vec![button()], at(900, 500));
    let effects = drag(&mut session, at(900, 500), at(1200, 700), 1000);
    assert!(
        !effects
            .iter()
            .any(|effect| matches!(effect, Effect::CopyText(_)))
    );
    assert_eq!(session.scene().pinned, []);
    // The notice waits for the clipboard's answer (WPC-352).
    assert_eq!(session.scene().notice, None);
    session.handle(event(at(1200, 700), 1100, Input::Copied));
    assert_eq!(
        session.scene().notice.copied(),
        Some(Notice {
            message: Message::Copied,
            at: at(1200, 700),
            monitor: 0,
            side: NoticeSide::Above,
        })
    );
}

#[test]
fn switching_to_bounds_in_the_middle_of_a_drag_drops_it_so_the_release_does_nothing() {
    let mut session = open(vec![button()], at(900, 500));
    hold(&mut session, at(900, 500), at(1200, 700), 1000);
    session.handle(event(at(1200, 700), 1030, Input::KeyDown(Key::Digit2)));
    assert_eq!(session.scene().drawn_rectangle, None);
    assert_eq!(
        session.handle(event(at(1200, 700), 1040, Input::Release)),
        []
    );
    assert_eq!(session.scene().pinned, []);
}

#[test]
fn switching_from_bounds_to_snip_in_the_middle_of_a_drag_drops_it_too() {
    let mut session = open(vec![button()], at(900, 500));
    session.handle(event(at(900, 500), 900, Input::KeyDown(Key::Digit2)));
    hold(&mut session, at(900, 500), at(1200, 700), 1000);
    session.handle(event(at(1200, 700), 1030, Input::KeyDown(Key::Digit4)));
    assert_eq!(session.scene().snip_area, None);
    assert_eq!(
        session.handle(event(at(1200, 700), 1040, Input::Release)),
        []
    );
}

fn rect(left: i32, top: i32, right: i32, bottom: i32) -> Rect {
    Rect {
        left,
        top,
        width: left.abs_diff(right) + 1,
        height: top.abs_diff(bottom) + 1,
    }
}

/// Alt held, and nothing else.
const ALT: Held = Held {
    shift: false,
    ctrl: false,
    alt: true,
};

const SHIFT: Held = Held {
    shift: true,
    ctrl: false,
    alt: false,
};

/// What the notice on screen says, if there is one.
fn notice(session: &Session) -> Option<Message> {
    session.scene().notice.map(|notice| notice.message)
}

/// The area the scene shows.
fn area(session: &Session) -> Option<Rect> {
    session.scene().snip_area.map(|area| area.rect)
}

#[test]
fn while_the_button_is_held_the_scene_shows_the_area_with_its_size_in_physical_pixels_alone() {
    // At 175% with "Also show logical pixels" on, as the defaults have it: Bounds' label would read 57.1 × 22.9 too.
    let mut session = open(vec![button()], at(900, 500));
    hold(&mut session, at(900, 500), at(1200, 700), 1000);
    let scene = session.scene();
    assert_eq!(
        scene.snip_area,
        Some(SnipArea {
            monitor: 0,
            rect: rect(1000, 600, 1099, 639),
            label: Label {
                physical: "100 × 40".to_owned(),
                logical: None,
            },
        })
    );
    // No Bounds measurement: the corners are Snip's own, with no fill.
    assert_eq!(scene.live, None);
    // The drawn rectangle shows dashed under the corners, as in Bounds.
    assert_eq!(
        scene.drawn_rectangle,
        Some(DrawnRectangle {
            monitor: 0,
            rect: rect(900, 500, 1200, 700),
        })
    );
}

#[test]
fn after_letting_go_the_area_and_the_drawn_rectangle_go_and_the_next_drag_takes_another_image() {
    let mut session = open(vec![button()], at(900, 500));
    drag(&mut session, at(900, 500), at(1200, 700), 1000);
    assert_eq!(session.scene().snip_area, None);
    assert_eq!(session.scene().drawn_rectangle, None);
    // Snip stays open, and the next drag, from the other corner, takes the same image again.
    let effects = drag(&mut session, at(1150, 680), at(950, 550), 2000);
    assert!(!effects.contains(&Effect::Close));
    let images = images(&effects);
    assert_eq!(images.len(), 1);
    assert_eq!((images[0].width(), images[0].height()), (100, 40));
}

#[test]
fn the_background_between_two_items_is_in_the_image() {
    // Two blue squares of 10 × 10, 20 columns of white between them, on a monitor at 100%.
    let screen = monitor(
        AOC,
        96,
        &[
            (BUTTON_BLUE, [100, 100, 109, 109]),
            (BUTTON_BLUE, [130, 100, 139, 109]),
        ],
    );
    let mut session = open(vec![screen], at(90, 90));
    let effects = drag(&mut session, at(90, 90), at(150, 120), 1000);
    let image = images(&effects)[0];
    assert_eq!((image.width(), image.height()), (40, 10));
    let row: Vec<[u8; 3]> = (0..40)
        .map(|x| {
            if (10..30).contains(&x) {
                WHITE
            } else {
                BUTTON_BLUE
            }
        })
        .collect();
    assert_eq!(colors(image), row.repeat(10));
}

#[test]
fn the_image_keeps_every_pixels_exact_color() {
    // Four colors side by side, each two columns wide: the image is never averaged, scaled or converted.
    let inks = [[1, 2, 3], [250, 128, 7], [0, 0, 0], [37, 99, 235]];
    let shapes: Vec<([u8; 3], [i32; 4])> = inks
        .iter()
        .zip([200, 202, 204, 206])
        .map(|(&ink, left)| (ink, [left, 300, left + 1, 301]))
        .collect();
    let mut session = open(vec![monitor(AOC, 144, &shapes)], at(190, 290));
    let effects = drag(&mut session, at(190, 290), at(220, 310), 1000);
    let image = images(&effects)[0];
    assert_eq!((image.width(), image.height()), (8, 2));
    let row: Vec<[u8; 3]> = inks.iter().flat_map(|&ink| [ink, ink]).collect();
    assert_eq!(colors(image), row.repeat(2));
}

#[test]
fn with_alt_held_as_the_button_comes_up_the_image_is_the_drawn_rectangle() {
    let mut session = open(vec![button()], at(900, 500));
    hold(&mut session, at(900, 500), at(1200, 700), 1000);
    let effects = session.handle(event_holding(at(1200, 700), ALT, 1030, Input::Release));
    let image = images(&effects)[0];
    assert_eq!((image.width(), image.height()), (301, 201));
    // The white around the button is in it, and the button 100 pixels in from its top left corner.
    let pixels = colors(image);
    assert_eq!(pixels[0], WHITE);
    assert_eq!(pixels[100 * 301 + 100], BUTTON_BLUE);
}

#[test]
fn alt_changes_the_preview_at_once_and_what_counts_is_its_state_as_the_button_comes_up() {
    let mut session = open(vec![button()], at(900, 500));
    hold(&mut session, at(900, 500), at(1200, 700), 1000);
    session.handle(event_holding(
        at(1200, 700),
        ALT,
        1030,
        Input::KeyDown(Key::Alt),
    ));
    assert_eq!(area(&session), Some(rect(900, 500, 1200, 700)));
    // With Alt, the drawn rectangle is the image itself: it isn't dashed under the corners.
    assert_eq!(session.scene().drawn_rectangle, None);
    // Alt let go before the button: the image is the content again.
    session.handle(event(at(1200, 700), 1040, Input::KeyUp(Key::Alt)));
    let effects = session.handle(event(at(1200, 700), 1050, Input::Release));
    let image = images(&effects)[0];
    assert_eq!((image.width(), image.height()), (100, 40));
}

#[test]
fn an_area_of_nothing_but_background_takes_no_image_and_says_no_content_found() {
    let mut session = open(vec![button()], at(100, 100));
    let effects = drag(&mut session, at(100, 100), at(400, 300), 1000);
    // Only the wake the notice asks for: no image, so the clipboard stays as it was.
    assert!(
        effects
            .iter()
            .all(|effect| matches!(effect, Effect::WakeAt(_))),
        "{effects:?}"
    );
    assert_eq!(notice(&session), Some(Message::NoContentFoundToSnip));
    assert_eq!(
        Message::NoContentFoundToSnip.text(),
        "No content found · Hold Alt to snip the rectangle"
    );
}

#[test]
fn while_the_drawn_rectangle_holds_no_content_only_the_drawn_rectangle_shows() {
    let mut session = open(vec![button()], at(100, 100));
    hold(&mut session, at(100, 100), at(400, 300), 1000);
    assert_eq!(area(&session), None);
    assert!(session.scene().drawn_rectangle.is_some());
}

#[test]
fn with_alt_an_area_of_nothing_but_background_becomes_an_image() {
    let mut session = open(vec![button()], at(100, 100));
    hold(&mut session, at(100, 100), at(400, 300), 1000);
    let effects = session.handle(event_holding(at(400, 300), ALT, 1030, Input::Release));
    let image = images(&effects)[0];
    assert_eq!((image.width(), image.height()), (301, 201));
    assert_eq!(colors(image), vec![WHITE; 301 * 201]);
    session.handle(event(at(400, 300), 1040, Input::Copied));
    assert_eq!(notice(&session), Some(Message::Copied));
}

#[test]
fn a_tiny_drag_takes_no_image_and_shows_no_notice_with_or_without_alt() {
    // Four pixels along both axes is still a click or a tremble of the mouse; five along either is a drag.
    for held in [Held::default(), ALT] {
        let mut session = open(vec![button()], at(1050, 620));
        session.handle(event(at(1050, 620), 1000, Input::Move));
        session.handle(event_holding(at(1050, 620), held, 1010, Input::Press));
        session.handle(event_holding(at(1054, 624), held, 1020, Input::Move));
        assert_eq!(area(&session), None, "{held:?}");
        let effects = session.handle(event_holding(at(1054, 624), held, 1030, Input::Release));
        assert_eq!(effects, [], "{held:?}");
        assert_eq!(notice(&session), None, "{held:?}");
    }
    // Started on the button, blue is the reference color, so Alt takes the drawn rectangle: a row of six pixels.
    let mut session = open(vec![button()], at(1050, 620));
    hold(&mut session, at(1050, 620), at(1055, 620), 1000);
    let effects = session.handle(event_holding(at(1055, 620), ALT, 1030, Input::Release));
    let image = images(&effects)[0];
    assert_eq!((image.width(), image.height()), (6, 1));
}

#[test]
fn esc_with_the_button_held_cancels_only_the_drag() {
    let mut session = open(vec![button()], at(900, 500));
    hold(&mut session, at(900, 500), at(1200, 700), 1000);
    let effects = session.handle(event(at(1200, 700), 1030, Input::KeyDown(Key::Esc)));
    assert_eq!(effects, []);
    assert_eq!(area(&session), None);
    assert_eq!(session.scene().drawn_rectangle, None);
    // The button coming up afterwards takes nothing, and the tools are still open for the next drag.
    assert_eq!(
        session.handle(event(at(1200, 700), 1040, Input::Release)),
        []
    );
    let effects = drag(&mut session, at(900, 500), at(1200, 700), 2000);
    assert_eq!(images(&effects).len(), 1);
}

#[test]
fn the_arrows_move_the_cursor_a_pixel_or_ten_with_shift_with_the_button_held() {
    // From above the button's left side to its middle: the area follows the cursor as the arrows move it.
    let mut session = open(vec![button()], at(900, 500));
    hold(&mut session, at(900, 500), at(1050, 700), 1000);
    assert_eq!(area(&session), Some(rect(1000, 600, 1050, 639)));
    let effects = session.handle(event(at(1050, 700), 1030, Input::KeyDown(Key::Right)));
    assert_eq!(effects, [Effect::MoveCursor(at(1051, 700))]);
    assert_eq!(area(&session), Some(rect(1000, 600, 1051, 639)));
    session.handle(event_holding(
        at(1051, 700),
        SHIFT,
        1040,
        Input::KeyDown(Key::Right),
    ));
    assert_eq!(area(&session), Some(rect(1000, 600, 1061, 639)));
    let effects = session.handle(event(at(1061, 700), 1050, Input::Release));
    let image = images(&effects)[0];
    assert_eq!((image.width(), image.height()), (62, 40));
}

#[test]
fn the_arrows_move_the_cursor_without_the_button_too() {
    let mut session = open(vec![button()], at(900, 500));
    session.handle(event(at(900, 500), 1000, Input::Move));
    let effects = session.handle(event(at(900, 500), 1010, Input::KeyDown(Key::Down)));
    assert_eq!(effects, [Effect::MoveCursor(at(900, 501))]);
}

#[test]
fn the_wheel_changes_the_tolerance_in_the_middle_of_a_drag_and_the_image_follows_it() {
    // A faint gray card, 7 levels from the white: content at the default tolerance of 6, background at 7. A blue
    // square inside it stays content at both.
    let faint = [248, 248, 248];
    let screen = monitor(
        AOC,
        96,
        &[
            (faint, [300, 300, 349, 319]),
            (BUTTON_BLUE, [320, 305, 329, 314]),
        ],
    );
    let mut session = open(vec![screen], at(250, 250));
    hold(&mut session, at(250, 250), at(400, 400), 1000);
    assert_eq!(area(&session), Some(rect(300, 300, 349, 319)));
    session.handle(event(at(400, 400), 1030, Input::Wheel(120)));
    assert_eq!(area(&session), Some(rect(320, 305, 329, 314)));
    let effects = session.handle(event(at(400, 400), 1040, Input::Release));
    let image = images(&effects)[0];
    assert_eq!((image.width(), image.height()), (10, 10));
}

/// The ARZOPA at 125%, right of the AOC.
const ARZOPA: Rect = Rect {
    left: 3840,
    top: 0,
    width: 1920,
    height: 1080,
};

#[test]
fn the_area_stops_at_the_limit_of_the_monitor_the_drag_began_on() {
    // A blue bar running to the AOC's right side, and blue on the ARZOPA too, which the image never takes.
    let aoc = monitor(AOC, 168, &[(BUTTON_BLUE, [3800, 500, 3839, 519])]);
    let arzopa = monitor(ARZOPA, 120, &[(BUTTON_BLUE, [3840, 400, 3999, 699])]);
    let mut session = open(vec![aoc, arzopa], at(3700, 450));
    let effects = drag(&mut session, at(3700, 450), at(4100, 600), 1000);
    let image = images(&effects)[0];
    assert_eq!((image.width(), image.height()), (40, 20));
    assert_eq!(colors(image), vec![BUTTON_BLUE; 40 * 20]);
    // "Copied" shows where the cursor is, on the ARZOPA.
    session.handle(event(at(4100, 600), 1100, Input::Copied));
    assert_eq!(session.scene().notice.map(|notice| notice.monitor), Some(1));
}

#[test]
fn on_a_monitor_left_of_the_main_one_the_image_is_taken_from_its_own_pixels() {
    // A Full HD monitor at 100% left of the main one, at negative coordinates.
    let left = Rect {
        left: -1920,
        top: -200,
        width: 1920,
        height: 1080,
    };
    let screen = monitor(left, 96, &[(BUTTON_BLUE, [-500, -100, -481, -91])]);
    let mut session = open(vec![screen, button()], at(-600, -150));
    let effects = drag(&mut session, at(-600, -150), at(-400, -50), 1000);
    let image = images(&effects)[0];
    assert_eq!((image.width(), image.height()), (20, 10));
    assert_eq!(colors(image), vec![BUTTON_BLUE; 20 * 10]);
}

#[test]
fn shift_as_the_button_comes_up_changes_nothing() {
    let mut plain = open(vec![button()], at(900, 500));
    let without = drag(&mut plain, at(900, 500), at(1200, 700), 1000);
    let mut shifted = open(vec![button()], at(900, 500));
    hold(&mut shifted, at(900, 500), at(1200, 700), 1000);
    let with = shifted.handle(event_holding(at(1200, 700), SHIFT, 1030, Input::Release));
    assert_eq!(with, without);
}

#[test]
fn backspace_and_delete_do_nothing_in_snip_and_a_bounds_pin_stays() {
    let mut session = open(vec![button()], at(900, 500));
    session.handle(event(at(900, 500), 900, Input::KeyDown(Key::Digit2)));
    drag(&mut session, at(900, 500), at(1200, 700), 1000);
    assert_eq!(session.scene().pinned.len(), 1);
    session.handle(event(at(1200, 700), 2000, Input::KeyDown(Key::Digit4)));
    for key in [Key::Backspace, Key::Delete] {
        assert_eq!(
            session.handle(event(at(1200, 700), 2100, Input::KeyDown(key))),
            []
        );
    }
    assert_eq!(session.scene().pinned.len(), 1);
    // The Snip after it pins nothing more.
    drag(&mut session, at(900, 500), at(1200, 700), 3000);
    assert_eq!(session.scene().pinned.len(), 1);
}

#[test]
fn key_4_and_the_toolbar_button_make_snip_the_tool_that_takes_the_image() {
    for switch in [
        Input::KeyDown(Key::Digit4),
        Input::Toolbar(ToolbarButton::Tool(Tool::Snip)),
    ] {
        let mut session = Session::open(Opening {
            frozen_screen: vec![Some(button())],
            cursor: at(900, 500),
            options: Options::default(),
            remembered: Remembered::default(),
            second_click_until: None,
        })
        .expect("a frozen screen the session takes");
        session.handle(event(at(900, 500), 900, switch.clone()));
        let effects = drag(&mut session, at(900, 500), at(1200, 700), 1000);
        assert_eq!(images(&effects).len(), 1, "{switch:?}");
    }
}

#[test]
fn the_second_click_of_the_double_click_that_called_screengauge_starts_no_drag() {
    let mut session = Session::open(Opening {
        frozen_screen: vec![Some(button())],
        cursor: at(900, 500),
        options: Options::default(),
        remembered: Remembered {
            last_tool: Tool::Snip,
            ..Remembered::default()
        },
        second_click_until: Some(Duration::from_millis(1500)),
    })
    .expect("a frozen screen the session takes");
    assert_eq!(drag(&mut session, at(900, 500), at(1200, 700), 1000), []);
    assert_eq!(notice(&session), None);
}
