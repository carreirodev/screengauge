//! Windows' standard notifications (`Windows.UI.Notifications`) for the installer copy, which has no package: the
//! AppUserModelID and the activator are registered in HKCU at every start, with no Start menu shortcut (ADR 0003). The
//! Store copy declares them in its manifest instead (WPC-334).

use std::cell::RefCell;
use std::ffi::{OsStr, OsString};
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::time::Instant;
use std::{fs, io};

use windows::Data::Xml::Dom::XmlDocument;
use windows::Foundation::TypedEventHandler;
use windows::UI::Notifications::{
    ToastDismissalReason, ToastDismissedEventArgs, ToastFailedEventArgs, ToastNotification,
    ToastNotificationManager,
};
use windows::Win32::Foundation::{E_FAIL, ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
use windows::Win32::System::Registry::{
    HKEY_CURRENT_USER, REG_SZ, RegDeleteKeyValueW, RegSetKeyValueW,
};
use windows::core::{Error, GUID, HSTRING, Ref};

use super::{Dismissal, Event, Notification, Toast, What, report};
use crate::{settings_file, tray};

/// The name Windows knows ScreenGauge's notifications by: the header shows the `DisplayName` registered for it.
pub const AUMID: &str = "WPCSolutions.ScreenGauge";

/// The activator's class, which Windows starts ScreenGauge with when a notification is clicked. Never changed: the
/// notifications already in the notification center carry it, and their click would go nowhere.
pub const CLSID: GUID = GUID::from_u128(0x3beb9695_da23_4599_b10c_7cd6cd96e50c);

/// Every notification's group, which taking one out of the notification center names along with its tag.
const GROUP: &str = "ScreenGauge";

/// The app icon the header shows, at each size Windows draws it: the small icon size, as the tray icon, from 100% to
/// 300% scale (`assets/notification`, from the icon generator). Given that size, Windows shows the drawing pixel for
/// pixel (174 of 174 pixels at 150% scale, measured in WPC-347), and scales any other (20 of 223 for a 48-pixel PNG).
const ICONS: [(i32, &[u8]); 8] = [
    (
        16,
        include_bytes!("../../../../assets/notification/NotificationIcon-16.png"),
    ),
    (
        20,
        include_bytes!("../../../../assets/notification/NotificationIcon-20.png"),
    ),
    (
        24,
        include_bytes!("../../../../assets/notification/NotificationIcon-24.png"),
    ),
    (
        28,
        include_bytes!("../../../../assets/notification/NotificationIcon-28.png"),
    ),
    (
        32,
        include_bytes!("../../../../assets/notification/NotificationIcon-32.png"),
    ),
    (
        36,
        include_bytes!("../../../../assets/notification/NotificationIcon-36.png"),
    ),
    (
        40,
        include_bytes!("../../../../assets/notification/NotificationIcon-40.png"),
    ),
    (
        48,
        include_bytes!("../../../../assets/notification/NotificationIcon-48.png"),
    ),
];

thread_local! {
    /// The latest notification of each kind shown, kept until the next of its kind replaces it, so its events still
    /// come once Windows is done with it.
    static SHOWN: RefCell<Vec<(Notification, ToastNotification)>> = const { RefCell::new(Vec::new()) };
}

/// Registers the AppUserModelID, with ScreenGauge's name and the app icon, for the signed-in user. With `activator`,
/// the click of a notification goes to the activator, which Windows starts the running `.exe` for when ScreenGauge is
/// closed. Without it, a click in the notification center would only close the notification (WPC-347, scenario s6).
pub fn register(activator: bool) -> windows::core::Result<()> {
    let key = format!(r"Software\Classes\AppUserModelId\{AUMID}");
    set_value(&key, Some("DisplayName"), OsStr::new("ScreenGauge"))?;
    match write_icon() {
        Ok(icon) => set_value(&key, Some("IconUri"), icon.as_os_str())?,
        // Windows then shows a generic icon; the notification still shows.
        Err(e) => crate::log::failure("notification: writing the icon", &e),
    }
    if !activator {
        // An earlier start's activator would take the clicks with no process to answer them.
        return delete_value(&key, "CustomActivator");
    }
    let exe = std::env::current_exe().map_err(|e| Error::new(E_FAIL, e.to_string()))?;
    let clsid = format!("{{{CLSID:?}}}");
    // The path alone: COM adds -Embedding when it starts the program.
    set_value(
        &format!(r"Software\Classes\CLSID\{clsid}\LocalServer32"),
        None,
        &quoted(exe.as_os_str()),
    )?;
    set_value(&key, Some("CustomActivator"), OsStr::new(&clsid))
}

/// `path` between double quotes.
fn quoted(path: &OsStr) -> OsString {
    let mut quoted = OsString::from("\"");
    quoted.push(path);
    quoted.push("\"");
    quoted
}

/// Sets `name` (the default value without one) of HKCU's `key`, creating the key when it is missing.
fn set_value(key: &str, name: Option<&str>, value: &OsStr) -> windows::core::Result<()> {
    let key = HSTRING::from(key);
    let name = name.map(HSTRING::from).unwrap_or_default();
    // From the UTF-16 Windows gave, so a path is written as it is, whatever its characters.
    let data: Vec<u16> = value.encode_wide().chain([0]).collect();
    let size = u32::try_from(data.len() * size_of::<u16>())
        .map_err(|e| Error::new(E_FAIL, e.to_string()))?;
    // SAFETY: the strings and `data` outlive the call, which reads `size` bytes of `data`.
    match unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            &key,
            &name,
            REG_SZ.0,
            Some(data.as_ptr().cast()),
            size,
        )
    } {
        ERROR_SUCCESS => Ok(()),
        error => Err(Error::from(error.to_hresult())),
    }
}

/// Deletes `name` of HKCU's `key`; a value or key already missing is no failure.
fn delete_value(key: &str, name: &str) -> windows::core::Result<()> {
    let key = HSTRING::from(key);
    let name = HSTRING::from(name);
    // SAFETY: the strings outlive the call.
    match unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, &key, &name) } {
        ERROR_SUCCESS | ERROR_FILE_NOT_FOUND => Ok(()),
        error => Err(Error::from(error.to_hresult())),
    }
}

/// Writes the icon at the size the taskbar draws its icons now into ScreenGauge's folder, where `IconUri` points; a
/// change of scale while ScreenGauge runs takes effect at its next start. The file name carries the icon's contents,
/// because Windows keeps what it read from a path: another icon goes to another file (the uninstaller, WPC-337,
/// removes every `notification-icon-*.png`).
fn write_icon() -> windows::core::Result<PathBuf> {
    let size = tray::icon_size()?;
    let icon = ICONS
        .iter()
        .find(|(drawn, _)| *drawn >= size)
        .map_or(ICONS[ICONS.len() - 1].1, |(_, bytes)| bytes);
    let folder = settings_file::installer_copy_folder()?;
    let path = folder.join(format!("notification-icon-{:08x}.png", fnv1a(icon)));
    if !path.exists() {
        fs::create_dir_all(&folder)
            .and_then(|()| write_whole(&path, icon))
            .map_err(|e| Error::new(E_FAIL, e.to_string()))?;
    }
    Ok(path)
}

/// Writes `bytes` to `path` through a temporary file, so Windows never reads half an icon.
fn write_whole(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let temporary = path.with_extension("png.tmp");
    fs::write(&temporary, bytes)?;
    fs::rename(&temporary, path)
}

/// The 32-bit FNV-1a hash of `bytes`, which names the icon file.
fn fnv1a(bytes: &[u8]) -> u32 {
    bytes.iter().fold(0x811c_9dc5, |hash, &byte| {
        (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193)
    })
}

/// Hands `toast` to Windows, which shows it in the corner of the screen, or only in the notification center when
/// "Do not disturb" is on or a full-screen program is in front. What becomes of it is reported to the resident window.
pub fn show(toast: &Toast) -> windows::core::Result<()> {
    let xml = XmlDocument::new()?;
    xml.LoadXml(&HSTRING::from(content(
        &toast.notification.text()?,
        &toast.launch,
    )))?;
    let shown = ToastNotification::CreateToastNotification(&xml)?;
    shown.SetTag(&HSTRING::from(toast.notification.key()))?;
    shown.SetGroup(&HSTRING::from(GROUP))?;

    // The events come on a thread of Windows', so they carry only what they need, and the time is read there: the
    // resident window's thread may be busy (a photo, Save As).
    let launch = toast.launch.clone();
    let showing = Instant::now();
    shown.Dismissed(&TypedEventHandler::new(
        move |_, args: Ref<ToastDismissedEventArgs>| {
            let after = showing.elapsed();
            let reason = match args.ok()?.Reason()? {
                ToastDismissalReason::UserCanceled => Dismissal::UserCanceled,
                ToastDismissalReason::ApplicationHidden => Dismissal::ApplicationHidden,
                ToastDismissalReason::TimedOut => Dismissal::TimedOut,
                // A reason newer than this build: nothing is learned from it.
                other => {
                    return Err(Error::new(
                        E_FAIL,
                        format!("notification dismissed for an unknown reason, {}", other.0),
                    ));
                }
            };
            report(Event {
                launch: launch.clone(),
                what: What::Dismissed { reason, after },
            })
        },
    ))?;
    let launch = toast.launch.clone();
    shown.Failed(&TypedEventHandler::new(
        move |_, args: Ref<ToastFailedEventArgs>| {
            let error = args.ok()?.ErrorCode()?;
            report(Event {
                launch: launch.clone(),
                what: What::Failed(Error::from(error).to_string()),
            })
        },
    ))?;

    ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(AUMID))?.Show(&shown)?;
    SHOWN.with_borrow_mut(|kept| {
        kept.retain(|(kind, _)| *kind != toast.notification);
        kept.push((toast.notification, shown));
    });
    Ok(())
}

/// Takes `notification` out of the notification center, if it is there.
pub fn remove(notification: Notification) -> windows::core::Result<()> {
    ToastNotificationManager::History()?.RemoveGroupedTagWithId(
        &HSTRING::from(notification.key()),
        &HSTRING::from(GROUP),
        &HSTRING::from(AUMID),
    )
}

/// The notification's XML: `text` alone, with no title, and `launch`, which its click hands the activator. ToastGeneric,
/// because the older templates never start the activator.
fn content(text: &str, launch: &str) -> String {
    format!(
        "<toast launch=\"{}\"><visual><binding template=\"ToastGeneric\"><text>{}</text></binding></visual></toast>",
        escape(launch),
        escape(text)
    )
}

/// `text` as XML text or attribute value.
fn escape(text: &str) -> String {
    text.chars()
        .fold(String::with_capacity(text.len()), |mut escaped, c| {
            match c {
                '&' => escaped.push_str("&amp;"),
                '<' => escaped.push_str("&lt;"),
                '>' => escaped.push_str("&gt;"),
                '"' => escaped.push_str("&quot;"),
                '\'' => escaped.push_str("&apos;"),
                c => escaped.push(c),
            }
            escaped
        })
}
