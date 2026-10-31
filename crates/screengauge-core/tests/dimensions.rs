//! Layer 1 tests of Dimensions (v1 spec WPC-296, Dimensions; ticket WPC-306): what it measures around the cursor on
//! frozen screens drawn here, rectangle by rectangle, without a screen. They look only at the live measurement the
//! session would draw and at the effects it returns.

use std::time::Duration;

use screengauge_core::session::{
    Colors, Effect, Event, Held, Image, Input, Key, Label, LocalTime, Measurement, Opening, Photo,
    Point, Rect, Scale, Session, Span,
};
use screengauge_core::settings::{DimensionsMode, Options, Remembered, Tool};

/// The owner's 4K monitor at 175%, Windows' main monitor.
const AOC: Rect = Rect {
    left: 0,
    top: 0,
    width: 3840,
    height: 2160,
};

const WHITE: [u8; 3] = [255, 255, 255];
const CARD_BLUE: [u8; 3] = [37, 99, 235];

/// A monitor's picture, painted by the tests one rectangle at a time. Colors are red, green, blue.
struct Canvas {
    rect: Rect,
    bgra: Vec<u8>,
}

impl Canvas {
    /// The monitor at `rect`, all in `color`.
    fn new(rect: Rect, color: [u8; 3]) -> Canvas {
        let pixels = usize::try_from(rect.width * rect.height).expect("a small monitor");
        Canvas {
            rect,
            bgra: [color[2], color[1], color[0], 255].repeat(pixels),
        }
    }

    /// Paints the pixels from (`left`, `top`) to (`right`, `bottom`), both included, in physical pixels of the
    /// virtual screen.
    fn paint(mut self, left: i32, top: i32, right: i32, bottom: i32, color: [u8; 3]) -> Canvas {
        for y in top..=bottom {
            for x in left..=right {
                self.set(x, y, color);
            }
        }
        self
    }

    fn set(&mut self, x: i32, y: i32, color: [u8; 3]) {
        let column = usize::try_from(x - self.rect.left).expect("a pixel on the monitor");
        let row = usize::try_from(y - self.rect.top).expect("a pixel on the monitor");
        let width = usize::try_from(self.rect.width).expect("a small monitor");
        let at = (row * width + column) * 4;
        self.bgra[at..at + 3].copy_from_slice(&[color[2], color[1], color[0]]);
    }

    fn photo(self, dpi: u32) -> Photo {
        Photo {
            rect: self.rect,
            scale: Scale::from_dpi(dpi).expect("a real scale"),
            colors: Colors::Exact,
            image: Image::new(self.rect.width, self.rect.height, self.bgra)
                .expect("the pixels fill the image"),
        }
    }
}

/// Dimensions in `mode`, the cursor at `cursor`, the default options.
fn open(frozen_screen: Vec<Photo>, mode: DimensionsMode, cursor: Point) -> Session {
    Session::open(Opening {
        frozen_screen: frozen_screen.into_iter().map(Some).collect(),
        cursor,
        options: Options::default(),
        remembered: Remembered {
            dimensions_mode: mode,
            ..Remembered::default()
        },
        second_click_until: None,
    })
    .expect("a frozen screen the session takes")
}

fn live(session: &Session) -> Option<Measurement> {
    session.scene().live.cloned()
}

fn at(x: i32, y: i32) -> Point {
    Point { x, y }
}

/// A label in physical pixels with the logical pixels after them, as the default options show it on a monitor whose
/// scale isn't 100% (ticket WPC-307).
fn label(physical: &str, logical: &str) -> Label {
    Label {
        physical: physical.to_owned(),
        logical: Some(logical.to_owned()),
    }
}

/// The span along row `y`, from column `first` to column `last`.
fn row(y: i32, first: i32, last: i32) -> Span {
    Span {
        first: at(first, y),
        last: at(last, y),
    }
}

/// The span along column `x`, from row `first` to row `last`.
fn column(x: i32, first: i32, last: i32) -> Span {
    Span {
        first: at(x, first),
        last: at(x, last),
    }
}

/// Two blue cards on the 4K monitor with a white strip of 24 pixels between them, columns 1000 to 1023.
fn two_cards() -> Photo {
    Canvas::new(AOC, WHITE)
        .paint(800, 500, 999, 700, CARD_BLUE)
        .paint(1024, 500, 1223, 700, CARD_BLUE)
        .photo(168)
}

#[test]
fn horizontal_measures_the_gap_between_two_cards_from_any_point_in_it() {
    for x in [1000, 1011, 1023] {
        let session = open(vec![two_cards()], DimensionsMode::Horizontal, at(x, 600));
        assert_eq!(
            live(&session),
            Some(Measurement {
                monitor: 0,
                horizontal: Some(row(600, 1000, 1023)),
                vertical: None,
                label: label("24", "13.7"),
            }),
            "cursor at x = {x}"
        );
    }
}

#[test]
fn vertical_measures_a_card_from_its_top_row_to_its_bottom_row() {
    // Inside the left card, rows 500 to 700: the white around it is the edge on each side.
    let session = open(vec![two_cards()], DimensionsMode::Vertical, at(900, 640));
    assert_eq!(
        live(&session),
        Some(Measurement {
            monitor: 0,
            horizontal: None,
            vertical: Some(column(900, 500, 700)),
            label: label("201", "114.9"),
        })
    );
}

#[test]
fn cross_measures_width_and_height_at_once_with_no_distance_to_each_edge() {
    // A white panel of 240 × 180, columns 2000 to 2239 and rows 1200 to 1379, on a blue page, with the cursor nearer
    // its top-left corner: only the two whole spans show, never how far the cursor is from each edge.
    let page = Canvas::new(AOC, CARD_BLUE)
        .paint(2000, 1200, 2239, 1379, WHITE)
        .photo(168);
    let session = open(vec![page], DimensionsMode::Cross, at(2010, 1250));
    assert_eq!(
        live(&session),
        Some(Measurement {
            monitor: 0,
            horizontal: Some(row(1250, 2000, 2239)),
            vertical: Some(column(2010, 1200, 1379)),
            label: label("240 × 180", "137.1 × 102.9"),
        })
    );
}

#[test]
fn an_item_of_one_pixel_measures_one() {
    // A dark line one pixel wide, column 1500, on white: from on it, it is its own span.
    let page = Canvas::new(AOC, WHITE)
        .paint(1500, 0, 1500, 2159, [17, 24, 39])
        .photo(168);
    let session = open(vec![page], DimensionsMode::Cross, at(1500, 900));
    assert_eq!(
        live(&session),
        Some(Measurement {
            monitor: 0,
            horizontal: Some(row(900, 1500, 1500)),
            vertical: Some(column(1500, 0, 2159)),
            label: label("1 × 2160", "0.6 × 1234.3"),
        })
    );
}

#[test]
fn with_no_edge_before_the_monitors_limit_the_span_runs_to_its_last_pixel_which_counts() {
    // A white strip from column 3000 to the 4K monitor's right limit and from row 1800 to its bottom: nothing stops the
    // line there, so it ends on the monitor's last column and last row, both counted.
    let page = Canvas::new(AOC, CARD_BLUE)
        .paint(3000, 1800, 3839, 2159, WHITE)
        .photo(168);
    let session = open(vec![page], DimensionsMode::Cross, at(3500, 2000));
    assert_eq!(
        live(&session),
        Some(Measurement {
            monitor: 0,
            horizontal: Some(row(2000, 3000, 3839)),
            vertical: Some(column(3500, 1800, 2159)),
            label: label("840 × 360", "480 × 205.7"),
        })
    );
}

#[test]
fn a_blank_monitor_measures_from_limit_to_limit() {
    let blank = Canvas::new(AOC, WHITE).photo(168);
    let session = open(vec![blank], DimensionsMode::Cross, at(0, 0));
    assert_eq!(
        live(&session).map(|measurement| measurement.label.physical),
        Some("3840 × 2160".to_owned())
    );
}

/// A gray ramp on the 4K monitor's row 1000: column 100 + n has the gray 100 + n, for n from 0 to 99, on white. Each
/// pixel differs from its neighbor by 1, far less than any tolerance.
fn ramp() -> Photo {
    let mut page = Canvas::new(AOC, WHITE);
    for n in 0..100_u8 {
        let x = 100 + i32::from(n);
        page = page.paint(x, 1000, x, 1000, [100 + n; 3]);
    }
    page.photo(168)
}

#[test]
fn in_a_gradient_the_edge_is_the_first_pixel_too_far_from_the_reference_color_not_from_its_neighbor()
 {
    // At the default tolerance, 2, a pixel counts as the same area while none of its red, green or blue is more than 6
    // from the reference color's (provisional until the calibration, WPC-332). From gray 150, at column 150, the span
    // runs from gray 144 to gray 156, 13 pixels, though no neighbor differs from the next by more than 1.
    let session = open(vec![ramp()], DimensionsMode::Horizontal, at(150, 1000));
    assert_eq!(
        live(&session),
        Some(Measurement {
            monitor: 0,
            horizontal: Some(row(1000, 144, 156)),
            vertical: None,
            label: label("13", "7.4"),
        })
    );
}

#[test]
fn a_tolerance_of_zero_stops_at_the_first_pixel_that_differs_at_all() {
    let session = Session::open(Opening {
        frozen_screen: vec![Some(ramp())],
        cursor: at(150, 1000),
        options: Options {
            default_tolerance: screengauge_core::settings::Tolerance::MIN,
            ..Options::default()
        },
        remembered: Remembered {
            dimensions_mode: DimensionsMode::Horizontal,
            ..Remembered::default()
        },
        second_click_until: None,
    })
    .expect("a frozen screen the session takes");
    assert_eq!(
        live(&session).and_then(|measurement| measurement.horizontal),
        Some(row(1000, 150, 150))
    );
}

/// The owner's Full HD monitor at 125%, to the left of the 4K.
const ARZOPA: Rect = Rect {
    left: -1920,
    top: 0,
    width: 1920,
    height: 1080,
};

/// `input` with the cursor at `cursor`, no modifier held, at 20:41:12 on 2026-10-26.
fn event(cursor: Point, input: Input) -> Event {
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
        uptime: Duration::from_millis(50),
        input,
    }
}

#[test]
fn moving_to_another_monitor_measures_there_on_that_monitor_alone() {
    // Both monitors blank and white, side by side: a span that ran on into the next monitor would mix pixels of two
    // sizes. On each monitor the line stops at its own limit, and the measurement says which monitor it is on.
    let monitors = vec![
        Canvas::new(AOC, WHITE).photo(168),
        Canvas::new(ARZOPA, WHITE).photo(120),
    ];
    let mut session = open(monitors, DimensionsMode::Cross, at(0, 500));
    assert_eq!(
        live(&session),
        Some(Measurement {
            monitor: 0,
            horizontal: Some(row(500, 0, 3839)),
            vertical: Some(column(0, 0, 2159)),
            label: label("3840 × 2160", "2194.3 × 1234.3"),
        }),
        "on the 4K monitor's first column"
    );
    assert_eq!(session.handle(event(at(-1, 1079), Input::Move)), []);
    assert_eq!(
        live(&session),
        Some(Measurement {
            monitor: 1,
            horizontal: Some(row(1079, -1920, -1)),
            vertical: Some(column(-1, 0, 1079)),
            label: label("1920 × 1080", "1536 × 864"),
        }),
        "on the Full HD monitor's last column and last row"
    );
}

#[test]
fn moving_the_mouse_measures_where_it_went() {
    let mut session = open(vec![two_cards()], DimensionsMode::Horizontal, at(900, 600));
    assert_eq!(
        live(&session).map(|measurement| measurement.label.physical),
        Some("200".to_owned()),
        "inside the left card"
    );
    session.handle(event(at(1005, 600), Input::Move));
    assert_eq!(
        live(&session).and_then(|measurement| measurement.horizontal),
        Some(row(600, 1000, 1023)),
        "in the strip between the cards"
    );
}

/// What the live measurement spans: (along the row, along the column).
fn spans(session: &Session) -> (Option<Span>, Option<Span>) {
    live(session).map_or((None, None), |measurement| {
        (measurement.horizontal, measurement.vertical)
    })
}

#[test]
fn h_v_and_c_change_the_mode_and_the_measurement_follows() {
    // Inside the left card, columns 800 to 999 and rows 500 to 700.
    let cursor = at(900, 600);
    let mut session = open(vec![two_cards()], DimensionsMode::Cross, cursor);
    let width = Some(row(600, 800, 999));
    let height = Some(column(900, 500, 700));
    assert_eq!(spans(&session), (width, height), "Cross");
    for (key, expected, label) in [
        (Key::H, (width, None), "200"),
        (Key::V, (None, height), "201"),
        (Key::C, (width, height), "200 × 201"),
        // The same key again changes nothing.
        (Key::C, (width, height), "200 × 201"),
    ] {
        assert_eq!(
            session.handle(event(cursor, Input::KeyDown(key))),
            [],
            "{key:?}"
        );
        assert_eq!(spans(&session), expected, "{key:?}");
        assert_eq!(
            live(&session).map(|measurement| measurement.label.physical),
            Some(label.to_owned()),
            "{key:?}"
        );
    }
}

#[test]
fn the_first_time_dimensions_measures_in_cross() {
    // Nothing remembered yet, as on the first call after installing.
    let session = Session::open(Opening {
        frozen_screen: vec![Some(two_cards())],
        cursor: at(900, 600),
        options: Options::default(),
        remembered: Remembered::default(),
        second_click_until: None,
    })
    .expect("a frozen screen the session takes");
    assert_eq!(
        spans(&session),
        (Some(row(600, 800, 999)), Some(column(900, 500, 700)))
    );
}

/// Remembered from earlier calls: Cross, with the rest of the remembered state away from its defaults.
fn remembered_in_cross() -> Remembered {
    Remembered {
        dimensions_mode: DimensionsMode::Cross,
        magnification: screengauge_core::settings::Magnification::X16,
        warned_about_other_copy: true,
        ..Remembered::default()
    }
}

fn open_remembering(remembered: Remembered) -> Session {
    Session::open(Opening {
        frozen_screen: vec![Some(two_cards())],
        cursor: at(900, 600),
        options: Options::default(),
        remembered,
        second_click_until: None,
    })
    .expect("a frozen screen the session takes")
}

#[test]
fn the_last_mode_is_remembered_whichever_way_the_tools_close() {
    for exit in [
        Input::KeyDown(Key::Esc),
        Input::GlobalShortcut,
        Input::CloseAsked,
        Input::OtherProgramInFront,
        Input::MonitorsChanged,
    ] {
        let mut session = open_remembering(remembered_in_cross());
        session.handle(event(at(900, 600), Input::KeyDown(Key::V)));
        // The tools close first, so the screen comes back at once; only the mode changed in what is remembered.
        assert_eq!(
            session.handle(event(at(900, 600), exit.clone())),
            [
                Effect::Close,
                Effect::SaveRemembered(Remembered {
                    dimensions_mode: DimensionsMode::Vertical,
                    ..remembered_in_cross()
                })
            ],
            "{exit:?}"
        );
    }
}

#[test]
fn a_mode_switched_back_before_closing_leaves_nothing_to_save() {
    let mut session = open_remembering(remembered_in_cross());
    for key in [Key::H, Key::V, Key::C] {
        session.handle(event(at(900, 600), Input::KeyDown(key)));
    }
    assert_eq!(
        session.handle(event(at(900, 600), Input::KeyDown(Key::Esc))),
        [Effect::Close]
    );
}

fn with_shift(mut event: Event) -> Event {
    event.held.shift = true;
    event
}

#[test]
fn an_arrow_moves_the_cursor_one_pixel_and_the_measurement_follows() {
    // From the left card's last column into the strip between the cards.
    let mut session = open(vec![two_cards()], DimensionsMode::Horizontal, at(999, 600));
    assert_eq!(
        session.handle(event(at(999, 600), Input::KeyDown(Key::Right))),
        [Effect::MoveCursor(at(1000, 600))]
    );
    assert_eq!(
        live(&session).map(|measurement| measurement.label.physical),
        Some("24".to_owned())
    );
    for (key, to) in [
        (Key::Down, at(1000, 601)),
        (Key::Left, at(999, 601)),
        (Key::Up, at(999, 600)),
    ] {
        let place = live(&session).and_then(|measurement| measurement.horizontal);
        assert_eq!(
            session.handle(event(at(999, 600), Input::KeyDown(key))),
            [Effect::MoveCursor(to)],
            "{key:?} after {place:?}"
        );
    }
}

#[test]
fn shift_and_an_arrow_move_the_cursor_ten_pixels() {
    let mut session = open(vec![two_cards()], DimensionsMode::Horizontal, at(990, 600));
    assert_eq!(
        session.handle(with_shift(event(at(990, 600), Input::KeyDown(Key::Right)))),
        [Effect::MoveCursor(at(1000, 600))]
    );
    assert_eq!(
        live(&session).and_then(|measurement| measurement.horizontal),
        Some(row(600, 1000, 1023))
    );
}

#[test]
fn arrows_held_down_move_on_from_where_the_last_one_put_the_cursor() {
    // A key held down repeats, and a repeat can come with the cursor's place from before Windows carried out the last
    // move. Each step goes on from where the session put the cursor, so none is lost.
    let mut session = open(vec![two_cards()], DimensionsMode::Horizontal, at(990, 600));
    for to in 991..=995 {
        assert_eq!(
            session.handle(event(at(990, 600), Input::KeyDown(Key::Right))),
            [Effect::MoveCursor(at(to, 600))]
        );
    }
}

#[test]
fn an_arrow_takes_the_cursor_onto_the_next_monitor_and_dimensions_measures_there() {
    let monitors = vec![
        Canvas::new(AOC, WHITE).photo(168),
        Canvas::new(ARZOPA, WHITE).photo(120),
    ];
    let mut session = open(monitors, DimensionsMode::Horizontal, at(0, 500));
    assert_eq!(
        session.handle(event(at(0, 500), Input::KeyDown(Key::Left))),
        [Effect::MoveCursor(at(-1, 500))]
    );
    assert_eq!(
        live(&session).map(|measurement| (measurement.monitor, measurement.horizontal)),
        Some((1, Some(row(500, -1920, -1))))
    );
}

#[test]
fn an_arrow_never_takes_the_cursor_off_every_monitor() {
    // Left of the 4K monitor below row 1079 there is no monitor: the Full HD one is only 1080 rows high. The cursor
    // stops at the 4K monitor's limit, which a step of ten reaches after five.
    let monitors = vec![
        Canvas::new(AOC, WHITE).photo(168),
        Canvas::new(ARZOPA, WHITE).photo(120),
    ];
    let mut session = open(monitors, DimensionsMode::Horizontal, at(5, 1500));
    assert_eq!(
        session.handle(with_shift(event(at(5, 1500), Input::KeyDown(Key::Left)))),
        [Effect::MoveCursor(at(0, 1500))]
    );
    assert_eq!(
        session.handle(event(at(0, 1500), Input::KeyDown(Key::Left))),
        [],
        "already at the limit: nothing to move"
    );
    assert_eq!(
        live(&session).map(|measurement| measurement.monitor),
        Some(0)
    );
}

#[test]
fn only_dimensions_measures() {
    // The other tools come with their own tickets; until then, when one of them was the last used, nothing measures.
    for last_tool in [Tool::Bounds, Tool::Inspect, Tool::Snip] {
        let session = open_remembering(Remembered {
            last_tool,
            ..Remembered::default()
        });
        assert_eq!(live(&session), None, "{last_tool:?}");
    }
    assert!(live(&open_remembering(Remembered::default())).is_some());
}

#[test]
fn h_v_and_c_change_nothing_while_another_tool_is_active() {
    // The mode buttons show only with Dimensions on (v1 spec WPC-296, Toolbar), and the keys follow them: with Inspect
    // the last tool used, the remembered mode stays as it was, so there is nothing to save.
    let mut session = open_remembering(Remembered {
        last_tool: Tool::Inspect,
        ..remembered_in_cross()
    });
    for key in [Key::H, Key::V] {
        assert_eq!(session.handle(event(at(900, 600), Input::KeyDown(key))), []);
    }
    assert_eq!(
        session.handle(event(at(900, 600), Input::KeyDown(Key::Esc))),
        [Effect::Close]
    );
}
