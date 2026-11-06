//! ScreenGauge.exe and its Windows layer: the resident process with the tray icon, which will carry out
//! what the core (`screengauge-core`) decides.

// No console window. Test builds keep the console, so their output shows.
#![cfg_attr(not(test), windows_subsystem = "windows")]

#[cfg(not(all(target_os = "windows", target_arch = "x86_64")))]
compile_error!("ScreenGauge v1 is built for 64-bit Windows (x64) only.");

mod app_message;
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
#[cfg(test)]
mod installer;
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
#[cfg(test)]
mod test_process;
mod theme;
mod tray;
mod tray_menu;
mod update_check;
mod win32;

use std::process::ExitCode;

use single_instance::Instance;
use windows::ApplicationModel::Activation::ActivationKind;

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
    let started_by = who_started();
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

/// Who started this process: its first argument says, and in the Store copy, Windows too (`started_in_package`).
fn who_started() -> StartedBy {
    let by_arguments = started_by(std::env::args());
    match package::activation_kind() {
        Ok(None) => by_arguments,
        Ok(Some(kind)) => {
            let started = started_in_package(by_arguments, kind);
            // In DebugView, the one trace of how a start at sign-in went (WPC-336).
            log::note(&format!(
                "started {started:?}: its arguments say {by_arguments:?}, Windows says {kind:?}"
            ));
            started
        }
        // The arguments still say, as they do for the installer copy.
        Err(e) => {
            log::failure("asking Windows how it started the Store copy", &e);
            by_arguments
        }
    }
}

/// Who started the Store copy, from its arguments (`started_by`) and how Windows says it started it (`kind`). The
/// startup task's start is at sign-in, whether or not Windows passed the manifest's `--sign-in` along with it.
fn started_in_package(by_arguments: StartedBy, kind: ActivationKind) -> StartedBy {
    match (by_arguments, kind) {
        (StartedBy::User, ActivationKind::StartupTask) => StartedBy::SignIn,
        _ => by_arguments,
    }
}

/// Who started this process, from its first argument.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartedBy {
    /// The user opened ScreenGauge, from the Start menu or the `.exe`: opening it is a call.
    User,
    /// COM, to take a notification's click with ScreenGauge closed: COM adds -Embedding to the command line registered
    /// for the activator, which is the `.exe`'s path alone (WPC-347, scenario s7).
    Click,
    /// Windows, at sign-in, from the `Run` value "Start with Windows" writes (WPC-326), or the Store copy's startup task
    /// (WPC-336): ScreenGauge starts hidden.
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
    fn the_store_copy_started_by_its_startup_task_starts_as_at_sign_in_even_without_the_flag() {
        let opened = started_by(args(&["ScreenGauge.exe"]));
        assert_eq!(
            started_in_package(opened, ActivationKind::StartupTask),
            StartedBy::SignIn
        );
        let flagged = started_by(args(&["ScreenGauge.exe", "--sign-in"]));
        assert_eq!(
            started_in_package(flagged, ActivationKind::StartupTask),
            StartedBy::SignIn
        );
    }

    #[test]
    fn the_store_copy_opened_from_the_start_menu_or_for_a_click_starts_as_its_arguments_say() {
        let opened = started_by(args(&["ScreenGauge.exe"]));
        assert_eq!(
            started_in_package(opened, ActivationKind::Launch),
            StartedBy::User
        );
        let clicked = started_by(args(&["ScreenGauge.exe", "-Embedding", "-Embedding"]));
        assert_eq!(
            started_in_package(clicked, ActivationKind::Launch),
            StartedBy::Click
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
