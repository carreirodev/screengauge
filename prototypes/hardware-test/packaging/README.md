# hwtest as an MSIX package

One-off packaging for the WPC-289 hardware test: it installs `hwtest.exe` as a packaged desktop app, so it can be checked against what the future Microsoft Store copy of ScreenGauge will do (package identity, startup task, AppData redirection, capture border). The identity, `WPCSolutions.ScreenGaugeHwtest` from publisher `CN=ScreenGauge hwtest`, is test-only and never the real app's.

| File | What it does |
| --- | --- |
| `AppxManifest.xml` | Full-trust app, startup task `HwtestStartup` (off), alias `screengauge-hwtest.exe`, capabilities `runFullTrust`, `graphicsCaptureWithoutBorder`, `graphicsCaptureProgrammatic` |
| `make-msix.ps1` | Builds, stages `target\msix\layout`, packs `target\msix\hwtest.msix`, signs only when given a certificate |
| `new-test-cert.ps1` | Creates the self-signed test certificate and prints the one elevated step |
| `register-loose.ps1` | Registers the staged layout in Developer Mode: no certificate, no admin |

Run every command below in PowerShell 7, from `prototypes\hardware-test`, unless it says "elevated".

## Build the package

```
pwsh -File packaging\make-msix.ps1
```

Without a certificate this leaves an unsigned `target\msix\hwtest.msix` and the staged layout. makeappx checks the manifest against the schema while packing.

The two install paths below share one identity, so only one can be installed at a time: uninstall one before trying the other.

## Path A: Developer Mode, no certificate, no admin

Registers the staged folder directly. The app runs from `target\msix\layout` with package identity, unsigned. Uninstall it before running `make-msix.ps1` again, since that script replaces the folder.

```
pwsh -File packaging\register-loose.ps1
```

## Path B: signed MSIX, like a real install

Closer to the Store copy: Windows copies the package to `C:\Program Files\WindowsApps` and locks it. It needs a test certificate and one elevated step.

1. Create the certificate (in `Cert:\CurrentUser\My`, valid three months). It prints the thumbprint and the exact commands for the next steps.

```
pwsh -File packaging\new-test-cert.ps1
```

2. Sign the package, with the thumbprint the previous step printed.

```
pwsh -File packaging\make-msix.ps1 -CertThumbprint <thumbprint>
```

3. Elevated (PowerShell opened with "Run as administrator"): trust the certificate, using the full `.cer` path the first step printed.

```
Import-Certificate -FilePath <full path>\target\msix\hwtest-test.cer -CertStoreLocation Cert:\LocalMachine\TrustedPeople
```

4. Install.

```
Add-AppxPackage -Path target\msix\hwtest.msix
```

## Run the packaged copy

From the Start menu, "ScreenGauge hwtest" starts with no arguments, so the packaged copy runs `run --hotkey win+ctrl+shift+m --exit-after-secs 3600` (the startup task launches it the same way). Right-click the tray icon to exit.

From a terminal opened after the install, the alias starts the packaged copy with arguments and with package identity:

```
screengauge-hwtest.exe package info
```

```
screengauge-hwtest.exe capture --hold-secs 3
```

The packaged copy writes its logs to `%USERPROFILE%\hwtest-out` (unless `HWTEST_OUT` is set), a folder Windows does not redirect, so they can be read from outside. Each log says which packaged default it applied.

### The `package` mode

| Command | Checks |
| --- | --- |
| `package info` | Package identity, full and family name, the exe path, the log folder, `LOCALAPPDATA`, `TEMP`, and the `ApplicationData` `LocalFolder` and `LocalCacheFolder` |
| `package startup` | The `HwtestStartup` task and its state; `--enable` calls `RequestEnableAsync`, `--disable` calls `Disable()` |
| `package settings` | Appends a line to `%LOCALAPPDATA%\ScreenGauge-hwtest\settings.txt` (`--name FILE` for another file), and logs the path used and the real path behind the open handle |

For the startup test, enable the task, sign out and back in, and look for a new `run-*.log` in `%USERPROFILE%\hwtest-out`:

```
screengauge-hwtest.exe package startup --enable
```

```
screengauge-hwtest.exe package startup --disable
```

For the AppData test, `settings.txt` must not exist before the packaged copy's first run. After `screengauge-hwtest.exe package settings`, check from a normal terminal whether the real folder got the file:

```
Test-Path "$env:LOCALAPPDATA\ScreenGauge-hwtest\settings.txt"
```

## Where the package keeps its data

The package's private folder is `%LOCALAPPDATA%\Packages\<package family name>`. The family name is `WPCSolutions.ScreenGaugeHwtest_` plus a hash of the publisher; `package info` logs it, and so does this:

```
(Get-AppxPackage WPCSolutions.ScreenGaugeHwtest).PackageFamilyName
```

`LocalState` there is `ApplicationData.LocalFolder`, and `LocalCache` is `ApplicationData.LocalCacheFolder`. What Learn says about AppData for a packaged desktop app on Windows 10 1903 and later:

- A file or folder the app newly creates under `AppData` goes to a private per-user, per-package location, and the app still sees it at the real path. Learn does not name that location; it is commonly `LocalCache\Local\...` and `LocalCache\Roaming\...` inside the package folder (unconfirmed, which `package settings` checks).
- Opening a file looks in the private location first, then in the real `AppData`. A file that already existed in the real `AppData` is opened and changed there, not redirected.
- Uninstalling removes the redirected files. Files changed in the real `AppData` stay.

## Uninstall

Exit the resident copy first (right-click the tray icon). Works for both paths:

```
Get-AppxPackage WPCSolutions.ScreenGaugeHwtest | Remove-AppxPackage
```

## Remove the test certificate

Elevated:

```
Get-ChildItem Cert:\LocalMachine\TrustedPeople | Where-Object Subject -EQ 'CN=ScreenGauge hwtest' | Remove-Item
```

Normal PowerShell, which also deletes the private key:

```
Get-ChildItem Cert:\CurrentUser\My | Where-Object Subject -EQ 'CN=ScreenGauge hwtest' | ForEach-Object { Remove-Item -LiteralPath "Cert:\CurrentUser\My\$($_.Thumbprint)" -DeleteKey }
```

## Sources

- Manifest template and `Add-AppxPackage -Register`: https://learn.microsoft.com/en-us/windows/msix/desktop/desktop-to-uwp-manual-conversion
- Startup task and alias for packaged desktop apps: https://learn.microsoft.com/en-us/windows/apps/desktop/modernize/desktop-to-uwp-extensions
- `StartupTask`, and `RequestEnableAsync` showing no dialog for a packaged desktop app: https://learn.microsoft.com/en-us/uwp/api/windows.applicationmodel.startuptask
- Capabilities: https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/app-capability-declarations and https://learn.microsoft.com/en-us/uwp/schemas/appxpackage/uapmanifestschema/element-uap11-capability
- AppData redirection and uninstall: https://learn.microsoft.com/en-us/windows/msix/desktop/desktop-to-uwp-behind-the-scenes and https://learn.microsoft.com/en-us/windows/msix/desktop/flexible-virtualization
- Test certificate and the Trusted People store: https://learn.microsoft.com/en-us/windows/msix/package/create-certificate-package-signing
- Signing: https://learn.microsoft.com/en-us/windows/msix/package/sign-app-package-using-signtool
- Packing: https://learn.microsoft.com/en-us/windows/msix/package/create-app-package-with-makeappx-tool
