#Requires -Version 7.4
<#
.SYNOPSIS
Builds the installer copy's installer, target\installer\ScreenGauge-<version>-setup.exe (WPC-337).

.DESCRIPTION
Builds ScreenGauge.exe in release and compiles ScreenGauge.iss with Inno Setup's compiler (ISCC), which reads the
version and the publisher from the exe's version resource and refuses an exe whose version isn't the app's.

It refuses an installer over 5 MB (v1 spec WPC-296). The installer is unsigned in v1.

.PARAMETER Iscc
Path of ISCC.exe. By default, Inno Setup 7's, installed for the user or for every user.
#>
[CmdletBinding()]
param(
    [string] $Iscc
)

$ErrorActionPreference = 'Stop'
# A native command that exits non-zero throws, like a cmdlet error (PowerShell 7.4+).
$PSNativeCommandUseErrorActionPreference = $true
Set-StrictMode -Version Latest

# Each download is at most 5 MB, read as decimal megabytes, the stricter reading (as crates\screengauge\tests\exe.rs).
$SizeLimitBytes = 5000000

$repo = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)

if (-not $Iscc) {
    $candidates = @(
        (Join-Path $env:LOCALAPPDATA 'Programs\Inno Setup 7\ISCC.exe'),
        (Join-Path $env:ProgramFiles 'Inno Setup 7\ISCC.exe'),
        (Join-Path ${env:ProgramFiles(x86)} 'Inno Setup 7\ISCC.exe')
    )
    $Iscc = $candidates | Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
    if (-not $Iscc) {
        throw "Inno Setup 7 isn't installed (looked for $($candidates -join ', ')). See README.md, or pass -Iscc."
    }
}
if (-not (Test-Path -LiteralPath $Iscc)) {
    throw "Not found: $Iscc."
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
$target = $meta.target_directory
$exe = Join-Path $target 'release\ScreenGauge.exe'
$outputDir = Join-Path $target 'installer'
$installer = Join-Path $outputDir "ScreenGauge-$appVersion-setup.exe"

Write-Host "== ISCC: $installer"
& $Iscc /Qp "/DScreenGaugeExe=$exe" "/DCargoVersion=$appVersion" "/O$outputDir" (Join-Path $PSScriptRoot 'ScreenGauge.iss')

$size = (Get-Item -LiteralPath $installer).Length
if ($size -gt $SizeLimitBytes) {
    throw "$installer is $size bytes, over the $SizeLimitBytes-byte limit."
}
Write-Host "   $size bytes, within the $SizeLimitBytes-byte limit"
Write-Host ''
Write-Host "UNSIGNED, as v1 ships it: $installer"
