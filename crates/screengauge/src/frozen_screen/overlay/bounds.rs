//! Bounds drawn over a window's photo (v1 spec WPC-296, Como as medidas aparecem; the approved prototype's variant A,
//! WPC-279): the rectangle the user is drawing, dashed in white over black along its own outer pixels, and the content
//! it shrinks to, filled with a light cyan, ringed in cyan just outside it and with the translucent dark outline beyond,
//! like a Dimensions line. The label sits above the ring's top left corner. Where each of them goes is worked out here
//! in whole pixels of the virtual screen, with no Windows call, and drawn with aliased fills, so each is crisp.

use screengauge_core::session::{DrawnRectangle, Rect};
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Direct2D::{
    D2D1_ANTIALIAS_MODE_ALIASED, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, ID2D1RenderTarget,
};

use super::geometry::{self, Parts, Size, scaled};
use super::{Look, Overlay};

// Every size below is in logical pixels, which `scaled` turns into physical pixels for the monitor's scale.

/// How long each dash along the drawn rectangle's sides is, and each gap of black between two (the prototype's
/// `setLineDash([4, 4])`).
const DASH: u32 = 4;
/// The gap between the label's bottom and the ring's top row.
const LABEL_GAP: u32 = 6;
/// Where the label goes when the monitor has no room above the ring: this far right of the ring's top left corner and
/// this far down, inside the ring, over the content.
const INSIDE_RIGHT: u32 = 4;
const INSIDE_DOWN: u32 = 5;

/// How much of the black under the dashes shows over the photo (`rgba(0,0,0,.85)`).
const DASH_BLACK: f32 = 0.85;
/// How much of the cyan over the content shows: more while it is live than once it is pinned (`.16` and `.1`).
const LIVE_FILL: f32 = 0.16;
const PINNED_FILL: f32 = 0.1;

/// The outer pixels of the rectangle from (`left`, `top`) to (`right`, `bottom`), both included, as rectangles that
/// don't overlap, so none is darkened twice: the top row whole, then the bottom row and the columns between them. A
/// rectangle one pixel tall or wide is its row or its column alone.
fn outer_pixels(left: i32, top: i32, right: i32, bottom: i32) -> Vec<RECT> {
    let row = |y: i32| RECT {
        left,
        top: y,
        right: right + 1,
        bottom: y + 1,
    };
    let column = |x: i32| RECT {
        left: x,
        top: top + 1,
        right: x + 1,
        bottom,
    };
    if bottom == top {
        return vec![row(top)];
    }
    if right == left {
        return vec![RECT {
            left,
            top,
            right: left + 1,
            bottom: bottom + 1,
        }];
    }
    let mut pixels = vec![row(top), row(bottom)];
    if bottom - top > 1 {
        pixels.extend([column(left), column(right)]);
    }
    pixels
}

/// What to fill around Bounds' content, as Dimensions' line has its [`geometry::parts`]: a cyan ring one pixel wide just
/// outside the content, whatever the scale, and the outline as the ring outside that.
pub fn parts(content: Rect) -> Parts {
    Parts {
        outline: ring(content, 2),
        line: ring(content, 1),
    }
}

/// The ring of pixels `by` pixels outside `content` all around it, one pixel wide.
fn ring(content: Rect, by: i32) -> Vec<RECT> {
    outer_pixels(
        content.left - by,
        content.top - by,
        content.right() + by,
        content.bottom() + by,
    )
}

/// The white dashes along the outer pixels of `drawn`, for a monitor of `dpi`. They follow the rectangle as the
/// prototype's canvas strokes it: clockwise from the top left pixel, along the top row, down the right column, back
/// along the bottom row and up the left column, a dash and then a gap of the same length, white first, without starting
/// again at the corners.
pub fn dashes(drawn: Rect, dpi: u32) -> Vec<RECT> {
    let dash = scaled(DASH, dpi).max(1);
    let (left, top, right, bottom) = (drawn.left, drawn.top, drawn.right(), drawn.bottom());
    // How many pixels along the rows and the columns lie past the first, which is how far the walk goes along each.
    let (across, down) = (right - left, bottom - top);
    let mut white = Vec::new();
    // One side: `count` pixels whose first is `start` pixels along the walk; `run` gives the rectangle of its pixels
    // from the `from`th to before the `to`th.
    let mut side = |start: i32, count: i32, run: &dyn Fn(i32, i32) -> RECT| {
        let mut from = 0;
        while from < count {
            let lap = (start + from) / dash;
            let to = ((lap + 1) * dash - start).min(count);
            if lap % 2 == 0 {
                white.push(run(from, to));
            }
            from = to;
        }
    };
    side(0, across + 1, &|from, to| RECT {
        left: left + from,
        top,
        right: left + to,
        bottom: top + 1,
    });
    if down > 0 {
        side(across + 1, down, &|from, to| RECT {
            left: right,
            top: top + 1 + from,
            right: right + 1,
            bottom: top + 1 + to,
        });
    }
    if down > 0 && across > 0 {
        side(across + down + 1, across, &|from, to| RECT {
            left: right - to,
            top: bottom,
            right: right - from,
            bottom: bottom + 1,
        });
    }
    if down > 1 && across > 0 {
        side(2 * across + down + 1, down - 1, &|from, to| RECT {
            left,
            top: bottom - to,
            right: left + 1,
            bottom: bottom - from,
        });
    }
    white
}

/// Where the label of a Bounds measurement whose content is `content` goes: its rectangle in the virtual screen, right
/// and bottom one past its last pixel. Its left side is on the ring's left column and its bottom `LABEL_GAP` above the
/// ring's top row; where the monitor has no room above, it goes inside the ring at its top left corner, over the
/// content. Where it would still stick out of `monitor`, it slides inside.
pub fn place_label(content: Rect, label: Size, monitor: RECT, dpi: u32) -> RECT {
    let (corner_x, corner_y) = (content.left - 1, content.top - 1);
    let above = corner_y - scaled(LABEL_GAP, dpi) - label.height;
    let (left, top) = if above >= monitor.top {
        (corner_x, above)
    } else {
        (
            corner_x + scaled(INSIDE_RIGHT, dpi),
            corner_y + scaled(INSIDE_DOWN, dpi),
        )
    };
    let left = geometry::inside(left, label.width, monitor.left, monitor.right);
    let top = geometry::inside(top, label.height, monitor.top, monitor.bottom);
    RECT {
        left,
        top,
        right: left + label.width,
        bottom: top + label.height,
    }
}

impl Overlay {
    /// Draws the outer pixels of the rectangle Bounds is drawing: black, a little see-through, and white dashes over it.
    pub(super) fn draw_drawn_rectangle(&self, target: &ID2D1RenderTarget, drawn: &DrawnRectangle) {
        let rect = drawn.rect;
        // SAFETY: drawing calls on a live render target with arguments that outlive them. Out of BeginDraw and EndDraw,
        // Direct2D only fails the frame at EndDraw. Aliased fills of rectangles on whole pixels cover exactly those
        // pixels, and the target is at 96 DPI, so one unit is one physical pixel.
        unsafe {
            target.SetAntialiasMode(D2D1_ANTIALIAS_MODE_ALIASED);
            self.shade.SetOpacity(DASH_BLACK);
            for pixels in outer_pixels(rect.left, rect.top, rect.right(), rect.bottom()) {
                target.FillRectangle(&self.in_window(&pixels), &self.shade);
            }
            for dash in dashes(rect, self.dpi) {
                target.FillRectangle(&self.in_window(&dash), &self.white);
            }
            target.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
        }
    }

    /// Fills each content with see-through cyan, lighter for a pinned one than for the live one. Should a content be too
    /// large to draw, none is.
    pub(super) fn draw_fills(
        &self,
        target: &ID2D1RenderTarget,
        fills: &[(Rect, Look)],
    ) -> windows::core::Result<()> {
        let fills = fills
            .iter()
            .map(|&(content, look)| Ok((geometry::bounds(content)?, look)))
            .collect::<windows::core::Result<Vec<_>>>()?;
        // SAFETY: as in `draw_drawn_rectangle`. The cyan brush is opaque again afterwards, as the lines and the live
        // pill take it.
        unsafe {
            target.SetAntialiasMode(D2D1_ANTIALIAS_MODE_ALIASED);
            for (content, look) in &fills {
                self.cyan.SetOpacity(match look {
                    Look::Live => LIVE_FILL,
                    Look::Pinned => PINNED_FILL,
                });
                target.FillRectangle(&self.in_window(content), &self.cyan);
            }
            self.cyan.SetOpacity(1.0);
            target.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
        }
        Ok(())
    }
}
