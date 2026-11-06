//! How a frozen screen window draws its part of the scene: its monitor's photo, put on the graphics card once, and
//! the overlay over it, made again with the render target when Direct2D loses it.

use screengauge_core::session::Photo;
use windows::Win32::Foundation::{D2DERR_RECREATE_TARGET, E_FAIL, HWND};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_SIZE_U, D2D1_ALPHA_MODE_IGNORE, D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_BITMAP_INTERPOLATION_MODE_NEAREST_NEIGHBOR, D2D1_BITMAP_PROPERTIES,
    D2D1_FEATURE_LEVEL_DEFAULT, D2D1_HWND_RENDER_TARGET_PROPERTIES,
    D2D1_PRESENT_OPTIONS_IMMEDIATELY, D2D1_RENDER_TARGET_PROPERTIES,
    D2D1_RENDER_TARGET_TYPE_DEFAULT, D2D1_RENDER_TARGET_USAGE_NONE, ID2D1Bitmap,
    ID2D1HwndRenderTarget,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::{BeginPaint, EndPaint, InvalidateRect, PAINTSTRUCT};
use windows::core::Error;

use super::overlay::Overlay;
use super::{Pressed, Window};
use crate::theme::Mode;
use crate::{graphics, log};

/// The photo on the graphics card, ready to draw, with what draws the measurements and the notice over it. They belong
/// to the render target, so they are made again together when it is lost.
pub(super) struct Drawing {
    target: ID2D1HwndRenderTarget,
    bitmap: ID2D1Bitmap,
    overlay: Overlay,
    /// The palette the overlay draws in. A paint that finds the window in the other one makes the drawing again.
    mode: Mode,
}

pub(super) fn paint(hwnd: HWND, window: &Window) {
    let mut paint = PAINTSTRUCT::default();
    // SAFETY: WM_PAINT on our own window, ended below. Direct2D draws without the device context it returns.
    unsafe { BeginPaint(hwnd, &mut paint) };
    if let Err(e) = draw(hwnd, window) {
        log::failure("frozen screen: drawing a window", &e);
    }
    // SAFETY: ends the BeginPaint above. EndPaint always returns TRUE.
    let _ = unsafe { EndPaint(hwnd, &paint) };
}

/// Draws the window's part of the scene: its monitor's photo, and over it the measurements pinned on this monitor, what
/// Dimensions measures when that is on this monitor, and Snip's area, Inspect's magnified view, the toolbar, the
/// tolerance indicator and the notice when they are here, in that order.
pub(super) fn draw(hwnd: HWND, window: &Window) -> windows::core::Result<()> {
    // A copy, so the session is borrowed only for a moment.
    let shown = window.session.shown();
    let here = shown.on(window.monitor);
    let mut drawing = window
        .drawing
        .try_borrow_mut()
        .map_err(|_| Error::new(E_FAIL, "painting started again while painting"))?;
    let ready = match drawing.take() {
        Some(ready) if ready.mode == window.mode.get() => ready,
        // None yet, or made before the theme changed (`FrozenScreen::follow`).
        _ => Drawing::new(hwnd, window)?,
    };
    // SAFETY: drawing calls on a live render target, between BeginDraw and EndDraw. With no rectangles, the whole
    // bitmap goes to the target's origin at its own size, and both are at 96 DPI: one pixel for one pixel.
    unsafe {
        ready.target.BeginDraw();
        ready.target.DrawBitmap(
            &ready.bitmap,
            None,
            1.0,
            D2D1_BITMAP_INTERPOLATION_MODE_NEAREST_NEIGHBOR,
            None,
        );
    }
    // A label, the indicator or a notice that can't be laid out leaves the photo and the lines as they are; the first
    // failure is returned once the frame is finished, and any later one is logged.
    let measured = ready
        .overlay
        .draw(&ready.target, &here.pinned, here.live, here.drawn_rectangle);
    // Snip's corners and label go over the drawn rectangle and the pinned measurements.
    let snipped = match here.snip_area {
        Some(area) => ready.overlay.draw_snip(&ready.target, area),
        None => Ok(()),
    };
    // The magnified view enlarges the photo itself, never what is drawn over it.
    let magnified = match here.magnified_view {
        Some(view) => ready
            .overlay
            .draw_magnified_view(&ready.target, view, &ready.bitmap),
        None => Ok(()),
    };
    let toolbar = match here.toolbar {
        Some(toolbar) => ready.overlay.draw_toolbar(
            &ready.target,
            toolbar,
            window.pressed.get().and_then(Pressed::on_toolbar),
        ),
        None => Ok(()),
    };
    let indicated = match here.tolerance_indicator {
        Some(indicator) => ready
            .overlay
            .draw_tolerance(&ready.target, indicator, here.live),
        None => Ok(()),
    };
    let noticed = match here.notice {
        Some(notice) => ready.overlay.draw_notice(&ready.target, notice),
        None => Ok(()),
    };
    let overlaid = first_failure([
        ("frozen screen: drawing the measurements", measured),
        ("frozen screen: drawing the Snip's area", snipped),
        ("frozen screen: drawing the magnified view", magnified),
        ("frozen screen: drawing the toolbar", toolbar),
        ("frozen screen: drawing the tolerance indicator", indicated),
        ("frozen screen: drawing the notice", noticed),
    ]);
    // SAFETY: ends the BeginDraw above.
    match unsafe { ready.target.EndDraw(None, None) } {
        Ok(()) => {
            *drawing = Some(ready);
            overlaid
        }
        // The graphics device was lost, as after a driver update: the next paint makes everything again.
        Err(e) if e.code() == D2DERR_RECREATE_TARGET => {
            // SAFETY: our own window.
            if !unsafe { InvalidateRect(Some(hwnd), None, false) }.as_bool() {
                return Err(Error::new(
                    E_FAIL,
                    "InvalidateRect returned FALSE after the graphics device was lost",
                ));
            }
            Ok(())
        }
        Err(e) => Err(e),
    }
}

impl Drawing {
    /// Puts the window's photo, from the scene, on the graphics card, and makes what draws the measurement over it.
    fn new(hwnd: HWND, window: &Window) -> windows::core::Result<Drawing> {
        let (size, rect, scale) = window.with_photo(|photo| {
            let size = D2D_SIZE_U {
                width: photo.image.width(),
                height: photo.image.height(),
            };
            (size, photo.rect, photo.scale)
        })?;
        let pitch = size
            .width
            .checked_mul(4)
            .ok_or_else(|| Error::new(E_FAIL, "the photo is too wide"))?;
        let format = D2D1_PIXEL_FORMAT {
            format: DXGI_FORMAT_B8G8R8A8_UNORM,
            alphaMode: D2D1_ALPHA_MODE_IGNORE,
        };
        // 96 DPI makes one Direct2D unit one physical pixel, on every monitor whatever its scale.
        let target_properties = D2D1_RENDER_TARGET_PROPERTIES {
            r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
            pixelFormat: format,
            dpiX: 96.0,
            dpiY: 96.0,
            usage: D2D1_RENDER_TARGET_USAGE_NONE,
            minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
        };
        let window_properties = D2D1_HWND_RENDER_TARGET_PROPERTIES {
            hwnd,
            pixelSize: size,
            // Kept IMMEDIATELY now that the measurement moves with the mouse: each frame must not wait for the next
            // refresh, or the message loop, and the mouse moves queued behind it, would slow to the display's pace.
            // A window is composed by the desktop, which shows whole frames, so presenting at once doesn't tear.
            presentOptions: D2D1_PRESENT_OPTIONS_IMMEDIATELY,
        };
        // SAFETY: both property structs outlive the call.
        let target = unsafe {
            window
                .factory
                .CreateHwndRenderTarget(&target_properties, &window_properties)
        }?;
        let bitmap_properties = D2D1_BITMAP_PROPERTIES {
            pixelFormat: format,
            dpiX: 96.0,
            dpiY: 96.0,
        };
        let bitmap = window.with_photo(|photo| {
            // SAFETY: an Image holds exactly `height` rows of `pitch` bytes, and outlives the call, which copies them.
            unsafe {
                target.CreateBitmap(
                    size,
                    Some(photo.image.bgra().as_ptr().cast()),
                    pitch,
                    &bitmap_properties,
                )
            }
        })??;
        let mode = window.mode.get();
        let overlay = Overlay::new(&target, &graphics::fonts()?, rect, scale, mode)?;
        Ok(Drawing {
            target,
            bitmap,
            overlay,
            mode,
        })
    }
}

impl Window {
    /// Runs `f` with the window's photo, borrowed from the session only meanwhile.
    fn with_photo<R>(&self, f: impl FnOnce(&Photo) -> R) -> windows::core::Result<R> {
        let session = self
            .session
            .core
            .try_borrow()
            .map_err(|_| Error::new(E_FAIL, "drawing started while an event was handled"))?;
        let photo = session
            .scene()
            .frozen_screen
            .get(self.monitor)
            .ok_or_else(|| Error::new(E_FAIL, "the scene has no photo for this window"))?;
        Ok(f(photo))
    }
}

/// The first of `results` that failed, with every later failure logged under its name.
fn first_failure<const N: usize>(
    results: [(&str, windows::core::Result<()>); N],
) -> windows::core::Result<()> {
    let mut first = Ok(());
    for (what, result) in results {
        if let Err(e) = result {
            if first.is_ok() {
                first = Err(e);
            } else {
                log::failure(what, &e);
            }
        }
    }
    first
}
