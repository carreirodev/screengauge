//! The installer copy's winget manifests, `packaging/winget/manifests` (WPC-396), held to the installer they describe:
//! `packaging/inno/ScreenGauge.iss`, the release it is downloaded from, and the Store copy's package name, which is the
//! package identifier winget users type. Each version's folder is laid out as in microsoft/winget-pkgs, where it is
//! submitted as it is. An older version's files are what was submitted then, so only the newest is held to the files
//! of today: the script, the Store page and the READMEs.

use std::fs;
use std::path::{Path, PathBuf};

use crate::installer::{self, APP_ID};
use crate::package::manifest;

/// The schema every file declares: the one microsoft/winget-pkgs's pull request template asks for (read 2026-11-07).
const MANIFEST_VERSION: &str = "1.28.0";

/// The publisher the script requires of the exe, and the one winget shows.
const PUBLISHER: &str = "WPC Solutions";

/// The package's folder, under the first letter of its publisher, as in microsoft/winget-pkgs.
fn package_folder() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../packaging/winget/manifests/w/WPCSolutions/ScreenGauge")
}

/// One version's three manifests, by the names winget-pkgs requires: the version's, the installer's and the default
/// locale's.
struct VersionManifests {
    /// The version, which is the folder's name.
    version: String,
    version_manifest: String,
    installer: String,
    locale: String,
}

/// Every version in the package's folder, oldest first.
fn every_version() -> Vec<VersionManifests> {
    let mut found = Vec::new();
    for entry in fs::read_dir(package_folder())
        .expect("packaging/winget/manifests/w/WPCSolutions/ScreenGauge")
    {
        let folder = entry.expect("a folder entry").path();
        let version = folder
            .file_name()
            .and_then(|name| name.to_str())
            .expect("a version folder")
            .to_string();
        let read = |suffix: &str| {
            let file = folder.join(format!("{}{suffix}.yaml", manifest::NAME));
            fs::read_to_string(&file).unwrap_or_else(|error| panic!("{}: {error}", file.display()))
        };
        found.push(VersionManifests {
            version_manifest: read(""),
            installer: read(".installer"),
            locale: read(".locale.en-US"),
            version,
        });
    }
    found.sort_by_key(|manifests| version_numbers(&manifests.version));
    found
}

/// The numbers of a version such as 1.0.0, so that 1.10.0 comes after 1.9.0.
fn version_numbers(version: &str) -> Vec<u32> {
    version
        .split('.')
        .map(|number| {
            number
                .parse()
                .unwrap_or_else(|_| panic!("{version} is not a version"))
        })
        .collect()
}

/// The newest version, the one of today's installer.
fn newest_version() -> VersionManifests {
    every_version()
        .pop()
        .unwrap_or_else(|| panic!("no version in {}", package_folder().display()))
}

/// The values of every `key: value` line of a manifest, at any depth and in list items too, quotes taken off.
fn values(manifest: &str, key: &str) -> Vec<String> {
    manifest
        .lines()
        .map(|line| line.trim_start().trim_start_matches("- "))
        .filter_map(|line| line.split_once(':'))
        .filter(|(name, _)| *name == key)
        .map(|(_, value)| {
            value
                .trim()
                .trim_matches(|c| c == '"' || c == '\'')
                .to_string()
        })
        .collect()
}

/// The value of `key`, which the manifest must give once.
fn value(manifest: &str, key: &str) -> String {
    let found = values(manifest, key);
    assert_eq!(found.len(), 1, "{key} is {found:?}");
    found[0].clone()
}

/// The value of `key` at the manifest's top level, outside every list and map, which the manifest must give once.
fn top_level_value(manifest: &str, key: &str) -> String {
    let found: Vec<_> = manifest
        .lines()
        .filter_map(|line| line.strip_prefix(key)?.strip_prefix(':'))
        .collect();
    assert_eq!(found.len(), 1, "{key} at the top level is {found:?}");
    found[0].trim().trim_matches('\'').to_string()
}

#[test]
fn winget_users_type_the_store_copy_s_package_name_and_each_file_says_its_version() {
    for manifests in every_version() {
        for (kind, file) in [
            ("version", &manifests.version_manifest),
            ("installer", &manifests.installer),
            ("defaultLocale", &manifests.locale),
        ] {
            assert_eq!(value(file, "PackageIdentifier"), manifest::NAME);
            assert_eq!(value(file, "PackageVersion"), manifests.version);
            assert_eq!(value(file, "ManifestType"), kind);
            assert_eq!(value(file, "ManifestVersion"), MANIFEST_VERSION);
            // The header winget-pkgs's validation requires, naming the same schema.
            let schema = format!(
                "# yaml-language-server: $schema=https://aka.ms/winget-manifest.{kind}.{MANIFEST_VERSION}.schema.json"
            );
            assert_eq!(file.lines().next(), Some(schema.as_str()));
        }
        assert_eq!(value(&manifests.version_manifest, "DefaultLocale"), "en-US");
        assert_eq!(value(&manifests.locale, "PackageLocale"), "en-US");
    }
}

#[test]
fn it_installs_the_installer_copy_for_the_user_alone_on_windows_11_x64() {
    let installer = newest_version().installer;
    // inno, not exe: winget then passes Inno Setup's own silent switches.
    assert_eq!(value(&installer, "InstallerType"), "inno");
    assert_eq!(installer::setup("PrivilegesRequired"), "lowest");
    assert_eq!(value(&installer, "Scope"), "user");
    // A 32-bit Setup that installs a 64-bit ScreenGauge is x64 to winget (winget-pkgs's Authoring.md).
    assert_eq!(installer::setup("ArchitecturesAllowed"), "x64compatible");
    assert_eq!(value(&installer, "Architecture"), "x64");
    assert_eq!(
        value(&installer, "MinimumOSVersion"),
        format!("{}.0", installer::setup("MinVersion"))
    );
    // Setup never asks for an administrator, so winget needn't either, nor refuse to run elevated.
    assert!(values(&installer, "ElevationRequirement").is_empty());
}

#[test]
fn winget_tells_the_installer_copy_by_its_entry_in_installed_apps() {
    let VersionManifests {
        version,
        installer,
        locale,
        ..
    } = newest_version();
    // Inno Setup names the uninstall entry after AppId, with _is1 after it: the installer's product code, and the
    // entry's, its one other ProductCode.
    let product_code = format!("{APP_ID}_is1");
    assert_eq!(top_level_value(&installer, "ProductCode"), product_code);
    assert_eq!(
        values(&installer, "ProductCode"),
        [product_code.clone(), product_code]
    );
    assert_eq!(
        value(&installer, "DisplayName"),
        installer::setup("UninstallDisplayName")
    );
    // The script refuses to build an installer whose exe is published by anyone else.
    assert!(installer::SCRIPT.contains(&format!(r#"#if Publisher != "{PUBLISHER}""#)));
    assert_eq!(value(&installer, "Publisher"), PUBLISHER);
    assert_eq!(value(&locale, "Publisher"), PUBLISHER);
    // Installed apps shows the exe's four-part version, the app's with a fourth part of 0, which the script enforces.
    // Without it, winget would read 1.0.0.0 as a version other than 1.0.0 and offer it as an upgrade.
    assert!(installer::SCRIPT.contains(r#"#if AppVersion != CargoVersion + ".0""#));
    assert_eq!(value(&installer, "DisplayVersion"), format!("{version}.0"));
}

#[test]
fn the_installer_is_downloaded_from_the_version_s_release_on_github() {
    let repository = env!("CARGO_PKG_REPOSITORY");
    let VersionManifests {
        version,
        installer,
        locale,
        ..
    } = newest_version();
    // The name the script gives the installer, for this version.
    let name = installer::setup("OutputBaseFilename").replace("{#CargoVersion}", &version);
    assert_eq!(
        value(&installer, "InstallerUrl"),
        format!("{repository}/releases/download/v{version}/{name}.exe")
    );
    // The published installer's, read from the release: builds aren't reproducible, so a local build has another.
    let sha256 = value(&installer, "InstallerSha256");
    assert!(
        sha256.len() == 64
            && sha256
                .chars()
                .all(|c| c.is_ascii_digit() || c.is_ascii_uppercase()),
        "{sha256}"
    );
    assert_eq!(
        value(&locale, "ReleaseNotesUrl"),
        format!("{repository}/releases/tag/v{version}")
    );
}

#[test]
fn the_package_page_says_what_the_store_page_and_the_readmes_say() {
    const LISTING: &str = include_str!("../../../packaging/store/listing.md");
    const README: &str = include_str!("../../../README.md");
    const README_PT_BR: &str = include_str!("../../../README.pt-BR.md");
    let locale = newest_version().locale;
    assert_eq!(value(&locale, "PackageName"), "ScreenGauge");
    let short_description = value(&locale, "ShortDescription");
    assert!(
        LISTING.contains(&format!("\n{short_description}\n")),
        "{short_description}"
    );
    // winget's limit (manifest.defaultLocale.1.28.0.json).
    assert!(short_description.chars().count() <= 256);
    assert_eq!(value(&locale, "License"), env!("CARGO_PKG_LICENSE"));
    assert_eq!(value(&locale, "PackageUrl"), env!("CARGO_PKG_REPOSITORY"));
    let privacy = value(&locale, "PrivacyUrl");
    assert!(README.contains(&format!("({privacy})")), "{privacy}");
    assert_eq!(value(&locale, "Moniker"), "screengauge");
    // The command both READMEs give is this package's.
    for readme in [README, README_PT_BR] {
        assert!(readme.contains(&format!("\nwinget install {}\n", manifest::NAME)));
    }
}
