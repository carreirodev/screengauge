//! The toolbar, kept as a bitmap between frames (WPC-310). The frozen screen's window repaints on every mouse move that
//! changes a measurement, and the toolbar underneath it only changes when another tool or mode is lit, so it is drawn
//! once, with all its shadows, on a bitmap of its own, and each frame lays that bitmap over the photo one pixel for one
//! pixel. Drawing it afresh each time would tessellate a hundred or so shadow layers for nothing.
//!
//! The bitmap holds the toolbar at the origin with room all round for the reach of its shadows, whichever buttons are lit,
//! so it is the same size for every state, never moves with the toolbar's place (dragging it, WPC-312, only moves where it
//! is laid), and a state that changes is drawn over the same bitmap. The part of it past the monitor's edge, which the
//! toolbar's margin leaves at the top, is cut off by the window as the bitmap is laid.
//!
//! Compositing the layers on a transparent bitmap and then that bitmap over the photo is not the same as compositing every
//! layer straight over the photo: a faint layer drawn over the photo's 8-bit pixels rounds away, as it changes a channel
//! by less than half a level, so a shadow comes out too light in the small channels and the glow off its hue. On the
//! bitmap the layers add up in the alpha channel first and keep their proportions, so the bitmap is the more faithful.
//! The two differ by up to 27 levels at a pixel, and by 2 to 4 on average over the toolbar (`toolbar_tests` measures
//! it). A toolbar is always drawn through the bitmap, so what is drawn does not depend on the cache's state.

use screengauge_core::session::{Point, Toolbar};
use windows::Win32::Foundation::E_FAIL;
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_RECT_F, D2D_SIZE_F, D2D_SIZE_U, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_BITMAP_INTERPOLATION_MODE_NEAREST_NEIGHBOR, D2D1_COMPATIBLE_RENDER_TARGET_OPTIONS_NONE,
    ID2D1Bitmap, ID2D1BitmapRenderTarget, ID2D1RenderTarget,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::core::Error;

use super::Overlay;
use super::palette::d2d_color;
use super::toolbar::Layout;
use super::toolbar_paint::{self, Margins};
use super::toolbar_palette;
use crate::log;

/// The bitmap and what it is of.
struct Kept {
    /// What the toolbar was drawn on, kept to draw the next state on it.
    target: ID2D1BitmapRenderTarget,
    bitmap: ID2D1Bitmap,
    /// What the bitmap shows: the lit tool, and the lit mode while there is one. The groups and the buttons are always
    /// the same, and so are the monitor, the scale and the palette, which an `Overlay` is made for; the toolbar's own
    /// monitor never changes for a window, so the whole scene's toolbar is the key.
    shows: Toolbar,
    /// How much of the bitmap is left, above and to the left of the toolbar, for the shadows.
    margins: Margins,
    width: i32,
    height: i32,
}

/// What an `Overlay` keeps of the toolbar.
#[derive(Default)]
pub(super) struct ToolbarCache {
    kept: Option<Kept>,
    /// How many times the toolbar was drawn on the bitmap, for the tests to see the bitmap is used.
    #[cfg(test)]
    pub(super) fills: u32,
}

impl Overlay {
    /// Lays the toolbar's bitmap over what `target` holds, between BeginDraw and EndDraw, drawing it first when the
    /// toolbar shows something else than the bitmap does.
    pub(super) fn show_toolbar(
        &self,
        target: &ID2D1RenderTarget,
        layout: &Layout,
        toolbar: &Toolbar,
    ) -> windows::core::Result<()> {
        let mut cache = self.toolbar_cache.try_borrow_mut().map_err(|_| {
            Error::new(
                E_FAIL,
                "the toolbar was drawn again while it was being drawn",
            )
        })?;
        let kept = match cache.kept.take() {
            Some(kept) if kept.shows == *toolbar => kept,
            old => {
                #[cfg(test)]
                {
                    cache.fills += 1;
                }
                self.draw_toolbar_bitmap(target, layout, toolbar, old)?
            }
        };
        // The bitmap's corner, in the window's own pixels, and the bitmap's size: laid one pixel for one pixel.
        let left = layout.toolbar.left - kept.margins.left - self.monitor.left;
        let top = layout.toolbar.top - kept.margins.top - self.monitor.top;
        let place = D2D_RECT_F {
            left: super::px(left),
            top: super::px(top),
            right: super::px(left + kept.width),
            bottom: super::px(top + kept.height),
        };
        // SAFETY: a drawing call on a live render target with arguments that outlive it. Out of BeginDraw and EndDraw,
        // Direct2D only fails the frame at EndDraw. The bitmap is at 96 DPI, like the target, and its place is on whole
        // pixels, so nearest neighbor copies each of its pixels to one of the target's.
        unsafe {
            target.DrawBitmap(
                &kept.bitmap,
                Some(&place),
                1.0,
                D2D1_BITMAP_INTERPOLATION_MODE_NEAREST_NEIGHBOR,
                None,
            );
        }
        cache.kept = Some(kept);
        Ok(())
    }

    /// The toolbar drawn on a bitmap of `target`'s kind: a new one, or `old`'s, cleared. A bitmap that can't be made or
    /// drawn on is an error, and the next frame tries again.
    fn draw_toolbar_bitmap(
        &self,
        target: &ID2D1RenderTarget,
        layout: &Layout,
        toolbar: &Toolbar,
        old: Option<Kept>,
    ) -> windows::core::Result<Kept> {
        let (bitmap_target, margins, width, height) = match old {
            Some(old) => (old.target, old.margins, old.width, old.height),
            None => {
                let palette = toolbar_palette::palette(self.mode);
                let margins = toolbar_paint::reach(palette, layout, self.dpi);
                let width =
                    layout.toolbar.right - layout.toolbar.left + margins.left + margins.right;
                let height =
                    layout.toolbar.bottom - layout.toolbar.top + margins.top + margins.bottom;
                (
                    self.new_bitmap_target(target, width, height)?,
                    margins,
                    width,
                    height,
                )
            }
        };
        // The bitmap's pixel (0, 0) is the virtual screen's point `margins` up and left of the toolbar's corner.
        let origin = Point {
            x: layout.toolbar.left - margins.left,
            y: layout.toolbar.top - margins.top,
        };
        // Premultiplied transparent: nothing of the bitmap's earlier contents, and nothing under the shadows.
        let clear = d2d_color([0, 0, 0], 0.0);
        // SAFETY: the bitmap target is ours, and ended below whatever happens in between.
        unsafe {
            bitmap_target.BeginDraw();
            bitmap_target.Clear(Some(&clear));
        }
        let painted = self.paint_toolbar(&bitmap_target, layout, toolbar, origin);
        // SAFETY: ends the BeginDraw above.
        let ended = unsafe { bitmap_target.EndDraw(None, None) };
        match (painted, ended) {
            (Ok(()), Ok(())) => {}
            (Err(e), Ok(())) | (Ok(()), Err(e)) => return Err(e),
            // Both failed: the painting's is the cause, and the other is logged, not lost.
            (Err(e), Err(also)) => {
                log::failure("frozen screen: ending the toolbar's bitmap", &also);
                return Err(e);
            }
        }
        // SAFETY: no arguments; the bitmap comes back as an owned interface.
        let bitmap = unsafe { bitmap_target.GetBitmap() }?;
        Ok(Kept {
            target: bitmap_target,
            bitmap,
            shows: *toolbar,
            margins,
            width,
            height,
        })
    }

    /// A render target on a bitmap of `width` by `height` pixels, compatible with `target`, with room for transparency:
    /// the window's own pixels are opaque, and the shadows are not.
    fn new_bitmap_target(
        &self,
        target: &ID2D1RenderTarget,
        width: i32,
        height: i32,
    ) -> windows::core::Result<ID2D1BitmapRenderTarget> {
        let too_large = || Error::new(E_FAIL, "the toolbar's bitmap is too large");
        let pixels = D2D_SIZE_U {
            width: u32::try_from(width).map_err(|_| too_large())?,
            height: u32::try_from(height).map_err(|_| too_large())?,
        };
        // Both are the same number because the target is at 96 DPI, where a unit is a pixel.
        let units = D2D_SIZE_F {
            width: super::px(width),
            height: super::px(height),
        };
        let format = D2D1_PIXEL_FORMAT {
            format: DXGI_FORMAT_B8G8R8A8_UNORM,
            alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
        };
        // SAFETY: the sizes and the format outlive the call, which keeps nothing of them.
        unsafe {
            target.CreateCompatibleRenderTarget(
                Some(&units),
                Some(&pixels),
                Some(&format),
                D2D1_COMPATIBLE_RENDER_TARGET_OPTIONS_NONE,
            )
        }
    }
}
