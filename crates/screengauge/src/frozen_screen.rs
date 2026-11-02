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

mod overlay;

use std::cell::{Cell, OnceCell, RefCell};
use std::collections::BTreeSet;
use std::ffi::c_void;
use std::rc::Rc;
use std::time::Duration;

use screengauge_core::session::toolbar::Hit;
use screengauge_core::session::{
    Cursor, DrawnRectangle, Effect, Event, Input, Key, MagnifiedView, Measurement, Notice, Photo,
    Point, Rect, Session, SnipArea, ToleranceIndicator, Toolbar,
};
use windows::Win32::Foundation::{
    D2DERR_RECREATE_TARGET, E_FAIL, ERROR_SUCCESS, GetLastError, HINSTANCE, HWND, LPARAM, LRESULT,
    SetLastError, WPARAM,
};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_SIZE_U, D2D1_ALPHA_MODE_IGNORE, D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_BITMAP_INTERPOLATION_MODE_NEAREST_NEIGHBOR, D2D1_BITMAP_PROPERTIES,
    D2D1_FEATURE_LEVEL_DEFAULT, D2D1_HWND_RENDER_TARGET_PROPERTIES,
    D2D1_PRESENT_OPTIONS_IMMEDIATELY, D2D1_RENDER_TARGET_PROPERTIES,
    D2D1_RENDER_TARGET_TYPE_DEFAULT, D2D1_RENDER_TARGET_USAGE_NONE, ID2D1Bitmap, ID2D1Factory,
    ID2D1HwndRenderTarget,
};
use windows::Win32::Graphics::Dwm::{
    DWM_WINDOW_CORNER_PREFERENCE, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND,
    DwmSetWindowAttribute,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::{BeginPaint, EndPaint, InvalidateRect, PAINTSTRUCT};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    MOD_ALT, MOD_NOREPEAT, RegisterHotKey, ReleaseCapture, SetCapture, UnregisterHotKey, VK_ESCAPE,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CREATESTRUCTW, CreateWindowExW, DefWindowProcW, DestroyWindow, GWLP_USERDATA,
    GetWindowLongPtrW, HTCLIENT, HWND_TOPMOST, IDC_ARROW, IDC_CROSS, KillTimer, LoadCursorW,
    PostMessageW, RegisterClassExW, SW_HIDE, SW_SHOWNA, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    SetCursor, SetForegroundWindow, SetTimer, SetWindowLongPtrW, SetWindowPos, ShowWindow,
    USER_TIMER_MAXIMUM, USER_TIMER_MINIMUM, WM_APP, WM_CAPTURECHANGED, WM_CLOSE, WM_DISPLAYCHANGE,
    WM_ERASEBKGND, WM_HOTKEY, WM_KEYDOWN, WM_KEYUP, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE,
    WM_MOUSEWHEEL, WM_NCCREATE, WM_NCDESTROY, WM_PAINT, WM_SETCURSOR, WM_SYSKEYDOWN, WM_SYSKEYUP,
    WM_TIMER, WNDCLASSEXW, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::{Error, PCWSTR, w};

use crate::theme::Mode;
use crate::win32::{low_word, struct_size};
use crate::{event, foreground, graphics, log};
use overlay::Overlay;

const CLASS_NAME: PCWSTR = w!("ScreenGauge.FrozenScreen");

/// What a frozen screen window posts to the resident window when the session core returned effects for it to carry
/// out. Posted, never sent: closing the tools destroys these windows, which must not happen inside one of their own
/// window procedures.
pub const EFFECTS_READY: u32 = WM_APP + 2;

/// What a frozen screen window posts to itself when Windows says the monitors changed. WM_APP + 4 is taken by the
/// notifications waiting to show (`notification::SHOW_WAITING`).
const MONITORS_CHANGED: u32 = WM_APP + 5;

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
        let posted =
            unsafe { PostMessageW(Some(self.resident), EFFECTS_READY, WPARAM(0), LPARAM(0)) };
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

/// What the windows draw over their photos: the scene without the photos, copied.
#[derive(Debug, Clone, PartialEq, Default)]
struct Shown {
    live: Option<Measurement>,
    drawn_rectangle: Option<DrawnRectangle>,
    snip_area: Option<SnipArea>,
    pinned: Vec<Measurement>,
    magnified_view: Option<MagnifiedView>,
    tolerance_indicator: Option<ToleranceIndicator>,
    notice: Option<Notice>,
    /// `None` only when the scene couldn't be read.
    toolbar: Option<Toolbar>,
    /// The cursor the scene names, which no window draws: Windows shows it. `None` only when the scene couldn't be read.
    cursor: Option<Cursor>,
}

/// One window's part of what is shown: what is on its monitor.
#[derive(Debug, PartialEq)]
struct OnMonitor<'a> {
    /// Oldest first.
    pinned: Vec<&'a Measurement>,
    live: Option<&'a Measurement>,
    drawn_rectangle: Option<&'a DrawnRectangle>,
    snip_area: Option<&'a SnipArea>,
    magnified_view: Option<&'a MagnifiedView>,
    tolerance_indicator: Option<&'a ToleranceIndicator>,
    notice: Option<&'a Notice>,
    toolbar: Option<&'a Toolbar>,
}

impl Shown {
    fn on(&self, monitor: usize) -> OnMonitor<'_> {
        OnMonitor {
            pinned: self
                .pinned
                .iter()
                .filter(|measurement| measurement.monitor == monitor)
                .collect(),
            live: self
                .live
                .as_ref()
                .filter(|measurement| measurement.monitor == monitor),
            drawn_rectangle: self
                .drawn_rectangle
                .as_ref()
                .filter(|drawn| drawn.monitor == monitor),
            snip_area: self
                .snip_area
                .as_ref()
                .filter(|area| area.monitor == monitor),
            magnified_view: self
                .magnified_view
                .as_ref()
                .filter(|view| view.monitor == monitor),
            tolerance_indicator: self
                .tolerance_indicator
                .as_ref()
                .filter(|indicator| indicator.monitor == monitor),
            notice: self
                .notice
                .as_ref()
                .filter(|notice| notice.monitor == monitor),
            toolbar: self
                .toolbar
                .as_ref()
                .filter(|toolbar| toolbar.monitor == monitor),
        }
    }

    /// Every monitor something is drawn on.
    fn monitors(&self) -> impl Iterator<Item = usize> + '_ {
        let measured = self
            .pinned
            .iter()
            .chain(&self.live)
            .map(|measurement| measurement.monitor)
            .chain(self.drawn_rectangle.iter().map(|drawn| drawn.monitor))
            .chain(self.snip_area.iter().map(|area| area.monitor));
        let magnified = self.magnified_view.iter().map(|view| view.monitor);
        let indicated = self
            .tolerance_indicator
            .iter()
            .map(|indicator| indicator.monitor);
        measured
            .chain(magnified)
            .chain(indicated)
            .chain(self.notice.iter().map(|notice| notice.monitor))
            .chain(self.toolbar.iter().map(|toolbar| toolbar.monitor))
    }
}

/// The monitors whose windows show something different from `before` to `after`, each once and in order: the one a
/// measurement left, which must lose its line, the one it is on now, the one a measurement was pinned on or taken off,
/// the one Bounds or Snip draws a rectangle on while it appears, changes or goes, the one Snip's area appeared on,
/// changed on or went from, the ones Inspect's magnified view appeared on,
/// changed on, followed the cursor between or went from, the ones the tolerance indicator appeared on,
/// followed the cursor between or went from, the one a notice appeared on or went from, and the toolbar's when another
/// of its buttons lights up or it moves.
fn monitors_to_draw_again(before: &Shown, after: &Shown) -> Vec<usize> {
    let monitors: BTreeSet<usize> = before.monitors().chain(after.monitors()).collect();
    monitors
        .into_iter()
        .filter(|&monitor| before.on(monitor) != after.on(monitor))
        .collect()
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
    /// The palette of what ScreenGauge draws itself over the photo.
    mode: Mode,
    session: Rc<SharedSession>,
    factory: ID2D1Factory,
    /// Made at the first draw, and again when Direct2D says the graphics device was lost.
    drawing: RefCell<Option<Drawing>>,
    /// What the left button went down on, while it is held: the window has the mouse until it comes up, wherever that
    /// is, so the release always reaches the core.
    pressed: Cell<Option<Pressed>>,
}

/// What the left button went down on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pressed {
    /// Something on the toolbar. A button acts only when the mouse comes up on the same button (WPC-310), and looks
    /// pressed meanwhile while the cursor is on it (WPC-311). Held on the background, the toolbar is being dragged, and
    /// the mouse coming up lets it go (WPC-312).
    Toolbar(Hit),
    /// The tool, off the toolbar: a click in Dimensions, or the corner a rectangle in Bounds is drawn from, whose drag
    /// ends where the mouse comes up, on this monitor or another (WPC-313).
    Tool,
}

impl Pressed {
    /// What on the toolbar is held, which looks pressed while the cursor is on it.
    fn on_toolbar(self) -> Option<Hit> {
        match self {
            Pressed::Toolbar(hit) => Some(hit),
            Pressed::Tool => None,
        }
    }
}

/// The photo on the graphics card, ready to draw, with what draws the measurements and the notice over it. They belong
/// to the render target, so they are made again together when it is lost.
struct Drawing {
    target: ID2D1HwndRenderTarget,
    bitmap: ID2D1Bitmap,
    overlay: Overlay,
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
        mode,
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
            WM_SETCURSOR if low_word(lparam) == HTCLIENT => {
                window.session.show_cursor();
                Some(LRESULT(1))
            }
            WM_MOUSEMOVE => deliver(Input::Move),
            // A press anywhere on the toolbar is the toolbar's: it never reaches the tool, which would pin and copy what
            // lies under the toolbar. Wherever the press is, the window keeps the mouse until the button comes up, so a
            // drag of the toolbar, or of a rectangle in Bounds, goes on over the other monitors' windows and its release
            // comes here.
            WM_LBUTTONDOWN => {
                let hit = window.session.toolbar_hit(event::message_cursor());
                window
                    .pressed
                    .set(Some(hit.map_or(Pressed::Tool, Pressed::Toolbar)));
                // SAFETY: our own window, on its thread, with the left button down. It returns the window that had the
                // mouse before, which needs nothing.
                let _ = unsafe { SetCapture(hwnd) };
                match hit {
                    Some(hit) => {
                        // The button under the cursor looks pressed.
                        redraw_window(hwnd);
                        match hit {
                            // On the background, the press picks the toolbar up, and the core drags it with the cursor.
                            Hit::Background => deliver(Input::Press),
                            Hit::Button(_) => Some(LRESULT(0)),
                        }
                    }
                    None => deliver(Input::Press),
                }
            }
            // A toolbar button acts when the mouse comes up on it, as a Windows button does, so moving off it first
            // cancels. Neither the press nor its release reaches the tool.
            // `pressed` is taken before the mouse is given back: ReleaseCapture sends WM_CAPTURECHANGED to this window at
            // once, which clears it.
            WM_LBUTTONUP => {
                let pressed = window.pressed.take();
                if pressed.is_some() {
                    // SAFETY: gives back the mouse this window took at the press, on its thread.
                    if let Err(e) = unsafe { ReleaseCapture() } {
                        log::failure("frozen screen: giving back the mouse", &e);
                    }
                }
                match pressed {
                    Some(Pressed::Toolbar(held)) => {
                        let released = window.session.toolbar_hit(event::message_cursor());
                        // The button no longer looks pressed, whatever the click does.
                        redraw_window(hwnd);
                        match held {
                            Hit::Button(button) if released == Some(held) => {
                                deliver(Input::Toolbar(button))
                            }
                            Hit::Button(_) => Some(LRESULT(0)),
                            // The dragged toolbar is let go where it is, whatever the cursor is on now.
                            Hit::Background => deliver(Input::Release),
                        }
                    }
                    // A rectangle in Bounds ends where the mouse comes up, on the toolbar too.
                    Some(Pressed::Tool) | None => deliver(Input::Release),
                }
            }
            // Another window took the mouse while the button was held, so its release may never come here: a toolbar
            // button held down goes back to rest, and a dragged toolbar is let go where it is, or it would follow the
            // cursor with the button up. A rectangle in Bounds is pinned and copied only when the button really comes
            // up, over one of these windows: losing the mouse alone, as to the Windows key, which then closes the tools,
            // copies nothing, and the clipboard stays as it was (the owner's choice in WPC-313).
            WM_CAPTURECHANGED => match window.pressed.take() {
                Some(Pressed::Toolbar(Hit::Background)) => {
                    redraw_window(hwnd);
                    deliver(Input::Release)
                }
                Some(Pressed::Toolbar(Hit::Button(_))) => {
                    redraw_window(hwnd);
                    Some(LRESULT(0))
                }
                Some(Pressed::Tool) | None => Some(LRESULT(0)),
            },
            WM_MOUSEWHEEL => deliver(Input::Wheel(event::wheel(wparam))),
            WM_KEYDOWN | WM_KEYUP | WM_SYSKEYDOWN | WM_SYSKEYUP => {
                let key = event::key(wparam)?;
                window
                    .session
                    .input(if matches!(message, WM_KEYDOWN | WM_SYSKEYDOWN) {
                        Input::KeyDown(key)
                    } else {
                        Input::KeyUp(key)
                    });
                // Alt, and keys pressed with it, come as system keys. Windows handles them afterwards as before, so
                // Alt+F4 still turns into WM_CLOSE.
                matches!(message, WM_KEYDOWN | WM_KEYUP).then_some(LRESULT(0))
            }
            // Alt+Esc, which the first window holds while the tools are open: the tools' Esc.
            WM_HOTKEY if i32::try_from(wparam.0) == Ok(ALT_ESC) => {
                deliver(Input::KeyDown(Key::Esc))
            }
            // Alt+F4: the core closes the tools as a whole, never one monitor's window alone.
            WM_CLOSE => deliver(Input::CloseAsked),
            foreground::OTHER_PROGRAM_IN_FRONT => deliver(Input::OtherProgramInFront),
            foreground::OWN_WINDOW_IN_FRONT => deliver(Input::BackInFront),
            // Windows sends this to every top-level window for any change of monitors: one connected or disconnected,
            // a scale, resolution or arrangement changed, HDR turned on or off (seen in WPC-281 and WPC-289). Moving
            // the SDR content brightness sends nothing. It comes from another thread, so it may arrive inside any
            // Windows call that waits on one, even while a window draws with the photo borrowed; posted on, it reaches
            // the core from the message loop. Each window passes it on; the first closes the tools, and the core takes
            // nothing after.
            WM_DISPLAYCHANGE => {
                // SAFETY: our own window; the message carries nothing.
                let posted =
                    unsafe { PostMessageW(Some(hwnd), MONITORS_CHANGED, WPARAM(0), LPARAM(0)) };
                if let Err(e) = posted {
                    log::failure("frozen screen: passing on a change of monitors", &e);
                }
                Some(LRESULT(0))
            }
            MONITORS_CHANGED => deliver(Input::MonitorsChanged),
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

fn paint(hwnd: HWND, window: &Window) {
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
fn draw(hwnd: HWND, window: &Window) -> windows::core::Result<()> {
    // A copy, so the session is borrowed only for a moment.
    let shown = window.session.shown();
    let here = shown.on(window.monitor);
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
        let overlay = Overlay::new(&target, &graphics::fonts()?, rect, scale, window.mode)?;
        Ok(Drawing {
            target,
            bitmap,
            overlay,
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

#[cfg(test)]
mod tests {
    use super::*;

    use screengauge_core::session::{Label, Message, ToolbarButton};
    use screengauge_core::settings::{DimensionsMode, Tolerance, Tool};

    /// A measurement on `monitor` whose label reads `text`, so two of them on one monitor differ.
    fn on(monitor: usize, text: &str) -> Measurement {
        Measurement {
            monitor,
            horizontal: None,
            vertical: None,
            content: None,
            label: Label {
                physical: text.to_owned(),
                logical: None,
            },
        }
    }

    fn live(measurement: Measurement) -> Shown {
        Shown {
            live: Some(measurement),
            ..Shown::default()
        }
    }

    fn copied_on(monitor: usize) -> Notice {
        Notice {
            message: Message::Copied,
            at: Point { x: 10, y: 10 },
            monitor,
        }
    }

    #[test]
    fn a_measurement_that_appears_or_goes_redraws_its_monitor() {
        let nothing = Shown::default();
        assert_eq!(monitors_to_draw_again(&nothing, &live(on(1, "24"))), [1]);
        assert_eq!(monitors_to_draw_again(&live(on(1, "24")), &nothing), [1]);
    }

    #[test]
    fn a_measurement_that_changes_on_one_monitor_redraws_it_once() {
        assert_eq!(
            monitors_to_draw_again(&live(on(0, "24")), &live(on(0, "25"))),
            [0]
        );
    }

    #[test]
    fn a_measurement_that_moves_to_another_monitor_redraws_both() {
        assert_eq!(
            monitors_to_draw_again(&live(on(0, "24")), &live(on(2, "24"))),
            [0, 2]
        );
    }

    fn drawing(monitor: usize, right: i32) -> Shown {
        Shown {
            drawn_rectangle: Some(DrawnRectangle {
                monitor,
                rect: Rect {
                    left: 100,
                    top: 100,
                    width: right.abs_diff(100) + 1,
                    height: 50,
                },
            }),
            ..Shown::default()
        }
    }

    #[test]
    fn a_drawn_rectangle_that_appears_grows_or_goes_redraws_its_monitor_alone() {
        // Over nothing but the reference color, Bounds measures nothing, and the drawn rectangle alone changes.
        let nothing = Shown::default();
        assert_eq!(monitors_to_draw_again(&nothing, &drawing(1, 300)), [1]);
        assert_eq!(
            monitors_to_draw_again(&drawing(1, 300), &drawing(1, 301)),
            [1]
        );
        assert_eq!(monitors_to_draw_again(&drawing(1, 301), &nothing), [1]);
        assert_eq!(
            monitors_to_draw_again(&drawing(1, 301), &drawing(1, 301)),
            Vec::<usize>::new()
        );
    }

    fn snipping(monitor: usize, right: i32) -> Shown {
        let width = right.abs_diff(100) + 1;
        Shown {
            snip_area: Some(SnipArea {
                monitor,
                rect: Rect {
                    left: 100,
                    top: 100,
                    width,
                    height: 50,
                },
                label: Label {
                    physical: format!("{width} × 50"),
                    logical: None,
                },
            }),
            ..Shown::default()
        }
    }

    #[test]
    fn a_snip_area_that_appears_grows_or_goes_redraws_its_monitor_alone() {
        // With Alt, Snip shows its area with no drawn rectangle under it, so the area alone changes.
        let nothing = Shown::default();
        assert_eq!(monitors_to_draw_again(&nothing, &snipping(1, 300)), [1]);
        assert_eq!(
            monitors_to_draw_again(&snipping(1, 300), &snipping(1, 301)),
            [1]
        );
        assert_eq!(monitors_to_draw_again(&snipping(1, 301), &nothing), [1]);
        assert_eq!(
            monitors_to_draw_again(&snipping(1, 301), &snipping(1, 301)),
            Vec::<usize>::new()
        );
    }

    #[test]
    fn the_same_measurement_or_none_redraws_nothing() {
        let same = live(on(0, "24"));
        assert_eq!(monitors_to_draw_again(&same, &same), Vec::<usize>::new());
        let nothing = Shown::default();
        assert_eq!(
            monitors_to_draw_again(&nothing, &nothing),
            Vec::<usize>::new()
        );
    }

    #[test]
    fn pinning_the_live_measurement_redraws_its_monitor_though_the_live_one_stays() {
        let before = live(on(0, "24"));
        let after = Shown {
            pinned: vec![on(0, "24")],
            ..before.clone()
        };
        assert_eq!(monitors_to_draw_again(&before, &after), [0]);
    }

    #[test]
    fn taking_off_a_pin_on_another_monitor_redraws_that_monitor_alone() {
        let before = Shown {
            pinned: vec![on(1, "180")],
            ..live(on(0, "24"))
        };
        let after = live(on(0, "24"));
        assert_eq!(monitors_to_draw_again(&before, &after), [1]);
    }

    /// A magnified view on `monitor` around the pixel `(x, 100)`, with its tag showing when `tag`.
    fn magnified_on(monitor: usize, x: i32, tag: bool) -> Shown {
        let square = |left: i32, top: i32, side: u32| Rect {
            left,
            top,
            width: side,
            height: side,
        };
        let coordinate = |text: &str, line: Rect| screengauge_core::session::Coordinate {
            label: Label {
                physical: text.to_owned(),
                logical: None,
            },
            line,
        };
        Shown {
            magnified_view: Some(MagnifiedView {
                monitor,
                panel: square(x + 22, 122, 200),
                frame: square(x + 29, 129, 168),
                source: square(x - 10, 90, 21),
                shown: square(x - 10, 90, 21),
                center: Point { x, y: 100 },
                magnification: screengauge_core::settings::Magnification::X8,
                grid: true,
                tag,
                x: coordinate(&x.to_string(), square(x + 33, 304, 16)),
                y: coordinate("100", square(x + 120, 304, 16)),
                color: screengauge_core::session::PixelColor {
                    rgb: [255, 255, 255],
                    swatch: square(x + 33, 325, 22),
                    first: screengauge_core::session::ColorText {
                        text: "#FFFFFF".to_owned(),
                        line: square(x + 63, 328, 16),
                    },
                    second: None,
                },
            }),
            ..Shown::default()
        }
    }

    #[test]
    fn a_magnified_view_that_appears_moves_or_goes_redraws_its_monitor() {
        let nothing = Shown::default();
        assert_eq!(
            monitors_to_draw_again(&nothing, &magnified_on(1, 500, false)),
            [1]
        );
        assert_eq!(
            monitors_to_draw_again(&magnified_on(1, 500, false), &magnified_on(1, 501, false)),
            [1]
        );
        assert_eq!(
            monitors_to_draw_again(&magnified_on(1, 501, false), &nothing),
            [1]
        );
        assert_eq!(
            monitors_to_draw_again(&magnified_on(1, 501, false), &magnified_on(1, 501, false)),
            Vec::<usize>::new()
        );
    }

    #[test]
    fn the_magnifications_tag_that_goes_redraws_the_views_monitor() {
        assert_eq!(
            monitors_to_draw_again(&magnified_on(0, 500, true), &magnified_on(0, 500, false)),
            [0]
        );
    }

    #[test]
    fn a_magnified_view_that_follows_the_cursor_to_another_monitor_redraws_both() {
        assert_eq!(
            monitors_to_draw_again(&magnified_on(0, 500, false), &magnified_on(2, 500, false)),
            [0, 2]
        );
    }

    fn tolerance_on(monitor: usize, value: u8) -> ToleranceIndicator {
        ToleranceIndicator {
            tolerance: Tolerance::new(value).expect("within the limits"),
            at: Point { x: 10, y: 10 },
            monitor,
        }
    }

    #[test]
    fn the_tolerance_indicator_that_appears_changes_or_goes_redraws_its_monitor() {
        let quiet = live(on(0, "24"));
        let indicated = Shown {
            tolerance_indicator: Some(tolerance_on(0, 3)),
            ..quiet.clone()
        };
        let raised = Shown {
            tolerance_indicator: Some(tolerance_on(0, 4)),
            ..quiet.clone()
        };
        assert_eq!(monitors_to_draw_again(&quiet, &indicated), [0]);
        assert_eq!(monitors_to_draw_again(&indicated, &raised), [0]);
        assert_eq!(monitors_to_draw_again(&indicated, &quiet), [0]);
    }

    #[test]
    fn the_tolerance_indicator_that_follows_the_cursor_to_another_monitor_redraws_both() {
        let here = Shown {
            tolerance_indicator: Some(tolerance_on(0, 3)),
            ..Shown::default()
        };
        let there = Shown {
            tolerance_indicator: Some(tolerance_on(1, 3)),
            ..Shown::default()
        };
        assert_eq!(monitors_to_draw_again(&here, &there), [0, 1]);
    }

    fn toolbar_on(monitor: usize, active: Tool, mode: Option<DimensionsMode>) -> Shown {
        Shown {
            toolbar: Some(Toolbar {
                monitor,
                corner: Point {
                    x: 1920 + 700,
                    y: 14,
                },
                active,
                mode,
                minimized: false,
                hovered: None,
            }),
            ..Shown::default()
        }
    }

    #[test]
    fn another_tool_or_mode_lit_redraws_the_toolbars_monitor_alone() {
        let dimensions = toolbar_on(1, Tool::Dimensions, Some(DimensionsMode::Cross));
        let bounds = toolbar_on(1, Tool::Bounds, None);
        let horizontal = toolbar_on(1, Tool::Dimensions, Some(DimensionsMode::Horizontal));
        assert_eq!(monitors_to_draw_again(&dimensions, &bounds), [1]);
        assert_eq!(monitors_to_draw_again(&dimensions, &horizontal), [1]);
        assert_eq!(
            monitors_to_draw_again(&dimensions, &dimensions),
            Vec::<usize>::new()
        );
    }

    /// `shown` with its toolbar changed by `change`.
    fn with_toolbar(shown: &Shown, change: impl FnOnce(&mut Toolbar)) -> Shown {
        let mut changed = shown.clone();
        if let Some(toolbar) = changed.toolbar.as_mut() {
            change(toolbar);
        }
        changed
    }

    #[test]
    fn the_cursor_onto_another_button_or_minimizing_redraws_the_toolbars_monitor_and_the_cursor_alone_nothing()
     {
        let rest = toolbar_on(1, Tool::Dimensions, None);
        let on_close = with_toolbar(&rest, |toolbar| {
            toolbar.hovered = Some(ToolbarButton::Close);
        });
        let on_gear = with_toolbar(&rest, |toolbar| {
            toolbar.hovered = Some(ToolbarButton::Settings);
        });
        let small = with_toolbar(&rest, |toolbar| toolbar.minimized = true);
        assert_eq!(monitors_to_draw_again(&rest, &on_close), [1]);
        assert_eq!(monitors_to_draw_again(&on_close, &on_gear), [1]);
        assert_eq!(monitors_to_draw_again(&rest, &small), [1]);
        // The cursor's shape is Windows' to show: it changing alone draws nothing again.
        let arrow = Shown {
            cursor: Some(Cursor::Arrow),
            ..rest.clone()
        };
        assert_eq!(monitors_to_draw_again(&rest, &arrow), Vec::<usize>::new());
    }

    #[test]
    fn a_dragged_toolbar_redraws_its_monitor_alone() {
        let here = toolbar_on(1, Tool::Dimensions, None);
        let lower = with_toolbar(&here, |toolbar| toolbar.corner.y += 300);
        assert_eq!(monitors_to_draw_again(&here, &lower), [1]);
    }

    #[test]
    fn a_measurement_moving_on_another_monitor_leaves_the_toolbars_alone() {
        let toolbar = toolbar_on(0, Tool::Dimensions, Some(DimensionsMode::Cross));
        let before = Shown {
            live: Some(on(1, "24")),
            ..toolbar.clone()
        };
        let after = Shown {
            live: Some(on(1, "25")),
            ..toolbar
        };
        assert_eq!(monitors_to_draw_again(&before, &after), [1]);
    }

    #[test]
    fn a_notice_that_appears_or_goes_redraws_its_monitor() {
        let quiet = live(on(0, "24"));
        let noticed = Shown {
            notice: Some(copied_on(1)),
            ..quiet.clone()
        };
        assert_eq!(monitors_to_draw_again(&quiet, &noticed), [1]);
        assert_eq!(monitors_to_draw_again(&noticed, &quiet), [1]);
    }
}
