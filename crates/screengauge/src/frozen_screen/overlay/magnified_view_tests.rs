//! Render tests of the magnified view on a WIC bitmap, like the notice's in `panel_tests`. The sizes here are the
//! prototype's (WPC-279), in logical pixels, scaled in the tests, and not read back from the code that is being tested;
//! the view the session core hands over is built by the rules the core states (`MagnifiedView`), with a photo whose every
//! pixel has a color of its own, so a pixel in the wrong place is a wrong color.

use screengauge_core::session::{
    ApproximateColor, ColorText, Coordinate, Label, MagnifiedView, NoticeLine, PixelColor, Point,
    Rect,
};
use screengauge_core::settings::Magnification;
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_SIZE_U, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{D2D1_BITMAP_PROPERTIES, ID2D1Bitmap};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;

use super::magnified_view::SHADOW_STRENGTH;
use super::panel_tests::{MODES, inside, over, rect, tokens};
use super::tests::{BACKGROUND, Canvas, Picture, near};
use super::*;

/// The window of these tests: big enough for a panel at 175% and its shadow, over a monitor left of and below the main
/// one, so coordinates in the virtual screen and in the window are different numbers.
pub(super) const SCREEN: Rect = Rect {
    left: -800,
    top: 300,
    width: 640,
    height: 640,
};

const BLACK: [u8; 3] = [0, 0, 0];
const WHITE: [u8; 3] = [255, 255, 255];

/// The prototype's `--blank`, written out a second time here so that a slip in the palette is caught.
fn blank(mode: Mode) -> [u8; 3] {
    match mode {
        Mode::Dark => [0x2a, 0x2c, 0x31],
        Mode::Light => [0xd9, 0xd6, 0xd0],
    }
}

/// `--panel-dim`, as above.
pub(super) fn dim(mode: Mode) -> [u8; 3] {
    match mode {
        Mode::Dark => [0x9a, 0xa0, 0xaa],
        Mode::Light => [0x6b, 0x68, 0x62],
    }
}

fn core_rect(left: i32, top: i32, width: i32, height: i32) -> Rect {
    Rect {
        left,
        top,
        width: u32::try_from(width).expect("a width"),
        height: u32::try_from(height).expect("a height"),
    }
}

/// `rect` with its right and bottom sides one past its last pixel.
pub(super) fn ints(rect: Rect) -> RECT {
    geometry::bounds(rect).expect("a rectangle")
}

/// The color of the photo's pixel at (`x`, `y`) of the virtual screen: different for every neighbor in both directions,
/// and none of them the background's, the panel's or the blank color.
fn photo_color(x: i32, y: i32) -> [u8; 3] {
    let (mx, my) = (x - SCREEN.left, y - SCREEN.top);
    let channel = |a: i32, b: i32| {
        u8::try_from((mx * a + my * b).rem_euclid(150) + 60).expect("a channel of a photo pixel")
    };
    [channel(37, 11), channel(13, 29), channel(53, 7)]
}

/// The monitor's photo as the overlay is given it: one Direct2D bitmap of the canvas's target at 96 DPI.
pub(super) fn photo(canvas: &Canvas) -> ID2D1Bitmap {
    let mut bytes = Vec::new();
    for y in 0..SCREEN.height {
        for x in 0..SCREEN.width {
            let at = |offset, value: u32| offset + i32::try_from(value).expect("a coordinate");
            let [r, g, b] = photo_color(at(SCREEN.left, x), at(SCREEN.top, y));
            bytes.extend([b, g, r, 255]);
        }
    }
    let format = D2D1_PIXEL_FORMAT {
        format: DXGI_FORMAT_B8G8R8A8_UNORM,
        alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
    };
    let properties = D2D1_BITMAP_PROPERTIES {
        pixelFormat: format,
        dpiX: 96.0,
        dpiY: 96.0,
    };
    // SAFETY: the pixels are exactly `height` rows of `pitch` bytes, and outlive the call, which copies them.
    unsafe {
        canvas.target.CreateBitmap(
            D2D_SIZE_U {
                width: SCREEN.width,
                height: SCREEN.height,
            },
            Some(bytes.as_ptr().cast()),
            SCREEN.width * 4,
            &properties,
        )
    }
    .expect("the photo's bitmap")
}

/// The magnified view for one situation, built by the rules the session core states for it, with every size taken from
/// the prototype and scaled one at a time.
#[derive(Clone)]
pub(super) struct Fixture {
    pub(super) dpi: u32,
    pub(super) magnification: Magnification,
    pub(super) center: Point,
    pub(super) grid: bool,
    pub(super) magnification_indicator: bool,
    pub(super) logical: bool,
    /// What X and Y say: the physical number, and the logical one when `logical`.
    pub(super) labels: [(&'static str, &'static str); 2],
    /// The color of the pixel under the cursor, which the swatch shows: one the photo has nowhere, its channels being
    /// 60 to 209.
    pub(super) rgb: [u8; 3],
    /// What the color's lines say: the first, and the one under it with both formats.
    pub(super) color: (&'static str, Option<&'static str>),
    /// Whether the readout ends with the Approximate color notice, as on a monitor whose photo didn't come through the
    /// exact path.
    pub(super) approximate: bool,
}

impl Fixture {
    /// A view in the middle of the monitor, with the grid wherever the core gives it (8x and up), no magnification
    /// indicator, and a logical part wherever the scale is not 100%.
    pub(super) fn new(dpi: u32, magnification: Magnification) -> Fixture {
        Fixture {
            dpi,
            magnification,
            center: Point {
                x: SCREEN.left + 300,
                y: SCREEN.top + 300,
            },
            grid: magnification.factor() >= 8,
            magnification_indicator: false,
            logical: dpi > 96,
            labels: [("1234", "1234.5"), ("1010", "1010.1")],
            rgb: [30, 144, 255],
            color: ("#1E90FF", Some("30, 144, 255")),
            approximate: false,
        }
    }

    pub(super) fn at(mut self, x: i32, y: i32) -> Fixture {
        self.center = Point { x, y };
        self
    }

    pub(super) fn factor(&self) -> i32 {
        i32::try_from(self.magnification.factor()).expect("a small factor")
    }

    /// The pixels across the source: the prototype's `floor(176 * dpr / mag)` made odd, at least 3.
    pub(super) fn across(&self) -> i32 {
        let css = i32::try_from(176 * self.dpi / 96).expect("a small size");
        let n = css / self.factor();
        let n = if n % 2 == 0 { n - 1 } else { n };
        n.max(3)
    }

    /// The height of one line of the readout: CSS 12px at 1.35, scaled and rounded to whole pixels.
    pub(super) fn line_height(&self) -> i32 {
        i32::try_from((162 * self.dpi + 480) / 960).expect("a small height")
    }

    /// The height of one line of the Approximate color notice: CSS 11px at the readout's 1.35, scaled and rounded to
    /// whole pixels.
    pub(super) fn notice_line_height(&self) -> i32 {
        i32::try_from((1485 * self.dpi + 4800) / 9600).expect("a small height")
    }

    pub(super) fn view(&self) -> MagnifiedView {
        let dpi = self.dpi;
        let (n, factor) = (self.across(), self.factor());
        let side = n * factor;
        let (ring, pad) = (scaled(1, dpi), scaled(6, dpi));
        let lines = if self.logical { 2 } else { 1 };
        let line_height = self.line_height();
        // The color's row: the swatch of 22 beside one line, or two with both formats, as tall as the taller of them.
        let swatch_side = scaled(22, dpi);
        let color_lines = if self.color.1.is_some() { 2 } else { 1 };
        let row = swatch_side.max(color_lines * line_height);
        // The notice: 4 under the color's row, two lines beside the sign of 13.
        let notice_line = self.notice_line_height();
        let notice = if self.approximate {
            scaled(4, dpi) + 2 * notice_line
        } else {
            0
        };
        let readout =
            scaled(7, dpi) + lines * line_height + scaled(5, dpi) + row + notice + scaled(2, dpi);
        let (left, top) = (SCREEN.left + 100, SCREEN.top + 70);
        let panel = core_rect(
            left,
            top,
            ring + pad + side + pad + ring,
            ring + pad + side + readout + pad + ring,
        );
        let (enlargement_left, enlargement_top) = (left + scaled(7, dpi), top + scaled(7, dpi));
        let enlargement = core_rect(enlargement_left, enlargement_top, side, side);

        let half = n / 2;
        let (source_left, source_top) = (self.center.x - half, self.center.y - half);
        let source = core_rect(source_left, source_top, n, n);
        let screen = ints(SCREEN);
        let shown_left = source_left.max(screen.left);
        let shown_top = source_top.max(screen.top);
        let shown = core_rect(
            shown_left,
            shown_top,
            (source_left + n).min(screen.right) - shown_left,
            (source_top + n).min(screen.bottom) - shown_top,
        );

        // The readout: right under the enlargement, as wide as it, its content inset 4 at each side.
        let inset = scaled(4, dpi);
        let width = side - 2 * inset;
        let readout_top = enlargement_top + side + scaled(7, dpi);
        let (x_line, y_line) = if self.logical {
            (
                core_rect(enlargement_left + inset, readout_top, width, line_height),
                core_rect(
                    enlargement_left + inset,
                    readout_top + line_height,
                    width,
                    line_height,
                ),
            )
        } else {
            // Two columns, 8 apart.
            let column = (width - scaled(8, dpi)) / 2;
            (
                core_rect(enlargement_left + inset, readout_top, column, line_height),
                core_rect(
                    enlargement_left + inset + column + scaled(8, dpi),
                    readout_top,
                    column,
                    line_height,
                ),
            )
        };
        let coordinate = |(physical, logical): (&str, &str), line| Coordinate {
            label: Label {
                physical: physical.to_owned(),
                logical: self.logical.then(|| logical.to_owned()),
            },
            line,
        };
        // The color's row is 5 under the position, the swatch at its left and the text 8 after it, each centered in the
        // row with the odd pixel below.
        let row_top = readout_top + lines * line_height + scaled(5, dpi);
        let beside = swatch_side + scaled(8, dpi);
        let swatch = core_rect(
            enlargement_left + inset,
            row_top + (row - swatch_side + 1) / 2,
            swatch_side,
            swatch_side,
        );
        let first_top = row_top + (row - color_lines * line_height + 1) / 2;
        let color_line = |top| {
            core_rect(
                enlargement_left + inset + beside,
                top,
                width - beside,
                line_height,
            )
        };
        let color = PixelColor {
            rgb: self.rgb,
            swatch,
            first: ColorText {
                text: self.color.0.to_owned(),
                line: color_line(first_top),
            },
            second: self.color.1.map(|text| ColorText {
                text: text.to_owned(),
                line: color_line(first_top + line_height),
            }),
        };
        MagnifiedView {
            monitor: 0,
            panel,
            enlargement,
            source,
            shown,
            center: self.center,
            magnification: self.magnification,
            grid: self.grid,
            magnification_indicator: self.magnification_indicator,
            x: coordinate(self.labels[0], x_line),
            y: coordinate(self.labels[1], y_line),
            color,
            approximate_color: self.approximate.then(|| {
                // The sign at the readout's left, centered beside the two lines with the odd pixel below, and the text
                // 5 after it.
                let top = row_top + row + scaled(4, dpi);
                let sign_side = scaled(13, dpi);
                let after_sign = sign_side + scaled(5, dpi);
                let line = |top, text| NoticeLine {
                    text,
                    line: core_rect(
                        enlargement_left + inset + after_sign,
                        top,
                        width - after_sign,
                        notice_line,
                    ),
                };
                ApproximateColor {
                    sign: core_rect(
                        enlargement_left + inset,
                        top + (2 * notice_line - sign_side + 1) / 2,
                        sign_side,
                        sign_side,
                    ),
                    lines: [
                        line(top, "Approximate color"),
                        line(top + notice_line, "(HDR on)"),
                    ],
                }
            }),
        }
    }
}

/// `view` drawn in `mode` on a canvas for the test window.
pub(super) fn render(
    canvas: &Canvas,
    photo: &ID2D1Bitmap,
    mode: Mode,
    dpi: u32,
    view: &MagnifiedView,
) -> Picture {
    canvas.render_in(mode, SCREEN, dpi, |overlay, target| {
        overlay
            .draw_magnified_view(target, view, photo)
            .expect("the magnified view");
    })
}

/// A pixel by its place in the virtual screen, which must be in the window.
pub(super) fn at(picture: &Picture, x: i32, y: i32) -> [u8; 3] {
    let column = usize::try_from(x - SCREEN.left).expect("a pixel right of the window's left side");
    let row = usize::try_from(y - SCREEN.top).expect("a pixel below the window's top");
    picture.at(column, row)
}

/// The pixels of `area` as (x, y) in the virtual screen.
pub(super) fn pixels(area: RECT) -> impl Iterator<Item = (i32, i32)> {
    (area.top..area.bottom).flat_map(move |y| (area.left..area.right).map(move |x| (x, y)))
}

/// `color` with `lines` layers of the grid's black at 20% over it.
fn gridded(color: [u8; 3], lines: u32) -> [u8; 3] {
    let keep = 0.8_f32.powi(i32::try_from(lines).expect("a few lines"));
    color.map(|c| {
        // A fraction of a channel's value, rounded, is a channel again.
        // TODO: mix in whole numbers, so no float comes back to `u8`.
        (f32::from(c) * keep).round() as u8
    })
}

/// What the prototype's `drawMag` leaves at the enlargement's pixel (`x`, `y`), and how far off a pixel may be: the
/// outline's two rings over everything, then the photo's pixel enlarged with the grid over it, or the blank color where
/// the enlargement is past the monitor.
fn wanted(view: &MagnifiedView, mode: Mode, x: i32, y: i32) -> ([u8; 3], u8) {
    let factor = i32::try_from(view.magnification.factor()).expect("a small factor");
    let (enlargement, source, shown) = (ints(view.enlargement), view.source, view.shown);
    let (cell_left, cell_top) = (
        enlargement.left + (view.center.x - source.left) * factor,
        enlargement.top + (view.center.y - source.top) * factor,
    );
    let beyond = |at: i32, first: i32| (first - at).max(at - (first + factor - 1)).max(0);
    match beyond(x, cell_left).max(beyond(y, cell_top)) {
        1 => return (BLACK, 0),
        2 => return (WHITE, 0),
        _ => {}
    }
    let (left, top) = (
        enlargement.left + (shown.left - source.left) * factor,
        enlargement.top + (shown.top - source.top) * factor,
    );
    let width = i32::try_from(shown.width).expect("a width") * factor;
    let height = i32::try_from(shown.height).expect("a height") * factor;
    if !(left..left + width).contains(&x) || !(top..top + height).contains(&y) {
        return (blank(mode), 0);
    }
    let color = photo_color(
        shown.left + (x - left) / factor,
        shown.top + (y - top) / factor,
    );
    let lines = [x - left, y - top]
        .into_iter()
        .filter(|&along| view.grid && along > 0 && along % factor == 0)
        .count();
    match u32::try_from(lines).expect("two lines at most") {
        0 => (color, 0),
        lines => (gridded(color, lines), 2),
    }
}

/// Whether (`x`, `y`) is in one of `enlargement`'s four rounded corners, where the clip lets the panel show.
fn in_corner(enlargement: &RECT, radius: i32, x: i32, y: i32) -> bool {
    let across = (x - enlargement.left).min(enlargement.right - 1 - x);
    let down = (y - enlargement.top).min(enlargement.bottom - 1 - y);
    across < radius && down < radius
}

/// Checks every pixel of the enlargement outside the corners, and outside `skip`, against the prototype's model.
fn check_enlargement(
    picture: &Picture,
    view: &MagnifiedView,
    mode: Mode,
    dpi: u32,
    skip: Option<RECT>,
) {
    let enlargement = ints(view.enlargement);
    let radius = scaled(9, dpi);
    let factor = view.magnification.factor();
    for (x, y) in pixels(enlargement) {
        if in_corner(&enlargement, radius, x, y) || skip.is_some_and(|skip| inside(&skip, x, y)) {
            continue;
        }
        let (color, tolerance) = wanted(view, mode, x, y);
        assert!(
            near(at(picture, x, y), color, tolerance),
            "pixel ({x}, {y}) of the enlargement is {:?}, wanted {color:?} at {factor}x, {mode:?}, {dpi} DPI, grid {}",
            at(picture, x, y),
            view.grid
        );
    }
}

fn magnifications() -> [Magnification; 5] {
    [
        Magnification::X2,
        Magnification::X4,
        Magnification::X8,
        Magnification::X16,
        Magnification::X32,
    ]
}

#[test]
fn every_enlarged_pixel_is_a_square_of_exactly_its_color_without_smoothing() {
    let canvas = Canvas::new(640, 640);
    let photo = photo(&canvas);
    for dpi in [96, 168] {
        for magnification in [Magnification::X2, Magnification::X8, Magnification::X32] {
            let view = Fixture::new(dpi, magnification).view();
            let picture = render(&canvas, &photo, Mode::Dark, dpi, &view);
            check_enlargement(&picture, &view, Mode::Dark, dpi, None);
        }
    }
}

#[test]
fn a_block_is_the_same_color_all_through() {
    // The model compares each pixel on its own; this reads a whole block back, away from the grid and the outline.
    let canvas = Canvas::new(640, 640);
    let photo = photo(&canvas);
    let fixture = Fixture {
        grid: false,
        ..Fixture::new(96, Magnification::X8)
    };
    let view = fixture.view();
    let picture = render(&canvas, &photo, Mode::Dark, 96, &view);
    let enlargement = ints(view.enlargement);
    // The third block from the enlargement's left, in the fifth row of blocks: a block of 8 by 8.
    let (left, top) = (enlargement.left + 2 * 8, enlargement.top + 4 * 8);
    let color = photo_color(view.source.left + 2, view.source.top + 4);
    for (x, y) in pixels(rect(left, top, left + 8, top + 8)) {
        assert_eq!(at(&picture, x, y), color, "({x}, {y})");
    }
    // Its neighbors are blocks of their own.
    let next = photo_color(view.source.left + 3, view.source.top + 4);
    assert_ne!(color, next);
    assert_eq!(at(&picture, left + 8, top), next);
    assert_eq!(at(&picture, left + 7, top), color);
}

/// Fixtures with the cursor near the monitor's top left corner and its bottom right one, so the source reaches past it.
fn near_the_edges(dpi: u32, magnification: Magnification) -> [Fixture; 2] {
    let fixture = Fixture::new(dpi, magnification);
    let width = i32::try_from(SCREEN.width).expect("a width");
    let height = i32::try_from(SCREEN.height).expect("a height");
    [
        fixture.clone().at(SCREEN.left + 2, SCREEN.top + 1),
        fixture.at(SCREEN.left + width - 1, SCREEN.top + height - 3),
    ]
}

#[test]
fn past_the_monitors_edge_the_enlargement_is_the_blank_color_with_no_grid_on_it() {
    let canvas = Canvas::new(640, 640);
    let photo = photo(&canvas);
    for mode in MODES {
        for magnification in [Magnification::X2, Magnification::X8, Magnification::X32] {
            for fixture in near_the_edges(96, magnification) {
                let view = fixture.view();
                // The scene is made for the test to mean something: part of the source is not on the monitor.
                assert!(
                    view.shown.width < view.source.width || view.shown.height < view.source.height
                );
                let picture = render(&canvas, &photo, mode, 96, &view);
                check_enlargement(&picture, &view, mode, 96, None);
                // And the blank color is really there: a good many pixels of the enlargement, not one stray.
                let enlargement = ints(view.enlargement);
                let blanks = pixels(enlargement)
                    .filter(|&(x, y)| at(&picture, x, y) == blank(mode))
                    .count();
                assert!(
                    blanks > 500,
                    "{blanks} blank pixels, {mode:?} at {}x",
                    view.magnification.factor()
                );
            }
        }
    }
}

#[test]
fn the_grid_is_drawn_from_8x_and_not_at_2x_and_4x() {
    let canvas = Canvas::new(640, 640);
    let photo = photo(&canvas);
    for magnification in magnifications() {
        let fixture = Fixture::new(96, magnification);
        let view = fixture.view();
        let picture = render(&canvas, &photo, Mode::Dark, 96, &view);
        check_enlargement(&picture, &view, Mode::Dark, 96, None);
        // Lines are where the model says, and nowhere in a view without a grid: count the darkened pixels.
        let enlargement = ints(view.enlargement);
        let darkened = pixels(enlargement)
            .filter(|&(x, y)| !in_corner(&enlargement, scaled(9, 96), x, y))
            .filter(|&(x, y)| {
                let (color, _) = wanted(
                    &Fixture {
                        grid: false,
                        ..fixture.clone()
                    }
                    .view(),
                    Mode::Dark,
                    x,
                    y,
                );
                at(&picture, x, y) != color
            })
            .count();
        if fixture.grid {
            assert!(
                darkened > 100,
                "{darkened} grid pixels at {}x",
                magnification.factor()
            );
        } else {
            assert_eq!(darkened, 0, "{}x has no grid", magnification.factor());
        }
    }
}

#[test]
fn the_grid_follows_the_views_flag_and_not_the_magnification() {
    let canvas = Canvas::new(640, 640);
    let photo = photo(&canvas);
    // 8x without a grid, and 4x with one: only the flag decides.
    for (magnification, grid) in [(Magnification::X8, false), (Magnification::X4, true)] {
        let fixture = Fixture {
            grid,
            ..Fixture::new(96, magnification)
        };
        let view = fixture.view();
        let picture = render(&canvas, &photo, Mode::Dark, 96, &view);
        check_enlargement(&picture, &view, Mode::Dark, 96, None);
    }
}

#[test]
fn a_grid_line_is_black_at_20_percent_one_pixel_wide_and_darker_where_two_cross() {
    let canvas = Canvas::new(640, 640);
    let photo = photo(&canvas);
    for dpi in [96, 168] {
        let view = Fixture::new(dpi, Magnification::X16).view();
        let picture = render(&canvas, &photo, Mode::Dark, dpi, &view);
        let enlargement = ints(view.enlargement);
        let (left, top) = (
            enlargement.left + (view.shown.left - view.source.left) * 16,
            enlargement.top + (view.shown.top - view.source.top) * 16,
        );
        // The enlarged pixel in the fourth column and row of the shown part, well inside the corners: its middle is the
        // photo's color, for the lines part pixels and don't frame them.
        let (first, second) = (left + 3 * 16, top + 3 * 16);
        let (source_x, source_y) = (view.shown.left + 3, view.shown.top + 3);
        let color = photo_color(source_x, source_y);
        assert_eq!(at(&picture, first + 5, second + 5), color, "at {dpi} DPI");
        // Its first column is a line over the pixel's rows: one layer of black at 20%, one pixel wide.
        let one = gridded(color, 1);
        assert!(
            near(at(&picture, first, second + 5), one, 1),
            "{:?}",
            at(&picture, first, second + 5)
        );
        assert_eq!(at(&picture, first + 1, second + 5), color);
        // The same along its first row.
        assert!(
            near(at(&picture, first + 5, second), one, 1),
            "{:?}",
            at(&picture, first + 5, second)
        );
        assert_eq!(at(&picture, first + 5, second + 1), color);
        // Where a vertical line and a horizontal one cross, black twice: 64% of the color.
        let two = gridded(color, 2);
        assert!(
            near(at(&picture, first, second), two, 1),
            "{:?}",
            at(&picture, first, second)
        );
        // The line before it is the last pixel of the pixel before, in its own color, not a line.
        assert_eq!(
            at(&picture, first - 1, second + 5),
            photo_color(source_x - 1, source_y)
        );
    }
}

/// The distance of (`x`, `y`) from `cell`: 0 inside it, 1 for the ring just outside it, and so on.
fn distance_from(cell: &RECT, x: i32, y: i32) -> i32 {
    let beyond = |at: i32, first: i32, past: i32| (first - at).max(at - (past - 1)).max(0);
    beyond(x, cell.left, cell.right).max(beyond(y, cell.top, cell.bottom))
}

#[test]
fn the_center_pixel_has_a_black_ring_and_a_white_one_just_outside_one_physical_pixel_thick() {
    let canvas = Canvas::new(640, 640);
    let photo = photo(&canvas);
    for dpi in [96, 168] {
        for magnification in magnifications() {
            for grid in [false, true] {
                let fixture = Fixture {
                    grid,
                    ..Fixture::new(dpi, magnification)
                };
                let view = fixture.view();
                let picture = render(&canvas, &photo, Mode::Dark, dpi, &view);
                let factor = fixture.factor();
                let enlargement = ints(view.enlargement);
                let cell_left = enlargement.left + (view.center.x - view.source.left) * factor;
                let cell_top = enlargement.top + (view.center.y - view.source.top) * factor;
                let cell = rect(cell_left, cell_top, cell_left + factor, cell_top + factor);
                let context = format!("{factor}x at {dpi} DPI, grid {grid}");
                // Around the cell, to 4 pixels out: black, then white, then the photo again, none of it any thicker.
                let around = rect(cell.left - 4, cell.top - 4, cell.right + 4, cell.bottom + 4);
                for (x, y) in pixels(around) {
                    let found = at(&picture, x, y);
                    match distance_from(&cell, x, y) {
                        1 => assert_eq!(found, BLACK, "the black ring at ({x}, {y}), {context}"),
                        2 => assert_eq!(found, WHITE, "the white ring at ({x}, {y}), {context}"),
                        _ => {
                            let (color, tolerance) = wanted(&view, Mode::Dark, x, y);
                            assert!(
                                near(found, color, tolerance),
                                "({x}, {y}) is {found:?}, wanted {color:?}, {context}"
                            );
                        }
                    }
                }
                // The cell keeps its color: its last pixel, which no grid line covers.
                let center = photo_color(view.center.x, view.center.y);
                assert_eq!(
                    at(&picture, cell.right - 1, cell.bottom - 1),
                    center,
                    "the cell, {context}"
                );
            }
        }
    }
}

#[test]
fn the_outline_stays_on_the_center_pixel_when_the_enlargement_is_blank_around_it() {
    // At the monitor's corner the rings are drawn over the blank color, and over nothing else.
    let canvas = Canvas::new(640, 640);
    let photo = photo(&canvas);
    for mode in MODES {
        let fixture = Fixture::new(96, Magnification::X32).at(SCREEN.left, SCREEN.top);
        let view = fixture.view();
        let picture = render(&canvas, &photo, mode, 96, &view);
        check_enlargement(&picture, &view, mode, 96, None);
        let enlargement = ints(view.enlargement);
        let cell_left = enlargement.left + (view.center.x - view.source.left) * 32;
        let cell_top = enlargement.top + (view.center.y - view.source.top) * 32;
        assert_eq!(at(&picture, cell_left - 1, cell_top + 5), BLACK);
        assert_eq!(at(&picture, cell_left - 2, cell_top + 5), WHITE);
        assert_eq!(at(&picture, cell_left - 3, cell_top + 5), blank(mode));
    }
}

#[test]
fn the_enlargements_corners_are_round_and_the_rest_of_its_edge_is_the_photo() {
    let canvas = Canvas::new(640, 640);
    let photo = photo(&canvas);
    for mode in MODES {
        let panel_color = tokens(mode).panel_over(BACKGROUND);
        for dpi in [96, 168] {
            let view = Fixture::new(dpi, Magnification::X8).view();
            let picture = render(&canvas, &photo, mode, dpi, &view);
            let enlargement = ints(view.enlargement);
            let radius = scaled(9, dpi);
            let context = format!("{mode:?} at {dpi} DPI");
            // The corner pixel is the panel, not the photo's color, and so is the one diagonally inside it, which the
            // arc has not reached.
            for (x, y) in [
                (enlargement.left, enlargement.top),
                (enlargement.right - 1, enlargement.top),
                (enlargement.left, enlargement.bottom - 1),
                (enlargement.right - 1, enlargement.bottom - 1),
            ] {
                assert!(
                    near(at(&picture, x, y), panel_color, 2),
                    "the corner pixel ({x}, {y}) is {:?}, the panel is {panel_color:?}, {context}",
                    at(&picture, x, y)
                );
            }
            // Along the top edge the photo is back by the radius, and the arc between is antialiased: neither all panel
            // nor all photo.
            let (color, _) = wanted(&view, mode, enlargement.left + radius + 1, enlargement.top);
            assert!(
                near(
                    at(&picture, enlargement.left + radius + 1, enlargement.top),
                    color,
                    6
                ),
                "{context}"
            );
            let (color, _) = wanted(&view, mode, enlargement.left, enlargement.top + radius + 1);
            assert!(
                near(
                    at(&picture, enlargement.left, enlargement.top + radius + 1),
                    color,
                    6
                ),
                "{context}"
            );
            let partly = pixels(rect(
                enlargement.left,
                enlargement.top,
                enlargement.left + radius,
                enlargement.top + radius,
            ))
            .filter(|&(x, y)| {
                let found = at(&picture, x, y);
                !near(found, panel_color, 2) && !near(found, wanted(&view, mode, x, y).0, 2)
            })
            .count();
            assert!(
                partly >= 3,
                "{partly} antialiased pixels on the arc, {context}"
            );
        }
    }
}

/// The magnification indicator's rectangle for a text `text_width` pixels wide: its top left `scaled(6)` inside the
/// enlargement's, padded 4 above and below and 6 at the sides around a line 11 tall.
fn magnification_indicator_rect(view: &MagnifiedView, dpi: u32, text_width: i32) -> RECT {
    let enlargement = ints(view.enlargement);
    let (left, top) = (
        enlargement.left + scaled(6, dpi),
        enlargement.top + scaled(6, dpi),
    );
    rect(
        left,
        top,
        left + scaled(6, dpi) + text_width + scaled(6, dpi),
        top + scaled(4, dpi) + scaled(11, dpi) + scaled(4, dpi),
    )
}

/// `fixture` drawn as `render` draws it, with its view, and how wide the magnification indicator's text is when laid
/// out on its own.
fn render_with_magnification_indicator(
    canvas: &Canvas,
    photo: &ID2D1Bitmap,
    mode: Mode,
    fixture: &Fixture,
) -> (Picture, MagnifiedView, i32) {
    let view = fixture.view();
    let mut width = None;
    let picture = canvas.render_in(mode, SCREEN, fixture.dpi, |overlay, target| {
        overlay
            .draw_magnified_view(target, &view, photo)
            .expect("the magnified view");
        let text = format!("{}\u{d7}", fixture.factor());
        width = Some(
            overlay
                .typeset_plain(&text, &overlay.magnification_indicator_format)
                .expect("the magnification indicator's text")
                .width,
        );
    });
    (picture, view, width.expect("the view was drawn"))
}

/// The pixels of `area` that are nearly white: the magnification indicator's text over the photo darkened by its black
/// at 60%, which is nowhere near that bright.
fn white_ink(picture: &Picture, area: RECT) -> Vec<(i32, i32)> {
    pixels(area)
        .filter(|&(x, y)| at(picture, x, y).iter().all(|&c| c >= 180))
        .collect()
}

#[test]
fn the_magnification_indicator_is_black_at_60_percent_in_a_rounded_rectangle_with_the_magnification_in_white()
 {
    let canvas = Canvas::new(640, 640);
    let photo = photo(&canvas);
    // The same in both palettes.
    for mode in MODES {
        for dpi in [96, 168] {
            let fixture = Fixture {
                magnification_indicator: true,
                ..Fixture::new(dpi, Magnification::X8)
            };
            let (picture, view, text) =
                render_with_magnification_indicator(&canvas, &photo, mode, &fixture);
            let indicator = magnification_indicator_rect(&view, dpi, text);
            let context = format!("{mode:?} at {dpi} DPI");
            // The photo under it, as it is without a magnification indicator.
            let under = |x: i32, y: i32| wanted(&view, mode, x, y).0;
            let darkened = |x: i32, y: i32| over([0, 0, 0], 0.6, under(x, y));
            // The padding above and below the text, past the corner's arc: the photo at 40%, from the magnification
            // indicator's first row to its last.
            let x = indicator.left + scaled(6, dpi) + 1;
            for y in [
                indicator.top,
                indicator.top + 1,
                indicator.bottom - 2,
                indicator.bottom - 1,
            ] {
                assert!(
                    near(at(&picture, x, y), darkened(x, y), 2),
                    "({x}, {y}) is {:?}, wanted {:?}, {context}",
                    at(&picture, x, y),
                    darkened(x, y)
                );
            }
            // The rectangle is exactly this tall and as wide as its text and padding: the pixels beside it are the photo.
            let middle = (indicator.top + indicator.bottom) / 2;
            for (x, y) in [
                (x, indicator.top - 1),
                (x, indicator.bottom),
                (indicator.left - 1, middle),
                (indicator.right, middle),
            ] {
                assert!(
                    near(at(&picture, x, y), under(x, y), 1),
                    "({x}, {y}), beside the magnification indicator, is {:?}, wanted {:?}, {context}",
                    at(&picture, x, y),
                    under(x, y)
                );
            }
            for x in [indicator.left + 1, indicator.right - 2] {
                assert!(
                    near(at(&picture, x, middle), darkened(x, middle), 2),
                    "({x}, {middle}), in the magnification indicator's padding, is {:?}, {context}",
                    at(&picture, x, middle)
                );
            }
            // Its corners are round: the corner pixel is the photo's, not the magnification indicator's.
            for (x, y) in [
                (indicator.left, indicator.top),
                (indicator.right - 1, indicator.top),
                (indicator.left, indicator.bottom - 1),
                (indicator.right - 1, indicator.bottom - 1),
            ] {
                assert!(
                    near(at(&picture, x, y), under(x, y), 8),
                    "the magnification indicator's corner ({x}, {y}) is {:?}, wanted the photo's {:?}, {context}",
                    at(&picture, x, y),
                    under(x, y)
                );
            }
            // Its text is white, inside the padding, and nowhere else in the magnification indicator is anything that
            // white.
            let text_area = rect(
                indicator.left + scaled(6, dpi) - 1,
                indicator.top + scaled(4, dpi) - 1,
                indicator.right - scaled(6, dpi) + 1,
                indicator.bottom - scaled(4, dpi) + 1,
            );
            // Over the photo's own pixels, which can be bright, white says nothing; in the magnification indicator, the
            // photo is at 40%.
            let ink = white_ink(&picture, indicator);
            assert!(ink.len() > 20, "{} white pixels, {context}", ink.len());
            assert!(
                ink.iter().all(|&(x, y)| inside(&text_area, x, y)),
                "white pixels outside the magnification indicator's text area, {context}"
            );
            // The white is the full white of the text where a stroke covers a pixel all through.
            assert!(
                ink.iter().any(|&(x, y)| near(at(&picture, x, y), WHITE, 6)),
                "the text is never fully white, {context}"
            );
        }
    }
}

#[test]
fn the_magnification_indicator_is_there_when_the_view_says_so_and_not_otherwise() {
    let canvas = Canvas::new(640, 640);
    let photo = photo(&canvas);
    for magnification_indicator in [false, true] {
        let fixture = Fixture {
            magnification_indicator,
            ..Fixture::new(96, Magnification::X8)
        };
        let (picture, view, text) =
            render_with_magnification_indicator(&canvas, &photo, Mode::Dark, &fixture);
        let area = magnification_indicator_rect(&view, 96, text);
        // The whole enlargement outside the magnification indicator is the model's, with the magnification indicator or
        // without; inside it, without one, too.
        let skip = magnification_indicator.then_some(area);
        check_enlargement(&picture, &view, Mode::Dark, 96, skip);
        let ink = white_ink(&picture, area).len();
        assert_eq!(
            ink > 0,
            magnification_indicator,
            "{ink} white pixels in the magnification indicator's place, shown: {magnification_indicator}"
        );
    }
}

#[test]
fn the_magnification_indicators_text_is_the_magnification_and_a_times_sign() {
    let canvas = Canvas::new(640, 640);
    let photo = photo(&canvas);
    let mut magnification_indicators = Vec::new();
    for magnification in magnifications() {
        let fixture = Fixture {
            magnification_indicator: true,
            ..Fixture::new(96, magnification)
        };
        let (picture, view, text) =
            render_with_magnification_indicator(&canvas, &photo, Mode::Dark, &fixture);
        let area = magnification_indicator_rect(&view, 96, text);
        magnification_indicators.push((magnification.factor(), text, white_ink(&picture, area)));
    }
    let found = |factor: u32| {
        magnification_indicators
            .iter()
            .find(|(f, ..)| *f == factor)
            .expect("a magnification indicator for every magnification")
    };
    // Two digits take more room than one, and "32" has the strokes of the "2" and more.
    assert!(
        found(32).1 > found(8).1 + 4,
        "{} against {}",
        found(32).1,
        found(8).1
    );
    assert!(
        found(32).2.len() > found(2).2.len() + 8,
        "{} against {}",
        found(32).2.len(),
        found(2).2.len()
    );
    // One digit each for 2, 4 and 8, with different strokes: the white pixels are not the same ones.
    for (a, b) in [(2, 4), (2, 8), (4, 8)] {
        assert_ne!(found(a).2, found(b).2, "{a}x and {b}x look the same");
    }
}

#[test]
fn nothing_is_drawn_outside_the_panel_but_its_shadow() {
    let canvas = Canvas::new(640, 640);
    let photo = photo(&canvas);
    for mode in MODES {
        for (dpi, magnification) in [
            (96, Magnification::X8),
            (96, Magnification::X32),
            (120, Magnification::X2),
            (144, Magnification::X16),
            (168, Magnification::X32),
            (168, Magnification::X8),
        ] {
            let fixture = Fixture {
                magnification_indicator: true,
                labels: [("3839", "2559.3"), ("2159", "1439.3")],
                ..Fixture::new(dpi, magnification)
            };
            // The view with the cursor in the middle, and with it near the monitor's corner, where the enlargement is
            // partly blank: the enlargement's clip and the magnification indicator are at the edges of what is drawn.
            let views = [
                fixture.view(),
                fixture.clone().at(SCREEN.left + 1, SCREEN.top + 1).view(),
            ];
            for view in views {
                let panel = ints(view.panel);
                let full = render(&canvas, &photo, mode, dpi, &view);
                let alone = canvas.render_in(mode, SCREEN, dpi, |overlay, target| {
                    overlay
                        .draw_floating_panel(target, &panel, 14, SHADOW_STRENGTH)
                        .expect("the panel");
                });
                let context = format!("{mode:?} at {dpi} DPI, {}x", fixture.factor());
                let mut inside_changed = 0;
                for (x, y) in pixels(ints(SCREEN)) {
                    if inside(&panel, x, y) {
                        inside_changed += usize::from(at(&full, x, y) != at(&alone, x, y));
                    } else {
                        assert_eq!(
                            at(&full, x, y),
                            at(&alone, x, y),
                            "({x}, {y}) outside the panel {panel:?}, {context}"
                        );
                    }
                }
                assert!(
                    inside_changed > 5000,
                    "{inside_changed} pixels drawn, {context}"
                );
            }
        }
    }
}

#[test]
fn the_panel_has_its_border_and_a_shadow_that_falls_below_it_and_reaches_no_further_than_the_notices()
 {
    let canvas = Canvas::new(640, 640);
    let photo = photo(&canvas);
    for mode in MODES {
        let t = tokens(mode);
        for dpi in [96, 168] {
            let view = Fixture::new(dpi, Magnification::X8).view();
            let picture = render(&canvas, &photo, mode, dpi, &view);
            let panel = ints(view.panel);
            let context = format!("{mode:?} at {dpi} DPI");
            // The border's ring along the top, over the panel, which is over the photo.
            let across = (panel.left + panel.right) / 2;
            assert!(
                near(at(&picture, across, panel.top), t.ring_over_panel(), 2),
                "the ring, {context}: {:?}",
                at(&picture, across, panel.top)
            );
            // The shadow falls below the panel, darker than it falls above it, and nothing is drawn beyond its reach: the
            // blur's 28 less the spread of 8 around the panel, 10 further down, and a pixel or two for the last band.
            let below = at(&picture, across, panel.bottom + scaled(4, dpi));
            let above = at(&picture, across, panel.top - scaled(4, dpi));
            assert!(
                below[0] < above[0],
                "{below:?} below and {above:?} above, {context}"
            );
            assert!(
                below[0] < BACKGROUND[0],
                "{below:?} is not darkened, {context}"
            );
            let reach = scaled(28, dpi) + 2;
            let drop = scaled(10, dpi);
            let reached = rect(
                panel.left - reach,
                panel.top - reach,
                panel.right + reach,
                panel.bottom + reach + drop,
            );
            for (x, y) in pixels(ints(SCREEN)) {
                if !inside(&reached, x, y) {
                    assert_eq!(
                        at(&picture, x, y),
                        BACKGROUND,
                        "({x}, {y}) is beyond the shadow, {context}"
                    );
                }
            }
        }
    }
}

#[test]
fn the_shadow_is_within_7_hundredths_of_opacity_of_the_prototypes_below_and_beside_the_panel() {
    // CSS `0 10px 28px -8px` at .75 in the dark palette and `0 10px 26px -8px` at .45 in the light one: a gaussian blur
    // of sigma half the blur radius, of the panel's box shrunk by 8 on every side and moved 10 down. The panel is much
    // taller than the blur, so down its middle the box is an edge: below the panel the edge is 2 further down than the
    // panel's bottom, and beside it 8 further in.
    let canvas = Canvas::new(640, 640);
    let photo = photo(&canvas);
    for (mode, alpha, sigma, shadow) in [
        (Mode::Dark, 0.75_f64, 14.0_f64, [0_u8, 0, 0]),
        (Mode::Light, 0.45, 13.0, [70, 60, 45]),
    ] {
        let view = Fixture::new(96, Magnification::X8).view();
        let picture = render(&canvas, &photo, mode, 96, &view);
        let panel = ints(view.panel);
        let (across, middle) = (
            (panel.left + panel.right) / 2,
            (panel.top + panel.bottom) / 2,
        );
        // How much of the shadow's color lies over the photo at (x, y), from the red channel.
        let drawn = |x: i32, y: i32| {
            let (background, shadow) = (f64::from(BACKGROUND[0]), f64::from(shadow[0]));
            (background - f64::from(at(&picture, x, y)[0])) / (background - shadow)
        };
        let css = |beyond_the_edge: f64| {
            alpha * (1.0 - crate::relief::shadow::normal_cdf(beyond_the_edge / sigma))
        };
        for d in [0, 3, 8, 14, 22, 30] {
            let wanted = css(f64::from(d) + 0.5 - 2.0);
            let found = drawn(across, panel.bottom + d);
            assert!(
                (found - wanted).abs() < 0.07,
                "{d} below the panel the shadow is {found}, CSS's is {wanted}, {mode:?}"
            );
            let wanted = css(f64::from(d) + 0.5 + 8.0);
            let found = drawn(panel.left - 1 - d, middle);
            assert!(
                (found - wanted).abs() < 0.07,
                "{d} beside the panel the shadow is {found}, CSS's is {wanted}, {mode:?}"
            );
        }
    }
}
