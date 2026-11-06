//! Render tests of Bounds on a WIC bitmap: the drawn rectangle's dashes, the content's fill, ring and outline, and where
//! the label goes. The sizes, colors and opacities are the approved prototype's (WPC-279, variant A: `dashedRect` and
//! `drawResult`), worked out here from the rectangles alone, not read back from the code that is being tested.

use screengauge_core::session::{DrawnRectangle, Label, Measured, Measurement, Rect};

use super::panel_tests::over;
use super::tests::{BACKGROUND, CYAN, Canvas, MONITOR, Picture, SCALES, near, text_width};
use super::*;

pub(super) fn rect(left: i32, top: i32, right: i32, bottom: i32) -> Rect {
    Rect {
        left,
        top,
        width: left.abs_diff(right) + 1,
        height: top.abs_diff(bottom) + 1,
    }
}

/// What Bounds measures for `content`, with its label reading `text` in physical pixels alone.
fn shrunk_to(content: Rect, text: &str) -> Measurement {
    Measurement {
        monitor: 0,
        measured: Measured::Content(content),
        label: Label {
            physical: text.to_owned(),
            logical: None,
        },
    }
}

/// `color` at `alpha` over the canvas's background, as a photo shows it.
fn over_background(color: [u8; 3], alpha: f32) -> [u8; 3] {
    over(color, alpha, BACKGROUND)
}

/// The window's pixel (`x`, `y`) of `picture`, by its place in the virtual screen.
pub(super) fn pixel(picture: &Picture, x: i32, y: i32) -> [u8; 3] {
    let column = usize::try_from(x - MONITOR.left).expect("a pixel in the window");
    let row = usize::try_from(y - MONITOR.top).expect("a pixel in the window");
    picture.at(column, row)
}

/// How far along the outer pixels of `r` the pixel (`x`, `y`) is, as the prototype's canvas strokes the rectangle:
/// clockwise from the top left pixel, along the top row, down the right column, back along the bottom row and up the
/// left column. `None` for a pixel that isn't one of them.
fn along_outline(r: Rect, x: i32, y: i32) -> Option<i32> {
    let (left, top, right, bottom) = (r.left, r.top, r.right(), r.bottom());
    let (w, h) = (right - left, bottom - top);
    if !(left..=right).contains(&x) || !(top..=bottom).contains(&y) {
        return None;
    }
    if y == top {
        Some(x - left)
    } else if x == right {
        Some(w + y - top)
    } else if y == bottom {
        Some(w + h + right - x)
    } else if x == left {
        Some(2 * w + h + bottom - y)
    } else {
        None
    }
}

/// What a pixel of the window is with only `drawn` drawn: a white dash, or the black between dashes over the
/// background, along its outer pixels, and the background everywhere else. The dashes are 4 logical pixels long, white
/// first (`setLineDash([4, 4])` in white over `rgba(0,0,0,.85)`).
fn dashed(drawn: Rect, dpi: u32, x: i32, y: i32) -> [u8; 3] {
    let dash = scaled(4, dpi);
    match along_outline(drawn, x, y) {
        Some(along) if (along / dash) % 2 == 0 => [255, 255, 255],
        Some(_) => over_background([0, 0, 0], 0.85),
        None => BACKGROUND,
    }
}

/// Checks every pixel of the window against `wanted`, within one level for the colors that are mixed.
pub(super) fn check_every_pixel(
    picture: &Picture,
    canvas: &Canvas,
    what: &str,
    wanted: impl Fn(i32, i32) -> [u8; 3],
) {
    let size = |pixels: u32| i32::try_from(pixels).expect("a small canvas");
    for row in 0..size(canvas.height) {
        for column in 0..size(canvas.width) {
            let (x, y) = (column + MONITOR.left, row + MONITOR.top);
            let wanted = wanted(x, y);
            assert!(
                near(pixel(picture, x, y), wanted, 1),
                "pixel ({x}, {y}) is {:?}, wanted {wanted:?}, for {what}",
                pixel(picture, x, y)
            );
        }
    }
}

#[test]
fn the_drawn_rectangle_is_white_dashes_over_black_along_its_outer_pixels() {
    let canvas = Canvas::new(480, 320);
    let drawn = DrawnRectangle {
        monitor: 0,
        rect: rect(-430, 230, -251, 361),
    };
    for dpi in SCALES {
        let picture = canvas.render(MONITOR, dpi, |overlay, target| {
            overlay
                .draw(target, &[], None, Some(&drawn))
                .expect("nothing to lay out");
        });
        check_every_pixel(
            &picture,
            &canvas,
            &format!("{drawn:?} at {dpi} DPI"),
            |x, y| dashed(drawn.rect, dpi, x, y),
        );
    }
}

#[test]
fn a_drawn_rectangle_one_pixel_tall_or_wide_has_each_pixel_once() {
    // A drag along one row or one column: its outer pixels are the row or the column itself, darkened once.
    let canvas = Canvas::new(480, 320);
    for drawn in [rect(-430, 300, -251, 300), rect(-300, 220, -300, 400)] {
        let picture = canvas.render(MONITOR, 168, |overlay, target| {
            let drawn = DrawnRectangle {
                monitor: 0,
                rect: drawn,
            };
            overlay
                .draw(target, &[], None, Some(&drawn))
                .expect("nothing to lay out");
        });
        check_every_pixel(&picture, &canvas, &format!("{drawn:?}"), |x, y| {
            dashed(drawn, 168, x, y)
        });
    }
}

/// Whether (`x`, `y`) is one of the pixels of the ring `by` pixels outside `content`, all around it.
fn on_ring(content: Rect, by: i32, x: i32, y: i32) -> bool {
    let (left, top, right, bottom) = (
        content.left - by,
        content.top - by,
        content.right() + by,
        content.bottom() + by,
    );
    let inside = (left..=right).contains(&x) && (top..=bottom).contains(&y);
    inside && (x == left || x == right || y == top || y == bottom)
}

/// What a pixel of the window is for the content of a Bounds measurement drawn without its label: the content filled
/// with cyan at 16% while live and 10% once pinned, the ring of pixels just outside it in cyan, the ring outside that in
/// black at 28% over the background, and the background everywhere else.
fn around_content(content: Rect, look: Look, x: i32, y: i32) -> [u8; 3] {
    let inside = (content.left..=content.right()).contains(&x)
        && (content.top..=content.bottom()).contains(&y);
    if inside {
        over_background(CYAN, if look == Look::Live { 0.16 } else { 0.1 })
    } else if on_ring(content, 1, x, y) {
        CYAN
    } else if on_ring(content, 2, x, y) {
        over_background([0, 0, 0], 0.28)
    } else {
        BACKGROUND
    }
}

/// The fill, the outline and the ring of `content`, in the order `draw` lays them, without the label.
fn draw_content(overlay: &Overlay, target: &ID2D1RenderTarget, content: Rect, look: Look) {
    let parts = [bounds::parts(content)];
    overlay
        .draw_fills(target, &[(content, look)])
        .expect("a small content");
    overlay.draw_outlines(target, &parts);
    overlay.draw_cyan(target, &parts);
}

#[test]
fn the_content_is_light_cyan_with_a_cyan_ring_just_outside_it_and_the_outline_beyond() {
    let canvas = Canvas::new(480, 320);
    let content = rect(-380, 300, -281, 339);
    for look in [Look::Live, Look::Pinned] {
        for dpi in SCALES {
            let picture = canvas.render(MONITOR, dpi, |overlay, target| {
                draw_content(overlay, target, content, look);
            });
            check_every_pixel(
                &picture,
                &canvas,
                &format!("{look:?} at {dpi} DPI"),
                |x, y| around_content(content, look, x, y),
            );
        }
    }
}

#[test]
fn a_content_of_one_pixel_still_has_its_ring_and_outline_whole() {
    let canvas = Canvas::new(480, 320);
    let content = rect(-300, 300, -300, 300);
    let picture = canvas.render(MONITOR, 120, |overlay, target| {
        draw_content(overlay, target, content, Look::Live);
    });
    check_every_pixel(&picture, &canvas, "one pixel", |x, y| {
        around_content(content, Look::Live, x, y)
    });
}

#[test]
fn where_the_content_touches_the_drawn_rectangle_the_ring_is_drawn_over_the_dashes() {
    let canvas = Canvas::new(480, 320);
    let drawn = DrawnRectangle {
        monitor: 0,
        rect: rect(-430, 230, -251, 361),
    };
    // The content starts one pixel inside the drawn rectangle's left and top sides, so its ring lies on them.
    let content = rect(-429, 231, -330, 270);
    let picture = canvas.render(MONITOR, 168, |overlay, target| {
        overlay
            .draw(
                target,
                &[],
                Some(&shrunk_to(content, "100 × 40")),
                Some(&drawn),
            )
            .expect("the label is laid out");
    });
    for x in [-430, -400, -331] {
        assert_eq!(pixel(&picture, x, 230), CYAN, "({x}, 230)");
    }
    for y in [230, 250, 271] {
        assert_eq!(pixel(&picture, -430, y), CYAN, "(-430, {y})");
    }
}

/// Where the label of `measurement` is, by the prototype: its pill's left side on the ring's left column, and its
/// bottom 6 logical pixels above the ring's top row; or, where the monitor has no room above, inside the ring, 4 logical
/// pixels right and 5 down of its top left corner. The pill is the live one's size for `text`.
fn wanted_pill(overlay: &Overlay, content: Rect, text: &str, dpi: u32, above: bool) -> RECT {
    let width = text_width(overlay, text, &overlay.format) + 2 * scaled(8, dpi);
    let height = scaled(12, dpi) + 2 * scaled(5, dpi);
    let (corner_x, corner_y) = (content.left - 1, content.top - 1);
    let (left, top) = if above {
        (corner_x, corner_y - scaled(6, dpi) - height)
    } else {
        (corner_x + scaled(4, dpi), corner_y + scaled(5, dpi))
    };
    RECT {
        left,
        top,
        right: left + width,
        bottom: top + height,
    }
}

/// Checks the live pill's sides at `pill`: opaque cyan just inside each side, along its middle, and not just outside.
pub(super) fn check_live_pill(picture: &Picture, pill: RECT, what: &str) {
    let middle_row = (pill.top + pill.bottom) / 2;
    // Left of the text, inside the padding.
    let padding_column = pill.left + 2;
    let middle_column = (pill.left + pill.right) / 2;
    assert_eq!(
        pixel(picture, pill.left, middle_row),
        CYAN,
        "left side, {what}"
    );
    assert_ne!(
        pixel(picture, pill.left - 1, middle_row),
        CYAN,
        "left of it, {what}"
    );
    assert_eq!(
        pixel(picture, pill.right - 1, middle_row),
        CYAN,
        "right side, {what}"
    );
    assert_ne!(
        pixel(picture, pill.right, middle_row),
        CYAN,
        "right of it, {what}"
    );
    assert_eq!(
        pixel(picture, middle_column, pill.top),
        CYAN,
        "top side, {what}"
    );
    assert_ne!(
        pixel(picture, middle_column, pill.top - 1),
        CYAN,
        "above it, {what}"
    );
    assert_eq!(
        pixel(picture, middle_column, pill.bottom - 1),
        CYAN,
        "bottom side, {what}"
    );
    assert_ne!(
        pixel(picture, middle_column, pill.bottom),
        CYAN,
        "below it, {what}"
    );
    assert_eq!(
        pixel(picture, padding_column, middle_row),
        CYAN,
        "padding, {what}"
    );
}

#[test]
fn the_live_label_sits_above_the_rings_top_left_corner() {
    let canvas = Canvas::new(480, 320);
    let content = rect(-380, 300, -281, 339);
    for dpi in SCALES {
        let mut pill = None;
        let picture = canvas.render(MONITOR, dpi, |overlay, target| {
            pill = Some(wanted_pill(overlay, content, "100 × 40", dpi, true));
            overlay
                .draw(target, &[], Some(&shrunk_to(content, "100 × 40")), None)
                .expect("the label is laid out");
        });
        let pill = pill.expect("drawn");
        check_live_pill(&picture, pill, &format!("{dpi} DPI"));
    }
}

#[test]
fn with_no_room_above_the_label_goes_inside_the_ring_at_its_corner() {
    let canvas = Canvas::new(480, 320);
    // 5 pixels under the monitor's top: the label would need 6 logical pixels and its own height above the ring.
    let content = rect(-380, 205, -181, 300);
    for dpi in SCALES {
        let mut pill = None;
        let picture = canvas.render(MONITOR, dpi, |overlay, target| {
            pill = Some(wanted_pill(overlay, content, "200 × 96", dpi, false));
            overlay
                .draw(target, &[], Some(&shrunk_to(content, "200 × 96")), None)
                .expect("the label is laid out");
        });
        let pill = pill.expect("drawn");
        check_live_pill(&picture, pill, &format!("{dpi} DPI"));
    }
}

#[test]
fn at_the_monitors_left_side_the_label_slides_inside_it() {
    let canvas = Canvas::new(480, 320);
    // The content starts on the monitor's first column, so its ring is just off the monitor.
    let content = rect(-480, 300, -381, 339);
    let picture = canvas.render(MONITOR, 168, |overlay, target| {
        overlay
            .draw(target, &[], Some(&shrunk_to(content, "100 × 40")), None)
            .expect("the label is laid out");
    });
    let middle_row = 299 - scaled(6, 168) - (scaled(12, 168) + 2 * scaled(5, 168)) / 2;
    assert_eq!(pixel(&picture, -480, middle_row), CYAN);
}

#[test]
fn a_pinned_labels_cyan_strip_starts_on_the_rings_left_column() {
    let canvas = Canvas::new(480, 320);
    let content = rect(-380, 300, -281, 339);
    for dpi in SCALES {
        let picture = canvas.render(MONITOR, dpi, |overlay, target| {
            overlay
                .draw(target, &[&shrunk_to(content, "100 × 40")], None, None)
                .expect("the label is laid out");
        });
        // The pinned pill is the live one's height and twice the ring taller; its bottom is where the live one's is.
        let height = scaled(12, dpi) + 2 * scaled(5, dpi) + 2 * scaled(1, dpi);
        let bottom = 299 - scaled(6, dpi);
        let middle_row = bottom - height / 2;
        let strip = scaled(3, dpi);
        for x in -381..-381 + strip {
            assert_eq!(
                pixel(&picture, x, middle_row),
                CYAN,
                "({x}, {middle_row}) at {dpi} DPI"
            );
        }
        assert_ne!(
            pixel(&picture, -382, middle_row),
            CYAN,
            "left of the strip at {dpi} DPI"
        );
        // The content under it is filled lighter than a live one's.
        assert!(
            near(pixel(&picture, -330, 320), over_background(CYAN, 0.1), 1),
            "the pinned fill at {dpi} DPI"
        );
    }
}
