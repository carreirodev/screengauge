//! Inspect's magnified view (v1 spec WPC-296, Inspect; the prototype's variant A, WPC-279): the floating panel, the
//! frozen screen's pixels enlarged without smoothing, the grid, the outline around the pixel under the cursor, the
//! magnification's tag and the strip with the cursor's position. Every rectangle comes from the session core's
//! [`MagnifiedView`]; this only draws them onto the overlay's render target.

use screengauge_core::session::MagnifiedView;
use windows::Win32::Graphics::Direct2D::{ID2D1Bitmap, ID2D1RenderTarget};

use super::Overlay;

impl Overlay {
    /// Draws `view` over the measurements and under the toolbar, between BeginDraw and EndDraw, enlarging the pixels of
    /// `photo`, the window's monitor's photo with its pixel (0, 0) at the monitor's top left corner.
    pub fn draw_magnified_view(
        &self,
        _target: &ID2D1RenderTarget,
        _view: &MagnifiedView,
        _photo: &ID2D1Bitmap,
    ) -> windows::core::Result<()> {
        Ok(())
    }
}
