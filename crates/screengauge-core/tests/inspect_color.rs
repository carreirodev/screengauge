//! Layer 1 tests of Inspect's color (v1 spec WPC-296, Inspect; ticket WPC-316): the color of the exact pixel under the
//! cursor, in HEX and in RGB as Color format says, on the magnified view's strip and in what the click copies, with
//! Shift copying the other format. They look only at the scene the session would draw and at the effects it returns, on
//! frozen screens made up here, without a screen.

use std::time::Duration;

use screengauge_core::session::toolbar;
use screengauge_core::session::{
    Colors, Effect, Event, Held, Image, Input, Key, LocalTime, MagnifiedView, Message, Opening,
    Photo, Point, Rect, Scale, Session,
};
use screengauge_core::settings::{ColorFormat, Options, Remembered, Tool};

/// A Full HD monitor at 100%, Windows' main monitor.
const FULL_HD: Rect = Rect {
    left: 0,
    top: 0,
    width: 1920,
    height: 1080,
};

/// The one pixel of `item`'s color, far from every side of the monitor and below the toolbar.
const ITEM: Point = Point { x: 700, y: 500 };

/// Dodger blue, the spec's example: `#1E90FF`, or `30, 144, 255`.
const DODGER_BLUE: [u8; 3] = [30, 144, 255];

/// A white Full HD monitor at 100% with a single pixel of `color` at `ITEM`, and each of its eight neighbors a color
/// of its own, none of them white, so a color averaged with any neighbor is a different color.
fn item(color: [u8; 3]) -> Photo {
    let mut bgra = [255; 4].repeat(1920 * 1080);
    let mut paint = |x: i32, y: i32, [red, green, blue]: [u8; 3]| {
        let at = (usize::try_from(y).expect("on the monitor") * 1920
            + usize::try_from(x).expect("on the monitor"))
            * 4;
        bgra[at..at + 3].copy_from_slice(&[blue, green, red]);
    };
    let mut neighbor = 0;
    for dy in -1..=1 {
        for dx in -1..=1 {
            if (dx, dy) != (0, 0) {
                paint(
                    ITEM.x + dx,
                    ITEM.y + dy,
                    [20 * neighbor, 0, 200 - 20 * neighbor],
                );
                neighbor += 1;
            }
        }
    }
    paint(ITEM.x, ITEM.y, color);
    Photo {
        rect: FULL_HD,
        scale: Scale::HUNDRED_PERCENT,
        colors: Colors::Exact,
        image: Image::new(1920, 1080, bgra).expect("the pixels fill the image"),
    }
}

/// Inspect on `photo` with the cursor at `ITEM` and Color format at `format`.
fn open(photo: Photo, format: ColorFormat) -> Session {
    Session::open(Opening {
        frozen_screen: vec![Some(photo)],
        cursor: ITEM,
        options: Options {
            color_format: format,
            ..Options::default()
        },
        remembered: Remembered {
            last_tool: Tool::Inspect,
            ..Remembered::default()
        },
        second_click_until: None,
    })
    .expect("a frozen screen the session takes")
}

/// `input` with the cursor at `ITEM`, Shift held when `shift`, `milliseconds` after Windows started.
fn event(milliseconds: u64, shift: bool, input: Input) -> Event {
    Event {
        cursor: ITEM,
        held: Held {
            shift,
            ..Held::default()
        },
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

/// A click at `ITEM`, with Shift held when `shift`: what its press returns.
fn click(session: &mut Session, milliseconds: u64, shift: bool) -> Vec<Effect> {
    let effects = session.handle(event(milliseconds, shift, Input::Press));
    session.handle(event(milliseconds + 50, shift, Input::Release));
    effects
}

/// The text a click put on the clipboard, if any.
fn copied(effects: &[Effect]) -> Option<&str> {
    effects.iter().find_map(|effect| match effect {
        Effect::CopyText(text) => Some(text.as_str()),
        _ => None,
    })
}

#[test]
fn a_click_copies_the_hex_of_the_exact_pixel_under_the_cursor_and_says_copied() {
    let mut session = open(item(DODGER_BLUE), ColorFormat::Both);
    let effects = click(&mut session, 1000, false);
    assert_eq!(copied(&effects), Some("#1E90FF"));
    // The notice waits for the clipboard's answer (WPC-352).
    assert_eq!(session.scene().notice, None);
    session.handle(event(1100, false, Input::Copied));
    let notice = session.scene().notice.copied().expect("a notice");
    assert_eq!(notice.message, Message::Copied);
    assert_eq!(notice.at, ITEM);
}

#[test]
fn with_both_formats_shift_and_click_copies_the_rgb() {
    let mut session = open(item(DODGER_BLUE), ColorFormat::Both);
    assert_eq!(
        copied(&click(&mut session, 1000, true)),
        Some("30, 144, 255")
    );
    // Shift was only held for that click.
    assert_eq!(copied(&click(&mut session, 2000, false)), Some("#1E90FF"));
}

#[test]
fn with_one_format_the_click_copies_it_and_shift_and_click_copies_the_other_one_hidden() {
    for (format, click_copies, shift_copies) in [
        (ColorFormat::Hex, "#1E90FF", "30, 144, 255"),
        (ColorFormat::Rgb, "30, 144, 255", "#1E90FF"),
    ] {
        let mut session = open(item(DODGER_BLUE), format);
        assert_eq!(
            copied(&click(&mut session, 1000, false)),
            Some(click_copies),
            "{format:?}"
        );
        assert_eq!(
            copied(&click(&mut session, 2000, true)),
            Some(shift_copies),
            "{format:?}"
        );
    }
}

#[test]
fn the_second_click_of_the_double_click_that_called_screengauge_copies_no_color() {
    // Called from the tray icon with a double-click, whose second click comes until 500 ms (WPC-308).
    let mut session = Session::open(Opening {
        frozen_screen: vec![Some(item(DODGER_BLUE))],
        cursor: ITEM,
        options: Options::default(),
        remembered: Remembered {
            last_tool: Tool::Inspect,
            ..Remembered::default()
        },
        second_click_until: Some(Duration::from_millis(500)),
    })
    .expect("a frozen screen the session takes");
    assert_eq!(click(&mut session, 400, false), []);
    assert_eq!(session.scene().notice, None);
    // A click after it copies.
    assert_eq!(copied(&click(&mut session, 600, false)), Some("#1E90FF"));
}

#[test]
fn a_click_pins_nothing_closes_nothing_and_copies_only_the_color_without_the_position() {
    let mut session = open(item(DODGER_BLUE), ColorFormat::Both);
    let effects = click(&mut session, 1000, false);
    // The copy alone: no Close, nothing else. The clipboard's answer brings the notice, and the wake for it to go.
    assert_eq!(effects, [Effect::CopyText("#1E90FF".to_owned())]);
    assert_eq!(
        session.handle(event(1100, false, Input::Copied)),
        [Effect::WakeAt(Duration::from_millis(1100 + 1300))]
    );
    let scene = session.scene();
    assert!(scene.pinned.is_empty());
    assert_eq!(scene.live, None);
    assert_eq!(scene.toolbar.active, Tool::Inspect);
    let view = scene
        .magnified_view
        .expect("Inspect still shows its magnified view");
    assert_eq!(view.center, ITEM);
}

#[test]
fn hex_gives_each_channel_two_digits_with_a_leading_zero_and_rgb_gives_none() {
    for (color, hex, rgb) in [
        ([0, 5, 10], "#00050A", "0, 5, 10"),
        ([255, 255, 255], "#FFFFFF", "255, 255, 255"),
        ([0, 0, 0], "#000000", "0, 0, 0"),
        ([171, 205, 239], "#ABCDEF", "171, 205, 239"),
    ] {
        let mut session = open(item(color), ColorFormat::Both);
        assert_eq!(copied(&click(&mut session, 1000, false)), Some(hex));
        assert_eq!(copied(&click(&mut session, 2000, true)), Some(rgb));
    }
}

/// An arrow key going down, `milliseconds` after Windows started.
fn arrow(key: Key, milliseconds: u64) -> Event {
    event(milliseconds, false, Input::KeyDown(key))
}

#[test]
fn an_arrow_key_moves_to_the_neighbor_and_the_click_copies_its_color_alone() {
    let mut session = open(item(DODGER_BLUE), ColorFormat::Hex);
    // The neighbor on the right is the fifth: 20 × 4 of red, no green, 200 − 20 × 4 of blue.
    session.handle(arrow(Key::Right, 1000));
    // Windows reports the press where it put the cursor.
    let effects = session.handle(Event {
        cursor: Point {
            x: ITEM.x + 1,
            y: ITEM.y,
        },
        ..event(1100, false, Input::Press)
    });
    assert_eq!(copied(&effects), Some("#500078"));
}

#[test]
fn a_press_on_the_toolbar_copies_no_color() {
    let mut session = open(item(DODGER_BLUE), ColorFormat::Both);
    // The toolbar's background, just inside its left side.
    let corner = session.scene().toolbar.corner;
    let toolbar = toolbar::layout(corner, Scale::HUNDRED_PERCENT).toolbar;
    let background = Point {
        x: toolbar.left + 2,
        y: toolbar.top + toolbar.height.cast_signed() / 2,
    };
    let effects = session.handle(Event {
        cursor: background,
        ..event(1000, false, Input::Press)
    });
    assert_eq!(copied(&effects), None);
    assert_eq!(session.scene().notice, None);
}

/// The magnified view the session shows now.
fn view(session: &Session) -> MagnifiedView {
    session
        .scene()
        .magnified_view
        .expect("Inspect shows its magnified view")
}

/// What the strip's color reads: its first text, and the one under it when there is one.
fn reads(view: &MagnifiedView) -> (&str, Option<&str>) {
    (
        view.color.first.text.as_str(),
        view.color
            .second
            .as_ref()
            .map(|second| second.text.as_str()),
    )
}

#[test]
fn the_strip_shows_the_exact_pixels_swatch_and_its_color_in_color_format() {
    for (format, shown) in [
        (ColorFormat::Both, ("#1E90FF", Some("30, 144, 255"))),
        (ColorFormat::Hex, ("#1E90FF", None)),
        (ColorFormat::Rgb, ("30, 144, 255", None)),
    ] {
        let view = view(&open(item(DODGER_BLUE), format));
        assert_eq!(view.color.rgb, DODGER_BLUE, "{format:?}");
        assert_eq!(reads(&view), shown, "{format:?}");
    }
}

#[test]
fn the_click_copies_the_strips_first_text_and_shift_and_click_its_second_when_it_shows() {
    for format in [ColorFormat::Both, ColorFormat::Hex, ColorFormat::Rgb] {
        let mut session = open(item(DODGER_BLUE), format);
        let view = view(&session);
        assert_eq!(
            copied(&click(&mut session, 1000, false)),
            Some(view.color.first.text.as_str()),
            "{format:?}"
        );
        if let Some(second) = &view.color.second {
            assert_eq!(
                copied(&click(&mut session, 2000, true)),
                Some(second.text.as_str()),
                "{format:?}"
            );
        }
    }
}

fn rect(left: i32, top: i32, width: u32, height: u32) -> Rect {
    Rect {
        left,
        top,
        width,
        height,
    }
}

#[test]
fn with_both_formats_hex_and_rgb_take_a_line_each_beside_the_swatch_centered_between_them() {
    // At 100% and 8× with the cursor at (700, 500): the frame of 168 starts at (729, 529), and X and Y share the line
    // 7 below it, 16 tall, from 704. The color's row follows 5 below, at 725: two lines of 16 beside the swatch of 22,
    // which sits 5 down to be centered in their 32. The strip's text starts 4 in from the frame, at 733, and the
    // color's 22 + 8 after it, at 763, as wide as the 160 of text less those 30.
    let view = view(&open(item(DODGER_BLUE), ColorFormat::Both));
    assert_eq!(view.x.line, rect(733, 704, 76, 16));
    assert_eq!(view.color.swatch, rect(733, 725 + 5, 22, 22));
    assert_eq!(view.color.first.line, rect(763, 725, 130, 16));
    let second = view.color.second.expect("RGB under HEX");
    assert_eq!(second.line, rect(763, 725 + 16, 130, 16));
    // The panel ends 2 below the row and 7 below that: 7 + 168 + 7 + 16 + 5 + 32 + 2 + 7.
    assert_eq!(view.panel, rect(722, 522, 182, 244));
}

#[test]
fn with_one_format_its_line_is_centered_beside_the_swatch() {
    // As above, but the row is the swatch's 22, and the line of 16 sits 3 down in it.
    for format in [ColorFormat::Hex, ColorFormat::Rgb] {
        let view = view(&open(item(DODGER_BLUE), format));
        assert_eq!(view.color.swatch, rect(733, 725, 22, 22), "{format:?}");
        assert_eq!(
            view.color.first.line,
            rect(763, 725 + 3, 130, 16),
            "{format:?}"
        );
        assert_eq!(view.color.second, None, "{format:?}");
        assert_eq!(
            view.panel.height,
            7 + 168 + 7 + 16 + 5 + 22 + 2 + 7,
            "{format:?}"
        );
    }
}

#[test]
fn at_150_percent_every_size_of_the_color_row_is_scaled_on_its_own() {
    // At 150% the frame of 264 starts 44 right of and below the cursor, at (744, 544), and X and Y take a line of 24
    // (24.3) each, 11 below it, from 819. The row follows 8 (7.5) below them, at 875: two lines of 24 beside the swatch
    // of 33, which sits 8 down (7.5, rounded up) to be centered in their 48. The strip's text starts 6 in, at 750, and
    // the color's 33 + 12 after it, at 795, as wide as the 252 of text less those 45.
    let mut photo = item(DODGER_BLUE);
    photo.scale = Scale::from_dpi(144).expect("a real scale");
    let view = view(&open(photo, ColorFormat::Both));
    assert_eq!(view.color.swatch, rect(750, 875 + 8, 33, 33));
    assert_eq!(view.color.first.line, rect(795, 875, 207, 24));
    let second = view.color.second.expect("RGB under HEX");
    assert_eq!(second.line, rect(795, 875 + 24, 207, 24));
    // 11 + 264 + 11 + 2 × 24 + 8 + 48 + 3 + 11.
    assert_eq!(view.panel.height, 404);
}
