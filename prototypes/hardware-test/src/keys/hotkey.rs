//! The `hotkey` and `hotkey-child` modes: RegisterHotKey on Win+Ctrl+M, conflict detection from a second process,
//! combinations Windows owns, and (with `--wait-secs`) the presses and foreground changes.

use std::cell::Cell;
use std::process::Command;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{DefWindowProcW, WM_HOTKEY};
use windows::core::w;

use super::combo::{self, Combo};
use super::foreground::Watcher;
use super::window::{self, Kind};
use crate::common::{Args, Ctx};
use crate::log;

/// What the real app would register by default.
const COMBO: &str = "win+ctrl+m";
/// The parent holds this one for the conflict test, so the test means something even when COMBO is taken by a
/// third program. Nothing known uses it.
const HELD_COMBO: &str = "win+ctrl+alt+8";
/// Nobody holds this one: a child registering it must succeed, which shows the test can also say "free".
const FREE_COMBO: &str = "win+ctrl+alt+9";

const ERROR_HOTKEY_ALREADY_REGISTERED: u32 = 1409;

/// Combinations Windows handles itself, as (label, combo text).
const WINDOWS_OWNED: [(&str, &str); 9] = [
    ("Win+E", "win+e"),
    ("Win+D", "win+d"),
    ("Win+L", "win+l"),
    ("Win+V", "win+v"),
    ("Win+Shift+S", "win+shift+s"),
    ("Win+Shift+M", "win+shift+m"),
    ("Win+Ctrl+D", "win+ctrl+d"),
    ("Win+Alt+R", "win+alt+r"),
    ("Win+Ctrl+Shift+B", "win+ctrl+shift+b"),
];

thread_local! {
    static PRESSES: Cell<u32> = const { Cell::new(0) };
}

unsafe extern "system" fn hotkey_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if msg == WM_HOTKEY {
        let n = PRESSES.get() + 1;
        PRESSES.set(n);
        // LPARAM: low word = modifiers, high word = virtual-key code.
        let packed = lparam.0.cast_unsigned();
        log::line(format!(
            "WM_HOTKEY #{n} id={} mods=0x{:X} vk=0x{:X}",
            wparam.0,
            packed & 0xFFFF,
            (packed >> 16) & 0xFFFF
        ));
        return LRESULT(0);
    }
    // SAFETY: forwards the message we were given, unchanged.
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

pub fn run(args: &Args) -> Result<(), String> {
    let wait_secs = args.num("wait-secs", 0)?;
    let combo = Combo::parse(COMBO)?;
    let window = window::create(w!("hwtest_hotkey"), w!("hwtest hotkey"), Some(hotkey_proc), Kind::Hidden)?;
    let hwnd = window.hwnd();
    let watcher = Watcher::start(&["setup", "wait"])?;

    // 1. Register Win+Ctrl+M, keep it for the whole run.
    log::line(format!("registering {combo} (with MOD_NOREPEAT) on a hidden message-only window"));
    let held = match combo::register(Some(hwnd), 1, combo) {
        Ok(reg) => {
            log::line(format!("RESULT hotkey register {combo}: registered"));
            Some(reg)
        }
        Err(e) => {
            log::line(format!("RESULT hotkey register {combo}: failed, {e}"));
            None
        }
    };

    if held.is_some() {
        same_process_duplicate(hwnd, combo)?;
    }

    // 2. A second process tries the same combination, then one this process certainly holds, then a free one.
    let child_code = run_child_process(COMBO)?;
    let (held_combo, free_combo) = (Combo::parse(HELD_COMBO)?, Combo::parse(FREE_COMBO)?);
    let held_test = match combo::register(Some(hwnd), 3, held_combo) {
        Ok(reg) => Some(reg),
        Err(e) => {
            log::line(format!("RESULT hotkey conflict-setup {held_combo}: the first process could not register it, {e}"));
            None
        }
    };
    let held_code = if held_test.is_some() { Some(run_child_process(HELD_COMBO)?) } else { None };
    if let Some(reg) = held_test {
        reg.unregister()?;
    }
    let free_code = run_child_process(FREE_COMBO)?;
    report_conflict(combo, held.is_some(), child_code, (held_combo, held_code), (free_combo, free_code));

    // 3. Combinations Windows owns.
    windows_owned(hwnd)?;

    // 4. Optional wait for presses.
    watcher.set_phase(1);
    if wait_secs == 0 {
        log::line("RESULT hotkey presses: not waited (--wait-secs 0)");
    } else {
        log::line(format!("waiting {wait_secs} s: press {combo} (and any combination you want to try)"));
        let deadline = Instant::now() + Duration::from_secs(wait_secs);
        let mut quit = false;
        while !quit && Instant::now() < deadline {
            let left = deadline.saturating_duration_since(Instant::now());
            quit = window::pump(u32::try_from(left.as_millis().min(250)).ctx("wait slice")?)?;
        }
        log::line(format!(
            "RESULT hotkey presses: WM_HOTKEY={} foreground-changes={} start-or-search-windows={} (waited {wait_secs} s)",
            PRESSES.get(),
            watcher.changes(1),
            watcher.start_like(1)
        ));
    }

    // 5. Release everything. The guards would do it too on an error path; this reports failures.
    if let Some(reg) = held {
        reg.unregister()?;
        log::line(format!("unregistered {combo}"));
    }
    Ok(())
}

/// Same process, same window: registers a combination this window already holds a second time.
fn same_process_duplicate(hwnd: HWND, combo: Combo) -> Result<(), String> {
    match combo::register(Some(hwnd), 2, combo) {
        Ok(second) => {
            log::line(format!("RESULT hotkey same-process-duplicate {combo}: registered a second time (no error)"));
            second.unregister()
        }
        Err(e) => {
            log::line(format!("RESULT hotkey same-process-duplicate {combo}: second registration failed, {e}"));
            Ok(())
        }
    }
}

/// Runs `hwtest hotkey-child --combo <text>` and returns the code it printed on its `CHILD_RESULT` line.
fn run_child_process(combo_text: &str) -> Result<u32, String> {
    let exe = std::env::current_exe().ctx("current_exe")?;
    log::line(format!("starting second process: hotkey-child --combo {combo_text}"));
    let out = Command::new(&exe)
        .args(["hotkey-child", "--combo", combo_text])
        .output()
        .ctx("run hotkey-child")?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    for line in stdout.lines() {
        log::line(format!("  child> {line}"));
    }
    let stderr = String::from_utf8_lossy(&out.stderr);
    for line in stderr.lines() {
        log::line(format!("  child stderr> {line}"));
    }
    log::line(format!("second process exited: {}", out.status));
    // The child's log lines carry a timestamp prefix, so look for the marker anywhere in a line.
    let code = stdout
        .lines()
        .find_map(|line| line.split_once("CHILD_RESULT ").map(|(_, rest)| rest.trim().to_string()))
        .ok_or_else(|| format!("hotkey-child {combo_text}: no CHILD_RESULT line in its output"))?;
    code.parse::<u32>().map_err(|e| format!("hotkey-child {combo_text}: CHILD_RESULT '{code}': {e}"))
}

fn report_conflict(combo: Combo, parent_holds: bool, child_code: u32, held: (Combo, Option<u32>), free: (Combo, u32)) {
    let ((held_combo, held_code), (free_combo, free_code)) = (held, free);
    let named = |code: u32| match code {
        0 => "0 (registered)".to_string(),
        ERROR_HOTKEY_ALREADY_REGISTERED => "1409 (ERROR_HOTKEY_ALREADY_REGISTERED)".to_string(),
        other => format!("{other}"),
    };
    let who = if parent_holds { "the first process holds it" } else { "the first process does NOT hold it" };
    log::line(format!("RESULT hotkey conflict-child {combo}: second process got {} ({who})", named(child_code)));
    if !parent_holds && child_code == ERROR_HOTKEY_ALREADY_REGISTERED {
        log::line(format!(
            "RESULT hotkey already-taken {combo}: a program other than hwtest already holds it (deduction: both \
             hwtest processes got 1409 and neither holds a registration; which program is unknown, RegisterHotKey \
             does not say)"
        ));
    }
    match held_code {
        Some(code) => log::line(format!(
            "RESULT hotkey conflict-held {held_combo}: second process got {} (the first process holds it)",
            named(code)
        )),
        None => log::line(format!("RESULT hotkey conflict-held {held_combo}: not tested, see the setup line above")),
    }
    log::line(format!("RESULT hotkey conflict-free {free_combo}: second process got {} (nobody holds it)", named(free_code)));
    if held_code == Some(ERROR_HOTKEY_ALREADY_REGISTERED) && free_code == 0 {
        log::line(
            "RESULT hotkey conflict-detection: yes, a second process gets 1409 for a combination another process \
             holds and 0 for a free one (deduction: a program can tell that another RegisterHotKey owner has the \
             combination)",
        );
    } else {
        log::line(format!(
            "RESULT hotkey conflict-detection: no clear signal (held gave {held_code:?}, free gave {free_code})"
        ));
    }
}

/// Registers and immediately unregisters each Windows-owned combination.
fn windows_owned(hwnd: HWND) -> Result<(), String> {
    let mut succeeded = Vec::new();
    let mut failed = Vec::new();
    for (i, (label, text)) in WINDOWS_OWNED.iter().enumerate() {
        let combo = Combo::parse(text)?;
        let id = 10 + i32::try_from(i).ctx("index")?;
        match combo::register(Some(hwnd), id, combo) {
            Ok(reg) => {
                let outcome = reg.unregister();
                log::line(format!("RESULT hotkey windows-owned {label}: RegisterHotKey succeeded (unregistered at once)"));
                outcome?;
                succeeded.push(*label);
            }
            Err(e) => {
                log::line(format!("RESULT hotkey windows-owned {label}: RegisterHotKey failed, {e}"));
                failed.push(*label);
            }
        }
    }
    log::line(format!(
        "RESULT hotkey windows-owned summary: succeeded for {} of {} [{}]; failed for {} [{}]",
        succeeded.len(),
        WINDOWS_OWNED.len(),
        succeeded.join(", "),
        failed.len(),
        failed.join(", ")
    ));
    if succeeded.is_empty() {
        log::line(
            "RESULT hotkey taken-signal: RegisterHotKey failed for every Windows-owned combination tried, so \
             here a failure does signal 'taken' (deduction: limited to these nine)",
        );
    } else {
        log::line(format!(
            "RESULT hotkey taken-signal: not reliable, RegisterHotKey succeeded for [{}] although Windows is \
             known to handle those keys (deduction: whether they still act as Windows shortcuts was not checked \
             here; a failed RegisterHotKey is a sufficient but not a necessary sign of 'taken')",
            succeeded.join(", ")
        ));
    }
    Ok(())
}

pub fn run_child(args: &Args) -> Result<(), String> {
    let text = args.value("combo").ok_or("hotkey-child needs --combo, e.g. --combo win+ctrl+m")?;
    let combo = Combo::parse(text)?;
    let window = window::create(w!("hwtest_hotkey_child"), w!("hwtest hotkey-child"), Some(hotkey_proc), Kind::Hidden)?;
    let (code, unregistered): (u32, Result<(), String>) = match combo::register(Some(window.hwnd()), 1, combo) {
        Ok(reg) => (0, reg.unregister()),
        Err(e) => {
            log::line(format!("child: RegisterHotKey {combo} failed, {e}"));
            (e.win32, Ok(()))
        }
    };
    // Printed before any unregister error is returned, so the parent still gets the registration outcome.
    log::line(format!("CHILD_RESULT {code}"));
    unregistered
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

    /// Exercises the window, the message pump, the window procedure and the foreground watcher without pressing
    /// any key: a WM_HOTKEY is posted to our own hidden window.
    #[test]
    fn hidden_window_receives_a_posted_hotkey_message() -> Result<(), String> {
        let window = window::create(w!("hwtest_hotkey_test"), w!("hwtest hotkey test"), Some(hotkey_proc), Kind::Hidden)?;
        let watcher = Watcher::start(&["test"])?;
        let before = PRESSES.get();
        // SAFETY: posts to a window of this thread.
        unsafe { PostMessageW(Some(window.hwnd()), WM_HOTKEY, WPARAM(7), LPARAM(0x4D_0000 | 0x0A)) }.ctx("PostMessageW")?;
        for _ in 0..5 {
            window::pump(20)?;
        }
        assert_eq!(PRESSES.get(), before + 1);
        // The watcher stayed installed through the pumping and can report counts.
        assert!(watcher.changes(0) < 1000);
        Ok(())
    }

    /// Holds a free combination for a moment (no key press involved) to exercise the registered-path code.
    #[test]
    fn duplicate_registration_in_one_window_is_reported() -> Result<(), String> {
        let window = window::create(w!("hwtest_hotkey_dup"), w!("hwtest hotkey dup"), Some(hotkey_proc), Kind::Hidden)?;
        let combo = Combo::parse("win+ctrl+alt+7")?;
        let first = combo::register(Some(window.hwnd()), 1, combo).map_err(|e| e.to_string())?;
        same_process_duplicate(window.hwnd(), combo)?;
        first.unregister()
    }
}
