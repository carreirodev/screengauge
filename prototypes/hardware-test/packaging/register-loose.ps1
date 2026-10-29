#Requires -Version 7.4
<#
.SYNOPSIS
One-off script for the WPC-289 hardware test: registers the unpacked layout, no certificate, no admin.

.DESCRIPTION
Registers target\msix\layout\AppxManifest.xml (staged by make-msix.ps1) with Add-AppxPackage -Register,
the Developer Mode path: the app gets package identity and runs from the layout folder, unsigned.
Run by the owner, never by an agent. See README.md in this folder.
#>
[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Set-StrictMode -Version Latest

$crate = Split-Path -Parent $PSScriptRoot
Push-Location -LiteralPath $crate
try {
    $meta = cargo metadata --format-version 1 --no-deps | ConvertFrom-Json
}
finally {
    Pop-Location
}
$manifest = Join-Path $meta.target_directory 'msix\layout\AppxManifest.xml'
if (-not (Test-Path -LiteralPath $manifest)) {
    throw "Not found: $manifest. Run make-msix.ps1 first (no certificate needed)."
}

# Developer Mode is this registry value (Learn, "Settings for developers"); reading it needs no admin.
$unlockKey = 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\AppModelUnlock'
$devMode = $false
if (Test-Path -LiteralPath $unlockKey) {
    $devMode = (Get-Item -LiteralPath $unlockKey).GetValue('AllowDevelopmentWithoutDevLicense') -eq 1
}
if (-not $devMode) {
    throw 'Developer Mode looks off (AllowDevelopmentWithoutDevLicense is not 1). Turn it on in Settings > System > Advanced > For developers.'
}

Write-Host "== Add-AppxPackage -Register $manifest"
Add-AppxPackage -Register $manifest
Get-AppxPackage -Name 'WPCSolutions.ScreenGaugeHwtest' | Format-List Name, PackageFullName, PackageFamilyName, InstallLocation, IsDevelopmentMode

Write-Host 'Registered. Start it from the Start menu (ScreenGauge hwtest), or from a NEW terminal:'
Write-Host '  screengauge-hwtest.exe package info'
Write-Host 'Uninstall:'
Write-Host '  Get-AppxPackage WPCSolutions.ScreenGaugeHwtest | Remove-AppxPackage'
