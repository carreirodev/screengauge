//! What the notice and the tolerance indicator have in common: a small panel that floats over the frozen screen, kept
//! inside its monitor, with a shadow, a border and a highlight along its top (the prototype's `.panel`, WPC-279). The
//! shadow is `relief`'s, the toolbar's and Settings' (WPC-393).

use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Direct2D::{D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, ID2D1RenderTarget};

use super::Overlay;
use super::geometry::{self, Size, scaled};

// Every size below is in logical pixels, which `scaled` turns into physical pixels for the monitor's scale.

/// How near the monitor's sides a floating panel may come at all.
pub const MARGIN: u32 = 2;

/// The panel `size` with its top left corner at (`left`, `top`), slid inside the monitor so its sides are `MARGIN` from
/// the monitor's. Should the monitor be smaller than the panel, it starts `MARGIN` inside the monitor's left and top
/// sides. The panel's rectangle is in the virtual screen, its right and bottom one past its last pixel.
pub fn inside_monitor(left: i32, top: i32, size: Size, monitor: RECT, dpi: u32) -> RECT {
    let margin = scaled(MARGIN, dpi);
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
    /// Draws a floating panel in `panel`, a rectangle in the virtual screen, with corners of `radius` logical pixels:
    /// the shadow, the panel with its border and the highlight inside its top. The shadow is blurred for the panel's own
    /// size, so a lower panel casts a fainter one, as in a browser.
    pub(super) fn draw_floating_panel(
        &self,
        target: &ID2D1RenderTarget,
        panel: &RECT,
        radius: u32,
    ) -> windows::core::Result<()> {
        let ring_width = scaled(geometry::RING, self.dpi());
        let radius = scaled(radius, self.dpi());
        let outer = self.shaped(panel, radius, 0, 0);

        // SAFETY: a drawing call on a live render target. Out of BeginDraw and EndDraw, Direct2D only fails the frame at
        // EndDraw.
        unsafe { target.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE) };
        // The shadow is cut out where the panel is: the panel is a little see-through, and CSS never paints an outer
        // shadow inside the box it belongs to.
        self.canvas(target)?
            .shadow_over(&self.surfaces.shadow, panel, radius, true)?;

        // The panel, its border over it, and the highlight along the inside of the border's top: the padding box less
        // itself moved down a pixel.
        let inside = self.shaped(panel, radius, -ring_width, 0);
        let lower = self.shaped(panel, radius, -ring_width, scaled(1, self.dpi()));
        let lower = self.geometry_of(&lower)?;
        self.fill_panel(target, &outer, ring_width);
        self.fill_except(target, &inside, &lower, &self.surfaces.highlight)
    }
}
