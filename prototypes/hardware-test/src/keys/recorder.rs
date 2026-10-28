//! The `recorder` mode: the prototype of Pixmeter's Settings shortcut recorder. It records a combination that
//! includes the Win key without the Start menu opening, then checks that Windows accepts registering it.
//!
//! Approach chosen: swallow every key event while recording (low-level keyboard hook returning 1), track the
//! modifiers inside the hook, and keep the hook until every swallowed key has been released. This is how
//! Microsoft PowerToys records shortcuts. Sources, pinned to PowerToys commit 16b3ffd31e2bf96c25b3c4c27bc45df8705b1ff6:
//! - https://github.com/microsoft/PowerToys/blob/16b3ffd31e2bf96c25b3c4c27bc45df8705b1ff6/src/common/interop/KeyboardHook.cpp
//!   (the hook returns 1 for every event while the control is active)
//! - https://github.com/microsoft/PowerToys/blob/16b3ffd31e2bf96c25b3c4c27bc45df8705b1ff6/src/settings-ui/Settings.UI/SettingsXAML/Controls/ShortcutControl/ShortcutControl.xaml.cs
//!   (the control tracks modifier state itself; keys already down on entering are handled apart)
//! - https://github.com/microsoft/PowerToys/blob/16b3ffd31e2bf96c25b3c4c27bc45df8705b1ff6/src/modules/keyboardmanager/KeyboardManagerEditorUI/Helpers/KeyboardHookHelper.cs
//!   (the Keyboard Manager editor uses the same hook; it defers unhooking out of the callback)
//!
//! The other approach, the "dummy key" (inject VK 0xFF between Win-down and Win-up), is what PowerToys' remapping
//! engine uses (keyboardmanager/common/Helpers.cpp `SetDummyKeyEvent`, constant `DUMMY_KEY = 0xFF`). It only stops
//! Start from opening; Win+<key> would still run the shell's shortcut while recording, and it needs SendInput.

use std::cell::Cell;
use std::thread;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, CreateFontW, DEFAULT_CHARSET, DeleteObject, EndPaint,
    InvalidateRect, OUT_DEFAULT_PRECIS, PAINTSTRUCT, SelectObject, SetBkMode, TRANSPARENT, TextOutW,
};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::{DefWindowProcW, IsWindow, PostQuitMessage, WM_DESTROY, WM_HOTKEY, WM_PAINT};
use windows::core::w;

use super::combo::{self, Combo, Registration};
use super::foreground::Watcher;
use super::llhook::{self, ALT, CTRL, HookHandle, HookMsg, KeyNote, Recorded, SHIFT, WIN};
use super::window::{self, Kind};
use crate::common::{Args, Ctx};
use crate::log;

const PHASES: &[&str] = &["before-recording", "recording", "after-recording"];
const BEFORE: usize = 0;
const RECORDING: usize = 1;
const AFTER: usize = 2;

/// How long after the hook is removed a foreground change still counts as caused by the recording.
const GRACE: Duration = Duration::from_millis(500);

thread_local! {
    static LINES: Cell<Vec<String>> = const { Cell::new(Vec::new()) };
    static PRESSES: Cell<u32> = const { Cell::new(0) };
}

unsafe extern "system" fn recorder_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_PAINT => {
            paint(hwnd);
            LRESULT(0)
        }
        WM_HOTKEY => {
            let n = PRESSES.get() + 1;
            PRESSES.set(n);
            let packed = lparam.0.cast_unsigned();
            log::line(format!(
                "WM_HOTKEY #{n} id={} mods=0x{:X} vk=0x{:X}",
                wparam.0,
                packed & 0xFFFF,
                (packed >> 16) & 0xFFFF
            ));
            LRESULT(0)
        }
        WM_DESTROY => {
            // SAFETY: plain call; ends the main loop when the user closes the window.
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        // SAFETY: forwards the message we were given, unchanged.
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

fn window_dpi(hwnd: HWND) -> Result<i32, String> {
    // SAFETY: plain query; 0 means the window handle is invalid.
    match unsafe { GetDpiForWindow(hwnd) } {
        0 => Err("GetDpiForWindow returned 0".to_string()),
        dpi => i32::try_from(dpi).ctx("dpi"),
    }
}

fn paint(hwnd: HWND) {
    let mut ps = PAINTSTRUCT::default();
    // SAFETY: `ps` is a valid out-parameter; paired with EndPaint below.
    let hdc = unsafe { BeginPaint(hwnd, &mut ps) };
    if hdc.is_invalid() {
        log::line("ERROR: BeginPaint returned no device context");
        return;
    }
    let dpi = window_dpi(hwnd).unwrap_or_else(|e| {
        log::line(format!("ERROR: {e}; painting at 96 dpi"));
        96
    });
    let line_height = 24 * dpi / 96;
    // SAFETY: creates a font that is deleted below.
    let font = unsafe {
        CreateFontW(
            -(17 * dpi / 96),
            0,
            0,
            0,
            400, // FW_NORMAL
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            CLEARTYPE_QUALITY,
            0,
            w!("Segoe UI"),
        )
    };
    // SAFETY: `hdc` is valid until EndPaint; the previous font is selected back before the font is deleted.
    let previous = unsafe { SelectObject(hdc, font.into()) };
    // SAFETY: `hdc` is valid.
    unsafe { SetBkMode(hdc, TRANSPARENT) };
    let lines = LINES.take();
    let mut y = 12 * dpi / 96;
    for line in &lines {
        let text: Vec<u16> = line.encode_utf16().collect();
        // SAFETY: `hdc` is valid; `text` outlives the call.
        if !unsafe { TextOutW(hdc, 14 * dpi / 96, y, &text) }.as_bool() {
            log::line("ERROR: TextOutW failed while painting");
        }
        y += line_height;
    }
    LINES.set(lines);
    // SAFETY: restores the device context's font, then frees ours and ends the paint started above.
    unsafe {
        SelectObject(hdc, previous);
        if !DeleteObject(font.into()).as_bool() {
            log::line("ERROR: DeleteObject failed for the recorder font");
        }
        // EndPaint always returns TRUE.
        _ = EndPaint(hwnd, &ps);
    }
}

/// Replaces the window text and asks for a repaint.
fn show(hwnd: HWND, lines: &[String]) {
    LINES.set(lines.to_vec());
    // SAFETY: plain query. The user can close the window while the run goes on.
    if !unsafe { IsWindow(Some(hwnd)) }.as_bool() {
        return;
    }
    // SAFETY: `hwnd` is the live recorder window of this thread.
    if !unsafe { InvalidateRect(Some(hwnd), None, true) }.as_bool() {
        log::line("ERROR: InvalidateRect failed");
    }
}

fn mods_text(held: u8) -> String {
    combo::mods_label(Combo::new(held & WIN != 0, held & CTRL != 0, held & ALT != 0, held & SHIFT != 0, 0).mods)
}

fn vk_name(vk: u32) -> String {
    match vk {
        0x5B => "LWin".into(),
        0x5C => "RWin".into(),
        0xA0 => "LShift".into(),
        0xA1 => "RShift".into(),
        0xA2 => "LCtrl".into(),
        0xA3 => "RCtrl".into(),
        0xA4 => "LAlt".into(),
        0xA5 => "RAlt".into(),
        0x1B => "Esc".into(),
        0x0D => "Enter".into(),
        other => combo::key_label(other),
    }
}

fn vk_list(keys: &[u32]) -> String {
    keys.iter().map(|vk| format!("{}(0x{vk:02X})", vk_name(*vk))).collect::<Vec<_>>().join(", ")
}

/// Pumps messages for `span`, so the window stays responsive.
fn pump_for(span: Duration) -> Result<(), String> {
    let end = Instant::now() + span;
    while Instant::now() < end {
        let left = end.saturating_duration_since(Instant::now());
        if window::pump(u32::try_from(left.as_millis().min(25)).ctx("wait slice")?)? {
            break;
        }
    }
    Ok(())
}

/// Last resort: if the main thread stalls, end the process so the OS removes the hook.
fn spawn_watchdog(limit: Duration) -> Result<(), String> {
    thread::Builder::new()
        .name("watchdog".into())
        .spawn(move || {
            thread::sleep(limit);
            log::line("ERROR: watchdog: the recorder is still running past its limit; exiting so the OS removes the hook");
            std::process::exit(3);
        })
        .map(drop)
        .ctx("spawn watchdog")
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Outcome {
    Pending,
    Recorded,
    Cancelled,
}

/// What the main thread knows about the recording while it runs.
struct Session<'a> {
    hwnd: HWND,
    watcher: &'a Watcher,
    outcome: Outcome,
    recorded: Option<Recorded>,
    registration: Option<Registration>,
    hook_removed_at: Option<Instant>,
    start_result_logged: bool,
    errors: Vec<String>,
    lines: [String; 5],
}

impl Session<'_> {
    fn refresh(&self) {
        show(self.hwnd, &self.lines);
    }

    fn on_key(&mut self, k: &KeyNote) {
        let flags = format!("{}{}", if k.injected { " injected" } else { "" }, if k.extended { " extended" } else { "" });
        log::line(format!(
            "key {} {} vk=0x{:02X} scan=0x{:02X}{flags} -> {} ({}) held=[{}] at {:.3} ms",
            if k.down { "down" } else { "up  " },
            vk_name(k.vk),
            k.vk,
            k.scan,
            if k.swallowed { "SWALLOWED" } else { "passed" },
            k.note,
            mods_text(k.held),
            k.t_ms
        ));
        if self.outcome == Outcome::Pending {
            self.lines[1] = format!("Recording... held: {}", mods_text(k.held));
            self.refresh();
        }
    }

    fn on_recorded(&mut self, r: Recorded) {
        let combo = Combo::new(r.held & WIN != 0, r.held & CTRL != 0, r.held & ALT != 0, r.held & SHIFT != 0, r.vk);
        self.outcome = Outcome::Recorded;
        self.recorded = Some(r);
        log::line(format!("RESULT recorder combination: {combo} (vk=0x{:02X}, recorded at {:.3} ms)", r.vk, r.t_ms));
        self.lines[1] = format!("Recorded: {combo}  (releasing keys...)");
        self.refresh();
    }

    /// The hook thread has ended: the keyboard is back to normal. Register what was recorded.
    fn on_hook_removed(&mut self) {
        self.hook_removed_at = Some(Instant::now());
        log::line("low-level keyboard hook removed; keys reach the system again");
        match (self.outcome, self.recorded) {
            (Outcome::Recorded, Some(r)) => self.register(r),
            (Outcome::Cancelled, _) => {
                log::line("RESULT recorder register: skipped, recording was cancelled with Esc");
                self.lines[1] = "Cancelled (Esc).".to_string();
                self.refresh();
            }
            _ => {
                log::line("RESULT recorder combination: none, the hook ended before a combination was recorded");
                self.lines[1] = "No combination recorded.".to_string();
                self.refresh();
            }
        }
    }

    fn register(&mut self, r: Recorded) {
        let combo = Combo::new(r.held & WIN != 0, r.held & CTRL != 0, r.held & ALT != 0, r.held & SHIFT != 0, r.vk);
        self.lines[1] = format!("Recorded: {combo}");
        if !combo.has_win() {
            log::line(format!("note: {combo} has no Win key; the recorder still handled it the same way"));
        }
        if combo.mods.0 == 0 {
            log::line(
                "RESULT recorder register: skipped, no modifier was held (a bare key registered system-wide would \
                 take that key away from every program)",
            );
            self.lines[2] = "RegisterHotKey: skipped (no modifier held)".to_string();
            self.refresh();
            return;
        }
        match combo::register(Some(self.hwnd), 1, combo) {
            Ok(reg) => {
                log::line(format!("RESULT recorder register {combo}: registered"));
                self.lines[2] = format!("RegisterHotKey: registered. Press {combo} to test.");
                self.registration = Some(reg);
            }
            Err(e) => {
                log::line(format!("RESULT recorder register {combo}: failed, {e}"));
                self.lines[2] = format!("RegisterHotKey failed: error {}", e.win32);
            }
        }
        self.refresh();
    }

    /// Start-menu verdict for the recording interval (hook installed until `GRACE` after it was removed).
    fn log_start_result(&mut self) {
        if self.start_result_logged {
            return;
        }
        self.start_result_logged = true;
        let starts = self.watcher.start_like(RECORDING);
        let changes = self.watcher.changes(RECORDING);
        let verdict = if starts == 0 { "no" } else { "yes" };
        log::line(format!(
            "RESULT recorder start-menu-opened: {verdict} (foreground changes during recording: {changes}, to \
             StartMenuExperienceHost.exe or SearchHost.exe: {starts})"
        ));
        self.lines[3] = format!("Start menu opened during recording: {verdict}");
        self.watcher.set_phase(AFTER);
        self.refresh();
    }

    fn on_msg(&mut self, hook: &mut HookHandle, msg: HookMsg) {
        match msg {
            HookMsg::Installed { .. } => {}
            HookMsg::Key(k) => self.on_key(&k),
            HookMsg::Recorded(r) => self.on_recorded(r),
            HookMsg::Cancelled { t_ms } => {
                self.outcome = Outcome::Cancelled;
                log::line(format!("RESULT recorder combination: cancelled with Esc at {t_ms:.3} ms"));
                self.lines[1] = "Cancelled (Esc). Releasing keys...".to_string();
                self.refresh();
            }
            HookMsg::Drained { t_ms } => {
                log::line(format!("every swallowed key was released at {t_ms:.3} ms; the hook thread removes the hook"));
            }
            HookMsg::Finished => {
                hook.finished = true;
                hook.join();
                self.on_hook_removed();
            }
            HookMsg::Failed(e) => {
                log::line(format!("ERROR: keyboard hook: {e}"));
                self.errors.push(e);
            }
        }
    }
}

pub fn run(args: &Args) -> Result<(), String> {
    let secs = args.num("secs", 30)?;
    let deadline = Instant::now() + Duration::from_secs(secs);
    spawn_watchdog(Duration::from_secs(secs + 5))?;

    log::line("RESULT recorder approach: swallow-all low-level hook (see the lines below for why)");
    log::line(
        "approach: while recording, a WH_KEYBOARD_LL hook on its own thread returns 1 for every key event, so \
         neither the shell nor any program sees the Win key (no Start menu, no Win+E / Win+D shortcuts). It tracks \
         the modifiers itself, because swallowed keys do not update GetAsyncKeyState. A key-up is swallowed only if \
         its key-down was swallowed; the hook stays until every swallowed key is released, then removes itself.",
    );
    log::line(
        "why: this is how PowerToys records shortcuts (KeyboardHook.cpp returns 1 while active; \
         ShortcutControl.xaml.cs tracks the modifiers). The dummy-key trick (inject VK 0xFF before Win-up, \
         PowerToys Helpers.cpp SetDummyKeyEvent) only stops Start; Win+<key> would still fire while recording.",
    );
    log::line(format!(
        "safety: the hook is removed after {secs} s at the latest, Esc cancels, closing the window ends the run, \
         a watchdog exits the process at {} s",
        secs + 5
    ));

    let window = window::create(
        w!("hwtest_recorder"),
        w!("hwtest recorder"),
        Some(recorder_proc),
        Kind::Visible { width: 640, height: 230 },
    )?;
    let hwnd = window.hwnd();
    let watcher = Watcher::start(PHASES)?;
    watcher.set_phase(BEFORE);

    let mut session = Session {
        hwnd,
        watcher: &watcher,
        outcome: Outcome::Pending,
        recorded: None,
        registration: None,
        hook_removed_at: None,
        start_result_logged: false,
        errors: Vec::new(),
        lines: [
            "Press a combination with the Win key. Esc cancels.".to_string(),
            "Getting ready...".to_string(),
            String::new(),
            String::new(),
            String::new(),
        ],
    };
    session.refresh();

    // Let the window settle and the key that launched the program (Enter) come up, so neither is recorded.
    pump_for(Duration::from_millis(300))?;
    let release_by = Instant::now() + Duration::from_secs(5);
    let mut down = llhook::down_keys();
    if !down.is_empty() {
        log::line(format!("waiting up to 5 s for all keys to be released: {}", vk_list(&down)));
        session.lines[1] = "Release all keys...".to_string();
        session.refresh();
    }
    while !down.is_empty() && Instant::now() < release_by {
        pump_for(Duration::from_millis(20))?;
        down = llhook::down_keys();
    }
    if !down.is_empty() {
        log::line(format!(
            "WARNING: still down after 5 s: {}; their events will pass through and are not recorded",
            vk_list(&down)
        ));
    }

    let mut hook = HookHandle::start(llhook::to_bits(&down))?;
    watcher.set_phase(RECORDING);
    log::line("RECORDING: hook installed, every key is swallowed until a combination is recorded or Esc is pressed");
    session.lines[1] = "Recording... held:".to_string();
    session.refresh();

    let mut quit = false;
    let mut last_second = u64::MAX;
    while !quit && Instant::now() < deadline {
        quit = window::pump(25)?;

        let ended_before_drain = hook.thread_ended() && !hook.finished;
        while let Ok(msg) = hook.rx.try_recv() {
            session.on_msg(&mut hook, msg);
        }
        if ended_before_drain && !hook.finished {
            return Err("the keyboard hook thread ended without reporting that the hook was removed".to_string());
        }

        if session.hook_removed_at.is_some_and(|at| at.elapsed() >= GRACE) {
            session.log_start_result();
        }

        let left = deadline.saturating_duration_since(Instant::now()).as_secs();
        if left != last_second {
            last_second = left;
            session.lines[4] = format!("Seconds left: {left}");
            session.refresh();
        }
    }

    // Deadline or closed window: the hook must not survive this function.
    if !hook.finished {
        log::line(if quit {
            "window closed while recording; stopping the hook"
        } else {
            "--secs elapsed while recording; stopping the hook"
        });
        hook.request_stop()?;
        hook.join();
        while let Ok(msg) = hook.rx.try_recv() {
            session.on_msg(&mut hook, msg);
        }
        if !hook.finished {
            return Err("the keyboard hook thread ended without reporting that the hook was removed".to_string());
        }
    }
    session.log_start_result();

    // The detector must have been seen to fire at least once, or "no" above proves nothing.
    let control = watcher.start_like(AFTER);
    log::line(format!(
        "RESULT recorder start-detector-control: Start/Search seen after recording: {} (press Win alone once \
         after the recording; \"no\" here means the control was not done, not that the detector is broken)",
        if control > 0 { "yes" } else { "no" }
    ));
    log::line(format!(
        "RESULT recorder hotkey-presses: WM_HOTKEY={} (foreground changes after recording: {})",
        PRESSES.get(),
        watcher.changes(AFTER)
    ));

    if let Some(reg) = session.registration.take() {
        reg.unregister()?;
        log::line("unregistered the recorded combination");
    }
    if session.errors.is_empty() { Ok(()) } else { Err(session.errors.join("; ")) }
}
