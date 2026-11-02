//! PROTOTYPE, throwaway (WPC-347). Never merged into main.
//!
//! Shows Windows' standard notification (`Windows.UI.Notifications`) from an unpackaged exe and logs everything Windows
//! says back, to `toast-probe.log` next to the exe. The registry setup for each variant is done by `probe.ps1`.
//!
//! toast-probe show <a|b|c> [--com] [--wait SECS] [--tag TAG] [--text TEXT] [--suppress-popup]
//! toast-probe serve <a|b|c> [--wait SECS]       registers the COM activator and waits, as a running app would
//! toast-probe history <a|b|c>                   logs what the notification center keeps for the variant
//! toast-probe clear <a|b|c>                     clears the variant's notifications from the center
//! toast-probe cold <a|b|c> ...                  what Windows runs on a click when the app is closed (LocalServer32)

use std::ffi::c_void;
use std::io::Write;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use windows::Data::Xml::Dom::XmlDocument;
use windows::Foundation::TypedEventHandler;
use windows::UI::Notifications::{
    ToastActivatedEventArgs, ToastDismissedEventArgs, ToastFailedEventArgs, ToastNotification,
    ToastNotificationManager,
};
use windows::Win32::Foundation::{CLASS_E_NOAGGREGATION, E_NOINTERFACE};
use windows::Win32::System::Com::{
    CLSCTX_LOCAL_SERVER, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx, CoRegisterClassObject,
    CoRevokeClassObject, IClassFactory, IClassFactory_Impl, REGCLS_AGILE, REGCLS_MULTIPLEUSE,
};
use windows::Win32::System::Environment::GetCommandLineW;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Notifications::{
    INotificationActivationCallback, INotificationActivationCallback_Impl, NOTIFICATION_USER_INPUT_DATA,
};
use windows::Win32::UI::Shell::SHQueryUserNotificationState;
use windows::Win32::UI::WindowsAndMessaging::{DispatchMessageW, MSG, PM_REMOVE, PeekMessageW, TranslateMessage};
use windows::core::{BOOL, GUID, HSTRING, IInspectable, IUnknown, Interface, PCWSTR, Ref, Result, implement};

struct Variant {
    key: &'static str,
    aumid: &'static str,
    clsid: GUID,
}

const VARIANTS: [Variant; 3] = [
    // a: AUMID registry keys only, no CustomActivator.
    Variant {
        key: "a",
        aumid: "WPCSolutions.ScreenGaugeProbeA",
        clsid: GUID::from_u128(0xd4c5e347_0a0a_4a7e_9f2c_5c1e0d347a01),
    },
    // b: CustomActivator set, no LocalServer32 key.
    Variant {
        key: "b",
        aumid: "WPCSolutions.ScreenGaugeProbeB",
        clsid: GUID::from_u128(0xd4c5e347_0b0b_4a7e_9f2c_5c1e0d347b01),
    },
    // c: CustomActivator plus LocalServer32 pointing at this exe: the full setup.
    Variant {
        key: "c",
        aumid: "WPCSolutions.ScreenGaugeProbeC",
        clsid: GUID::from_u128(0xd4c5e347_0c0c_4a7e_9f2c_5c1e0d347c01),
    },
];

fn log(line: &str) {
    let ms = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    // SAFETY: no arguments.
    let tid = unsafe { GetCurrentThreadId() };
    let text = format!("{ms} pid={} tid={tid} {line}\n", std::process::id());
    print!("{text}");
    let path = std::env::current_exe().map(|p| p.with_file_name("toast-probe.log")).expect("exe path");
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(path).expect("open log");
    f.write_all(text.as_bytes()).expect("write log");
}

#[implement(INotificationActivationCallback)]
struct Callback;

impl INotificationActivationCallback_Impl for Callback_Impl {
    fn Activate(
        &self,
        aumid: &PCWSTR,
        args: &PCWSTR,
        _data: *const NOTIFICATION_USER_INPUT_DATA,
        count: u32,
    ) -> Result<()> {
        // SAFETY: Windows passes valid strings for the call's duration.
        let (aumid, args) = unsafe { (aumid.to_string().unwrap_or_default(), args.to_string().unwrap_or_default()) };
        log(&format!("COM Activate aumid={aumid} args={args:?} inputs={count}"));
        Ok(())
    }
}

#[implement(IClassFactory)]
struct Factory;

impl IClassFactory_Impl for Factory_Impl {
    fn CreateInstance(&self, outer: Ref<IUnknown>, iid: *const GUID, object: *mut *mut c_void) -> Result<()> {
        // SAFETY: Windows passes a valid out pointer.
        unsafe { *object = std::ptr::null_mut() };
        if outer.is_some() {
            return Err(CLASS_E_NOAGGREGATION.into());
        }
        log("COM CreateInstance");
        let callback: INotificationActivationCallback = Callback.into();
        // SAFETY: iid and object come from COM.
        let hr = unsafe { callback.query(iid, object) };
        if hr.is_err() { Err(E_NOINTERFACE.into()) } else { Ok(()) }
    }

    fn LockServer(&self, lock: BOOL) -> Result<()> {
        log(&format!("COM LockServer {}", lock.as_bool()));
        Ok(())
    }
}

fn register_activator(v: &Variant) -> Result<u32> {
    let factory: IClassFactory = Factory.into();
    // SAFETY: the factory lives until CoRevokeClassObject.
    let cookie =
        unsafe { CoRegisterClassObject(&v.clsid, &factory, CLSCTX_LOCAL_SERVER, REGCLS_MULTIPLEUSE | REGCLS_AGILE) }?;
    log(&format!("CoRegisterClassObject {:?} cookie={cookie}", v.clsid));
    Ok(cookie)
}

fn quns() -> String {
    // SAFETY: no pointers.
    match unsafe { SHQueryUserNotificationState() } {
        Ok(s) => format!("{}", s.0),
        Err(e) => format!("error {e}"),
    }
}

fn history(v: &Variant) -> String {
    let result = (|| -> Result<String> {
        let list = ToastNotificationManager::History()?.GetHistoryWithId(&HSTRING::from(v.aumid))?;
        let mut tags = Vec::new();
        for i in 0..list.Size()? {
            tags.push(list.GetAt(i)?.Tag()?.to_string());
        }
        Ok(format!("{} {:?}", tags.len(), tags))
    })();
    result.unwrap_or_else(|e| format!("error {e}"))
}

/// Pumps messages until the deadline, logging the center's history for the variant whenever it changes.
fn pump(v: &Variant, seconds: u64) {
    let end = Instant::now() + Duration::from_secs(seconds);
    let mut last = String::new();
    let mut next_check = Instant::now();
    while Instant::now() < end {
        let mut msg = MSG::default();
        // SAFETY: msg is a valid out pointer.
        while unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE) }.as_bool() {
            // SAFETY: msg came from PeekMessageW.
            unsafe {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        if Instant::now() >= next_check {
            let now = history(v);
            if now != last {
                log(&format!("history {now} quns={}", quns()));
                last = now;
            }
            next_check = Instant::now() + Duration::from_millis(500);
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1).cloned())
}

fn variant(key: Option<&String>) -> &'static Variant {
    let key = key.map(String::as_str).unwrap_or("");
    VARIANTS.iter().find(|v| v.key == key).unwrap_or_else(|| panic!("unknown variant {key:?}"))
}

fn show(v: &Variant, args: &[String]) -> Result<()> {
    let tag = arg(args, "--tag").unwrap_or_else(|| "t1".into());
    let text = arg(args, "--text").unwrap_or_else(|| format!("ScreenGauge probe {} {tag}", v.key));
    let wait = arg(args, "--wait").and_then(|s| s.parse().ok()).unwrap_or(20);
    let cookie = if args.iter().any(|a| a == "--com") { Some(register_activator(v)?) } else { None };

    let xml = XmlDocument::new()?;
    xml.LoadXml(&HSTRING::from(format!(
        "<toast launch=\"probe-launch-{}-{tag}\"><visual><binding template=\"ToastGeneric\"><text>{text}</text></binding></visual></toast>",
        v.key
    )))?;
    let toast = ToastNotification::CreateToastNotification(&xml)?;
    toast.SetTag(&HSTRING::from(&tag))?;
    if args.iter().any(|a| a == "--suppress-popup") {
        toast.SetSuppressPopup(true)?;
    }
    toast.Activated(&TypedEventHandler::<ToastNotification, IInspectable>::new(|_, a| {
        let what = a.ok().and_then(|a| a.cast::<ToastActivatedEventArgs>()).and_then(|a| a.Arguments());
        log(&format!("event Activated args={what:?}"));
        Ok(())
    }))?;
    toast.Dismissed(&TypedEventHandler::<ToastNotification, ToastDismissedEventArgs>::new(|_, a| {
        log(&format!("event Dismissed reason={:?}", a.ok().and_then(|a| a.Reason()).map(|r| r.0)));
        Ok(())
    }))?;
    toast.Failed(&TypedEventHandler::<ToastNotification, ToastFailedEventArgs>::new(|_, a| {
        log(&format!("event Failed error={:?}", a.ok().and_then(|a| a.ErrorCode())));
        Ok(())
    }))?;

    let notifier = ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(v.aumid))?;
    log(&format!("notifier setting={:?} quns={}", notifier.Setting().map(|s| s.0), quns()));
    notifier.Show(&toast)?;
    log(&format!("Show ok variant={} tag={tag} text={text:?}", v.key));
    pump(v, wait);
    if let Some(cookie) = cookie {
        // SAFETY: cookie came from CoRegisterClassObject.
        unsafe { CoRevokeClassObject(cookie) }?;
    }
    log("end");
    Ok(())
}

fn main() -> Result<()> {
    // SAFETY: no reserved pointer; the same apartment ScreenGauge's window thread uses.
    unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) }.ok()?;
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("cold") => {
            // What Windows runs on a click with the app closed: register first, then look around.
            let v = variant(args.get(2));
            let cookie = register_activator(v)?;
            // SAFETY: GetCommandLineW returns the process's own string.
            log(&format!("cold start command line: {}", unsafe { GetCommandLineW().to_string() }.unwrap_or_default()));
            pump(v, arg(&args, "--wait").and_then(|s| s.parse().ok()).unwrap_or(20));
            // SAFETY: cookie came from CoRegisterClassObject.
            unsafe { CoRevokeClassObject(cookie) }?;
            log("cold end");
        }
        Some("show") => show(variant(args.get(2)), &args)?,
        Some("serve") => {
            let v = variant(args.get(2));
            let cookie = register_activator(v)?;
            pump(v, arg(&args, "--wait").and_then(|s| s.parse().ok()).unwrap_or(30));
            // SAFETY: cookie came from CoRegisterClassObject.
            unsafe { CoRevokeClassObject(cookie) }?;
            log("serve end");
        }
        Some("history") => log(&format!("history {} {}", args[2], history(variant(args.get(2))))),
        Some("clear") => {
            let v = variant(args.get(2));
            ToastNotificationManager::History()?.ClearWithId(&HSTRING::from(v.aumid))?;
            log(&format!("cleared {}", v.aumid));
        }
        _ => {
            // SAFETY: GetCommandLineW returns the process's own string.
            log(&format!("unknown command line: {}", unsafe { GetCommandLineW().to_string() }.unwrap_or_default()));
        }
    }
    Ok(())
}
