//! Tests of where a notice goes next to Inspect's magnified view (ticket WPC-367): a real session copies a color with
//! the cursor where the view goes above the cursor, near the monitor's bottom, and the notice it shows goes below the
//! cursor, leaving the view's position and color uncovered, at 100%, 150% and 175% and in every Color format. Only on
//! the monitor's last rows, where there is no room below, may it cover the swatch's bottom edge, never the text (the
//! owner's choice in WPC-367). Away from the bottom the notice is where it always was.

use std::sync::OnceLock;
use std::time::Duration;

use screengauge_core::session::{
    Colors, Event, Held, Image, Input, LocalTime, MagnifiedView, Notice, NoticeSide, Opening,
    Photo, Point, Rect, Scale, Session,
};
use screengauge_core::settings::{ColorFormat, Magnification, Options, Remembered, Tool};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_RECT_F, D2D_SIZE_U, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_BITMAP_INTERPOLATION_MODE_NEAREST_NEIGHBOR, D2D1_BITMAP_PROPERTIES, ID2D1Bitmap,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;

use super::geometry;
use super::notice;
use super::panel_tests::MODES;
use super::tests::{Canvas, Picture, text_width};
use super::*;

/// A Full HD monitor right of the main one, so nothing assumes the origin.
const MONITOR: Rect = Rect {
    left: 1920,
    top: 0,
    width: WIDTH.unsigned_abs(),
    height: HEIGHT.unsigned_abs(),
};
const WIDTH: i32 = 1920;
const HEIGHT: i32 = 1080;
const BOTTOM: i32 = MONITOR.top + HEIGHT;
const RIGHT: i32 = MONITOR.left + WIDTH;
/// A column well away from the monitor's sides.
const MIDDLE: i32 = MONITOR.left + 700;

const SCALES: [u32; 3] = [96, 144, 168];

const FORMATS: [ColorFormat; 3] = [ColorFormat::Hex, ColorFormat::Rgb, ColorFormat::Both];

/// The light gray of the monitor, and the red of a bar along its bottom, 40 pixels tall, like a button by the taskbar.
const GRAY: [u8; 3] = [0xf3, 0xf3, 0xf3];
const RED: [u8; 3] = [0xc0, 0x39, 0x2b];
const BAR: usize = 40;

fn at(x: i32, y: i32) -> Point {
    Point { x, y }
}

/// The monitor's pixels in rows from the top, B, G, R and A each, made once.
fn monitor_pixels() -> &'static [u8] {
    static PIXELS: OnceLock<Vec<u8>> = OnceLock::new();
    PIXELS.get_or_init(|| {
        let columns = usize::try_from(WIDTH).expect("a width");
        let rows = usize::try_from(HEIGHT).expect("a height");
        let row = |[r, g, b]: [u8; 3]| [b, g, r, 255].repeat(columns);
        let gray = row(GRAY).repeat(rows - BAR);
        let red = row(RED).repeat(BAR);
        [gray, red].concat()
    })
}

/// Inspect on `MONITOR` at `dpi`, the color in `format`, at `magnification`.
fn open(dpi: u32, format: ColorFormat, magnification: Magnification) -> Session {
    Session::open(Opening {
        frozen_screen: vec![Some(Photo {
            rect: MONITOR,
            scale: Scale::from_dpi(dpi).expect("a scale"),
            colors: Colors::Exact,
            image: Image::new(MONITOR.width, MONITOR.height, monitor_pixels().to_vec())
                .expect("the pixels fill the image"),
        })],
        cursor: at(2500, 500),
        options: Options {
            color_format: format,
            ..Options::default()
        },
        remembered: Remembered {
            last_tool: Tool::Inspect,
            magnification,
            ..Remembered::default()
        },
        second_click_until: None,
    })
    .expect("a frozen screen the session takes")
}

fn event(cursor: Point, milliseconds: u64, input: Input) -> Event {
    Event {
        cursor,
        held: Held::default(),
        time: LocalTime {
            year: 2026,
            month: 11,
            day: 5,
            hour: 14,
            minute: 2,
            second: 0,
        },
        uptime: Duration::from_millis(milliseconds),
        input,
    }
}

/// One click of Inspect: the session's settings, where the cursor is, and whether Windows took the copy.
#[derive(Clone, Copy, Debug)]
struct Click {
    dpi: u32,
    format: ColorFormat,
    magnification: Magnification,
    at: Point,
    made: bool,
}

/// The magnified view with the cursor at the click, and the notice once the clipboard answered it.
fn click(click: Click) -> (MagnifiedView, Notice) {
    let mut session = open(click.dpi, click.format, click.magnification);
    session.handle(event(click.at, 1000, Input::Move));
    session.handle(event(click.at, 1010, Input::Press));
    session.handle(event(click.at, 1020, Input::Release));
    let answer = if click.made {
        Input::Copied
    } else {
        Input::NotCopied
    };
    session.handle(event(click.at, 1030, answer));
    let scene = session.scene();
    let view = scene.magnified_view.expect("the magnified view");
    let shown = scene
        .notice
        .copied()
        .unwrap_or_else(|| panic!("no notice for {click:?}"));
    (view, shown)
}

/// The widths of "Copied" and "Couldn't copy." at `dpi`, as the notice lays them out.
fn widths(canvas: &Canvas, dpi: u32) -> (i32, i32) {
    let mut found = None;
    canvas.render(MONITOR, dpi, |overlay, _| {
        found = Some((
            text_width(overlay, "Copied", &overlay.format),
            text_width(overlay, "Couldn't copy.", &overlay.format),
        ));
    });
    found.expect("the overlay")
}

/// Where `shown` goes on `MONITOR` at `dpi`, its text `width` pixels wide.
fn placed(shown: Notice, width: i32, dpi: u32) -> RECT {
    notice::place(
        shown.at,
        shown.side,
        notice::size(width, dpi),
        geometry::bounds(MONITOR).expect("the monitor"),
        dpi,
    )
}

/// The view's swatch, and the color's second line, in Both.
const SWATCH: &str = "swatch";
const SECOND_LINE: &str = "second color line";

/// The view's parts the notice must leave uncovered, by name: the cursor's position and the color's swatch and lines.
fn parts(view: &MagnifiedView) -> Vec<(&'static str, Rect)> {
    let color = &view.color;
    [
        Some(("X", view.x.line)),
        Some(("Y", view.y.line)),
        Some((SWATCH, color.swatch)),
        Some(("first color line", color.first.line)),
        color
            .second
            .as_ref()
            .map(|second| (SECOND_LINE, second.line)),
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// `rect` with its right and bottom sides one past its last pixel.
fn sides(rect: Rect) -> RECT {
    geometry::bounds(rect).expect("a rectangle on the monitor")
}

/// How many rows of `part` `panel` covers, 0 for none.
fn rows_covered(panel: RECT, part: Rect) -> i32 {
    let part = sides(part);
    let columns = panel.right.min(part.right) - panel.left.max(part.left);
    let rows = panel.bottom.min(part.bottom) - panel.top.max(part.top);
    if columns > 0 && rows > 0 { rows } else { 0 }
}

/// The first row, at `x`, where the view goes above the cursor, as the cursor moves down from the monitor's middle,
/// below the toolbar.
fn first_row_above(dpi: u32, format: ColorFormat, magnification: Magnification, x: i32) -> i32 {
    let mut session = open(dpi, format, magnification);
    (MONITOR.top + HEIGHT / 2..BOTTOM)
        .find(|&y| {
            session.handle(event(at(x, y), 1000, Input::Move));
            session
                .scene()
                .magnified_view
                .is_some_and(|view| view.panel.top < y)
        })
        .expect("the view goes above the cursor near the bottom")
}

/// The most rows of the swatch's bottom edge the notice may cover with the cursor at `y`, at `dpi`: none, but on the
/// monitor's last rows, where the notice has no room below the cursor and slides up to 2 pixels from the bottom.
/// There it covers 4 rows at 150%, on the last 4 rows of the monitor, and 1 row at 100% and 175%, on the last one (the
/// owner's choice in WPC-367).
fn edge_rows_allowed(y: i32, dpi: u32) -> i32 {
    let edge = if dpi == 144 { 4 } else { 1 };
    let from_bottom = BOTTOM - y;
    (edge + 1 - from_bottom).max(0)
}

#[test]
fn near_the_bottom_the_notice_leaves_the_position_and_the_colors_text_uncovered() {
    let canvas = Canvas::new(64, 64);
    let mut wrong = Vec::new();
    let mut swatch_edges = 0;
    for dpi in SCALES {
        let widths = widths(&canvas, dpi);
        for format in FORMATS {
            for magnification in [Magnification::X8, Magnification::X32] {
                let first = first_row_above(dpi, format, magnification, MIDDLE);
                // The first row where the view goes above the cursor, which the ticket calls the start of the bottom
                // quarter (row 810 at 100%, higher at 150% and 175%, where the view is taller); 50 pixels from the
                // bottom; the last five rows; and the bottom right corner.
                let mut places = vec![
                    ("first row above", at(MIDDLE, first)),
                    ("50 from the bottom", at(MIDDLE, BOTTOM - 50)),
                    ("corner", at(RIGHT - 1, BOTTOM - 1)),
                ];
                places.extend((1..=5).map(|rows| ("last rows", at(MIDDLE, BOTTOM - rows))));
                for (name, click_at) in places {
                    for made in [true, false] {
                        let (view, shown) = click(Click {
                            dpi,
                            format,
                            magnification,
                            at: click_at,
                            made,
                        });
                        let case = format!(
                            "{name} {click_at:?} at {dpi} DPI, {format:?}, {magnification:?}, made {made}"
                        );
                        assert_eq!(shown.side, NoticeSide::Below, "{case}");
                        let width = if made { widths.0 } else { widths.1 };
                        let panel = placed(shown, width, dpi);
                        for (part, rect) in parts(&view) {
                            let rows = rows_covered(panel, rect);
                            // The swatch's bottom edge, and the bottom of the second color line's box, which lies as
                            // low: the render test below checks how much of that line's ink the notice hides there.
                            let allowed = if part == SWATCH || part == SECOND_LINE {
                                edge_rows_allowed(click_at.y, dpi)
                            } else {
                                0
                            };
                            if part == SWATCH && rows > 0 {
                                swatch_edges += 1;
                            }
                            if rows > allowed {
                                wrong.push(format!("{case}: {rows} rows of the {part}"));
                            }
                        }
                    }
                }
            }
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    // The last rows do reach the swatch with HEX and RGB, so the allowance above is used, not idle.
    assert!(swatch_edges > 0);
}

#[test]
fn with_the_view_below_the_cursor_the_notice_is_where_it_always_was() {
    let canvas = Canvas::new(64, 64);
    for dpi in SCALES {
        let (copied, _) = widths(&canvas, dpi);
        let (view, shown) = click(Click {
            dpi,
            format: ColorFormat::Both,
            magnification: Magnification::X8,
            at: at(MIDDLE, 500),
            made: true,
        });
        assert!(view.panel.top > 500, "the view is below the cursor");
        assert_eq!(shown.side, NoticeSide::Above);
        // 14 logical pixels right of the cursor and 14 above it, as before WPC-367.
        let size = notice::size(copied, dpi);
        let reach = scaled(14, dpi);
        assert_eq!(
            placed(shown, copied, dpi),
            RECT {
                left: MIDDLE + reach,
                top: 500 - reach - size.height,
                right: MIDDLE + reach + size.width,
                bottom: 500 - reach,
            },
            "at {dpi} DPI"
        );
    }
}

/// The part of `MONITOR` a canvas `width` by `height` shows: its bottom rows, from `left`.
fn crop(left: i32, width: u32, height: u32) -> Rect {
    Rect {
        left,
        top: BOTTOM - i32::try_from(height).expect("a height"),
        width,
        height,
    }
}

/// `crop`'s pixels as the overlay is given a monitor's photo: one Direct2D bitmap of the canvas's target at 96 DPI.
fn photo(canvas: &Canvas, crop: Rect) -> ID2D1Bitmap {
    let mut bytes = Vec::new();
    let (left, top) = (crop.left - MONITOR.left, crop.top - MONITOR.top);
    for y in top..top + i32::try_from(crop.height).expect("a height") {
        let start = usize::try_from((y * WIDTH + left) * 4).expect("on the monitor");
        let length = usize::try_from(crop.width * 4).expect("a row");
        bytes.extend_from_slice(&monitor_pixels()[start..start + length]);
    }
    let properties = D2D1_BITMAP_PROPERTIES {
        pixelFormat: D2D1_PIXEL_FORMAT {
            format: DXGI_FORMAT_B8G8R8A8_UNORM,
            alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
        },
        dpiX: 96.0,
        dpiY: 96.0,
    };
    // SAFETY: the pixels are exactly `height` rows of `width * 4` bytes, and outlive the call, which copies them.
    unsafe {
        canvas.target.CreateBitmap(
            D2D_SIZE_U {
                width: crop.width,
                height: crop.height,
            },
            Some(bytes.as_ptr().cast()),
            crop.width * 4,
            &properties,
        )
    }
    .expect("the photo's bitmap")
}

/// The crop's photo with the view over it, and the notice over both when there is one, in `mode`. The crop's bottom is
/// the monitor's, and so is its right side when it ends there, so the notice is kept inside it exactly as on the whole
/// monitor.
fn render(
    canvas: &Canvas,
    crop: Rect,
    mode: Mode,
    dpi: u32,
    view: &MagnifiedView,
    shown: Option<&Notice>,
) -> Picture {
    let photo = photo(canvas, crop);
    let whole = D2D_RECT_F {
        left: 0.0,
        top: 0.0,
        right: f32::from(u16::try_from(crop.width).expect("a crop's width")),
        bottom: f32::from(u16::try_from(crop.height).expect("a crop's height")),
    };
    canvas.render_in(mode, crop, dpi, |overlay, target| {
        // SAFETY: a drawing call between the BeginDraw and EndDraw of `render_in`, with arguments that outlive it.
        unsafe {
            target.DrawBitmap(
                &photo,
                Some(&whole),
                1.0,
                D2D1_BITMAP_INTERPOLATION_MODE_NEAREST_NEIGHBOR,
                None,
            );
        }
        overlay
            .draw_magnified_view(target, view, &photo)
            .expect("the magnified view");
        if let Some(shown) = shown {
            overlay.draw_notice(target, shown).expect("the notice");
        }
    })
}

/// The most levels the notice's soft shadow may darken a pixel of the view's text, as it does any panel under it.
const SHADOW: u8 = 24;

/// How far from the strip's background a pixel of a text's line must be to count as ink, in any channel: the strip's
/// own background shades by less.
const INK: u8 = 40;

/// How many pixels of `part`'s ink the notice may hide with the cursor at `y`, at `dpi`, in `format`: none, but at
/// 150% on the monitor's last row, where the notice's top edge covers the bottom tip of the two commas of the RGB in
/// Both, 2 pixels each (the owner's choice in WPC-367).
fn ink_allowed(part: &str, y: i32, dpi: u32, format: ColorFormat) -> usize {
    if part == SECOND_LINE && format == ColorFormat::Both && dpi == 144 && y == BOTTOM - 1 {
        4
    } else {
        0
    }
}

#[test]
fn on_the_last_rows_the_colors_text_and_the_position_stay_whole() {
    let (width, height) = (720, 420);
    let canvas = Canvas::new(width, height);
    let mut checked = 0;
    for mode in MODES {
        for dpi in SCALES {
            for format in FORMATS {
                for (click_at, left) in [
                    (at(MIDDLE, BOTTOM - 1), MIDDLE - 300),
                    (at(MIDDLE, BOTTOM - 2), MIDDLE - 300),
                    (at(MIDDLE, BOTTOM - 4), MIDDLE - 300),
                    (at(RIGHT - 1, BOTTOM - 1), RIGHT - 720),
                ] {
                    let crop = crop(left, width, height);
                    let (view, shown) = click(Click {
                        dpi,
                        format,
                        magnification: Magnification::X8,
                        at: click_at,
                        made: false,
                    });
                    let alone = render(&canvas, crop, mode, dpi, &view, None);
                    let covered = render(&canvas, crop, mode, dpi, &view, Some(&shown));
                    let case = format!("{click_at:?} at {dpi} DPI, {format:?}, {mode:?}");
                    // The ink of each text's line, the pixels well apart from the strip's background (the line's top
                    // left pixel, above any letter), is as it was without the notice, but for the notice's shadow.
                    for (part, rect) in parts(&view).into_iter().filter(|(part, _)| *part != SWATCH)
                    {
                        let rect = sides(rect);
                        let x0 = usize::try_from(rect.left - crop.left).expect("in the crop");
                        let y0 = usize::try_from(rect.top - crop.top).expect("in the crop");
                        let background = alone.at(x0, y0);
                        let (mut ink, mut hidden) = (0, 0);
                        for y in y0..y0 + usize::try_from(rect.bottom - rect.top).expect("a height")
                        {
                            for x in
                                x0..x0 + usize::try_from(rect.right - rect.left).expect("a width")
                            {
                                let before = alone.at(x, y);
                                if background
                                    .iter()
                                    .zip(before)
                                    .all(|(&a, b)| a.abs_diff(b) <= INK)
                                {
                                    continue;
                                }
                                ink += 1;
                                let after = covered.at(x, y);
                                if !before
                                    .iter()
                                    .zip(after)
                                    .all(|(&a, b)| a.abs_diff(b) <= SHADOW)
                                {
                                    hidden += 1;
                                }
                            }
                        }
                        assert!(ink > 0, "{case}: the {part} has ink");
                        let allowed = ink_allowed(part, click_at.y, dpi, format);
                        assert!(
                            hidden <= allowed,
                            "{case}: {hidden} pixels of the {part}'s ink hidden, {allowed} allowed"
                        );
                        checked += 1;
                    }
                }
            }
        }
    }
    assert!(checked > 0);
}
