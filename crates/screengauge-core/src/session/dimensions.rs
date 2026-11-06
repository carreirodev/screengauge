//! Dimensions: the span of pixels around the cursor, from the edge on one side to the edge on the other, along the
//! cursor's row, its column or both (v1 spec WPC-296, Dimensions).

use super::pixels::{Reference, color};
use super::{Label, Measured, Measurement, Photo, Point, Span, units};
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
    let monitor = super::frozen_screen::monitor_at(frozen_screen, cursor)?;
    let photo = &frozen_screen[monitor];
    let along = |axis| span(photo, cursor, axis, tolerance);
    let (horizontal, vertical) = match mode {
        DimensionsMode::Horizontal => (Some(along(Axis::Row)), None),
        DimensionsMode::Vertical => (None, Some(along(Axis::Column))),
        DimensionsMode::Cross => (Some(along(Axis::Row)), Some(along(Axis::Column))),
    };
    let mut measurement = Measurement {
        monitor,
        measured: Measured::Spans {
            horizontal,
            vertical,
        },
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
    let reference = Reference::new(color(photo, cursor), tolerance);
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
    let same = |along: i32| !reference.differs(color(photo, point(along)));
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
