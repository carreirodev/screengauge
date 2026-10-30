//! One copy of ScreenGauge at a time, the Store copy and the installer copy included: both claim the
//! same named mutex, so the name depends on nothing but this constant (not the package, not the path).

use windows::Win32::Foundation::{
    CloseHandle, ERROR_ACCESS_DENIED, ERROR_ALREADY_EXISTS, GetLastError, HANDLE,
};
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
            log::failure("single instance: CloseHandle", &e);
        }
    }
}

pub fn claim(name: &str) -> windows::core::Result<Instance> {
    // Bound before the call, so dropping it can't run between CreateMutexW and GetLastError.
    let name = HSTRING::from(name);
    // SAFETY: default security, no initial owner, and a valid name that outlives the call.
    let handle = match unsafe { CreateMutexW(None, false, &name) } {
        Ok(handle) => handle,
        // The name exists, but this process may not open it: a copy started as administrator holds it.
        Err(e) if e.code() == ERROR_ACCESS_DENIED.to_hresult() => {
            return Ok(Instance::AlreadyRunning);
        }
        Err(e) => return Err(e),
    };
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
    use windows::Win32::Foundation::{HLOCAL, LocalFree};
    use windows::Win32::Security::Authorization::{
        ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
    };
    use windows::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
    use windows::core::w;

    use super::*;
    use crate::win32::struct_size;

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

    #[test]
    fn a_copy_this_process_cannot_open_counts_as_running() {
        let name = test_name("no-access");
        let _other = claim_as_administrator(&name);
        assert!(matches!(claim(&name), Ok(Instance::AlreadyRunning)));
    }

    /// The name as a copy started as administrator leaves it: it exists, but this process may only
    /// wait on it (SYNCHRONIZE), not open it fully.
    fn claim_as_administrator(name: &str) -> InstanceGuard {
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        // SAFETY: a constant SDDL string; the descriptor it allocates is freed below.
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                w!("D:(A;;0x00100000;;;WD)"),
                SDDL_REVISION_1,
                &mut descriptor,
                None,
            )
        }
        .expect("SDDL conversion");
        let attributes = SECURITY_ATTRIBUTES {
            nLength: const { struct_size::<SECURITY_ATTRIBUTES>() },
            lpSecurityDescriptor: descriptor.0,
            bInheritHandle: false.into(),
        };
        let name = HSTRING::from(name);
        // SAFETY: the attributes and the name outlive the call.
        let handle =
            unsafe { CreateMutexW(Some(&attributes), false, &name) }.expect("CreateMutexW");
        // SAFETY: the descriptor came from the SDDL conversion and is freed once; null means freed.
        assert!(unsafe { LocalFree(Some(HLOCAL(descriptor.0))) }.is_invalid());
        InstanceGuard(handle)
    }
}
