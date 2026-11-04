//! Which copy of ScreenGauge this process is: the Store copy, which runs from its MSIX package and so has the package's
//! identity, or the installer copy, which has none (WPC-334). Both are the same `.exe`, built once; only Windows knows
//! which one started.

use std::path::PathBuf;

use windows::ApplicationModel::Activation::ActivationKind;
use windows::ApplicationModel::AppInstance;
use windows::Storage::ApplicationData;
use windows::Win32::Foundation::{
    APPMODEL_ERROR_NO_PACKAGE, E_FAIL, ERROR_INSUFFICIENT_BUFFER, ERROR_SUCCESS, WIN32_ERROR,
};
use windows::Win32::Storage::Packaging::Appx::{
    GetCurrentApplicationUserModelId, GetCurrentPackageFullName,
};
use windows::core::{Error, PWSTR};

/// A copy of ScreenGauge, as GLOSSARY.md names them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppCopy {
    /// Installed from the Microsoft Store: the process has the package's identity.
    Store,
    /// Installed by the installer from GitHub: the process has no package identity.
    Installer,
}

/// Which copy this process is, asked of Windows. An error says neither: the caller must not take it for the installer
/// copy, or the Store copy would use the installer copy's folder.
pub fn this_copy() -> windows::core::Result<AppCopy> {
    let mut length = 0;
    // SAFETY: a size query, with no buffer: the call only writes `length`.
    match unsafe { GetCurrentPackageFullName(&mut length, None) } {
        APPMODEL_ERROR_NO_PACKAGE => Ok(AppCopy::Installer),
        ERROR_INSUFFICIENT_BUFFER => Ok(AppCopy::Store),
        error => Err(Error::from(error.to_hresult())),
    }
}

/// How Windows started the Store copy, as from the Start menu (`Launch`) or by its startup task (`StartupTask`); `None`
/// in the installer copy, or when Windows doesn't say. Windows answers only once per process: it is asked at startup.
pub fn activation_kind() -> windows::core::Result<Option<ActivationKind>> {
    if this_copy()? == AppCopy::Installer {
        return Ok(None);
    }
    match AppInstance::GetActivatedEventArgs() {
        Ok(arguments) => arguments.Kind().map(Some),
        // A null answer, which the windows crate gives as an error with no failure code.
        Err(e) if e.code().is_ok() => Ok(None),
        Err(e) => Err(e),
    }
}

/// The package's own folder for its data, `LocalState`, which Windows deletes when the package is uninstalled. Only
/// the Store copy has one: in the installer copy, Windows answers with an error.
pub fn local_folder() -> windows::core::Result<PathBuf> {
    let path = ApplicationData::Current()?.LocalFolder()?.Path()?;
    Ok(PathBuf::from(path.to_os_string()))
}

/// The AppUserModelID Windows gave the package's app, `<package family name>!<app ID>`, which its notifications go by.
/// Only the Store copy has one.
pub fn application_user_model_id() -> windows::core::Result<String> {
    package_string(GetCurrentApplicationUserModelId)
}

/// One of the `GetCurrentPackage...` strings: asks for its length, and then for the string.
fn package_string(
    get: unsafe fn(*mut u32, Option<PWSTR>) -> WIN32_ERROR,
) -> windows::core::Result<String> {
    let mut length = 0;
    // SAFETY: a size query, with no buffer: the call only writes `length`.
    match unsafe { get(&mut length, None) } {
        ERROR_INSUFFICIENT_BUFFER => {}
        error => return Err(Error::from(error.to_hresult())),
    }
    let mut buffer = vec![0u16; usize::try_from(length)?];
    // SAFETY: `buffer` holds the `length` UTF-16 units the size query asked for, and outlives the call.
    match unsafe { get(&mut length, Some(PWSTR(buffer.as_mut_ptr()))) } {
        ERROR_SUCCESS => {}
        error => return Err(Error::from(error.to_hresult())),
    }
    // `length` counts the terminating null.
    buffer.truncate(usize::try_from(length)?.saturating_sub(1));
    String::from_utf16(&buffer).map_err(|e| Error::new(E_FAIL, e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_process_without_a_package_is_the_installer_copy() {
        // The tests run from the build folder, not from a package.
        assert_eq!(this_copy().expect("Windows answers"), AppCopy::Installer);
    }

    #[test]
    fn without_a_package_windows_is_not_asked_how_it_started_screengauge() {
        assert_eq!(activation_kind().expect("Windows answers"), None);
    }

    #[test]
    fn without_a_package_there_is_no_package_folder_nor_package_aumid() {
        assert!(local_folder().is_err());
        assert!(application_user_model_id().is_err());
    }
}

/// The Store copy's manifest, `packaging/msix/AppxManifest.xml`, held to the code and to the v1 spec (WPC-296).
#[cfg(test)]
pub(crate) mod manifest {
    use std::path::Path;

    use windows::Data::Xml::Dom::XmlDocument;
    use windows::core::HSTRING;

    use crate::notification::STORE_COPY_CLSID;
    use crate::{StartedBy, start_with_windows, started_by};

    const MANIFEST: &str = include_str!("../../../packaging/msix/AppxManifest.xml");

    /// The package's name and publisher, as Partner Center gives them for the WPC Solutions account (Product
    /// identity). Windows names the package family after them, which the installer looks for (WPC-338).
    pub(crate) const NAME: &str = "WPCSolutions.ScreenGauge";
    pub(crate) const PUBLISHER: &str = "CN=A42AF38D-FCA3-4813-A9BB-788209C42998";

    fn manifest() -> XmlDocument {
        let document = XmlDocument::new().expect("an XML document");
        document
            .LoadXml(&HSTRING::from(MANIFEST))
            .expect("the manifest is well-formed XML");
        document
    }

    /// The text of every node `path` finds, an XPath that names elements by their local name only, so it needs no
    /// namespace prefixes.
    fn all(path: &str) -> Vec<String> {
        let nodes = manifest()
            .SelectNodes(&HSTRING::from(path))
            .expect("the path is valid XPath");
        (0..nodes.Length().expect("a node count"))
            .map(|index| {
                nodes
                    .Item(index)
                    .and_then(|node| node.InnerText())
                    .expect("the node's text")
                    .to_string()
            })
            .collect()
    }

    /// The text of the one node `path` finds.
    fn one(path: &str) -> String {
        let found = all(path);
        assert_eq!(found.len(), 1, "{path} finds {found:?}");
        found[0].clone()
    }

    #[test]
    fn the_identity_is_the_one_partner_center_gives() {
        assert_eq!(one("//*[local-name()='Identity']/@Name"), NAME);
        assert_eq!(one("//*[local-name()='Identity']/@Publisher"), PUBLISHER);
        assert_eq!(
            one("//*[local-name()='PublisherDisplayName']"),
            "WPC Solutions"
        );
        assert_eq!(
            one("//*[local-name()='Properties']/*[local-name()='DisplayName']"),
            "ScreenGauge"
        );
    }

    #[test]
    fn the_store_copy_has_the_installer_copy_s_version() {
        // The exe's version resource, which the installer copy will carry, comes from Cargo too.
        assert_eq!(
            one("//*[local-name()='Identity']/@Version"),
            format!("{}.0", env!("CARGO_PKG_VERSION"))
        );
    }

    #[test]
    fn only_windows_11_on_x64() {
        assert_eq!(
            one("//*[local-name()='Identity']/@ProcessorArchitecture"),
            "x64"
        );
        assert_eq!(
            one("//*[local-name()='TargetDeviceFamily']/@Name"),
            "Windows.Desktop"
        );
        // Build 22000 is Windows 11's first.
        assert_eq!(
            one("//*[local-name()='TargetDeviceFamily']/@MinVersion"),
            "10.0.22000.0"
        );
    }

    #[test]
    fn full_trust_is_the_only_capability() {
        assert_eq!(
            all("//*[local-name()='Capabilities']/*/@Name"),
            ["runFullTrust"]
        );
    }

    #[test]
    fn the_start_menu_entry_starts_screengauge() {
        assert_eq!(
            one("//*[local-name()='Application']/@Executable"),
            "ScreenGauge.exe"
        );
        assert_eq!(
            one("//*[local-name()='VisualElements']/@DisplayName"),
            "ScreenGauge"
        );
        // AppListEntry="none" would keep it out of the Start menu.
        assert!(all("//*[local-name()='VisualElements']/@AppListEntry").is_empty());
    }

    #[test]
    fn the_activator_is_the_store_copy_s_class() {
        let class = format!("{STORE_COPY_CLSID:?}").to_lowercase();
        assert_eq!(
            one("//*[local-name()='ExeServer']/*[local-name()='Class']/@Id"),
            class
        );
        assert_eq!(
            one("//*[local-name()='ToastNotificationActivation']/@ToastActivatorCLSID"),
            class
        );
    }

    #[test]
    fn a_click_with_screengauge_closed_starts_it_as_a_click() {
        let executable = one("//*[local-name()='ExeServer']/@Executable");
        assert_eq!(executable, "ScreenGauge.exe");
        // COM starts the executable with the arguments, and may add -Embedding after them.
        let arguments = one("//*[local-name()='ExeServer']/@Arguments");
        let line = std::iter::once(executable)
            .chain(arguments.split_whitespace().map(str::to_string))
            .chain(["-Embedding".to_string()]);
        assert_eq!(started_by(line), StartedBy::Click);
    }

    /// The `windows.startupTask` extension, the Store copy's "Start with Windows" (WPC-336).
    const STARTUP_TASK: &str = "//*[local-name()='Extension'][@Category='windows.startupTask']";

    #[test]
    fn the_startup_task_is_the_code_s_and_off_until_the_user_turns_it_on() {
        assert_eq!(
            one(&format!(
                "{STARTUP_TASK}/*[local-name()='StartupTask']/@TaskId"
            )),
            start_with_windows::STARTUP_TASK_ID
        );
        assert_eq!(
            one(&format!(
                "{STARTUP_TASK}/*[local-name()='StartupTask']/@Enabled"
            )),
            "false"
        );
        // Task Manager and Settings > Apps > Startup list it by this name.
        assert_eq!(
            one(&format!(
                "{STARTUP_TASK}/*[local-name()='StartupTask']/@DisplayName"
            )),
            "ScreenGauge"
        );
        assert_eq!(
            one(&format!("{STARTUP_TASK}/@EntryPoint")),
            "Windows.FullTrustApplication"
        );
    }

    #[test]
    fn the_startup_task_starts_screengauge_as_at_sign_in() {
        let executable = one(&format!("{STARTUP_TASK}/@Executable"));
        assert_eq!(executable, "ScreenGauge.exe");
        let parameters = one(&format!("{STARTUP_TASK}/@*[local-name()='Parameters']"));
        let line =
            std::iter::once(executable).chain(parameters.split_whitespace().map(str::to_string));
        assert_eq!(started_by(line), StartedBy::SignIn);
    }

    #[test]
    fn every_image_named_is_in_assets_package() {
        let package = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/package");
        let images = [
            one("//*[local-name()='Properties']/*[local-name()='Logo']"),
            one("//*[local-name()='VisualElements']/@Square150x150Logo"),
            one("//*[local-name()='VisualElements']/@Square44x44Logo"),
        ];
        for image in images {
            // Assets\AppList.png names AppList.scale-100.png and its other scales, which make-msix.ps1 copies to
            // Assets\ and the resource index (resources.pri) lists.
            let name = image
                .strip_prefix(r"Assets\")
                .and_then(|name| name.strip_suffix(".png"))
                .unwrap_or_else(|| panic!("{image} isn't a PNG in Assets\\"));
            for scale in [100, 125, 150, 200, 400] {
                let file = package.join(format!("{name}.scale-{scale}.png"));
                assert!(file.is_file(), "{} is missing", file.display());
            }
        }
    }
}
