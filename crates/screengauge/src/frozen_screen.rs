//! The frozen screen on screen: one window over each monitor, at its exact place and size in physical pixels, drawing
//! the session core's scene. Each window draws its monitor's photo pixel for pixel with Direct2D, shows the cursor the
//! scene names, and hands the core what the user does over it. The windows share the session; destroying them when the
//! tools close, by dropping [`FrozenScreen`], drops the session and erases the photos with it.

use std::cell::RefCell;
use std::ffi::c_void;
use std::rc::Rc;

use screengauge_core::session::{Cursor, Effect, Event, Input, Rect, Session};
use windows::Win32::Foundation::{
    D2DERR_RECREATE_TARGET, E_FAIL, ERROR_SUCCESS, GetLastError, HINSTANCE, HWND, LPARAM, LRESULT,
    POINT, SetLastError, WPARAM,
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
use windows::Win32::UI::WindowsAndMessaging::{
    CREATESTRUCTW, CreateWindowExW, DefWindowProcW, DestroyWindow, GWLP_USERDATA, GetCursorPos,
    GetWindowLongPtrW, HTCLIENT, IDC_CROSS, LoadCursorW, PostMessageW, RegisterClassExW, SW_SHOWNA,
    SetCursor, SetForegroundWindow, SetWindowLongPtrW, ShowWindow, WM_APP, WM_CLOSE, WM_ERASEBKGND,
    WM_KEYDOWN, WM_KEYUP, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_NCCREATE,
    WM_NCDESTROY, WM_PAINT, WM_SETCURSOR, WM_SYSKEYDOWN, WM_SYSKEYUP, WNDCLASSEXW,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::{Error, PCWSTR, w};

use crate::win32::{low_word, struct_size};
use crate::{event, log};

const CLASS_NAME: PCWSTR = w!("ScreenGauge.FrozenScreen");

/// What a frozen screen window posts to the resident window when the session core returned effects for it to carry
/// out. Posted, never sent: closing the tools destroys these windows, which must not happen inside one of their own
/// window procedures.
pub const EFFECTS_READY: u32 = WM_APP + 2;

/// Registered once at startup. The class has no cursor: the session core's scene names it.
pub fn register_class(instance: HINSTANCE) -> windows::core::Result<()> {
    let class = WNDCLASSEXW {
        cbSize: const { struct_size::<WNDCLASSEXW>() },
        lpfnWndProc: Some(window_proc),
        hInstance: instance,
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
    static FACTORY: RefCell<Option<ID2D1Factory>> = const { RefCell::new(None) };
}

fn factory() -> windows::core::Result<ID2D1Factory> {
    FACTORY.with_borrow_mut(|kept| {
        if let Some(factory) = kept {
            return Ok(factory.clone());
        }
        // SAFETY: a single-threaded factory with default options, used only on this thread.
        let factory: ID2D1Factory =
            unsafe { D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None) }?;
        Ok(kept.insert(factory).clone())
    })
}

/// Releases the kept factory, and the graphics device with it, as ScreenGauge exits. Left to the end of the process,
/// the release happened while Windows was already taking it down: the exit took 3 seconds and failed.
pub fn release_factory() {
    drop(FACTORY.with_borrow_mut(Option::take));
}

/// The frozen screen's windows, one per monitor, and the session they show, while the tools are open.
pub struct FrozenScreen {
    windows: Vec<HWND>,
    shared: Rc<Shared>,
}

/// What the windows share.
struct Shared {
    /// Borrowed mutably only while the core handles an event, which calls no Windows function, so no window procedure
    /// runs meanwhile.
    session: RefCell<Session>,
    /// What the core returned while handling the windows' events, for the resident window to carry out.
    effects: RefCell<Vec<Effect>>,
    /// Receives [`EFFECTS_READY`].
    resident: HWND,
}

impl FrozenScreen {
    /// Shows the session's scene, one window per monitor, and gives the keyboard to the window under the cursor so
    /// Esc reaches it. `resident` receives [`EFFECTS_READY`].
    pub fn show(session: Session, resident: HWND) -> windows::core::Result<FrozenScreen> {
        // SAFETY: no module name, so this is the handle of ScreenGauge.exe itself.
        let instance: HINSTANCE = unsafe { GetModuleHandleW(None) }?.into();
        let factory = factory()?;
        let rects: Vec<Rect> = session
            .scene()
            .frozen_screen
            .iter()
            .map(|photo| photo.rect)
            .collect();
        // Should a window fail, dropping `frozen` destroys the ones already made.
        let mut frozen = FrozenScreen {
            windows: Vec::with_capacity(rects.len()),
            shared: Rc::new(Shared {
                session: RefCell::new(session),
                effects: RefCell::new(Vec::new()),
                resident,
            }),
        };
        for (monitor, &rect) in rects.iter().enumerate() {
            let window = create_window(instance, monitor, rect, &factory, &frozen.shared)?;
            frozen.windows.push(window);
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

    /// Hands `event` to the session core and returns what it makes happen, with whatever the windows' events left for
    /// the resident window and it hasn't carried out yet, first.
    pub fn handle(&self, event: Event) -> Vec<Effect> {
        let mut effects = self.take_effects();
        effects.extend(self.shared.handle(event));
        effects
    }

    /// What the windows' events left for the resident window since it last took them.
    pub fn take_effects(&self) -> Vec<Effect> {
        match self.shared.effects.try_borrow_mut() {
            Ok(mut queued) => std::mem::take(&mut *queued),
            Err(_) => {
                log::failure(
                    "frozen screen",
                    &"the effects were being queued while being taken",
                );
                Vec::new()
            }
        }
    }

    /// Brings the window under the cursor to the front, so the keyboard goes to it, and shows the scene's cursor. The
    /// photo is already on screen and closes with the global shortcut, so a failure here is logged and the call goes
    /// on. The global shortcut grants ScreenGauge the right to come to the front; should Windows refuse anyway, Esc
    /// won't reach the frozen screen until it is clicked.
    fn take_keyboard(&self, rects: &[Rect]) {
        let mut cursor = POINT::default();
        // SAFETY: fills a local.
        let under_cursor = match unsafe { GetCursorPos(&mut cursor) } {
            Ok(()) => rects.iter().position(|&rect| contains(rect, cursor)),
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
        // The cursor shows at once, without waiting for the mouse to move.
        self.shared.show_cursor();
    }
}

impl Drop for FrozenScreen {
    fn drop(&mut self) {
        for window in self.windows.drain(..) {
            destroy(window);
        }
    }
}

impl Shared {
    fn handle(&self, event: Event) -> Vec<Effect> {
        match self.session.try_borrow_mut() {
            Ok(mut session) => session.handle(event),
            // Can't happen: the core calls no Windows function, so no event arrives while it handles one.
            Err(_) => {
                log::failure(
                    "frozen screen",
                    &"an event came while the session core was handling another, and was dropped",
                );
                Vec::new()
            }
        }
    }

    /// Hands what happened over a window to the session core, and passes what it returns to the resident window.
    fn input(&self, input: Input) {
        let effects = self.handle(event::of_message(input));
        if effects.is_empty() {
            return;
        }
        match self.effects.try_borrow_mut() {
            Ok(mut queued) => queued.extend(effects),
            // Can't happen: the queue is borrowed only for a moment, with no Windows call.
            Err(_) => {
                log::failure(
                    "frozen screen",
                    &"the effects were being taken while being queued, and were dropped",
                );
                return;
            }
        }
        // Should the message be lost, the resident window still takes the effects with the next global shortcut.
        // SAFETY: posts to the resident window, which outlives the frozen screen.
        let posted =
            unsafe { PostMessageW(Some(self.resident), EFFECTS_READY, WPARAM(0), LPARAM(0)) };
        if let Err(e) = posted {
            log::failure("frozen screen: handing effects to the resident window", &e);
        }
    }

    /// Shows the scene's cursor.
    fn show_cursor(&self) {
        let Ok(session) = self.session.try_borrow() else {
            log::failure(
                "frozen screen",
                &"the cursor was asked for while an event was handled",
            );
            return;
        };
        let name = match session.scene().cursor {
            Cursor::Cross => IDC_CROSS,
        };
        // SAFETY: loads a shared system cursor, which is never freed.
        match unsafe { LoadCursorW(None, name) } {
            Ok(cursor) => {
                // SAFETY: a valid cursor. SetCursor returns the previous one, which needs nothing.
                let _ = unsafe { SetCursor(Some(cursor)) };
            }
            Err(e) => log::failure("frozen screen: showing the cursor", &e),
        }
    }
}

/// Whether `point`, in physical pixels of the virtual screen, is in `rect`.
fn contains(rect: Rect, point: POINT) -> bool {
    let (x, y) = (i64::from(point.x), i64::from(point.y));
    let (left, top) = (i64::from(rect.left), i64::from(rect.top));
    (left..left + i64::from(rect.width)).contains(&x)
        && (top..top + i64::from(rect.height)).contains(&y)
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
    /// Its monitor's place in the scene's frozen screen.
    monitor: usize,
    shared: Rc<Shared>,
    factory: ID2D1Factory,
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
    monitor: usize,
    rect: Rect,
    factory: &ID2D1Factory,
    shared: &Rc<Shared>,
) -> windows::core::Result<HWND> {
    let too_large = || Error::new(E_FAIL, "the monitor is too large for a window");
    let width = i32::try_from(rect.width).map_err(|_| too_large())?;
    let height = i32::try_from(rect.height).map_err(|_| too_large())?;
    let mut slot: Slot = Some(Box::new(Window {
        monitor,
        shared: Rc::clone(shared),
        factory: factory.clone(),
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
            width,
            height,
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
    let handled = with_state(hwnd, |window| {
        let input = |input: Input| {
            window.shared.input(input);
            Some(LRESULT(0))
        };
        match message {
            WM_ERASEBKGND => Some(LRESULT(1)),
            WM_PAINT => {
                paint(hwnd, window);
                Some(LRESULT(0))
            }
            WM_SETCURSOR if low_word(lparam) == HTCLIENT => {
                window.shared.show_cursor();
                Some(LRESULT(1))
            }
            WM_MOUSEMOVE => input(Input::Move),
            WM_LBUTTONDOWN => input(Input::Press),
            WM_LBUTTONUP => input(Input::Release),
            WM_MOUSEWHEEL => input(Input::Wheel(event::wheel(wparam))),
            WM_KEYDOWN => event::key(wparam).and_then(|key| input(Input::KeyDown(key))),
            WM_KEYUP => event::key(wparam).and_then(|key| input(Input::KeyUp(key))),
            // Alt, and keys pressed with it, come as system keys. Windows handles them afterwards as before, so
            // Alt+F4 still turns into WM_CLOSE.
            WM_SYSKEYDOWN | WM_SYSKEYUP => {
                if let Some(key) = event::key(wparam) {
                    window.shared.input(if message == WM_SYSKEYDOWN {
                        Input::KeyDown(key)
                    } else {
                        Input::KeyUp(key)
                    });
                }
                None
            }
            // Alt+F4: the core closes the tools as a whole, never one monitor's window alone.
            WM_CLOSE => input(Input::CloseAsked),
            _ => None,
        }
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

/// Frees the window state, and the window's share of the session with it.
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

/// Draws the window's part of the scene: its monitor's photo, with nothing over it yet.
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
    /// Puts the window's photo, from the scene, on the graphics card.
    fn new(hwnd: HWND, window: &Window) -> windows::core::Result<Drawing> {
        let session = window
            .shared
            .session
            .try_borrow()
            .map_err(|_| Error::new(E_FAIL, "drawing started while an event was handled"))?;
        let scene = session.scene();
        let image = &scene
            .frozen_screen
            .get(window.monitor)
            .ok_or_else(|| Error::new(E_FAIL, "the scene has no photo for this window"))?
            .image;
        let pitch = image
            .width()
            .checked_mul(4)
            .ok_or_else(|| Error::new(E_FAIL, "the photo is too wide"))?;
        let size = D2D_SIZE_U {
            width: image.width(),
            height: image.height(),
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
        // SAFETY: an Image holds exactly `height` rows of `pitch` bytes, and outlives the call, which copies them.
        let bitmap = unsafe {
            target.CreateBitmap(
                size,
                Some(image.bgra().as_ptr().cast()),
                pitch,
                &bitmap_properties,
            )
        }?;
        Ok(Drawing { target, bitmap })
    }
}
