//! Render tests of the magnified view on a WIC bitmap, like the notice's in `panel_tests`. The sizes here are the
//! prototype's (WPC-279), in logical pixels, scaled in the tests, and not read back from the code that is being tested;
//! the view the session core hands over is built by the rules the core states (`MagnifiedView`), with a photo whose every
//! pixel has a color of its own, so a pixel in the wrong place is a wrong color.

use screengauge_core::session::{Coordinate, Label, MagnifiedView, Point, Rect};
use screengauge_core::settings::Magnification;
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_SIZE_U, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{D2D1_BITMAP_PROPERTIES, ID2D1Bitmap};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;

use super::magnified_view::SHADOW_STRENGTH;
use super::panel_tests::{MODES, inside, over, rect, tokens};
use super::tests::{BACKGROUND, Canvas, Picture, near, text_width};
use super::*;

/// The window of these tests: big enough for a panel at 175% and its shadow, over a monitor left of and below the main
/// one, so coordinates in the virtual screen and in the window are different numbers.
const SCREEN: Rect = Rect {
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
fn dim(mode: Mode) -> [u8; 3] {
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
fn ints(rect: Rect) -> RECT {
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
fn photo(canvas: &Canvas) -> ID2D1Bitmap {
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
struct Fixture {
    dpi: u32,
    magnification: Magnification,
    center: Point,
    grid: bool,
    tag: bool,
    logical: bool,
    /// What X and Y say: the physical number, and the logical one when `logical`.
    labels: [(&'static str, &'static str); 2],
}

impl Fixture {
    /// A view in the middle of the monitor, with the grid wherever the core gives it (8x and up), no tag, and a logical
    /// part wherever the scale is not 100%.
    fn new(dpi: u32, magnification: Magnification) -> Fixture {
        Fixture {
            dpi,
            magnification,
            center: Point {
                x: SCREEN.left + 300,
                y: SCREEN.top + 300,
            },
            grid: magnification.factor() >= 8,
            tag: false,
            logical: dpi > 96,
            labels: [("1234", "1234.5"), ("1010", "1010.1")],
        }
    }

    fn at(mut self, x: i32, y: i32) -> Fixture {
        self.center = Point { x, y };
        self
    }

    fn factor(&self) -> i32 {
        i32::try_from(self.magnification.factor()).expect("a small factor")
    }

    /// The pixels across the source: the prototype's `floor(176 * dpr / mag)` made odd, at least 3.
    fn across(&self) -> i32 {
        let css = i32::try_from(176 * self.dpi / 96).expect("a small size");
        let n = css / self.factor();
        let n = if n % 2 == 0 { n - 1 } else { n };
        n.max(3)
    }

    /// The height of one line of the strip: CSS 12px at 1.35, scaled and rounded to whole pixels.
    fn line_height(&self) -> i32 {
        i32::try_from((162 * self.dpi + 480) / 960).expect("a small height")
    }

    fn view(&self) -> MagnifiedView {
        let dpi = self.dpi;
        let (n, factor) = (self.across(), self.factor());
        let side = n * factor;
        let (ring, pad) = (scaled(1, dpi), scaled(6, dpi));
        let lines = if self.logical { 2 } else { 1 };
        let strip = scaled(7, dpi) + lines * self.line_height() + scaled(2, dpi);
        let (left, top) = (SCREEN.left + 100, SCREEN.top + 70);
        let panel = core_rect(
            left,
            top,
            ring + pad + side + pad + ring,
            ring + pad + side + strip + pad + ring,
        );
        let (frame_left, frame_top) = (left + scaled(7, dpi), top + scaled(7, dpi));
        let frame = core_rect(frame_left, frame_top, side, side);

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

        // The strip: right under the frame, as wide as it, its content inset 4 at each side.
        let inset = scaled(4, dpi);
        let width = side - 2 * inset;
        let strip_top = frame_top + side + scaled(7, dpi);
        let line_height = self.line_height();
        let (x_line, y_line) = if self.logical {
            (
                core_rect(frame_left + inset, strip_top, width, line_height),
                core_rect(
                    frame_left + inset,
                    strip_top + line_height,
                    width,
                    line_height,
                ),
            )
        } else {
            // Two columns, 8 apart.
            let column = (width - scaled(8, dpi)) / 2;
            (
                core_rect(frame_left + inset, strip_top, column, line_height),
                core_rect(
                    frame_left + inset + column + scaled(8, dpi),
                    strip_top,
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
        MagnifiedView {
            monitor: 0,
            panel,
            frame,
            source,
            shown,
            center: self.center,
            magnification: self.magnification,
            grid: self.grid,
            tag: self.tag,
            x: coordinate(self.labels[0], x_line),
            y: coordinate(self.labels[1], y_line),
        }
    }
}

/// `view` drawn in `mode` on a canvas for the test window.
fn render(
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
fn at(picture: &Picture, x: i32, y: i32) -> [u8; 3] {
    let column = usize::try_from(x - SCREEN.left).expect("a pixel right of the window's left side");
    let row = usize::try_from(y - SCREEN.top).expect("a pixel below the window's top");
    picture.at(column, row)
}

/// The pixels of `area` as (x, y) in the virtual screen.
fn pixels(area: RECT) -> impl Iterator<Item = (i32, i32)> {
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

/// What the prototype's `drawMag` leaves at the frame's pixel (`x`, `y`), and how far off a pixel may be: the outline's
/// two rings over everything, then the photo's pixel enlarged with the grid over it, or the blank color where the frame
/// is past the monitor.
fn wanted(view: &MagnifiedView, mode: Mode, x: i32, y: i32) -> ([u8; 3], u8) {
    let factor = i32::try_from(view.magnification.factor()).expect("a small factor");
    let (frame, source, shown) = (ints(view.frame), view.source, view.shown);
    let (cell_left, cell_top) = (
        frame.left + (view.center.x - source.left) * factor,
        frame.top + (view.center.y - source.top) * factor,
    );
    let beyond = |at: i32, first: i32| (first - at).max(at - (first + factor - 1)).max(0);
    match beyond(x, cell_left).max(beyond(y, cell_top)) {
        1 => return (BLACK, 0),
        2 => return (WHITE, 0),
        _ => {}
    }
    let (left, top) = (
        frame.left + (shown.left - source.left) * factor,
        frame.top + (shown.top - source.top) * factor,
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

/// Whether (`x`, `y`) is in one of `frame`'s four rounded corners, where the clip lets the panel show.
fn in_corner(frame: &RECT, radius: i32, x: i32, y: i32) -> bool {
    let across = (x - frame.left).min(frame.right - 1 - x);
    let down = (y - frame.top).min(frame.bottom - 1 - y);
    across < radius && down < radius
}

/// Checks every pixel of the frame outside the corners, and outside `skip`, against the prototype's model.
fn check_frame(picture: &Picture, view: &MagnifiedView, mode: Mode, dpi: u32, skip: Option<RECT>) {
    let frame = ints(view.frame);
    let radius = scaled(9, dpi);
    let factor = view.magnification.factor();
    for (x, y) in pixels(frame) {
        if in_corner(&frame, radius, x, y) || skip.is_some_and(|skip| inside(&skip, x, y)) {
            continue;
        }
        let (color, tolerance) = wanted(view, mode, x, y);
        assert!(
            near(at(picture, x, y), color, tolerance),
            "pixel ({x}, {y}) of the frame is {:?}, wanted {color:?} at {factor}x, {mode:?}, {dpi} DPI, grid {}",
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
            check_frame(&picture, &view, Mode::Dark, dpi, None);
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
    let frame = ints(view.frame);
    // The third block from the frame's left, in the fifth row of blocks: a block of 8 by 8.
    let (left, top) = (frame.left + 2 * 8, frame.top + 4 * 8);
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
fn past_the_monitors_edge_the_frame_is_the_blank_color_with_no_grid_on_it() {
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
                check_frame(&picture, &view, mode, 96, None);
                // And the blank color is really there: a good many pixels of the frame, not one stray.
                let frame = ints(view.frame);
                let blanks = pixels(frame)
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
        check_frame(&picture, &view, Mode::Dark, 96, None);
        // Lines are where the model says, and nowhere in a view without a grid: count the darkened pixels.
        let frame = ints(view.frame);
        let darkened = pixels(frame)
            .filter(|&(x, y)| !in_corner(&frame, scaled(9, 96), x, y))
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
        check_frame(&picture, &view, Mode::Dark, 96, None);
    }
}

#[test]
fn a_grid_line_is_black_at_20_percent_one_pixel_wide_and_darker_where_two_cross() {
    let canvas = Canvas::new(640, 640);
    let photo = photo(&canvas);
    for dpi in [96, 168] {
        let view = Fixture::new(dpi, Magnification::X16).view();
        let picture = render(&canvas, &photo, Mode::Dark, dpi, &view);
        let frame = ints(view.frame);
        let (left, top) = (
            frame.left + (view.shown.left - view.source.left) * 16,
            frame.top + (view.shown.top - view.source.top) * 16,
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
                let frame = ints(view.frame);
                let cell_left = frame.left + (view.center.x - view.source.left) * factor;
                let cell_top = frame.top + (view.center.y - view.source.top) * factor;
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
fn the_outline_stays_on_the_center_pixel_when_the_frame_is_blank_around_it() {
    // At the monitor's corner the rings are drawn over the blank color, and over nothing else.
    let canvas = Canvas::new(640, 640);
    let photo = photo(&canvas);
    for mode in MODES {
        let fixture = Fixture::new(96, Magnification::X32).at(SCREEN.left, SCREEN.top);
        let view = fixture.view();
        let picture = render(&canvas, &photo, mode, 96, &view);
        check_frame(&picture, &view, mode, 96, None);
        let frame = ints(view.frame);
        let cell_left = frame.left + (view.center.x - view.source.left) * 32;
        let cell_top = frame.top + (view.center.y - view.source.top) * 32;
        assert_eq!(at(&picture, cell_left - 1, cell_top + 5), BLACK);
        assert_eq!(at(&picture, cell_left - 2, cell_top + 5), WHITE);
        assert_eq!(at(&picture, cell_left - 3, cell_top + 5), blank(mode));
    }
}

#[test]
fn the_frames_corners_are_round_and_the_rest_of_its_edge_is_the_photo() {
    let canvas = Canvas::new(640, 640);
    let photo = photo(&canvas);
    for mode in MODES {
        let panel_color = tokens(mode).panel_over(BACKGROUND);
        for dpi in [96, 168] {
            let view = Fixture::new(dpi, Magnification::X8).view();
            let picture = render(&canvas, &photo, mode, dpi, &view);
            let frame = ints(view.frame);
            let radius = scaled(9, dpi);
            let context = format!("{mode:?} at {dpi} DPI");
            // The corner pixel is the panel, not the photo's color, and so is the one diagonally inside it, which the
            // arc has not reached.
            for (x, y) in [
                (frame.left, frame.top),
                (frame.right - 1, frame.top),
                (frame.left, frame.bottom - 1),
                (frame.right - 1, frame.bottom - 1),
            ] {
                assert!(
                    near(at(&picture, x, y), panel_color, 2),
                    "the corner pixel ({x}, {y}) is {:?}, the panel is {panel_color:?}, {context}",
                    at(&picture, x, y)
                );
            }
            // Along the top edge the photo is back by the radius, and the arc between is antialiased: neither all panel
            // nor all photo.
            let (color, _) = wanted(&view, mode, frame.left + radius + 1, frame.top);
            assert!(
                near(at(&picture, frame.left + radius + 1, frame.top), color, 6),
                "{context}"
            );
            let (color, _) = wanted(&view, mode, frame.left, frame.top + radius + 1);
            assert!(
                near(at(&picture, frame.left, frame.top + radius + 1), color, 6),
                "{context}"
            );
            let partly = pixels(rect(
                frame.left,
                frame.top,
                frame.left + radius,
                frame.top + radius,
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

/// The tag's rectangle for a text `text_width` pixels wide: its top left `scaled(6)` inside the frame's, padded 4 above
/// and below and 6 at the sides around a line 11 tall.
fn tag_rect(view: &MagnifiedView, dpi: u32, text_width: i32) -> RECT {
    let frame = ints(view.frame);
    let (left, top) = (frame.left + scaled(6, dpi), frame.top + scaled(6, dpi));
    rect(
        left,
        top,
        left + scaled(6, dpi) + text_width + scaled(6, dpi),
        top + scaled(4, dpi) + scaled(11, dpi) + scaled(4, dpi),
    )
}

/// `fixture` drawn as `render` draws it, with its view, and how wide the tag's text is when laid out on its own.
fn render_with_tag(
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
                .typeset_plain(&text, &overlay.tag_format)
                .expect("the tag's text")
                .width,
        );
    });
    (picture, view, width.expect("the view was drawn"))
}

/// The pixels of `area` that are nearly white: the tag's text over the photo darkened by the tag's black at 60%, which is
/// nowhere near that bright.
fn white_ink(picture: &Picture, area: RECT) -> Vec<(i32, i32)> {
    pixels(area)
        .filter(|&(x, y)| at(picture, x, y).iter().all(|&c| c >= 180))
        .collect()
}

#[test]
fn the_tag_is_black_at_60_percent_in_a_rounded_rectangle_with_the_magnification_in_white() {
    let canvas = Canvas::new(640, 640);
    let photo = photo(&canvas);
    // The same in both palettes.
    for mode in MODES {
        for dpi in [96, 168] {
            let fixture = Fixture {
                tag: true,
                ..Fixture::new(dpi, Magnification::X8)
            };
            let (picture, view, text) = render_with_tag(&canvas, &photo, mode, &fixture);
            let tag = tag_rect(&view, dpi, text);
            let context = format!("{mode:?} at {dpi} DPI");
            // The photo under it, as it is without a tag.
            let under = |x: i32, y: i32| wanted(&view, mode, x, y).0;
            let darkened = |x: i32, y: i32| {
                // Black at 60% leaves 40% of the color.
                // TODO: mix in whole numbers, so no float comes back to `u8`.
                under(x, y).map(|c| (f32::from(c) * 0.4).round() as u8)
            };
            // The padding above and below the text, past the corner's arc: the photo at 40%, from the tag's first row to
            // its last.
            let x = tag.left + scaled(6, dpi) + 1;
            for y in [tag.top, tag.top + 1, tag.bottom - 2, tag.bottom - 1] {
                assert!(
                    near(at(&picture, x, y), darkened(x, y), 2),
                    "({x}, {y}) is {:?}, wanted {:?}, {context}",
                    at(&picture, x, y),
                    darkened(x, y)
                );
            }
            // The rectangle is exactly this tall and as wide as its text and padding: the pixels beside it are the photo.
            let middle = (tag.top + tag.bottom) / 2;
            for (x, y) in [
                (x, tag.top - 1),
                (x, tag.bottom),
                (tag.left - 1, middle),
                (tag.right, middle),
            ] {
                assert!(
                    near(at(&picture, x, y), under(x, y), 1),
                    "({x}, {y}), beside the tag, is {:?}, wanted {:?}, {context}",
                    at(&picture, x, y),
                    under(x, y)
                );
            }
            for x in [tag.left + 1, tag.right - 2] {
                assert!(
                    near(at(&picture, x, middle), darkened(x, middle), 2),
                    "({x}, {middle}), in the tag's padding, is {:?}, {context}",
                    at(&picture, x, middle)
                );
            }
            // Its corners are round: the corner pixel is the photo's, not the tag's.
            for (x, y) in [
                (tag.left, tag.top),
                (tag.right - 1, tag.top),
                (tag.left, tag.bottom - 1),
                (tag.right - 1, tag.bottom - 1),
            ] {
                assert!(
                    near(at(&picture, x, y), under(x, y), 8),
                    "the tag's corner ({x}, {y}) is {:?}, wanted the photo's {:?}, {context}",
                    at(&picture, x, y),
                    under(x, y)
                );
            }
            // Its text is white, inside the padding, and nowhere else in the tag is anything that white.
            let text_area = rect(
                tag.left + scaled(6, dpi) - 1,
                tag.top + scaled(4, dpi) - 1,
                tag.right - scaled(6, dpi) + 1,
                tag.bottom - scaled(4, dpi) + 1,
            );
            // Over the photo's own pixels, which can be bright, white says nothing; in the tag, the photo is at 40%.
            let ink = white_ink(&picture, tag);
            assert!(ink.len() > 20, "{} white pixels, {context}", ink.len());
            assert!(
                ink.iter().all(|&(x, y)| inside(&text_area, x, y)),
                "white pixels outside the tag's text area, {context}"
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
fn the_tag_is_there_when_the_view_says_so_and_not_otherwise() {
    let canvas = Canvas::new(640, 640);
    let photo = photo(&canvas);
    for tag in [false, true] {
        let fixture = Fixture {
            tag,
            ..Fixture::new(96, Magnification::X8)
        };
        let (picture, view, text) = render_with_tag(&canvas, &photo, Mode::Dark, &fixture);
        let area = tag_rect(&view, 96, text);
        // The whole frame outside the tag is the model's, tag or no tag; inside it, without a tag, too.
        let skip = tag.then_some(area);
        check_frame(&picture, &view, Mode::Dark, 96, skip);
        let ink = white_ink(&picture, area).len();
        assert_eq!(
            ink > 0,
            tag,
            "{ink} white pixels in the tag's place, tag {tag}"
        );
    }
}

#[test]
fn the_tags_text_is_the_magnification_and_a_times_sign() {
    let canvas = Canvas::new(640, 640);
    let photo = photo(&canvas);
    let mut tags = Vec::new();
    for magnification in magnifications() {
        let fixture = Fixture {
            tag: true,
            ..Fixture::new(96, magnification)
        };
        let (picture, view, text) = render_with_tag(&canvas, &photo, Mode::Dark, &fixture);
        let area = tag_rect(&view, 96, text);
        tags.push((magnification.factor(), text, white_ink(&picture, area)));
    }
    let found = |factor: u32| {
        tags.iter()
            .find(|(f, ..)| *f == factor)
            .expect("a tag for every magnification")
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

/// The widths of one coordinate's texts, each laid out again on its own, in whole pixels: the key, the physical number
/// and, when there is one, the logical number (0 when there is none).
#[derive(Clone, Copy)]
struct Widths {
    key: i32,
    physical: i32,
    logical: i32,
}

/// A view drawn, with what the tests know about its strip from outside the drawing code.
struct Drawn {
    picture: Picture,
    view: MagnifiedView,
    mode: Mode,
    dpi: u32,
    x: Widths,
    y: Widths,
}

/// `view` drawn in `mode` at `dpi`, with the widths of its strip's texts.
fn draw_view(
    canvas: &Canvas,
    photo: &ID2D1Bitmap,
    mode: Mode,
    dpi: u32,
    view: MagnifiedView,
) -> Drawn {
    let mut found = None;
    let picture = canvas.render_in(mode, SCREEN, dpi, |overlay, target| {
        overlay
            .draw_magnified_view(target, &view, photo)
            .expect("the magnified view");
        let widths = |key: &str, coordinate: &Coordinate| Widths {
            key: overlay
                .typeset_plain(key, &overlay.logical_format)
                .expect("the key")
                .width,
            physical: text_width(overlay, &coordinate.label.physical, &overlay.format),
            logical: coordinate
                .label
                .logical
                .as_deref()
                .map_or(0, |text| text_width(overlay, text, &overlay.logical_format)),
        };
        found = Some((widths("X", &view.x), widths("Y", &view.y)));
    });
    let (x, y) = found.expect("the view was drawn");
    Drawn {
        picture,
        view,
        mode,
        dpi,
        x,
        y,
    }
}

impl Drawn {
    fn sized(&self, logical: u32) -> i32 {
        scaled(logical, self.dpi)
    }

    /// The strip: under the frame, which holds the lines and the padding around them and nothing else. A few pixels in
    /// from the panel's border and its rounded corners, where the border's own antialiasing would look like ink.
    fn strip(&self) -> RECT {
        let (panel, frame) = (ints(self.view.panel), ints(self.view.frame));
        let side = self.sized(1) + self.sized(3);
        let below = self.sized(1) + self.sized(6);
        rect(
            panel.left + side,
            frame.bottom,
            panel.right - side,
            panel.bottom - below,
        )
    }

    fn panel_color(&self) -> [u8; 3] {
        tokens(self.mode).panel_over(BACKGROUND)
    }

    /// The pixels of `area` that differ from the panel by more than `by` levels of some channel: ink.
    fn ink(&self, area: RECT, by: u8) -> Vec<(i32, i32)> {
        let panel = self.panel_color();
        pixels(area)
            .filter(|&(x, y)| !near(at(&self.picture, x, y), panel, by))
            .collect()
    }

    /// How far the strongest pixel of `area` is from the panel, in levels of green: the ink at full strength is as far as
    /// the palette's ink is from the panel, and fainter ink is nearer.
    fn contrast(&self, area: RECT) -> u8 {
        let panel = self.panel_color()[1];
        pixels(area)
            .map(|(x, y)| at(&self.picture, x, y)[1].abs_diff(panel))
            .max()
            .expect("an area with pixels")
    }

    /// A coordinate's line and widths: X's, or Y's.
    fn of(&self, y: bool) -> (RECT, Widths) {
        if y {
            (ints(self.view.y.line), self.y)
        } else {
            (ints(self.view.x.line), self.x)
        }
    }

    /// Where the parts of a coordinate's line start, as the prototype's flex row lays them out: the key's box, at least 14
    /// wide, a gap, the physical number, a gap and a margin, the bar, a margin and a gap, the logical number. Each size
    /// is scaled on its own.
    fn columns(&self, y: bool) -> (i32, i32, i32) {
        let (line, widths) = self.of(y);
        let physical = line.left + widths.key.max(self.sized(14)) + self.sized(8);
        let bar = physical + widths.physical + self.sized(8) + self.sized(7);
        let logical = bar + self.sized(1) + self.sized(7) + self.sized(8);
        (physical, bar, logical)
    }
}

/// The widest realistic strip: a monitor 3840 by 2560 at 150% or 175%, the cursor in the far corner. At 100% there is no
/// logical part.
fn widest(dpi: u32, magnification: Magnification) -> Fixture {
    Fixture {
        labels: [("3839", "2559.3"), ("2159", "1439.3")],
        ..Fixture::new(dpi, magnification)
    }
}

#[test]
fn the_strips_texts_stay_inside_their_lines_and_the_panel_for_the_widest_numbers() {
    let canvas = Canvas::new(640, 640);
    let photo = photo(&canvas);
    // 32x is the narrowest frame, so the narrowest strip. 100% has the lines side by side, the others one under the other.
    for mode in MODES {
        for dpi in [96, 120, 144, 168] {
            let fixture = widest(dpi, Magnification::X32);
            let drawn = draw_view(&canvas, &photo, mode, dpi, fixture.view());
            let context = format!("{mode:?} at {dpi} DPI");
            let (x_line, y_line) = (ints(drawn.view.x.line), ints(drawn.view.y.line));
            let strip = drawn.strip();
            // Everything in the strip is in one of the two lines, which are inside the panel's border.
            let ink = drawn.ink(strip, 8);
            assert!(ink.len() > 100, "{} ink pixels, {context}", ink.len());
            for &(x, y) in &ink {
                assert!(
                    inside(&x_line, x, y) || inside(&y_line, x, y),
                    "ink at ({x}, {y}) outside the lines {x_line:?} and {y_line:?}, {context}"
                );
            }
            // Each coordinate's number and bar are inside its own line: what changes when its label is blanked.
            for (y_axis, line) in [(false, x_line), (true, y_line)] {
                let mut blanked = drawn.view.clone();
                let coordinate = if y_axis {
                    &mut blanked.y
                } else {
                    &mut blanked.x
                };
                coordinate.label = Label {
                    physical: " ".to_owned(),
                    logical: None,
                };
                let without = draw_view(&canvas, &photo, mode, dpi, blanked);
                let changed: Vec<(i32, i32)> = pixels(strip)
                    .filter(|&(x, y)| at(&drawn.picture, x, y) != at(&without.picture, x, y))
                    .collect();
                assert!(changed.len() > 30, "{} pixels, {context}", changed.len());
                for &(x, y) in &changed {
                    assert!(
                        inside(&line, x, y),
                        "{} text at ({x}, {y}) outside its line {line:?}, {context}",
                        if y_axis { "Y's" } else { "X's" }
                    );
                }
                // A logical part, when there is one, ends before the line does.
                if drawn.view.x.label.logical.is_some() {
                    let right = changed.iter().map(|&(x, _)| x).max().expect("ink");
                    assert!(
                        right < line.right,
                        "ink to column {right}, line {line:?}, {context}"
                    );
                }
            }
            // Side by side, the lines don't meet, and neither do the texts.
            if drawn.view.x.label.logical.is_none() {
                assert!(
                    x_line.right < y_line.left,
                    "{x_line:?} and {y_line:?}, {context}"
                );
                assert!(
                    ink.iter().any(|&(x, _)| x >= y_line.left),
                    "Y has no ink, {context}"
                );
                assert!(
                    ink.iter().any(|&(x, _)| x < x_line.right),
                    "X has no ink, {context}"
                );
            }
        }
    }
}

#[test]
fn the_parts_of_a_line_sit_a_key_box_a_gap_a_bar_and_a_gap_apart() {
    let canvas = Canvas::new(640, 640);
    let photo = photo(&canvas);
    for mode in MODES {
        for dpi in [120, 144, 168] {
            let drawn = draw_view(
                &canvas,
                &photo,
                mode,
                dpi,
                Fixture::new(dpi, Magnification::X16).view(),
            );
            let ink = tokens(mode).ink;
            for y_axis in [false, true] {
                let (line, widths) = drawn.of(y_axis);
                let (physical, bar, logical) = drawn.columns(y_axis);
                let context = format!("{mode:?} at {dpi} DPI, line {y_axis}");
                let rows = |left: i32, right: i32| rect(left, line.top, right, line.bottom);
                // The key is first, and the room up to the number, which is the key's box and a gap, is empty after it.
                assert!(
                    !drawn
                        .ink(rows(line.left, line.left + widths.key), 20)
                        .is_empty(),
                    "{context}"
                );
                let gap = drawn.ink(rows(line.left + widths.key + 1, physical - 1), 8);
                assert!(
                    gap.is_empty(),
                    "{} ink pixels between the key and the number, {context}",
                    gap.len()
                );
                // The number starts there, and ends before the gap that leads to the bar, where the bar is.
                assert!(
                    !drawn
                        .ink(rows(physical, physical + drawn.sized(6)), 20)
                        .is_empty(),
                    "{context}"
                );
                let gap = drawn.ink(rows(physical + widths.physical, bar), 8);
                assert!(
                    gap.is_empty(),
                    "{} ink pixels between the number and the bar, {context}",
                    gap.len()
                );
                // The bar is a pixel of the ink at 40%, as wide and as tall as the prototype's, scaled, in the middle
                // of the line, with the panel on every side.
                let (width, height) = (drawn.sized(1), drawn.sized(11));
                let top = line.top + (line.bottom - line.top - height + 1) / 2;
                let faint = over(ink, 0.4, drawn.panel_color());
                for (x, y) in pixels(rect(bar, top, bar + width, top + height)) {
                    assert!(
                        near(at(&drawn.picture, x, y), faint, 2),
                        "the bar at ({x}, {y}) is {:?}, wanted {faint:?}, {context}",
                        at(&drawn.picture, x, y)
                    );
                }
                let around = [
                    (bar - 1, top + 2),
                    (bar + width, top + 2),
                    (bar, top - 1),
                    (bar, top + height),
                ];
                for (x, y) in around {
                    assert!(
                        near(at(&drawn.picture, x, y), drawn.panel_color(), 1),
                        "({x}, {y}) beside the bar is {:?}, {context}",
                        at(&drawn.picture, x, y)
                    );
                }
                // The logical number follows the bar and a margin and a gap.
                let gap = drawn.ink(rows(bar + width, logical), 8);
                assert!(
                    gap.is_empty(),
                    "{} ink pixels between the bar and the logical number, {context}",
                    gap.len()
                );
                assert!(
                    !drawn
                        .ink(rows(logical, logical + widths.logical), 20)
                        .is_empty(),
                    "{context}"
                );
            }
        }
    }
}

#[test]
fn the_logical_part_is_fainter_than_the_physical_part_and_the_key_is_dim() {
    let canvas = Canvas::new(640, 640);
    let photo = photo(&canvas);
    for mode in MODES {
        let drawn = draw_view(
            &canvas,
            &photo,
            mode,
            144,
            Fixture::new(144, Magnification::X16).view(),
        );
        let t = tokens(mode);
        let context = format!("{mode:?}");
        for y_axis in [false, true] {
            let (line, widths) = drawn.of(y_axis);
            let (physical, _, logical) = drawn.columns(y_axis);
            let rows = |left: i32, right: i32| rect(left, line.top, right, line.bottom);
            let strong = drawn.contrast(rows(physical, physical + widths.physical));
            let faint = drawn.contrast(rows(logical, logical + widths.logical));
            let key = drawn.contrast(rows(line.left, line.left + widths.key));
            // The physical number reaches the ink all through; the logical one at CSS opacity .62 and the bar at .4.
            let ink_distance = t.ink[1].abs_diff(drawn.panel_color()[1]);
            assert!(
                strong.abs_diff(ink_distance) <= 6,
                "{strong} against {ink_distance}, {context}"
            );
            let ratio = f32::from(faint) / f32::from(strong);
            assert!(
                (0.5..0.72).contains(&ratio),
                "the logical part is {ratio} of the physical one, {context}"
            );
            // The key is in the dim color: between the two, the ink's own contrast being the greater.
            let dim_distance = dim(mode)[1].abs_diff(drawn.panel_color()[1]);
            assert!(
                key.abs_diff(dim_distance) <= 14,
                "{key} against {dim_distance}, {context}"
            );
            assert!(
                key + 20 < strong,
                "the key at {key} is not dimmer than the number at {strong}, {context}"
            );
        }
    }
}

#[test]
fn the_texts_of_a_line_share_a_baseline_and_sit_in_the_middle_of_the_line() {
    let canvas = Canvas::new(640, 640);
    let photo = photo(&canvas);
    for mode in MODES {
        for dpi in [96, 144, 168] {
            // At 100% the lines are side by side and have no logical part: the key and the number share a baseline.
            let drawn = draw_view(
                &canvas,
                &photo,
                mode,
                dpi,
                Fixture::new(dpi, Magnification::X16).view(),
            );
            for y_axis in [false, true] {
                let (line, widths) = drawn.of(y_axis);
                let (physical, _, logical) = drawn.columns(y_axis);
                let context = format!("{mode:?} at {dpi} DPI, line {y_axis}");
                let extent = |left: i32, right: i32| {
                    let rows: Vec<i32> = drawn
                        .ink(rect(left, line.top, right, line.bottom), 40)
                        .iter()
                        .map(|&(_, y)| y)
                        .collect();
                    (
                        *rows.iter().min().expect("strong ink"),
                        *rows.iter().max().expect("strong ink"),
                    )
                };
                let (key_top, key_bottom) = extent(line.left, line.left + widths.key);
                let (number_top, number_bottom) = extent(physical, physical + widths.physical);
                // Capital letters and digits stand on the same line, the same height or so.
                assert_eq!(key_bottom, number_bottom, "{context}");
                assert!(
                    key_top.abs_diff(number_top) <= 1,
                    "{key_top} against {number_top}, {context}"
                );
                if widths.logical > 0 {
                    let (_, logical_bottom) = extent(logical, logical + widths.logical);
                    assert_eq!(
                        logical_bottom, number_bottom,
                        "the logical number's baseline, {context}"
                    );
                }
                // In the middle of the line, as CSS centers a line of text: a digit has room above it and below, within
                // what the font's ascent and descent leave, which is a pixel or two more above than below.
                let (above, below) = (number_top - line.top, line.bottom - 1 - number_bottom);
                assert!(
                    above > 0 && below > 0,
                    "{above} above and {below} below, {context}"
                );
                assert!(
                    above.abs_diff(below) <= u32::try_from(drawn.sized(2) + 1).expect("a size"),
                    "{above} above and {below} below, {context}"
                );
            }
        }
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
                tag: true,
                labels: [("3839", "2559.3"), ("2159", "1439.3")],
                ..Fixture::new(dpi, magnification)
            };
            // The view with the cursor in the middle, and with it near the monitor's corner, where the frame is partly
            // blank: the frame's clip and the tag are at the edges of what is drawn.
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
            alpha * (1.0 - super::shadow::normal_cdf(beyond_the_edge / sigma))
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
