//! ScreenGauge.exe and its Windows layer: the resident process with the tray icon, which will carry out
//! what the core (`screengauge-core`) decides.

// No console window. Test builds keep the console, so their output shows.
#![cfg_attr(not(test), windows_subsystem = "windows")]

#[cfg(not(all(target_os = "windows", target_arch = "x86_64")))]
compile_error!("ScreenGauge v1 is built for 64-bit Windows (x64) only.");

mod log;
mod resident;
mod single_instance;
mod tray;
mod win32;

use std::process::ExitCode;

use single_instance::Instance;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            // Until Windows notifications exist (WPC-303), a failed start reaches only the debugger output.
            log::line(&format!("stopped: {e}"));
            ExitCode::FAILURE
        }
    }
}

fn run() -> windows::core::Result<()> {
    let _instance = match single_instance::claim(single_instance::NAME)? {
        Instance::First(guard) => guard,
        // Another copy is running, so this one exits. Calling that copy instead comes with WPC-301.
        Instance::AlreadyRunning => return Ok(()),
    };
    resident::run()
}
