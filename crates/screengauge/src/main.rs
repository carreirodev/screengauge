//! ScreenGauge.exe and its Windows layer: the resident process with the tray icon, which will carry out
//! what the core (`screengauge-core`) decides.

// No console window. Test builds keep the console, so their output shows.
#![cfg_attr(not(test), windows_subsystem = "windows")]

#[cfg(not(all(target_os = "windows", target_arch = "x86_64")))]
compile_error!("ScreenGauge v1 is built for 64-bit Windows (x64) only.");

mod capture;
mod event;
mod foreground;
mod frozen_screen;
mod global_shortcut;
mod log;
mod notification;
mod resident;
mod settings_file;
mod single_instance;
mod tray;
mod win32;

use std::process::ExitCode;

use single_instance::Instance;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            // A failed start reaches only the debugger output: without the tray icon, no Windows notification can show.
            log::failure("stopped", &e);
            ExitCode::FAILURE
        }
    }
}

fn run() -> windows::core::Result<()> {
    let _instance = match single_instance::claim(single_instance::NAME)? {
        Instance::First(guard) => guard,
        // Opening ScreenGauge while a copy runs calls that copy, and this one exits.
        Instance::AlreadyRunning => return resident::call_running_copy(),
    };
    // Only the copy that runs touches the settings file. The global shortcut, the options and the remembered state the
    // tools open with come from it, and the remembered state goes back to it; Settings will save to it too.
    let (file, stored) = settings_file::open_at_startup();
    resident::run(file, stored)
}
