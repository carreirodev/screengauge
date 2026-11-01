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

use super::panel_tests::{MODES, inside, rect, tokens};
use super::tests::{BACKGROUND, Canvas, Picture, near};
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
