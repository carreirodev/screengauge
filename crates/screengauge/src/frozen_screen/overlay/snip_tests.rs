//! Render tests of Snip on a WIC bitmap: the corners around the area, which leave every pixel of the image untouched,
//! and the label with the camera and the size. The sizes are the approved prototype's (WPC-279, variant A: `drawResult`
//! for `snip`, `IC.cam`, `.pill`), worked out here from the rectangles alone, not read back from the code being tested.

use screengauge_core::session::{Label, Rect, SnipArea};

use super::bounds_tests::{check_every_pixel, check_live_pill, pixel, rect};
use super::tests::{BACKGROUND, CYAN, Canvas, MONITOR, Picture, SCALES, near, text_width};
use super::*;

/// What the prototype does with `n` logical pixels on a monitor of `dpi`: `Math.round(n * dpr)`, which for these
/// positive sizes rounds halves up.
fn device(n: u32, dpi: u32) -> i32 {
    i32::try_from((2 * n * dpi + 96) / 192).expect("a few dozen pixels")
}

/// Whether (`x`, `y`) is on one of the corners the prototype fills around `r`: at each corner a bar `arm` long and 2
/// thick along the row and one along the column, from 2 pixels outside the image, where `arm` is 14 logical pixels, or
/// less for a small image, but never under 3.
fn on_corner(r: Rect, dpi: u32, x: i32, y: i32) -> bool {
    let (w, h) = (
        i32::try_from(r.width).expect("small"),
        i32::try_from(r.height).expect("small"),
    );
    let arm = device(14, dpi).min(w / 2).min(h / 2).max(3);
    let (x0, y0, x1, y1) = (r.left - 2, r.top - 2, r.left + w, r.top + h);
    // `fillRect(x, y, width, height)`.
    let fill = |fx: i32, fy: i32, fw: i32, fh: i32| {
        (fx..fx + fw).contains(&x) && (fy..fy + fh).contains(&y)
    };
    [
        (x0, y0, 1, 1),
        (x1, y0, -1, 1),
        (x0, y1, 1, -1),
        (x1, y1, -1, -1),
    ]
    .into_iter()
    .any(|(cx, cy, sx, sy)| {
        fill(if sx > 0 { cx } else { cx - arm + 2 }, cy, arm, 2)
            || fill(cx, if sy > 0 { cy } else { cy - arm + 2 }, 2, arm)
    })
}

#[test]
fn the_corners_are_cyan_two_pixels_thick_just_outside_the_image_and_cover_none_of_it() {
    let canvas = Canvas::new(480, 320);
    let area = rect(-380, 300, -281, 339);
    for dpi in SCALES {
        let picture = canvas.render(MONITOR, dpi, |overlay, target| {
            overlay.draw_corners(target, area);
        });
        check_every_pixel(&picture, &canvas, &format!("{dpi} DPI"), |x, y| {
            if on_corner(area, dpi, x, y) {
                CYAN
            } else {
                BACKGROUND
            }
        });
    }
}

#[test]
fn a_small_image_has_shorter_arms_and_never_under_three_pixels() {
    let canvas = Canvas::new(480, 320);
    // 4 × 2: half its height is 1, so the arms are 3 pixels long, and those around a 20 × 12 image are 6.
    for area in [rect(-300, 300, -297, 301), rect(-200, 300, -181, 311)] {
        for dpi in [96, 168] {
            let picture = canvas.render(MONITOR, dpi, |overlay, target| {
                overlay.draw_corners(target, area);
            });
            check_every_pixel(
                &picture,
                &canvas,
                &format!("{area:?} at {dpi} DPI"),
                |x, y| {
                    if on_corner(area, dpi, x, y) {
                        CYAN
                    } else {
                        BACKGROUND
                    }
                },
            );
        }
    }
}

fn snip_area(area: Rect, text: &str) -> SnipArea {
    SnipArea {
        monitor: 0,
        rect: area,
        label: Label {
            physical: text.to_owned(),
            logical: None,
        },
    }
}

/// Where Snip's label is, by the prototype: a live pill with a 12-pixel camera, a gap of 5 and the size, 8 pixels of
/// padding at either end and 5 above and below the 12-pixel text, its left side on the corners' left column and its
/// bottom 6 logical pixels above their top row; or, where the monitor has no room above, 4 logical pixels right and 6
/// down of the corners' top left pixel.
fn wanted_pill(overlay: &Overlay, area: Rect, text: &str, dpi: u32, above: bool) -> RECT {
    let s = |n| scaled(n, dpi);
    let width = s(8) + s(12) + s(5) + text_width(overlay, text, &overlay.format) + s(8);
    let height = s(12) + 2 * s(5);
    let (corner_x, corner_y) = (area.left - 2, area.top - 2);
    let (left, top) = if above {
        (corner_x, corner_y - s(6) - height)
    } else {
        (corner_x + s(4), corner_y + s(6))
    };
    RECT {
        left,
        top,
        right: left + width,
        bottom: top + height,
    }
}

#[test]
fn the_label_sits_above_the_corners_top_left_and_the_image_stays_unfilled() {
    let canvas = Canvas::new(480, 320);
    let area = rect(-380, 300, -281, 339);
    for dpi in SCALES {
        let mut pill = None;
        let picture = canvas.render(MONITOR, dpi, |overlay, target| {
            pill = Some(wanted_pill(overlay, area, "100 × 40", dpi, true));
            overlay
                .draw_snip(target, &snip_area(area, "100 × 40"))
                .expect("the label is laid out");
        });
        let pill = pill.expect("drawn");
        check_live_pill(&picture, pill, &format!("{dpi} DPI"));
        // No fill: no pixel of the image shows lighter than the photo has it, as cyan over it would make it. The pill's
        // faint shadow falls on its first rows, as the prototype's CSS shadow does, and only darkens them; past its
        // reach, 12 logical pixels down, every pixel is exactly the photo's. The image itself comes from the frozen screen.
        for y in area.top..=area.bottom() {
            for x in area.left..=area.right() {
                let shown = pixel(&picture, x, y);
                let darker = shown.iter().zip(BACKGROUND).all(|(&s, b)| s <= b);
                assert!(darker, "({x}, {y}) is {shown:?} at {dpi} DPI");
                if y >= area.top + scaled(12, dpi) {
                    assert_eq!(shown, BACKGROUND, "({x}, {y}) at {dpi} DPI");
                }
            }
        }
    }
}

#[test]
fn with_no_room_above_the_label_goes_inside_the_corners() {
    let canvas = Canvas::new(480, 320);
    // 5 pixels under the monitor's top: the label would need 6 logical pixels and its own height above the corners.
    let area = rect(-380, 205, -181, 300);
    for dpi in SCALES {
        let mut pill = None;
        let picture = canvas.render(MONITOR, dpi, |overlay, target| {
            pill = Some(wanted_pill(overlay, area, "200 × 96", dpi, false));
            overlay
                .draw_snip(target, &snip_area(area, "200 × 96"))
                .expect("the label is laid out");
        });
        check_live_pill(&picture, pill.expect("drawn"), &format!("{dpi} DPI"));
    }
}

/// How many pixels of `picture` in the columns `columns` and the rows `rows` are the label's dark ink, give or take
/// the edges the strokes and the letters blend with the cyan.
fn inked(picture: &Picture, columns: std::ops::Range<i32>, rows: std::ops::Range<i32>) -> usize {
    let ink = [0x04, 0x16, 0x1b];
    rows.flat_map(|y| columns.clone().map(move |x| (x, y)))
        .filter(|&(x, y)| near(pixel(picture, x, y), ink, 60))
        .count()
}

#[test]
fn the_camera_is_stroked_in_the_labels_ink_before_the_size_with_cyan_between_them() {
    let canvas = Canvas::new(480, 320);
    let area = rect(-380, 300, -281, 339);
    for dpi in SCALES {
        let s = |n| scaled(n, dpi);
        let mut pill = None;
        let picture = canvas.render(MONITOR, dpi, |overlay, target| {
            pill = Some(wanted_pill(overlay, area, "100 × 40", dpi, true));
            overlay
                .draw_snip(target, &snip_area(area, "100 × 40"))
                .expect("the label is laid out");
        });
        let pill = pill.expect("drawn");
        let camera = pill.left + s(8);
        let text = camera + s(12) + s(5);
        let rows = pill.top..pill.bottom;
        // The camera's body is 16 of its 24 units wide and 14 tall, stroked 2.4 units thick: dozens of dark pixels.
        let in_camera = inked(&picture, camera..camera + s(12), rows.clone());
        assert!(
            in_camera >= 20,
            "{in_camera} dark pixels in the camera at {dpi} DPI"
        );
        // The camera's strokes and the text's letters stay on their own sides of the gap, which is all cyan.
        for x in camera + s(12) + 1..text - 1 {
            for y in rows.clone() {
                assert_eq!(
                    pixel(&picture, x, y),
                    CYAN,
                    "the gap at ({x}, {y}), {dpi} DPI"
                );
            }
        }
        let in_text = inked(&picture, text..pill.right - s(8), rows);
        assert!(
            in_text >= 20,
            "{in_text} dark pixels in the size at {dpi} DPI"
        );
    }
}
