//! The tool windows of the frozen screen: one overlay per monitor showing the photo 1:1, and the toolbar.
//! Each window owns its state through GWLP_USERDATA (a Box<ToolState> set in WM_NCCREATE, freed in
//! WM_NCDESTROY). The procedure touches only that state, never the app state, so it is safe to run
//! re-entrantly while the main window's handler holds the app state (CreateWindowEx, UpdateWindow and
//! DestroyWindow send messages synchronously).

use std::cell::RefCell;
use std::ffi::c_void;
use std::panic::AssertUnwindSafe;
use std::rc::Rc;

use windows::Win32::Foundation::{D2DERR_RECREATE_TARGET, HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_RECT_F, D2D_SIZE_U, D2D1_ALPHA_MODE_IGNORE, D2D1_COLOR_F, D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_BITMAP_INTERPOLATION_MODE_NEAREST_NEIGHBOR, D2D1_BITMAP_PROPERTIES, D2D1_DRAW_TEXT_OPTIONS_NONE,
    D2D1_FEATURE_LEVEL_DEFAULT, D2D1_HWND_RENDER_TARGET_PROPERTIES, D2D1_PRESENT_OPTIONS,
    D2D1_PRESENT_OPTIONS_IMMEDIATELY, D2D1_PRESENT_OPTIONS_NONE, D2D1_RENDER_TARGET_PROPERTIES,
    D2D1_RENDER_TARGET_TYPE_DEFAULT, D2D1_RENDER_TARGET_USAGE_NONE, D2D1_ROUNDED_RECT, ID2D1Bitmap, ID2D1Factory,
    ID2D1HwndRenderTarget, ID2D1SolidColorBrush,
};
use windows::Win32::Graphics::DirectWrite::{DWRITE_MEASURING_MODE_NATURAL, IDWriteTextFormat};
use windows::Win32::Graphics::Dwm::{DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND, DwmSetWindowAttribute};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::{BeginPaint, EndPaint, InvalidateRect, PAINTSTRUCT};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::VK_ESCAPE;
use windows::Win32::UI::WindowsAndMessaging::{
    CREATESTRUCTW, CreateWindowExW, DefWindowProcW, GWLP_USERDATA, GetClientRect, GetWindowLongPtrW, IDC_ARROW,
    LoadCursorW, PostMessageW, RegisterClassExW, SetWindowLongPtrW, WINDOW_EX_STYLE, WM_ACTIVATEAPP, WM_DEVICECHANGE,
    WM_DISPLAYCHANGE, WM_DPICHANGED, WM_ERASEBKGND, WM_KEYDOWN, WM_NCCREATE, WM_NCDESTROY, WM_PAINT, WM_SETTINGCHANGE,
    WNDCLASSEXW, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::{PCWSTR, w};

use super::sys::{self, f32_from_u32};
use super::{CloseReason, DPICHANGED_ON_TOOLS, MAIN_HWND, WM_APP_CLOSE};
use crate::common::{Ctx, Photo, PxRect};
use crate::log;

const OVERLAY_CLASS: PCWSTR = w!("PixmeterHwtestOverlay");
const TOOLBAR_CLASS: PCWSTR = w!("PixmeterHwtestToolbar");
const TOOLBAR_TEXT: &str = "hwtest — Esc fecha";

/// What the integrator chose for Direct2D presentation (`--present-vsync`).
#[derive(Clone, Copy)]
pub struct Present {
    pub vsync: bool,
}

impl Present {
    fn options(self) -> D2D1_PRESENT_OPTIONS {
        if self.vsync { D2D1_PRESENT_OPTIONS_NONE } else { D2D1_PRESENT_OPTIONS_IMMEDIATELY }
    }
}

struct ToolState {
    /// "overlay DISPLAY1" or "toolbar", for the log; readable without borrowing `inner`.
    role: String,
    inner: RefCell<ToolWindow>,
}

enum ToolWindow {
    Overlay(Overlay),
    Toolbar(Toolbar),
}

struct Overlay {
    factory: ID2D1Factory,
    present: Present,
    photo: Rc<Photo>,
    target: Option<ID2D1HwndRenderTarget>,
    bitmap: Option<ID2D1Bitmap>,
    painted: bool,
}

struct Toolbar {
    factory: ID2D1Factory,
    present: Present,
    text: IDWriteTextFormat,
    /// DPI of the monitor the toolbar was placed on, from the monitor list.
    expected_dpi: u32,
    target: Option<ID2D1HwndRenderTarget>,
    brushes: Option<(ID2D1SolidColorBrush, ID2D1SolidColorBrush)>,
    painted: bool,
}

/// What CreateWindowExW carries to WM_NCCREATE. If WM_NCCREATE never runs, the state stays here and drops normally.
type Slot = Option<Box<ToolState>>;

pub fn register_classes(hinstance: HINSTANCE) -> Result<(), String> {
    // SAFETY: loads a shared system cursor; nothing to free.
    let cursor = unsafe { LoadCursorW(None, IDC_ARROW) }.ctx("LoadCursorW(IDC_ARROW)")?;
    for class in [OVERLAY_CLASS, TOOLBAR_CLASS] {
        let wc = WNDCLASSEXW {
            cbSize: sys::size_u32::<WNDCLASSEXW>()?,
            lpfnWndProc: Some(tool_proc),
            hInstance: hinstance,
            hCursor: cursor,
            lpszClassName: class,
            ..Default::default()
        };
        // SAFETY: wc is fully initialized; the class name is a static wide string.
        if unsafe { RegisterClassExW(&wc) } == 0 {
            return Err(format!("RegisterClassExW (tool windows): {}", windows::core::Error::from_thread()));
        }
    }
    Ok(())
}

/// An overlay exactly at the photo's rectangle, in physical pixels: the process is Per-Monitor V2, so the
/// window is created at its final position and DPI and should never receive WM_DPICHANGED.
pub fn create_overlay(
    hinstance: HINSTANCE,
    factory: &ID2D1Factory,
    present: Present,
    photo: Rc<Photo>,
    short_name: &str,
) -> Result<HWND, String> {
    let rect = photo.rect;
    let state = ToolState {
        role: format!("overlay {short_name}"),
        inner: RefCell::new(ToolWindow::Overlay(Overlay {
            factory: factory.clone(),
            present,
            photo,
            target: None,
            bitmap: None,
            painted: false,
        })),
    };
    let hwnd = create(OVERLAY_CLASS, w!("hwtest overlay"), rect, None, hinstance, state)?;
    no_rounded_corners(hwnd, short_name);
    Ok(hwnd)
}

/// The toolbar, owned by the overlay of its monitor so it always stays above that overlay,
/// even when the overlay is clicked and activated.
pub fn create_toolbar(
    hinstance: HINSTANCE,
    factory: &ID2D1Factory,
    present: Present,
    text: &IDWriteTextFormat,
    owner: Option<HWND>,
    rect: PxRect,
    expected_dpi: u32,
) -> Result<HWND, String> {
    let state = ToolState {
        role: "toolbar".to_string(),
        inner: RefCell::new(ToolWindow::Toolbar(Toolbar {
            factory: factory.clone(),
            present,
            text: text.clone(),
            expected_dpi,
            target: None,
            brushes: None,
            painted: false,
        })),
    };
    create(TOOLBAR_CLASS, w!("hwtest toolbar"), rect, owner, hinstance, state)
}

fn create(
    class: PCWSTR,
    title: PCWSTR,
    rect: PxRect,
    owner: Option<HWND>,
    hinstance: HINSTANCE,
    state: ToolState,
) -> Result<HWND, String> {
    let mut slot: Slot = Some(Box::new(state));
    let ex: WINDOW_EX_STYLE = WS_EX_TOPMOST | WS_EX_TOOLWINDOW;
    // SAFETY: the class is registered; `slot` outlives the call and is reached only through this pointer
    // (in WM_NCCREATE) while the call runs.
    let created = unsafe {
        CreateWindowExW(
            ex,
            class,
            title,
            WS_POPUP,
            rect.left,
            rect.top,
            rect.width(),
            rect.height(),
            owner,
            None,
            Some(hinstance),
            Some((&raw mut slot).cast::<c_void>().cast_const()),
        )
    };
    if slot.is_some() {
        log::line("tool window: WM_NCCREATE did not take its state; it is dropped here");
    }
    created.ctx("CreateWindowExW (tool window)")
}

fn no_rounded_corners(hwnd: HWND, short_name: &str) {
    let pref = DWMWCP_DONOTROUND;
    let size = match sys::size_u32::<windows::Win32::Graphics::Dwm::DWM_WINDOW_CORNER_PREFERENCE>() {
        Ok(s) => s,
        Err(e) => {
            log::line(format!("overlay {short_name}: corner preference: {e}"));
            return;
        }
    };
    // SAFETY: pref lives for the call and size matches its type.
    let r = unsafe { DwmSetWindowAttribute(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE, (&raw const pref).cast(), size) };
    if let Err(e) = r {
        log::line(format!("overlay {short_name}: DwmSetWindowAttribute(DONOTROUND): {e}"));
    }
}

pub fn client_size(hwnd: HWND) -> Result<(i32, i32), String> {
    let mut rc = RECT::default();
    // SAFETY: rc is a valid out pointer.
    unsafe { GetClientRect(hwnd, &mut rc) }.ctx("GetClientRect")?;
    Ok((rc.right - rc.left, rc.bottom - rc.top))
}

unsafe extern "system" fn tool_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match std::panic::catch_unwind(AssertUnwindSafe(|| tool_message(hwnd, msg, wparam, lparam))) {
        Ok(r) => r,
        Err(_) => {
            log::line(format!("ERROR: panic in the tool window procedure (message {msg:#x})"));
            // SAFETY: default handling for our own window.
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
    }
}

fn tool_message(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // SAFETY: default handling for our own window.
    let default = || unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
    if msg == WM_NCCREATE {
        take_state_from_create(hwnd, lparam);
        return default();
    }
    if msg == WM_NCDESTROY {
        free_state(hwnd);
        return default();
    }
    // SAFETY: reads our own window's slot.
    let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *const ToolState;
    if ptr.is_null() {
        return default();
    }
    // SAFETY: set in WM_NCCREATE from Box::into_raw and freed only in WM_NCDESTROY, which detaches it first;
    // all on this thread, so the reference cannot outlive the allocation within this call.
    let state = unsafe { &*ptr };
    match msg {
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => {
            paint(hwnd, state);
            LRESULT(0)
        }
        WM_KEYDOWN if sys::wparam_bits(wparam) == usize::from(VK_ESCAPE.0) => {
            log::line(format!("Esc on {}", state.role));
            request_close(CloseReason::Esc);
            LRESULT(0)
        }
        WM_DPICHANGED => {
            DPICHANGED_ON_TOOLS.with(|c| c.set(c.get().saturating_add(1)));
            log::line(format!("UNEXPECTED: WM_DPICHANGED on {} (kept size; no SetWindowPos)", state.role));
            super::note_signal(&state.role, msg, wparam, lparam);
            LRESULT(0)
        }
        WM_DISPLAYCHANGE | WM_SETTINGCHANGE | WM_DEVICECHANGE => {
            super::note_signal(&state.role, msg, wparam, lparam);
            default()
        }
        WM_ACTIVATEAPP => {
            super::log_activate_app(&state.role, wparam, lparam);
            default()
        }
        _ => default(),
    }
}

fn take_state_from_create(hwnd: HWND, lparam: LPARAM) {
    let cs = lparam.0 as *const CREATESTRUCTW;
    if cs.is_null() {
        log::line("tool window: WM_NCCREATE without CREATESTRUCTW");
        return;
    }
    // SAFETY: WM_NCCREATE's lParam is the CREATESTRUCTW of this CreateWindowExW call.
    let slot = unsafe { (*cs).lpCreateParams }.cast::<Slot>();
    if slot.is_null() {
        log::line("tool window: WM_NCCREATE without a state slot");
        return;
    }
    // SAFETY: `create` passed a pointer to its live `Slot` local and does not touch it during the call.
    let Some(state) = (unsafe { (*slot).take() }) else {
        log::line("tool window: state slot already empty");
        return;
    };
    // SAFETY: stores the pointer in our own window's slot. The return value is the previous value (0 for a
    // new window); the only failure mode is an invalid hwnd, and this is the window being created.
    unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(state) as isize) };
}

fn free_state(hwnd: HWND) {
    // SAFETY: detaches the pointer from our own window's slot so no later message can reach it.
    let ptr = unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0) } as *mut ToolState;
    if ptr.is_null() {
        return;
    }
    // SAFETY: the pointer came from Box::into_raw in WM_NCCREATE and was just detached.
    let state = unsafe { Box::from_raw(ptr) };
    if state.inner.try_borrow_mut().is_err() {
        // Destroyed from inside its own paint (should not happen); leaking beats freeing memory in use.
        log::line(format!("tool window {}: destroyed while its state was in use; leaking it", state.role));
        std::mem::forget(state);
    }
}

fn paint(hwnd: HWND, state: &ToolState) {
    let mut ps = PAINTSTRUCT::default();
    // SAFETY: WM_PAINT on our own window; paired with EndPaint below.
    let hdc = unsafe { BeginPaint(hwnd, &mut ps) };
    if hdc.is_invalid() {
        log::line(format!("{}: BeginPaint returned no DC", state.role));
    }
    match state.inner.try_borrow_mut() {
        Ok(mut inner) => {
            let r = match &mut *inner {
                ToolWindow::Overlay(o) => paint_overlay(hwnd, o, &state.role),
                ToolWindow::Toolbar(t) => paint_toolbar(hwnd, t, &state.role),
            };
            if let Err(e) = r {
                log::line(format!("ERROR: painting {}: {e}", state.role));
            }
        }
        Err(_) => log::line(format!("{}: WM_PAINT re-entered while painting; skipped", state.role)),
    }
    // SAFETY: pairs the BeginPaint above.
    if !unsafe { EndPaint(hwnd, &ps) }.as_bool() {
        log::line(format!("{}: EndPaint returned FALSE", state.role));
    }
}

/// 1 DIP = 1 pixel: the overlay draws the photo pixel for pixel.
fn target_properties(dpi: f32) -> D2D1_RENDER_TARGET_PROPERTIES {
    D2D1_RENDER_TARGET_PROPERTIES {
        r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
        pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_IGNORE },
        dpiX: dpi,
        dpiY: dpi,
        usage: D2D1_RENDER_TARGET_USAGE_NONE,
        minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
    }
}

fn create_target(
    factory: &ID2D1Factory,
    hwnd: HWND,
    dpi: f32,
    present: Present,
) -> Result<ID2D1HwndRenderTarget, String> {
    let (w, h) = client_size(hwnd)?;
    let size =
        D2D_SIZE_U { width: u32::try_from(w).ctx("client width")?, height: u32::try_from(h).ctx("client height")? };
    let props = target_properties(dpi);
    let hwnd_props = D2D1_HWND_RENDER_TARGET_PROPERTIES { hwnd, pixelSize: size, presentOptions: present.options() };
    // SAFETY: both property structs live for the call.
    unsafe { factory.CreateHwndRenderTarget(&props, &hwnd_props) }.ctx("CreateHwndRenderTarget")
}

/// After EndDraw: on D2DERR_RECREATE_TARGET, drop the device resources and ask for another paint.
fn end_draw(hwnd: HWND, target: &ID2D1HwndRenderTarget, role: &str) -> Result<bool, String> {
    // SAFETY: pairs the BeginDraw in the caller.
    match unsafe { target.EndDraw(None, None) } {
        Ok(()) => Ok(true),
        Err(e) if e.code() == D2DERR_RECREATE_TARGET => {
            log::line(format!("{role}: D2DERR_RECREATE_TARGET; recreating the render target"));
            // SAFETY: invalidates our own window.
            if !unsafe { InvalidateRect(Some(hwnd), None, false) }.as_bool() {
                log::line(format!("{role}: InvalidateRect returned FALSE"));
            }
            Ok(false)
        }
        Err(e) => Err(format!("EndDraw: {e}")),
    }
}

fn paint_overlay(hwnd: HWND, o: &mut Overlay, role: &str) -> Result<(), String> {
    let t_start = log::now_ms();
    let first = !o.painted;
    let target = match &o.target {
        Some(t) => t.clone(),
        None => {
            let t = create_target(&o.factory, hwnd, 96.0, o.present)?;
            o.target = Some(t.clone());
            t
        }
    };
    let t_target = log::now_ms();
    let bitmap = match &o.bitmap {
        Some(b) => b.clone(),
        None => {
            let b = upload(&target, &o.photo)?;
            o.bitmap = Some(b.clone());
            b
        }
    };
    let t_bitmap = log::now_ms();
    let r =
        D2D_RECT_F { left: 0.0, top: 0.0, right: f32_from_u32(o.photo.width), bottom: f32_from_u32(o.photo.height) };
    let black = D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 1.0 };
    // SAFETY: plain drawing calls on a live render target, bracketed by BeginDraw/EndDraw.
    unsafe {
        target.BeginDraw();
        target.Clear(Some(&black));
        target.DrawBitmap(&bitmap, Some(&r), 1.0, D2D1_BITMAP_INTERPOLATION_MODE_NEAREST_NEIGHBOR, Some(&r));
    }
    if !end_draw(hwnd, &target, role)? {
        o.target = None;
        o.bitmap = None;
        return Ok(());
    }
    o.painted = true;
    if first {
        let done = log::now_ms();
        log::line(format!(
            "{role} first paint: render target {:.1} ms, bitmap upload {:.1} ms, draw {:.1} ms",
            t_target - t_start,
            t_bitmap - t_target,
            done - t_bitmap
        ));
    }
    Ok(())
}

fn upload(target: &ID2D1HwndRenderTarget, photo: &Photo) -> Result<ID2D1Bitmap, String> {
    let pitch = photo.width.checked_mul(4).ok_or("photo pitch overflows")?;
    let expected = u64::from(pitch) * u64::from(photo.height);
    let actual = u64::try_from(photo.bgra.len()).ctx("photo length")?;
    if actual != expected {
        return Err(format!("photo buffer is {actual} bytes; {}x{} BGRA needs {expected}", photo.width, photo.height));
    }
    let props = D2D1_BITMAP_PROPERTIES {
        pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_IGNORE },
        dpiX: 96.0,
        dpiY: 96.0,
    };
    let size = D2D_SIZE_U { width: photo.width, height: photo.height };
    // SAFETY: the source buffer holds height rows of `pitch` bytes (checked above) and lives for the call.
    unsafe { target.CreateBitmap(size, Some(photo.bgra.as_ptr().cast()), pitch, &props) }.ctx("CreateBitmap")
}

fn paint_toolbar(hwnd: HWND, t: &mut Toolbar, role: &str) -> Result<(), String> {
    let target = match &t.target {
        Some(target) => target.clone(),
        None => {
            // SAFETY: plain query on our own window.
            let dpi = unsafe { GetDpiForWindow(hwnd) };
            if dpi != t.expected_dpi {
                log::line(format!("{role}: window DPI {dpi} differs from its monitor's {}", t.expected_dpi));
            }
            let dpi = if dpi == 0 { t.expected_dpi } else { dpi };
            let target = create_target(&t.factory, hwnd, f32_from_u32(dpi), t.present)?;
            t.target = Some(target.clone());
            target
        }
    };
    let (panel, text) = match &t.brushes {
        Some(b) => b.clone(),
        None => {
            let panel_color = D2D1_COLOR_F { r: 0.16, g: 0.16, b: 0.17, a: 1.0 };
            let text_color = D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 1.0 };
            // SAFETY: the colors live for the calls.
            let panel = unsafe { target.CreateSolidColorBrush(&panel_color, None) }.ctx("CreateSolidColorBrush")?;
            // SAFETY: as above.
            let text = unsafe { target.CreateSolidColorBrush(&text_color, None) }.ctx("CreateSolidColorBrush")?;
            t.brushes = Some((panel.clone(), text.clone()));
            (panel, text)
        }
    };
    let backdrop = D2D1_COLOR_F { r: 0.05, g: 0.05, b: 0.05, a: 1.0 };
    let words: Vec<u16> = TOOLBAR_TEXT.encode_utf16().collect();
    // SAFETY: plain drawing calls on a live render target, bracketed by BeginDraw/EndDraw.
    unsafe {
        let size = target.GetSize();
        let full = D2D_RECT_F { left: 0.0, top: 0.0, right: size.width, bottom: size.height };
        let rounded = D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F { left: 1.0, top: 1.0, right: size.width - 1.0, bottom: size.height - 1.0 },
            radiusX: 8.0,
            radiusY: 8.0,
        };
        target.BeginDraw();
        target.Clear(Some(&backdrop));
        target.FillRoundedRectangle(&rounded, &panel);
        target.DrawText(&words, &t.text, &full, &text, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
    }
    if !end_draw(hwnd, &target, role)? {
        t.target = None;
        t.brushes = None;
        return Ok(());
    }
    if !t.painted {
        t.painted = true;
        log::line(format!("{role} first paint done"));
    }
    Ok(())
}

/// Posts a close request to the main window; it is handled from the message loop, outside any
/// re-entrant call.
pub fn request_close(reason: CloseReason) {
    let main = MAIN_HWND.with(std::cell::Cell::get);
    let at = match sys::lparam_from_ms(log::now_ms()) {
        Ok(l) => l,
        Err(e) => {
            log::line(format!("ERROR: close request ({}): {e}", reason.name()));
            return;
        }
    };
    // SAFETY: posts to our own main window.
    if let Err(e) = unsafe { PostMessageW(Some(main), WM_APP_CLOSE, WPARAM(reason.code()), at) } {
        log::line(format!("ERROR: posting the close request ({}): {e}", reason.name()));
    }
}
