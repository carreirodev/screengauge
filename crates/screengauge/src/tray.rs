//! The tray icon next to the clock, drawn with the app icon. The notification area draws an icon at one size, the
//! taskbar's small-icon size at the taskbar's DPI (16 pixels at 100% scale, 28 at 175%), and scales any other. The
//! app icon has an image for each of those sizes, so ScreenGauge loads the one the taskbar draws, and Windows shows its
//! pixels as they are. When the scale changes, the icon follows the taskbar to the new size (`follow_scale`).
//!
//! Measured on Windows 11 in WPC-304: after a change of scale while ScreenGauge runs, Explorer may go on drawing the
//! icon it already holds at the old size, enlarged or shrunk, even when given the new size's picture; an icon it adds
//! afresh once the taskbar has settled is drawn pixel for pixel. So a new size means adding the icon again.

use std::cell::{Cell, RefCell};

use windows::Win32::Foundation::{E_FAIL, HWND, LPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{GetDpiForSystem, GetDpiForWindow, GetSystemMetricsForDpi};
use windows::Win32::UI::Shell::{
    NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_REALTIME, NIF_SHOWTIP, NIF_TIP, NIM_ADD, NIM_DELETE,
    NIM_MODIFY, NIM_SETVERSION, NIN_SELECT, NINF_KEY, NOTIFY_ICON_MESSAGE, NOTIFYICON_VERSION_4,
    NOTIFYICONDATAW, Shell_NotifyIconW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DestroyIcon, FindWindowW, HICON, IMAGE_ICON, KillTimer, LR_DEFAULTCOLOR, LoadImageW,
    SM_CXSMICON, SetTimer, WM_APP,
};
use windows::core::{Error, PCWSTR, w};

use crate::log;
use crate::win32::{low_word, struct_size};

/// What Windows sends the resident window for the icon's clicks. With version 4, the event (such as
/// `WM_CONTEXTMENU` for a right click) is the low word of lParam.
pub const CALLBACK_MESSAGE: u32 = WM_APP + 1;

/// One icon per process, so its ID is a constant.
const ICON_ID: u32 = 1;

/// The windows crate has NIN_SELECT and NINF_KEY but not their combination, which Windows' headers define this way.
const NIN_KEYSELECT: u32 = NIN_SELECT | NINF_KEY;

/// The ID of the exe's icon group, which build.rs writes.
const APP_ICON_ID: usize = 1;

/// The resident window's timer that follows the taskbar after a change of scale (`follow_scale`). It runs only then:
/// at rest ScreenGauge has no timer.
pub const SCALE_TIMER: usize = 1;
/// How often the icon checks the taskbar's size while following it, and how many times at most: eight seconds.
const SCALE_CHECK_MS: u32 = 200;
const SCALE_CHECKS: u32 = 40;
/// How many checks in a row must see the new size before the icon is added again: one second, for Explorer to finish
/// taking the new scale.
const SETTLE_CHECKS: u32 = 5;

thread_local! {
    /// The icon Windows shows, kept alive for as long as it shows it. Only the resident thread touches it. Nothing
    /// holds a borrow across a call to Shell_NotifyIconW, which waits for Explorer and so may let a message reach this
    /// thread.
    static ICON: RefCell<Option<AppIcon>> = const { RefCell::new(None) };
    /// While the icon follows the taskbar, where it stands; `None` when the scale timer isn't running.
    static FOLLOWING: Cell<Option<Following>> = const { Cell::new(None) };
}

/// An icon loaded from the exe's resources, destroyed when dropped.
struct AppIcon {
    handle: HICON,
    /// The width and height it was loaded at, in pixels.
    size: i32,
}

impl AppIcon {
    /// The app icon's image of `size` pixels. The size is exact: Windows scales an icon only when the group has no
    /// image of it. LoadImageW comes from user32, since LoadIconMetric needs comctl32 version 6, which ScreenGauge
    /// doesn't activate.
    fn load(size: i32) -> windows::core::Result<Self> {
        // SAFETY: no module name, so this is the handle of ScreenGauge.exe itself.
        let module = unsafe { GetModuleHandleW(None) }?;
        // SAFETY: MAKEINTRESOURCE of the icon group, in this exe's own module.
        let handle = unsafe {
            LoadImageW(
                Some(module.into()),
                PCWSTR(std::ptr::without_provenance(APP_ICON_ID)),
                IMAGE_ICON,
                size,
                size,
                LR_DEFAULTCOLOR,
            )
        }?;
        Ok(Self {
            handle: HICON(handle.0),
            size,
        })
    }
}

impl Drop for AppIcon {
    fn drop(&mut self) {
        // SAFETY: the icon came from LoadImageW, and only this owner destroys it.
        if let Err(e) = unsafe { DestroyIcon(self.handle) } {
            log::failure("tray icon: freeing an icon", &e);
        }
    }
}

/// Adds the icon, at the size the taskbar draws it at now. Also called again when the taskbar is created (see
/// `resident`), after Explorer has dropped every icon: the icon it replaces is freed once the new one is in.
pub fn add(hwnd: HWND) -> windows::core::Result<()> {
    let mut data = icon_data(hwnd);
    data.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP | NIF_SHOWTIP;
    data.uCallbackMessage = CALLBACK_MESSAGE;
    if !fill(&mut data.szTip, "ScreenGauge") {
        return Err(Error::new(E_FAIL, "the tooltip is too long"));
    }
    let icon = AppIcon::load(icon_size()?)?;
    data.hIcon = icon.handle;
    // A failure drops the new icon and keeps the old one, whose size then still differs from the taskbar's, so
    // following the scale tries again.
    notify(NIM_ADD, &data, "NIM_ADD")?;
    drop(ICON.replace(Some(icon)));
    data.Anonymous.uVersion = NOTIFYICON_VERSION_4;
    notify(NIM_SETVERSION, &data, "NIM_SETVERSION")
}

/// Windows announced a change that may give the taskbar another scale. The resident window, which is never shown,
/// hears of it through WM_SETTINGCHANGE and WM_DISPLAYCHANGE, never WM_DPICHANGED, and 400 to 500 ms before the
/// taskbar takes the new scale (measured in WPC-304). So the icon follows the taskbar for a few seconds, every
/// `SCALE_CHECK_MS`: once the taskbar has held a new size for `SETTLE_CHECKS` checks, the icon is added again at it.
pub fn follow_scale(hwnd: HWND) -> windows::core::Result<()> {
    let started = Following {
        checks_left: SCALE_CHECKS,
        settled: 0,
    };
    if FOLLOWING.replace(Some(started)).is_none() {
        // SAFETY: our own window, on the thread that created it; with no callback, WM_TIMER comes to the window.
        if unsafe { SetTimer(Some(hwnd), SCALE_TIMER, SCALE_CHECK_MS, None) } == 0 {
            FOLLOWING.set(None);
            return Err(Error::from_thread());
        }
    }
    Ok(())
}

/// The scale timer fired: checks the taskbar's size, adds the icon again once a new size has settled, and stops
/// following once it did or the checks ran out.
pub fn on_scale_timer(hwnd: HWND) -> windows::core::Result<()> {
    let Some(mut following) = FOLLOWING.get() else {
        return Ok(());
    };
    // Without the icon there is nothing to follow: it comes back with TaskbarCreated, at the size of that moment.
    let Some(shown) = ICON.with_borrow(|icon| icon.as_ref().map(|icon| icon.size)) else {
        return stop_following(hwnd);
    };
    let taskbar = match icon_size() {
        Ok(size) => size,
        Err(e) => {
            stop_following(hwnd)?;
            return Err(e);
        }
    };
    let next = following.check(shown, taskbar);
    FOLLOWING.set(Some(following));
    match next {
        Next::Wait => Ok(()),
        Next::Stop => stop_following(hwnd),
        // A failure keeps following, so the next check tries again until the checks run out.
        Next::AddAgain => add_again(hwnd).and_then(|()| stop_following(hwnd)),
    }
}

/// Where following the taskbar stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Following {
    checks_left: u32,
    /// How many checks in a row saw the taskbar at another size than the icon's.
    settled: u32,
}

/// What follows a check of the taskbar's size.
#[derive(Debug, PartialEq, Eq)]
enum Next {
    Wait,
    AddAgain,
    Stop,
}

impl Following {
    /// Counts a check that found the icon at `shown` pixels and the taskbar at `taskbar`.
    fn check(&mut self, shown: i32, taskbar: i32) -> Next {
        self.checks_left = self.checks_left.saturating_sub(1);
        self.settled = if taskbar == shown {
            0
        } else {
            self.settled + 1
        };
        if self.settled >= SETTLE_CHECKS {
            Next::AddAgain
        } else if self.checks_left == 0 {
            Next::Stop
        } else {
            Next::Wait
        }
    }
}

fn stop_following(hwnd: HWND) -> windows::core::Result<()> {
    if FOLLOWING.take().is_none() {
        return Ok(());
    }
    // SAFETY: the timer `follow_scale` set on our own window.
    unsafe { KillTimer(Some(hwnd), SCALE_TIMER) }
}

/// Removes the icon and adds it at the taskbar's size, so Explorer draws it afresh (see the module's notes). A Windows
/// notification showing at that moment goes with the old icon.
fn add_again(hwnd: HWND) -> windows::core::Result<()> {
    // When the removal fails, the icon may already be gone: adding it is what matters, and its failure is returned.
    if let Err(e) = notify(NIM_DELETE, &icon_data(hwnd), "NIM_DELETE") {
        log::failure("tray icon: removing it to add it again", &e);
    }
    add(hwnd)
}

/// Removes the icon, and frees it whether or not Windows took the removal: ScreenGauge is exiting.
pub fn remove(hwnd: HWND) -> windows::core::Result<()> {
    let removed = notify(NIM_DELETE, &icon_data(hwnd), "NIM_DELETE");
    drop(ICON.take());
    removed
}

/// The pixel size the taskbar draws its icons at now.
fn icon_size() -> windows::core::Result<i32> {
    // The system's DPI when there is no taskbar to ask.
    let dpi = taskbar_dpi().unwrap_or_else(|| {
        // SAFETY: no arguments.
        unsafe { GetDpiForSystem() }
    });
    match small_icon_size(dpi) {
        size if size > 0 => Ok(size),
        size => Err(Error::new(
            E_FAIL,
            format!("the small icon size at {dpi} DPI is {size}"),
        )),
    }
}

/// The DPI of the primary taskbar, whose notification area holds the icon. `None` when there is no taskbar to ask: at
/// sign-in it may not exist yet, and while Explorer restarts it is gone for a moment.
fn taskbar_dpi() -> Option<u32> {
    // SAFETY: a valid class name and no window name. No such window is the expected failure here, and the caller
    // falls back to the system's DPI.
    let taskbar = unsafe { FindWindowW(w!("Shell_TrayWnd"), PCWSTR::null()) }.ok()?;
    // SAFETY: a window handle; the DPI is 0 if the window closed since FindWindowW found it.
    match unsafe { GetDpiForWindow(taskbar) } {
        0 => None,
        dpi => Some(dpi),
    }
}

/// The small icon size (SM_CXSMICON) at `dpi`: 16, 20, 24, 28 and 32 pixels at 96, 120, 144, 168 and 192 DPI, which are
/// the scales 100%, 125%, 150%, 175% and 200%.
fn small_icon_size(dpi: u32) -> i32 {
    // SAFETY: no pointers.
    unsafe { GetSystemMetricsForDpi(SM_CXSMICON, dpi) }
}

/// Shows `text` as a Windows notification from the icon: Windows 11 shows a tray icon's balloon as one of its own
/// notifications (ADR 0003), and its events come back as `CALLBACK_MESSAGE`. With `right_away_only`, Windows drops it
/// when it can't show it at once. Fails when the icon isn't in the notification area, or when `text` is too long.
pub fn balloon(hwnd: HWND, text: &str, right_away_only: bool) -> windows::core::Result<()> {
    let mut data = icon_data(hwnd);
    data.uFlags = NIF_INFO;
    if right_away_only {
        data.uFlags |= NIF_REALTIME;
    }
    if !fill(&mut data.szInfo, text) {
        return Err(Error::new(
            E_FAIL,
            format!("the notification \"{text}\" is too long"),
        ));
    }
    notify(NIM_MODIFY, &data, "NIM_MODIFY")
}

/// Whether `text` fits in a balloon notification. The notifications' tests check their texts with it.
#[cfg(test)]
pub fn fits_in_a_balloon(text: &str) -> bool {
    fill(&mut icon_data(HWND::default()).szInfo, text)
}

/// Copies `text` into a null-terminated buffer, or returns false, leaving the buffer as it was, when it doesn't fit.
fn fill(buffer: &mut [u16], text: &str) -> bool {
    let units: Vec<u16> = text.encode_utf16().chain([0]).collect();
    let Some(start) = buffer.get_mut(..units.len()) else {
        return false;
    };
    start.copy_from_slice(&units);
    true
}

/// Whether a `CALLBACK_MESSAGE` calls ScreenGauge: a left click, which Windows reports once as NIN_SELECT when the
/// button is released, or Enter or Space while the keyboard is on the icon. A double click is a first click that calls
/// and a second one that lands on the frozen screen, which by then covers the taskbar.
pub fn is_call(lparam: LPARAM) -> bool {
    matches!(low_word(lparam), NIN_SELECT | NIN_KEYSELECT)
}

/// What names the icon to Windows: its window and ID.
fn icon_data(hwnd: HWND) -> NOTIFYICONDATAW {
    NOTIFYICONDATAW {
        cbSize: const { struct_size::<NOTIFYICONDATAW>() },
        hWnd: hwnd,
        uID: ICON_ID,
        ..Default::default()
    }
}

/// Shell_NotifyIconW returns only FALSE and documents no error code, so the error names the message.
fn notify(
    message: NOTIFY_ICON_MESSAGE,
    data: &NOTIFYICONDATAW,
    message_name: &str,
) -> windows::core::Result<()> {
    // SAFETY: `data` starts from `icon_data`, so it is initialized and its cbSize matches its type.
    if unsafe { Shell_NotifyIconW(message, data) }.as_bool() {
        Ok(())
    } else {
        Err(Error::new(
            E_FAIL,
            format!("Shell_NotifyIconW({message_name}) returned FALSE"),
        ))
    }
}

#[cfg(test)]
mod tests {
    use windows::Win32::UI::WindowsAndMessaging::{
        WM_CONTEXTMENU, WM_LBUTTONDBLCLK, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE,
    };

    use super::*;

    /// The lParam Windows sends with version 4: the event in the low word, the icon's ID in the high word.
    fn event(event: u32) -> LPARAM {
        LPARAM(isize::try_from(ICON_ID << 16 | event).expect("fits in an isize"))
    }

    #[test]
    fn a_left_click_calls() {
        // NIN_SELECT, which Windows sends once the button is released.
        assert!(is_call(event(0x400)));
    }

    #[test]
    fn selecting_the_icon_from_the_keyboard_calls() {
        // NIN_KEYSELECT: Enter or Space while the keyboard is on the icon.
        assert!(is_call(event(0x401)));
    }

    #[test]
    fn the_button_messages_around_a_click_dont_call_again() {
        for button in [WM_LBUTTONDOWN, WM_LBUTTONUP, WM_LBUTTONDBLCLK] {
            assert!(!is_call(event(button)), "{button:#x}");
        }
    }

    #[test]
    fn a_balloon_holds_255_characters_and_no_more() {
        assert!(fits_in_a_balloon(&"x".repeat(255)));
        assert!(!fits_in_a_balloon(&"x".repeat(256)));
    }

    #[test]
    fn text_that_doesnt_fit_leaves_the_buffer_as_it_was() {
        let mut buffer = [7; 4];
        assert!(!fill(&mut buffer, "abcd"));
        assert_eq!(buffer, [7; 4]);
        assert!(fill(&mut buffer, "abc"));
        assert_eq!(buffer, [97, 98, 99, 0]);
    }

    fn following() -> Following {
        Following {
            checks_left: SCALE_CHECKS,
            settled: 0,
        }
    }

    #[test]
    fn the_icon_is_added_again_once_a_new_taskbar_size_held_for_a_second() {
        let mut state = following();
        // The announcement comes before the taskbar changes: the first checks still see the old size.
        assert_eq!(state.check(20, 20), Next::Wait);
        assert_eq!(state.check(20, 20), Next::Wait);
        for _ in 1..SETTLE_CHECKS {
            assert_eq!(state.check(20, 24), Next::Wait);
        }
        assert_eq!(state.check(20, 24), Next::AddAgain);
    }

    #[test]
    fn a_size_that_goes_back_starts_settling_again() {
        let mut state = following();
        for _ in 1..SETTLE_CHECKS {
            state.check(20, 24);
        }
        assert_eq!(state.check(20, 20), Next::Wait);
        assert_eq!(state.check(20, 24), Next::Wait);
    }

    #[test]
    fn following_stops_when_the_checks_run_out_without_a_new_size() {
        let mut state = following();
        for _ in 1..SCALE_CHECKS {
            assert_eq!(state.check(28, 28), Next::Wait);
        }
        assert_eq!(state.check(28, 28), Next::Stop);
    }

    #[test]
    fn the_icon_is_drawn_at_the_size_the_taskbar_draws_at_each_common_scale() {
        for (dpi, size) in [(96, 16), (120, 20), (144, 24), (168, 28), (192, 32)] {
            assert_eq!(small_icon_size(dpi), size, "{dpi} DPI");
        }
    }

    /// The same call the tray makes, in the process's own module, so an icon group with another ID than `APP_ICON_ID`
    /// fails here.
    #[test]
    fn the_app_icon_loads_at_every_size_the_tray_asks_for() {
        for size in [16, 20, 24, 28, 32] {
            let icon = AppIcon::load(size).unwrap_or_else(|e| panic!("loading at {size}: {e}"));
            assert_eq!(icon.size, size);
        }
    }

    #[test]
    fn a_right_click_or_the_mouse_passing_over_doesnt_call() {
        assert!(!is_call(event(WM_CONTEXTMENU)));
        assert!(!is_call(event(WM_MOUSEMOVE)));
    }
}
