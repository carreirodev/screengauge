//! The notice: a short message on a panel with a sign before its text, drawn above and to the right of where the cursor
//! was when it appeared (the prototype's notice, WPC-279). Where it goes needs no Windows, so [`size`] and [`place`]
//! are tested without a monitor; the rest draws it onto the overlay's render target.

use screengauge_core::session::{Point, Sign};
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Direct2D::Common::{D2D1_FIGURE_BEGIN_HOLLOW, D2D1_FIGURE_END_OPEN};
use windows::Win32::Graphics::Direct2D::{
    D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_ELLIPSE,
    D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE, ID2D1RenderTarget, ID2D1StrokeStyle,
};

use super::geometry::{self, Size, scaled};
use super::{Overlay, px, ring};

// Every size below is in logical pixels, which `scaled` turns into physical pixels for the monitor's scale.

/// The panel's corner radius.
const RADIUS: u32 = 10;
/// The room between the panel's border and what it holds.
const PAD_TOP: u32 = 7;
const PAD_RIGHT: u32 = 11;
const PAD_BOTTOM: u32 = 7;
const PAD_LEFT: u32 = 9;
/// The sign's side, the gap between it and the text, and the height of the text's line (CSS 1.25 times the 12px text).
const SIGN: u32 = 13;
const GAP: u32 = 7;
const LINE: u32 = 15;

/// How far the notice sits from where the cursor was: to the right and above it, and when there is no room, to its left
/// and below it.
const REACH: u32 = 14;
const BELOW: u32 = 18;
/// How near the monitor's right and top sides the notice may come before it moves to the other side of the cursor, and
/// how near the monitor's sides it may come at all.
const EDGE: u32 = 4;
const MARGIN: u32 = 2;

/// The shadow, CSS `0 10px 28px -8px`: it falls this far and is this much smaller than the panel on every side before
/// it blurs. The blur is `SHADOW_LAYERS` rounded rectangles, each this much bigger than the one before it.
const SHADOW_DROP: u32 = 10;
const SHADOW_SPREAD: u32 = 8;
const SHADOW_STEP: usize = 4;

/// The sign is drawn in the 24 by 24 units of its prototype, and scaled to `SIGN` pixels.
const SIGN_UNITS: f32 = 24.0;

/// The panel around a text `text_width` pixels wide: the border, the padding, the sign, a gap and the text.
pub fn size(text_width: i32, dpi: u32) -> Size {
    let ring = scaled(geometry::RING, dpi);
    Size {
        width: ring
            + scaled(PAD_LEFT, dpi)
            + scaled(SIGN, dpi)
            + scaled(GAP, dpi)
            + text_width
            + scaled(PAD_RIGHT, dpi)
            + ring,
        height: ring + scaled(PAD_TOP, dpi) + scaled(LINE, dpi) + scaled(PAD_BOTTOM, dpi) + ring,
    }
}

/// Where the panel goes: its rectangle in the virtual screen, right and bottom one past its last pixel. It sits `REACH`
/// right of `at` and `REACH` above it. Where it would come within `EDGE` of the monitor's right side it goes to the left
/// of `at` instead, and where it would come within `EDGE` of the monitor's top it goes `BELOW` below `at`. Where it
/// would still stick out, it slides inside, `MARGIN` from the sides; should the monitor be smaller than the panel, it
/// starts `MARGIN` inside the monitor's left and top sides.
pub fn place(at: Point, size: Size, monitor: RECT, dpi: u32) -> RECT {
    let (reach, below, edge, margin) = (
        scaled(REACH, dpi),
        scaled(BELOW, dpi),
        scaled(EDGE, dpi),
        scaled(MARGIN, dpi),
    );
    let mut left = at.x + reach;
    let mut top = at.y - reach - size.height;
    if left + size.width > monitor.right - edge {
        left = at.x - reach - size.width;
    }
    if top < monitor.top + edge {
        top = at.y + below;
    }
    // Not `clamp`, which panics when the monitor is smaller than the panel.
    let left = left
        .min(monitor.right - margin - size.width)
        .max(monitor.left + margin);
    let top = top
        .min(monitor.bottom - margin - size.height)
        .max(monitor.top + margin);
    RECT {
        left,
        top,
        right: left + size.width,
        bottom: top + size.height,
    }
}

impl Overlay {
    /// Draws a notice that says `text` after `sign`, for the cursor having been at `at`: the shadow, the panel with
    /// its border and the highlight inside its top, the sign and the text.
    pub(super) fn show_notice(
        &self,
        target: &ID2D1RenderTarget,
        at: Point,
        text: &str,
        sign: Sign,
    ) -> windows::core::Result<()> {
        let typeset = self.typeset(text, &self.format)?;
        let panel = place(at, size(typeset.width, self.dpi), self.monitor, self.dpi);
        let ring_width = scaled(geometry::RING, self.dpi);
        let radius = scaled(RADIUS, self.dpi);
        let outer = self.shaped(&panel, radius, 0, 0);

        // The shadow is cut out where the panel is: the panel is a little see-through, and CSS never paints an outer
        // shadow inside the box it belongs to.
        let hole = self.hollow(&outer)?;
        let drop = scaled(SHADOW_DROP, self.dpi);
        // SAFETY: drawing calls on a live render target with arguments that outlive them. Out of BeginDraw and EndDraw,
        // Direct2D only fails the frame at EndDraw.
        unsafe { target.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE) };
        let layers = (0u32..).step_by(SHADOW_STEP);
        for (step, &opacity) in layers.zip(&self.surfaces.shadow_opacities) {
            // The layers start smaller than the panel by the spread and grow from there; their corners grow with them.
            let grow = scaled(step, self.dpi) - scaled(SHADOW_SPREAD, self.dpi);
            // SAFETY: as above.
            unsafe { self.surfaces.shadow.SetOpacity(opacity) };
            let layer = self.shaped(&panel, radius, grow, drop);
            self.fill_except(target, &layer, &hole, &self.surfaces.shadow)?;
        }

        // The panel, its border over it, and the highlight along the inside of the border's top: the padding box less
        // itself moved down a pixel.
        let inside = self.shaped(&panel, radius, -ring_width, 0);
        let lower = self.shaped(&panel, radius, -ring_width, scaled(1, self.dpi));
        let lower = self.hollow(&lower)?;
        // SAFETY: as above.
        unsafe {
            target.FillRoundedRectangle(&outer, &self.surfaces.panel);
            target.DrawRoundedRectangle(
                &ring(&outer, px(ring_width)),
                &self.surfaces.border,
                px(ring_width),
                None::<&ID2D1StrokeStyle>,
            );
        }
        self.fill_except(target, &inside, &lower, &self.surfaces.highlight)?;

        let window = self.in_window(&panel);
        let content_left = window.left + px(ring_width + scaled(PAD_LEFT, self.dpi));
        let content_top = window.top + px(ring_width + scaled(PAD_TOP, self.dpi));
        self.draw_sign(target, sign, content_left, content_top)?;

        // The text sits in its line the way CSS does it: the line's extra room, above and below the text's own height,
        // is shared equally. Its baseline is on a whole pixel, so the letters are crisp; "Copied" has a letter that
        // drops below the line, so the middle of its digits is no guide here, as it is for a measurement's label.
        let line = px(scaled(LINE, self.dpi));
        let baseline =
            (content_top + (line - typeset.line_height) / 2.0 + typeset.baseline).round();
        // DrawTextLayout takes a Vector2, which windows 0.62 takes from the windows-numerics crate without
        // re-exporting it, and this program may not add the crate. Direct2D's ellipse holds one.
        let mut origin = D2D1_ELLIPSE::default().point;
        origin.X = content_left + px(scaled(SIGN, self.dpi) + scaled(GAP, self.dpi));
        origin.Y = baseline - typeset.baseline;
        // SAFETY: as above.
        unsafe {
            // Grayscale, not ClearType: color fringes would show on the see-through panel.
            target.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
            // The brush keeps the opacity of the last label's logical part.
            self.surfaces.ink.SetOpacity(1.0);
            target.DrawTextLayout(
                origin,
                &typeset.layout,
                &self.surfaces.ink,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
            );
        }
        Ok(())
    }

    /// The sign with its top left corner at (`left`, `top`) in the window: the check mark in the accent's color, or the
    /// exclamation mark in a circle in the warning's. Both are strokes with round caps and joins, as in the prototype,
    /// in a box of 24 units that is `SIGN` logical pixels wide.
    fn draw_sign(
        &self,
        target: &ID2D1RenderTarget,
        sign: Sign,
        left: f32,
        top: f32,
    ) -> windows::core::Result<()> {
        let unit = px(scaled(SIGN, self.dpi)) / SIGN_UNITS;
        // A Vector2 in the sign's units, from the same ellipse trick as for the text's origin.
        let at = |x: f32, y: f32| {
            let mut point = D2D1_ELLIPSE::default().point;
            point.X = left + x * unit;
            point.Y = top + y * unit;
            point
        };
        // SAFETY: a drawing call on a live render target. Out of BeginDraw and EndDraw, Direct2D only fails the frame
        // at EndDraw.
        unsafe { target.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE) };
        match sign {
            Sign::Ok => {
                // SAFETY: no arguments.
                let path = unsafe { self.factory.CreatePathGeometry() }?;
                // SAFETY: no arguments.
                let sink = unsafe { path.Open() }?;
                // SAFETY: the points are plain values, and the figure is begun, extended and ended in order on the
                // sink this function opened and closes below.
                unsafe {
                    sink.BeginFigure(at(5.0, 12.5), D2D1_FIGURE_BEGIN_HOLLOW);
                    sink.AddLine(at(9.5, 17.0));
                    sink.AddLine(at(19.0, 7.5));
                    sink.EndFigure(D2D1_FIGURE_END_OPEN);
                }
                // SAFETY: closes the sink opened above, once.
                unsafe { sink.Close() }?;
                // SAFETY: a drawing call on a live render target with arguments that outlive it; as above.
                unsafe {
                    target.DrawGeometry(&path, &self.surfaces.accent, 3.0 * unit, &self.stroke);
                }
            }
            Sign::Alert => {
                let width = 2.4 * unit;
                let circle = D2D1_ELLIPSE {
                    point: at(12.0, 12.0),
                    radiusX: 9.5 * unit,
                    radiusY: 9.5 * unit,
                };
                // SAFETY: drawing calls on a live render target with arguments that outlive them; as above.
                unsafe {
                    target.DrawEllipse(&circle, &self.surfaces.warn, width, &self.stroke);
                    target.DrawLine(
                        at(12.0, 7.0),
                        at(12.0, 13.0),
                        &self.surfaces.warn,
                        width,
                        &self.stroke,
                    );
                    // The dot is a line a hundredth of a unit long: its round caps make it a dot as wide as the stroke.
                    target.DrawLine(
                        at(12.0, 17.0),
                        at(12.01, 17.0),
                        &self.surfaces.warn,
                        width,
                        &self.stroke,
                    );
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point(x: i32, y: i32) -> Point {
        Point { x, y }
    }

    /// A second monitor to the right of the main one and below it, so nothing assumes the origin.
    const MONITOR: RECT = RECT {
        left: 1920,
        top: 100,
        right: 3840,
        bottom: 1180,
    };

    const PANEL: Size = Size {
        width: 80,
        height: 31,
    };

    fn rect(left: i32, top: i32, right: i32, bottom: i32) -> RECT {
        RECT {
            left,
            top,
            right,
            bottom,
        }
    }

    #[test]
    fn the_panel_is_the_border_the_padding_the_sign_a_gap_and_the_text() {
        // 1 + 9 + 13 + 7 + 40 + 11 + 1 wide, and 1 + 7 + 15 + 7 + 1 tall.
        assert_eq!(
            size(40, 96),
            Size {
                width: 82,
                height: 31
            }
        );
        // Each size is scaled on its own: at 125%, 1 + 11 + 16 + 9 + 40 + 14 + 1 and 1 + 9 + 19 + 9 + 1.
        assert_eq!(
            size(40, 120),
            Size {
                width: 92,
                height: 39
            }
        );
        // At 175%: 2 + 16 + 23 + 12 + 40 + 19 + 2 and 2 + 12 + 26 + 12 + 2.
        assert_eq!(
            size(40, 168),
            Size {
                width: 114,
                height: 54
            }
        );
    }

    #[test]
    fn the_notice_is_above_and_to_the_right_of_where_the_cursor_was() {
        let at = point(2500, 600);
        // 14 right, and 14 above the bottom side.
        assert_eq!(
            place(at, PANEL, MONITOR, 96),
            rect(2514, 600 - 14 - 31, 2514 + 80, 600 - 14)
        );
        // The distances scale: 14 is 18 at 125% and 25 at 175%, and the panel is another size.
        let panel = size(40, 120);
        assert_eq!(
            place(at, panel, MONITOR, 120),
            rect(
                2500 + 18,
                600 - 18 - panel.height,
                2500 + 18 + panel.width,
                600 - 18
            )
        );
        let panel = size(40, 168);
        let placed = place(at, panel, MONITOR, 168);
        assert_eq!(
            (placed.left, placed.bottom),
            (2500 + 25, 600 - 25),
            "at 175%"
        );
    }

    #[test]
    fn the_notice_goes_to_the_left_where_there_is_no_room_on_the_right() {
        // The panel would end at the monitor's right side less 4 or further.
        let room = MONITOR.right - 4 - PANEL.width - 14;
        let fits = place(point(room, 600), PANEL, MONITOR, 96);
        assert_eq!(fits.left, room + 14);
        assert_eq!(fits.right, MONITOR.right - 4);
        // One pixel more, and it goes to the other side of the cursor, its right side 14 from it.
        let flips = place(point(room + 1, 600), PANEL, MONITOR, 96);
        assert_eq!(flips.right, room + 1 - 14);
        assert_eq!(flips.left, room + 1 - 14 - PANEL.width);
        // The scaled limit: 4 is 7 at 175%, and 14 is 25.
        let panel = size(40, 168);
        let room = MONITOR.right - 7 - panel.width - 25;
        assert_eq!(place(point(room, 600), panel, MONITOR, 168).left, room + 25);
        assert_eq!(
            place(point(room + 1, 600), panel, MONITOR, 168).right,
            room + 1 - 25
        );
    }

    #[test]
    fn the_notice_goes_below_where_there_is_no_room_above() {
        // Above, it would start at the monitor's top plus 4 or lower.
        let room = MONITOR.top + 4 + PANEL.height + 14;
        let fits = place(point(2500, room), PANEL, MONITOR, 96);
        assert_eq!(fits.top, MONITOR.top + 4);
        // One pixel less, and it goes 18 below the cursor.
        let below = place(point(2500, room - 1), PANEL, MONITOR, 96);
        assert_eq!(below.top, room - 1 + 18);
        assert_eq!(below.bottom, room - 1 + 18 + PANEL.height);
        // The scaled distance: 18 is 32 at 175%.
        let panel = size(40, 168);
        let at = point(2500, MONITOR.top);
        assert_eq!(
            place(at, panel, MONITOR, 168).top,
            MONITOR.top + scaled(18, 168)
        );
    }

    #[test]
    fn the_corner_flips_both_ways_at_once() {
        // The cursor in the top right corner: left of it and below it.
        let at = point(MONITOR.right - 1, MONITOR.top);
        let placed = place(at, PANEL, MONITOR, 96);
        assert_eq!(placed.right, MONITOR.right - 1 - 14);
        assert_eq!(placed.top, MONITOR.top + 18);
    }

    #[test]
    fn the_notice_stays_2_pixels_inside_the_monitor_at_every_scale() {
        for dpi in [96, 120, 144, 168] {
            let panel = size(40, dpi);
            let margin = scaled(MARGIN, dpi);
            // The four corners and the middle, where the cursor may be, and points past each side, which the clamp is
            // for.
            for at in [
                point(MONITOR.left, MONITOR.top),
                point(MONITOR.right - 1, MONITOR.top),
                point(MONITOR.left, MONITOR.bottom - 1),
                point(MONITOR.right - 1, MONITOR.bottom - 1),
                point(2500, 600),
                point(MONITOR.right + 100, 600),
                point(MONITOR.left - 300, 600),
                point(2500, MONITOR.top - 50),
                point(2500, MONITOR.bottom + 200),
            ] {
                let placed = place(at, panel, MONITOR, dpi);
                assert!(
                    placed.left >= MONITOR.left + margin
                        && placed.top >= MONITOR.top + margin
                        && placed.right <= MONITOR.right - margin
                        && placed.bottom <= MONITOR.bottom - margin,
                    "{placed:?} for {at:?} at {dpi} DPI"
                );
                assert_eq!(placed.right - placed.left, panel.width);
                assert_eq!(placed.bottom - placed.top, panel.height);
            }
        }
        // Where the clamp moves it, it stops exactly 2 from the side.
        let past_right = place(point(MONITOR.right + 100, 600), PANEL, MONITOR, 96);
        assert_eq!(past_right.right, MONITOR.right - 2);
        let past_left = place(point(MONITOR.left - 300, 600), PANEL, MONITOR, 96);
        assert_eq!(past_left.left, MONITOR.left + 2);
        let past_bottom = place(point(2500, MONITOR.bottom + 200), PANEL, MONITOR, 96);
        assert_eq!(past_bottom.bottom, MONITOR.bottom - 2);
    }

    #[test]
    fn a_monitor_smaller_than_the_panel_gets_the_panel_at_its_top_left() {
        // 50 by 20 pixels: no room for a panel of 80 by 31. This must not panic, and the panel starts inside the
        // monitor's corner rather than past it.
        let tiny = rect(100, 100, 150, 120);
        let placed = place(point(120, 110), PANEL, tiny, 96);
        assert_eq!((placed.left, placed.top), (tiny.left + 2, tiny.top + 2));
    }
}
