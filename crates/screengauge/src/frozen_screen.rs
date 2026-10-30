//! The frozen screen on screen: one window over each monitor, at its exact place and size in physical pixels, drawing
//! that monitor's photo pixel for pixel with Direct2D. Nothing marks the screen as frozen (no dimming, border, icon or
//! word), and the cursor over it is the thin cross. Each window owns its photo, so destroying the windows when the
//! tools close, by dropping [`FrozenScreen`], erases the photos with them.

use std::cell::{OnceCell, RefCell};
use std::ffi::c_void;

use windows::Win32::Foundation::{
    D2DERR_RECREATE_TARGET, E_FAIL, ERROR_SUCCESS, GetLastError, HINSTANCE, HWND, LPARAM, LRESULT,
    POINT, RECT, SetLastError, WPARAM,
};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_SIZE_U, D2D1_ALPHA_MODE_IGNORE, D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_BITMAP_INTERPOLATION_MODE_NEAREST_NEIGHBOR, D2D1_BITMAP_PROPERTIES,
    D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_FEATURE_LEVEL_DEFAULT,
    D2D1_HWND_RENDER_TARGET_PROPERTIES, D2D1_PRESENT_OPTIONS_IMMEDIATELY,
    D2D1_RENDER_TARGET_PROPERTIES, D2D1_RENDER_TARGET_TYPE_DEFAULT, D2D1_RENDER_TARGET_USAGE_NONE,
    D2D1CreateFactory, ID2D1Bitmap, ID2D1Factory, ID2D1HwndRenderTarget,
};
use windows::Win32::Graphics::Dwm::{
    DWM_WINDOW_CORNER_PREFERENCE, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND,
    DwmSetWindowAttribute,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::{BeginPaint, EndPaint, InvalidateRect, PAINTSTRUCT};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::VK_ESCAPE;
use windows::Win32::UI::WindowsAndMessaging::{
    CREATESTRUCTW, CreateWindowExW, DefWindowProcW, DestroyWindow, GWLP_USERDATA, GetCursorPos,
    GetWindowLongPtrW, IDC_CROSS, LoadCursorW, PostMessageW, RegisterClassExW, SW_SHOWNA,
    SetCursor, SetForegroundWindow, SetWindowLongPtrW, ShowWindow, WM_APP, WM_CLOSE, WM_ERASEBKGND,
    WM_KEYDOWN, WM_NCCREATE, WM_NCDESTROY, WM_PAINT, WNDCLASSEXW, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
    WS_POPUP,
};
use windows::core::{Error, PCWSTR, w};

use crate::capture::Photo;
use crate::log;
use crate::win32::struct_size;

const CLASS_NAME: PCWSTR = w!("ScreenGauge.FrozenScreen");

/// What a frozen screen window posts to the resident window when the user closes the tools. Posted, never sent: the
/// resident window then destroys these windows, which must not happen inside one of their own window procedures.
pub const CLOSE_REQUEST: u32 = WM_APP + 2;

/// Registered once at startup. The cursor over the frozen screen is Windows' thin cross.
pub fn register_class(instance: HINSTANCE) -> windows::core::Result<()> {
    let class = WNDCLASSEXW {
        cbSize: const { struct_size::<WNDCLASSEXW>() },
        lpfnWndProc: Some(window_proc),
        hInstance: instance,
        // SAFETY: loads a shared system cursor, which is never freed.
        hCursor: unsafe { LoadCursorW(None, IDC_CROSS) }?,
        lpszClassName: CLASS_NAME,
        ..Default::default()
    };
    // SAFETY: `class` is fully initialized, and its name is a static string.
    if unsafe { RegisterClassExW(&class) } == 0 {
        return Err(Error::from_thread());
    }
    Ok(())
}

thread_local! {
    /// Made at the first call and kept until ScreenGauge exits, so nothing exists before the first call. The factory
    /// keeps the graphics device its windows draw with, and with it the graphics card's driver. On the owner's
    /// computer, a factory made for each call made every call about 350 ms slower, past the 250 ms target (WPC-286);
    /// kept, ScreenGauge at rest after four calls used 47 MB in Task Manager, under the 80 MB target.
    static FACTORY: OnceCell<ID2D1Factory> = const { OnceCell::new() };
}

fn factory() -> windows::core::Result<ID2D1Factory> {
    FACTORY.with(|kept| {
        if let Some(factory) = kept.get() {
            return Ok(factory.clone());
        }
        // SAFETY: a single-threaded factory with default options, used only on this thread.
        let factory: ID2D1Factory =
            unsafe { D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None) }?;
        Ok(kept.get_or_init(|| factory).clone())
    })
}

/// The frozen screen's windows, one per monitor, while the tools are open.
pub struct FrozenScreen {
    windows: Vec<HWND>,
}

impl FrozenScreen {
    /// Shows `photos`, one window per monitor, and gives the keyboard to the window under the cursor so Esc reaches
    /// it. `resident` receives [`CLOSE_REQUEST`].
    pub fn show(photos: Vec<Photo>, resident: HWND) -> windows::core::Result<FrozenScreen> {
        // SAFETY: no module name, so this is the handle of ScreenGauge.exe itself.
        let instance: HINSTANCE = unsafe { GetModuleHandleW(None) }?.into();
        let factory = factory()?;
        let rects: Vec<RECT> = photos.iter().map(|photo| photo.rect).collect();
        // Should a window fail, dropping `frozen` destroys the ones already made.
        let mut frozen = FrozenScreen {
            windows: Vec::with_capacity(photos.len()),
        };
        for photo in photos {
            frozen
                .windows
                .push(create_window(instance, photo, &factory, resident)?);
        }
        for &window in &frozen.windows {
            // SAFETY: our own window. ShowWindow returns whether it was visible before, not an error.
            let _ = unsafe { ShowWindow(window, SW_SHOWNA) };
            // Drawn here rather than left to WM_PAINT, so a photo that can't be drawn fails the call instead of
            // leaving an empty window over its monitor. The WM_PAINT still pending draws it once more.
            with_state(window, |state| draw(window, state)).unwrap_or_else(|| {
                Err(Error::new(E_FAIL, "a frozen screen window has no state"))
            })?;
        }
        frozen.take_keyboard(&rects);
        Ok(frozen)
    }

    /// Brings the window under the cursor to the front, so the keyboard goes to it, and shows the cross. The photo is
    /// already on screen and closes with the global shortcut, so a failure here is logged and the call goes on. The
    /// global shortcut grants ScreenGauge the right to come to the front; should Windows refuse anyway, Esc won't
    /// reach the frozen screen until it is clicked.
    fn take_keyboard(&self, rects: &[RECT]) {
        let mut cursor = POINT::default();
        // SAFETY: fills a local.
        let under_cursor = match unsafe { GetCursorPos(&mut cursor) } {
            Ok(()) => rects.iter().position(|rect| {
                (rect.left..rect.right).contains(&cursor.x)
                    && (rect.top..rect.bottom).contains(&cursor.y)
            }),
            Err(e) => {
                log::failure("frozen screen: finding the cursor", &e);
                None
            }
        };
        // The cursor is always on a monitor; should it not be, the first monitor's window takes the keyboard.
        if let Some(&window) = self.windows.get(under_cursor.unwrap_or(0)) {
            // SAFETY: our own window.
            if !unsafe { SetForegroundWindow(window) }.as_bool() {
                log::failure(
                    "frozen screen",
                    &"Windows didn't bring it to the front, so Esc won't reach it",
                );
            }
        }
        // The cross shows at once, without waiting for the mouse to move.
        // SAFETY: loads a shared system cursor, which is never freed.
        match unsafe { LoadCursorW(None, IDC_CROSS) } {
            Ok(cross) => {
                // SAFETY: a valid cursor. SetCursor returns the previous one, which needs nothing.
                let _ = unsafe { SetCursor(Some(cross)) };
            }
            Err(e) => log::failure("frozen screen: showing the cross", &e),
        }
    }
}

impl Drop for FrozenScreen {
    fn drop(&mut self) {
        for window in self.windows.drain(..) {
            destroy(window);
        }
    }
}

/// Destroys one of the frozen screen's windows, from outside its window procedure.
fn destroy(window: HWND) {
    // SAFETY: our own window, on the thread that created it, and outside its window procedure.
    if let Err(e) = unsafe { DestroyWindow(window) } {
        log::failure("frozen screen: closing a window", &e);
    }
}

/// What each window keeps, from WM_NCCREATE to WM_NCDESTROY.
struct Window {
    photo: Photo,
    factory: ID2D1Factory,
    resident: HWND,
    /// Made at the first draw, and again when Direct2D says the graphics device was lost.
    drawing: RefCell<Option<Drawing>>,
}

/// The photo on the graphics card, ready to draw.
struct Drawing {
    target: ID2D1HwndRenderTarget,
    bitmap: ID2D1Bitmap,
}

/// What CreateWindowExW hands to WM_NCCREATE. Should WM_NCCREATE never come, the window state stays here and drops
/// with it.
type Slot = Option<Box<Window>>;

fn create_window(
    instance: HINSTANCE,
    photo: Photo,
    factory: &ID2D1Factory,
    resident: HWND,
) -> windows::core::Result<HWND> {
    let rect = photo.rect;
    let mut slot: Slot = Some(Box::new(Window {
        photo,
        factory: factory.clone(),
        resident,
        drawing: RefCell::new(None),
    }));
    // SAFETY: the class is registered. `slot` outlives the call, and only WM_NCCREATE reaches it, through this
    // pointer. The process is Per-Monitor V2, so the rectangle is in physical pixels.
    let window = unsafe {
        CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            CLASS_NAME,
            w!("ScreenGauge"),
            WS_POPUP,
            rect.left,
            rect.top,
            rect.right - rect.left,
            rect.bottom - rect.top,
            None,
            None,
            Some(instance),
            Some((&raw mut slot).cast::<c_void>().cast_const()),
        )
    }?;
    // Windows 11 may round a window's corners, which would show the live screen in them.
    let corners = DWMWCP_DONOTROUND;
    // SAFETY: `corners` outlives the call, and the size is its type's.
    let squared = unsafe {
        DwmSetWindowAttribute(
            window,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            (&raw const corners).cast(),
            const { struct_size::<DWM_WINDOW_CORNER_PREFERENCE>() },
        )
    };
    if let Err(e) = squared {
        destroy(window);
        return Err(e);
    }
    Ok(window)
}

extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        // FALSE from WM_NCCREATE makes CreateWindowExW fail.
        WM_NCCREATE if !adopt(hwnd, lparam) => return LRESULT(0),
        WM_NCDESTROY => release(hwnd),
        _ => {}
    }
    let handled = with_state(hwnd, |window| match message {
        WM_ERASEBKGND => Some(LRESULT(1)),
        WM_PAINT => {
            paint(hwnd, window);
            Some(LRESULT(0))
        }
        WM_KEYDOWN if wparam.0 == usize::from(VK_ESCAPE.0) => {
            request_close(window.resident);
            Some(LRESULT(0))
        }
        // Alt+F4 closes the tools as a whole, never one monitor's window alone.
        WM_CLOSE => {
            request_close(window.resident);
            Some(LRESULT(0))
        }
        _ => None,
    });
    // SAFETY: the arguments are the ones Windows passed to this window procedure.
    handled
        .flatten()
        .unwrap_or_else(|| unsafe { DefWindowProcW(hwnd, message, wparam, lparam) })
}

/// Runs `f` with the window's state, when it has one: from WM_NCCREATE to WM_NCDESTROY.
fn with_state<R>(hwnd: HWND, f: impl FnOnce(&Window) -> R) -> Option<R> {
    // SAFETY: the pointer was stored by `adopt` and is cleared by `release` before the state is freed, all on this
    // thread. Only WM_NCDESTROY frees it, and nothing `f` does destroys the window.
    let window = unsafe { (GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const Window).as_ref() };
    window.map(f)
}

/// Moves the window state from `create_window`'s slot to the window. False when it can't, so the window is never
/// created: without its state it would show nothing and ignore Esc.
fn adopt(hwnd: HWND, lparam: LPARAM) -> bool {
    // SAFETY: WM_NCCREATE's lParam is the CREATESTRUCTW of the CreateWindowExW call in `create_window`.
    let create = unsafe { &*(lparam.0 as *const CREATESTRUCTW) };
    // SAFETY: `create_window` passed a pointer to its live slot and doesn't touch it during the call.
    let Some(slot) = (unsafe { create.lpCreateParams.cast::<Slot>().as_mut() }) else {
        return false;
    };
    let Some(window) = slot.take() else {
        return false;
    };
    let window = Box::into_raw(window);
    // SetWindowLongPtrW returns the previous value, 0 for a new window, and 0 on failure too: only the thread's last
    // error tells them apart.
    // SAFETY: sets this thread's last error.
    unsafe { SetLastError(ERROR_SUCCESS) };
    // SAFETY: our own window.
    let previous = unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, window as isize) };
    // SAFETY: reads this thread's last error.
    if previous == 0 && unsafe { GetLastError() } != ERROR_SUCCESS {
        // SAFETY: the pointer came from Box::into_raw above and was never stored, so the slot owns it again and
        // `create_window` drops it.
        *slot = Some(unsafe { Box::from_raw(window) });
        return false;
    }
    true
}

/// Frees the window state, the photo with it.
fn release(hwnd: HWND) {
    // SAFETY: our own window; clearing the pointer first means no later message can reach the state.
    let window = unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0) } as *mut Window;
    if !window.is_null() {
        // SAFETY: the pointer came from Box::into_raw in `adopt`, and was just detached, so it is freed once.
        drop(unsafe { Box::from_raw(window) });
    }
}

fn paint(hwnd: HWND, window: &Window) {
    let mut paint = PAINTSTRUCT::default();
    // SAFETY: WM_PAINT on our own window, ended below. Direct2D draws without the device context it returns.
    unsafe { BeginPaint(hwnd, &mut paint) };
    if let Err(e) = draw(hwnd, window) {
        log::failure("frozen screen: drawing the photo", &e);
    }
    // SAFETY: ends the BeginPaint above. EndPaint always returns TRUE.
    let _ = unsafe { EndPaint(hwnd, &paint) };
}

fn draw(hwnd: HWND, window: &Window) -> windows::core::Result<()> {
    let mut drawing = window
        .drawing
        .try_borrow_mut()
        .map_err(|_| Error::new(E_FAIL, "painting started again while painting"))?;
    let ready = match drawing.take() {
        Some(ready) => ready,
        None => Drawing::new(hwnd, window)?,
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
    // SAFETY: ends the BeginDraw above.
    match unsafe { ready.target.EndDraw(None, None) } {
        Ok(()) => {
            *drawing = Some(ready);
            Ok(())
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
    fn new(hwnd: HWND, window: &Window) -> windows::core::Result<Drawing> {
        let image = &window.photo.image;
        let pitch = image
            .width
            .checked_mul(4)
            .ok_or_else(|| Error::new(E_FAIL, "the photo is too wide"))?;
        // CreateBitmap below reads `height` rows of `pitch` bytes from the pixels.
        let length = usize::try_from(pitch)
            .ok()
            .zip(usize::try_from(image.height).ok())
            .and_then(|(pitch, height)| pitch.checked_mul(height));
        if length != Some(image.bgra.len()) {
            return Err(Error::new(
                E_FAIL,
                "the photo's pixels don't match its size",
            ));
        }
        let size = D2D_SIZE_U {
            width: image.width,
            height: image.height,
        };
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
            // The photo never moves, so there is no frame to wait for.
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
        // SAFETY: `image.bgra` holds exactly `height` rows of `pitch` bytes (checked above) and outlives the call,
        // which copies it.
        let bitmap = unsafe {
            target.CreateBitmap(
                size,
                Some(image.bgra.as_ptr().cast()),
                pitch,
                &bitmap_properties,
            )
        }?;
        Ok(Drawing { target, bitmap })
    }
}

/// Asks the resident window to close the tools.
fn request_close(resident: HWND) {
    // SAFETY: posts to the resident window, which outlives the frozen screen.
    if let Err(e) = unsafe { PostMessageW(Some(resident), CLOSE_REQUEST, WPARAM(0), LPARAM(0)) } {
        log::failure("frozen screen: asking to close", &e);
    }
}
