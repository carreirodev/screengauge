#![windows_subsystem = "windows"]
//! hwtest: throwaway hardware test for Pixmeter (WPC-281, then WPC-289). Never merged into main.
//! Each mode checks one group of items and logs what it measured; see README.md.

mod capture;
mod common;
mod frozen;
mod hdr;
mod image;
mod keys;
mod log;
mod measure;
mod monitors;

use std::process::ExitCode;

use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
use windows::Win32::UI::HiDpi::{
    AreDpiAwarenessContextsEqual, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetThreadDpiAwarenessContext,
};

const MODES: &str = "info, capture, measure, hotkey, recorder, run, hdr";

fn main() -> ExitCode {
    let args = common::Args::from_env();
    log::init(&args.mode);
    log::line(format!("hwtest {} mode={} args={:?}", env!("CARGO_PKG_VERSION"), args.mode, args.rest));

    // SAFETY: plain queries on the calling thread's DPI context.
    let pmv2 = unsafe {
        AreDpiAwarenessContextsEqual(GetThreadDpiAwarenessContext(), DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2)
    }
    .as_bool();
    log::line(format!("dpi awareness from the manifest: per-monitor v2 = {pmv2}"));
    if !pmv2 {
        log::line("ERROR: the embedded manifest did not make the process Per-Monitor V2; every number would be wrong");
        return ExitCode::FAILURE;
    }

    // SAFETY: first COM call on this thread; every mode runs on it.
    let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
    if hr.is_err() {
        log::line(format!("ERROR: CoInitializeEx: {hr:?}"));
        return ExitCode::FAILURE;
    }

    let result = match args.mode.as_str() {
        "info" => monitors::info_mode(&args),
        "capture" => capture::capture_mode(&args),
        "measure" => measure::measure_mode(&args),
        "hotkey" => keys::hotkey_mode(&args),
        "hotkey-child" => keys::hotkey_child_mode(&args),
        "recorder" => keys::recorder_mode(&args),
        "run" => frozen::run_mode(&args),
        "hdr" => hdr::hdr_mode(&args),
        other => Err(format!("unknown mode '{other}'; modes: {MODES}")),
    };
    match result {
        Ok(()) => {
            log::line("done");
            ExitCode::SUCCESS
        }
        Err(e) => {
            log::line(format!("ERROR: {e}"));
            ExitCode::FAILURE
        }
    }
}
