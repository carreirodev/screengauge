//! Layer 1 tests of the Approximate color notice (v1 spec WPC-296, Inspect and Colors and HDR; ticket WPC-328): Inspect
//! says "Approximate color (HDR on)" in its magnified view's strip, under the color, while the view is on a monitor
//! whose photo didn't come through the exact path, and on that monitor alone. They start from each monitor's exact path
//! mark, on two monitors side by side, one in each case, and look only at the scene the session would draw and at the
//! effects it returns, without a screen.

use std::time::Duration;

use screengauge_core::session::{
    ApproximateColor, Colors, Effect, Event, Held, Image, Input, LocalTime, MagnifiedView, Message,
    NoticeLine, Opening, Photo, Point, Rect, Scale, Session,
};
use screengauge_core::settings::{Options, Remembered, Tool};

/// Windows' main monitor, Full HD, whose photo came through the exact path.
const EXACT: Rect = Rect {
    left: 0,
    top: 0,
    width: 1920,
    height: 1080,
};

/// The Full HD monitor to its right, with HDR on and a photo that didn't: GDI's, or its SDR white level unread.
const APPROXIMATE: Rect = Rect {
    left: 1920,
    top: 0,
    width: 1920,
    height: 1080,
};

/// Far from every side of the approximate monitor and below the toolbar.
const ON_APPROXIMATE: Point = Point { x: 2820, y: 600 };

/// The same place on the exact monitor.
const ON_EXACT: Point = Point { x: 900, y: 600 };

/// A white monitor at `rect` and `dpi` (96 is 100%), with its photo's colors marked `colors`. White is also what HDR
/// content brighter than any standard color is cut to.
fn monitor(rect: Rect, dpi: u32, colors: Colors) -> Photo {
    let width = usize::try_from(rect.width).expect("a small monitor");
    let height = usize::try_from(rect.height).expect("a small monitor");
    Photo {
        rect,
        scale: Scale::from_dpi(dpi).expect("a real scale"),
        colors,
        image: Image::new(rect.width, rect.height, [255; 4].repeat(width * height))
            .expect("the pixels fill the image"),
    }
}

/// The two monitors at 100%: the exact one on the left, the approximate one on the right.
fn both() -> Vec<Photo> {
    vec![
        monitor(EXACT, 96, Colors::Exact),
        monitor(APPROXIMATE, 96, Colors::Approximate),
    ]
}

/// `tool` on `monitors`, with the cursor at `cursor`.
fn open(monitors: Vec<Photo>, tool: Tool, cursor: Point) -> Session {
    Session::open(Opening {
        frozen_screen: monitors.into_iter().map(Some).collect(),
        cursor,
        options: Options::default(),
        remembered: Remembered {
            last_tool: tool,
            ..Remembered::default()
        },
        second_click_until: None,
    })
    .expect("a frozen screen the session takes")
}

/// `input` with the cursor at `cursor`, `milliseconds` after Windows started.
fn event(milliseconds: u64, cursor: Point, input: Input) -> Event {
    Event {
        cursor,
        held: Held::default(),
        time: LocalTime {
            year: 2026,
            month: 11,
            day: 3,
            hour: 18,
            minute: 0,
            second: 0,
        },
        uptime: Duration::from_millis(milliseconds),
        input,
    }
}

/// The magnified view the session shows now.
fn view(session: &Session) -> MagnifiedView {
    session
        .scene()
        .magnified_view
        .expect("Inspect shows its magnified view")
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
fn on_the_approximate_monitor_the_strip_says_approximate_color_hdr_on_on_two_lines_under_the_color()
{
    // At 100% and 8×, the frame of 168 starts 22 + 7 right of and below the cursor, at (2849, 629). The strip's text
    // starts 4 in, at 2853, 160 wide; X and Y take a line of 16 from 804, and the color's row, HEX above RGB, two lines
    // of 16 from 825 to 857. The notice follows 4 below, at 861: two lines of 15 (11px text at a line height of 1.35,
    // 14.85), with the sign of 13 at the strip's left, 9 down (8.5, rounded up) to be centered in their 30, and the
    // text 13 + 5 after it, at 2871, as wide as the 160 less those 18.
    let view = view(&open(both(), Tool::Inspect, ON_APPROXIMATE));
    assert_eq!(
        view.approximate_color,
        Some(ApproximateColor {
            sign: rect(2853, 861 + 9, 13, 13),
            lines: [
                NoticeLine {
                    text: "Approximate color",
                    line: rect(2871, 861, 142, 15),
                },
                NoticeLine {
                    text: "(HDR on)",
                    line: rect(2871, 861 + 15, 142, 15),
                },
            ],
        })
    );
    // 7 + 168 + 7 + 16 + 5 + 2 × 16, then 4 + 2 × 15 for the notice, and 2 + 7.
    assert_eq!(view.panel.height, 278);
}

#[test]
fn the_notice_goes_when_the_cursor_moves_to_the_exact_monitor_and_comes_back_with_it() {
    let mut session = open(both(), Tool::Inspect, ON_APPROXIMATE);
    session.handle(event(1000, ON_EXACT, Input::Move));
    let exact = view(&session);
    assert_eq!(exact.monitor, 0);
    // The exact monitor is white, as HDR content is cut to: the nearest standard color, with no mark.
    assert_eq!(exact.color.first.text, "#FFFFFF");
    assert_eq!(exact.approximate_color, None);
    // The strip ends with the color: 7 + 168 + 7 + 16 + 5 + 2 × 16 + 2 + 7.
    assert_eq!(exact.panel.height, 244);

    session.handle(event(1100, ON_APPROXIMATE, Input::Move));
    let approximate = view(&session);
    assert_eq!(approximate.monitor, 1);
    assert!(approximate.approximate_color.is_some());
    assert_eq!(approximate.panel.height, 278);
}

#[test]
fn the_notice_shows_on_the_approximate_monitor_alone_whichever_monitor_the_tools_opened_on() {
    let mut session = open(both(), Tool::Inspect, ON_EXACT);
    assert_eq!(view(&session).approximate_color, None);
    // The approximate monitor's first column.
    let first_column = Point {
        x: APPROXIMATE.left,
        y: 600,
    };
    session.handle(event(1000, first_column, Input::Move));
    assert!(view(&session).approximate_color.is_some());
    // The exact monitor's last column, one pixel to the left.
    let last_column = Point {
        x: APPROXIMATE.left - 1,
        y: 600,
    };
    session.handle(event(1100, last_column, Input::Move));
    assert_eq!(view(&session).approximate_color, None);
}

#[test]
fn the_click_on_the_approximate_monitor_still_copies_the_color_and_copied_shows_at_the_cursor() {
    let mut session = open(both(), Tool::Inspect, ON_APPROXIMATE);
    let effects = session.handle(event(1000, ON_APPROXIMATE, Input::Press));
    session.handle(event(1050, ON_APPROXIMATE, Input::Release));
    assert!(
        effects.contains(&Effect::CopyText("#FFFFFF".to_owned())),
        "{effects:?}"
    );
    // Copied is a notice like any other, at the cursor, which goes by itself; the strip keeps the Approximate color one.
    let scene = session.scene();
    let notice = scene.notice.copied().expect("a notice");
    assert_eq!(notice.message, Message::Copied);
    assert_eq!(notice.at, ON_APPROXIMATE);
    assert!(
        scene
            .magnified_view
            .expect("Inspect shows its magnified view")
            .approximate_color
            .is_some()
    );
}

#[test]
fn dimensions_bounds_and_snip_show_nothing_of_it_on_the_approximate_monitor() {
    for tool in [Tool::Dimensions, Tool::Bounds, Tool::Snip] {
        let mut session = open(both(), tool, ON_APPROXIMATE);
        session.handle(event(1000, Point { x: 2830, y: 610 }, Input::Move));
        let scene = session.scene();
        assert_eq!(scene.magnified_view, None, "{tool:?}");
        assert_eq!(scene.notice, None, "{tool:?}");
    }
}

#[test]
fn the_taller_panel_goes_above_the_cursor_where_the_one_without_the_notice_still_fits_below() {
    // With the cursor at 800 the panel would start at 822. Without the notice its 244 end at 1066, inside the 4 left
    // before the monitor's bottom; with it, its 278 would reach 1100, so it goes 22 above the cursor instead.
    let on_exact = Point { x: 900, y: 800 };
    let on_approximate = Point { x: 2820, y: 800 };
    let exact = view(&open(both(), Tool::Inspect, on_exact));
    assert_eq!((exact.panel.top, exact.panel.height), (822, 244));
    let approximate = view(&open(both(), Tool::Inspect, on_approximate));
    assert_eq!(
        (approximate.panel.top, approximate.panel.height),
        (800 - 22 - 278, 278)
    );
}

#[test]
fn at_150_percent_every_size_of_the_notice_is_scaled_on_its_own() {
    // At 150% and 8× the frame of 264 starts 33 + 11 right of and below the cursor, at (2864, 444). X and Y take a line
    // of 24 each, 11 below it, from 719, and the color's row its two lines of 24 from 775 (8 below them) to 823. The
    // notice follows 6 below, at 829: two lines of 22 (22.275), the sign of 20 (19.5) 12 down to be centered in their
    // 44, and the text 20 + 8 (7.5) after the strip's text's left, 2870, as wide as its 252 less those 28.
    let monitors = vec![monitor(APPROXIMATE, 144, Colors::Approximate)];
    let view = view(&open(monitors, Tool::Inspect, Point { x: 2820, y: 400 }));
    let notice = view.approximate_color.expect("the notice");
    assert_eq!(notice.sign, rect(2870, 829 + 12, 20, 20));
    assert_eq!(notice.lines[0].line, rect(2898, 829, 224, 22));
    assert_eq!(notice.lines[1].line, rect(2898, 829 + 22, 224, 22));
    // 11 + 264 + 11 + 2 × 24 + 8 + 2 × 24, then 6 + 2 × 22 for the notice, and 3 + 11.
    assert_eq!(view.panel.height, 454);
}
