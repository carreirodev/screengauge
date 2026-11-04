//! The installer copy's Inno Setup script, `packaging/inno/ScreenGauge.iss` (WPC-337), held to the code and to the v1
//! spec (WPC-296): what the uninstaller deletes is exactly what the installer copy writes, and nothing of the Store
//! copy's.

use std::collections::HashMap;
use std::path::PathBuf;

use crate::notification::{INSTALLER_COPY_AUMID, INSTALLER_COPY_CLSID, STORE_COPY_CLSID};
use crate::{StartedBy, resident, settings_file, start_with_windows, started_by};

const SCRIPT: &str = include_str!("../../../packaging/inno/ScreenGauge.iss");

/// The installer copy's `AppId`, which names its uninstall entry,
/// `HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\{1DF55A92-C67C-41D9-B304-B2442423D407}_is1`. Never
/// changed: a new one would install a second ScreenGauge next to the first, and the Store copy looks for this entry
/// to tell whether the installer copy is installed too (WPC-339).
const APP_ID: &str = "{1DF55A92-C67C-41D9-B304-B2442423D407}";

/// The lines of `[name]`, without blank lines and comments.
fn section(name: &str) -> Vec<&'static str> {
    let header = format!("[{name}]");
    SCRIPT
        .lines()
        .map(str::trim)
        .skip_while(|line| !line.eq_ignore_ascii_case(&header))
        .skip(1)
        .take_while(|line| !line.starts_with('['))
        .filter(|line| !line.is_empty() && !line.starts_with(';'))
        .collect()
}

/// The value of `[Setup]`'s `directive`, which must be set once.
fn setup(directive: &str) -> String {
    let found: Vec<_> = section("Setup")
        .into_iter()
        .filter_map(|line| line.split_once('='))
        .filter(|(name, _)| name.trim().eq_ignore_ascii_case(directive))
        .map(|(_, value)| value.trim().to_string())
        .collect();
    assert_eq!(found.len(), 1, "[Setup] {directive} is {found:?}");
    found[0].clone()
}

/// The value of the preprocessor's `#define name "value"`.
fn define(name: &str) -> String {
    SCRIPT
        .lines()
        .filter_map(|line| line.trim().strip_prefix("#define "))
        .filter_map(|rest| rest.trim().split_once(' '))
        .find(|(defined, _)| *defined == name)
        .and_then(|(_, value)| value.trim().strip_prefix('"')?.strip_suffix('"'))
        .unwrap_or_else(|| panic!("no #define {name} \"...\""))
        .to_string()
}

/// Every entry of `[name]`, as its parameters (`Name: "..."; Flags: ...`) by their lowercase name, quotes taken off.
fn entries(name: &str) -> Vec<HashMap<String, String>> {
    section(name).into_iter().map(parameters).collect()
}

fn parameters(line: &str) -> HashMap<String, String> {
    let mut found = HashMap::new();
    let mut rest = line;
    while !rest.is_empty() {
        let (name, after) = rest
            .split_once(':')
            .unwrap_or_else(|| panic!("no parameter name in {rest:?}"));
        let after = after.trim_start();
        let (value, after) = match after.strip_prefix('"') {
            // A quoted value; "" stands for one quote inside it.
            Some(quoted) => {
                let mut value = String::new();
                let mut chars = quoted.char_indices().peekable();
                let end = loop {
                    let (index, c) = chars
                        .next()
                        .unwrap_or_else(|| panic!("unclosed quote in {line:?}"));
                    if c != '"' {
                        value.push(c);
                    } else if chars.peek().map(|(_, next)| *next) == Some('"') {
                        chars.next();
                        value.push('"');
                    } else {
                        break index + 1;
                    }
                };
                (value, &quoted[end..])
            }
            None => {
                let end = after.find(';').unwrap_or(after.len());
                (after[..end].trim().to_string(), &after[end..])
            }
        };
        found.insert(name.trim().to_lowercase(), value);
        rest = after.trim_start().trim_start_matches(';').trim_start();
    }
    found
}

/// The flags of an entry, lowercase.
fn flags(entry: &HashMap<String, String>) -> Vec<String> {
    entry
        .get("flags")
        .map(|flags| flags.split_whitespace().map(str::to_lowercase).collect())
        .unwrap_or_default()
}

/// Whether `name` matches `pattern`, whose `*` stands for any run of characters, ignoring case as Windows does.
fn matches(pattern: &str, name: &str) -> bool {
    let (pattern, name) = (pattern.to_lowercase(), name.to_lowercase());
    let mut parts = pattern.split('*');
    let first = parts.next().unwrap_or_default();
    let Some(mut rest) = name.strip_prefix(first) else {
        return false;
    };
    let mut parts: Vec<_> = parts.collect();
    let Some(last) = parts.pop() else {
        return rest.is_empty();
    };
    for part in parts {
        match rest.find(part) {
            Some(at) => rest = &rest[at + part.len()..],
            None => return false,
        }
    }
    rest.len() >= last.len() && rest.ends_with(last)
}

/// The text of a constant wide string of the code.
fn text(string: windows::core::PCWSTR) -> String {
    // SAFETY: the code's constants come from `w!`, null-terminated and valid for the whole run.
    unsafe { string.to_string() }.expect("UTF-16 text")
}

/// The registry entries the uninstaller deletes and the installer never creates: on uninstall, the whole key, or one
/// value.
fn deleted_at_uninstall() -> Vec<HashMap<String, String>> {
    entries("Registry")
        .into_iter()
        .filter(|entry| {
            let flags = flags(entry);
            assert!(
                flags.contains(&"dontcreatekey".to_string()),
                "the installer creates {entry:?}"
            );
            flags.contains(&"uninsdeletekey".to_string())
                || flags.contains(&"uninsdeletevalue".to_string())
        })
        .collect()
}

#[test]
fn the_app_id_is_the_one_the_store_copy_looks_for() {
    // "{{" is Inno Setup's escape for a brace that begins the value.
    assert_eq!(setup("AppId"), format!("{{{APP_ID}"));
}

#[test]
fn it_installs_for_the_user_alone_without_asking_for_an_administrator() {
    assert_eq!(setup("PrivilegesRequired"), "lowest");
    // Allowing an override would offer an install for every user, which asks for an administrator.
    assert!(
        section("Setup").iter().all(|line| !line
            .to_lowercase()
            .starts_with("privilegesrequiredoverridesallowed")),
        "{:?}",
        section("Setup")
    );
    // {autopf} is %LOCALAPPDATA%\Programs without an administrator: a folder of its own, never the settings folder.
    assert_eq!(setup("DefaultDirName"), r"{autopf}\ScreenGauge");
    assert_eq!(setup("DisableDirPage"), "yes");
}

#[test]
fn only_windows_11_on_x64() {
    // Build 22000 is Windows 11's first, as in the Store copy's manifest.
    assert_eq!(setup("MinVersion"), "10.0.22000");
    // x64compatible is x64 and, through Windows' emulation, Windows 11 on Arm (v1 spec WPC-296, Platform).
    assert_eq!(setup("ArchitecturesAllowed"), "x64compatible");
    // The 64-bit view of the registry, where ScreenGauge writes: HKCU\Software\Classes\CLSID has a 32-bit one too.
    assert_eq!(setup("ArchitecturesInstallIn64BitMode"), "x64compatible");
    // What Setup says below MinVersion's build, in English as everything ScreenGauge shows (WPC-293).
    assert!(
        section("Messages")
            .contains(&"WindowsVersionNotSupported=ScreenGauge requires Windows 11 or later."),
        "{:?}",
        section("Messages")
    );
}

#[test]
fn installed_apps_lists_screengauge_by_wpc_solutions_with_its_icon() {
    assert_eq!(setup("AppName"), "ScreenGauge");
    // Inno Setup's default would add the version to the name.
    assert_eq!(setup("UninstallDisplayName"), "ScreenGauge");
    assert_eq!(setup("UninstallDisplayIcon"), r"{app}\ScreenGauge.exe");
    // The publisher and the version are the exe's version resource's (WPC-304), which make-installer.ps1 checks.
    assert_eq!(setup("AppPublisher"), "{#Publisher}");
    assert_eq!(setup("AppVersion"), "{#AppVersion}");
}

#[test]
fn the_exe_is_the_one_file_installed() {
    let files = entries("Files");
    assert_eq!(files.len(), 1, "{files:?}");
    assert_eq!(files[0]["source"], "{#ScreenGaugeExe}");
    assert_eq!(files[0]["destdir"], "{app}");
}

#[test]
fn the_start_menu_entry_opens_screengauge_as_a_call() {
    let icons = entries("Icons");
    assert_eq!(icons.len(), 1, "{icons:?}");
    let icon = &icons[0];
    assert_eq!(icon["name"], r"{autoprograms}\ScreenGauge");
    assert_eq!(icon["filename"], r"{app}\ScreenGauge.exe");
    // The registry alone names the notifications' AppUserModelID (ADR 0003): the shortcut carries none.
    assert!(!icon.contains_key("appusermodelid"), "{icon:?}");
    let parameters = icon.get("parameters").cloned().unwrap_or_default();
    let line = std::iter::once("ScreenGauge.exe".to_string())
        .chain(parameters.split_whitespace().map(str::to_string));
    assert_eq!(started_by(line), StartedBy::User);
}

#[test]
fn finish_opens_screengauge_as_the_start_menu_does_unless_the_install_is_silent() {
    let runs = entries("Run");
    assert_eq!(runs.len(), 1, "{runs:?}");
    let run = &runs[0];
    assert_eq!(run["filename"], r"{app}\ScreenGauge.exe");
    assert_eq!(run["description"], "Launch ScreenGauge");
    assert!(!run.contains_key("parameters"), "{run:?}");
    let flags = flags(run);
    for flag in ["postinstall", "skipifsilent", "nowait"] {
        assert!(flags.contains(&flag.to_string()), "{flag} missing: {run:?}");
    }
    // Checked, as the owner chose (WPC-337).
    assert!(!flags.contains(&"unchecked".to_string()), "{run:?}");
}

#[test]
fn uninstalling_deletes_the_notifications_registration_of_the_installer_copy() {
    let deleted: Vec<_> = deleted_at_uninstall()
        .into_iter()
        .filter(|entry| flags(entry).contains(&"uninsdeletekey".to_string()))
        // "{{" is Inno Setup's escape for a brace, which would begin a constant.
        .map(|entry| (entry["root"].clone(), entry["subkey"].replace("{{", "{")))
        .collect();
    let expected = [
        format!(r"Software\Classes\AppUserModelId\{INSTALLER_COPY_AUMID}"),
        format!(r"Software\Classes\CLSID\{{{INSTALLER_COPY_CLSID:?}}}"),
    ];
    for key in expected {
        assert!(
            deleted
                .iter()
                .any(|(root, subkey)| root == "HKCU64" && subkey.eq_ignore_ascii_case(&key)),
            "{key} isn't deleted: {deleted:?}"
        );
    }
    assert_eq!(deleted.len(), 2, "{deleted:?}");
}

#[test]
fn uninstalling_takes_screengauge_off_the_sign_in_list_and_leaves_the_user_s_choice_to_windows() {
    let values: Vec<_> = deleted_at_uninstall()
        .into_iter()
        .filter(|entry| flags(entry).contains(&"uninsdeletevalue".to_string()))
        .collect();
    assert_eq!(values.len(), 1, "{values:?}");
    let run = &values[0];
    assert_eq!(run["root"], "HKCU64");
    assert_eq!(run["subkey"], text(start_with_windows::WINDOWS.run));
    assert_eq!(run["valuename"], text(start_with_windows::WINDOWS.name));
    // StartupApproved is Windows' (ADR 0004).
    let registry = entries("Registry");
    assert!(
        registry
            .iter()
            .all(|entry| !entry["subkey"].contains("StartupApproved")),
        "{registry:?}"
    );
}

#[test]
fn nothing_of_the_store_copy_s_is_named() {
    let script = SCRIPT.to_lowercase();
    assert!(!script.contains(&format!("{STORE_COPY_CLSID:?}").to_lowercase()));
    // The Store copy's settings are in its package's folder, which Windows deletes with the package.
    assert!(!script.contains("packages"));
}

/// A file named by the code, and the folder it is in, as `[UninstallDelete]` writes them.
fn in_settings_folder(name: &str) -> String {
    let local_app_data = PathBuf::from(std::env::var_os("LOCALAPPDATA").expect("LOCALAPPDATA"));
    let folder = settings_file::installer_copy_folder().expect("the folder");
    let relative = folder
        .strip_prefix(&local_app_data)
        .expect("the folder is in %LOCALAPPDATA%");
    format!(r"{{localappdata}}\{}\{name}", relative.display())
}

#[test]
fn uninstalling_deletes_every_file_screengauge_writes_in_its_folder_and_then_the_folder() {
    let deleted = entries("UninstallDelete");
    let files: Vec<_> = deleted
        .iter()
        .filter(|entry| entry["type"] == "files")
        .map(|entry| entry["name"].clone())
        .collect();
    let written = [
        settings_file::FILE_NAME,
        settings_file::TEMPORARY_NAME,
        // A damaged file kept aside, and the next one.
        "settings.damaged.ini",
        "settings.damaged-2.ini",
        // The header icon of the notifications, named by a hash of its contents, and the file written before it.
        "notification-icon-4be52f28.png",
        "notification-icon-4be52f28.png.tmp",
    ];
    for name in written {
        let path = in_settings_folder(name);
        assert!(
            files.iter().any(|pattern| matches(pattern, &path)),
            "{path} isn't deleted: {files:?}"
        );
    }
    // Only what ScreenGauge writes: a Snip saved there by the user stays, and the folder with it.
    for name in ["screengauge-20261104-101500.png", "notes.txt"] {
        let path = in_settings_folder(name);
        assert!(
            !files.iter().any(|pattern| matches(pattern, &path)),
            "{path} would be deleted: {files:?}"
        );
    }
    let folder = in_settings_folder("");
    let folder = folder.trim_end_matches('\\');
    assert!(
        deleted
            .iter()
            .any(|entry| entry["type"] == "dirifempty" && entry["name"] == folder),
        "{folder} isn't deleted once empty: {deleted:?}"
    );
    assert!(
        deleted
            .iter()
            .all(|entry| entry["type"] != "filesandordirs"),
        "{deleted:?}"
    );
}

#[test]
fn screengauge_running_from_the_installed_folder_is_closed_by_its_window_class() {
    assert_eq!(define("ResidentClass"), text(resident::CLASS_NAME));
    // The single-instance name is shared with the Store copy, which must keep running: never AppMutex.
    assert!(
        section("Setup")
            .iter()
            .all(|line| !line.to_lowercase().starts_with("appmutex"))
    );
}

#[test]
fn the_pattern_matcher_matches_as_windows_does() {
    assert!(matches(r"C:\A\settings*.ini", r"c:\a\settings.ini"));
    assert!(matches(
        r"C:\A\settings*.ini",
        r"C:\A\settings.damaged-2.ini"
    ));
    assert!(!matches(r"C:\A\settings*.ini", r"C:\A\settings.ini.tmp"));
    assert!(matches(r"C:\A\n-*.png", r"C:\A\n-1.png"));
    assert!(!matches(r"C:\A\n-*.png", r"C:\A\n-1.png.tmp"));
    assert!(matches(r"C:\A\x", r"C:\A\x"));
    assert!(!matches(r"C:\A\x", r"C:\A\xy"));
}
