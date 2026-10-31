//! What the notice and the tolerance indicator have in common: a small panel that floats over the frozen screen, kept
//! inside its monitor, with a shadow, a border and a highlight along its top (the prototype's `.panel`, WPC-279).

use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Direct2D::{D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, ID2D1RenderTarget};

use super::Overlay;
use super::geometry::{self, Size, scaled};

// Every size below is in logical pixels, which `scaled` turns into physical pixels for the monitor's scale.

/// How near the monitor's sides a floating panel may come at all.
pub const MARGIN: u32 = 2;

/// The shadow, CSS `0 10px 28px -8px`: it falls this far and is this much smaller than the panel on every side before
/// it blurs. The blur is `SHADOW_LAYERS` rounded rectangles, each this much bigger than the one before it.
const SHADOW_DROP: u32 = 10;
const SHADOW_SPREAD: u32 = 8;
const SHADOW_STEP: usize = 4;

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
    /// the shadow, the panel with its border and the highlight inside its top. `shadow_strength` multiplies the opacity
    /// of every layer of the shadow: the layers are made for the notice's height, and a lower panel casts a fainter one.
    pub(super) fn draw_floating_panel(
        &self,
        target: &ID2D1RenderTarget,
        panel: &RECT,
        radius: u32,
        shadow_strength: f32,
    ) -> windows::core::Result<()> {
        let ring_width = scaled(geometry::RING, self.dpi);
        let radius = scaled(radius, self.dpi);
        let outer = self.shaped(panel, radius, 0, 0);

        // The shadow is cut out where the panel is: the panel is a little see-through, and CSS never paints an outer
        // shadow inside the box it belongs to.
        let hole = self.geometry_of(&outer)?;
        let drop = scaled(SHADOW_DROP, self.dpi);
        // SAFETY: drawing calls on a live render target with arguments that outlive them. Out of BeginDraw and EndDraw,
        // Direct2D only fails the frame at EndDraw.
        unsafe { target.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE) };
        let layers = (0u32..).step_by(SHADOW_STEP);
        for (step, &opacity) in layers.zip(&self.surfaces.shadow_opacities) {
            // The layers start smaller than the panel by the spread and grow from there; their corners grow with them.
            let grow = scaled(step, self.dpi) - scaled(SHADOW_SPREAD, self.dpi);
            // SAFETY: as above.
            unsafe { self.surfaces.shadow.SetOpacity(opacity * shadow_strength) };
            let layer = self.shaped(panel, radius, grow, drop);
            self.fill_except(target, &layer, &hole, &self.surfaces.shadow)?;
        }

        // The panel, its border over it, and the highlight along the inside of the border's top: the padding box less
        // itself moved down a pixel.
        let inside = self.shaped(panel, radius, -ring_width, 0);
        let lower = self.shaped(panel, radius, -ring_width, scaled(1, self.dpi));
        let lower = self.geometry_of(&lower)?;
        self.fill_panel(target, &outer, ring_width);
        self.fill_except(target, &inside, &lower, &self.surfaces.highlight)
    }
}
