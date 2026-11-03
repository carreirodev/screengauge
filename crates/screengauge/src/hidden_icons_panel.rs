//! The hidden icons panel: the window Windows 11 opens above the taskbar's ^, holding the tray icons that aren't on
//! the taskbar. A click on ScreenGauge's tray icon inside it leaves it open, over what the user is about to measure,
//! so the panel is hidden before the photo (WPC-329). Windows offers no way to close it: ScreenGauge finds it by its
//! window class and hides it with ShowWindow. The owner accepted that a Windows update may break this; when it fails,
//! the photo has the panel in it, and nothing else happens.
//!
//! Measured in WPC-329 on Windows 11: the panel is gone from the screen after one DwmFlush (14 ms), it doesn't come
//! back on its own, and the ^ opens it again on its first click, with or without another window taking the front
//! meanwhile.

use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::Graphics::Dwm::DwmFlush;
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowExW, GetWindowRect, IsWindowVisible, SW_HIDE, ShowWindow,
};
use windows::core::{PCWSTR, w};

use crate::log;
use crate::win32::contains;

/// The panel's window class on Windows 11.
const CLASS: PCWSTR = w!("TopLevelWindowForOverflowXamlIsland");

/// Hides the open panel that holds `anchor`, where Windows says the tray icon was clicked, and waits for the screen to
/// be drawn without it. When no open panel holds it, as for a click on the icon on the taskbar, nothing happens and
/// there is no wait.
pub fn hide_holding(anchor: POINT) {
    let Some(panel) = open_panel_holding(anchor) else {
        return;
    };
    // SAFETY: a window handle; a window of another process is hidden the same way. The result says whether it was
    // visible before: if it closed on its own meanwhile, there is nothing to wait for.
    if !unsafe { ShowWindow(panel, SW_HIDE) }.as_bool() {
        return;
    }
    // SAFETY: no arguments.
    if let Err(e) = unsafe { DwmFlush() } {
        log::failure("hidden icons panel: waiting for the screen without it", &e);
    }
}

/// The visible panel window that holds `point`. Each monitor's taskbar may have a panel of its own, so every window of
/// the class is looked at.
fn open_panel_holding(point: POINT) -> Option<HWND> {
    let mut after = None;
    loop {
        // SAFETY: a valid class name and no window name; `after` is a window this loop found. No more windows of the
        // class is how the loop ends.
        let panel = unsafe { FindWindowExW(None, after, CLASS, PCWSTR::null()) }.ok()?;
        if is_open(panel) && rect_of(panel).is_some_and(|rect| contains(&rect, point)) {
            return Some(panel);
        }
        after = Some(panel);
    }
}

fn is_open(panel: HWND) -> bool {
    // SAFETY: a window handle, only read.
    unsafe { IsWindowVisible(panel) }.as_bool()
}

/// Where the window is on screen; `None` when it closed since it was found, and then it holds nothing.
fn rect_of(panel: HWND) -> Option<RECT> {
    let mut rect = RECT::default();
    // SAFETY: fills a local.
    unsafe { GetWindowRect(panel, &mut rect) }.ok()?;
    Some(rect)
}

#[cfg(test)]
mod tests {
    use windows::Win32::Foundation::{HINSTANCE, LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, IsWindowVisible, RegisterClassExW,
        SW_SHOWNOACTIVATE, ShowWindow, WNDCLASSEXW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_POPUP,
    };

    use super::*;
    use crate::win32::struct_size;

    extern "system" fn default_proc(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        // SAFETY: the arguments are the ones Windows passed to this window procedure.
        unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
    }

    /// Where the stand-in panel sits: far outside any monitor, so it never shows on screen and Explorer's own panel,
    /// above the taskbar, never holds these points.
    const LEFT: i32 = -30_000;
    const TOP: i32 = -30_000;
    const SIZE: i32 = 300;

    /// A visible window of the panel's class, standing in for Explorer's. One test makes it, since a class registers
    /// once per process.
    fn stand_in_panel() -> HWND {
        // SAFETY: no module name, so this is the handle of the test executable.
        let instance: HINSTANCE = unsafe { GetModuleHandleW(None) }
            .expect("GetModuleHandleW")
            .into();
        let class = WNDCLASSEXW {
            cbSize: const { struct_size::<WNDCLASSEXW>() },
            lpfnWndProc: Some(default_proc),
            hInstance: instance,
            lpszClassName: CLASS,
            ..Default::default()
        };
        // SAFETY: `class` is fully initialized, and its name is a static string.
        let registered = unsafe { RegisterClassExW(&class) };
        assert_ne!(registered, 0, "registering the panel's class");
        // SAFETY: the class was just registered; the window never takes the front.
        let panel = unsafe {
            CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                CLASS,
                None,
                WS_POPUP,
                LEFT,
                TOP,
                SIZE,
                SIZE,
                None,
                None,
                Some(instance),
                None,
            )
        }
        .expect("CreateWindowExW");
        // SAFETY: the test's own window. The result says whether it was visible before.
        let _ = unsafe { ShowWindow(panel, SW_SHOWNOACTIVATE) };
        panel
    }

    fn is_visible(window: HWND) -> bool {
        // SAFETY: a window handle, only read.
        unsafe { IsWindowVisible(window) }.as_bool()
    }

    #[test]
    fn a_click_inside_the_open_panel_hides_it_and_one_elsewhere_leaves_it() {
        let panel = stand_in_panel();
        assert!(is_visible(panel));

        // On the taskbar, or anywhere the panel doesn't cover: its right and bottom edges are outside it.
        for outside in [
            POINT {
                x: LEFT - 1,
                y: TOP + 10,
            },
            POINT {
                x: LEFT + SIZE,
                y: TOP + 10,
            },
            POINT {
                x: LEFT + 10,
                y: TOP + SIZE,
            },
        ] {
            hide_holding(outside);
            assert!(is_visible(panel), "a click at {outside:?} hid the panel");
        }

        hide_holding(POINT {
            x: LEFT + SIZE - 1,
            y: TOP + SIZE - 1,
        });
        assert!(!is_visible(panel), "the click inside left the panel open");

        // SAFETY: the test's own window, on the thread that created it.
        unsafe { DestroyWindow(panel) }.expect("DestroyWindow");
    }
}
