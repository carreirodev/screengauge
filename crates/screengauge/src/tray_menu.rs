//! The tray icon's menu, which a right click on the icon opens: "Settings", "Start with Windows", with a check mark
//! while it is on, and "Exit" (v1 spec WPC-296, the tray icon's right-click menu; WPC-326). ScreenGauge draws it itself, in its theme and at the scale of the monitor it opens on (the spec's
//! derived rule 1, which follows from ADR 0001); Windows only rounds its corners, draws its 1-pixel border and its
//! shadow.
//!
//! It closes with Esc, with a click anywhere else, which takes the front from it, and once an item is chosen. Like
//! Settings' controls, an item acts only when the left button comes up on the one it went down on; the arrow keys light
//! an item and Enter chooses it. The item chosen goes to the resident window (`CHOSEN`), which acts once the menu is
//! gone.
//!
//! Only the resident thread touches it, and there is one menu at most, so its state lives here.

mod layout;
mod paint;

use std::cell::{Cell, RefCell};

use windows::Win32::Foundation::{
    COLORREF, D2DERR_RECREATE_TARGET, E_FAIL, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM,
};
use windows::Win32::Graphics::Direct2D::Common::D2D_SIZE_U;
use windows::Win32::Graphics::Direct2D::ID2D1HwndRenderTarget;
use windows::Win32::Graphics::Dwm::{
    DWM_WINDOW_CORNER_PREFERENCE, DWMWA_BORDER_COLOR, DWMWA_TRANSITIONS_FORCEDISABLED,
    DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND, DwmFlush, DwmSetWindowAttribute,
};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, EndPaint, GetMonitorInfoW, InvalidateRect, MONITOR_DEFAULTTONEAREST, MONITORINFO,
    MonitorFromPoint, PAINTSTRUCT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, GetDpiForWindow, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    ReleaseCapture, SetCapture, TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent, VIRTUAL_KEY, VK_DOWN,
    VK_ESCAPE, VK_RETURN, VK_SPACE, VK_UP,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CS_DROPSHADOW, CreateWindowExW, DefWindowProcW, DestroyWindow, IDC_ARROW, IsWindowVisible,
    LoadCursorW, PostMessageW, RegisterClassExW, SW_HIDE, SW_SHOW, SWP_NOACTIVATE, SWP_NOZORDER,
    SetForegroundWindow, SetWindowPos, ShowWindow, WA_INACTIVE, WM_ACTIVATE, WM_APP,
    WM_CAPTURECHANGED, WM_CLOSE, WM_DESTROY, WM_ERASEBKGND, WM_KEYDOWN, WM_LBUTTONDOWN,
    WM_LBUTTONUP, WM_MOUSEMOVE, WM_PAINT, WNDCLASSEXW, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::{BOOL, Error, PCWSTR, w};

use crate::theme::Mode;
use crate::win32::{low_word, signed_words, struct_size};
use crate::{graphics, log};
pub use layout::Item;

const CLASS_NAME: PCWSTR = w!("ScreenGauge.TrayMenu");

/// What the menu posts the resident window once an item is chosen, with the item in wParam (`chosen`). WM_APP + 1 to
/// + 12 are taken.
pub const CHOSEN: u32 = WM_APP + 13;

/// What the menu posts the resident window when it opens or closes, so the tray icon's tip follows (`is_open`): hidden
/// while it is open, which it would otherwise cover (WPC-373). Posted rather than done here: the menu opens while
/// Explorer waits for the icon's message to be answered, and closes on the way to the photo. WM_APP + 1 to + 14 are
/// taken.
pub const OPENED_OR_CLOSED: u32 = WM_APP + 15;

/// The message TrackMouseEvent sends when the cursor leaves the window, as in Settings.
const WM_MOUSELEAVE: u32 = 0x02A3;

thread_local! {
    /// The menu while it is open.
    static WINDOW: Cell<Option<HWND>> = const { Cell::new(None) };
    /// The window that hears of the item chosen.
    static RESIDENT: Cell<Option<HWND>> = const { Cell::new(None) };
    static MODE: Cell<Mode> = const { Cell::new(Mode::Light) };
    /// Whether "Start with Windows" shows its check mark.
    static CHECKED: Cell<bool> = const { Cell::new(false) };
    /// Made at the first paint, and again after the graphics device was lost.
    static TARGET: RefCell<Option<ID2D1HwndRenderTarget>> = const { RefCell::new(None) };
    /// The item tinted: the one under the mouse, or the one the arrow keys lit.
    static LIT: Cell<Option<Item>> = const { Cell::new(None) };
    /// The item the left button went down on and still holds.
    static PRESSED: Cell<Option<Item>> = const { Cell::new(None) };
    /// Whether Windows has been asked to say when the cursor leaves the menu.
    static TRACKING_LEAVE: Cell<bool> = const { Cell::new(false) };
    /// Whether the menu has closed since the last photo. It may close on the press of a left click on the tray icon,
    /// before the release calls ScreenGauge, and the screen may not be drawn without it yet (`close_before_photo`).
    static GONE_SINCE_PHOTO: Cell<bool> = const { Cell::new(false) };
}

/// Registered once at startup. Its shadow is the one Windows gives its own menus.
pub fn register_class(instance: HINSTANCE) -> windows::core::Result<()> {
    // SAFETY: a system cursor, shared, never destroyed.
    let arrow = unsafe { LoadCursorW(None, IDC_ARROW) }?;
    let class = WNDCLASSEXW {
        cbSize: const { struct_size::<WNDCLASSEXW>() },
        style: CS_DROPSHADOW,
        lpfnWndProc: Some(window_proc),
        hInstance: instance,
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

/// Opens the menu for a click at `anchor` (`tray::anchor_of`), in `mode`'s palette, at the scale of the click's
/// monitor, with "Start with Windows" `checked` or not, and gives it the front, so a click anywhere else closes it. A
/// menu already open closes first. The item chosen goes to `resident`.
pub fn open(resident: HWND, anchor: POINT, mode: Mode, checked: bool) -> windows::core::Result<()> {
    close();
    let (work, dpi) = monitor_at(anchor)?;
    let size = layout::size_at(dpi);
    let corner = layout::placed(anchor, size, work);
    // SAFETY: no module name, so this is the handle of ScreenGauge.exe itself.
    let instance: HINSTANCE = unsafe { GetModuleHandleW(None) }?.into();
    // SAFETY: the class is registered at startup. The window is made hidden, at its place, so Windows gives it that
    // monitor's scale from the start; a tool window has no taskbar button, and topmost keeps it above the taskbar.
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
            CLASS_NAME,
            w!("ScreenGauge"),
            WS_POPUP,
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
    RESIDENT.set(Some(resident));
    MODE.set(mode);
    CHECKED.set(checked);
    // SAFETY: our own window.
    let given = unsafe { GetDpiForWindow(hwnd) };
    if given != dpi {
        // Windows put it on another scale than its monitor's: it is sized and placed again for that one.
        let size = layout::size_at(given);
        let corner = layout::placed(anchor, size, work);
        // SAFETY: our own window.
        if let Err(e) = unsafe {
            SetWindowPos(
                hwnd,
                None,
                corner.x,
                corner.y,
                size.cx,
                size.cy,
                SWP_NOZORDER | SWP_NOACTIVATE,
            )
        } {
            close();
            return Err(e);
        }
    }
    frame(hwnd, mode);
    // SAFETY: our own window. The result says whether it was visible before.
    let _ = unsafe { ShowWindow(hwnd, SW_SHOW) };
    tell_resident(resident);
    // The tray icon's click lets ScreenGauge take the front. Without it, a click elsewhere wouldn't close the menu, nor
    // would Esc reach it; it still closes on an item, or on the next right click on the icon.
    // SAFETY: our own window.
    if !unsafe { SetForegroundWindow(hwnd) }.as_bool() {
        log::failure(
            "tray menu: bringing it to the front",
            &"Windows kept another window in front",
        );
    }
    Ok(())
}

/// Asks Windows to round the corners, as its own menus', draw the border in the palette's edge, and show and hide the
/// menu at once, with no fade a photo could catch. A failure leaves Windows' own way, and the menu works the same.
fn frame(hwnd: HWND, mode: Mode) {
    let corners = DWMWCP_ROUND;
    // SAFETY: our own window; the value outlives the call, which reads exactly its size.
    let rounded = unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            (&raw const corners).cast(),
            const { struct_size::<DWM_WINDOW_CORNER_PREFERENCE>() },
        )
    };
    if let Err(e) = rounded {
        log::failure("tray menu: rounding its corners", &e);
    }
    let [r, g, b] = paint::border(mode);
    let border = COLORREF(u32::from(r) | u32::from(g) << 8 | u32::from(b) << 16);
    // SAFETY: as above.
    let bordered = unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_BORDER_COLOR,
            (&raw const border).cast(),
            const { struct_size::<COLORREF>() },
        )
    };
    if let Err(e) = bordered {
        log::failure("tray menu: its border's color", &e);
    }
    let no_fade = BOOL::from(true);
    // SAFETY: as above.
    let at_once = unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_TRANSITIONS_FORCEDISABLED,
            (&raw const no_fade).cast(),
            const { struct_size::<BOOL>() },
        )
    };
    if let Err(e) = at_once {
        log::failure("tray menu: showing it without a fade", &e);
    }
}

/// Closes the menu, should it be open, choosing nothing.
pub fn close() {
    if let Some(hwnd) = WINDOW.get() {
        // SAFETY: our own window, on the thread that created it. Its WM_DESTROY clears its state.
        if let Err(e) = unsafe { DestroyWindow(hwnd) } {
            log::failure("tray menu: closing it", &e);
        }
    }
}

/// Closes the menu before ScreenGauge photographs the screen, and waits for the screen to be drawn without it, so it is
/// never in the photo: when it is open, and when it closed since the last photo, as on the press of the left click that
/// calls. Otherwise nothing, and no wait.
pub fn close_before_photo() {
    if let Some(hwnd) = WINDOW.get() {
        // Hidden first: destroyed, it is gone from the screen once Windows composes the next frame, which DwmFlush
        // waits for.
        // SAFETY: our own window. The result says whether it was visible before.
        let _ = unsafe { ShowWindow(hwnd, SW_HIDE) };
        close();
    }
    if !GONE_SINCE_PHOTO.replace(false) {
        return;
    }
    // SAFETY: no arguments.
    if let Err(e) = unsafe { DwmFlush() } {
        log::failure("tray menu: waiting for the screen without it", &e);
    }
}

/// Whether `hwnd` is the menu.
pub fn is(hwnd: HWND) -> bool {
    WINDOW.get() == Some(hwnd)
}

/// Whether the menu is open.
pub fn is_open() -> bool {
    WINDOW.get().is_some()
}

/// Posts `OPENED_OR_CLOSED` to `resident`. A failure leaves the tray icon's tip as it was until the next one.
fn tell_resident(resident: HWND) {
    // SAFETY: the resident window, on this thread; the message carries nothing.
    if let Err(e) = unsafe { PostMessageW(Some(resident), OPENED_OR_CLOSED, WPARAM(0), LPARAM(0)) }
    {
        log::failure("tray menu: telling the tray icon it opened or closed", &e);
    }
}

/// The item a `CHOSEN` message carries.
pub fn chosen(wparam: WPARAM) -> Option<Item> {
    layout::ITEMS.get(wparam.0).copied()
}

/// The work area of the monitor at `point`, or the nearest one, without the taskbar, and that monitor's DPI.
fn monitor_at(point: POINT) -> windows::core::Result<(RECT, u32)> {
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

extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        // Direct2D paints every pixel; erasing first would only flash.
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => {
            paint(hwnd);
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            track_leave(hwnd);
            light(hwnd, item_at(hwnd, lparam));
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            TRACKING_LEAVE.set(false);
            // A held button keeps the mouse here (SetCapture) and its item stays as it is.
            if PRESSED.get().is_none() {
                light(hwnd, None);
            }
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            if let Some(item) = item_at(hwnd, lparam) {
                PRESSED.set(Some(item));
                // SAFETY: our own window; the release comes here wherever the mouse is.
                unsafe { SetCapture(hwnd) };
            }
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            if let Some(pressed) = PRESSED.take() {
                // SAFETY: ends the capture taken above. Its WM_CAPTURECHANGED finds nothing held any more.
                if let Err(e) = unsafe { ReleaseCapture() } {
                    log::failure("tray menu: letting the mouse go", &e);
                }
                if item_at(hwnd, lparam) == Some(pressed) {
                    choose(hwnd, pressed);
                }
            }
            LRESULT(0)
        }
        WM_CAPTURECHANGED => {
            // Another window took the mouse while the button was held: nothing is chosen.
            PRESSED.set(None);
            LRESULT(0)
        }
        WM_KEYDOWN => {
            match u16::try_from(wparam.0).map(VIRTUAL_KEY) {
                Ok(VK_ESCAPE) => close(),
                Ok(VK_DOWN) => light(hwnd, Some(layout::next(LIT.get(), layout::Direction::Down))),
                Ok(VK_UP) => light(hwnd, Some(layout::next(LIT.get(), layout::Direction::Up))),
                Ok(VK_RETURN | VK_SPACE) => {
                    if let Some(item) = LIT.get() {
                        choose(hwnd, item);
                    }
                }
                _ => {}
            }
            LRESULT(0)
        }
        // A click anywhere else gives the front to another window: the menu closes once this message is answered, as
        // Windows advises against destroying a window while it is being deactivated.
        WM_ACTIVATE if low_word(LPARAM(wparam.0.cast_signed())) == WA_INACTIVE => {
            // SAFETY: our own window; the message carries nothing.
            if let Err(e) = unsafe { PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0)) } {
                log::failure("tray menu: closing it as it lost the front", &e);
                close();
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            if WINDOW.get() == Some(hwnd) {
                WINDOW.set(None);
                GONE_SINCE_PHOTO.set(true);
                drop(TARGET.take());
                LIT.set(None);
                PRESSED.set(None);
                TRACKING_LEAVE.set(false);
                // Every way the menu closes ends here: Esc, a click elsewhere, an item chosen, a call, an exit.
                if let Some(resident) = RESIDENT.get() {
                    tell_resident(resident);
                }
            }
            LRESULT(0)
        }
        // SAFETY: the arguments are the ones Windows passed to this window procedure.
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}

/// The item under a mouse message's point.
fn item_at(hwnd: HWND, lparam: LPARAM) -> Option<Item> {
    let (x, y) = signed_words(lparam.0.cast_unsigned());
    let point = POINT {
        x: i32::from(x),
        y: i32::from(y),
    };
    // SAFETY: our own window.
    layout::item_at(point, unsafe { GetDpiForWindow(hwnd) })
}

/// Tints `item`, and draws the menu again when that changed.
fn light(hwnd: HWND, item: Option<Item>) {
    if LIT.replace(item) != item {
        // SAFETY: our own window.
        if !unsafe { InvalidateRect(Some(hwnd), None, false) }.as_bool() {
            log::failure(
                "tray menu: drawing it again",
                &"InvalidateRect returned FALSE",
            );
        }
    }
}

/// Sends `item` to the resident window and closes the menu, which is gone by the time the resident window acts.
fn choose(hwnd: HWND, item: Item) {
    match RESIDENT.get() {
        Some(resident) => {
            let item = WPARAM(item.index());
            // SAFETY: the resident window, on this thread; the message carries the item's place in the list.
            if let Err(e) = unsafe { PostMessageW(Some(resident), CHOSEN, item, LPARAM(0)) } {
                log::failure("tray menu: passing on the item chosen", &e);
            }
        }
        None => log::failure(
            "tray menu: passing on the item chosen",
            &"no window to tell",
        ),
    }
    // SAFETY: our own window, on the thread that created it.
    if let Err(e) = unsafe { DestroyWindow(hwnd) } {
        log::failure("tray menu: closing it", &e);
    }
}

/// Asks Windows for WM_MOUSELEAVE when the cursor leaves the menu, once each time it comes in.
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
        // The item may stay tinted after the cursor leaves; the next move over the menu asks again.
        Err(e) => log::failure("tray menu: following the cursor out of it", &e),
    }
}

fn paint(hwnd: HWND) {
    let mut paint = PAINTSTRUCT::default();
    // SAFETY: WM_PAINT on our own window, ended below. Direct2D draws without the device context it returns.
    unsafe { BeginPaint(hwnd, &mut paint) };
    if let Err(e) = draw(hwnd) {
        log::failure("tray menu: drawing it", &e);
    }
    // SAFETY: ends the BeginPaint above. EndPaint always returns TRUE.
    let _ = unsafe { EndPaint(hwnd, &paint) };
}

/// Draws the menu at the window's scale.
fn draw(hwnd: HWND) -> windows::core::Result<()> {
    // SAFETY: our own window.
    if !unsafe { IsWindowVisible(hwnd) }.as_bool() {
        return Ok(());
    }
    // SAFETY: our own window.
    let dpi = unsafe { GetDpiForWindow(hwnd) };
    let size = layout::size_at(dpi);
    let target = match TARGET.take() {
        Some(target) => target,
        None => graphics::window_target(
            hwnd,
            D2D_SIZE_U {
                width: size.cx.max(0).cast_unsigned(),
                height: size.cy.max(0).cast_unsigned(),
            },
        )?,
    };
    let fonts = graphics::fonts()?;
    // SAFETY: drawing calls on a live render target, between BeginDraw and EndDraw.
    unsafe { target.BeginDraw() };
    let drawn = paint::draw(&target, &fonts, MODE.get(), dpi, LIT.get(), CHECKED.get());
    // SAFETY: ends the BeginDraw above.
    match unsafe { target.EndDraw(None, None) } {
        Ok(()) => TARGET.set(Some(target)),
        // The graphics device was lost, as after a driver update: the next paint makes the target again.
        Err(e) if e.code() == D2DERR_RECREATE_TARGET => {
            if let Err(inside) = drawn {
                log::failure("tray menu: drawing it", &inside);
            }
            // SAFETY: our own window.
            if !unsafe { InvalidateRect(Some(hwnd), None, false) }.as_bool() {
                return Err(Error::new(E_FAIL, "InvalidateRect returned FALSE"));
            }
            return Ok(());
        }
        Err(e) => {
            // The frame failed as a whole; what failed inside it, if anything, is logged so it isn't lost.
            if let Err(inside) = drawn {
                log::failure("tray menu: drawing it", &inside);
            }
            return Err(e);
        }
    }
    drawn
}
