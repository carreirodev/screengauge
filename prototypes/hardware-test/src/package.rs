//! The `package` mode, and the defaults a packaged (MSIX) copy applies at start; see packaging/README.md.
//!
//! `package info`: package identity, the paths the process sees, and the ApplicationData folders.
//! `package startup [--enable | --disable]`: the `HwtestStartup` startup task declared in the manifest.
//! `package settings [--name FILE]`: appends to `%LOCALAPPDATA%\ScreenGauge-hwtest\FILE` (default
//! `settings.txt`) and logs where the write really landed, to see whether Windows redirected it.

use std::fs::{File, OpenOptions};
use std::io::{ErrorKind, Write};
use std::os::windows::io::AsRawHandle;
use std::path::{Path, PathBuf};
use std::time::Duration;

use windows::ApplicationModel::{StartupTask, StartupTaskState};
use windows::Storage::ApplicationData;
use windows::Win32::Foundation::{
    APPMODEL_ERROR_NO_PACKAGE, ERROR_INSUFFICIENT_BUFFER, ERROR_SUCCESS, HANDLE, WIN32_ERROR,
};
use windows::Win32::Storage::FileSystem::{
    FILE_NAME_NORMALIZED, GETFINALPATHNAMEBYHANDLE_FLAGS, GetFinalPathNameByHandleW, VOLUME_NAME_DOS, VOLUME_NAME_NT,
};
use windows::Win32::Storage::Packaging::Appx::{GetCurrentPackageFamilyName, GetCurrentPackageFullName};
use windows::core::{HSTRING, PWSTR};

use crate::capture::pump_messages;
use crate::common::{Args, Ctx};
use crate::log;
use crate::monitors::wide;

/// TaskId of the startup task in packaging/AppxManifest.xml.
const STARTUP_TASK_ID: &str = "HwtestStartup";
/// How long to wait for a WinRT async operation before cancelling it.
const ASYNC_LIMIT_MS: f64 = 10_000.0;
/// The Start menu and the startup task launch the packaged copy with no arguments; it then runs `run` with these.
const PACKAGED_RUN_ARGS: [&str; 4] = ["--hotkey", "win+ctrl+shift+m", "--exit-after-secs", "3600"];
/// Folder under `%LOCALAPPDATA%` for `package settings`, where the real app would keep its settings.
const SETTINGS_DIR: &str = "ScreenGauge-hwtest";

/// Polls a WinRT async operation until it completes, cancelling it after `ASYNC_LIMIT_MS`, the way
/// `capture::wgc::request_borderless` does. Evaluates to `Result<(result, elapsed ms), String>`.
/// A macro, not a function: `IAsyncOperation` lives in windows-future, which the windows crate does not
/// re-export, so the type cannot be named here.
macro_rules! wait_op {
    ($op:expr, $what:expr) => {{
        let op = $op;
        let what: &str = $what;
        let t0 = log::now_ms();
        loop {
            let status = match op.Status() {
                Ok(s) => s.0,
                Err(e) => break Err(format!("{what}: Status() failed: {e}")),
            };
            // AsyncStatus: 0 Started, 1 Completed, 2 Canceled, 3 Error, compared raw as in capture::wgc.
            match status {
                0 if log::now_ms() - t0 > ASYNC_LIMIT_MS => {
                    let cancel = match op.Cancel() {
                        Ok(()) => "cancelled".to_string(),
                        Err(e) => format!("Cancel failed: {e}"),
                    };
                    break Err(format!("{what}: still pending after {ASYNC_LIMIT_MS} ms; {cancel}"));
                }
                0 => {
                    pump_messages();
                    std::thread::sleep(Duration::from_millis(5));
                }
                1 => break op.GetResults().map(|r| (r, log::now_ms() - t0)).ctx(&format!("{what}: GetResults")),
                other => {
                    let code = match op.ErrorCode() {
                        // LowerHex prints a negative i32 as its two's complement bits, the usual HRESULT form.
                        Ok(c) => format!("0x{:08x} {}", c.0, c.message()),
                        Err(e) => format!("ErrorCode() failed: {e}"),
                    };
                    break Err(format!("{what}: ended with AsyncStatus {other} (2 Canceled, 3 Error), error {code}"));
                }
            }
        }
    }};
}

/// For a packaged copy, before `log::init`: points the log at a folder Windows does not redirect, and turns
/// "no mode given" into the resident `run` mode. Returns the lines to log once the log is open.
pub fn apply_packaged_defaults(args: &mut Args) -> Vec<String> {
    let full_name = match package_string(GetCurrentPackageFullName, "GetCurrentPackageFullName") {
        Ok(Some(n)) => n,
        Ok(None) => return vec!["not packaged: no packaged defaults".to_string()],
        Err(e) => return vec![format!("ERROR: package identity check failed, so no packaged defaults: {e}")],
    };
    let mut notes = vec![format!("packaged as {full_name}")];
    match (std::env::var_os("HWTEST_OUT"), std::env::var_os("USERPROFILE")) {
        (Some(out), _) => {
            let out = Path::new(&out).display();
            notes.push(format!("RESULT package-default-out: HWTEST_OUT already set to {out}, kept"));
        }
        (None, Some(home)) => {
            let out = PathBuf::from(home).join("hwtest-out");
            // SAFETY: edition 2024 makes set_var unsafe because another thread could read the environment at the
            // same time. This runs first thing in main, before log::init, COM or any mode: no other thread of
            // this program exists yet.
            unsafe { std::env::set_var("HWTEST_OUT", &out) };
            notes.push(format!(
                "RESULT package-default-out: HWTEST_OUT was unset, set to {} (outside AppData, so not redirected)",
                out.display()
            ));
        }
        (None, None) => notes.push(
            "RESULT package-default-out: HWTEST_OUT unset and USERPROFILE missing; the log stays in the default folder"
                .to_string(),
        ),
    }
    if args.mode == "help" {
        *args = Args { mode: "run".to_string(), rest: PACKAGED_RUN_ARGS.iter().map(|a| a.to_string()).collect() };
        notes.push(format!(
            "RESULT package-default-mode: no mode given, running 'run {}' (Start menu and startup task launch)",
            PACKAGED_RUN_ARGS.join(" ")
        ));
    } else {
        notes.push(format!("RESULT package-default-mode: mode '{}' given on the command line, kept", args.mode));
    }
    notes
}

pub fn package_mode(args: &Args) -> Result<(), String> {
    match args.rest.first().map(String::as_str) {
        Some("info") => info(),
        Some("startup") => startup(args),
        Some("settings") => settings(args),
        other => Err(format!(
            "package {}: use 'package info', 'package startup [--enable | --disable]' or 'package settings [--name FILE]'",
            other.unwrap_or("(nothing)")
        )),
    }
}

fn info() -> Result<(), String> {
    let full = package_string(GetCurrentPackageFullName, "GetCurrentPackageFullName")?;
    let family = package_string(GetCurrentPackageFamilyName, "GetCurrentPackageFamilyName")?;
    let none = "(none: APPMODEL_ERROR_NO_PACKAGE)";
    log::line(format!("RESULT package-identity: packaged={}", full.is_some()));
    log::line(format!("RESULT package-full-name: {}", full.as_deref().unwrap_or(none)));
    log::line(format!("RESULT package-family-name: {}", family.as_deref().unwrap_or(none)));
    match std::env::current_exe() {
        Ok(p) => log::line(format!("RESULT package-exe: {}", p.display())),
        Err(e) => log::line(format!("RESULT package-exe: ERROR {e}")),
    }
    log::line(format!("RESULT package-log-dir: {}", log::out_dir().display()));
    for var in ["LOCALAPPDATA", "TEMP"] {
        match std::env::var_os(var) {
            Some(v) => log::line(format!("RESULT package-env: {var}={}", Path::new(&v).display())),
            None => log::line(format!("RESULT package-env: {var} is not set")),
        }
    }
    if full.is_none() {
        log::line("RESULT package-appdata: skipped, ApplicationData needs package identity");
        return Ok(());
    }
    match ApplicationData::Current() {
        Ok(data) => {
            log::line(format!("RESULT package-appdata: LocalFolder {}", folder_path(data.LocalFolder())));
            log::line(format!("RESULT package-appdata: LocalCacheFolder {}", folder_path(data.LocalCacheFolder())));
        }
        Err(e) => log::line(format!("RESULT package-appdata: ApplicationData::Current ERROR {e}")),
    }
    Ok(())
}

fn startup(args: &Args) -> Result<(), String> {
    let (enable, disable) = (args.flag("enable"), args.flag("disable"));
    if enable && disable {
        return Err("package startup: give --enable or --disable, not both".to_string());
    }
    let packaged = package_string(GetCurrentPackageFullName, "GetCurrentPackageFullName")?.is_some();
    log::line(format!("startup: packaged={packaged} (StartupTask needs package identity)"));
    let id = HSTRING::from(STARTUP_TASK_ID);
    let got =
        StartupTask::GetAsync(&id).ctx("StartupTask::GetAsync").and_then(|op| wait_op!(op, "StartupTask::GetAsync"));
    let (task, ms) = match got {
        Ok(v) => v,
        Err(e) => {
            log::line(format!("RESULT startup-task: GetAsync(\"{STARTUP_TASK_ID}\") failed: {e}"));
            return Err(e);
        }
    };
    let task_id = match task.TaskId() {
        Ok(t) => t.to_string(),
        Err(e) => format!("(TaskId ERROR {e})"),
    };
    log::line(format!("RESULT startup-task: GetAsync(\"{STARTUP_TASK_ID}\") -> TaskId {task_id} after {ms:.1} ms"));
    log::line(format!("RESULT startup-state-before: {}", state_text(task.State())));
    if enable {
        let (state, ms) =
            task.RequestEnableAsync().ctx("RequestEnableAsync").and_then(|op| wait_op!(op, "RequestEnableAsync"))?;
        log::line(format!(
            "RESULT startup-enable: RequestEnableAsync -> {} after {ms:.1} ms (Learn: no consent dialog for a packaged desktop app)",
            state_name(state)
        ));
        log::line(format!("RESULT startup-state-after: {}", state_text(task.State())));
    } else if disable {
        task.Disable().ctx("StartupTask::Disable")?;
        log::line(format!("RESULT startup-disable: Disable() OK; State now {}", state_text(task.State())));
    }
    Ok(())
}

fn settings(args: &Args) -> Result<(), String> {
    let name = args.value("name").unwrap_or("settings.txt");
    if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\\', ':']) {
        return Err(format!("--name {name}: give a plain file name, no folder"));
    }
    let packaged = package_string(GetCurrentPackageFullName, "GetCurrentPackageFullName")?.is_some();
    let base = std::env::var_os("LOCALAPPDATA").ok_or("LOCALAPPDATA is not set")?;
    let dir = PathBuf::from(base).join(SETTINGS_DIR);
    let path = dir.join(name);
    log::line(format!("RESULT settings-path-used: {} (packaged={packaged})", path.display()));
    let dir_existed = dir.try_exists().ctx(&format!("checking {}", dir.display()))?;
    log::line(format!("settings: folder {} existed before: {dir_existed}", dir.display()));
    match std::fs::read_to_string(&path) {
        Ok(text) => {
            log::line(format!("RESULT settings-before: exists, {} bytes, {} lines", text.len(), text.lines().count()));
            for l in text.lines() {
                log::line(format!("settings-before | {l}"));
            }
        }
        Err(e) if e.kind() == ErrorKind::NotFound => log::line("RESULT settings-before: absent"),
        Err(e) => return Err(format!("reading {}: {e}", path.display())),
    }

    std::fs::create_dir_all(&dir).ctx(&format!("creating {}", dir.display()))?;
    let entry = format!("{} packaged={packaged} pid={}", log::stamp(), std::process::id());
    let mut file =
        OpenOptions::new().create(true).append(true).open(&path).ctx(&format!("opening {}", path.display()))?;
    writeln!(file, "{entry}").ctx(&format!("writing {}", path.display()))?;
    log::line(format!("settings: appended '{entry}'"));
    let real = final_path(&file, VOLUME_NAME_DOS);
    match &real {
        Ok(p) => log::line(format!("RESULT settings-real-path: {p} (GetFinalPathNameByHandleW on the open handle)")),
        Err(e) => log::line(format!("RESULT settings-real-path: ERROR {e}")),
    }
    match final_path(&file, VOLUME_NAME_NT) {
        Ok(p) => log::line(format!("settings: NT path behind the handle: {p}")),
        Err(e) => log::line(format!("settings: NT path behind the handle: ERROR {e}")),
    }
    drop(file);

    let back = std::fs::read_to_string(&path).ctx(&format!("reading back {}", path.display()))?;
    log::line(format!(
        "RESULT settings-after: {} lines; last '{}'",
        back.lines().count(),
        back.lines().last().unwrap_or("")
    ));
    if let Ok(p) = &real {
        let shown = p.strip_prefix(r"\\?\").unwrap_or(p);
        let verdict = if shown.to_lowercase() == path.display().to_string().to_lowercase() {
            "same as the path used: not redirected"
        } else {
            "differs from the path used: redirected"
        };
        log::line(format!("RESULT settings-redirect: {verdict}"));
    }
    if packaged {
        // Not on Learn: the private location below is where redirected Local AppData writes are commonly seen.
        // Checked as extra evidence in case the handle path hides the redirection.
        match ApplicationData::Current().and_then(|d| d.LocalCacheFolder()).and_then(|f| f.Path()) {
            Ok(cache) => {
                let guess = PathBuf::from(cache.to_string()).join("Local").join(SETTINGS_DIR).join(name);
                match guess.try_exists() {
                    Ok(v) => log::line(format!("RESULT settings-private-copy: {} exists={v}", guess.display())),
                    Err(e) => log::line(format!("RESULT settings-private-copy: {} ERROR {e}", guess.display())),
                }
            }
            Err(e) => log::line(format!("RESULT settings-private-copy: LocalCacheFolder ERROR {e}")),
        }
    }
    Ok(())
}

/// GetCurrentPackageFullName or GetCurrentPackageFamilyName; `None` when the process has no package identity.
fn package_string(
    get: unsafe fn(*mut u32, Option<PWSTR>) -> WIN32_ERROR,
    what: &str,
) -> Result<Option<String>, String> {
    let mut len = 0u32;
    // SAFETY: a size query: with no buffer the call only writes `len`.
    let rc = unsafe { get(&mut len, None) };
    if rc == APPMODEL_ERROR_NO_PACKAGE {
        return Ok(None);
    }
    if rc != ERROR_INSUFFICIENT_BUFFER {
        return Err(format!("{what} size query: {} ({})", rc.0, windows::core::Error::from(rc)));
    }
    let mut buf = vec![0u16; len as usize];
    // SAFETY: `buf` holds `len` UTF-16 units, as the size query asked for, and outlives the call.
    let rc = unsafe { get(&mut len, Some(PWSTR(buf.as_mut_ptr()))) };
    if rc != ERROR_SUCCESS {
        return Err(format!("{what}: {} ({})", rc.0, windows::core::Error::from(rc)));
    }
    Ok(Some(wide(&buf)))
}

/// The path behind an open file handle, normalized; `volume` picks the DOS (`C:\`) or NT (`\Device\...`) form.
fn final_path(file: &File, volume: GETFINALPATHNAMEBYHANDLE_FLAGS) -> Result<String, String> {
    let flags = GETFINALPATHNAMEBYHANDLE_FLAGS(FILE_NAME_NORMALIZED.0 | volume.0);
    let handle = HANDLE(file.as_raw_handle());
    let mut buf = vec![0u16; 512];
    // Two tries: when the buffer is too small, the first returns the size needed, terminator included.
    for _ in 0..2 {
        // SAFETY: `handle` is owned by `file`, open for the whole call; `buf` is a live, writable slice.
        let n = unsafe { GetFinalPathNameByHandleW(handle, &mut buf, flags) } as usize;
        if n == 0 {
            return Err(format!("GetFinalPathNameByHandleW: {}", windows::core::Error::from_thread()));
        }
        if n < buf.len() {
            return Ok(String::from_utf16_lossy(&buf[..n]));
        }
        buf.resize(n, 0);
    }
    Err("GetFinalPathNameByHandleW: the path kept growing".to_string())
}

fn folder_path(folder: windows::core::Result<windows::Storage::StorageFolder>) -> String {
    match folder.and_then(|f| f.Path()) {
        Ok(p) => p.to_string(),
        Err(e) => format!("ERROR {e}"),
    }
}

fn state_text(state: windows::core::Result<StartupTaskState>) -> String {
    match state {
        Ok(s) => state_name(s),
        Err(e) => format!("ERROR {e}"),
    }
}

fn state_name(s: StartupTaskState) -> String {
    let name = match s {
        StartupTaskState::Disabled => "Disabled",
        StartupTaskState::DisabledByUser => "DisabledByUser",
        StartupTaskState::Enabled => "Enabled",
        StartupTaskState::DisabledByPolicy => "DisabledByPolicy",
        StartupTaskState::EnabledByPolicy => "EnabledByPolicy",
        _ => "unknown",
    };
    format!("{} {name}", s.0)
}
