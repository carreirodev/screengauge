//! Layer 1 tests of the logical pixel beside every measurement (v1 spec WPC-296, Unidades e vários monitores; ticket
//! WPC-307): what the live label says on made-up arrangements of monitors and at every Windows scale, without a screen.
//! They look only at the label the session would draw.

use screengauge_core::session::{
    Colors, Event, Held, Image, Input, Label, LocalTime, Opening, Photo, Point, Rect, Scale,
    Session,
};
use screengauge_core::settings::{DimensionsMode, Options, Remembered};

const WHITE: [u8; 3] = [255, 255, 255];
const BUTTON_BLUE: [u8; 3] = [37, 99, 235];

/// A monitor at `rect` and `dpi` (96 is 100%), white, with a blue rectangle from (`left`, `top`) to (`right`,
/// `bottom`), both included, in physical pixels of the virtual screen.
fn monitor(rect: Rect, dpi: u32, [left, top, right, bottom]: [i32; 4]) -> Photo {
    let width = usize::try_from(rect.width).expect("a small monitor");
    let height = usize::try_from(rect.height).expect("a small monitor");
    let mut bgra = [WHITE[2], WHITE[1], WHITE[0], 255].repeat(width * height);
    for y in top..=bottom {
        for x in left..=right {
            let column = usize::try_from(x - rect.left).expect("a pixel on the monitor");
            let row = usize::try_from(y - rect.top).expect("a pixel on the monitor");
            let at = (row * width + column) * 4;
            bgra[at..at + 3].copy_from_slice(&[BUTTON_BLUE[2], BUTTON_BLUE[1], BUTTON_BLUE[0]]);
        }
    }
    Photo {
        rect,
        scale: Scale::from_dpi(dpi).expect("a real scale"),
        colors: Colors::Exact,
        image: Image::new(rect.width, rect.height, bgra).expect("the pixels fill the image"),
    }
}

/// A monitor of 400 × 100 at `dpi`, at the virtual screen's corner, with a button `width` physical pixels wide and
/// 41 high: columns 100 to 100 + width − 1, rows 30 to 70.
fn button(width: i32, dpi: u32) -> Photo {
    let rect = Rect {
        left: 0,
        top: 0,
        width: 400,
        height: 100,
    };
    monitor(rect, dpi, [100, 30, 100 + width - 1, 70])
}

fn at(x: i32, y: i32) -> Point {
    Point { x, y }
}

/// Dimensions in `mode` with the cursor at `cursor` and the options `options`.
fn open_with(
    frozen_screen: Vec<Photo>,
    mode: DimensionsMode,
    cursor: Point,
    options: Options,
) -> Session {
    Session::open(Opening {
        frozen_screen: frozen_screen.into_iter().map(Some).collect(),
        cursor,
        options,
        remembered: Remembered {
            dimensions_mode: mode,
            ..Remembered::default()
        },
    })
    .expect("a frozen screen the session takes")
}

/// Dimensions in `mode` with the cursor at `cursor` and the default options, where "Also show logical pixels" is on.
fn open(frozen_screen: Vec<Photo>, mode: DimensionsMode, cursor: Point) -> Session {
    open_with(frozen_screen, mode, cursor, Options::default())
}

/// What the live label says.
fn label(session: &Session) -> Label {
    session
        .scene()
        .live
        .expect("Dimensions measures around the cursor")
        .label
        .clone()
}

/// The label for `physical`, with `logical` after the bar.
fn reads(physical: &str, logical: &str) -> Label {
    Label {
        physical: physical.to_owned(),
        logical: Some(logical.to_owned()),
    }
}

#[test]
fn a_button_of_150_physical_pixels_at_150_percent_shows_100_beside_it() {
    let session = open(
        vec![button(150, 144)],
        DimensionsMode::Horizontal,
        at(170, 50),
    );
    assert_eq!(label(&session), reads("150", "100"));
}

/// The label for `physical` alone, with no bar and no logical pixels.
fn alone(physical: &str) -> Label {
    Label {
        physical: physical.to_owned(),
        logical: None,
    }
}

#[test]
fn at_100_percent_the_label_has_only_the_physical_pixels() {
    let session = open(
        vec![button(150, 96)],
        DimensionsMode::Horizontal,
        at(170, 50),
    );
    assert_eq!(label(&session), alone("150"));
}

#[test]
fn the_logical_pixels_have_one_decimal_place_after_a_point_and_none_when_whole() {
    // At 175%: 200 / 1.75 = 114.29, 1 / 1.75 = 0.57, 140 / 1.75 = 80.
    for (width, logical) in [(200, "114.3"), (1, "0.6"), (140, "80")] {
        let session = open(
            vec![button(width, 168)],
            DimensionsMode::Horizontal,
            at(100, 50),
        );
        assert_eq!(
            label(&session),
            reads(&width.to_string(), logical),
            "{width} physical pixels"
        );
    }
}

#[test]
fn a_logical_pixel_halfway_between_two_tenths_rounds_up() {
    // At 400% a logical pixel is 4 physical pixels, so odd widths land on a half tenth: 1 / 4 = 0.25 and 5 / 4 = 1.25
    // round up to 0.3 and 1.3, where rounding halves to even would give 0.2 and 1.2, and cutting off would too.
    for (width, logical) in [(1, "0.3"), (5, "1.3"), (3, "0.8"), (2, "0.5")] {
        let session = open(
            vec![button(width, 384)],
            DimensionsMode::Horizontal,
            at(100, 50),
        );
        assert_eq!(
            label(&session),
            reads(&width.to_string(), logical),
            "{width} physical pixels"
        );
    }
}

#[test]
fn every_windows_scale_divides_the_same_button() {
    // Each scale Windows offers in Settings, as Windows reports it in DPI (96 is 100%), and 150 physical pixels divided
    // by it, worked out by hand to the nearest tenth.
    let scales = [
        (96, None),
        (120, Some("120")),
        (144, Some("100")),
        (168, Some("85.7")),
        (192, Some("75")),
        (216, Some("66.7")),
        (240, Some("60")),
        (288, Some("50")),
        (336, Some("42.9")),
        (384, Some("37.5")),
        (432, Some("33.3")),
        (480, Some("30")),
    ];
    for (dpi, logical) in scales {
        let session = open(
            vec![button(150, dpi)],
            DimensionsMode::Horizontal,
            at(170, 50),
        );
        assert_eq!(
            label(&session),
            Label {
                physical: "150".to_owned(),
                logical: logical.map(str::to_owned),
            },
            "at {dpi} DPI"
        );
    }
}

#[test]
fn the_cross_label_reads_width_by_height_in_both_units() {
    // A panel of 240 × 180 physical pixels at 150%.
    let rect = Rect {
        left: 0,
        top: 0,
        width: 600,
        height: 400,
    };
    let panel = monitor(rect, 144, [100, 100, 339, 279]);
    let session = open(vec![panel], DimensionsMode::Cross, at(200, 150));
    assert_eq!(label(&session), reads("240 × 180", "160 × 120"));
}

/// The mouse moved to `cursor`, with no modifier held, at 20:41:12 on 2026-10-26.
fn move_to(cursor: Point) -> Event {
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
        input: Input::Move,
    }
}

/// A 4K monitor at 175%, Windows' main monitor, with a button of 350 × 140 physical pixels.
fn four_k_at_175() -> Photo {
    let rect = Rect {
        left: 0,
        top: 0,
        width: 3840,
        height: 2160,
    };
    monitor(rect, 168, [1000, 600, 1349, 739])
}

/// A Full HD monitor at 125% to the left of the 4K, with a button of 350 × 140 physical pixels too.
fn full_hd_at_125() -> Photo {
    let rect = Rect {
        left: -1920,
        top: 0,
        width: 1920,
        height: 1080,
    };
    monitor(rect, 120, [-1500, 300, -1151, 439])
}

#[test]
fn the_same_button_on_two_monitors_is_divided_by_the_scale_of_the_monitor_it_is_on() {
    // 350 × 140 physical pixels are 200 × 80 logical pixels at 175% and 280 × 112 at 125%.
    let mut session = open(
        vec![four_k_at_175(), full_hd_at_125()],
        DimensionsMode::Cross,
        at(1100, 650),
    );
    assert_eq!(
        label(&session),
        reads("350 × 140", "200 × 80"),
        "on the 4K at 175%"
    );
    session.handle(move_to(at(-1400, 350)));
    assert_eq!(
        label(&session),
        reads("350 × 140", "280 × 112"),
        "on the Full HD at 125%"
    );
    session.handle(move_to(at(1100, 650)));
    assert_eq!(
        label(&session),
        reads("350 × 140", "200 × 80"),
        "back on the 4K"
    );
}

#[test]
fn moving_from_a_scaled_monitor_to_one_at_100_percent_drops_the_logical_pixels() {
    // The Full HD at 100% this time, beside the 4K at 175%.
    let mut full_hd = full_hd_at_125();
    full_hd.scale = Scale::HUNDRED_PERCENT;
    let mut session = open(
        vec![four_k_at_175(), full_hd],
        DimensionsMode::Horizontal,
        at(1100, 650),
    );
    assert_eq!(label(&session), reads("350", "200"), "on the 4K at 175%");
    session.handle(move_to(at(-1400, 350)));
    assert_eq!(label(&session), alone("350"), "on the Full HD at 100%");
}

#[test]
fn with_also_show_logical_pixels_off_the_label_has_only_the_physical_pixels_at_any_scale() {
    let off = Options {
        also_show_logical_pixels: false,
        ..Options::default()
    };
    for dpi in [120, 144, 168, 480] {
        let session = open_with(
            vec![button(150, dpi)],
            DimensionsMode::Horizontal,
            at(170, 50),
            off.clone(),
        );
        assert_eq!(label(&session), alone("150"), "at {dpi} DPI");
    }
}
