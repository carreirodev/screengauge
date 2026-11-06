//! The COM activator: what Windows calls when a notification is clicked, in the corner of the screen or in the
//! notification center, with ScreenGauge running or not. Windows starts the `.exe` with -Embedding for it when
//! ScreenGauge is closed (WPC-347, scenario s7); the class is registered at every start, so that process takes the click.

use std::cell::Cell;
use std::ffi::c_void;

use windows::Win32::Foundation::{CLASS_E_NOAGGREGATION, E_INVALIDARG, E_POINTER};
use windows::Win32::System::Com::{
    CLSCTX_LOCAL_SERVER, CoRegisterClassObject, CoRevokeClassObject, IClassFactory,
    IClassFactory_Impl, REGCLS_AGILE, REGCLS_MULTIPLEUSE,
};
use windows::Win32::UI::Notifications::{
    INotificationActivationCallback, INotificationActivationCallback_Impl,
    NOTIFICATION_USER_INPUT_DATA,
};
use windows::core::{BOOL, Error, GUID, IUnknown, Interface, PCWSTR, Ref, implement};

use super::{Event, Launch, What, report};

thread_local! {
    /// The registration of the class, on the resident window's thread, from startup until exit.
    static COOKIE: Cell<Option<u32>> = const { Cell::new(None) };
}

/// Registers `class`, this copy's activator class, with COM, so a click comes to this process: every click while it
/// runs, from any copy of the class's caller (`REGCLS_MULTIPLEUSE`), on a thread of COM's own (`REGCLS_AGILE`, since
/// this thread's apartment is single-threaded). COM must be started on this thread. Both copies register at every
/// start: for the Store copy, the package only tells Windows which `.exe` serves the class.
pub fn register(class: GUID) -> windows::core::Result<()> {
    let factory: IClassFactory = Factory.into();
    // SAFETY: a valid class ID and factory; COM keeps its own reference to the factory until the class is revoked.
    let cookie = unsafe {
        CoRegisterClassObject(
            &class,
            &factory,
            CLSCTX_LOCAL_SERVER,
            REGCLS_MULTIPLEUSE | REGCLS_AGILE,
        )
    }?;
    COOKIE.set(Some(cookie));
    Ok(())
}

/// Takes the class back from COM as ScreenGauge exits.
pub fn revoke() -> windows::core::Result<()> {
    match COOKIE.take() {
        // SAFETY: the cookie CoRegisterClassObject returned, revoked once.
        Some(cookie) => unsafe { CoRevokeClassObject(cookie) },
        None => Ok(()),
    }
}

#[implement(INotificationActivationCallback)]
struct Callback;

impl INotificationActivationCallback_Impl for Callback_Impl {
    fn Activate(
        &self,
        _aumid: &PCWSTR,
        launch: &PCWSTR,
        _data: *const NOTIFICATION_USER_INPUT_DATA,
        _count: u32,
    ) -> windows::core::Result<()> {
        // SAFETY: Windows passes a valid, null-terminated string for the call's duration.
        let launch =
            unsafe { launch.to_string() }.map_err(|e| Error::new(E_INVALIDARG, e.to_string()))?;
        report(Event {
            launch: Launch::from(launch),
            what: What::Clicked,
        })
    }
}

#[implement(IClassFactory)]
struct Factory;

impl IClassFactory_Impl for Factory_Impl {
    fn CreateInstance(
        &self,
        outer: Ref<IUnknown>,
        iid: *const GUID,
        object: *mut *mut c_void,
    ) -> windows::core::Result<()> {
        if object.is_null() {
            return Err(E_POINTER.into());
        }
        // SAFETY: a non-null out pointer from COM, set to null until there is an object to give.
        unsafe { *object = std::ptr::null_mut() };
        if outer.is_some() {
            return Err(CLASS_E_NOAGGREGATION.into());
        }
        let callback: INotificationActivationCallback = Callback.into();
        // SAFETY: `iid` and `object` come from COM, which asks for the interface `iid` names.
        unsafe { callback.query(iid, object) }.ok()
    }

    fn LockServer(&self, _lock: BOOL) -> windows::core::Result<()> {
        Ok(())
    }
}
