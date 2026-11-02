//! ScreenGauge.exe and its Windows layer: the resident process with the tray icon, which will carry out
//! what the core (`screengauge-core`) decides.

// No console window. Test builds keep the console, so their output shows.
#![cfg_attr(not(test), windows_subsystem = "windows")]

#[cfg(not(all(target_os = "windows", target_arch = "x86_64")))]
compile_error!("ScreenGauge v1 is built for 64-bit Windows (x64) only.");

mod capture;
mod clipboard;
mod com;
mod event;
mod folder_picker;
mod foreground;
mod frozen_screen;
mod global_shortcut;
mod graphics;
mod keyboard_hook;
mod log;
mod notification;
mod png;
mod relief;
mod resident;
mod save_as;
mod settings_file;
mod settings_window;
mod single_instance;
mod snip_file;
#[cfg(test)]
mod test_folder;
mod theme;
mod tray;
mod win32;

use std::process::ExitCode;

use single_instance::Instance;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            // A failed start reaches only the debugger output: no Windows notification is ready that early.
            log::failure("stopped", &e);
            ExitCode::FAILURE
        }
    }
}

fn run() -> windows::core::Result<()> {
    let for_a_click = started_for_a_click(std::env::args());
    let _instance = match single_instance::claim(single_instance::NAME)? {
        Instance::First(guard) => guard,
        // Windows started this process for a click on a notification, but a copy runs that isn't the class's, as the
        // other of the Store and installer copies would be (WPC-339): this one exits without calling it, which would
        // open its tools.
        Instance::AlreadyRunning if for_a_click => return Ok(()),
        // Opening ScreenGauge while a copy runs calls that copy, and this one exits.
        Instance::AlreadyRunning => return resident::call_running_copy(),
    };
    // Only the copy that runs touches the settings file. The global shortcut, the options and the remembered state the
    // tools open with come from it, and the remembered state goes back to it; Settings will save to it too.
    let (file, stored) = settings_file::open_at_startup();
    resident::run(file, stored, for_a_click)
}

/// Whether COM started this process to take a notification's click, with ScreenGauge closed: COM adds -Embedding to the
/// command line registered for the activator, which is the `.exe`'s path alone (WPC-347, scenario s7).
fn started_for_a_click(mut args: impl Iterator<Item = String>) -> bool {
    args.nth(1).is_some_and(|arg| {
        arg.eq_ignore_ascii_case("-Embedding") || arg.eq_ignore_ascii_case("/Embedding")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(line: &[&str]) -> impl Iterator<Item = String> {
        line.iter()
            .map(|arg| (*arg).to_string())
            .collect::<Vec<_>>()
            .into_iter()
    }

    #[test]
    fn com_starts_it_for_a_click() {
        assert!(started_for_a_click(args(&[
            "ScreenGauge.exe",
            "-Embedding"
        ])));
        assert!(started_for_a_click(args(&[
            "ScreenGauge.exe",
            "/embedding"
        ])));
    }

    #[test]
    fn the_user_opens_it_otherwise() {
        assert!(!started_for_a_click(args(&["ScreenGauge.exe"])));
        assert!(!started_for_a_click(args(&[
            "ScreenGauge.exe",
            "Embedding"
        ])));
    }
}
