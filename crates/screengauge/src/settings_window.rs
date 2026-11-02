//! Settings: the one window, apart from the tools, where the user changes ScreenGauge's options (v1 spec WPC-296,
//! Settings). It opens at the center of the current monitor, shows on the taskbar and in Alt+Tab while it is open, and
//! opening it again brings the same window to the front. Its title bar is Windows' own, dark in the dark theme, and
//! follows the theme at once when it changes, from Settings or, in Automatic, from Windows (WPC-321); below
//! it, ScreenGauge draws one page that the user can't resize: the blocks' cards with their rows, and the footer with
//! the version and the link to the project's page. Its X closes only the window, and ScreenGauge keeps running.
//!
//! A click on a control changes the option at once, in the settings file too, with no Save or Cancel (WPC-320). The Snip's
//! "Change…", and its switch turned on with no folder kept, open Windows' folder window over this one (WPC-322). Like
//! the link, a control acts only when the left button comes up on the same one it went down on, as the toolbar's
//! buttons do (WPC-310). The global shortcut's field records a new combination (`recorder`, WPC-323); a press anywhere
//! else, on the title bar too, or Settings losing the front, stops it.
//!
//! Only the resident thread touches it. Its state lives here rather than in the window, since there is one window at
//! most, and nothing holds a borrow of it across a call that may send the window a message.

mod controls;
mod layout;
mod paint;
mod recorder;
mod recorder_field;
mod row_message;
mod snip_card;

use std::cell::{Cell, RefCell};

use screengauge_core::session::toolbar::scaled;
use screengauge_core::settings::{Folder, Options};
use windows::Win32::Foundation::{
    D2DERR_RECREATE_TARGET, E_FAIL, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM,
};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_SIZE_U, D2D1_ALPHA_MODE_IGNORE, D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_FEATURE_LEVEL_DEFAULT, D2D1_HWND_RENDER_TARGET_PROPERTIES, D2D1_PRESENT_OPTIONS_NONE,
    D2D1_RENDER_TARGET_PROPERTIES, D2D1_RENDER_TARGET_TYPE_DEFAULT, D2D1_RENDER_TARGET_USAGE_NONE,
    ID2D1HwndRenderTarget,
};
use windows::Win32::Graphics::Dwm::{DWMWA_USE_IMMERSIVE_DARK_MODE, DwmSetWindowAttribute};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::{
    BeginPaint, EndPaint, GetMonitorInfoW, InvalidateRect, MONITOR_DEFAULTTONEAREST, MONITORINFO,
    MonitorFromPoint, MonitorFromWindow, PAINTSTRUCT, ScreenToClient,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{
    AdjustWindowRectExForDpi, GetDpiForMonitor, GetDpiForWindow, MDT_EFFECTIVE_DPI,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    ReleaseCapture, SetCapture, TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent,
};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GW_ENABLEDPOPUP, GW_OWNER, GetClientRect,
    GetCursorPos, GetWindow, GetWindowRect, HTCLIENT, IDC_ARROW, IDC_HAND, IsIconic, LoadCursorW,
    LoadIconW, PostQuitMessage, RegisterClassExW, SW_RESTORE, SW_SHOW, SW_SHOWNORMAL,
    SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SetCursor,
    SetForegroundWindow, SetWindowPos, ShowWindow, WA_INACTIVE, WINDOW_EX_STYLE, WINDOW_STYLE,
    WM_ACTIVATE, WM_CAPTURECHANGED, WM_DESTROY, WM_DPICHANGED, WM_ERASEBKGND, WM_GETDPISCALEDSIZE,
    WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_NCLBUTTONDOWN, WM_PAINT,
    WM_SETCURSOR, WM_SYSKEYDOWN, WM_TIMER, WNDCLASSEXW, WS_CAPTION, WS_MINIMIZEBOX, WS_OVERLAPPED,
    WS_SYSMENU,
};
use windows::core::{BOOL, Error, HSTRING, PCWSTR, w};

use crate::theme::Mode;
use crate::win32::{low_word, signed_words, struct_size};
use crate::{
    com, event, folder_picker, graphics, keyboard_hook, log, resident, settings_file, tray,
};
use controls::{Control, Pointer};

const CLASS_NAME: PCWSTR = w!("ScreenGauge.Settings");

/// The message TrackMouseEvent sends when the cursor leaves the window. The windows crate has it only among the common
/// controls' features, which ScreenGauge doesn't otherwise need.
const WM_MOUSELEAVE: u32 = 0x02A3;

/// The title Windows shows in the title bar, on the taskbar and in Alt+Tab.
const TITLE: PCWSTR = w!("ScreenGauge Settings");

/// A title bar with the minimize and close buttons, and a thin frame the user can't drag to resize. No maximize button:
/// the page has one size.
const STYLE: WINDOW_STYLE =
    WINDOW_STYLE(WS_OVERLAPPED.0 | WS_CAPTION.0 | WS_SYSMENU.0 | WS_MINIMIZEBOX.0);

/// No extended style: a window with no owner and not a tool window is what shows on the taskbar and in Alt+Tab.
const EX_STYLE: WINDOW_EX_STYLE = WINDOW_EX_STYLE(0);

/// What "Project page" opens in the browser: the repository the workspace's manifest names.
const PROJECT_PAGE: &str = env!("CARGO_PKG_REPOSITORY");

thread_local! {
    /// The window while it is open.
    static WINDOW: Cell<Option<HWND>> = const { Cell::new(None) };
    /// The palette it is drawn in: the theme's when it opened, and again each time the theme changes (`follow`).
    static MODE: Cell<Mode> = const { Cell::new(Mode::Light) };
    /// Made at the first paint, and again after the scale changed or the graphics device was lost.
    static TARGET: RefCell<Option<ID2D1HwndRenderTarget>> = const { RefCell::new(None) };
    /// Where the last paint put what a click acts on, in the window's client pixels.
    static DRAWN: RefCell<Option<paint::Drawn>> = const { RefCell::new(None) };
    /// What the left button went down on and still holds: it acts only when the button comes up on it too.
    static PRESSED: Cell<Option<Hit>> = const { Cell::new(None) };
    /// The control the cursor is on, which the − and + show by brightening their sign.
    static OVER: Cell<Option<Control>> = const { Cell::new(None) };
    /// Whether Windows has been asked to say when the cursor leaves the window, so the brightened sign goes back.
    static TRACKING_LEAVE: Cell<bool> = const { Cell::new(false) };
}

/// What a click in the page can act on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Hit {
    Link,
    Control(Control),
}

/// Registered once at startup.
pub fn register_class(instance: HINSTANCE) -> windows::core::Result<()> {
    // SAFETY: MAKEINTRESOURCE of the icon group, in this exe's own module. A shared icon: never destroyed. With no small
    // icon given, Windows takes the title bar's from the same group.
    let icon = unsafe {
        LoadIconW(
            Some(instance),
            PCWSTR(std::ptr::without_provenance(tray::APP_ICON_ID)),
        )
    }?;
    // SAFETY: a system cursor, shared, never destroyed.
    let arrow = unsafe { LoadCursorW(None, IDC_ARROW) }?;
    let class = WNDCLASSEXW {
        cbSize: const { struct_size::<WNDCLASSEXW>() },
        lpfnWndProc: Some(window_proc),
        hInstance: instance,
        hIcon: icon,
        hCursor: arrow,
        lpszClassName: CLASS_NAME,
        ..Default::default()
    };
    // SAFETY: `class` is fully initialized, and its name is a static string.
    if unsafe { RegisterClassExW(&class) } == 0 {
        return Err(Error::from_thread());
    }
    Ok(())
}

/// Opens Settings in `mode`'s palette at the center of the monitor under the cursor, or, when it is open already,
/// brings it to the front as it is, restored should it be minimized.
pub fn open(mode: Mode) -> windows::core::Result<()> {
    if let Some(hwnd) = WINDOW.get() {
        // SAFETY: our own window, on the thread that created it.
        if unsafe { IsIconic(hwnd) }.as_bool() {
            // SAFETY: as above. The result says whether it was visible before, not whether this worked.
            let _ = unsafe { ShowWindow(hwnd, SW_RESTORE) };
        }
        // With the folder window open, Settings is disabled under it: the front goes to the folder window, which has
        // the keyboard to answer.
        to_front(enabled_popup(hwnd).unwrap_or(hwnd));
        return Ok(());
    }
    let (work, dpi) = monitor_at(event::cursor_now()?)?;
    let size = window_size(dpi)?;
    let corner = centered(work, size);
    // SAFETY: no module name, so this is the handle of ScreenGauge.exe itself.
    let instance: HINSTANCE = unsafe { GetModuleHandleW(None) }?.into();
    // SAFETY: the class is registered at startup. The window is made hidden, so its title bar is dark before it shows,
    // and at its place, so Windows gives it that monitor's scale from the start.
    let hwnd = unsafe {
        CreateWindowExW(
            EX_STYLE,
            CLASS_NAME,
            TITLE,
            STYLE,
            corner.x,
            corner.y,
            size.cx,
            size.cy,
            None,
            None,
            Some(instance),
            None,
        )
    }?;
    WINDOW.set(Some(hwnd));
    MODE.set(mode);
    title_bar(hwnd, mode);
    // SAFETY: our own window.
    let given = unsafe { GetDpiForWindow(hwnd) };
    if given != dpi {
        // Windows put it on another scale than its monitor's: the page is sized again for that one.
        fit(hwnd, given)?;
    }
    // SAFETY: our own window. The result says whether it was visible before.
    let _ = unsafe { ShowWindow(hwnd, SW_SHOW) };
    to_front(hwnd);
    Ok(())
}

/// Draws Settings in `mode`'s palette from now on, its title bar included, should it be open and in the other one.
pub fn follow(mode: Mode) {
    let Some(hwnd) = WINDOW.get() else {
        return;
    };
    if MODE.replace(mode) == mode {
        return;
    }
    title_bar(hwnd, mode);
    invalidate(hwnd);
}

/// Makes the title bar dark in the dark theme and light in the light one. Windows draws it again at once only when
/// told its frame changed. A failure leaves the window as it is, with a title bar of the other theme.
fn title_bar(hwnd: HWND, mode: Mode) {
    let dark = BOOL::from(mode == Mode::Dark);
    // SAFETY: our own window; the value outlives the call, which reads exactly its size. Documented from Windows 11's
    // first build, 22000, which ScreenGauge requires.
    let set = unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            (&raw const dark).cast(),
            const { struct_size::<BOOL>() },
        )
    };
    if let Err(e) = set {
        log::failure("Settings: the title bar's theme", &e);
        return;
    }
    // SAFETY: our own window; nothing moves, resizes, or changes order or activation.
    let redrawn = unsafe {
        SetWindowPos(
            hwnd,
            None,
            0,
            0,
            0,
            0,
            SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
        )
    };
    if let Err(e) = redrawn {
        log::failure("Settings: drawing the title bar again", &e);
    }
}

/// Closes Settings, should it be open, as ScreenGauge exits: before the graphics factories are released, since its
/// render target belongs to them.
pub fn close() {
    if let Some(hwnd) = WINDOW.get() {
        // SAFETY: our own window, on the thread that created it. Its WM_DESTROY clears its state.
        if let Err(e) = unsafe { DestroyWindow(hwnd) } {
            log::failure("Settings: closing it", &e);
        }
    }
}

/// Whether the global shortcut's field records: the global shortcut then doesn't call ScreenGauge.
pub fn is_recording() -> bool {
    recorder::is_recording()
}

/// Whether `hwnd` is the Settings window.
pub fn is(hwnd: HWND) -> bool {
    WINDOW.get() == Some(hwnd)
}

/// Whether `hwnd` is owned by the Settings window, directly or through a window that is: Windows' folder window is. It
/// counts as Settings for the window in front (`foreground`).
pub fn owns(hwnd: HWND) -> bool {
    let Some(settings) = WINDOW.get() else {
        return false;
    };
    let mut window = hwnd;
    for _ in 0..OWNER_DEPTH {
        // SAFETY: a window handle, which Windows checks. The windows crate turns "no owner", and a window already gone,
        // into an error, and neither is owned by Settings.
        let Ok(owner) = (unsafe { GetWindow(window, GW_OWNER) }) else {
            return false;
        };
        if owner == settings {
            return true;
        }
        window = owner;
    }
    false
}

/// How many owners up `owns` looks: a dialog owned by Settings is one level up, a window of that dialog two.
const OWNER_DEPTH: usize = 4;

/// The enabled window Settings owns and waits for, Windows' folder window, when it is open.
fn enabled_popup(hwnd: HWND) -> Option<HWND> {
    // SAFETY: our own window. With no enabled popup Windows returns the window itself, which is none.
    match unsafe { GetWindow(hwnd, GW_ENABLEDPOPUP) } {
        Ok(popup) if popup != hwnd => Some(popup),
        Ok(_) | Err(_) => None,
    }
}

/// Gives the keyboard to `hwnd`. Windows may refuse when another program took the front since the user's last click in
/// ScreenGauge; the window then flashes on the taskbar instead, as any program's would.
fn to_front(hwnd: HWND) {
    // SAFETY: our own window.
    if !unsafe { SetForegroundWindow(hwnd) }.as_bool() {
        log::failure(
            "Settings: bringing it to the front",
            &"Windows kept another window in front",
        );
    }
}

/// The work area of the monitor at `cursor`, or the nearest one, without the taskbar, and that monitor's DPI.
fn monitor_at(cursor: screengauge_core::session::Point) -> windows::core::Result<(RECT, u32)> {
    let point = POINT {
        x: cursor.x,
        y: cursor.y,
    };
    // SAFETY: no pointers; the nearest monitor is always one.
    let monitor = unsafe { MonitorFromPoint(point, MONITOR_DEFAULTTONEAREST) };
    let mut info = MONITORINFO {
        cbSize: const { struct_size::<MONITORINFO>() },
        ..Default::default()
    };
    // SAFETY: fills a local whose size is set.
    if !unsafe { GetMonitorInfoW(monitor, &mut info) }.as_bool() {
        return Err(Error::new(E_FAIL, "GetMonitorInfoW found no monitor"));
    }
    let (mut dpi, mut unused) = (0, 0);
    // SAFETY: fills two locals.
    unsafe { GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi, &mut unused) }?;
    Ok((info.rcWork, dpi))
}

/// The window's whole size at `dpi`, title bar and frame included, for the page's 560 logical pixels wide and its height
/// inside, taller while a message shows under a row's label.
fn window_size(dpi: u32) -> windows::core::Result<SIZE> {
    window_size_with(dpi, recorder::grown())
}

/// The window's whole size at `dpi` for the page grown as `grown` says.
fn window_size_with(dpi: u32, grown: Option<layout::Grown>) -> windows::core::Result<SIZE> {
    let too_large = || Error::new(E_FAIL, "the page is too large for a window");
    let mut rect = RECT {
        left: 0,
        top: 0,
        right: i32::try_from(scaled(layout::WIDTH, dpi)).map_err(|_| too_large())?,
        bottom: i32::try_from(scaled(layout::page(grown).height, dpi)).map_err(|_| too_large())?,
    };
    // SAFETY: adjusts a local.
    unsafe { AdjustWindowRectExForDpi(&mut rect, STYLE, false, EX_STYLE, dpi) }?;
    Ok(SIZE {
        cx: rect.right - rect.left,
        cy: rect.bottom - rect.top,
    })
}

/// Where a window of `size` goes to be centered in `work`. One taller than the work area goes to its top instead, so
/// its title bar stays where the mouse can take it, and the page runs off the bottom (Full HD at 175%, which the owner
/// accepted).
fn centered(work: RECT, size: SIZE) -> POINT {
    let width = work.right - work.left;
    let height = work.bottom - work.top;
    POINT {
        x: work.left + (width - size.cx) / 2,
        y: work.top + ((height - size.cy) / 2).max(0),
    }
}

/// Sizes the window for `dpi`, where it is, and draws it again. A window grown past the bottom of its monitor's work
/// area, by a message under a row's label, moves up just enough to stay above the taskbar.
fn fit(hwnd: HWND, dpi: u32) -> windows::core::Result<()> {
    let size = window_size(dpi)?;
    let mut place = RECT::default();
    // SAFETY: fills a local.
    unsafe { GetWindowRect(hwnd, &mut place) }?;
    let work = work_area_of(hwnd)?;
    let top = kept_inside(place.top, size.cy, work);
    // SAFETY: our own window.
    unsafe {
        SetWindowPos(
            hwnd,
            None,
            place.left,
            top,
            size.cx,
            size.cy,
            SWP_NOZORDER | SWP_NOACTIVATE,
        )
    }?;
    redraw(hwnd);
    Ok(())
}

/// The work area of the monitor `hwnd` is on, or the nearest one.
fn work_area_of(hwnd: HWND) -> windows::core::Result<RECT> {
    // SAFETY: our own window; the nearest monitor is always one.
    let monitor = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
    let mut info = MONITORINFO {
        cbSize: const { struct_size::<MONITORINFO>() },
        ..Default::default()
    };
    // SAFETY: fills a local whose size is set.
    if !unsafe { GetMonitorInfoW(monitor, &mut info) }.as_bool() {
        return Err(Error::new(E_FAIL, "GetMonitorInfoW found no monitor"));
    }
    Ok(info.rcWork)
}

/// Where the top of a window `height` tall goes, from `top`: where it is, unless its bottom would pass the bottom of
/// `work`, and then just high enough, but never above the work area's top, where its title bar stays reachable.
fn kept_inside(top: i32, height: i32, work: RECT) -> i32 {
    if top + height > work.bottom {
        (work.bottom - height).max(work.top)
    } else {
        top
    }
}

/// Sizes the window for the page as it is now, where it is: a message under a row's label came or went. The top stays
/// where it is, and the window grows or shrinks at the bottom.
fn follow_page(hwnd: HWND) {
    // SAFETY: our own window.
    let dpi = unsafe { GetDpiForWindow(hwnd) };
    if let Err(e) = fit(hwnd, dpi) {
        log::failure("Settings: following the page's height", &e);
    }
}

/// Drops the render target, made for the old size, and asks for a paint.
fn redraw(hwnd: HWND) {
    drop(TARGET.take());
    // SAFETY: our own window.
    if !unsafe { InvalidateRect(Some(hwnd), None, false) }.as_bool() {
        log::failure(
            "Settings: drawing it again",
            &"InvalidateRect returned FALSE",
        );
    }
}

extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        // Windows asks the size for the new scale before it moves the window to another monitor; given exactly, the
        // page stays 560 logical pixels wide instead of the frame and the page growing in proportion.
        WM_GETDPISCALEDSIZE => {
            let size = u32::try_from(wparam.0)
                .map_err(|_| Error::new(E_FAIL, "a DPI past any monitor's"))
                .and_then(window_size);
            match size {
                Ok(size) => {
                    // SAFETY: WM_GETDPISCALEDSIZE's lParam points to the SIZE Windows reads back.
                    unsafe { *(lparam.0 as *mut SIZE) = size };
                    LRESULT(1)
                }
                Err(e) => {
                    log::failure("Settings: sizing it for a new scale", &e);
                    LRESULT(0)
                }
            }
        }
        WM_DPICHANGED => {
            // SAFETY: WM_DPICHANGED's lParam points to the rectangle Windows suggests, which the size given above makes.
            let suggested = unsafe { *(lparam.0 as *const RECT) };
            // SAFETY: our own window.
            let moved = unsafe {
                SetWindowPos(
                    hwnd,
                    None,
                    suggested.left,
                    suggested.top,
                    suggested.right - suggested.left,
                    suggested.bottom - suggested.top,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                )
            };
            if let Err(e) = moved {
                log::failure("Settings: following a new scale", &e);
            }
            redraw(hwnd);
            LRESULT(0)
        }
        // Direct2D paints every pixel; erasing first would only flash.
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => {
            paint(hwnd);
            LRESULT(0)
        }
        WM_SETCURSOR
            if low_word(lparam) == HTCLIENT && hit_at(cursor_in(hwnd)) == Some(Hit::Link) =>
        {
            // SAFETY: a system cursor, shared.
            match unsafe { LoadCursorW(None, IDC_HAND) } {
                // SAFETY: a valid cursor.
                Ok(hand) => unsafe { SetCursor(Some(hand)) },
                Err(e) => {
                    log::failure("Settings: the link's hand", &e);
                    return default(hwnd, message, wparam, lparam);
                }
            };
            LRESULT(1)
        }
        WM_MOUSEMOVE => {
            track_leave(hwnd);
            point_at(hwnd, control_at(Some(client_point(lparam))));
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            TRACKING_LEAVE.set(false);
            point_at(hwnd, None);
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            let point = Some(client_point(lparam));
            // A press anywhere but on the field stops it recording; the press then acts as it would.
            let hit = hit_at(point);
            if hit != Some(Hit::Control(Control::ShortcutField)) {
                recorder::cancel(hwnd, hit.is_some());
            }
            // A press with no move before it, as after Alt+Tab with the cursor already on +, still finds the cursor
            // on what it pressed, so a held − or + looks pressed at once.
            OVER.set(control_at(point));
            if let Some(hit) = hit_at(point) {
                PRESSED.set(Some(hit));
                // SAFETY: our own window; the release comes here wherever the mouse is.
                unsafe { SetCapture(hwnd) };
                // A held − or + looks pressed.
                invalidate(hwnd);
            }
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            if let Some(pressed) = PRESSED.take() {
                // SAFETY: ends the capture taken above. Its WM_CAPTURECHANGED finds nothing held any more.
                if let Err(e) = unsafe { ReleaseCapture() } {
                    log::failure("Settings: letting the mouse go", &e);
                }
                if hit_at(Some(client_point(lparam))) == Some(pressed) {
                    act(hwnd, pressed);
                }
                // ScreenGauge may have exited while the folder window was open, and taken this window with it.
                if WINDOW.get() == Some(hwnd) {
                    invalidate(hwnd);
                }
            }
            recorder::settle(hwnd);
            LRESULT(0)
        }
        WM_CAPTURECHANGED => {
            // Another window took the mouse while the button was held: nothing acts.
            if PRESSED.take().is_some() {
                invalidate(hwnd);
            }
            recorder::settle(hwnd);
            LRESULT(0)
        }
        // A press on the title bar or the frame, and another window coming to the front, stop the field recording.
        WM_NCLBUTTONDOWN => {
            recorder::cancel(hwnd, false);
            default(hwnd, message, wparam, lparam)
        }
        WM_ACTIVATE => {
            if low_word(LPARAM(wparam.0.cast_signed())) == WA_INACTIVE {
                recorder::cancel(hwnd, false);
            }
            default(hwnd, message, wparam, lparam)
        }
        WM_TIMER if wparam.0 == recorder::BLINK_TIMER => {
            invalidate_field(hwnd);
            LRESULT(0)
        }
        // While the field records, the hook keeps every new key from Settings too: a first press reaching it means
        // Windows removed the hook. The repeats of a key held since before the click, and the no key a Win comes up
        // behind, pass on purpose.
        WM_KEYDOWN | WM_SYSKEYDOWN
            if recorder::is_recording()
                && lparam.0 & (1 << 30) == 0
                && u32::try_from(wparam.0).is_ok_and(|vk| {
                    !keyboard_hook::is_modifier(vk) && vk != keyboard_hook::NO_KEY
                }) =>
        {
            recorder::hook_lost(hwnd);
            default(hwnd, message, wparam, lparam)
        }
        keyboard_hook::KEY => {
            recorder::on_key(hwnd, wparam, lparam);
            LRESULT(0)
        }
        keyboard_hook::ENDED => {
            recorder::on_hook_ended(hwnd, wparam);
            LRESULT(0)
        }
        WM_DESTROY => {
            // The keyboard goes back to Windows at once.
            recorder::close();
            WINDOW.set(None);
            drop(TARGET.take());
            DRAWN.set(None);
            PRESSED.set(None);
            OVER.set(None);
            TRACKING_LEAVE.set(false);
            LRESULT(0)
        }
        _ => default(hwnd, message, wparam, lparam),
    }
}

fn default(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // SAFETY: the arguments are the ones Windows passed to this window procedure.
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}

/// The point a mouse message carries, in the window's client pixels.
fn client_point(lparam: LPARAM) -> POINT {
    let (x, y) = signed_words(lparam.0.cast_unsigned());
    POINT {
        x: i32::from(x),
        y: i32::from(y),
    }
}

/// Where the cursor is now, in the window's client pixels. `None` when Windows can't say, which is never on the link.
fn cursor_in(hwnd: HWND) -> Option<POINT> {
    let mut point = POINT::default();
    // SAFETY: fills a local.
    if let Err(e) = unsafe { GetCursorPos(&mut point) } {
        log::failure("Settings: finding the cursor", &e);
        return None;
    }
    // SAFETY: our own window; converts a local in place.
    if !unsafe { ScreenToClient(hwnd, &mut point) }.as_bool() {
        log::failure(
            "Settings: placing the cursor in the window",
            &"ScreenToClient returned FALSE",
        );
        return None;
    }
    Some(point)
}

fn contains(rect: &RECT, point: POINT) -> bool {
    (rect.left..rect.right).contains(&point.x) && (rect.top..rect.bottom).contains(&point.y)
}

/// What a click at `point` acts on, as the last paint drew it.
fn hit_at(point: Option<POINT>) -> Option<Hit> {
    let point = point?;
    DRAWN.with_borrow(|drawn| {
        let drawn = drawn.as_ref()?;
        if contains(&drawn.link, point) {
            return Some(Hit::Link);
        }
        drawn
            .controls
            .iter()
            .find(|(_, place)| contains(place, point))
            .map(|&(control, _)| Hit::Control(control))
    })
}

fn control_at(point: Option<POINT>) -> Option<Control> {
    match hit_at(point)? {
        Hit::Control(control) => Some(control),
        Hit::Link => None,
    }
}

/// Notes that the cursor is on `over`, and draws the page again when that changed.
fn point_at(hwnd: HWND, over: Option<Control>) {
    if OVER.replace(over) != over {
        invalidate(hwnd);
    }
}

/// Asks Windows for WM_MOUSELEAVE when the cursor leaves the window, once each time it comes in.
fn track_leave(hwnd: HWND) {
    if TRACKING_LEAVE.get() {
        return;
    }
    let mut track = TRACKMOUSEEVENT {
        cbSize: const { struct_size::<TRACKMOUSEEVENT>() },
        dwFlags: TME_LEAVE,
        hwndTrack: hwnd,
        dwHoverTime: 0,
    };
    // SAFETY: our own window; the struct is fully set and outlives the call.
    match unsafe { TrackMouseEvent(&mut track) } {
        Ok(()) => TRACKING_LEAVE.set(true),
        // The sign may stay bright after the cursor leaves; the next move over the window asks again.
        Err(e) => log::failure("Settings: following the cursor out of the window", &e),
    }
}

/// Does what a click on `hit` does.
fn act(hwnd: HWND, hit: Hit) {
    match hit {
        Hit::Link => open_project_page(hwnd),
        Hit::Control(Control::ShortcutField) => recorder::click_field(hwnd),
        Hit::Control(Control::ShortcutReset) => recorder::click_reset(hwnd),
        Hit::Control(control) => {
            // Read first and nothing held: the folder window below runs a message loop, in which this window is drawn
            // from the options and the file is saved again.
            let options = match settings_file::current() {
                Ok(stored) => stored.options,
                Err(e) => {
                    log::failure("Settings: reading the options for a click", &e);
                    return;
                }
            };
            match controls::change(control, &options) {
                Some(change) => save(|options| options.apply(change)),
                None => {
                    choose_folder(hwnd, control, &options);
                    resident::folder_window_closed();
                }
            }
            // A theme chosen here shows at once, in Settings and in the tools too: they may be stepped aside while
            // Save As waits, and come back in the theme chosen (seen in review).
            if let Control::Theme(_) = control {
                resident::follow_theme();
            }
        }
    }
}

/// Changes the options with `change`, which says whether it changed anything, and saves them at once. The change holds
/// in this run even when the file couldn't be saved.
fn save(change: impl FnOnce(&mut Options) -> bool) {
    if let Err(e) = settings_file::update(|stored| change(&mut stored.options)) {
        log::failure("Settings: saving a change", &e);
    }
}

/// "Change…", and the switch turned on with no folder kept: Windows' folder window opens, in the folder kept when there
/// is one. The folder chosen is kept, and with the switch it turns it on; cancelling changes nothing, so the switch stays
/// off (v1 spec WPC-296, Settings). Must be called with nothing borrowed that this window's procedure borrows: the folder
/// window runs its own message loop.
fn choose_folder(hwnd: HWND, control: Control, options: &Options) {
    let chosen = folder_picker::choose(hwnd, options.default_folder.folder());
    // ScreenGauge exited while the folder window was open, as it can from the tray icon: the end of the message loop it
    // asked for went to the folder window's own loop, which kept it, so it is asked for again (as with Save As, seen in
    // WPC-318). Asked of the resident window, not of this one, which closes for no reason a quit should follow.
    if resident::is_exiting() {
        // SAFETY: ends this thread's message loop.
        unsafe { PostQuitMessage(0) };
        return;
    }
    let path = match chosen {
        Ok(Some(path)) => path,
        Ok(None) => return,
        Err(e) => {
            log::failure("Settings: the folder window", &e);
            return;
        }
    };
    // The log never shows the path, which holds the Windows user name.
    let Some(folder) = Folder::new(&path) else {
        log::failure(
            "Settings: keeping the folder chosen",
            &"it isn't a full path the settings file can keep",
        );
        return;
    };
    let turn_on = control == Control::SaveToDefaultFolder;
    save(|options| options.choose_default_folder(folder, turn_on));
}

/// Asks for a paint of the global shortcut's field, as the blink does. Direct2D draws the whole frame still; Windows
/// only has less to bring up to date.
fn invalidate_field(hwnd: HWND) {
    let field = DRAWN.with_borrow(|drawn| {
        drawn.as_ref().and_then(|drawn| {
            drawn
                .controls
                .iter()
                .find(|(control, _)| *control == Control::ShortcutField)
                .map(|&(_, place)| place)
        })
    });
    let Some(field) = field else {
        invalidate(hwnd);
        return;
    };
    // SAFETY: our own window; the rectangle outlives the call.
    if !unsafe { InvalidateRect(Some(hwnd), Some(&field), false) }.as_bool() {
        log::failure(
            "Settings: drawing the field again",
            &"InvalidateRect returned FALSE",
        );
    }
}

/// Asks for a paint with the render target kept, as after a click or a move that changes what the page shows.
fn invalidate(hwnd: HWND) {
    // SAFETY: our own window.
    if !unsafe { InvalidateRect(Some(hwnd), None, false) }.as_bool() {
        log::failure(
            "Settings: drawing it again",
            &"InvalidateRect returned FALSE",
        );
    }
}

/// Opens the project's page in the browser the user chose in Windows. A failure is only logged: the window stays as
/// it is.
fn open_project_page(hwnd: HWND) {
    if let Err(e) = com::ensure_started() {
        log::failure("Settings: starting COM for the project page", &e);
        return;
    }
    let page = HSTRING::from(PROJECT_PAGE);
    // SAFETY: the strings outlive the call. Windows hands the address to the browser.
    let opened = unsafe {
        ShellExecuteW(
            Some(hwnd),
            w!("open"),
            &page,
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };
    // ShellExecuteW returns a number above 32 when it worked, and an error code otherwise, in a handle's place: the
    // pointer's address is that number, and an isize holds every address on x64.
    let code = opened.0 as isize;
    if code <= 32 {
        log::failure(
            "Settings: opening the project page",
            &format!("ShellExecuteW returned {code}"),
        );
    }
}

fn paint(hwnd: HWND) {
    let mut paint = PAINTSTRUCT::default();
    // SAFETY: WM_PAINT on our own window, ended below. Direct2D draws without the device context it returns.
    unsafe { BeginPaint(hwnd, &mut paint) };
    if let Err(e) = draw(hwnd) {
        log::failure("Settings: drawing it", &e);
    }
    // SAFETY: ends the BeginPaint above. EndPaint always returns TRUE.
    let _ = unsafe { EndPaint(hwnd, &paint) };
}

/// Draws the page at the window's scale, onto a render target of the client area's size.
fn draw(hwnd: HWND) -> windows::core::Result<()> {
    let mut client = RECT::default();
    // SAFETY: fills a local.
    unsafe { GetClientRect(hwnd, &mut client) }?;
    let size = D2D_SIZE_U {
        width: client.right.max(0).cast_unsigned(),
        height: client.bottom.max(0).cast_unsigned(),
    };
    // Minimized, the window has no client area to draw.
    if size.width == 0 || size.height == 0 {
        return Ok(());
    }
    let target = match TARGET.take() {
        Some(target) => target,
        None => render_target(hwnd, size)?,
    };
    // SAFETY: our own window.
    let dpi = unsafe { GetDpiForWindow(hwnd) };
    let fonts = graphics::fonts()?;
    let factory = graphics::factory()?;
    let options = settings_file::current()
        .map_err(|e| Error::new(E_FAIL, e.to_string()))?
        .options;
    let shown = paint::Shown {
        recorder: recorder::view(options.global_shortcut),
        message: recorder::message(),
        grown: recorder::grown(),
    };
    let pointer = Pointer {
        over: OVER.get(),
        held: match PRESSED.get() {
            Some(Hit::Control(control)) => Some(control),
            Some(Hit::Link) | None => None,
        },
    };
    let surface = paint::Surface {
        target: &target,
        factory: &factory,
        fonts: &fonts,
        mode: MODE.get(),
        dpi,
        width: size.width,
    };
    // SAFETY: drawing calls on a live render target, between BeginDraw and EndDraw.
    unsafe { target.BeginDraw() };
    let drawn = paint::draw(&surface, &options, pointer, &shown);
    // SAFETY: ends the BeginDraw above.
    match unsafe { target.EndDraw(None, None) } {
        Ok(()) => TARGET.set(Some(target)),
        // The graphics device was lost, as after a driver update: the next paint makes the target again.
        Err(e) if e.code() == D2DERR_RECREATE_TARGET => {
            redraw(hwnd);
            return Ok(());
        }
        Err(e) => {
            // The frame failed as a whole; what failed inside it, if anything, is logged so it isn't lost.
            if let Err(inside) = drawn {
                log::failure("Settings: drawing it", &inside);
            }
            return Err(e);
        }
    }
    DRAWN.set(Some(drawn?));
    Ok(())
}

/// A render target of `size` for the window, at 96 DPI so one Direct2D unit is one physical pixel at any scale.
fn render_target(hwnd: HWND, size: D2D_SIZE_U) -> windows::core::Result<ID2D1HwndRenderTarget> {
    let target_properties = D2D1_RENDER_TARGET_PROPERTIES {
        r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
        pixelFormat: D2D1_PIXEL_FORMAT {
            format: DXGI_FORMAT_B8G8R8A8_UNORM,
            alphaMode: D2D1_ALPHA_MODE_IGNORE,
        },
        dpiX: 96.0,
        dpiY: 96.0,
        usage: D2D1_RENDER_TARGET_USAGE_NONE,
        minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
    };
    let window_properties = D2D1_HWND_RENDER_TARGET_PROPERTIES {
        hwnd,
        pixelSize: size,
        presentOptions: D2D1_PRESENT_OPTIONS_NONE,
    };
    // SAFETY: both property structs outlive the call.
    unsafe { graphics::factory()?.CreateHwndRenderTarget(&target_properties, &window_properties) }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The owner's ARZOPA monitor, Full HD, with the taskbar's 48 logical pixels at the bottom.
    fn full_hd_work_area(dpi: u32) -> RECT {
        let taskbar = i32::try_from(scaled(48, dpi)).expect("a small taskbar");
        RECT {
            left: 3840,
            top: 0,
            right: 3840 + 1920,
            bottom: 1080 - taskbar,
        }
    }

    #[test]
    fn at_150_percent_on_full_hd_the_whole_window_fits_above_the_taskbar() {
        // The spec's 672 logical pixels of room: 1080 / 1.5 less the taskbar's 48.
        let work = full_hd_work_area(144);
        assert_eq!(work.bottom - work.top, 1008);
        let size = window_size(144).expect("a size for 150%");
        let corner = centered(work, size);
        assert!(corner.y >= work.top, "{corner:?}");
        assert!(corner.y + size.cy <= work.bottom, "{size:?} at {corner:?}");
    }

    #[test]
    fn the_page_inside_the_frame_is_560_logical_pixels_wide_at_every_scale() {
        for (dpi, page) in [(96, 560), (120, 700), (144, 840), (168, 980)] {
            let size = window_size(dpi).expect("a size");
            let mut frame = RECT::default();
            // SAFETY: adjusts a local.
            unsafe { AdjustWindowRectExForDpi(&mut frame, STYLE, false, EX_STYLE, dpi) }
                .expect("the frame");
            assert_eq!(size.cx - (frame.right - frame.left), page, "{dpi}");
        }
    }

    #[test]
    fn the_window_is_centered_in_the_work_area_of_its_monitor() {
        let work = full_hd_work_area(120);
        let corner = centered(work, SIZE { cx: 720, cy: 760 });
        // 1080 less the taskbar's 60 at 125% leaves 1020 of room: (1020 - 760) / 2 above.
        assert_eq!(
            corner,
            POINT {
                x: 3840 + 600,
                y: 130
            }
        );
    }

    #[test]
    fn a_message_at_the_widest_field_grows_the_window_and_it_moves_up_to_stay_above_the_taskbar() {
        use screengauge_core::settings::{Key, Modifiers, Shortcut};

        // Full HD at 150%: 672 logical pixels of work area, the window centered there at its usual height.
        let dpi = 144;
        let work = full_hd_work_area(dpi);
        let corner = centered(work, window_size_with(dpi, None).expect("a size"));
        // The widest field there is, and the longest message: it wraps on the most lines.
        let widest = Shortcut::new(
            Modifiers {
                win: true,
                ctrl: true,
                alt: true,
                shift: true,
            },
            Key::Function(12),
        )
        .expect("allowed");
        let fonts = graphics::Fonts::new().expect("the fonts");
        let message = row_message::Message {
            row: layout::Row::GlobalShortcut,
            text: recorder_field::message(screengauge_core::settings::Refusal::CtrlAltWithoutWin),
            band: false,
            width: recorder_field::message_room(widest, &paint::measurer(&fonts, 1.0))
                .expect("the room"),
        };
        let grown = message.growth(&fonts).expect("the growth");
        let size = window_size_with(dpi, Some(grown)).expect("a size");
        assert!(
            corner.y + size.cy > work.bottom,
            "the grown window would reach under the taskbar: {size:?} at {corner:?}, {grown:?}"
        );
        let top = kept_inside(corner.y, size.cy, work);
        assert!(top >= work.top, "{top}");
        assert_eq!(top + size.cy, work.bottom);
    }

    #[test]
    fn a_window_that_fits_keeps_its_top_and_one_taller_than_the_work_area_stays_at_its_top() {
        let work = full_hd_work_area(144);
        assert_eq!(kept_inside(100, 600, work), 100);
        assert_eq!(kept_inside(400, 700, work), work.bottom - 700);
        assert_eq!(kept_inside(400, 5000, work), work.top);
    }

    #[test]
    fn a_window_taller_than_the_work_area_goes_to_its_top_so_the_title_bar_stays_reachable() {
        let work = full_hd_work_area(168);
        let corner = centered(work, SIZE { cx: 1000, cy: 1100 });
        assert_eq!(
            corner,
            POINT {
                x: 3840 + 460,
                y: 0
            }
        );
    }
}
