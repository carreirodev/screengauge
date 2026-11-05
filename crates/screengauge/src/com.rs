//! COM, which the Windows notifications' activator needs from startup, WIC to write the Snip's PNG, and the Windows
//! objects the `windows` crate keeps for the whole process, such as the one that reads GitHub's reply.

use std::cell::Cell;
use std::sync::atomic::{AtomicBool, Ordering};

use windows::Win32::System::Com::{
    COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoIncrementMTAUsage, CoInitializeEx,
};
#[cfg(test)]
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoUninitialize};

thread_local! {
    /// Whether COM is started on this thread.
    static STARTED: Cell<bool> = const { Cell::new(false) };
}

/// Whether the process keeps COM's multithreaded apartment open until it exits.
static MULTITHREADED_KEPT: AtomicBool = AtomicBool::new(false);

/// Starts COM on this thread in a single-threaded apartment, once; later calls do nothing. Single-threaded because this
/// is the thread of ScreenGauge's windows: Windows' Save As window, which the Snip opens, needs that
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

/// Keeps COM's multithreaded apartment open until ScreenGauge exits; once it has, later calls do nothing. Called before
/// the first use of a Windows object the `windows` crate keeps for the whole process, such as the one that makes the
/// `JsonObject` reading GitHub's reply. The crate keeps the one it makes in the apartment that is open at the time and
/// never lets it go. When another thread opened that apartment and later ends its COM, the apartment closes under the
/// kept object, and the next use, on any thread, fails with "CoInitialize has not been called" or crashes ScreenGauge
/// (WPC-387). The crate keeps the apartment open itself only when none is open at all.
pub fn keep_multithreaded_apartment() -> windows::core::Result<()> {
    if MULTITHREADED_KEPT.load(Ordering::Acquire) {
        return Ok(());
    }
    // SAFETY: no pointers. The cookie is never handed to CoDecrementMTAUsage, so the apartment stays open until the
    // process exits; two threads getting here at once only keep it open twice.
    unsafe { CoIncrementMTAUsage() }?;
    MULTITHREADED_KEPT.store(true, Ordering::Release);
    Ok(())
}

/// For tests, which run on worker threads where COM isn't started: COM for the test's thread in the multithreaded
/// apartment, which WIC and the test bitmaps need, ended when this drops, after every object made with it: also when a
/// check fails and unwinds. The process keeps the apartment open first, so this thread ending its COM never closes it
/// under the Windows objects the `windows` crate keeps for every test, such as `XmlDocument`'s (WPC-387).
#[cfg(test)]
pub struct TestCom;

#[cfg(test)]
impl TestCom {
    pub fn start() -> TestCom {
        keep_multithreaded_apartment().expect("the process keeps COM's multithreaded apartment");
        // SAFETY: starts COM for this thread, undone once by the drop of the TestCom it returns. A thread that already
        // started it in the same way gets S_FALSE, which is success.
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }
            .ok()
            .expect("COM starts on a test thread");
        TestCom
    }
}

#[cfg(test)]
impl Drop for TestCom {
    fn drop(&mut self) {
        // SAFETY: pairs the CoInitializeEx in `start`.
        unsafe { CoUninitialize() };
    }
}

#[cfg(test)]
mod tests {
    use std::thread;

    use windows::Data::Json::JsonObject;
    use windows::Data::Xml::Dom::XmlDocument;
    use windows::core::HSTRING;

    use super::*;
    use crate::test_process;

    /// The tag of a JSON object read from text, `"v1.1.0"`.
    fn tag_from_json() -> windows::core::Result<String> {
        let object = JsonObject::Parse(&HSTRING::from(r#"{"tag_name":"v1.1.0"}"#))?;
        Ok(object
            .GetNamedString(&HSTRING::from("tag_name"))?
            .to_string_lossy())
    }

    #[test]
    fn json_and_xml_still_work_after_a_test_com_ended() {
        test_process::run_alone(concat!(
            module_path!(),
            "::alone_json_and_xml_still_work_after_a_test_com_ended"
        ));
    }

    /// One thread reads JSON and makes an XML document inside a TestCom, which ends; then a thread with no COM does the
    /// same. Alone, because another test holding COM open would hide what happens once nothing does.
    #[test]
    #[ignore = "the shared run starts it alone, in a process of its own, from the test above"]
    fn alone_json_and_xml_still_work_after_a_test_com_ended() {
        thread::spawn(|| {
            let _com = TestCom::start();
            assert_eq!(
                tag_from_json().expect("JSON read inside a TestCom"),
                "v1.1.0"
            );
            XmlDocument::new().expect("an XML document inside a TestCom");
        })
        .join()
        .expect("the TestCom's thread");
        thread::spawn(|| {
            assert_eq!(tag_from_json().expect("JSON read with no COM"), "v1.1.0");
            XmlDocument::new().expect("an XML document with no COM");
        })
        .join()
        .expect("the thread with no COM");
    }
}
