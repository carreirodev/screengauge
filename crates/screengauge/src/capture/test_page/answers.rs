//! The approved answers of the test page samples (v1 spec WPC-296, test layer 1; ticket WPC-332): each case of
//! `test-page/test-page.html`, measured with Dimensions and Bounds on the three samples in `test-page/samples/` at the
//! default tolerance, through the session core as the tools measure the real screen, and the answer the owner approved
//! for it once. Snip takes the same area as Bounds, so each Bounds case checks that Snip's image is the sample's
//! pixels in the rectangle Bounds measures.
//!
//! A change of rule or of the default tolerance that changes an answer fails here, with every changed answer listed
//! next to the approved one: the new answers go back to the owner, and only their approval writes them into the table.
//!
//! The session is layer 1 and makes no Windows call; only reading the samples' PNGs uses WIC, which is why the tests
//! live here and not in `screengauge-core`. `test-page/README.md` lists the cases and ADR 0005 records the calibration.

use std::time::Duration;

use screengauge_core::session::{
    Colors, Effect, Event, Held, Image, Input, LocalTime, Opening, Photo, Point, Rect, Scale,
    Session,
};
use screengauge_core::settings::{DimensionsMode, Options, Remembered, Tolerance, Tool};

use super::test_page_folder;
use crate::png;

/// The samples, in the order of each case's answers, with their monitors' DPI: the AOC at 175% and the ARZOPA at 125%
/// and 100%.
const SAMPLES: [(&str, u32); 3] = [
    ("3840x2160-175.png", 168),
    ("1920x1080-125.png", 120),
    ("1920x1080-100.png", 96),
];

/// What a case does, in CSS px of the page's stage, as test-page.html places its elements.
enum Gesture {
    /// Dimensions in this mode, with the cursor at this point.
    Dimensions(DimensionsMode, (i32, i32)),
    /// A Bounds drag, and a Snip one, from the first point to the second; the first is the reference color's.
    Bounds((i32, i32), (i32, i32)),
}

struct Case {
    /// The element of test-page.html, and where on or around it the case measures.
    name: &'static str,
    gesture: Gesture,
    /// The approved answer on each sample, in the order of [`SAMPLES`]. Dimensions': the label's number, then the
    /// pixels its line runs over, from the first to the last, along the cursor's row and its column, in physical pixels
    /// of the sample. Bounds': the text a release copies and the result's top left corner, or the notice of an empty
    /// area.
    answers: [&'static str; 3],
}

/// The default tolerance the answers were approved at (WPC-332).
const APPROVED_AT: u8 = 6;

#[test]
fn the_answers_were_approved_at_the_default_tolerance() {
    // A new default changes answers, which go back to the owner: this names the cause before the answers fail.
    assert_eq!(Tolerance::default().get(), APPROVED_AT);
}

#[test]
fn every_case_of_the_test_page_gives_the_answer_the_owner_approved() {
    crate::com::ensure_started().expect("COM starts on a test thread");
    let mut changed = Vec::new();
    for (index, &(name, dpi)) in SAMPLES.iter().enumerate() {
        let photo = sample(name, dpi);
        for case in &CASES {
            let now = answer(&photo, &case.gesture, dpi);
            let approved = case.answers[index];
            if now != approved {
                changed.push(format!(
                    "{} on {name}: approved \"{approved}\", now \"{now}\"",
                    case.name
                ));
            }
        }
    }
    assert!(
        changed.is_empty(),
        "{} answers changed; each new answer goes back to the owner (WPC-332):\n{}",
        changed.len(),
        changed.join("\n")
    );
}

#[test]
fn the_table_has_a_case_for_each_kind_the_page_draws() {
    // 19 Dimensions cases and 37 Bounds cases: cards, text, shadows, gradients and 1-pixel lines (test-page/README.md).
    let dimensions = CASES
        .iter()
        .filter(|case| matches!(case.gesture, Gesture::Dimensions(..)))
        .count();
    assert_eq!((dimensions, CASES.len() - dimensions), (19, 37));
}

/// The sample `name` as the photo of a monitor at `dpi` whose top left corner is the virtual screen's origin.
fn sample(name: &str, dpi: u32) -> Photo {
    let bytes = std::fs::read(test_page_folder().join("samples").join(name)).expect("the sample");
    let decoded = png::read_back::decode(&bytes);
    let image = Image::new(
        decoded.width,
        decoded.height,
        decoded.bgra.iter().flatten().copied().collect(),
    )
    .expect("the pixels fill the sample");
    Photo {
        rect: Rect {
            left: 0,
            top: 0,
            width: decoded.width,
            height: decoded.height,
        },
        scale: Scale::from_dpi(dpi).expect("a Windows scale"),
        colors: Colors::Exact,
        image,
    }
}

/// A plain white monitor at 100% right of the sample, where the toolbar opens, so it covers no case: it stays there
/// while the tools measure the sample.
fn toolbar_monitor(page: &Photo) -> Photo {
    let rect = Rect {
        left: i32::try_from(page.rect.width).expect("a small sample"),
        top: 0,
        width: 800,
        height: 600,
    };
    let pixels = usize::try_from(rect.width * rect.height).expect("a small monitor");
    Photo {
        rect,
        scale: Scale::from_dpi(96).expect("100%"),
        colors: Colors::Exact,
        image: Image::new(rect.width, rect.height, [255; 4].repeat(pixels))
            .expect("the pixels fill the image"),
    }
}

/// A point of the stage, `x` and `y` in CSS px, at `dpi`: the physical pixel of the sample it falls in, so rounded
/// down.
fn physical((x, y): (i32, i32), dpi: u32) -> Point {
    let dpi = i32::try_from(dpi).expect("a Windows DPI");
    Point {
        x: x * dpi / 96,
        y: y * dpi / 96,
    }
}

fn event(cursor: Point, milliseconds: u64, input: Input) -> Event {
    Event {
        cursor,
        held: Held::default(),
        time: LocalTime {
            year: 2026,
            month: 11,
            day: 3,
            hour: 19,
            minute: 0,
            second: 0,
        },
        uptime: Duration::from_millis(milliseconds),
        input,
    }
}

/// The tools opened with `tool` and Dimensions in `mode`, at the default options, with the cursor on the toolbar's
/// monitor.
fn open(page: &Photo, tool: Tool, mode: DimensionsMode) -> Session {
    let parking = toolbar_monitor(page);
    let cursor = Point {
        x: parking.rect.left + 400,
        y: 500,
    };
    Session::open(Opening {
        frozen_screen: vec![Some(page.clone()), Some(parking)],
        cursor,
        options: Options::default(),
        remembered: Remembered {
            last_tool: tool,
            dimensions_mode: mode,
            ..Remembered::default()
        },
        second_click_until: None,
    })
    .expect("a frozen screen the session takes")
}

/// What `gesture` answers on `page`, a sample at `dpi`, in the form of [`Case::answers`].
fn answer(page: &Photo, gesture: &Gesture, dpi: u32) -> String {
    match *gesture {
        Gesture::Dimensions(mode, at) => dimensions(page, mode, physical(at, dpi)),
        Gesture::Bounds(from, to) => {
            let (from, to) = (physical(from, dpi), physical(to, dpi));
            let (bounds, content) = bounds(page, from, to);
            // Snip takes the same area: its image is the sample's pixels in the rectangle Bounds measures, and there
            // is none where Bounds finds no content.
            let snip = snip(page, from, to);
            let wanted = content.map(|content| crop(page, content));
            if snip == wanted {
                bounds
            } else {
                let taken = snip.map_or("no image".to_owned(), |image| {
                    format!("an image of {} x {}", image.width(), image.height())
                });
                format!("{bounds}, but Snip takes {taken} that isn't the sample's pixels there")
            }
        }
    }
}

fn dimensions(page: &Photo, mode: DimensionsMode, cursor: Point) -> String {
    let mut session = open(page, Tool::Dimensions, mode);
    session.handle(event(cursor, 1000, Input::Move));
    let Some(live) = session.scene().live else {
        return "nothing measured".to_owned();
    };
    let row = live
        .horizontal
        .map(|span| format!("row {}..{}", span.first.x, span.last.x));
    let column = live
        .vertical
        .map(|span| format!("column {}..{}", span.first.y, span.last.y));
    let places: Vec<String> = [row, column].into_iter().flatten().collect();
    format!("{}: {}", live.label.physical, places.join(", "))
}

/// Presses at `from`, drags to `to` and lets go there: what the release returns.
fn drag(session: &mut Session, from: Point, to: Point) -> Vec<Effect> {
    session.handle(event(from, 1000, Input::Move));
    session.handle(event(from, 1010, Input::Press));
    session.handle(event(to, 1020, Input::Move));
    session.handle(event(to, 1030, Input::Release))
}

/// Bounds' answer for a drag from `from` to `to`, and the rectangle it pinned.
fn bounds(page: &Photo, from: Point, to: Point) -> (String, Option<Rect>) {
    let mut session = open(page, Tool::Bounds, DimensionsMode::default());
    let effects = drag(&mut session, from, to);
    let copied = effects.iter().find_map(|effect| match effect {
        Effect::CopyText(text) => Some(text.clone()),
        _ => None,
    });
    let pinned = session
        .scene()
        .pinned
        .last()
        .and_then(|measurement| measurement.content);
    let answer = match (copied, pinned) {
        (Some(text), Some(content)) => format!("{text} at {},{}", content.left, content.top),
        (None, None) => session
            .scene()
            .notice
            .map_or("nothing", |notice| notice.message.text())
            .to_owned(),
        (copied, pinned) => format!("copied {copied:?} but pinned {pinned:?}"),
    };
    (answer, pinned)
}

/// The pixels of `page` in `rect`, which lies on it, as an image of their own.
fn crop(page: &Photo, rect: Rect) -> Image {
    let width = usize::try_from(page.rect.width).expect("a small sample");
    let (left, top) = (
        usize::try_from(rect.left).expect("on the sample"),
        usize::try_from(rect.top).expect("on the sample"),
    );
    let columns = usize::try_from(rect.width).expect("a small rectangle");
    let rows = usize::try_from(rect.height).expect("a small rectangle");
    let bgra = (top..top + rows)
        .flat_map(|row| {
            let start = (row * width + left) * 4;
            page.image.bgra()[start..start + columns * 4]
                .iter()
                .copied()
        })
        .collect();
    Image::new(rect.width, rect.height, bgra).expect("the pixels fill the image")
}

/// The image a Snip drag from `from` to `to` copies, if any.
fn snip(page: &Photo, from: Point, to: Point) -> Option<Image> {
    let mut session = open(page, Tool::Snip, DimensionsMode::default());
    drag(&mut session, from, to)
        .into_iter()
        .find_map(|effect| match effect {
            Effect::CopyImage(image) => Some(image),
            _ => None,
        })
}

/// Every case, with the answers the owner approved at the default tolerance, 6, on 2026-11-03 (WPC-332). Each Bounds
/// drag starts on the background around its element, except where its name says otherwise.
const CASES: [Case; 56] = [
    Case {
        name: "gap-24 (white strip between two cards)",
        gesture: Gesture::Dimensions(DimensionsMode::Horizontal, (356, 84)),
        answers: ["42: row 602..643", "30: row 430..459", "24: row 344..367"],
    },
    Case {
        name: "card-200x120",
        gesture: Gesture::Dimensions(DimensionsMode::Cross, (124, 84)),
        answers: [
            "350 × 210: row 42..391, column 42..251",
            "250 × 150: row 30..279, column 30..179",
            "200 × 120: row 24..223, column 24..143",
        ],
    },
    Case {
        name: "card-75x45-offgrid",
        gesture: Gesture::Dimensions(DimensionsMode::Cross, (526, 51)),
        answers: [
            "131 × 79: row 856..986, column 51..129",
            "94 × 57: row 611..704, column 36..92",
            "75 × 45: row 489..563, column 29..73",
        ],
    },
    Case {
        name: "card-rounded (center)",
        gesture: Gesture::Dimensions(DimensionsMode::Cross, (668, 84)),
        answers: [
            "280 × 210: row 1029..1308, column 42..251",
            "200 × 150: row 735..934, column 30..179",
            "160 × 120: row 588..747, column 24..143",
        ],
    },
    Case {
        name: "card-outlined (inside the border)",
        gesture: Gesture::Dimensions(DimensionsMode::Cross, (852, 84)),
        answers: [
            "278 × 208: row 1352..1629, column 43..250",
            "198 × 148: row 966..1163, column 31..178",
            "158 × 118: row 773..930, column 25..142",
        ],
    },
    Case {
        name: "card-light-gray",
        gesture: Gesture::Dimensions(DimensionsMode::Cross, (1056, 84)),
        answers: [
            "350 × 210: row 1673..2022, column 42..251",
            "250 × 150: row 1195..1444, column 30..179",
            "200 × 120: row 956..1155, column 24..143",
        ],
    },
    Case {
        name: "lines-gap-3 (between the lines)",
        gesture: Gesture::Dimensions(DimensionsMode::Horizontal, (810, 660)),
        answers: ["5: row 1416..1420", "4: row 1011..1014", "3: row 809..811"],
    },
    Case {
        name: "card-on-gradient",
        gesture: Gesture::Dimensions(DimensionsMode::Cross, (596, 556)),
        answers: [
            "168 × 98: row 959..1126, column 924..1021",
            "120 × 70: row 685..804, column 660..729",
            "96 × 56: row 548..643, column 528..583",
        ],
    },
    Case {
        name: "box-css-outline (inside)",
        gesture: Gesture::Dimensions(DimensionsMode::Cross, (928, 660)),
        answers: [
            "278 × 124: row 1485..1762, column 1093..1216",
            "198 × 88: row 1061..1258, column 781..868",
            "158 × 70: row 849..1006, column 625..694",
        ],
    },
    Case {
        name: "box-device-outline (inside)",
        gesture: Gesture::Dimensions(DimensionsMode::Cross, (1104, 660)),
        answers: [
            "250 × 124: row 1807..2056, column 1093..1216",
            "178 × 88: row 1291..1468, column 781..868",
            "142 × 70: row 1033..1174, column 625..694",
        ],
    },
    Case {
        name: "dark band (above the text)",
        gesture: Gesture::Dimensions(DimensionsMode::Horizontal, (1150, 172)),
        answers: [
            "994: row 1064..2057",
            "710: row 760..1469",
            "568: row 608..1175",
        ],
    },
    Case {
        name: "dark band (right of the text, V)",
        gesture: Gesture::Dimensions(DimensionsMode::Vertical, (1150, 196)),
        answers: [
            "98: column 294..391",
            "70: column 210..279",
            "56: column 168..223",
        ],
    },
    Case {
        name: "white between line-h-css-black and -offgrid",
        gesture: Gesture::Dimensions(DimensionsMode::Vertical, (900, 562)),
        answers: [
            "31: column 968..998",
            "23: column 691..713",
            "18: column 553..570",
        ],
    },
    Case {
        name: "white between line-h-device and box rows",
        gesture: Gesture::Dimensions(DimensionsMode::Vertical, (900, 614)),
        answers: [
            "34: column 1058..1091",
            "24: column 756..779",
            "19: column 605..623",
        ],
    },
    Case {
        name: "gradient-strong (middle)",
        gesture: Gesture::Dimensions(DimensionsMode::Horizontal, (204, 600)),
        answers: ["74: row 321..394", "54: row 229..282", "44: row 183..226"],
    },
    Case {
        name: "gradient-subtle (left part)",
        gesture: Gesture::Dimensions(DimensionsMode::Horizontal, (40, 512)),
        answers: ["413: row 42..454", "301: row 30..330", "239: row 24..262"],
    },
    Case {
        name: "gradient-vertical (middle)",
        gesture: Gesture::Dimensions(DimensionsMode::Vertical, (440, 556)),
        answers: [
            "62: column 938..999",
            "44: column 672..715",
            "34: column 540..573",
        ],
    },
    Case {
        name: "shadow-large (inside the white card)",
        gesture: Gesture::Dimensions(DimensionsMode::Cross, (956, 456)),
        answers: [
            "210 × 140: row 1568..1777, column 728..867",
            "150 × 100: row 1120..1269, column 520..619",
            "120 × 80: row 896..1015, column 416..495",
        ],
    },
    Case {
        name: "card-light-gray from the white above it",
        gesture: Gesture::Dimensions(DimensionsMode::Vertical, (1056, 156)),
        answers: [
            "42: column 252..293",
            "30: column 180..209",
            "24: column 144..167",
        ],
    },
    Case {
        name: "card-200x120",
        gesture: Gesture::Bounds((16, 16), (232, 152)),
        answers: [
            "350 x 210 at 42,42",
            "250 x 150 at 30,30",
            "200 x 120 at 24,24",
        ],
    },
    Case {
        name: "two cards with the 24 px gap",
        gesture: Gesture::Bounds((240, 16), (472, 152)),
        answers: [
            "378 x 210 at 434,42",
            "270 x 150 at 310,30",
            "216 x 120 at 248,24",
        ],
    },
    Case {
        name: "card-75x45-offgrid",
        gesture: Gesture::Bounds((480, 20), (572, 82)),
        answers: [
            "131 x 79 at 856,51",
            "94 x 57 at 611,36",
            "75 x 45 at 489,29",
        ],
    },
    Case {
        name: "card-rounded",
        gesture: Gesture::Bounds((580, 16), (756, 152)),
        answers: [
            "280 x 210 at 1029,42",
            "200 x 150 at 735,30",
            "160 x 120 at 588,24",
        ],
    },
    Case {
        name: "card-outlined",
        gesture: Gesture::Bounds((764, 16), (940, 152)),
        answers: [
            "280 x 210 at 1351,42",
            "200 x 150 at 965,30",
            "160 x 120 at 772,24",
        ],
    },
    Case {
        name: "card-light-gray",
        gesture: Gesture::Bounds((948, 16), (1164, 152)),
        answers: [
            "350 x 210 at 1673,42",
            "250 x 150 at 1195,30",
            "200 x 120 at 956,24",
        ],
    },
    Case {
        name: "text-10",
        gesture: Gesture::Bounds((16, 163), (600, 183)),
        answers: [
            "603 x 18 at 42,296",
            "431 x 13 at 30,211",
            "345 x 9 at 24,170",
        ],
    },
    Case {
        name: "text-12",
        gesture: Gesture::Bounds((16, 182), (600, 202)),
        answers: [
            "723 x 21 at 42,329",
            "517 x 14 at 30,235",
            "414 x 12 at 24,188",
        ],
    },
    Case {
        name: "text-14",
        gesture: Gesture::Bounds((16, 202), (600, 226)),
        answers: [
            "704 x 25 at 43,364",
            "504 x 18 at 30,260",
            "403 x 13 at 24,208",
        ],
    },
    Case {
        name: "text-16",
        gesture: Gesture::Bounds((16, 224), (600, 252)),
        answers: [
            "646 x 27 at 43,407",
            "463 x 20 at 30,291",
            "370 x 16 at 24,232",
        ],
    },
    Case {
        name: "text-20",
        gesture: Gesture::Bounds((16, 253), (600, 286)),
        answers: [
            "627 x 34 at 43,458",
            "448 x 25 at 31,327",
            "359 x 20 at 24,261",
        ],
    },
    Case {
        name: "text-28",
        gesture: Gesture::Bounds((16, 286), (600, 326)),
        answers: [
            "577 x 48 at 44,516",
            "413 x 34 at 31,369",
            "330 x 27 at 25,294",
        ],
    },
    Case {
        name: "text-40",
        gesture: Gesture::Bounds((16, 326), (600, 378)),
        answers: [
            "522 x 66 at 48,592",
            "373 x 48 at 34,422",
            "299 x 37 at 27,338",
        ],
    },
    Case {
        name: "text-64",
        gesture: Gesture::Bounds((16, 380), (600, 470)),
        answers: [
            "431 x 80 at 47,695",
            "309 x 58 at 33,496",
            "246 x 47 at 27,396",
        ],
    },
    Case {
        name: "text-on-dark-16 (start on the dark band)",
        gesture: Gesture::Bounds((612, 172), (1170, 197)),
        answers: [
            "428 x 27 at 1092,312",
            "306 x 20 at 780,223",
            "245 x 16 at 624,178",
        ],
    },
    Case {
        name: "text-on-dark-12 (start on the dark band)",
        gesture: Gesture::Bounds((612, 198), (1170, 220)),
        answers: [
            "321 x 21 at 1092,357",
            "230 x 14 at 780,255",
            "184 x 12 at 624,204",
        ],
    },
    Case {
        name: "text-red-20",
        gesture: Gesture::Bounds((600, 238), (1184, 274)),
        answers: [
            "257 x 34 at 1066,433",
            "184 x 25 at 762,309",
            "147 x 20 at 609,247",
        ],
    },
    Case {
        name: "text-bold-14",
        gesture: Gesture::Bounds((600, 276), (1184, 306)),
        answers: [
            "205 x 25 at 1065,493",
            "146 x 18 at 761,352",
            "118 x 13 at 608,282",
        ],
    },
    Case {
        name: "text-on-gray-14 (start on the gray)",
        gesture: Gesture::Bounds((612, 315), (884, 345)),
        answers: [
            "335 x 25 at 1085,568",
            "239 x 18 at 775,406",
            "191 x 13 at 620,325",
        ],
    },
    Case {
        name: "text-shadow-28",
        gesture: Gesture::Bounds((600, 352), (866, 398)),
        answers: [
            "414 x 46 at 1062,638",
            "296 x 32 at 758,456",
            "237 x 26 at 607,364",
        ],
    },
    Case {
        name: "shadow-small",
        gesture: Gesture::Bounds((724, 406), (868, 508)),
        answers: [
            "216 x 146 at 1285,727",
            "154 x 105 at 918,519",
            "124 x 84 at 734,415",
        ],
    },
    Case {
        name: "shadow-large",
        gesture: Gesture::Bounds((864, 392), (1042, 540)),
        answers: [
            "264 x 194 at 1541,715",
            "188 x 138 at 1101,511",
            "150 x 110 at 881,409",
        ],
    },
    Case {
        name: "shadow-blue",
        gesture: Gesture::Bounds((1046, 396), (1196, 530)),
        answers: [
            "242 x 172 at 1832,719",
            "174 x 124 at 1308,513",
            "138 x 98 at 1047,411",
        ],
    },
    Case {
        name: "gradient-subtle (fades into white on the right)",
        gesture: Gesture::Bounds((16, 472), (392, 552)),
        answers: [
            "251 x 112 at 42,840",
            "179 x 80 at 30,600",
            "145 x 64 at 24,480",
        ],
    },
    Case {
        name: "gradient-strong",
        gesture: Gesture::Bounds((16, 560), (392, 640)),
        answers: [
            "630 x 112 at 42,994",
            "450 x 80 at 30,710",
            "360 x 64 at 24,568",
        ],
    },
    Case {
        name: "gradient-radial (white center)",
        gesture: Gesture::Bounds((16, 642), (392, 712)),
        answers: [
            "630 x 98 at 42,1134",
            "450 x 70 at 30,810",
            "360 x 56 at 24,648",
        ],
    },
    Case {
        name: "gradient-vertical (white at the top)",
        gesture: Gesture::Bounds((400, 472), (480, 640)),
        answers: [
            "112 x 236 at 714,870",
            "80 x 168 at 510,622",
            "64 x 134 at 408,498",
        ],
    },
    Case {
        name: "card-on-gradient (start on the gradient)",
        gesture: Gesture::Bounds((510, 500), (680, 610)),
        answers: [
            "299 x 193 at 892,875",
            "214 x 138 at 637,625",
            "171 x 111 at 510,500",
        ],
    },
    Case {
        name: "line-h-css-black (thin drag)",
        gesture: Gesture::Bounds((712, 548), (1184, 556)),
        answers: [
            "798 x 2 at 1260,966",
            "570 x 1 at 900,690",
            "456 x 1 at 720,552",
        ],
    },
    Case {
        name: "line-h-css-offgrid (thin drag)",
        gesture: Gesture::Bounds((712, 566), (1184, 576)),
        answers: [
            "798 x 2 at 1260,999",
            "570 x 1 at 900,714",
            "456 x 1 at 720,571",
        ],
    },
    Case {
        name: "line-h-css-divider #E5E5E5",
        gesture: Gesture::Bounds((712, 584), (1184, 592)),
        answers: [
            "798 x 2 at 1260,1029",
            "570 x 1 at 900,735",
            "456 x 1 at 720,588",
        ],
    },
    Case {
        name: "line-h-device-black",
        gesture: Gesture::Bounds((712, 600), (1184, 608)),
        answers: [
            "798 x 1 at 1260,1057",
            "570 x 1 at 900,755",
            "456 x 1 at 720,604",
        ],
    },
    Case {
        name: "line-v-css-black",
        gesture: Gesture::Bounds((740, 618), (750, 702)),
        answers: [
            "2 x 126 at 1302,1092",
            "1 x 90 at 930,780",
            "1 x 72 at 744,624",
        ],
    },
    Case {
        name: "line-v-device-black",
        gesture: Gesture::Bounds((770, 618), (782, 702)),
        answers: [
            "1 x 126 at 1358,1092",
            "1 x 90 at 970,780",
            "1 x 72 at 776,624",
        ],
    },
    Case {
        name: "lines-gap-3 pair",
        gesture: Gesture::Bounds((800, 618), (820, 702)),
        answers: [
            "9 x 126 at 1414,1092",
            "6 x 90 at 1010,780",
            "5 x 72 at 808,624",
        ],
    },
    Case {
        name: "box-css-outline",
        gesture: Gesture::Bounds((840, 616), (1016, 704)),
        answers: [
            "280 x 126 at 1484,1092",
            "200 x 90 at 1060,780",
            "160 x 72 at 848,624",
        ],
    },
    Case {
        name: "box-device-outline #808080",
        gesture: Gesture::Bounds((1024, 616), (1184, 704)),
        answers: [
            "252 x 126 at 1806,1092",
            "180 x 90 at 1290,780",
            "144 x 72 at 1032,624",
        ],
    },
];
