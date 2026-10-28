//! Small Win32 helpers for the run mode: drop guards, process usage, window descriptions,
//! and the few numeric conversions Win32 forces on us.

use windows::Win32::Foundation::{CloseHandle, FILETIME, HANDLE, HWND, LPARAM, RECT, WPARAM};
use windows::Win32::System::ProcessStatus::{
    GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX2,
};
use windows::Win32::System::Threading::{
    GetCurrentProcess, GetProcessTimes, OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
    QueryFullProcessImageNameW,
};
use windows::Win32::UI::Accessibility::{HWINEVENTHOOK, UnhookWinEvent};
use windows::Win32::UI::Input::KeyboardAndMouse::UnregisterHotKey;
use windows::Win32::UI::Shell::{
    NIF_ICON, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_SETVERSION, NOTIFYICON_VERSION_4,
    NOTIFYICONDATAW, Shell_NotifyIconW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DestroyWindow, GetClassNameW, GetWindowRect, GetWindowTextW, GetWindowThreadProcessId, IDI_APPLICATION, IsWindow,
    KillTimer, LoadIconW, SetTimer,
};
use windows::core::PWSTR;

use crate::common::{Ctx, PxRect};
use crate::log;

/// Closes a kernel handle on drop.
pub struct HandleGuard(pub HANDLE);

impl Drop for HandleGuard {
    fn drop(&mut self) {
        // SAFETY: the handle came from a successful create/open call and is closed once, here.
        if let Err(e) = unsafe { CloseHandle(self.0) } {
            log::line(format!("cleanup: CloseHandle: {e}"));
        }
    }
}

/// Destroys a window on drop.
pub struct WindowGuard {
    pub hwnd: HWND,
    pub what: &'static str,
}

impl Drop for WindowGuard {
    fn drop(&mut self) {
        destroy_window(self.hwnd, self.what);
    }
}

/// DestroyWindow with logging; a window that is already gone (an owner took it along) is logged, not an error.
pub fn destroy_window(hwnd: HWND, what: &str) {
    // SAFETY: IsWindow accepts any value.
    if !unsafe { IsWindow(Some(hwnd)) }.as_bool() {
        log::line(format!("cleanup: {what} window was already destroyed"));
        return;
    }
    // SAFETY: called on the thread that created the window (the run mode is single-threaded).
    if let Err(e) = unsafe { DestroyWindow(hwnd) } {
        log::line(format!("cleanup: DestroyWindow({what}): {e}"));
    }
}

/// The notification-area icon; NIM_DELETE on drop.
pub struct TrayIcon {
    hwnd: HWND,
    id: u32,
    callback: u32,
}

impl TrayIcon {
    pub fn add(hwnd: HWND, id: u32, callback: u32) -> Result<Self, String> {
        let icon = Self { hwnd, id, callback };
        icon.register()?;
        Ok(icon)
    }

    /// NIM_ADD then NIM_SETVERSION 4. Also used again when Explorer restarts (TaskbarCreated).
    pub fn register(&self) -> Result<(), String> {
        let mut nid = self.data()?;
        nid.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP | NIF_SHOWTIP;
        nid.uCallbackMessage = self.callback;
        // SAFETY: loads a shared system icon; nothing to free.
        nid.hIcon = unsafe { LoadIconW(None, IDI_APPLICATION) }.ctx("LoadIconW(IDI_APPLICATION)")?;
        for (dst, src) in nid.szTip.iter_mut().zip("hwtest".encode_utf16()) {
            *dst = src;
        }
        // SAFETY: nid is fully initialized and cbSize matches its type.
        if !unsafe { Shell_NotifyIconW(NIM_ADD, &nid) }.as_bool() {
            return Err("Shell_NotifyIconW(NIM_ADD) returned FALSE".to_string());
        }
        nid.Anonymous.uVersion = NOTIFYICON_VERSION_4;
        // SAFETY: as above.
        if !unsafe { Shell_NotifyIconW(NIM_SETVERSION, &nid) }.as_bool() {
            return Err("Shell_NotifyIconW(NIM_SETVERSION 4) returned FALSE".to_string());
        }
        Ok(())
    }

    fn data(&self) -> Result<NOTIFYICONDATAW, String> {
        Ok(NOTIFYICONDATAW {
            cbSize: size_u32::<NOTIFYICONDATAW>()?,
            hWnd: self.hwnd,
            uID: self.id,
            ..Default::default()
        })
    }
}

impl Drop for TrayIcon {
    fn drop(&mut self) {
        match self.data() {
            Ok(nid) => {
                // SAFETY: nid identifies our icon by hWnd + uID.
                if !unsafe { Shell_NotifyIconW(NIM_DELETE, &nid) }.as_bool() {
                    log::line("cleanup: Shell_NotifyIconW(NIM_DELETE) returned FALSE");
                }
            }
            Err(e) => log::line(format!("cleanup: tray icon: {e}")),
        }
    }
}

/// UnregisterHotKey on drop.
pub struct HotkeyGuard {
    pub hwnd: HWND,
    pub id: i32,
}

impl Drop for HotkeyGuard {
    fn drop(&mut self) {
        // SAFETY: the hotkey was registered on this window and thread.
        if let Err(e) = unsafe { UnregisterHotKey(Some(self.hwnd), self.id) } {
            log::line(format!("cleanup: UnregisterHotKey: {e}"));
        }
    }
}

/// UnhookWinEvent on drop.
pub struct WinEventGuard(pub HWINEVENTHOOK);

impl Drop for WinEventGuard {
    fn drop(&mut self) {
        // SAFETY: the hook came from a successful SetWinEventHook on this thread.
        if !unsafe { UnhookWinEvent(self.0) }.as_bool() {
            log::line("cleanup: UnhookWinEvent returned FALSE");
        }
    }
}

pub fn set_timer(hwnd: HWND, id: usize, ms: u64) -> Result<(), String> {
    let ms = u32::try_from(ms).ctx("timer interval")?;
    // SAFETY: a plain window timer on our own window; no callback.
    if unsafe { SetTimer(Some(hwnd), id, ms, None) } == 0 {
        return Err(format!("SetTimer({id}, {ms} ms): {}", windows::core::Error::from_thread()));
    }
    Ok(())
}

pub fn kill_timer(hwnd: HWND, id: usize) {
    // SAFETY: a timer on our own window.
    if let Err(e) = unsafe { KillTimer(Some(hwnd), id) } {
        log::line(format!("KillTimer({id}): {e}"));
    }
}

pub fn window_rect(hwnd: HWND) -> Result<PxRect, String> {
    let mut r = RECT::default();
    // SAFETY: r is a valid out pointer.
    unsafe { GetWindowRect(hwnd, &mut r) }.ctx("GetWindowRect")?;
    Ok(PxRect { left: r.left, top: r.top, right: r.right, bottom: r.bottom })
}

/// Process memory and CPU time, for the consumption targets.
#[derive(Clone, Copy)]
pub struct Usage {
    pub working_set: usize,
    pub private: usize,
    /// Task Manager's "Memory (private working set)" column.
    pub private_ws: usize,
    pub cpu_ms: f64,
}

impl std::fmt::Display for Usage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "working set {:.1} MB, private {:.1} MB, private working set {:.1} MB, cpu {:.0} ms",
            mb(self.working_set),
            mb(self.private),
            mb(self.private_ws),
            self.cpu_ms
        )
    }
}

pub fn usage() -> Result<Usage, String> {
    let mut mem = PROCESS_MEMORY_COUNTERS_EX2 { cb: size_u32::<PROCESS_MEMORY_COUNTERS_EX2>()?, ..Default::default() };
    // SAFETY: the pseudo handle is always valid; the buffer is the EX2 struct and cb says so,
    // which GetProcessMemoryInfo accepts through the base-struct pointer.
    unsafe { GetProcessMemoryInfo(GetCurrentProcess(), (&raw mut mem).cast::<PROCESS_MEMORY_COUNTERS>(), mem.cb) }
        .ctx("GetProcessMemoryInfo")?;
    let (mut created, mut exited, mut kernel, mut user) =
        (FILETIME::default(), FILETIME::default(), FILETIME::default(), FILETIME::default());
    // SAFETY: four valid out pointers.
    unsafe { GetProcessTimes(GetCurrentProcess(), &mut created, &mut exited, &mut kernel, &mut user) }
        .ctx("GetProcessTimes")?;
    Ok(Usage {
        working_set: mem.WorkingSetSize,
        private: mem.PrivateUsage,
        private_ws: mem.PrivateWorkingSetSize,
        cpu_ms: ms_from_100ns(filetime(kernel).saturating_add(filetime(user))),
    })
}

/// "<exe> class=<class> title=<title>" for any window, with whatever failed spelled out.
pub fn describe_window(hwnd: HWND) -> String {
    if hwnd.is_invalid() {
        return "no window (null hwnd)".to_string();
    }
    let mut class = [0u16; 256];
    // SAFETY: the slice carries the buffer length.
    let n = unsafe { GetClassNameW(hwnd, &mut class) };
    let class = match usize::try_from(n) {
        Ok(len) if len > 0 => String::from_utf16_lossy(&class[..len.min(class.len())]),
        _ => format!("? (GetClassNameW: {})", windows::core::Error::from_thread()),
    };
    let mut title = [0u16; 256];
    // SAFETY: the slice carries the buffer length. A window in another process is read without a message.
    let n = unsafe { GetWindowTextW(hwnd, &mut title) };
    // 0 is also a legitimately empty title, so it is not treated as a failure.
    let title = String::from_utf16_lossy(&title[..usize::try_from(n).unwrap_or(0).min(title.len())]);
    let mut pid = 0u32;
    // SAFETY: pid is a valid out pointer.
    let thread = unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    let exe = if thread == 0 {
        format!("? (GetWindowThreadProcessId: {})", windows::core::Error::from_thread())
    } else {
        match exe_name(pid) {
            Ok(name) => name,
            Err(e) => format!("? pid {pid} ({e})"),
        }
    };
    format!("{exe} class=\"{class}\" title=\"{title}\"")
}

fn exe_name(pid: u32) -> Result<String, String> {
    // SAFETY: plain open with the minimum right; the handle is closed by the guard.
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ctx("OpenProcess")?;
    let _close = HandleGuard(handle);
    let mut buf = [0u16; 1024];
    let mut size = u32::try_from(buf.len()).ctx("buffer size")?;
    // SAFETY: buf is writable for `size` units; size is updated to the length written.
    unsafe { QueryFullProcessImageNameW(handle, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut size) }
        .ctx("QueryFullProcessImageNameW")?;
    let len = usize::try_from(size).ctx("name length")?.min(buf.len());
    let full = String::from_utf16_lossy(&buf[..len]);
    Ok(full.rsplit('\\').next().unwrap_or(&full).to_string())
}

pub fn size_u32<T>() -> Result<u32, String> {
    u32::try_from(size_of::<T>()).ctx("struct size")
}

fn filetime(t: FILETIME) -> u64 {
    (u64::from(t.dwHighDateTime) << 32) | u64::from(t.dwLowDateTime)
}

/// 100-ns units to milliseconds.
fn ms_from_100ns(v: u64) -> f64 {
    // Lossy only above 2^53 * 100 ns (about 28 years of CPU time).
    // TODO: tighten if this ever leaves the throwaway prototype.
    v as f64 / 10_000.0
}

pub fn mb(bytes: usize) -> f64 {
    // Lossy only above 2^53 bytes.
    // TODO: tighten if this ever leaves the throwaway prototype.
    bytes as f64 / (1024.0 * 1024.0)
}

/// Pixel count to f64 for percentages and timings.
pub fn f64_from_u64(v: u64) -> f64 {
    // Lossy only above 2^53, far beyond any pixel count here.
    // TODO: tighten if this ever leaves the throwaway prototype.
    v as f64
}

/// Integer pixels to Direct2D coordinates.
pub fn f32_from_u32(v: u32) -> f32 {
    // Exact below 2^24, far beyond any monitor size.
    // TODO: tighten if this ever leaves the throwaway prototype.
    v as f32
}

/// LOWORD / HIWORD of a message parameter; the truncation is the point.
pub fn loword(v: usize) -> u16 {
    (v & 0xFFFF) as u16
}

pub fn hiword(v: usize) -> u16 {
    ((v >> 16) & 0xFFFF) as u16
}

pub fn wparam_bits(w: WPARAM) -> usize {
    w.0
}

pub fn lparam_bits(l: LPARAM) -> usize {
    l.0.cast_unsigned()
}

/// Carries an f64 timestamp through a posted message's LPARAM (64-bit only, like the whole program).
pub fn lparam_from_ms(ms: f64) -> Result<LPARAM, String> {
    Ok(LPARAM(isize::try_from(ms.to_bits().cast_signed()).ctx("timestamp into LPARAM")?))
}

pub fn ms_from_lparam(l: LPARAM) -> Result<f64, String> {
    Ok(f64::from_bits(i64::try_from(l.0).ctx("timestamp from LPARAM")?.cast_unsigned()))
}

/// Physical pixels for a length given in DIPs at `dpi`, rounded to nearest.
pub fn scale_dip(dip: u32, dpi: u32) -> i32 {
    let px = (u64::from(dip) * u64::from(dpi) + 48) / 96;
    i32::try_from(px).unwrap_or(i32::MAX)
}
