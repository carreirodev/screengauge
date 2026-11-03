//! The screen as the routine uses it: the monitors, the stage laid under the toolbar's zone, reading a few of the
//! screen's pixels, the cursor, the global shortcut pressed with real key events, and waiting while this thread's
//! windows keep answering.

use std::time::{Duration, Instant};

use screengauge_core::session::{Point, Rect};
use screengauge_core::settings::{Key, Shortcut};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WAIT_FAILED, WPARAM};
use windows::Win32::Graphics::Dwm::{
    DWMWA_TRANSITIONS_FORCEDISABLED, DwmFlush, DwmSetWindowAttribute,
};
use windows::Win32::Graphics::Gdi::{
    BITMAPINFO, BITMAPINFOHEADER, BitBlt, CAPTUREBLT, CreateCompatibleBitmap, CreateCompatibleDC,
    CreateSolidBrush, DIB_RGB_COLORS, DeleteDC, DeleteObject, EnumDisplayMonitors, GetDC,
    GetDIBits, GetMonitorInfoW, HBITMAP, HDC, HGDIOBJ, HMONITOR, MONITORINFO, MONITORINFOEXW,
    ROP_CODE, ReleaseDC, SRCCOPY, SelectObject, UpdateWindow,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForMonitor, MDT_EFFECTIVE_DPI,
    SetThreadDpiAwarenessContext,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_KEYUP, SendInput,
    VIRTUAL_KEY, VK_CONTROL, VK_F1, VK_LWIN, VK_MENU, VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, HWND_TOPMOST,
    MONITORINFOF_PRIMARY, MSG, MsgWaitForMultipleObjects, PM_REMOVE, PeekMessageW, QS_ALLINPUT,
    RegisterClassExW, SW_HIDE, SWP_NOACTIVATE, SWP_SHOWWINDOW, SetCursorPos, SetWindowPos,
    ShowWindow, TranslateMessage, UnregisterClassW, WNDCLASSEXW, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::{BOOL, PCWSTR, w};

use crate::{struct_size, wide_text};

/// Makes this thread see physical pixels on every monitor, as ScreenGauge does (Per-Monitor V2).
pub fn see_physical_pixels() -> Result<(), String> {
    // SAFETY: a documented context constant; the call changes only this thread.
    let before =
        unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
    if before.is_invalid() {
        return Err("SetThreadDpiAwarenessContext(Per-Monitor V2) failed".to_string());
    }
    Ok(())
}

/// A monitor as Windows shows it now.
#[derive(Debug, Clone)]
pub struct Monitor {
    pub device: String,
    pub rect: Rect,
    pub dpi: u32,
    pub primary: bool,
}

pub fn monitors() -> Result<Vec<Monitor>, String> {
    let mut found: Vec<Result<Monitor, String>> = Vec::new();
    // SAFETY: `found` outlives the call and `each` uses it only during it.
    unsafe { EnumDisplayMonitors(None, None, Some(each), LPARAM((&raw mut found) as isize)) }
        .ok()
        .map_err(|e| format!("EnumDisplayMonitors: {e}"))?;
    found.into_iter().collect()
}

extern "system" fn each(monitor: HMONITOR, _: HDC, _: *mut RECT, lparam: LPARAM) -> BOOL {
    // SAFETY: `monitors` passes a pointer to its own vector, alive for the whole enumeration.
    let found = unsafe { &mut *(lparam.0 as *mut Vec<Result<Monitor, String>>) };
    found.push(describe(monitor));
    BOOL::from(true)
}

fn describe(monitor: HMONITOR) -> Result<Monitor, String> {
    let mut info = MONITORINFOEXW::default();
    info.monitorInfo.cbSize = const { struct_size::<MONITORINFOEXW>() };
    // SAFETY: `info` is a MONITORINFOEXW whose cbSize says so.
    unsafe { GetMonitorInfoW(monitor, (&raw mut info).cast::<MONITORINFO>()) }
        .ok()
        .map_err(|e| format!("GetMonitorInfoW: {e}"))?;
    let (mut dpi, mut unused) = (0, 0);
    // SAFETY: a monitor EnumDisplayMonitors gave, and two u32 for the call to fill.
    unsafe { GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi, &mut unused) }
        .map_err(|e| format!("GetDpiForMonitor: {e}"))?;
    let r = info.monitorInfo.rcMonitor;
    Ok(Monitor {
        device: wide_text(&info.szDevice),
        rect: Rect {
            left: r.left,
            top: r.top,
            width: u32::try_from(r.right - r.left).map_err(|e| e.to_string())?,
            height: u32::try_from(r.bottom - r.top).map_err(|e| e.to_string())?,
        },
        dpi,
        primary: info.monitorInfo.dwFlags & MONITORINFOF_PRIMARY != 0,
    })
}

/// Handles this thread's window messages until `duration` has passed, sleeping in between.
pub fn wait(duration: Duration) {
    let deadline = Instant::now() + duration;
    loop {
        pump();
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return;
        }
        // Waits past 49 days are cut to the longest Windows takes; the loop waits again.
        let ms = u32::try_from(left.as_millis()).unwrap_or(u32::MAX - 1);
        // SAFETY: no handles; it returns on a message for this thread or after `ms`.
        let woke = unsafe { MsgWaitForMultipleObjects(None, false, ms, QS_ALLINPUT) };
        if woke == WAIT_FAILED {
            panic!(
                "MsgWaitForMultipleObjects failed: {}",
                windows::core::Error::from_thread()
            );
        }
    }
}

/// Handles every window message waiting for this thread.
pub fn pump() {
    let mut message = MSG::default();
    // SAFETY: `message` is a valid MSG for each call to fill, and only messages just taken are dispatched.
    unsafe {
        while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}

const STAGE_CLASS: PCWSTR = w!("ScreenGauge.GoalsStage");

/// A topmost window of one color, never active, laid under the toolbar's zone so the toolbar's pixels can be told
/// from what was there before.
pub struct Stage {
    hwnd: HWND,
    pub color: [u8; 3],
}

extern "system" fn stage_proc(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // SAFETY: the stage's own messages, handled the default way: the class brush paints it.
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}

impl Stage {
    pub fn new(color: [u8; 3]) -> Result<Stage, String> {
        let [r, g, b] = color.map(u32::from);
        // SAFETY: a solid brush for the class, which Windows deletes when the class is unregistered.
        let brush = unsafe { CreateSolidBrush(COLORREF(r | g << 8 | b << 16)) };
        if brush.is_invalid() {
            return Err("CreateSolidBrush failed".to_string());
        }
        // SAFETY: no module name, so the handle of this test program.
        let instance = unsafe { GetModuleHandleW(None) }.map_err(|e| e.to_string())?;
        let class = WNDCLASSEXW {
            cbSize: const { struct_size::<WNDCLASSEXW>() },
            lpfnWndProc: Some(stage_proc),
            hInstance: instance.into(),
            hbrBackground: brush,
            lpszClassName: STAGE_CLASS,
            ..Default::default()
        };
        // SAFETY: `class` is fully initialized, with a static name; the class owns the brush from here on.
        if unsafe { RegisterClassExW(&class) } == 0 {
            // SAFETY: the brush was made above and nothing uses it.
            unsafe {
                let _ = DeleteObject(HGDIOBJ(brush.0));
            }
            return Err("RegisterClassExW(stage) failed".to_string());
        }
        // SAFETY: the class was just registered; the window is shown only by `show`.
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                STAGE_CLASS,
                w!("ScreenGauge goals stage"),
                WS_POPUP,
                0,
                0,
                1,
                1,
                None,
                None,
                Some(instance.into()),
                None,
            )
        }
        .map_err(|e| {
            // SAFETY: the class registered above, with no window; unregistering it deletes its brush.
            if let Err(e) = unsafe { UnregisterClassW(STAGE_CLASS, Some(instance.into())) } {
                eprintln!("warning: UnregisterClassW(stage): {e}");
            }
            format!("CreateWindowExW(stage): {e}")
        })?;
        let stage = Stage { hwnd, color };
        let off = BOOL::from(true);
        // SAFETY: our window, and a BOOL the size given.
        unsafe {
            DwmSetWindowAttribute(
                hwnd,
                DWMWA_TRANSITIONS_FORCEDISABLED,
                (&raw const off).cast(),
                const { struct_size::<BOOL>() },
            )
        }
        .map_err(|e| format!("DwmSetWindowAttribute(stage): {e}"))?;
        Ok(stage)
    }

    /// Shows the stage over `rect`, above other windows but without taking the front, and waits until it is on screen.
    pub fn show(&self, rect: Rect) -> Result<(), String> {
        let width = i32::try_from(rect.width).map_err(|e| e.to_string())?;
        let height = i32::try_from(rect.height).map_err(|e| e.to_string())?;
        // SAFETY: our window, on this thread.
        unsafe {
            SetWindowPos(
                self.hwnd,
                Some(HWND_TOPMOST),
                rect.left,
                rect.top,
                width,
                height,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            )
        }
        .map_err(|e| format!("showing the stage: {e}"))?;
        // SAFETY: our window; DwmFlush waits for the next composition.
        unsafe {
            let _ = UpdateWindow(self.hwnd);
            DwmFlush().map_err(|e| format!("DwmFlush: {e}"))?;
        }
        wait(Duration::from_millis(100));
        Ok(())
    }

    pub fn hide(&self) {
        // SAFETY: our window; the result is only whether it was visible before.
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_HIDE);
        }
        pump();
    }
}

impl Drop for Stage {
    fn drop(&mut self) {
        // SAFETY: our window and class, each released once, the window first; unregistering the class deletes its brush.
        unsafe {
            if let Err(e) = DestroyWindow(self.hwnd) {
                eprintln!("warning: DestroyWindow(stage): {e}");
            }
            if let Ok(instance) = GetModuleHandleW(None)
                && let Err(e) = UnregisterClassW(STAGE_CLASS, Some(instance.into()))
            {
                eprintln!("warning: UnregisterClassW(stage): {e}");
            }
        }
    }
}

/// A small patch of the screen, read with a GDI BitBlt into a bitmap of its own: GDI reads the screen as shown with
/// HDR on or off alike.
pub struct Patch {
    screen: HDC,
    mem: HDC,
    bitmap: HBITMAP,
    pub rect: Rect,
}

impl Patch {
    pub fn new(rect: Rect) -> Result<Patch, String> {
        let width = i32::try_from(rect.width).map_err(|e| e.to_string())?;
        let height = i32::try_from(rect.height).map_err(|e| e.to_string())?;
        // SAFETY: plain GDI calls; what is made here is released in `drop`, or below when a later call fails.
        unsafe {
            let screen = GetDC(None);
            if screen.is_invalid() {
                return Err("GetDC(NULL) failed".to_string());
            }
            let mem = CreateCompatibleDC(Some(screen));
            if mem.is_invalid() {
                ReleaseDC(None, screen);
                return Err("CreateCompatibleDC failed".to_string());
            }
            let bitmap = CreateCompatibleBitmap(screen, width, height);
            if bitmap.is_invalid() {
                let _ = DeleteDC(mem);
                ReleaseDC(None, screen);
                return Err("CreateCompatibleBitmap failed".to_string());
            }
            Ok(Patch {
                screen,
                mem,
                bitmap,
                rect,
            })
        }
    }

    /// The patch's pixels, red, green and blue, row by row.
    pub fn read(&mut self) -> Result<Vec<[u8; 3]>, String> {
        let width = i32::try_from(self.rect.width).map_err(|e| e.to_string())?;
        let height = i32::try_from(self.rect.height).map_err(|e| e.to_string())?;
        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: const { struct_size::<BITMAPINFOHEADER>() },
                biWidth: width,
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                ..Default::default()
            },
            ..Default::default()
        };
        let pixels =
            usize::try_from(self.rect.width * self.rect.height).map_err(|e| e.to_string())?;
        let mut bits = vec![0u8; pixels * 4];
        // SAFETY: the DCs and the bitmap are ours; the bitmap is selected only while BitBlt draws into it, and
        // deselected before GetDIBits reads it; `bits` holds width × height × 4 bytes.
        let lines = unsafe {
            let previous = SelectObject(self.mem, HGDIOBJ(self.bitmap.0));
            let copied = BitBlt(
                self.mem,
                0,
                0,
                width,
                height,
                Some(self.screen),
                self.rect.left,
                self.rect.top,
                ROP_CODE(SRCCOPY.0 | CAPTUREBLT.0),
            );
            SelectObject(self.mem, previous);
            copied.map_err(|e| format!("BitBlt of the patch: {e}"))?;
            GetDIBits(
                self.mem,
                self.bitmap,
                0,
                self.rect.height,
                Some(bits.as_mut_ptr().cast()),
                &mut info,
                DIB_RGB_COLORS,
            )
        };
        if lines != height {
            return Err(format!("GetDIBits returned {lines} lines of {height}"));
        }
        Ok(bits
            .as_chunks::<4>()
            .0
            .iter()
            .map(|p| [p[2], p[1], p[0]])
            .collect())
    }
}

impl Drop for Patch {
    fn drop(&mut self) {
        // SAFETY: made in `new` and released once, the bitmap first (it isn't selected).
        unsafe {
            if !DeleteObject(HGDIOBJ(self.bitmap.0)).as_bool() {
                eprintln!("warning: DeleteObject(patch bitmap) failed");
            }
            if !DeleteDC(self.mem).as_bool() {
                eprintln!("warning: DeleteDC(patch) failed");
            }
            ReleaseDC(None, self.screen);
        }
    }
}

pub fn put_cursor(point: Point) -> Result<(), String> {
    // SAFETY: plain call; physical pixels, since this thread is Per-Monitor V2.
    unsafe { SetCursorPos(point.x, point.y) }.map_err(|e| format!("SetCursorPos: {e}"))
}

/// Presses and lets go of `shortcut` with real key events, as the user would, and returns when its last key went down,
/// the moment Windows tells ScreenGauge of it.
pub fn press(shortcut: Shortcut) -> Result<Instant, String> {
    let modifiers = shortcut.modifiers();
    let held: Vec<VIRTUAL_KEY> = [
        (modifiers.win, VK_LWIN),
        (modifiers.ctrl, VK_CONTROL),
        (modifiers.alt, VK_MENU),
        (modifiers.shift, VK_SHIFT),
    ]
    .into_iter()
    .filter_map(|(on, key)| on.then_some(key))
    .collect();
    let key = virtual_key(shortcut.key())?;
    let down: Vec<INPUT> = held.iter().map(|&k| key_event(k, false)).collect();
    send(&down)?;
    let at = Instant::now();
    send(&[key_event(key, false), key_event(key, true)])?;
    let up: Vec<INPUT> = held.iter().rev().map(|&k| key_event(k, true)).collect();
    send(&up)?;
    Ok(at)
}

fn virtual_key(key: Key) -> Result<VIRTUAL_KEY, String> {
    // The virtual keys of A to Z and 0 to 9 are their ASCII codes.
    Ok(match key {
        Key::Letter(letter) => VIRTUAL_KEY(u16::from(
            u8::try_from(letter).map_err(|_| format!("the key {letter} isn't ASCII"))?,
        )),
        Key::Digit(digit) => VIRTUAL_KEY(u16::from(b'0' + digit)),
        Key::Function(number) => VIRTUAL_KEY(VK_F1.0 + u16::from(number) - 1),
    })
}

fn key_event(key: VIRTUAL_KEY, up: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: key,
                dwFlags: if up {
                    KEYEVENTF_KEYUP
                } else {
                    KEYBD_EVENT_FLAGS(0)
                },
                ..Default::default()
            },
        },
    }
}

fn send(inputs: &[INPUT]) -> Result<(), String> {
    if inputs.is_empty() {
        return Ok(());
    }
    // SAFETY: `inputs` are fully initialized keyboard events of the size given.
    let sent = unsafe { SendInput(inputs, const { struct_size::<INPUT>() }.cast_signed()) };
    if usize::try_from(sent).ok() != Some(inputs.len()) {
        return Err(format!(
            "SendInput sent {sent} of {} key events: another program may be blocking input",
            inputs.len()
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_shortcut_s_key_is_pressed_by_its_virtual_key() {
        // The virtual-key codes Windows documents: 'X' is 0x58, '7' is 0x37, F1 is 0x70 and F12 0x7B.
        assert_eq!(virtual_key(Key::Letter('X')), Ok(VIRTUAL_KEY(0x58)));
        assert_eq!(virtual_key(Key::Digit(7)), Ok(VIRTUAL_KEY(0x37)));
        assert_eq!(virtual_key(Key::Function(1)), Ok(VIRTUAL_KEY(0x70)));
        assert_eq!(virtual_key(Key::Function(12)), Ok(VIRTUAL_KEY(0x7B)));
    }
}
