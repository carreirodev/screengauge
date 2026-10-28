//! Results log: every line goes to stdout and to `<out>/<mode>-<date>-<time>.log`.
//! `<out>` is `%HWTEST_OUT%` when set, otherwise `%TEMP%\hwtest`.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};
use windows::Win32::System::SystemInformation::GetLocalTime;

static FILE: OnceLock<Mutex<File>> = OnceLock::new();
static CLOCK: OnceLock<(i64, f64)> = OnceLock::new();

pub fn out_dir() -> PathBuf {
    match std::env::var_os("HWTEST_OUT") {
        Some(dir) => PathBuf::from(dir),
        None => std::env::temp_dir().join("hwtest"),
    }
}

/// Opens the log file for this run. Call once, first thing in `main`.
pub fn init(mode: &str) {
    now_ms();
    let dir = out_dir();
    if let Err(e) = std::fs::create_dir_all(&dir) {
        println!("log: cannot create {}: {e}; logging to stdout only", dir.display());
        return;
    }
    let path = dir.join(format!("{mode}-{}.log", stamp()));
    match OpenOptions::new().create(true).append(true).open(&path) {
        Ok(f) => {
            if FILE.set(Mutex::new(f)).is_err() {
                println!("log: init called twice; keeping the first file");
            }
            println!("log: {}", path.display());
        }
        Err(e) => println!("log: cannot open {}: {e}; logging to stdout only", path.display()),
    }
}

/// Milliseconds since the process started, from the high-resolution counter.
pub fn now_ms() -> f64 {
    let (start, ticks_per_ms) = *CLOCK.get_or_init(|| (qpc(), qpf() / 1000.0));
    (qpc() - start) as f64 / ticks_per_ms
}

pub fn line(text: impl AsRef<str>) {
    // SAFETY: GetLocalTime only fills the returned struct.
    let t = unsafe { GetLocalTime() };
    let out = format!(
        "{:02}:{:02}:{:02}.{:03} [{:>10.3} ms] {}",
        t.wHour,
        t.wMinute,
        t.wSecond,
        t.wMilliseconds,
        now_ms(),
        text.as_ref()
    );
    println!("{out}");
    if let Some(file) = FILE.get() {
        // A poisoned lock means another thread panicked mid-write; keep logging anyway.
        let mut f = file.lock().unwrap_or_else(|p| p.into_inner());
        if let Err(e) = writeln!(f, "{out}") {
            println!("log: write failed: {e}");
        }
    }
}

/// `YYYYMMDD-HHMMSS`, local time, for file names.
pub fn stamp() -> String {
    // SAFETY: GetLocalTime only fills the returned struct.
    let t = unsafe { GetLocalTime() };
    format!("{:04}{:02}{:02}-{:02}{:02}{:02}", t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond)
}

fn qpc() -> i64 {
    let mut v = 0i64;
    // SAFETY: writes one i64; cannot fail on Windows XP and later.
    let _ = unsafe { QueryPerformanceCounter(&mut v) };
    v
}

fn qpf() -> f64 {
    let mut v = 0i64;
    // SAFETY: writes one i64; cannot fail on Windows XP and later.
    let _ = unsafe { QueryPerformanceFrequency(&mut v) };
    v as f64
}
