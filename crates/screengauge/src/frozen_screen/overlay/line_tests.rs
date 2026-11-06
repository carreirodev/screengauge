//! Dimensions' line, its ticks and its outline, pixel by pixel.

use super::tests::{
    BACKGROUND, CYAN, Canvas, MONITOR, cross, expected, horizontal, near, vertical,
};
use super::*;

/// The lines of `m` alone: its outline, then its cyan pixels.
fn draw_lines(overlay: &Overlay, target: &ID2D1RenderTarget, m: &Measurement) {
    let parts = [geometry::parts(m, overlay.dpi())];
    overlay.draw_outlines(target, &parts);
    overlay.draw_cyan(target, &parts);
}

/// Checks every pixel of the window against the model, the line alone with no label.
fn check_line(canvas: &Canvas, m: &Measurement, dpi: u32) {
    let k = scaled(4, dpi);
    let picture = canvas.render(MONITOR, dpi, |overlay, target| {
        draw_lines(overlay, target, m);
    });
    for y in 0..canvas.height as usize {
        for x in 0..canvas.width as usize {
            // The window's pixel is the virtual screen's pixel minus the monitor's corner.
            let (vx, vy) = (x as i32 + MONITOR.left, y as i32 + MONITOR.top);
            let wanted = expected(m, k, vx, vy);
            let tolerance = if wanted == CYAN || wanted == BACKGROUND {
                0
            } else {
                1
            };
            assert!(
                near(picture.at(x, y), wanted, tolerance),
                "pixel ({vx}, {vy}) at {dpi} DPI is {:?}, wanted {wanted:?}, for {m:?}",
                picture.at(x, y)
            );
        }
    }
}

#[test]
fn a_horizontal_line_is_its_pixels_in_cyan_with_the_outline_beside_them() {
    let canvas = Canvas::new(480, 320);
    for dpi in [96, 120, 168] {
        check_line(&canvas, &horizontal(-400, -301, 350, ""), dpi);
    }
}

#[test]
fn a_vertical_line_is_its_pixels_in_cyan_with_the_outline_beside_them() {
    let canvas = Canvas::new(480, 320);
    for dpi in [96, 120, 168] {
        check_line(&canvas, &vertical(230, 480, -200, ""), dpi);
    }
}

#[test]
fn a_cross_is_both_lines_without_darkening_where_they_meet() {
    let canvas = Canvas::new(480, 320);
    for dpi in [96, 120, 168] {
        check_line(
            &canvas,
            &cross((-350, -100), (230, 480), -230, 360, ""),
            dpi,
        );
    }
}

#[test]
fn spans_of_one_two_and_three_pixels_keep_each_outline_pixel_single() {
    let canvas = Canvas::new(480, 320);
    for dpi in [96, 168] {
        for last in [-300, -299, -298] {
            check_line(&canvas, &horizontal(-300, last, 350, ""), dpi);
            check_line(&canvas, &vertical(300, last + 600, -300, ""), dpi);
        }
        check_line(
            &canvas,
            &cross((-300, -300), (350, 350), -300, 350, ""),
            dpi,
        );
    }
}

#[test]
fn a_line_at_the_monitors_limit_is_cut_not_moved() {
    let canvas = Canvas::new(480, 320);
    // From the monitor's first column to its last, and down its first row.
    check_line(&canvas, &horizontal(-480, -1, 350, ""), 96);
    check_line(&canvas, &vertical(200, 519, -200, ""), 96);
    check_line(&canvas, &horizontal(-480, -1, 200, ""), 168);
}

#[test]
fn the_junctions_are_darkened_once() {
    let canvas = Canvas::new(480, 320);
    let m = horizontal(-400, -301, 350, "");
    let picture = canvas.render(MONITOR, 96, |overlay, target| {
        draw_lines(overlay, target, &m)
    });
    // The window's pixel for (-400, 350) is (80, 150). The pixel diagonal to the span's first pixel is in the
    // outline of both the line and the tick.
    let once = BACKGROUND.map(|c| (f32::from(c) * 0.72).round() as u8);
    assert!(
        near(picture.at(81, 151), once, 1),
        "{:?}",
        picture.at(81, 151)
    );
    assert!(
        near(picture.at(79, 149), once, 1),
        "{:?}",
        picture.at(79, 149)
    );
    // The outline's own color is not twice 28%, which would be 52% of the background.
    assert!(picture.at(81, 151)[0] > 135);
}

#[test]
fn a_tick_is_two_k_plus_one_pixels_long_at_every_scale() {
    let canvas = Canvas::new(480, 320);
    for (dpi, k) in [(96, 4), (120, 5), (168, 7)] {
        let m = horizontal(-400, -301, 350, "");
        let picture = canvas.render(MONITOR, dpi, |overlay, target| {
            draw_lines(overlay, target, &m)
        });
        // The first and last pixels' columns.
        for x in [80, 179] {
            let cyan = (0..320).filter(|&y| picture.at(x, y) == CYAN).count();
            assert_eq!(cyan, 2 * k + 1, "the tick at column {x}, {dpi} DPI");
        }
        // A column in the middle only has the line.
        assert_eq!((0..320).filter(|&y| picture.at(130, y) == CYAN).count(), 1);
    }
}

#[test]
fn the_ticks_of_a_vertical_line_are_two_k_plus_one_pixels_wide() {
    let canvas = Canvas::new(480, 320);
    let m = vertical(230, 480, -200, "");
    let picture = canvas.render(MONITOR, 120, |overlay, target| {
        draw_lines(overlay, target, &m)
    });
    // Rows 30 and 280 of the window hold the first and last pixels.
    for y in [30, 280] {
        assert_eq!((0..480).filter(|&x| picture.at(x, y) == CYAN).count(), 11);
    }
    assert_eq!((0..480).filter(|&x| picture.at(x, 100) == CYAN).count(), 1);
}

#[test]
fn the_line_is_one_pixel_thick_at_168_dpi_with_one_pixel_of_outline_each_side() {
    let canvas = Canvas::new(480, 320);
    let m = horizontal(-400, -301, 350, "");
    let picture = canvas.render(MONITOR, 168, |overlay, target| {
        draw_lines(overlay, target, &m)
    });
    // Column 130 of the window is in the middle of the span, and row 150 is its row.
    let column: Vec<[u8; 3]> = (0..320).map(|y| picture.at(130, y)).collect();
    assert_eq!(column.iter().filter(|&&p| p == CYAN).count(), 1);
    assert_eq!(column[150], CYAN);
    let shaded = BACKGROUND.map(|c| (f32::from(c) * 0.72).round() as u8);
    assert!(near(column[149], shaded, 1) && near(column[151], shaded, 1));
    assert_eq!(column[148], BACKGROUND, "two rows away is untouched");
    assert_eq!(column[152], BACKGROUND);
}

#[test]
fn the_crossing_pixel_is_pure_cyan() {
    let canvas = Canvas::new(480, 320);
    for dpi in [96, 168] {
        let m = cross((-350, -100), (230, 480), -230, 360, "");
        let picture = canvas.render(MONITOR, dpi, |overlay, target| {
            draw_lines(overlay, target, &m)
        });
        // (-230, 360) in the window.
        assert_eq!(picture.at(250, 160), CYAN, "at {dpi} DPI");
    }
}
