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
mod hidden_icons_panel;
mod keyboard_hook;
mod log;
mod notification;
mod package;
mod png;
mod relief;
mod resident;
mod save_as;
mod settings_file;
mod settings_window;
mod sign;
mod single_instance;
mod snip_file;
mod start_with_windows;
#[cfg(test)]
mod test_bitmap;
#[cfg(test)]
mod test_folder;
mod theme;
mod tray;
mod tray_menu;
mod update_check;
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
    let started_by = started_by(std::env::args());
    let _instance = match single_instance::claim(single_instance::NAME)? {
        Instance::First(guard) => guard,
        // Windows started this process, and a copy runs already. For a click on a notification, that copy isn't the
        // class's, as the other of the Store and installer copies would be (WPC-339). At sign-in, it is the other copy,
        // both set to start with Windows, or this one opened from the Start menu before Windows got to it (v1 spec
        // WPC-296, one copy only). Either way this one exits in silence: calling the running copy would open its tools.
        Instance::AlreadyRunning if started_by != StartedBy::User => return Ok(()),
        // Opening ScreenGauge while a copy runs calls that copy, and this one exits.
        Instance::AlreadyRunning => return resident::call_running_copy(),
    };
    // Only the copy that runs touches the settings file. The global shortcut, the options and the remembered state the
    // tools open with come from it, and the remembered state goes back to it; Settings will save to it too.
    let (file, stored) = settings_file::open_at_startup();
    resident::run(file, stored, started_by)
}

/// Who started this process, from its first argument.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartedBy {
    /// The user opened ScreenGauge, from the Start menu or the `.exe`: opening it is a call.
    User,
    /// COM, to take a notification's click with ScreenGauge closed: COM adds -Embedding to the command line registered
    /// for the activator, which is the `.exe`'s path alone (WPC-347, scenario s7).
    Click,
    /// Windows, at sign-in, from the `Run` value "Start with Windows" writes (WPC-326): ScreenGauge starts hidden.
    SignIn,
}

fn started_by(mut args: impl Iterator<Item = String>) -> StartedBy {
    match args.nth(1) {
        Some(arg)
            if arg.eq_ignore_ascii_case("-Embedding") || arg.eq_ignore_ascii_case("/Embedding") =>
        {
            StartedBy::Click
        }
        Some(arg) if arg == start_with_windows::SIGN_IN => StartedBy::SignIn,
        _ => StartedBy::User,
    }
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
        for arg in ["-Embedding", "/embedding"] {
            assert_eq!(
                started_by(args(&["ScreenGauge.exe", arg])),
                StartedBy::Click
            );
        }
    }

    #[test]
    fn windows_starts_it_at_sign_in_with_the_run_value_s_flag() {
        assert_eq!(
            started_by(args(&["ScreenGauge.exe", "--sign-in"])),
            StartedBy::SignIn
        );
    }

    #[test]
    fn the_user_opens_it_otherwise() {
        for line in [
            &["ScreenGauge.exe"][..],
            &["ScreenGauge.exe", "Embedding"],
            &["ScreenGauge.exe", "--sign-in-later"],
        ] {
            assert_eq!(started_by(args(line)), StartedBy::User, "{line:?}");
        }
    }
}
