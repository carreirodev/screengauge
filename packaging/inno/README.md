# The installer copy's installer

The installer copy of ScreenGauge is this Inno Setup installer, downloaded from GitHub Releases (WPC-337). It holds the same `ScreenGauge.exe` as the Store copy's package, with the same version. It installs for the signed-in user alone, with no administrator:

- in `%LOCALAPPDATA%\Programs\ScreenGauge`, with no folder to choose, so it is never the settings folder;
- with a Start menu entry, ScreenGauge, which opens it as a call (WPC-301), and an entry in Settings > Apps > Installed apps, named ScreenGauge, published by WPC Solutions, with the exe's four-part version, as the Store copy's package shows it;
- on Windows 11 and later only, x64, which Windows 11 on Arm runs through its emulation. There is no portable `.exe`, and v1 is unsigned.

Its uninstall entry is `HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\{1DF55A92-C67C-41D9-B304-B2442423D407}_is1`, from the script's `AppId`, which never changes: the Store copy looks for it (WPC-339).

Uninstalling deletes what the installer copy wrote, and nothing of the Store copy's:

- its settings in `%LOCALAPPDATA%\ScreenGauge`: `settings.ini`, the temporary file a save writes, the damaged files kept aside, and the notifications' header icons (`notification-icon-*.png`), then the folder once empty. Any other file in it stays, a Snip the user saved there included;
- in HKCU, the notifications' registration (ADR 0003): `Software\Classes\AppUserModelId\WPCSolutions.ScreenGauge` and `Software\Classes\CLSID\{3BEB9695-DA23-4599-B10C-7CD6CD96E50C}`;
- the `ScreenGauge` value of `Software\Microsoft\Windows\CurrentVersion\Run`, "Start with Windows" (ADR 0004). The value of the same name in `Explorer\StartupApproved\Run` is Windows' and stays.

The installer never creates these: ScreenGauge writes them as it runs.

Installing over a ScreenGauge that runs, or uninstalling it, first asks it to exit, as its tray menu's Exit does, and only when it runs from the installed folder: a Store copy that runs is left running. Setup's `AppMutex` isn't used, because the two copies claim the same single-instance name.

| File | What it does |
| --- | --- |
| `ScreenGauge.iss` | The installer: for the user alone, Windows 11 and x64, the Start menu entry, "Launch ScreenGauge" on the last page (checked, skipped in a silent install), and what the uninstaller deletes |
| `make-installer.ps1` | Builds `ScreenGauge.exe` in release and compiles `target\installer\ScreenGauge-<version>-setup.exe`; refuses an exe whose version isn't the app's and an installer over 5 MB |

The tests in `crates/screengauge/src/installer.rs` hold the script to the code: what the uninstaller deletes is the code's AppUserModelID, class, `Run` value and settings files, the Start menu entry starts ScreenGauge as a call, and the script names nothing of the Store copy's.

## Install Inno Setup

Once per computer, Inno Setup 7, for the user alone (choose "Install for me only"):

```
winget install --id JRSoftware.InnoSetup.7 -e -s winget -i
```

## Build the installer

In PowerShell 7, from the repository's root:

```
pwsh -File packaging\inno\make-installer.ps1
```

## Install, silently too

Run the installer, or install with no window at all:

```
target\installer\ScreenGauge-0.1.0-setup.exe /VERYSILENT
```

A silent install never opens ScreenGauge at the end.

## Uninstall

From Settings > Apps > Installed apps, ScreenGauge, Uninstall. Or with no window:

```
& "$env:LOCALAPPDATA\Programs\ScreenGauge\unins000.exe" /VERYSILENT
```

## Sources

Inno Setup's documentation, read on 2026-11-04:

- Version 7.1.1, the current stable, and its winget ID: https://jrsoftware.org/isdl.php
- What changed in 7 (`SetupArchitecture`, `AppVerName`): https://jrsoftware.org/files/is7-whatsnew.htm
- `[Registry]`, its 64-bit roots and the `dontcreatekey` flag: https://jrsoftware.org/ishelp/topic_registrysection.htm
- `MinVersion`: https://jrsoftware.org/ishelp/topic_setup_minversion.htm
- `CloseApplications`: https://jrsoftware.org/ishelp/topic_setup_closeapplications.htm
