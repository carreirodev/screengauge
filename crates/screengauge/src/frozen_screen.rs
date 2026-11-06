//! The frozen screen on screen: one window over each monitor, at its exact place and size in physical pixels, drawing
//! the session core's scene. Each window draws its monitor's photo pixel for pixel with Direct2D, shows the cursor the
//! scene names, and hands the core what the user does over it and what Windows tells it: another program came to the
//! front, or the monitors changed. Over its monitor's photo, each window draws the measurements pinned there, what
//! Dimensions measures right now when the cursor is there, the toolbar, and the tolerance indicator and the notice when
//! they are there, and is drawn again whenever one of them changes. A click on the toolbar goes to the core as that
//! button's, never as a press of the tool, and a press on its background as one the core drags the toolbar by; the core
//! says where the toolbar is, and which of its buttons a point is on.
//! The first window also keeps the timer that wakes the session when the indicator or the notice is due to go. The
//! windows share the session; destroying them when the tools close, by dropping [`FrozenScreen`], drops the session and
//! erases the photos with it.

mod drawing;
mod input;
mod overlay;
mod shown;

use std::cell::{Cell, OnceCell, RefCell};
use std::ffi::c_void;
use std::rc::Rc;
use std::time::Duration;

use screengauge_core::session::toolbar::Hit;
use screengauge_core::session::{Cursor, Effect, Event, Input, Point, Rect, Session};
use windows::Win32::Foundation::{
    E_FAIL, ERROR_SUCCESS, GetLastError, HINSTANCE, HWND, LPARAM, LRESULT, SetLastError, WPARAM,
};
use windows::Win32::Graphics::Direct2D::ID2D1Factory;
use windows::Win32::Graphics::Dwm::{
    DWM_WINDOW_CORNER_PREFERENCE, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND,
    DwmSetWindowAttribute,
};
use windows::Win32::Graphics::Gdi::InvalidateRect;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    MOD_ALT, MOD_NOREPEAT, RegisterHotKey, UnregisterHotKey, VK_ESCAPE,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CREATESTRUCTW, CreateWindowExW, DefWindowProcW, DestroyWindow, GWLP_USERDATA,
    GetWindowLongPtrW, HWND_TOPMOST, IDC_ARROW, IDC_CROSS, KillTimer, LoadCursorW, PostMessageW,
    RegisterClassExW, SW_HIDE, SW_SHOWNA, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SetCursor,
    SetForegroundWindow, SetTimer, SetWindowLongPtrW, SetWindowPos, ShowWindow, USER_TIMER_MAXIMUM,
    USER_TIMER_MINIMUM, WM_CLOSE, WM_DISPLAYCHANGE, WM_ERASEBKGND, WM_NCCREATE, WM_NCDESTROY,
    WM_PAINT, WM_TIMER, WNDCLASSEXW, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::{Error, PCWSTR, w};

use crate::theme::Mode;
use crate::win32::struct_size;
use crate::{app_message, event, foreground, graphics, log};
use drawing::{Drawing, draw, paint};
use input::Pressed;
use shown::{Shown, monitors_to_draw_again};

const CLASS_NAME: PCWSTR = w!("ScreenGauge.FrozenScreen");

/// The first window's hot key while the tools are open: Alt+Esc. Windows acts on Alt+Esc itself, before any window gets
/// the key, and brings another program to the front, which closes the tools; held by the first window, it reaches the
/// session core as Esc instead, so Esc pressed with Alt held, as to cancel a drag Bounds draws exactly with Alt, cancels
/// it (seen in WPC-314; the owner's choice there). A hot key's ID belongs to its window, so this one can't clash with the
/// global shortcut's, which the resident window holds.
const ALT_ESC: i32 = 1;

/// The first window's timer that wakes the session core at the moment it asked for (`Effect::WakeAt`). Each window has
/// its own timers, so the number is apart from the resident window's, and the timer goes with the window.
const WAKE_TIMER: usize = 1;

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

/// The frozen screen's windows, one per monitor, and the session they show, while the tools are open.
pub struct FrozenScreen {
    windows: Vec<HWND>,
    session: Rc<SharedSession>,
    /// Tells the first window when another program comes to the front, from the moment the frozen screen asked for the
    /// keyboard. `None` before, and when Windows refused it.
    watch: Option<foreground::Watch>,
    /// Whether the first window holds Alt+Esc ([`ALT_ESC`]): from the moment the frozen screen asked for the keyboard,
    /// unless another program already held it.
    holds_alt_esc: Cell<bool>,
}

/// The session, as the windows and the resident window share it.
struct SharedSession {
    /// Borrowed mutably only while the core handles an event, which calls no Windows function, so no window procedure
    /// runs meanwhile.
    core: RefCell<Session>,
    /// What the core returned while handling the windows' events, for the resident window to carry out.
    effects: RefCell<Vec<Effect>>,
    /// The windows, in the monitors' order, so one can be drawn again when what it shows changes. Set once, when they
    /// are all made; an event that comes before that changes nothing a window has drawn yet.
    windows: OnceCell<Vec<HWND>>,
    /// Receives [`EFFECTS_READY`].
    resident: HWND,
}

impl FrozenScreen {
    /// Shows the session's scene, one window per monitor, with ScreenGauge's own surfaces in the palette of `mode`, and
    /// gives the keyboard to the window under the cursor so Esc reaches it. `resident` receives [`EFFECTS_READY`].
    pub fn show(
        session: Session,
        resident: HWND,
        mode: Mode,
    ) -> windows::core::Result<FrozenScreen> {
        // SAFETY: no module name, so this is the handle of ScreenGauge.exe itself.
        let instance: HINSTANCE = unsafe { GetModuleHandleW(None) }?.into();
        let factory = graphics::factory()?;
        let rects: Vec<Rect> = session
            .scene()
            .frozen_screen
            .iter()
            .map(|photo| photo.rect)
            .collect();
        // Should a window fail, dropping `frozen` destroys the ones already made.
        let mut frozen = FrozenScreen {
            windows: Vec::with_capacity(rects.len()),
            session: Rc::new(SharedSession {
                core: RefCell::new(session),
                effects: RefCell::new(Vec::new()),
                windows: OnceCell::new(),
                resident,
            }),
            watch: None,
            holds_alt_esc: Cell::new(false),
        };
        for (monitor, &rect) in rects.iter().enumerate() {
            let window = create_window(instance, monitor, rect, mode, &factory, &frozen.session)?;
            frozen.windows.push(window);
        }
        if frozen.session.windows.set(frozen.windows.clone()).is_err() {
            // Can't happen: this is the only place that sets it, once per session.
            log::failure("frozen screen", &"the windows were registered twice");
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
        frozen.take_keyboard();
        frozen.watch_the_front();
        frozen.hold_alt_esc();
        Ok(frozen)
    }

    /// Hands `event` to the session core and returns what it makes happen, with whatever the windows' events left for
    /// the resident window and it hasn't carried out yet, first.
    pub fn handle(&self, event: Event) -> Vec<Effect> {
        let mut effects = self.take_effects();
        effects.extend(self.session.handle(event));
        effects
    }

    /// Has the first window hand the session core [`Input::Wake`] once Windows has been running `uptime`, in place of
    /// any wake still waiting. Should Windows refuse the timer, what was due to go stays until the next event.
    pub fn wake_at(&self, uptime: Duration) {
        // Can't happen: the session core refuses a frozen screen without monitors.
        let Some(&window) = self.windows.first() else {
            return;
        };
        let wait = uptime.saturating_sub(event::uptime());
        // Windows waits at least USER_TIMER_MINIMUM, and a wait past USER_TIMER_MAXIMUM, about 25 days, is clamped to it.
        let milliseconds = u32::try_from(wait.as_millis())
            .unwrap_or(USER_TIMER_MAXIMUM)
            .clamp(USER_TIMER_MINIMUM, USER_TIMER_MAXIMUM);
        // SAFETY: our own window, on its thread. A timer with the same number is replaced.
        if unsafe { SetTimer(Some(window), WAKE_TIMER, milliseconds, None) } == 0 {
            log::failure("frozen screen: waking the session", &Error::from_thread());
        }
    }

    /// What the windows' events left for the resident window since it last took them.
    pub fn take_effects(&self) -> Vec<Effect> {
        match self.session.effects.try_borrow_mut() {
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
    /// on. Each call grants ScreenGauge the right to come to the front: the global shortcut and the tray icon's click
    /// through Windows, and opening ScreenGauge through the program just opened; should Windows refuse anyway, Esc
    /// won't reach the frozen screen until it is clicked.
    fn take_keyboard(&self) {
        let under_cursor = match event::cursor_now() {
            Ok(cursor) => self.window_at(cursor),
            Err(e) => {
                log::failure("frozen screen: finding the cursor", &e);
                self.windows.first().copied()
            }
        };
        if let Some(window) = under_cursor {
            // SAFETY: our own window.
            if !unsafe { SetForegroundWindow(window) }.as_bool() {
                log::failure(
                    "frozen screen",
                    &"Windows didn't bring it to the front, so Esc won't reach it",
                );
            }
        }
        // The cursor shows at once, without waiting for the mouse to move.
        self.session.show_cursor();
    }

    /// From here on, another program coming to the front closes the tools. Watched only once the frozen screen asked
    /// for the keyboard, so nothing on the way here counts, such as the Start menu closing or the click on the tray
    /// icon. When Windows kept the front with the program the user was in, that program bringing another of its windows
    /// to the front closes the tools too. Should Windows refuse the watch, the tools still close with Esc and the
    /// global shortcut, so the call goes on.
    fn watch_the_front(&mut self) {
        // Can't happen: the session core refuses a frozen screen without monitors.
        let Some(&window) = self.windows.first() else {
            return;
        };
        match foreground::Watch::start(window) {
            Ok(watch) => self.watch = Some(watch),
            Err(e) => log::failure("frozen screen: watching for another program in front", &e),
        }
    }

    /// Has the first window hold Alt+Esc while the tools are open ([`ALT_ESC`]). Should another program hold it already,
    /// Alt+Esc does what Windows does with it and closes the tools, as Alt+Tab does, and the call goes on.
    fn hold_alt_esc(&self) {
        // Can't happen: the session core refuses a frozen screen without monitors.
        let Some(&window) = self.windows.first() else {
            return;
        };
        // With MOD_NOREPEAT, Alt+Esc held down comes once.
        // SAFETY: our own window, on the thread that created it.
        match unsafe {
            RegisterHotKey(
                Some(window),
                ALT_ESC,
                MOD_ALT | MOD_NOREPEAT,
                u32::from(VK_ESCAPE.0),
            )
        } {
            Ok(()) => self.holds_alt_esc.set(true),
            Err(e) => log::failure("frozen screen: holding Alt+Esc", &e),
        }
    }

    /// Gives Alt+Esc back to Windows, when the first window holds it.
    fn give_back_alt_esc(&self) {
        if !self.holds_alt_esc.get() {
            return;
        }
        let Some(&window) = self.windows.first() else {
            return;
        };
        // SAFETY: our own window, on its thread, which holds the hot key.
        match unsafe { UnregisterHotKey(Some(window), ALT_ESC) } {
            Ok(()) => self.holds_alt_esc.set(false),
            Err(e) => log::failure("frozen screen: giving back Alt+Esc", &e),
        }
    }

    /// The window over the monitor at `at`, or, should `at` be on none, the first one.
    pub fn window_at(&self, at: Point) -> Option<HWND> {
        let monitor = match self.session.core.try_borrow() {
            Ok(session) => session
                .scene()
                .frozen_screen
                .iter()
                .position(|photo| photo.rect.contains(at))
                .unwrap_or(0),
            // Can't happen: this is read outside the moment the core handles an event.
            Err(_) => {
                log::failure(
                    "frozen screen",
                    &"a window was asked for while an event was handled",
                );
                0
            }
        };
        self.windows.get(monitor).copied()
    }

    /// Draws ScreenGauge's own surfaces over the photo in `mode`'s palette from now on: the toolbar, the notices and the
    /// pinned measurements' labels. A window in the other palette makes what it draws with again at its next paint, which
    /// only the paint itself does, as it holds the drawing.
    pub fn follow(&self, mode: Mode) {
        for &window in &self.windows {
            let changed = with_state(window, |state| state.mode.replace(mode) != mode);
            if changed == Some(true) {
                // SAFETY: our own window.
                if !unsafe { InvalidateRect(Some(window), None, false) }.as_bool() {
                    log::failure(
                        "frozen screen: drawing it in another theme",
                        &"InvalidateRect returned FALSE",
                    );
                }
            }
        }
    }

    /// Hides the windows, so the program in front and the ones behind it show, while Save As waits behind it (the
    /// owner's choice in WPC-318). The session and its photos stay as they were.
    pub fn step_aside(&self) {
        // The program in front has Alt+Esc as Windows gives it.
        self.give_back_alt_esc();
        for &window in &self.windows {
            // SAFETY: our own window. ShowWindow returns whether it was visible before, not an error.
            let _ = unsafe { ShowWindow(window, SW_HIDE) };
        }
    }

    /// Shows the windows again over every program, as they were before they stepped aside. Save As comes back over
    /// them afterwards.
    pub fn come_back(&self) {
        self.hold_alt_esc();
        for &window in &self.windows {
            // SAFETY: our own window. ShowWindow returns whether it was visible before, not an error.
            let _ = unsafe { ShowWindow(window, SW_SHOWNA) };
            // Hiding kept them topmost; this puts them over whatever came to the front meanwhile.
            // SAFETY: our own window, on its thread.
            if let Err(e) = unsafe {
                SetWindowPos(
                    window,
                    Some(HWND_TOPMOST),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                )
            } {
                log::failure("frozen screen: coming back", &e);
            }
        }
    }
}

impl Drop for FrozenScreen {
    fn drop(&mut self) {
        // The watch goes before the window it tells: closing the window in front hands the front to another program,
        // which isn't one coming to the front while the tools are open.
        self.watch = None;
        // Alt+Esc is Windows' again once the tools close.
        self.give_back_alt_esc();
        for window in self.windows.drain(..) {
            destroy(window);
        }
    }
}

impl SharedSession {
    /// Hands `event` to the session core, draws again the windows whose part of the scene it changed, and shows the
    /// cursor the scene names when that changed. Here rather than in `input`, so an event that comes from the resident
    /// window redraws too. Windows asks for the cursor's shape before it reports the move that brought the cursor onto
    /// the toolbar or off it, so the shape is set again here, once the core knows where the cursor is.
    fn handle(&self, event: Event) -> Vec<Effect> {
        let before = self.shown();
        let effects = match self.core.try_borrow_mut() {
            Ok(mut session) => session.handle(event),
            // Can't happen: the core calls no Windows function, so no event arrives while it handles one.
            Err(_) => {
                log::failure(
                    "frozen screen",
                    &"an event came while the session core was handling another, and was dropped",
                );
                Vec::new()
            }
        };
        let after = self.shown();
        // Moving along the same row often measures the same span, and then nothing is drawn again.
        self.redraw(&monitors_to_draw_again(&before, &after));
        if before.cursor != after.cursor {
            self.show_cursor();
        }
        effects
    }

    /// What the windows draw over their photos right now, as a copy to compare with a later one.
    fn shown(&self) -> Shown {
        match self.core.try_borrow() {
            Ok(session) => {
                let scene = session.scene();
                Shown {
                    live: scene.live.cloned(),
                    drawn_rectangle: scene.drawn_rectangle,
                    snip_area: scene.snip_area,
                    pinned: scene.pinned.to_vec(),
                    magnified_view: scene.magnified_view,
                    tolerance_indicator: scene.tolerance_indicator,
                    notice: scene.notice.copied(),
                    toolbar: Some(scene.toolbar),
                    cursor: Some(scene.cursor),
                }
            }
            // Can't happen: this is read outside the moment the core handles an event.
            Err(_) => {
                log::failure(
                    "frozen screen",
                    &"the scene was asked for while an event was handled",
                );
                Shown::default()
            }
        }
    }

    /// Marks the windows of `monitors` for drawing.
    fn redraw(&self, monitors: &[usize]) {
        let Some(windows) = self.windows.get() else {
            return;
        };
        for &monitor in monitors {
            let Some(&window) = windows.get(monitor) else {
                // Can't happen: the core draws only on monitors of the frozen screen.
                log::failure(
                    "frozen screen",
                    &"the scene has something on a monitor with no window",
                );
                continue;
            };
            redraw_window(window);
        }
    }

    /// What `at` is on in the toolbar as it is now, as the core says: `None` off it. Minimized, the toolbar is its small
    /// button alone; whole, H, V and C count only while they show.
    fn toolbar_hit(&self, at: Point) -> Option<Hit> {
        match self.core.try_borrow() {
            Ok(session) => session.toolbar_hit(at),
            // Can't happen: this is read outside the moment the core handles an event.
            Err(_) => {
                log::failure(
                    "frozen screen",
                    &"the toolbar was asked for while an event was handled",
                );
                None
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
        let posted = unsafe {
            PostMessageW(
                Some(self.resident),
                app_message::EFFECTS_READY,
                WPARAM(0),
                LPARAM(0),
            )
        };
        if let Err(e) = posted {
            log::failure("frozen screen: handing effects to the resident window", &e);
        }
    }

    /// Shows the scene's cursor.
    fn show_cursor(&self) {
        let Ok(session) = self.core.try_borrow() else {
            log::failure(
                "frozen screen",
                &"the cursor was asked for while an event was handled",
            );
            return;
        };
        let name = match session.scene().cursor {
            Cursor::Cross => IDC_CROSS,
            Cursor::Arrow => IDC_ARROW,
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

/// Marks `window` for drawing.
fn redraw_window(window: HWND) {
    // SAFETY: our own window. WM_ERASEBKGND is handled, so nothing is erased first.
    if !unsafe { InvalidateRect(Some(window), None, false) }.as_bool() {
        log::failure(
            "frozen screen: drawing a window again",
            &"InvalidateRect returned FALSE",
        );
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
    /// Its monitor's index in the scene's frozen screen.
    monitor: usize,
    /// The palette of what ScreenGauge draws itself over the photo. It changes with the theme while the tools are open.
    mode: Cell<Mode>,
    session: Rc<SharedSession>,
    factory: ID2D1Factory,
    /// Made at the first draw, and again when Direct2D says the graphics device was lost.
    drawing: RefCell<Option<Drawing>>,
    /// What the left button went down on, while it is held: the window has the mouse until it comes up, wherever that
    /// is, so the release always reaches the core.
    pressed: Cell<Option<Pressed>>,
}

/// What CreateWindowExW hands to WM_NCCREATE. Should WM_NCCREATE never come, the window state stays here and drops
/// with it.
type Slot = Option<Box<Window>>;

fn create_window(
    instance: HINSTANCE,
    monitor: usize,
    rect: Rect,
    mode: Mode,
    factory: &ID2D1Factory,
    session: &Rc<SharedSession>,
) -> windows::core::Result<HWND> {
    let too_large = || Error::new(E_FAIL, "the monitor is too large for a window");
    let width = i32::try_from(rect.width).map_err(|_| too_large())?;
    let height = i32::try_from(rect.height).map_err(|_| too_large())?;
    let mut slot: Slot = Some(Box::new(Window {
        monitor,
        mode: Cell::new(mode),
        session: Rc::clone(session),
        factory: factory.clone(),
        drawing: RefCell::new(None),
        pressed: Cell::new(None),
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
        // The mouse, the wheel and the keys go to the core through `input`, which knows which messages they are. One it
        // leaves to Windows' default, as a system key, comes on to the match below, which has no arm for it either.
        if let Some(result) = input::route(hwnd, window, message, wparam, lparam) {
            return Some(result);
        }
        let deliver = |input: Input| {
            window.session.input(input);
            Some(LRESULT(0))
        };
        match message {
            WM_ERASEBKGND => Some(LRESULT(1)),
            WM_PAINT => {
                paint(hwnd, window);
                Some(LRESULT(0))
            }
            // Alt+F4: the core closes the tools as a whole, never one monitor's window alone.
            WM_CLOSE => deliver(Input::CloseAsked),
            app_message::OTHER_PROGRAM_IN_FRONT => deliver(Input::OtherProgramInFront),
            app_message::OWN_WINDOW_IN_FRONT => deliver(Input::BackInFront),
            // Windows sends this to every top-level window for any change of monitors: one connected or disconnected,
            // a scale, resolution or arrangement changed, HDR turned on or off (seen in WPC-281 and WPC-289). Moving
            // the SDR content brightness sends nothing. It comes from another thread, so it may arrive inside any
            // Windows call that waits on one, even while a window draws with the photo borrowed; posted on, it reaches
            // the core from the message loop. Each window passes it on; the first closes the tools, and the core takes
            // nothing after.
            WM_DISPLAYCHANGE => {
                // SAFETY: our own window; the message carries nothing.
                let posted = unsafe {
                    PostMessageW(
                        Some(hwnd),
                        app_message::MONITORS_CHANGED,
                        WPARAM(0),
                        LPARAM(0),
                    )
                };
                if let Err(e) = posted {
                    log::failure("frozen screen: passing on a change of monitors", &e);
                }
                Some(LRESULT(0))
            }
            app_message::MONITORS_CHANGED => deliver(Input::MonitorsChanged),
            WM_TIMER if wparam.0 == WAKE_TIMER => {
                // Once: the session asks again when it needs another wake.
                // SAFETY: our own window's timer.
                if let Err(e) = unsafe { KillTimer(Some(hwnd), WAKE_TIMER) } {
                    log::failure("frozen screen: stopping the wake timer", &e);
                }
                deliver(Input::Wake)
            }
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
