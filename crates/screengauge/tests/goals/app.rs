//! ScreenGauge.exe seen from outside, as another process: starting it, finding its windows, reading its memory and
//! processor time, closing its tools and asking it to exit. Nothing here is in the exe.

use std::os::windows::io::AsRawHandle;
use std::path::Path;
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{FILETIME, HANDLE, HWND, LPARAM, WPARAM};
use windows::Win32::System::ProcessStatus::{
    GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX2,
};
use windows::Win32::System::Threading::GetProcessTimes;
use windows::Win32::System::WindowsProgramming::QueryProcessCycleTime;
use windows::Win32::UI::Input::KeyboardAndMouse::VK_ESCAPE;
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, FindWindowExW, GetClassNameW, GetWindowThreadProcessId, IsWindowVisible,
    PostMessageW, WM_CLOSE, WM_KEYDOWN,
};
use windows::core::{BOOL, PCWSTR, w};

use crate::{screen, struct_size};

/// The resident window's class (`resident.rs`), never shown: ScreenGauge runs while it exists.
const RESIDENT: PCWSTR = w!("ScreenGauge.Resident");
/// The frozen screen's windows' class (`frozen_screen.rs`), one per monitor while the tools are open.
const FROZEN_SCREEN: &str = "ScreenGauge.FrozenScreen";

/// How long ScreenGauge gets to exit after WM_CLOSE.
const EXIT_WAIT: Duration = Duration::from_secs(10);

/// A ScreenGauge.exe this routine started.
pub struct App {
    child: Child,
}

/// Whether any ScreenGauge runs now, this routine's or not.
pub fn any_running() -> bool {
    // SAFETY: a static class name and no window name.
    unsafe { FindWindowExW(None, None, RESIDENT, PCWSTR::null()) }.is_ok()
}

impl App {
    /// Starts `exe`; at `sign_in`, with the flag "Start with Windows" gives it, so it starts hidden instead of calling.
    pub fn start(exe: &Path, sign_in: bool) -> Result<App, String> {
        let mut command = Command::new(exe);
        if sign_in {
            command.arg("--sign-in");
        }
        let child = command
            .spawn()
            .map_err(|e| format!("starting {}: {e}", exe.display()))?;
        Ok(App { child })
    }

    fn handle(&self) -> HANDLE {
        HANDLE(self.child.as_raw_handle())
    }

    fn pid(&self) -> u32 {
        self.child.id()
    }

    /// Waits for the resident window, which ScreenGauge makes once it has read its settings and before it registers
    /// the global shortcut; `pump` keeps this thread's windows answering meanwhile.
    pub fn wait_until_resident(&mut self, within: Duration) -> Result<(), String> {
        let deadline = Instant::now() + within;
        while self.windows(ClassName::Resident).is_empty() {
            if let Some(status) = self.child.try_wait().map_err(|e| e.to_string())? {
                return Err(format!("ScreenGauge exited at start, {status}"));
            }
            if Instant::now() > deadline {
                return Err(format!(
                    "ScreenGauge made no resident window within {within:?}"
                ));
            }
            screen::wait(Duration::from_millis(20));
        }
        Ok(())
    }

    /// The private working set, in bytes: what Task Manager's Memory column shows.
    pub fn private_working_set(&self) -> Result<u64, String> {
        let mut counters = PROCESS_MEMORY_COUNTERS_EX2 {
            cb: const { struct_size::<PROCESS_MEMORY_COUNTERS_EX2>() },
            ..Default::default()
        };
        // SAFETY: the child's handle is open while `self` lives; PROCESS_MEMORY_COUNTERS_EX2 starts with the fields of
        // PROCESS_MEMORY_COUNTERS, and `cb` gives the size actually passed.
        unsafe {
            GetProcessMemoryInfo(
                self.handle(),
                (&raw mut counters).cast::<PROCESS_MEMORY_COUNTERS>(),
                counters.cb,
            )
        }
        .map_err(|e| format!("GetProcessMemoryInfo: {e}"))?;
        u64::try_from(counters.PrivateWorkingSetSize).map_err(|e| e.to_string())
    }

    /// The processor cycles every thread of the process has used, kernel and user: exact, unlike the process's times,
    /// which Windows counts in steps of 15.6 ms.
    pub fn cycles(&self) -> Result<u64, String> {
        let mut cycles = 0;
        // SAFETY: the child's handle is open while `self` lives, and `cycles` is a u64 for the call to fill.
        unsafe { QueryProcessCycleTime(self.handle(), &mut cycles) }
            .map_err(|e| format!("QueryProcessCycleTime: {e}"))?;
        Ok(cycles)
    }

    /// The process's kernel and user time as Windows counts it, in steps of the clock tick: a check on the cycle counts.
    pub fn times(&self) -> Result<Duration, String> {
        let (mut created, mut exited, mut kernel, mut user) = (
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
        );
        // SAFETY: the child's handle is open while `self` lives, and four FILETIMEs for the call to fill.
        unsafe {
            GetProcessTimes(
                self.handle(),
                &mut created,
                &mut exited,
                &mut kernel,
                &mut user,
            )
        }
        .map_err(|e| format!("GetProcessTimes: {e}"))?;
        let ticks = |t: FILETIME| u64::from(t.dwHighDateTime) << 32 | u64::from(t.dwLowDateTime);
        // FILETIME counts 100 ns.
        Ok(Duration::from_nanos((ticks(kernel) + ticks(user)) * 100))
    }

    /// The frozen screen's windows now shown: one per monitor while the tools are open.
    pub fn frozen_screens(&self) -> Vec<HWND> {
        self.windows(ClassName::FrozenScreen)
            .into_iter()
            // SAFETY: any window handle may be asked whether it is visible.
            .filter(|&hwnd| unsafe { IsWindowVisible(hwnd) }.as_bool())
            .collect()
    }

    /// Closes the tools as Esc does, posted to a frozen screen window so no key goes to another program, and waits until
    /// every frozen screen window is gone.
    pub fn close_tools(&self, within: Duration) -> Result<(), String> {
        let shown = self.frozen_screens();
        let Some(&first) = shown.first() else {
            return Err("the tools aren't open: no frozen screen window shows".to_string());
        };
        let escape = usize::from(VK_ESCAPE.0);
        // SAFETY: posting to a window of another process; the message carries no pointer.
        unsafe { PostMessageW(Some(first), WM_KEYDOWN, WPARAM(escape), LPARAM(1)) }
            .map_err(|e| format!("posting Esc: {e}"))?;
        let deadline = Instant::now() + within;
        while !self.frozen_screens().is_empty() {
            if Instant::now() > deadline {
                return Err(format!("the tools were still open {within:?} after Esc"));
            }
            screen::wait(Duration::from_millis(5));
        }
        Ok(())
    }

    /// Exits ScreenGauge as its tray menu's Exit does, by closing the resident window, and waits for the process to end.
    pub fn exit(mut self) -> Result<(), String> {
        self.close_and_wait()
    }

    fn close_and_wait(&mut self) -> Result<(), String> {
        if self.child.try_wait().map_err(|e| e.to_string())?.is_some() {
            return Ok(());
        }
        for hwnd in self.windows(ClassName::Resident) {
            // SAFETY: posting to a window of another process; the message carries nothing.
            unsafe { PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0)) }
                .map_err(|e| format!("posting WM_CLOSE: {e}"))?;
        }
        let deadline = Instant::now() + EXIT_WAIT;
        loop {
            if let Some(status) = self.child.try_wait().map_err(|e| e.to_string())? {
                return if status.success() {
                    Ok(())
                } else {
                    Err(format!("ScreenGauge exited with {status}"))
                };
            }
            if Instant::now() > deadline {
                // Stopping it leaves a ghost tray icon until the mouse passes over it, but never a ScreenGauge running.
                self.child.kill().map_err(|e| e.to_string())?;
                return Err(format!(
                    "ScreenGauge didn't exit within {EXIT_WAIT:?} of WM_CLOSE, so it was stopped"
                ));
            }
            screen::wait(Duration::from_millis(20));
        }
    }

    fn windows(&self, class: ClassName) -> Vec<HWND> {
        let mut search = Search {
            pid: self.pid(),
            class: class.text(),
            found: Vec::new(),
        };
        // SAFETY: `search` outlives the call, and `collect` reads it only during the call. EnumWindows fails only when
        // the callback stops it, which `collect` never does.
        let listed = unsafe { EnumWindows(Some(collect), LPARAM((&raw mut search) as isize)) };
        if let Err(e) = listed {
            panic!("EnumWindows: {e}");
        }
        search.found
    }
}

impl Drop for App {
    /// A ScreenGauge left running would answer the next start of the routine, or the owner's next opening: whatever
    /// went wrong, it exits.
    fn drop(&mut self) {
        if let Err(e) = self.close_and_wait() {
            eprintln!("warning: {e}");
        }
    }
}

#[derive(Clone, Copy)]
enum ClassName {
    Resident,
    FrozenScreen,
}

impl ClassName {
    fn text(self) -> &'static str {
        match self {
            ClassName::Resident => "ScreenGauge.Resident",
            ClassName::FrozenScreen => FROZEN_SCREEN,
        }
    }
}

struct Search {
    pid: u32,
    class: &'static str,
    found: Vec<HWND>,
}

extern "system" fn collect(hwnd: HWND, lparam: LPARAM) -> BOOL {
    // SAFETY: `windows` passes a pointer to its own `Search`, alive for the whole EnumWindows call.
    let search = unsafe { &mut *(lparam.0 as *mut Search) };
    let mut pid = 0;
    // SAFETY: any window handle EnumWindows gives; `pid` is a u32 for the call to fill.
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    if pid == search.pid {
        let mut name = [0u16; 64];
        // SAFETY: `name` is a buffer of its own length.
        let length = unsafe { GetClassNameW(hwnd, &mut name) };
        let length = usize::try_from(length).unwrap_or(0);
        if String::from_utf16_lossy(&name[..length]) == search.class {
            search.found.push(hwnd);
        }
    }
    BOOL::from(true)
}
