//! One copy of ScreenGauge at a time, the Store copy and the installer copy included: both claim the
//! same named mutex, so the name depends on nothing but this constant (not the package, not the path).

use windows::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE};
use windows::Win32::System::Threading::CreateMutexW;
use windows::core::HSTRING;

use crate::log;

/// `Local\` is the signed-in user's session, so each Windows account runs its own copy.
pub const NAME: &str = "Local\\ScreenGauge.SingleInstance";

pub enum Instance {
    /// This process is the copy that runs; it stays the one while the guard lives.
    First(InstanceGuard),
    /// Another copy holds the name, so this process should exit.
    AlreadyRunning,
}

/// Keeps the name claimed until dropped, normally at process exit.
pub struct InstanceGuard(HANDLE);

impl Drop for InstanceGuard {
    fn drop(&mut self) {
        // SAFETY: the handle came from CreateMutexW and is closed once, here.
        if let Err(e) = unsafe { CloseHandle(self.0) } {
            log::line(&format!("single instance: CloseHandle: {e}"));
        }
    }
}

pub fn claim(name: &str) -> windows::core::Result<Instance> {
    // SAFETY: default security, no initial owner, and a valid name that outlives the call.
    let handle = unsafe { CreateMutexW(None, false, &HSTRING::from(name)) }?;
    // Read right away: the error code of a successful CreateMutexW tells whether the mutex already existed.
    // SAFETY: GetLastError only reads this thread's last error code.
    let already_existed = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
    let guard = InstanceGuard(handle);
    if already_existed {
        // The guard drops here and closes this process's handle; the first copy keeps its own.
        return Ok(Instance::AlreadyRunning);
    }
    Ok(Instance::First(guard))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A name of its own per test, so a ScreenGauge running on this computer never interferes.
    fn test_name(test: &str) -> String {
        format!("Local\\ScreenGauge.Test.{}.{test}", std::process::id())
    }

    #[test]
    fn the_first_copy_runs() {
        let name = test_name("first");
        assert!(matches!(claim(&name), Ok(Instance::First(_))));
    }

    #[test]
    fn a_second_copy_finds_the_first_running() {
        let name = test_name("second");
        let first = claim(&name);
        assert!(matches!(first, Ok(Instance::First(_))));
        assert!(matches!(claim(&name), Ok(Instance::AlreadyRunning)));
    }

    #[test]
    fn a_new_copy_runs_once_the_first_has_exited() {
        let name = test_name("after-exit");
        let first = claim(&name);
        assert!(matches!(first, Ok(Instance::First(_))));
        drop(first);
        assert!(matches!(claim(&name), Ok(Instance::First(_))));
    }
}
