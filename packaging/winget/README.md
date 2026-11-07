# The installer copy on winget

`winget install WPCSolutions.ScreenGauge` installs the installer copy (WPC-396): winget downloads the version's
`ScreenGauge-<version>-setup.exe` from GitHub Releases, checks its SHA-256, and runs it. The package identifier is the
Store copy's package name, `WPCSolutions.ScreenGauge`, but the package is not the Store copy: that one is winget's
`msstore` source, `winget install --id 9PGNSRZ2BPJ2 --source msstore`, and a community manifest can no longer point to
the Store.

`manifests` holds each version's manifests as they were submitted to winget's community repository,
[microsoft/winget-pkgs](https://github.com/microsoft/winget-pkgs), at the same path:
`manifests/w/WPCSolutions/ScreenGauge/<version>/`. Each version has three files, in schema 1.28.0:

| File | What it says |
| --- | --- |
| `WPCSolutions.ScreenGauge.yaml` | The package and version, and that the default locale is `en-US` |
| `WPCSolutions.ScreenGauge.installer.yaml` | The installer: Inno Setup, for the user alone, x64, Windows 11 (`10.0.22000.0`) and later, its URL and SHA-256, and the entry it writes in Installed apps |
| `WPCSolutions.ScreenGauge.locale.en-US.yaml` | The package's page: name, publisher, license, descriptions, links, moniker `screengauge` and tags |

The tests in `crates/screengauge/src/winget.rs` check that every version's files name the package, the Store copy
manifest's name, and their own version. They hold the newest version's files to the installer they describe, since an
older version's are what was submitted then: the product code is the Inno Setup script's `AppId` followed by `_is1`,
the scope, architecture and minimum Windows are the script's, the URL is the version's release with the name the
script gives the installer, the version in Installed apps is the version with a fourth part of 0, and the short
description and privacy policy are the Store page's and the README's. The SHA-256 is the published installer's: builds
aren't reproducible, so it is read from the release, never from a local build.

## How winget runs the installer

winget passes Inno Setup's own switches (winget-cli's `ManifestCommon.cpp`):

| Command | Switches | The box when the Store copy is installed |
| --- | --- | --- |
| `winget install WPCSolutions.ScreenGauge` | `/SP- /SILENT /SUPPRESSMSGBOXES /NORESTART` | Not shown: Setup's log says the Store copy was found |
| `winget install WPCSolutions.ScreenGauge --silent` | `/SP- /VERYSILENT /SUPPRESSMSGBOXES /NORESTART` | Not shown, as above |
| `winget install WPCSolutions.ScreenGauge -i` | none: the installer's windows | Shown, with Cancel the default |

Setup closes a ScreenGauge that runs from the installed folder before installing over it, so `winget upgrade` needs
nothing more. Canceling the box ends Setup with exit code 1, which winget reports as a failed install: it reads only 2
and 5 as canceled by the user.

## Check a version's manifests

With winget, from the repository's root:

```
winget validate --manifest packaging\winget\manifests\w\WPCSolutions\ScreenGauge\1.0.0
```

The SHA-256 of the published installer, to compare with `InstallerSha256`:

```
gh release download v1.0.0 -p ScreenGauge-1.0.0-setup.exe -O - | sha256sum
```

## Submit a version

One pull request per version to microsoft/winget-pkgs, with only that version's folder, from a fork under the account
that signed Microsoft's Contributor License Agreement (https://cla.opensource.microsoft.com). Its validation downloads
the installer, scans it, and installs it as a standard user; a moderator then reviews it by hand. There is no stated
time for that review: a new package waits longest, and once merged it reaches winget within about an hour.

## Sources

Read on 2026-11-07, at microsoft/winget-pkgs commit 6922bc0 and microsoft/winget-cli commit afc9a47:

- The pull request template, which asks for schema 1.28 and the CLA:
  https://github.com/microsoft/winget-pkgs/blob/master/.github/PULL_REQUEST_TEMPLATE.md
- Multi-file manifests only, and `winget validate` before submitting:
  https://github.com/microsoft/winget-pkgs/blob/master/doc/FirstContribution.md
- The folder path, and x64 for an x86 installer that installs x64 binaries:
  https://github.com/microsoft/winget-pkgs/blob/master/doc/Authoring.md
- The validation's steps: URL, scans, install as a standard user, and `inno` rather than `exe`:
  https://github.com/microsoft/winget-pkgs/blob/master/doc/Validation.md
- Moderation, with no stated review time: https://github.com/microsoft/winget-pkgs/blob/master/doc/Moderation.md
- A merged package reaching winget within about an hour: https://github.com/microsoft/winget-pkgs/blob/master/doc/FAQ.md
- Policies: only MSIX installers must be signed; 1.5.1 and 1.5.2 on personal information:
  https://github.com/microsoft/winget-pkgs/blob/master/doc/Policies.md and
  https://learn.microsoft.com/en-us/windows/package-manager/package/windows-package-manager-policies
- The 1.28.0 fields and their limits: https://github.com/microsoft/winget-pkgs/tree/master/doc/manifest/schema/1.28.0
  and https://github.com/microsoft/winget-cli/tree/master/schemas/JSON/manifests/v1.28.0, whose installer types have
  no `msstore`
- Inno Setup's switches and the exit codes winget reads as canceled, `GetDefaultKnownSwitches` and
  `GetDefaultKnownReturnCodes`:
  https://github.com/microsoft/winget-cli/blob/master/src/AppInstallerCommonCore/Manifest/ManifestCommon.cpp
- Which switch each mode uses, silent with progress by default:
  https://github.com/microsoft/winget-cli/blob/master/src/AppInstallerCLICore/Workflows/ShellExecuteInstallerHandler.cpp
- `_is1` after an Inno Setup `AppId` as the product code, in a published manifest:
  https://github.com/microsoft/winget-pkgs/tree/master/manifests/w/WinSCP/WinSCP/6.7
- Setup's exit code 1 when `InitializeSetup` returns False:
  https://github.com/jrsoftware/issrc/blob/main/Projects/Src/Setup.MainFunc.pas
