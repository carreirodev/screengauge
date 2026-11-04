//! "Start with Windows" for the installer copy (WPC-326, ADR 0004): ScreenGauge is listed to start at sign-in by a value
//! in the current user's `Run` key, which Windows reads at each sign-in, and it starts then with [`SIGN_IN`] on its
//! command line, hidden, without opening the tools. What the user chose in Task Manager or in Settings > Apps > Startup
//! is the value of the same name in `StartupApproved\Run`, which only Windows writes: ScreenGauge reads it and never
//! writes it, so that choice is never overridden. The rule of what the switch shows is the core's
//! ([`StartWithWindows`]).
//!
//! The Store copy starts with its package's startup task instead (WPC-336), which its manifest declares off: turning
//! the option on enables the task, and Windows keeps the user's choice in the task's own state, which only Windows
//! turns to "disabled by the user". The Store copy never writes the `Run` value, whose path would hold the package's
//! version and so break at its first update.

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;

use screengauge_core::settings::{Change, Click, StartWithWindows};
use windows::ApplicationModel::{StartupTask, StartupTaskState};
use windows::Win32::Foundation::{E_FAIL, ERROR_FILE_NOT_FOUND, ERROR_SUCCESS, WIN32_ERROR};
use windows::Win32::System::Registry::{
    HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_BINARY, RRF_RT_REG_SZ, RegDeleteKeyValueW, RegGetValueW,
    RegSetKeyValueW,
};
use windows::core::{Error, HSTRING, PCWSTR, w};

use crate::package::{self, AppCopy};
use crate::{log, settings_file};

/// What the `Run` value adds after the `.exe`'s path, so a start at sign-in is told from the user opening ScreenGauge.
pub const SIGN_IN: &str = "--sign-in";

/// The Store copy's startup task, as `packaging/msix/AppxManifest.xml` declares it.
pub const STARTUP_TASK_ID: &str = "ScreenGauge";

/// Where Windows looks: the `Run` key, where the value lists ScreenGauge, and the key that keeps the user's choice for
/// it, both under the current user, and the value's name in both.
struct Keys {
    run: PCWSTR,
    approved: PCWSTR,
    name: PCWSTR,
}

const WINDOWS: Keys = Keys {
    run: w!(r"Software\Microsoft\Windows\CurrentVersion\Run"),
    approved: w!(r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run"),
    name: w!("ScreenGauge"),
};

/// What the switch and the tray menu's check mark show now. Not knowing which copy this is, it is an error: the Store
/// copy must never read, nor write, the installer copy's `Run` value.
pub fn state() -> windows::core::Result<StartWithWindows> {
    match package::this_copy()? {
        AppCopy::Store => Ok(of_task(startup_task()?.State()?)),
        AppCopy::Installer => state_in(&WINDOWS),
    }
}

/// Has ScreenGauge start at sign-in, or not: the installer copy lists this `.exe` as it runs now, or takes it off the
/// list, and the Store copy enables or disables its startup task. What the user chose in Windows stays as it is either
/// way: Windows doesn't enable a task the user disabled.
pub fn turn(on: bool) -> windows::core::Result<()> {
    match package::this_copy()? {
        AppCopy::Store => {
            let task = startup_task()?;
            if on {
                // A packaged desktop app's request shows no dialog; Windows answers with the task's new state, which
                // `click` reads again.
                task.RequestEnableAsync()?.join()?;
                Ok(())
            } else {
                task.Disable()
            }
        }
        AppCopy::Installer if on => {
            let exe = std::env::current_exe().map_err(|e| Error::new(E_FAIL, e.to_string()))?;
            list(&WINDOWS, &command_line(exe.as_os_str()))
        }
        AppCopy::Installer => unlist(&WINDOWS),
    }
}

/// What a click on Settings' switch or on the tray menu's item did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Clicked {
    /// Turned on or off, as the user asked.
    Turned,
    /// Windows has it turned off: the amber message says where to turn it on. Nothing changed, or, from off, ScreenGauge
    /// is now listed where Windows shows the user's choice, which Windows still has off.
    TurnedOffInWindows,
}

/// A click on the switch or the item: it turns "Start with Windows" to the other side, unless Windows turned it off,
/// and the settings file follows. The registry is written first, as Windows is what holds it.
pub fn click() -> windows::core::Result<Clicked> {
    let on = match state()?.click() {
        Click::TurnOn => true,
        Click::TurnOff => false,
        Click::SayTurnedOffInWindows => return Ok(Clicked::TurnedOffInWindows),
    };
    turn(on)?;
    let saved = settings_file::update(|stored| stored.options.apply(Change::StartWithWindows(on)));
    // It holds at the next sign-in even when the file couldn't be saved.
    if let Err(e) = saved {
        log::failure("Start with Windows: saving the choice", &e);
    }
    Ok(match state() {
        Ok(StartWithWindows::TurnedOffInWindows) => Clicked::TurnedOffInWindows,
        Ok(StartWithWindows::On | StartWithWindows::Off) => Clicked::Turned,
        // The change is made; what shows reads it again, and says so should that fail too.
        Err(e) => {
            log::failure("Start with Windows: reading it after the click", &e);
            Clicked::Turned
        }
    })
}

/// The Store copy's startup task, asked of Windows each time, so what the user changed in Task Manager since shows.
fn startup_task() -> windows::core::Result<StartupTask> {
    StartupTask::GetAsync(&HSTRING::from(STARTUP_TASK_ID))?.join()
}

/// What the switch shows for the Store copy's task in `state`. A policy that holds it off shows as the user's choice in
/// Windows, as ScreenGauge can't turn it on either; one that holds it on shows on. A state Windows may add later shows
/// off, and a click then asks Windows to enable the task, which never overrides the user.
fn of_task(state: StartupTaskState) -> StartWithWindows {
    match state {
        StartupTaskState::Enabled | StartupTaskState::EnabledByPolicy => StartWithWindows::On,
        StartupTaskState::DisabledByUser | StartupTaskState::DisabledByPolicy => {
            StartWithWindows::TurnedOffInWindows
        }
        _ => StartWithWindows::Off,
    }
}

/// The `Run` value's text: the `.exe`'s path in quotes, as it may hold spaces, then [`SIGN_IN`], null-terminated.
fn command_line(exe: &OsStr) -> Vec<u16> {
    let mut line: Vec<u16> = "\"".encode_utf16().collect();
    line.extend(exe.encode_wide());
    line.extend(format!("\" {SIGN_IN}").encode_utf16());
    line.push(0);
    line
}

fn state_in(keys: &Keys) -> windows::core::Result<StartWithWindows> {
    let listed = is_listed(keys)?;
    let approved = approved(keys)?;
    Ok(StartWithWindows::of(listed, approved.as_deref()))
}

/// Whether the `Run` key holds ScreenGauge's value. Its key or its value missing is not listed.
fn is_listed(keys: &Keys) -> windows::core::Result<bool> {
    // SAFETY: no buffer: only whether the value is there, of a text type, is asked. The names outlive the call.
    let read = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            keys.run,
            keys.name,
            RRF_RT_REG_SZ,
            None,
            None,
            None,
        )
    };
    match read {
        ERROR_SUCCESS => Ok(true),
        ERROR_FILE_NOT_FOUND => Ok(false),
        error => Err(failed(error)),
    }
}

/// What Windows keeps of the user's choice for ScreenGauge, or `None` while the user has never chosen.
fn approved(keys: &Keys) -> windows::core::Result<Option<Vec<u8>>> {
    // Windows writes 12 bytes; room for more, should another build write more.
    let mut bytes = vec![0u8; 64];
    let mut size = 64;
    // SAFETY: `bytes` and `size` outlive the call, which writes at most `size` bytes into `bytes`. The names outlive
    // the call.
    let read = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            keys.approved,
            keys.name,
            RRF_RT_REG_BINARY,
            None,
            Some(bytes.as_mut_ptr().cast()),
            Some(&mut size),
        )
    };
    match read {
        ERROR_SUCCESS => {
            // At most the 64 bytes asked for, which a usize holds.
            bytes.truncate(usize::try_from(size).unwrap_or(bytes.len()));
            Ok(Some(bytes))
        }
        ERROR_FILE_NOT_FOUND => Ok(None),
        error => Err(failed(error)),
    }
}

/// Writes ScreenGauge's `Run` value, `line` a null-terminated text, making the key should it be missing.
fn list(keys: &Keys, line: &[u16]) -> windows::core::Result<()> {
    let bytes = u32::try_from(size_of_val(line))
        .map_err(|_| Error::new(E_FAIL, "a command line longer than the registry keeps"))?;
    // SAFETY: `line` outlives the call, which reads `bytes` bytes of it. The names outlive the call.
    let written = unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            keys.run,
            keys.name,
            REG_SZ.0,
            Some(line.as_ptr().cast()),
            bytes,
        )
    };
    match written {
        ERROR_SUCCESS => Ok(()),
        error => Err(failed(error)),
    }
}

/// Deletes ScreenGauge's `Run` value. Already gone is done.
fn unlist(keys: &Keys) -> windows::core::Result<()> {
    // SAFETY: the names outlive the call.
    match unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, keys.run, keys.name) } {
        ERROR_SUCCESS | ERROR_FILE_NOT_FOUND => Ok(()),
        error => Err(failed(error)),
    }
}

fn failed(error: WIN32_ERROR) -> Error {
    Error::from(error.to_hresult())
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::System::Registry::{REG_BINARY, RegDeleteTreeW};

    /// Keys of its own per test under `HKCU\Software`, deleted when the test ends: no test ever touches Windows' own
    /// `Run` key, which would start the test's `.exe` at the next sign-in.
    struct TestKeys {
        root: HSTRING,
        run: HSTRING,
        approved: HSTRING,
    }

    impl TestKeys {
        fn new(test: &str) -> TestKeys {
            let root = format!(r"Software\ScreenGauge.Test.{}.{test}", std::process::id());
            let keys = TestKeys {
                run: HSTRING::from(format!(r"{root}\Run")),
                approved: HSTRING::from(format!(r"{root}\StartupApproved\Run")),
                root: HSTRING::from(root),
            };
            keys.delete();
            keys
        }

        fn keys(&self) -> Keys {
            Keys {
                run: PCWSTR(self.run.as_ptr()),
                approved: PCWSTR(self.approved.as_ptr()),
                name: WINDOWS.name,
            }
        }

        /// Writes what Windows writes when the user chooses in Task Manager.
        fn choose_in_windows(&self, bytes: &[u8]) {
            // SAFETY: `bytes` outlives the call, which reads its length; the names outlive the call.
            let written = unsafe {
                RegSetKeyValueW(
                    HKEY_CURRENT_USER,
                    PCWSTR(self.approved.as_ptr()),
                    WINDOWS.name,
                    REG_BINARY.0,
                    Some(bytes.as_ptr().cast()),
                    u32::try_from(bytes.len()).expect("a few bytes"),
                )
            };
            assert_eq!(written, ERROR_SUCCESS);
        }

        /// The `Run` value's text, or `None` without it.
        fn run_value(&self) -> Option<String> {
            let mut text = vec![0u16; 1024];
            let mut size = 2048;
            // SAFETY: `text` and `size` outlive the call, which writes at most `size` bytes into `text`.
            let read = unsafe {
                RegGetValueW(
                    HKEY_CURRENT_USER,
                    PCWSTR(self.run.as_ptr()),
                    WINDOWS.name,
                    RRF_RT_REG_SZ,
                    None,
                    Some(text.as_mut_ptr().cast()),
                    Some(&mut size),
                )
            };
            match read {
                ERROR_SUCCESS => {
                    let units = usize::try_from(size).expect("small") / 2;
                    let text = String::from_utf16(&text[..units]).expect("a Unicode value");
                    Some(text.trim_end_matches('\0').to_owned())
                }
                ERROR_FILE_NOT_FOUND => None,
                error => panic!("reading the Run value: {error:?}"),
            }
        }

        fn delete(&self) {
            // SAFETY: a key of this test's own.
            let deleted = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, PCWSTR(self.root.as_ptr())) };
            assert!(
                deleted == ERROR_SUCCESS || deleted == ERROR_FILE_NOT_FOUND,
                "{deleted:?}"
            );
        }
    }

    impl Drop for TestKeys {
        fn drop(&mut self) {
            self.delete();
        }
    }

    /// What Windows wrote on the owner's computer for an entry turned off in Task Manager, and for one left on.
    const TURNED_OFF: [u8; 12] = [3, 0, 0, 0, 0xd8, 0x4d, 0xca, 0xb1, 0x59, 0x53, 0xdd, 0x01];
    const TURNED_ON: [u8; 12] = [2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];

    fn line(exe: &str) -> Vec<u16> {
        command_line(OsStr::new(exe))
    }

    #[test]
    fn the_store_copy_s_task_shows_on_only_while_windows_has_it_on() {
        assert_eq!(of_task(StartupTaskState::Disabled), StartWithWindows::Off);
        assert_eq!(of_task(StartupTaskState::Enabled), StartWithWindows::On);
        assert_eq!(
            of_task(StartupTaskState::EnabledByPolicy),
            StartWithWindows::On
        );
    }

    #[test]
    fn the_store_copy_s_task_turned_off_in_windows_shows_so() {
        // The user turned it off in Task Manager or in Settings > Apps > Startup.
        assert_eq!(
            of_task(StartupTaskState::DisabledByUser),
            StartWithWindows::TurnedOffInWindows
        );
        // An administrator's policy holds it off: ScreenGauge can't turn it on either.
        assert_eq!(
            of_task(StartupTaskState::DisabledByPolicy),
            StartWithWindows::TurnedOffInWindows
        );
    }

    #[test]
    fn with_nothing_in_windows_it_is_off() {
        let test = TestKeys::new("nothing");
        assert_eq!(
            state_in(&test.keys()).expect("the state"),
            StartWithWindows::Off
        );
    }

    #[test]
    fn turned_on_it_is_listed_with_the_exe_in_quotes_and_the_sign_in_flag() {
        let test = TestKeys::new("listed");
        let exe = r"C:\Users\Ana Lima\AppData\Local\Programs\ScreenGauge\ScreenGauge.exe";
        list(&test.keys(), &line(exe)).expect("listed");
        assert_eq!(
            test.run_value().as_deref(),
            Some(
                r#""C:\Users\Ana Lima\AppData\Local\Programs\ScreenGauge\ScreenGauge.exe" --sign-in"#
            )
        );
        assert_eq!(
            state_in(&test.keys()).expect("the state"),
            StartWithWindows::On
        );
    }

    #[test]
    fn turned_off_its_value_goes_and_turning_it_off_again_is_fine() {
        let test = TestKeys::new("unlisted");
        list(&test.keys(), &line(r"C:\ScreenGauge.exe")).expect("listed");
        unlist(&test.keys()).expect("unlisted");
        assert_eq!(test.run_value(), None);
        assert_eq!(
            state_in(&test.keys()).expect("the state"),
            StartWithWindows::Off
        );
        unlist(&test.keys()).expect("already gone is done");
    }

    #[test]
    fn turned_off_in_task_manager_it_shows_so_and_on_again_there_it_is_on() {
        let test = TestKeys::new("approved");
        list(&test.keys(), &line(r"C:\ScreenGauge.exe")).expect("listed");
        test.choose_in_windows(&TURNED_OFF);
        assert_eq!(
            state_in(&test.keys()).expect("the state"),
            StartWithWindows::TurnedOffInWindows
        );
        test.choose_in_windows(&TURNED_ON);
        assert_eq!(
            state_in(&test.keys()).expect("the state"),
            StartWithWindows::On
        );
    }

    #[test]
    fn listing_and_unlisting_never_touch_what_the_user_chose_in_windows() {
        let test = TestKeys::new("untouched");
        test.choose_in_windows(&TURNED_OFF);
        list(&test.keys(), &line(r"C:\ScreenGauge.exe")).expect("listed");
        unlist(&test.keys()).expect("unlisted");
        assert_eq!(
            approved(&test.keys()).expect("readable"),
            Some(TURNED_OFF.to_vec())
        );
    }
}
