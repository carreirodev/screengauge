//! COM, which WIC needs to write the Snip's PNG. It starts the first time the Snip takes an image, not at startup, as
//! nothing is prepared before the first call (v1 spec WPC-296, Metas de velocidade e de consumo).

use std::cell::Cell;

use windows::Win32::System::Com::{
    COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx,
};

thread_local! {
    /// Whether COM is started on this thread.
    static STARTED: Cell<bool> = const { Cell::new(false) };
}

/// Starts COM on this thread in a single-threaded apartment, once; later calls do nothing. Single-threaded because this
/// is the thread of ScreenGauge's windows: Windows' Save As window, which the Snip opens with WPC-318, needs that
/// apartment, and a thread can't change apartments once COM is started on it. COM then stays started until the thread
/// ends, which for the windows' thread is when ScreenGauge exits, so no COM object made on it outlives COM.
pub fn ensure_started() -> windows::core::Result<()> {
    if STARTED.get() {
        return Ok(());
    }
    // SAFETY: no reserved pointer. Never undone: COM goes with the thread. S_FALSE, should COM already be started this
    // way, is success.
    unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) }.ok()?;
    STARTED.set(true);
    Ok(())
}
