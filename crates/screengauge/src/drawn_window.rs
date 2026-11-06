//! What Settings and the tray icon's menu do alike as windows ScreenGauge draws itself with Direct2D: they follow the
//! cursor out of the window, and paint onto a render target they keep from one paint to the next, made again after the
//! graphics device was lost.

use std::cell::{Cell, RefCell};
use std::thread::LocalKey;

use windows::Win32::Foundation::{D2DERR_RECREATE_TARGET, E_FAIL, HWND};
use windows::Win32::Graphics::Direct2D::ID2D1HwndRenderTarget;
use windows::Win32::Graphics::Gdi::{BeginPaint, EndPaint, InvalidateRect, PAINTSTRUCT};
use windows::Win32::UI::Input::KeyboardAndMouse::{TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent};
use windows::core::Error;

use crate::log;
use crate::win32::struct_size;

/// The message TrackMouseEvent sends when the cursor leaves the window. The windows crate has it only among the common
/// controls' features, which ScreenGauge doesn't otherwise need.
pub const WM_MOUSELEAVE: u32 = 0x02A3;

/// The render target a window keeps between paints: none before the first, and none after the graphics device was lost.
pub type KeptTarget = LocalKey<RefCell<Option<ID2D1HwndRenderTarget>>>;

/// Asks Windows for WM_MOUSELEAVE when the cursor leaves `hwnd`, once each time it comes in: `asked` says whether it
/// was asked since the last WM_MOUSELEAVE, which sets it back. A failure is logged under `what`: the window may then
/// stay as the cursor left it, and the next move over it asks again.
pub fn follow_leave(hwnd: HWND, asked: &'static LocalKey<Cell<bool>>, what: &str) {
    if asked.get() {
        return;
    }
    let mut track = TRACKMOUSEEVENT {
        cbSize: const { struct_size::<TRACKMOUSEEVENT>() },
        dwFlags: TME_LEAVE,
        hwndTrack: hwnd,
        dwHoverTime: 0,
    };
    // SAFETY: the caller's own window; the struct is fully set and outlives the call.
    match unsafe { TrackMouseEvent(&mut track) } {
        Ok(()) => asked.set(true),
        Err(e) => log::failure(what, &e),
    }
}

/// Answers WM_PAINT on `hwnd` with `draw`, whose failure is logged under `what`.
pub fn paint(hwnd: HWND, what: &str, draw: impl FnOnce() -> windows::core::Result<()>) {
    let mut paint = PAINTSTRUCT::default();
    // SAFETY: WM_PAINT on the caller's own window, ended below. Direct2D draws without the device context it returns.
    unsafe { BeginPaint(hwnd, &mut paint) };
    if let Err(e) = draw() {
        log::failure(what, &e);
    }
    // SAFETY: ends the BeginPaint above. EndPaint always returns TRUE.
    let _ = unsafe { EndPaint(hwnd, &paint) };
}

/// Draws one frame of `hwnd` with `draw` on the target `kept` holds, or on one `make` makes when it holds none, and
/// keeps the target for the next frame. When the graphics device was lost, as after a driver update, the target is
/// dropped and the window asked for another paint, which makes it again: then there is no frame, and `None` comes back.
/// A failure inside a frame that failed as a whole is logged under `what`, so it isn't lost.
pub fn frame<T>(
    hwnd: HWND,
    kept: &'static KeptTarget,
    make: impl FnOnce() -> windows::core::Result<ID2D1HwndRenderTarget>,
    what: &str,
    draw: impl FnOnce(&ID2D1HwndRenderTarget) -> windows::core::Result<T>,
) -> windows::core::Result<Option<T>> {
    let target = match kept.take() {
        Some(target) => target,
        None => make()?,
    };
    // SAFETY: drawing calls on a live render target, between BeginDraw and EndDraw.
    unsafe { target.BeginDraw() };
    let drawn = draw(&target);
    // SAFETY: ends the BeginDraw above.
    let ended = unsafe { target.EndDraw(None, None) };
    let failed = match ended {
        Ok(()) => {
            kept.set(Some(target));
            return drawn.map(Some);
        }
        Err(e) => e,
    };
    if let Err(inside) = drawn {
        log::failure(what, &inside);
    }
    if failed.code() != D2DERR_RECREATE_TARGET {
        return Err(failed);
    }
    // SAFETY: the caller's own window.
    if !unsafe { InvalidateRect(Some(hwnd), None, false) }.as_bool() {
        return Err(Error::new(
            E_FAIL,
            "InvalidateRect returned FALSE after the graphics device was lost",
        ));
    }
    Ok(None)
}
