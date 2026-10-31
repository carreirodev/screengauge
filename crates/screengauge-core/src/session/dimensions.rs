//! Dimensions: the span of pixels around the cursor, from the edge on one side to the edge on the other, along the
//! cursor's row, its column or both (v1 spec WPC-296, Dimensions).

use super::{Label, Measurement, Photo, Point, Span, units};
use crate::settings::{DimensionsMode, Tolerance};

/// What Dimensions measures with the cursor at `cursor`, on the monitor under it. The label shows the logical pixels
/// too when `also_show_logical_pixels` is on and that monitor's scale isn't 100%.
pub(super) fn measure(
    frozen_screen: &[Photo],
    cursor: Point,
    mode: DimensionsMode,
    tolerance: Tolerance,
    also_show_logical_pixels: bool,
) -> Option<Measurement> {
    let monitor = frozen_screen
        .iter()
        .position(|photo| photo.rect.contains(cursor))?;
    let photo = &frozen_screen[monitor];
    let along = |axis| span(photo, cursor, axis, tolerance);
    let (horizontal, vertical) = match mode {
        DimensionsMode::Horizontal => (Some(along(Axis::Row)), None),
        DimensionsMode::Vertical => (None, Some(along(Axis::Column))),
        DimensionsMode::Cross => (Some(along(Axis::Row)), Some(along(Axis::Column))),
    };
    let mut measurement = Measurement {
        monitor,
        horizontal,
        vertical,
        label: Label::default(),
    };
    measurement.label = units::label(
        &measurement.lengths(),
        photo.scale,
        also_show_logical_pixels,
    );
    Some(measurement)
}

/// Which way a span runs.
#[derive(Clone, Copy)]
enum Axis {
    /// Along the cursor's row, left to right.
    Row,
    /// Along the cursor's column, top to bottom.
    Column,
}

/// The pixels around `cursor` along `axis` that don't differ from the color under the cursor, the reference color, by
/// more than `tolerance`, up to the edge on each side or the monitor's limit. Each pixel is compared with the reference
/// color, never with its neighbor.
fn span(photo: &Photo, cursor: Point, axis: Axis, tolerance: Tolerance) -> Span {
    let reference = color(photo, cursor);
    let rect = photo.rect;
    let (at, low, high) = match axis {
        Axis::Row => (cursor.x, rect.left, rect.right()),
        Axis::Column => (cursor.y, rect.top, rect.bottom()),
    };
    let point = |along: i32| match axis {
        Axis::Row => Point {
            x: along,
            y: cursor.y,
        },
        Axis::Column => Point {
            x: cursor.x,
            y: along,
        },
    };
    let same = |along: i32| !differs(reference, color(photo, point(along)), tolerance);
    let mut first = at;
    while first > low && same(first - 1) {
        first -= 1;
    }
    let mut last = at;
    while last < high && same(last + 1) {
        last += 1;
    }
    Span {
        first: point(first),
        last: point(last),
    }
}

/// The red, green and blue of the pixel at `point`, which must be on the monitor. The fourth byte says nothing.
fn color(photo: &Photo, point: Point) -> [u8; 3] {
    // `point` is on the monitor, so both differences are its place in the photo. u32 to usize is lossless on x64, the
    // only target ScreenGauge builds for.
    let column = point.x.abs_diff(photo.rect.left) as usize;
    let row = point.y.abs_diff(photo.rect.top) as usize;
    let at = (row * photo.rect.width as usize + column) * 4;
    let bgra = &photo.image.bgra()[at..at + 4];
    [bgra[2], bgra[1], bgra[0]]
}

/// Whether `pixel` differs from `reference` by more than `tolerance`: one of its red, green or blue differs by more
/// than three times the tolerance. The prototype's rule, provisional until the calibration (WPC-332).
fn differs(reference: [u8; 3], pixel: [u8; 3], tolerance: Tolerance) -> bool {
    let limit = u16::from(tolerance.get()) * 3;
    reference
        .iter()
        .zip(pixel)
        .any(|(&a, b)| u16::from(a.abs_diff(b)) > limit)
}
