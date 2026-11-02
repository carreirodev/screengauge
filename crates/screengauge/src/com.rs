//! COM, which WIC needs to write the Snip's PNG, started once for the thread the resident window and the tools run on.

use std::marker::PhantomData;

use windows::Win32::System::Com::{
    COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx, CoUninitialize,
};

/// COM started on this thread in a single-threaded apartment, until it drops. Single-threaded because this is the
/// thread of ScreenGauge's windows: Windows' Save As window, which the Snip opens with WPC-318, needs that apartment, and
/// a thread can't change apartments once COM is started on it.
pub struct Apartment {
    /// Keeps it on the thread that started COM: a raw pointer is neither Send nor Sync.
    _thread: PhantomData<*const ()>,
}

impl Apartment {
    /// Starts COM for this thread, for as long as the [`Apartment`] lives. Every COM object made on the thread must be
    /// released before it drops.
    pub fn enter() -> windows::core::Result<Apartment> {
        // SAFETY: no reserved pointer. Undone once by the drop of the Apartment returned; S_FALSE, a thread that already
        // started COM this way, is success and is undone the same way.
        unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) }.ok()?;
        Ok(Apartment {
            _thread: PhantomData,
        })
    }
}

impl Drop for Apartment {
    fn drop(&mut self) {
        // SAFETY: pairs the CoInitializeEx in `enter`, on the same thread: the Apartment is neither Send nor Sync.
        unsafe { CoUninitialize() };
    }
}
