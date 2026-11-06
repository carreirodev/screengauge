//! Bounds: a rectangle the user drags out, shrunk until each side touches the content inside it (v1 spec WPC-296,
//! Bounds).

use super::pixels::{self, Reference, color, rgb};
use super::{Label, Measured, Measurement, Photo, Point, Rect, units};
use crate::settings::Tolerance;

/// The drag distance: how many physical pixels the cursor may go from where the drag began, along each axis, with the
/// press still a click, which does nothing (v1 spec WPC-296, Bounds, "o limite se calibra na construção"). It is
/// Windows' own drag distance, SM_CXDRAG and SM_CYDRAG, the pixels on either side of a press the mouse may move before
/// a drag begins, which let a click come up without starting one: 4 at every scale from 100% to 200% on the owner's
/// computer, measured with GetSystemMetricsForDpi in WPC-314, so the same here whatever the monitor's scale. It is
/// fixed rather than read from Windows, so a drag distance changed in Windows isn't followed (the owner's choice in
/// WPC-314).
const DRAG_DISTANCE: u32 = 4;

/// A drag of Bounds or Snip, from the press until the button comes up.
#[derive(Debug, Clone, Copy)]
pub(super) struct Drag {
    /// Where the button went down: the drawn rectangle's fixed corner, and the place of the reference color.
    pub(super) start: Point,
    /// The monitor the drag began on, as an index in the frozen screen's order. The drawn rectangle never leaves it.
    pub(super) monitor: usize,
    /// Whether Alt is held, as the last event had it: Bounds then measures exactly the drawn rectangle, and Snip takes
    /// it, for photos, gradients and empty areas, without shrinking it (v1 spec WPC-296, Bounds and Snip).
    exact: bool,
    /// Whether the cursor has gone further than [`DRAG_DISTANCE`] from the start along either axis since the press.
    past_drag_distance: bool,
}

impl Drag {
    /// A drag begun at `start`, on `monitor`, exactly as drawn while `exact`.
    pub(super) fn new(start: Point, monitor: usize, exact: bool) -> Drag {
        Drag {
            start,
            monitor,
            exact,
            past_drag_distance: false,
        }
    }

    /// Notes that the cursor went to `cursor`, wherever it is, on this monitor or another. Once the cursor has gone
    /// further than the drag distance, the drag counts until the button comes up, even should the cursor come back near
    /// the start: the mouse went that far (the owner's choice in WPC-314).
    pub(super) fn went_to(&mut self, cursor: Point) {
        let far = |from: i32, to: i32| from.abs_diff(to) > DRAG_DISTANCE;
        self.past_drag_distance |= far(self.start.x, cursor.x) || far(self.start.y, cursor.y);
    }

    /// Has the drag measure exactly the drawn rectangle while `alt` is held, and shrink it to the content otherwise.
    /// Whether that changed what it measures.
    pub(super) fn follow_alt(&mut self, alt: bool) -> bool {
        let changed = self.exact != alt;
        self.exact = alt;
        changed
    }

    /// Whether the drag is still a click: the cursor hasn't gone past the drag distance since the press. Then nothing
    /// shows, and the button coming up does nothing, with or without Alt.
    pub(super) fn is_click(self) -> bool {
        !self.past_drag_distance
    }

    /// Whether the drawn rectangle shows dashed: once the drag counts, without Alt. With Alt, the drawn rectangle is
    /// what Bounds measures, and shows only as the live measurement, already in the result's style (v1 spec WPC-296,
    /// Como as medidas aparecem).
    pub(super) fn shows_dashes(self) -> bool {
        !self.is_click() && !self.exact
    }
}

/// The drawn rectangle of `drag` with the cursor at `cursor`: from the drag's start to the cursor, both corners
/// included, whichever way the drag went. Where the cursor has gone past the limit of the monitor the drag began on,
/// the rectangle stops at that limit (v1 spec WPC-296, Unidades e vários monitores).
pub(super) fn drawn_rectangle(photo: &Photo, drag: Drag, cursor: Point) -> Rect {
    let (start, end) = (drag.start, photo.rect.nearest(cursor));
    Rect {
        left: start.x.min(end.x),
        top: start.y.min(end.y),
        width: start.x.abs_diff(end.x) + 1,
        height: start.y.abs_diff(end.y) + 1,
    }
}

/// The area `drag` takes with the cursor at `cursor`, which Bounds measures and Snip takes the image of: the content
/// of the drawn rectangle, every pixel that differs from the reference color, the color where the drag began, by more
/// than `tolerance`, or, with Alt held, the drawn rectangle itself (v1 spec WPC-296, Bounds and Snip). `None` when
/// there is no content, and on `photo`, the photo of the monitor the drag began on.
pub(super) fn area(photo: &Photo, drag: Drag, cursor: Point, tolerance: Tolerance) -> Option<Rect> {
    let drawn = drawn_rectangle(photo, drag, cursor);
    if drag.exact {
        Some(drawn)
    } else {
        shrink(photo, drawn, color(photo, drag.start), tolerance)
    }
}

/// What Bounds measures for `drag` with the cursor at `cursor`: its [`area`], with a label. `None` when there is no
/// content. The label shows the logical pixels too when `also_show_logical_pixels` is on and the monitor's scale isn't
/// 100%.
pub(super) fn measure(
    frozen_screen: &[Photo],
    drag: Drag,
    cursor: Point,
    tolerance: Tolerance,
    also_show_logical_pixels: bool,
) -> Option<Measurement> {
    let Some(photo) = frozen_screen.get(drag.monitor) else {
        // Can't happen: a drag begins on a monitor of the frozen screen.
        return None;
    };
    let content = area(photo, drag, cursor, tolerance)?;
    let mut measurement = Measurement {
        monitor: drag.monitor,
        measured: Measured::Content(content),
        label: Label::default(),
    };
    measurement.label = units::label(
        &measurement.lengths(),
        photo.scale,
        also_show_logical_pixels,
    );
    Some(measurement)
}

/// `drawn`, which lies on `photo`'s monitor, with each side moved in to the first row or column holding a pixel that
/// differs from `reference` by more than `tolerance`: the smallest rectangle holding the content, never larger than
/// what was drawn. Pixels are compared with the reference color, never with their neighbors, so the content may be
/// several objects, and the gaps between them are inside the result. `None` when every pixel is like the reference.
///
/// It reads the photo a row at a time, the way it lies in memory, and each pixel at most once: a drag across a whole 4K
/// monitor of background reads every one of its pixels at each move of the mouse.
fn shrink(photo: &Photo, drawn: Rect, reference: [u8; 3], tolerance: Tolerance) -> Option<Rect> {
    let reference = Reference::new(reference, tolerance);
    let content = |pixel: &[u8; 4]| reference.differs(rgb(*pixel));
    let row = |y: i32| pixels::row(photo, y, drawn.left, drawn.width);
    let holds_content = |y: &i32| row(*y).iter().any(content);
    let top = (drawn.top..=drawn.bottom()).find(holds_content)?;
    // The top row holds content, so a row at or below it does.
    let bottom = (top..=drawn.bottom()).rev().find(holds_content)?;
    // How far into each row the first and the last pixel of content lie, as the top row has them; each row below can
    // only move them out, so only what lies beyond them is read.
    let mut first = row(top).iter().position(content)?;
    let mut last = row(top).iter().rposition(content)?;
    for y in top + 1..=bottom {
        let pixels = row(y);
        if let Some(further) = pixels[..first].iter().position(content) {
            first = further;
        }
        if let Some(further) = pixels[last + 1..].iter().rposition(content) {
            last += 1 + further;
        }
    }
    // Can't happen that either conversion fails: both lie inside the drawn rectangle, whose width is a u32 and whose
    // left side plus it an i32.
    let left = drawn
        .left
        .checked_add_unsigned(u32::try_from(first).ok()?)?;
    Some(Rect {
        left,
        top,
        width: u32::try_from(last - first + 1).ok()?,
        height: top.abs_diff(bottom) + 1,
    })
}
