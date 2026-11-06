//! The notice: a short message on a panel with a sign before its text, drawn above and to the right of where the cursor
//! was when it appeared (the prototype's notice, WPC-279), or below it where the magnified view is above it (WPC-367).
//! Where it goes needs no Windows, so [`size`] and [`place`] are tested without a monitor; the rest draws it onto the
//! overlay's render target.

use screengauge_core::session::{NoticeSide, Point, Sign};
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Direct2D::{
    D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE, ID2D1RenderTarget,
};

use super::floating;
use super::geometry::{self, Size, scaled};
use super::{Overlay, dot, px};

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
/// How near the monitor's right and top sides the notice may come before it moves to the other side of the cursor.
const EDGE: u32 = 4;

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

/// Where the panel goes: its rectangle in the virtual screen, right and bottom one past its last pixel. On `side`
/// [`NoticeSide::Above`] it sits `REACH` right of `at` and `REACH` above it, and where it would come within `EDGE` of
/// the monitor's top it goes `BELOW` below `at`; on [`NoticeSide::Below`] it sits `BELOW` below `at` from the start.
/// Where it would come within `EDGE` of the monitor's right side it goes to the left of `at` instead. Where it would
/// still stick out, it slides inside, `floating::MARGIN` from the sides; should the monitor be smaller than the panel,
/// it starts that far inside the monitor's left and top sides.
pub fn place(at: Point, side: NoticeSide, size: Size, monitor: RECT, dpi: u32) -> RECT {
    let (reach, below, edge) = (scaled(REACH, dpi), scaled(BELOW, dpi), scaled(EDGE, dpi));
    let mut left = at.x + reach;
    if left + size.width > monitor.right - edge {
        left = at.x - reach - size.width;
    }
    let above = at.y - reach - size.height;
    let top = match side {
        NoticeSide::Above if above >= monitor.top + edge => above,
        NoticeSide::Above | NoticeSide::Below => at.y + below,
    };
    floating::inside_monitor(left, top, size, monitor, dpi)
}

impl Overlay {
    /// Draws a notice that says `text` after `sign`, for the cursor having been at `at`, on `side` of it: the
    /// floating panel, the sign and the text.
    pub(super) fn show_notice(
        &self,
        target: &ID2D1RenderTarget,
        at: Point,
        side: NoticeSide,
        text: &str,
        sign: Sign,
    ) -> windows::core::Result<()> {
        let typeset = self.typeset(text, &self.format)?;
        let panel = place(
            at,
            side,
            size(typeset.width, self.dpi()),
            self.monitor,
            self.dpi(),
        );
        self.draw_floating_panel(target, &panel, RADIUS, 1.0)?;

        let ring_width = scaled(geometry::RING, self.dpi());
        let window = self.in_window(&panel);
        let content_left = window.left + px(ring_width + scaled(PAD_LEFT, self.dpi()));
        let content_top = window.top + px(ring_width + scaled(PAD_TOP, self.dpi()));
        self.draw_sign(target, sign, content_left, content_top)?;

        // The text sits in its line the way CSS does it: the line's extra room, above and below the text's own height,
        // is shared equally. Its baseline is on a whole pixel, so the letters are crisp; "Copied" has a letter that
        // drops below the line, so the middle of its digits is no guide here, as it is for a measurement's label.
        let line = px(scaled(LINE, self.dpi()));
        let baseline = typeset.baseline_in(content_top, line);
        let origin = dot(
            content_left + px(scaled(SIGN, self.dpi()) + scaled(GAP, self.dpi())),
            baseline - typeset.baseline,
        )
        .point;
        // SAFETY: drawing calls on a live render target with arguments that outlive them. Out of BeginDraw and EndDraw,
        // Direct2D only fails the frame at EndDraw.
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

    /// The sign with its top left corner at (`left`, `top`) in the window, `SIGN` logical pixels wide: the check mark
    /// in the accent's color, or the exclamation mark in a circle in the warning's.
    pub(super) fn draw_sign(
        &self,
        target: &ID2D1RenderTarget,
        sign: Sign,
        left: f32,
        top: f32,
    ) -> windows::core::Result<()> {
        let brush = match sign {
            Sign::Ok => &self.surfaces.accent,
            Sign::Alert => &self.surfaces.warn,
        };
        let pen = crate::sign::Pen {
            target,
            factory: &self.factory,
            stroke: &self.stroke,
        };
        crate::sign::draw(&pen, brush, sign, left, top, px(scaled(SIGN, self.dpi())))
    }
}

#[cfg(test)]
mod tests {
    use super::floating::MARGIN;
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
            place(at, NoticeSide::Above, PANEL, MONITOR, 96),
            rect(2514, 600 - 14 - 31, 2514 + 80, 600 - 14)
        );
        // The distances scale: 14 is 18 at 125% and 25 at 175%, and the panel is another size.
        let panel = size(40, 120);
        assert_eq!(
            place(at, NoticeSide::Above, panel, MONITOR, 120),
            rect(
                2500 + 18,
                600 - 18 - panel.height,
                2500 + 18 + panel.width,
                600 - 18
            )
        );
        let panel = size(40, 168);
        let placed = place(at, NoticeSide::Above, panel, MONITOR, 168);
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
        let fits = place(point(room, 600), NoticeSide::Above, PANEL, MONITOR, 96);
        assert_eq!(fits.left, room + 14);
        assert_eq!(fits.right, MONITOR.right - 4);
        // One pixel more, and it goes to the other side of the cursor, its right side 14 from it.
        let flips = place(point(room + 1, 600), NoticeSide::Above, PANEL, MONITOR, 96);
        assert_eq!(flips.right, room + 1 - 14);
        assert_eq!(flips.left, room + 1 - 14 - PANEL.width);
        // The scaled limit: 4 is 7 at 175%, and 14 is 25.
        let panel = size(40, 168);
        let room = MONITOR.right - 7 - panel.width - 25;
        assert_eq!(
            place(point(room, 600), NoticeSide::Above, panel, MONITOR, 168).left,
            room + 25
        );
        assert_eq!(
            place(point(room + 1, 600), NoticeSide::Above, panel, MONITOR, 168).right,
            room + 1 - 25
        );
    }

    #[test]
    fn the_notice_goes_below_where_there_is_no_room_above() {
        // Above, it would start at the monitor's top plus 4 or lower.
        let room = MONITOR.top + 4 + PANEL.height + 14;
        let fits = place(point(2500, room), NoticeSide::Above, PANEL, MONITOR, 96);
        assert_eq!(fits.top, MONITOR.top + 4);
        // One pixel less, and it goes 18 below the cursor.
        let below = place(point(2500, room - 1), NoticeSide::Above, PANEL, MONITOR, 96);
        assert_eq!(below.top, room - 1 + 18);
        assert_eq!(below.bottom, room - 1 + 18 + PANEL.height);
        // The scaled distance: 18 is 32 at 175%.
        let panel = size(40, 168);
        let at = point(2500, MONITOR.top);
        assert_eq!(
            place(at, NoticeSide::Above, panel, MONITOR, 168).top,
            MONITOR.top + scaled(18, 168)
        );
    }

    #[test]
    fn the_corner_flips_both_ways_at_once() {
        // The cursor in the top right corner: left of it and below it.
        let at = point(MONITOR.right - 1, MONITOR.top);
        let placed = place(at, NoticeSide::Above, PANEL, MONITOR, 96);
        assert_eq!(placed.right, MONITOR.right - 1 - 14);
        assert_eq!(placed.top, MONITOR.top + 18);
    }

    #[test]
    fn below_the_notice_is_below_and_to_the_right_of_where_the_cursor_was() {
        let at = point(2500, 600);
        // 14 right, and 18 below, as it goes where there is no room above.
        assert_eq!(
            place(at, NoticeSide::Below, PANEL, MONITOR, 96),
            rect(2514, 600 + 18, 2514 + 80, 600 + 18 + 31)
        );
        // The distances scale: 14 is 25 at 175%, and 18 is 32.
        let panel = size(40, 168);
        assert_eq!(
            place(at, NoticeSide::Below, panel, MONITOR, 168),
            rect(
                2500 + 25,
                600 + 32,
                2500 + 25 + panel.width,
                600 + 32 + panel.height
            )
        );
        // Near the top too, where above would have gone below as well.
        let top = point(2500, MONITOR.top);
        assert_eq!(
            place(top, NoticeSide::Below, PANEL, MONITOR, 96),
            place(top, NoticeSide::Above, PANEL, MONITOR, 96)
        );
    }

    #[test]
    fn below_the_notice_still_goes_left_where_there_is_no_room_on_the_right() {
        let room = MONITOR.right - 4 - PANEL.width - 14;
        assert_eq!(
            place(point(room, 600), NoticeSide::Below, PANEL, MONITOR, 96).left,
            room + 14
        );
        let flips = place(point(room + 1, 600), NoticeSide::Below, PANEL, MONITOR, 96);
        assert_eq!(flips.right, room + 1 - 14);
        assert_eq!(flips.top, 600 + 18);
    }

    #[test]
    fn below_the_notice_slides_up_inside_the_monitor_near_its_bottom() {
        // 18 + 31 below the cursor ends 2 from the bottom with the cursor 51 above it, and fits.
        let fits = place(
            point(2500, MONITOR.bottom - 51),
            NoticeSide::Below,
            PANEL,
            MONITOR,
            96,
        );
        assert_eq!(fits.top, MONITOR.bottom - 51 + 18);
        assert_eq!(fits.bottom, MONITOR.bottom - 2);
        // Lower, it stays 2 from the bottom, nearer the cursor.
        for from_bottom in [50, 20, 1] {
            let placed = place(
                point(2500, MONITOR.bottom - from_bottom),
                NoticeSide::Below,
                PANEL,
                MONITOR,
                96,
            );
            assert_eq!(
                placed.bottom,
                MONITOR.bottom - 2,
                "{from_bottom} from the bottom"
            );
        }
    }

    #[test]
    fn the_notice_stays_2_pixels_inside_the_monitor_at_every_scale() {
        for (dpi, side) in [96, 120, 144, 168]
            .into_iter()
            .flat_map(|dpi| [(dpi, NoticeSide::Above), (dpi, NoticeSide::Below)])
        {
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
                let placed = place(at, side, panel, MONITOR, dpi);
                assert!(
                    placed.left >= MONITOR.left + margin
                        && placed.top >= MONITOR.top + margin
                        && placed.right <= MONITOR.right - margin
                        && placed.bottom <= MONITOR.bottom - margin,
                    "{placed:?} for {at:?} {side:?} at {dpi} DPI"
                );
                assert_eq!(placed.right - placed.left, panel.width);
                assert_eq!(placed.bottom - placed.top, panel.height);
            }
        }
        // Where the clamp moves it, it stops exactly 2 from the side.
        let past_right = place(
            point(MONITOR.right + 100, 600),
            NoticeSide::Above,
            PANEL,
            MONITOR,
            96,
        );
        assert_eq!(past_right.right, MONITOR.right - 2);
        let past_left = place(
            point(MONITOR.left - 300, 600),
            NoticeSide::Above,
            PANEL,
            MONITOR,
            96,
        );
        assert_eq!(past_left.left, MONITOR.left + 2);
        let past_bottom = place(
            point(2500, MONITOR.bottom + 200),
            NoticeSide::Above,
            PANEL,
            MONITOR,
            96,
        );
        assert_eq!(past_bottom.bottom, MONITOR.bottom - 2);
    }

    #[test]
    fn a_monitor_smaller_than_the_panel_gets_the_panel_at_its_top_left() {
        // 50 by 20 pixels: no room for a panel of 80 by 31. This must not panic, and the panel starts inside the
        // monitor's corner rather than past it.
        let tiny = rect(100, 100, 150, 120);
        let placed = place(point(120, 110), NoticeSide::Above, PANEL, tiny, 96);
        assert_eq!((placed.left, placed.top), (tiny.left + 2, tiny.top + 2));
    }
}
