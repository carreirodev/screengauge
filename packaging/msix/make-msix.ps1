#Requires -Version 7.4
<#
.SYNOPSIS
Builds the Store copy's MSIX package, target\msix\ScreenGauge.msix (WPC-334).

.DESCRIPTION
Builds ScreenGauge.exe in release, stages it with AppxManifest.xml and the images of assets\package in
target\msix\layout, writes the resource index (resources.pri) that lets Windows pick an image by scale and size, and
packs the package with MakeAppx, which checks the manifest against the schema.

It refuses a manifest whose version isn't the app's, and a package over 5 MB (v1 spec WPC-296).

The package is left unsigned, as Partner Center takes it: Microsoft signs it when it is submitted to the Store. With
-CertThumbprint, it is signed with a test certificate (new-test-cert.ps1), so it can be installed on this computer.

.PARAMETER CertThumbprint
Thumbprint of a code-signing certificate in Cert:\CurrentUser\My whose subject is the manifest's Publisher.

.PARAMETER SdkBin
Folder with makeappx.exe, makepri.exe and signtool.exe. By default, the newest Windows SDK's x64 folder.
#>
[CmdletBinding()]
param(
    [string] $CertThumbprint,
    [string] $SdkBin
)

$ErrorActionPreference = 'Stop'
# A native command that exits non-zero throws, like a cmdlet error (PowerShell 7.4+).
$PSNativeCommandUseErrorActionPreference = $true
Set-StrictMode -Version Latest

# Each download is at most 5 MB, read as decimal megabytes, the stricter reading (as crates\screengauge\tests\exe.rs).
$SizeLimitBytes = 5000000

$repo = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)

if (-not $SdkBin) {
    $kits = 'C:\Program Files (x86)\Windows Kits\10\bin'
    $newest = Get-ChildItem -LiteralPath $kits -Directory -Filter '10.*' -ErrorAction SilentlyContinue |
        Where-Object { Test-Path -LiteralPath (Join-Path $_.FullName 'x64\makeappx.exe') } |
        Sort-Object { [version] $_.Name } -Descending |
        Select-Object -First 1
    if (-not $newest) {
        throw "No Windows SDK with x64\makeappx.exe under $kits. Install the Windows SDK, or pass -SdkBin."
    }
    $SdkBin = Join-Path $newest.FullName 'x64'
}
$makeappx = Join-Path $SdkBin 'makeappx.exe'
$makepri = Join-Path $SdkBin 'makepri.exe'
$signtool = Join-Path $SdkBin 'signtool.exe'
foreach ($tool in $makeappx, $makepri, $signtool) {
    if (-not (Test-Path -LiteralPath $tool)) {
        throw "Not found: $tool. Pass -SdkBin with a Windows SDK's bin\<version>\x64 folder."
    }
}

# From the repository's root, so cargo reads .cargo\config.toml (the static C runtime, ADR 0001).
Push-Location -LiteralPath $repo
try {
    Write-Host '== cargo build --release'
    cargo build --release --package screengauge
    $meta = cargo metadata --format-version 1 --no-deps | ConvertFrom-Json
}
finally {
    Pop-Location
}

$appVersion = ($meta.packages | Where-Object name -EQ 'screengauge').version
$manifestPath = Join-Path $PSScriptRoot 'AppxManifest.xml'
[xml] $manifest = Get-Content -LiteralPath $manifestPath -Raw
$packageVersion = $manifest.Package.Identity.Version
# The Store copy and the installer copy have the same version; the Store keeps the fourth part for itself.
if ($packageVersion -ne "$appVersion.0") {
    throw "AppxManifest.xml has Version $packageVersion, but the app is $appVersion. Set it to $appVersion.0."
}

$target = $meta.target_directory
$exe = Join-Path $target 'release\ScreenGauge.exe'
$msixDir = Join-Path $target 'msix'
$layout = Join-Path $msixDir 'layout'
$assets = Join-Path $layout 'Assets'
$priConfig = Join-Path $msixDir 'priconfig.xml'
$msix = Join-Path $msixDir 'ScreenGauge.msix'

Write-Host "== staging $layout"
if (Test-Path -LiteralPath $layout) {
    Remove-Item -LiteralPath $layout -Recurse -Force
}
New-Item -ItemType Directory -Path $assets -Force | Out-Null
Copy-Item -LiteralPath $exe -Destination $layout
Copy-Item -LiteralPath $manifestPath -Destination $layout
Copy-Item -Path (Join-Path $repo 'assets\package\*.png') -Destination $assets

# The images are named by scale and size (AppList.scale-100.png, AppList.targetsize-16.png...), and the manifest names
# only AppList.png: the resource index tells Windows which file each name and size goes to. The configuration stays
# out of the layout, so it isn't packed.
Write-Host '== makepri: resources.pri'
& $makepri createconfig /cf $priConfig /dq en-US /pv 10.0.0 /o
# The default configuration splits the images by scale into resources.scale-*.pri, for resource packages of their own
# in a bundle. ScreenGauge is one package, so every scale goes in the one resources.pri.
[xml] $config = Get-Content -LiteralPath $priConfig -Raw
$packaging = $config.resources.SelectSingleNode('packaging')
if ($packaging) {
    [void] $config.resources.RemoveChild($packaging)
}
$config.Save($priConfig)
& $makepri new /pr $layout /cf $priConfig /mn (Join-Path $layout 'AppxManifest.xml') /of (Join-Path $layout 'resources.pri') /o

Write-Host "== makeappx pack: $msix"
& $makeappx pack /o /h SHA256 /d $layout /p $msix

if ($CertThumbprint) {
    Write-Host "== signtool: signing with Cert:\CurrentUser\My\$CertThumbprint"
    & $signtool sign /fd SHA256 /sha1 $CertThumbprint $msix
}

# Measured last, so a signed package is measured with its signature.
$size = (Get-Item -LiteralPath $msix).Length
if ($size -gt $SizeLimitBytes) {
    throw "$msix is $size bytes, over the $SizeLimitBytes-byte limit."
}
Write-Host "   $size bytes, within the $SizeLimitBytes-byte limit"

Write-Host ''
if ($CertThumbprint) {
    Write-Host "SIGNED with the test certificate: $msix"
}
else {
    Write-Host "UNSIGNED, as Partner Center takes it: $msix"
    Write-Host 'To install it on this computer, sign it with a test certificate: see README.md.'
}
