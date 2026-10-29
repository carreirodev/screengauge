//! Builder C. The `run` mode: resident like Pixmeter, with the frozen screen.
//!
//! One thread. The app state lives in a thread_local `RefCell<Option<App>>` that only the main window's
//! handlers borrow; tool windows keep their own state (see `tools`) and talk to the app by posting
//! messages to the main window, so no state is aliased when Win32 re-enters a window procedure.
//!
//! Flags: `--hotkey SPEC` (win+ctrl+m), `--trigger-after-ms N`, `--auto-close-secs N` (20),
//! `--exit-after-secs N` (900), `--once`, `--no-save`, `--save-photo`, `--present-vsync`.

mod app;
mod checks;
mod sys;
mod tools;

use std::cell::{Cell, RefCell};
use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError, HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::System::Console::{
    CTRL_BREAK_EVENT, CTRL_C_EVENT, CTRL_CLOSE_EVENT, CTRL_LOGOFF_EVENT, CTRL_SHUTDOWN_EVENT, SetConsoleCtrlHandler,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::{CreateMutexW, GetCurrentProcessId};
use windows::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN, RegisterHotKey,
};
use windows::Win32::UI::Shell::{NIN_POPUPCLOSE, NIN_POPUPOPEN, NIN_SELECT, NINF_KEY};
use windows::Win32::UI::WindowsAndMessaging::{
    ASFW_ANY, AllowSetForegroundWindow, CreateWindowExW, DefWindowProcW, DispatchMessageW, EVENT_SYSTEM_FOREGROUND,
    FindWindowW, GetMessageW, GetWindowThreadProcessId, ISMEX_CALLBACK, ISMEX_NOSEND, ISMEX_NOTIFY, ISMEX_REPLIED,
    ISMEX_SEND, InSendMessageEx, MSG, PostMessageW, PostQuitMessage, RegisterClassExW, RegisterWindowMessageW,
    TranslateMessage, WINDOW_EX_STYLE, WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS, WM_ACTIVATEAPP, WM_APP,
    WM_CLOSE, WM_CONTEXTMENU, WM_DESTROY, WM_DEVICECHANGE, WM_DISPLAYCHANGE, WM_DPICHANGED, WM_HOTKEY,
    WM_LBUTTONDBLCLK, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_RBUTTONDOWN, WM_RBUTTONUP, WM_SETTINGCHANGE,
    WM_TIMER, WNDCLASSEXW, WS_OVERLAPPED,
};
use windows::core::{BOOL, PCWSTR, w};

use self::app::App;
use self::sys::{HandleGuard, HotkeyGuard, WinEventGuard, WindowGuard};
use crate::common::{Args, Ctx};
use crate::log;

const MAIN_CLASS: PCWSTR = w!("PixmeterHwtestMain");
const MUTEX_NAME: PCWSTR = w!("Local\\PixmeterHwtestRun");
const SHOW_MESSAGE: PCWSTR = w!("PixmeterHwtestShowFromStart");

const WM_APP_TRAY: u32 = WM_APP + 1;
/// wParam: `CloseReason::code`, lParam: the request time (`sys::lparam_from_ms`).
const WM_APP_CLOSE: u32 = WM_APP + 2;
/// wParam: `Source::code`, lParam: t0 (`sys::lparam_from_ms`).
const WM_APP_TRIGGER: u32 = WM_APP + 3;

const TRAY_ID: u32 = 1;
const HOTKEY_ID: i32 = 1;
const NIN_KEYSELECT: u32 = NIN_SELECT | NINF_KEY;

const TIMER_POLL: usize = 1;
const TIMER_IDLE: usize = 2;
const TIMER_TRIGGER: usize = 3;
const TIMER_AUTO_CLOSE: usize = 4;
const TIMER_EXIT: usize = 5;
const TIMER_CHECK: usize = 6;
const TIMER_SERIES: usize = 7;

const POLL_MS: u64 = 1000;
const IDLE_MS: u64 = 60_000;
/// "After DwmFlush + ~150 ms" for the identical check.
const CHECK_DELAY_MS: u64 = 150;
/// Foreground changes are logged this long after a trigger even with the tools closed (the series window).
const FOREGROUND_LOG_WINDOW_MS: u32 = 5000;

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
    /// Monitor-change messages since the last poll, for "which signal arrived".
    static SIGNALS: RefCell<Vec<Signal>> = const { RefCell::new(Vec::new()) };
    static MAIN_HWND: Cell<HWND> = const { Cell::new(HWND(std::ptr::null_mut())) };
    /// GetTickCount just before the toolbar took the foreground; None while the tools are closed.
    static FG_GATE: Cell<Option<u32>> = const { Cell::new(None) };
    /// GetTickCount of the last trigger, to place WinEvents relative to it.
    static TRIGGER_TICK: Cell<Option<u32>> = const { Cell::new(None) };
    static DPICHANGED_ON_TOOLS: Cell<u32> = const { Cell::new(0) };
    static MSG_SHOW_FROM_START: Cell<u32> = const { Cell::new(0) };
    static MSG_TASKBAR_CREATED: Cell<u32> = const { Cell::new(0) };
}

/// The main window as an integer, for the console control handler, which runs on a thread of its own
/// (an HWND is only an identifier there; it is used for PostMessageW alone). 0 = none.
static CTRL_TARGET: AtomicIsize = AtomicIsize::new(0);
/// Set once run_mode has dropped every guard, so a console close can wait for the cleanup.
static CLEANED_UP: AtomicBool = AtomicBool::new(false);
/// How long a console close, logoff or shutdown waits for the cleanup before Windows ends the process.
const CTRL_CLOSE_WAIT: Duration = Duration::from_secs(4);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Source {
    Hotkey,
    Tray,
    Start,
    Timer,
}

impl Source {
    const ALL: [Source; 4] = [Source::Hotkey, Source::Tray, Source::Start, Source::Timer];

    fn name(self) -> &'static str {
        match self {
            Source::Hotkey => "hotkey",
            Source::Tray => "tray",
            Source::Start => "start",
            Source::Timer => "timer",
        }
    }

    fn code(self) -> usize {
        match self {
            Source::Hotkey => 1,
            Source::Tray => 2,
            Source::Start => 3,
            Source::Timer => 4,
        }
    }

    fn from_code(code: usize) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.code() == code)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CloseReason {
    Esc,
    HotkeyAgain,
    Foreground,
    MonitorsChanged,
    AutoClose,
    Exit,
}

impl CloseReason {
    const ALL: [CloseReason; 6] = [
        CloseReason::Esc,
        CloseReason::HotkeyAgain,
        CloseReason::Foreground,
        CloseReason::MonitorsChanged,
        CloseReason::AutoClose,
        CloseReason::Exit,
    ];

    fn name(self) -> &'static str {
        match self {
            CloseReason::Esc => "Esc",
            CloseReason::HotkeyAgain => "hotkey pressed again",
            CloseReason::Foreground => "another program came to the foreground",
            CloseReason::MonitorsChanged => "monitors changed",
            CloseReason::AutoClose => "auto close (safety net)",
            CloseReason::Exit => "program exiting",
        }
    }

    fn code(self) -> usize {
        match self {
            CloseReason::Esc => 1,
            CloseReason::HotkeyAgain => 2,
            CloseReason::Foreground => 3,
            CloseReason::MonitorsChanged => 4,
            CloseReason::AutoClose => 5,
            CloseReason::Exit => 6,
        }
    }

    fn from_code(code: usize) -> Option<Self> {
        Self::ALL.into_iter().find(|r| r.code() == code)
    }
}

/// A monitor-change message, as received.
struct Signal {
    at: f64,
    name: &'static str,
    role: String,
}

/// `--hotkey`: modifiers `win`, `ctrl`, `shift`, `alt` joined by `+`, then one letter or digit.
#[derive(Clone)]
struct HotkeySpec {
    text: String,
    modifiers: HOT_KEY_MODIFIERS,
    vk: u32,
}

impl HotkeySpec {
    const DEFAULT: &str = "win+ctrl+m";

    fn parse(spec: &str) -> Result<Self, String> {
        let text = spec.trim().to_ascii_lowercase();
        let mut parts: Vec<&str> = text.split('+').map(str::trim).collect();
        let key = parts.pop().ok_or_else(|| format!("--hotkey {spec}: empty"))?;
        let mut modifiers = HOT_KEY_MODIFIERS(0);
        for part in parts {
            let m = match part {
                "win" => MOD_WIN,
                "ctrl" => MOD_CONTROL,
                "shift" => MOD_SHIFT,
                "alt" => MOD_ALT,
                other => return Err(format!("--hotkey {spec}: unknown modifier '{other}' (win, ctrl, shift, alt)")),
            };
            if modifiers.0 & m.0 != 0 {
                return Err(format!("--hotkey {spec}: '{part}' appears twice"));
            }
            modifiers |= m;
        }
        if modifiers.0 == 0 {
            return Err(format!("--hotkey {spec}: needs at least one modifier"));
        }
        let mut chars = key.chars();
        let vk = match (chars.next(), chars.next()) {
            // Virtual-key codes of letters and digits are their uppercase ASCII codes.
            (Some(c), None) if c.is_ascii_alphanumeric() => u32::from(c.to_ascii_uppercase()),
            _ => return Err(format!("--hotkey {spec}: the key must be one letter or digit, got '{key}'")),
        };
        Ok(Self { text, modifiers, vk })
    }
}

#[derive(Clone)]
struct Opts {
    trigger_after_ms: Option<u64>,
    auto_close_secs: u64,
    exit_after_secs: u64,
    once: bool,
    no_save: bool,
    save_photo: bool,
    /// Tray and Start triggers take the foreground before the delay series (experiment: the hidden-icons
    /// flyout stays open until another app activates).
    activate_first: bool,
    /// Tray triggers cycle through doing nothing, NIM_SETFOCUS and hiding the flyout window before the photo
    /// (WPC-290: does anything close the hidden-icons flyout, and harmlessly).
    overflow_cycle: bool,
    present: tools::Present,
    hotkey: HotkeySpec,
}

impl Opts {
    fn parse(args: &Args) -> Result<Self, String> {
        let trigger_after_ms = match args.value("trigger-after-ms") {
            Some(_) => Some(args.num("trigger-after-ms", 0)?),
            None => None,
        };
        let hotkey = HotkeySpec::parse(args.value("hotkey").unwrap_or(HotkeySpec::DEFAULT))?;
        let opts = Self {
            hotkey,
            trigger_after_ms,
            auto_close_secs: args.num("auto-close-secs", 20)?,
            exit_after_secs: args.num("exit-after-secs", 900)?,
            once: args.flag("once"),
            no_save: args.flag("no-save"),
            save_photo: args.flag("save-photo"),
            activate_first: args.flag("activate-first"),
            overflow_cycle: args.flag("overflow-cycle"),
            present: tools::Present { vsync: args.flag("present-vsync") },
        };
        if opts.auto_close_secs == 0 || opts.exit_after_secs == 0 {
            return Err("--auto-close-secs and --exit-after-secs must be at least 1".to_string());
        }
        Ok(opts)
    }

    fn describe(&self) -> String {
        format!(
            "hotkey={} trigger-after-ms={} auto-close-secs={} exit-after-secs={} once={} no-save={} save-photo={} activate-first={} overflow-cycle={} present={}",
            self.hotkey.text,
            self.trigger_after_ms.map_or_else(|| "off".to_string(), |v| v.to_string()),
            self.auto_close_secs,
            self.exit_after_secs,
            self.once,
            self.no_save,
            self.save_photo,
            self.activate_first,
            self.overflow_cycle,
            if self.present.vsync { "vsync (D2D1_PRESENT_OPTIONS_NONE)" } else { "immediately" }
        )
    }
}

pub fn run_mode(args: &Args) -> Result<(), String> {
    // Declared first, so it drops last: a console close waits for it.
    let _cleaned_up = CleanupDone;
    let opts = Opts::parse(args)?;

    // SAFETY: plain mutex creation; GetLastError is read right after, before any other API call.
    let (mutex, last_error) = unsafe {
        let mutex = CreateMutexW(None, false, MUTEX_NAME);
        (mutex, GetLastError())
    };
    let _mutex = HandleGuard(mutex.ctx("CreateMutexW(Local\\PixmeterHwtestRun)")?);
    if last_error == ERROR_ALREADY_EXISTS {
        return second_launch();
    }

    log::line(format!("run: resident start; {}", opts.describe()));
    // SAFETY: the module handle of our own executable.
    let hinstance: HINSTANCE = unsafe { GetModuleHandleW(PCWSTR::null()) }.ctx("GetModuleHandleW")?.into();
    register_main_class(hinstance)?;
    tools::register_classes(hinstance)?;

    // SAFETY: registers a message name; 0 means failure.
    let show = unsafe { RegisterWindowMessageW(SHOW_MESSAGE) };
    if show == 0 {
        return Err(format!("RegisterWindowMessageW(show): {}", windows::core::Error::from_thread()));
    }
    MSG_SHOW_FROM_START.with(|c| c.set(show));
    // SAFETY: as above.
    let taskbar_created = unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) };
    if taskbar_created == 0 {
        log::line(format!(
            "RegisterWindowMessageW(TaskbarCreated): {}; the tray icon will not come back if Explorer restarts",
            windows::core::Error::from_thread()
        ));
    }
    MSG_TASKBAR_CREATED.with(|c| c.set(taskbar_created));

    // Hidden top-level window, not message-only: message-only windows miss broadcasts like WM_DISPLAYCHANGE.
    // SAFETY: the class is registered; no create parameter.
    let main = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            MAIN_CLASS,
            w!("hwtest"),
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(hinstance),
            None,
        )
    }
    .ctx("CreateWindowExW(main)")?;
    let _main_window = WindowGuard { hwnd: main, what: "main" };
    MAIN_HWND.with(|c| c.set(main));

    // Ctrl+C or closing the console would otherwise end the process without running the guards
    // (leaving a ghost tray icon). The handler asks the main window to leave the message loop instead.
    CTRL_TARGET.store(main.0 as isize, Ordering::SeqCst);
    // SAFETY: registers a plain function; it only reads atomics and posts a message.
    let _console_handler = match unsafe { SetConsoleCtrlHandler(Some(on_console_ctrl), true) } {
        Ok(()) => Some(ConsoleHandlerGuard),
        Err(e) => {
            log::line(format!("SetConsoleCtrlHandler: {e}; Ctrl+C will end the process without cleanup"));
            None
        }
    };

    let app = App::new(main, hinstance, opts.clone())?;
    APP.with(|cell| match cell.try_borrow_mut() {
        Ok(mut slot) => {
            *slot = Some(app);
            Ok(())
        }
        Err(e) => Err(format!("installing the app state: {e}")),
    })?;
    let _app = AppGuard;

    let hotkey = &opts.hotkey;
    // SAFETY: registers a hotkey on our own window.
    let registered = unsafe { RegisterHotKey(Some(main), HOTKEY_ID, hotkey.modifiers | MOD_NOREPEAT, hotkey.vk) };
    let _hotkey = match registered {
        Ok(()) => {
            log::line(format!("RESULT run hotkey registration {}: ok", hotkey.text));
            Some(HotkeyGuard { hwnd: main, id: HOTKEY_ID })
        }
        Err(e) => {
            log::line(format!(
                "RESULT run hotkey registration {}: failed ({e}); continuing without the hotkey",
                hotkey.text
            ));
            None
        }
    };

    // SAFETY: out-of-context hook; the callback runs on this thread while it pumps messages.
    let hook = unsafe {
        SetWinEventHook(
            EVENT_SYSTEM_FOREGROUND,
            EVENT_SYSTEM_FOREGROUND,
            None,
            Some(on_foreground),
            0,
            0,
            WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
        )
    };
    if hook.is_invalid() {
        return Err("SetWinEventHook(EVENT_SYSTEM_FOREGROUND) failed".to_string());
    }
    let _hook = WinEventGuard(hook);

    // WPC-286: HWTEST_NO_POLL drops the 1 s monitor poll, which the real app would not run.
    if std::env::var_os("HWTEST_NO_POLL").is_none() {
        sys::set_timer(main, TIMER_POLL, POLL_MS)?;
    }
    sys::set_timer(main, TIMER_IDLE, IDLE_MS)?;
    sys::set_timer(main, TIMER_EXIT, opts.exit_after_secs.saturating_mul(1000))?;
    if let Some(ms) = opts.trigger_after_ms {
        sys::set_timer(main, TIMER_TRIGGER, ms.max(1))?;
        log::line(format!("timer trigger armed: {ms} ms"));
    }
    log::line(format!(
        "run: resident; {} or a left click on the tray icon opens the tools, a right click on it exits",
        hotkey.text
    ));

    message_loop()
}

fn register_main_class(hinstance: HINSTANCE) -> Result<(), String> {
    let wc = WNDCLASSEXW {
        cbSize: sys::size_u32::<WNDCLASSEXW>()?,
        lpfnWndProc: Some(main_proc),
        hInstance: hinstance,
        lpszClassName: MAIN_CLASS,
        ..Default::default()
    };
    // SAFETY: wc is fully initialized; the class name is a static wide string.
    if unsafe { RegisterClassExW(&wc) } == 0 {
        return Err(format!("RegisterClassExW(main): {}", windows::core::Error::from_thread()));
    }
    Ok(())
}

/// The Start menu shortcut runs `hwtest.exe run` again: hand the foreground right to the resident
/// instance and ask it to open the tools.
fn second_launch() -> Result<(), String> {
    log::line("run: another instance is resident; this is a second launch (Start menu path)");
    // SAFETY: plain call; lets the resident process take the foreground we got from the Start menu.
    match unsafe { AllowSetForegroundWindow(ASFW_ANY) } {
        Ok(()) => log::line("AllowSetForegroundWindow(ASFW_ANY): ok"),
        Err(e) => log::line(format!("AllowSetForegroundWindow(ASFW_ANY): {e}")),
    }
    // SAFETY: registers a message name; 0 means failure.
    let show = unsafe { RegisterWindowMessageW(SHOW_MESSAGE) };
    if show == 0 {
        return Err(format!("RegisterWindowMessageW(show): {}", windows::core::Error::from_thread()));
    }
    // SAFETY: plain lookup by class name.
    let target = unsafe { FindWindowW(MAIN_CLASS, PCWSTR::null()) }.ctx("FindWindowW(PixmeterHwtestMain)")?;
    // SAFETY: posts a registered message to the resident instance's main window.
    unsafe { PostMessageW(Some(target), show, WPARAM(0), LPARAM(0)) }.ctx("PostMessageW(show from Start)")?;
    log::line("posted the show message to the resident instance");
    Ok(())
}

fn message_loop() -> Result<(), String> {
    let mut msg = MSG::default();
    loop {
        // SAFETY: msg is a valid out pointer.
        let r = unsafe { GetMessageW(&mut msg, None, 0, 0) };
        match r.0 {
            0 => return Ok(()),
            -1 => return Err(format!("GetMessageW: {}", windows::core::Error::from_thread())),
            _ => {
                // SAFETY: msg came from GetMessageW. TranslateMessage's BOOL only says whether a
                // character message was produced, not an error.
                unsafe {
                    let _translated = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
        }
    }
}

/// Takes the app out of the thread_local before dropping it, so window messages sent during its
/// cleanup see "no app" instead of a borrowed cell.
struct AppGuard;

impl Drop for AppGuard {
    fn drop(&mut self) {
        let app = APP.with(|cell| match cell.try_borrow_mut() {
            Ok(mut slot) => slot.take(),
            Err(_) => {
                log::line("cleanup: app state still borrowed; it is left to thread exit");
                None
            }
        });
        if let Some(mut app) = app {
            app.shutdown();
            drop(app);
        }
    }
}

/// Marks the end of run_mode's cleanup for the console control handler.
struct CleanupDone;

impl Drop for CleanupDone {
    fn drop(&mut self) {
        CTRL_TARGET.store(0, Ordering::SeqCst);
        CLEANED_UP.store(true, Ordering::SeqCst);
    }
}

/// Removes the console control handler.
struct ConsoleHandlerGuard;

impl Drop for ConsoleHandlerGuard {
    fn drop(&mut self) {
        // SAFETY: removes the handler registered in run_mode.
        if let Err(e) = unsafe { SetConsoleCtrlHandler(Some(on_console_ctrl), false) } {
            log::line(format!("cleanup: SetConsoleCtrlHandler(remove): {e}"));
        }
    }
}

/// Runs on a thread Windows creates for the event. Posts WM_CLOSE to the main window, whose handler
/// leaves the message loop so every guard runs. For close, logoff and shutdown, Windows ends the process
/// as soon as this returns, so it waits (bounded) for the cleanup.
unsafe extern "system" fn on_console_ctrl(ctrl: u32) -> BOOL {
    match std::panic::catch_unwind(|| console_ctrl(ctrl)) {
        Ok(handled) => BOOL::from(handled),
        Err(_) => {
            log::line("ERROR: panic in the console control handler");
            BOOL::from(false)
        }
    }
}

fn console_ctrl(ctrl: u32) -> bool {
    let name = match ctrl {
        CTRL_C_EVENT => "Ctrl+C",
        CTRL_BREAK_EVENT => "Ctrl+Break",
        CTRL_CLOSE_EVENT => "console closed",
        CTRL_LOGOFF_EVENT => "logoff",
        CTRL_SHUTDOWN_EVENT => "shutdown",
        _ => "other",
    };
    let target = CTRL_TARGET.load(Ordering::SeqCst);
    if target == 0 {
        log::line(format!("console event {name}: no main window; default handling"));
        return false;
    }
    log::line(format!("console event {name}: asking the main window to exit so the cleanup runs"));
    // The integer is the main window's handle, stored by run_mode; it is only passed to PostMessageW.
    let hwnd = HWND(target as *mut std::ffi::c_void);
    // SAFETY: PostMessageW may be called from any thread.
    if let Err(e) = unsafe { PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0)) } {
        log::line(format!("console event {name}: PostMessageW(WM_CLOSE): {e}; default handling"));
        return false;
    }
    if matches!(ctrl, CTRL_CLOSE_EVENT | CTRL_LOGOFF_EVENT | CTRL_SHUTDOWN_EVENT) {
        let start = Instant::now();
        while !CLEANED_UP.load(Ordering::SeqCst) && start.elapsed() < CTRL_CLOSE_WAIT {
            std::thread::sleep(Duration::from_millis(50));
        }
        if !CLEANED_UP.load(Ordering::SeqCst) {
            log::line(format!("console event {name}: cleanup not finished after {CTRL_CLOSE_WAIT:?}"));
        }
    }
    true
}

enum IfBusy {
    Repost,
    Skip,
}

/// Runs `f` on the app state. If a handler up the stack holds it (a re-entrant message), the message is
/// posted again or skipped; it is never handled with aliased state.
fn with_app(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM, if_busy: IfBusy, f: impl FnOnce(&mut App)) {
    enum Problem {
        Missing,
        Busy,
    }
    let problem = APP.with(|cell| match cell.try_borrow_mut() {
        Ok(mut slot) => match slot.as_mut() {
            Some(app) => {
                f(app);
                None
            }
            None => Some(Problem::Missing),
        },
        Err(_) => Some(Problem::Busy),
    });
    let Some(problem) = problem else { return };
    match problem {
        Problem::Missing => {
            log::line(format!("message {msg:#x} arrived with no app state (startup or shutdown); ignored"))
        }
        Problem::Busy => match if_busy {
            IfBusy::Repost => {
                log::line(format!("message {msg:#x} arrived while the app state was busy; posted again"));
                // SAFETY: posts to our own window.
                if let Err(e) = unsafe { PostMessageW(Some(hwnd), msg, wparam, lparam) } {
                    log::line(format!("ERROR: re-posting message {msg:#x}: {e}"));
                }
            }
            IfBusy::Skip => log::line(format!("message {msg:#x} skipped: the app state was busy")),
        },
    }
}

unsafe extern "system" fn main_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match std::panic::catch_unwind(AssertUnwindSafe(|| main_message(hwnd, msg, wparam, lparam))) {
        Ok(r) => r,
        Err(_) => {
            log::line(format!("ERROR: panic in the main window procedure (message {msg:#x})"));
            // SAFETY: default handling for our own window.
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
    }
}

fn main_message(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // SAFETY: default handling for our own window.
    let default = || unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
    let show = MSG_SHOW_FROM_START.with(Cell::get);
    let taskbar_created = MSG_TASKBAR_CREATED.with(Cell::get);
    match msg {
        WM_APP_TRAY => {
            tray_event(lparam);
            LRESULT(0)
        }
        WM_HOTKEY => {
            if wparam.0 == usize::try_from(HOTKEY_ID).unwrap_or(usize::MAX) {
                queue_trigger(hwnd, Source::Hotkey);
            } else {
                log::line(format!("WM_HOTKEY with unknown id {}", wparam.0));
            }
            LRESULT(0)
        }
        _ if show != 0 && msg == show => {
            queue_trigger(hwnd, Source::Start);
            LRESULT(0)
        }
        _ if taskbar_created != 0 && msg == taskbar_created => {
            log::line("TaskbarCreated: Explorer restarted; adding the tray icon again");
            with_app(hwnd, msg, wparam, lparam, IfBusy::Repost, App::taskbar_created);
            LRESULT(0)
        }
        WM_APP_TRIGGER => {
            match (Source::from_code(wparam.0), sys::ms_from_lparam(lparam)) {
                (Some(source), Ok(t0)) => {
                    with_app(hwnd, msg, wparam, lparam, IfBusy::Repost, |app| app.trigger(source, t0));
                }
                (source, t0) => log::line(format!("ERROR: bad trigger message: source {source:?}, t0 {t0:?}")),
            }
            LRESULT(0)
        }
        WM_APP_CLOSE => {
            match (CloseReason::from_code(wparam.0), sys::ms_from_lparam(lparam)) {
                (Some(reason), Ok(at)) => {
                    with_app(hwnd, msg, wparam, lparam, IfBusy::Repost, |app| app.close(reason, at));
                }
                (reason, at) => log::line(format!("ERROR: bad close message: reason {reason:?}, at {at:?}")),
            }
            LRESULT(0)
        }
        WM_TIMER => {
            // A busy state only happens re-entrantly; the timer fires again on its own.
            with_app(hwnd, msg, wparam, lparam, IfBusy::Skip, |app| app.timer(wparam.0));
            LRESULT(0)
        }
        WM_DISPLAYCHANGE => {
            note_signal("main", msg, wparam, lparam);
            tools::request_close(CloseReason::MonitorsChanged);
            default()
        }
        WM_DPICHANGED | WM_SETTINGCHANGE | WM_DEVICECHANGE => {
            note_signal("main", msg, wparam, lparam);
            default()
        }
        WM_ACTIVATEAPP => {
            log_activate_app("main", wparam, lparam);
            default()
        }
        WM_CLOSE => {
            // From the console handler (Ctrl+C, console closed) or `taskkill` without /F. Leaving the loop,
            // instead of DefWindowProc's DestroyWindow, keeps the cleanup order of run_mode's guards.
            log::line("WM_CLOSE on the main window: exiting");
            // SAFETY: plain call on this thread.
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        WM_DESTROY => {
            log::line("main window destroyed; leaving the message loop");
            // SAFETY: plain call on this thread.
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        _ => default(),
    }
}

/// NOTIFYICON_VERSION_4: LOWORD(lParam) is the event, HIWORD(lParam) the icon id.
fn tray_event(lparam: LPARAM) {
    let bits = sys::lparam_bits(lparam);
    let event = u32::from(sys::loword(bits));
    let icon = sys::hiword(bits);
    // Hover traffic would flood the log.
    if matches!(event, WM_MOUSEMOVE | NIN_POPUPOPEN | NIN_POPUPCLOSE) {
        return;
    }
    let name = match event {
        NIN_SELECT => "NIN_SELECT",
        NIN_KEYSELECT => "NIN_KEYSELECT",
        WM_CONTEXTMENU => "WM_CONTEXTMENU",
        WM_LBUTTONDOWN => "WM_LBUTTONDOWN",
        WM_LBUTTONUP => "WM_LBUTTONUP",
        WM_LBUTTONDBLCLK => "WM_LBUTTONDBLCLK",
        WM_RBUTTONDOWN => "WM_RBUTTONDOWN",
        WM_RBUTTONUP => "WM_RBUTTONUP",
        _ => "other",
    };
    // SAFETY: plain query about the message being handled.
    let how = unsafe { InSendMessageEx(None) };
    log::line(format!("tray event {name} ({event:#x}) icon {icon}, delivered {}", describe_send(how)));
    match event {
        NIN_SELECT | NIN_KEYSELECT => queue_trigger(MAIN_HWND.with(Cell::get), Source::Tray),
        WM_CONTEXTMENU => {
            log::line("tray right click: exiting");
            // SAFETY: plain call on this thread.
            unsafe { PostQuitMessage(0) };
        }
        _ => log::line("tray event logged only"),
    }
}

fn describe_send(how: u32) -> String {
    if how == ISMEX_NOSEND {
        return "posted".to_string();
    }
    let mut parts = Vec::new();
    if how & ISMEX_SEND != 0 {
        parts.push("sent, sender waiting");
    }
    if how & ISMEX_NOTIFY != 0 {
        parts.push("sent with SendNotifyMessage");
    }
    if how & ISMEX_CALLBACK != 0 {
        parts.push("sent with SendMessageCallback");
    }
    if how & ISMEX_REPLIED != 0 {
        parts.push("already replied");
    }
    parts.join(", ")
}

/// t0 is taken here, on arrival. The work runs from a posted message, so a sender that waits on this
/// message (Explorer, for the tray) is released at once and its panels are not held up by the capture.
fn queue_trigger(hwnd: HWND, source: Source) {
    let t0 = log::now_ms();
    let lparam = match sys::lparam_from_ms(t0) {
        Ok(l) => l,
        Err(e) => {
            log::line(format!("ERROR: trigger {}: {e}", source.name()));
            return;
        }
    };
    // SAFETY: posts to our own window.
    if let Err(e) = unsafe { PostMessageW(Some(hwnd), WM_APP_TRIGGER, WPARAM(source.code()), lparam) } {
        log::line(format!("ERROR: queuing trigger {}: {e}", source.name()));
    }
}

/// Logs a monitor-change message on any of our windows and records it for the next poll.
fn note_signal(role: &str, msg: u32, wparam: WPARAM, lparam: LPARAM) {
    let w = sys::wparam_bits(wparam);
    let l = sys::lparam_bits(lparam);
    let (name, detail) = match msg {
        WM_DISPLAYCHANGE => ("WM_DISPLAYCHANGE", format!("{w} bpp, {}x{}", sys::loword(l), sys::hiword(l))),
        WM_DPICHANGED => (
            "WM_DPICHANGED",
            format!("dpi {}x{}, suggested {}", sys::loword(w), sys::hiword(w), suggested_rect(lparam)),
        ),
        WM_SETTINGCHANGE => ("WM_SETTINGCHANGE", format!("wParam {w:#x}, lParam {}", setting_string(lparam))),
        WM_DEVICECHANGE => ("WM_DEVICECHANGE", format!("event {w:#x}")),
        _ => ("other", format!("message {msg:#x}")),
    };
    let at = log::now_ms();
    log::line(format!("signal {name} on {role}: {detail}"));
    SIGNALS.with(|cell| match cell.try_borrow_mut() {
        Ok(mut list) => list.push(Signal { at, name, role: role.to_string() }),
        Err(_) => log::line("signal buffer busy; this signal is logged but not recorded for the poll"),
    });
}

fn suggested_rect(lparam: LPARAM) -> String {
    let p = lparam.0 as *const RECT;
    if p.is_null() {
        return "none".to_string();
    }
    // SAFETY: WM_DPICHANGED's lParam points to the suggested RECT for the duration of the message.
    let r = unsafe { *p };
    format!("({},{})-({},{})", r.left, r.top, r.right, r.bottom)
}

/// WM_SETTINGCHANGE's lParam is NULL or a NUL-terminated string naming the changed area.
pub(crate) fn setting_string(lparam: LPARAM) -> String {
    let p = lparam.0 as *const u16;
    if p.is_null() {
        return "null".to_string();
    }
    let mut units = Vec::new();
    for i in 0..256 {
        // SAFETY: a non-null lParam is a NUL-terminated string; reading stops at NUL or 256 units.
        let c = unsafe { *p.add(i) };
        if c == 0 {
            break;
        }
        units.push(c);
    }
    format!("\"{}\"", String::from_utf16_lossy(&units))
}

fn log_activate_app(role: &str, wparam: WPARAM, lparam: LPARAM) {
    let state = if wparam.0 != 0 { "activated" } else { "deactivated" };
    log::line(format!("WM_ACTIVATEAPP on {role}: {state}, other thread {}", lparam.0));
}

fn take_signals() -> Vec<Signal> {
    SIGNALS.with(|cell| match cell.try_borrow_mut() {
        Ok(mut list) => std::mem::take(&mut *list),
        Err(_) => {
            log::line("signal buffer busy at poll; read next time");
            Vec::new()
        }
    })
}

unsafe extern "system" fn on_foreground(
    _hook: HWINEVENTHOOK,
    event: u32,
    hwnd: HWND,
    _id_object: i32,
    _id_child: i32,
    _event_thread: u32,
    event_tick: u32,
) {
    if std::panic::catch_unwind(AssertUnwindSafe(|| foreground_event(event, hwnd, event_tick))).is_err() {
        log::line("ERROR: panic in the foreground WinEvent callback");
    }
}

/// Out-of-context WinEvents are queued and delivered when this thread pumps messages, possibly long after
/// the change (for example after the 1.5 s series). The event's own tick decides whether it happened
/// after the toolbar took the foreground.
fn foreground_event(event: u32, hwnd: HWND, event_tick: u32) {
    if event != EVENT_SYSTEM_FOREGROUND {
        log::line(format!("WinEvent {event:#x} not asked for; ignored"));
        return;
    }
    let mut pid = 0u32;
    // SAFETY: pid is a valid out pointer. On failure pid stays 0, which the check below treats as
    // "not ours"; the window description logged next spells the failure out.
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    // SAFETY: plain query.
    if pid != 0 && pid == unsafe { GetCurrentProcessId() } {
        log::line("foreground event for our own process despite WINEVENT_SKIPOWNPROCESS; ignored");
        return;
    }
    let since_trigger = TRIGGER_TICK.with(Cell::get).map(|t| event_tick.wrapping_sub(t));
    let after_trigger = since_trigger.map_or_else(|| "?".to_string(), |d| d.cast_signed().to_string());
    match FG_GATE.with(Cell::get) {
        Some(gate) if event_tick.wrapping_sub(gate).cast_signed() >= 0 => {
            log::line(format!(
                "foreground left the process -> {} (event at +{after_trigger} ms after the trigger)",
                sys::describe_window(hwnd)
            ));
            tools::request_close(CloseReason::Foreground);
        }
        Some(gate) => log::line(format!(
            "foreground -> {} (event at +{after_trigger} ms after the trigger, {} ms before the toolbar took the foreground; delivered late, ignored)",
            sys::describe_window(hwnd),
            gate.wrapping_sub(event_tick)
        )),
        None => {
            if since_trigger.is_some_and(|d| d <= FOREGROUND_LOG_WINDOW_MS) {
                log::line(format!(
                    "foreground -> {} (tools closed; {} ms after the trigger)",
                    sys::describe_window(hwnd),
                    since_trigger.unwrap_or(0)
                ));
            }
        }
    }
}
