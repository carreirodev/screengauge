# The Store copy's MSIX package

The Microsoft Store copy of ScreenGauge is this MSIX package (WPC-334). It holds the same `ScreenGauge.exe` as the installer copy, with the same version. Inside a package, ScreenGauge knows it is the Store copy and:

- keeps its settings file in the package's own folder, `%LOCALAPPDATA%\Packages\WPCSolutions.ScreenGauge_8rc0q4yanedb4\LocalState`, which Windows deletes when the package is uninstalled. It never uses the installer copy's `%LOCALAPPDATA%\ScreenGauge`: Windows would let the package open and change a file already there (WPC-289);
- writes nothing in the registry for its Windows notifications. The manifest declares their activator, with a class of the Store copy's own, and their header shows the package's name and icon.

| File | What it does |
| --- | --- |
| `AppxManifest.xml` | The package: identity from Partner Center, Windows 11 and x64 only, `runFullTrust`, the Start menu entry, the images of `assets/package`, and the notifications' activator |
| `make-msix.ps1` | Builds `ScreenGauge.exe` in release and packs `target\msix\ScreenGauge.msix`; refuses a version other than the app's and a package over 5 MB |
| `new-test-cert.ps1` | Creates the test certificate that lets the package be installed on this computer |

The tests in `crates/screengauge/src/package.rs` hold the manifest to the code: its version is Cargo's, its activator class is the code's, and every image it names is in `assets/package`.

Run the commands below in PowerShell 7, from the repository's root. They need the Windows SDK (MakeAppx, MakePri, SignTool); the script finds the newest one installed.

## Build the package for the Store

```
pwsh -File packaging\msix\make-msix.ps1
```

This leaves `target\msix\ScreenGauge.msix` unsigned, as Partner Center takes it: Microsoft signs it when it is submitted.

## Install a test build on this computer

Windows installs only a signed package, so a test build is signed with a certificate of our own whose subject is the manifest's Publisher.

1. Create the certificate. It prints the thumbprint and the exact commands of the next steps.

```
pwsh -File packaging\msix\new-test-cert.ps1
```

2. In a PowerShell opened with "Run as administrator", trust the certificate, with the `.cer` path the first step printed:

```
Import-Certificate -FilePath <path>\target\msix\ScreenGauge-test.cer -CertStoreLocation Cert:\LocalMachine\TrustedPeople
```

3. Build the package signed with it:

```
pwsh -File packaging\msix\make-msix.ps1 -CertThumbprint <thumbprint>
```

4. Install it:

```
Add-AppxPackage -Path target\msix\ScreenGauge.msix
```

ScreenGauge is then in the Start menu. Only one copy runs at a time: exit the installer copy first, or opening the Store copy only calls it.

## Uninstall

Exit ScreenGauge first (the tray icon's menu, Exit). Uninstalling deletes the Store copy's settings.

```
Get-AppxPackage WPCSolutions.ScreenGauge | Remove-AppxPackage
```

## Remove the test certificate

As administrator:

```
Get-ChildItem Cert:\LocalMachine\TrustedPeople | Where-Object Subject -EQ 'CN=A42AF38D-FCA3-4813-A9BB-788209C42998' | Remove-Item
```

In a normal PowerShell, which deletes the private key too:

```
Get-ChildItem Cert:\CurrentUser\My | Where-Object Subject -EQ 'CN=A42AF38D-FCA3-4813-A9BB-788209C42998' | ForEach-Object { Remove-Item -LiteralPath "Cert:\CurrentUser\My\$($_.Thumbprint)" -DeleteKey }
```

## Sources

Microsoft Learn, read on 2026-11-03:

- The manifest, MakePri and `resources.pri`: https://learn.microsoft.com/en-us/windows/msix/desktop/desktop-to-uwp-manual-conversion
- `com:ExeServer`, whose `Arguments` are the `LocalServer32` command line's: https://learn.microsoft.com/en-us/uwp/schemas/appxpackage/uapmanifestschema/element-com-exeserver
- `desktop:ToastNotificationActivation`: https://learn.microsoft.com/en-us/uwp/schemas/appxpackage/uapmanifestschema/element-desktop-toastnotificationactivation
- The images' names: "Construct your Windows app's icon" (WPC-304's note on WPC-334)
- Test certificate and the Trusted People store: https://learn.microsoft.com/en-us/windows/msix/package/create-certificate-package-signing
- Packing: https://learn.microsoft.com/en-us/windows/msix/package/create-app-package-with-makeappx-tool
