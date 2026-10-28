//! Foreground-window watcher: logs every foreground change (class, title, process exe name) and counts the ones
//! that look like the Start menu or Search opening. The WinEvent hook is out of context, so its callback runs on
//! the thread that created the watcher, inside the message pump.

use std::cell::Cell;

use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND};
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent};
use windows::Win32::UI::WindowsAndMessaging::{
    EVENT_SYSTEM_FOREGROUND, GetClassNameW, GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId,
    WINEVENT_OUTOFCONTEXT,
};
use windows::core::PWSTR;

use crate::common::Ctx;
use crate::log;

pub const MAX_PHASES: usize = 4;

/// Process names that mean the Start menu or Search took the foreground.
const START_LIKE: [&str; 2] = ["StartMenuExperienceHost.exe", "SearchHost.exe"];

thread_local! {
    static PHASE: Cell<usize> = const { Cell::new(0) };
    static NAMES: Cell<&'static [&'static str]> = const { Cell::new(&[]) };
    static CHANGES: Cell<[u32; MAX_PHASES]> = const { Cell::new([0; MAX_PHASES]) };
    static STARTS: Cell<[u32; MAX_PHASES]> = const { Cell::new([0; MAX_PHASES]) };
}

pub struct Watcher {
    hook: HWINEVENTHOOK,
}

impl Watcher {
    /// Starts watching. `phases` names the phases (at most `MAX_PHASES`); every line and count is tagged with the
    /// current one, which begins at phase 0.
    pub fn start(phases: &'static [&'static str]) -> Result<Self, String> {
        if phases.is_empty() || phases.len() > MAX_PHASES {
            return Err(format!("foreground watcher: 1 to {MAX_PHASES} phases, got {}", phases.len()));
        }
        NAMES.set(phases);
        PHASE.set(0);
        CHANGES.set([0; MAX_PHASES]);
        STARTS.set([0; MAX_PHASES]);

        // SAFETY: plain query.
        let current = unsafe { GetForegroundWindow() };
        log::line(format!("foreground [initial] {}", describe(current).line()));

        // SAFETY: the callback is an `extern "system"` fn that does not unwind; out of context, so it runs on this
        // thread inside the message pump.
        let hook = unsafe {
            SetWinEventHook(
                EVENT_SYSTEM_FOREGROUND,
                EVENT_SYSTEM_FOREGROUND,
                None,
                Some(on_foreground),
                0,
                0,
                WINEVENT_OUTOFCONTEXT,
            )
        };
        if hook.is_invalid() {
            return Err(format!("SetWinEventHook: {}", windows::core::Error::from_thread()));
        }
        Ok(Self { hook })
    }

    pub fn set_phase(&self, phase: usize) {
        PHASE.set(phase.min(MAX_PHASES - 1));
    }

    /// Foreground changes seen in `phase`.
    pub fn changes(&self, phase: usize) -> u32 {
        CHANGES.get().get(phase).copied().unwrap_or(0)
    }

    /// Foreground changes to StartMenuExperienceHost.exe or SearchHost.exe seen in `phase`.
    pub fn start_like(&self, phase: usize) -> u32 {
        STARTS.get().get(phase).copied().unwrap_or(0)
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        // SAFETY: the hook was created by this thread and is removed once.
        if !unsafe { UnhookWinEvent(self.hook) }.as_bool() {
            log::line(format!("ERROR: UnhookWinEvent on cleanup: {}", windows::core::Error::from_thread()));
        }
    }
}

struct Described {
    pid: u32,
    exe: String,
    class: String,
    title: String,
}

impl Described {
    fn line(&self) -> String {
        format!("pid={} exe={} class={} title=\"{}\"", self.pid, self.exe, self.class, self.title)
    }

    fn is_start_like(&self) -> bool {
        START_LIKE.iter().any(|name| self.exe.eq_ignore_ascii_case(name))
    }
}

/// Never fails: a lookup that does not work is written into the field, so the log shows why.
fn describe(hwnd: HWND) -> Described {
    if hwnd.is_invalid() {
        return Described { pid: 0, exe: "-".into(), class: "-".into(), title: "(no window)".into() };
    }
    let mut buf = [0u16; 256];
    // SAFETY: writes at most `buf.len()` UTF-16 units.
    let n = unsafe { GetClassNameW(hwnd, &mut buf) };
    let class = match usize::try_from(n) {
        Ok(n) if n > 0 && n <= buf.len() => String::from_utf16_lossy(&buf[..n]),
        _ => format!("?({})", windows::core::Error::from_thread()),
    };
    // SAFETY: writes at most `buf.len()` UTF-16 units. A length of 0 is also what an empty title returns.
    let n = unsafe { GetWindowTextW(hwnd, &mut buf) };
    let title = match usize::try_from(n) {
        Ok(n) if n <= buf.len() => String::from_utf16_lossy(&buf[..n]),
        _ => format!("?({})", windows::core::Error::from_thread()),
    };
    let mut pid = 0u32;
    // SAFETY: writes one u32.
    let thread = unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    let exe = if thread == 0 {
        format!("?(GetWindowThreadProcessId: {})", windows::core::Error::from_thread())
    } else {
        exe_name(pid).unwrap_or_else(|e| format!("?({e})"))
    };
    Described { pid, exe, class, title }
}

/// Closes a process handle on drop.
struct ProcessHandle(HANDLE);

impl Drop for ProcessHandle {
    fn drop(&mut self) {
        // SAFETY: the handle came from OpenProcess and is closed once.
        if let Err(e) = unsafe { CloseHandle(self.0) } {
            log::line(format!("ERROR: CloseHandle on a process handle: {e}"));
        }
    }
}

fn exe_name(pid: u32) -> Result<String, String> {
    // SAFETY: plain call; the limited-information right is enough for QueryFullProcessImageNameW.
    let handle = ProcessHandle(unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ctx("OpenProcess")?);
    let mut buf = [0u16; 1024];
    let mut len = u32::try_from(buf.len()).ctx("buffer length")?;
    // SAFETY: `len` is the buffer capacity in UTF-16 units; the call updates it to the length written.
    unsafe { QueryFullProcessImageNameW(handle.0, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len) }
        .ctx("QueryFullProcessImageNameW")?;
    let len = usize::try_from(len).ctx("path length")?;
    let path = String::from_utf16_lossy(buf.get(..len).ok_or("path length beyond buffer")?);
    Ok(path.rsplit('\\').next().unwrap_or(&path).to_string())
}

/// Bumps the counter of the current phase in a `[u32; MAX_PHASES]` cell.
fn bump(cell: &'static std::thread::LocalKey<Cell<[u32; MAX_PHASES]>>, phase: usize) {
    let mut counts = cell.get();
    if let Some(slot) = counts.get_mut(phase) {
        *slot += 1;
    }
    cell.set(counts);
}

unsafe extern "system" fn on_foreground(
    _hook: HWINEVENTHOOK,
    event: u32,
    hwnd: HWND,
    _id_object: i32,
    _id_child: i32,
    _event_thread: u32,
    _event_time: u32,
) {
    if event != EVENT_SYSTEM_FOREGROUND {
        return;
    }
    let phase = PHASE.get();
    let what = describe(hwnd);
    bump(&CHANGES, phase);
    let tag = NAMES.get().get(phase).copied().unwrap_or("?");
    let start = what.is_start_like();
    if start {
        bump(&STARTS, phase);
    }
    log::line(format!(
        "foreground [{tag}] hwnd=0x{:X} {}{}",
        hwnd.0.addr(),
        what.line(),
        if start { "  <-- Start menu / Search" } else { "" }
    ));
}
