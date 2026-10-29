#Requires -Version 7.4
<#
.SYNOPSIS
One-off script for the WPC-289 hardware test, not part of any build: packs hwtest.exe into an MSIX.

.DESCRIPTION
Builds hwtest in release, stages hwtest.exe, AppxManifest.xml and placeholder logos into
target\msix\layout, and packs target\msix\hwtest.msix with makeappx (which validates the manifest).
Signs the package only when given a certificate; otherwise leaves it unsigned and says so.
Installs nothing. See README.md in this folder.

.PARAMETER CertThumbprint
Thumbprint of a code-signing certificate in Cert:\CurrentUser\My (new-test-cert.ps1 prints it).

.PARAMETER PfxPath
A .pfx file to sign with instead.

.PARAMETER PfxPassword
The .pfx password, if it has one.

.PARAMETER SdkBin
Folder with makeappx.exe and signtool.exe.
#>
[CmdletBinding(DefaultParameterSetName = 'Unsigned')]
param(
    [Parameter(ParameterSetName = 'Store', Mandatory)]
    [string] $CertThumbprint,

    [Parameter(ParameterSetName = 'Pfx', Mandatory)]
    [string] $PfxPath,

    [Parameter(ParameterSetName = 'Pfx')]
    [securestring] $PfxPassword,

    [string] $SdkBin = 'C:\Program Files (x86)\Windows Kits\10\bin\10.0.26100.0\x64'
)

$ErrorActionPreference = 'Stop'
# A native command that exits non-zero throws, like a cmdlet error (PowerShell 7.4+).
$PSNativeCommandUseErrorActionPreference = $true
Set-StrictMode -Version Latest

$crate = Split-Path -Parent $PSScriptRoot
$makeappx = Join-Path $SdkBin 'makeappx.exe'
$signtool = Join-Path $SdkBin 'signtool.exe'
foreach ($tool in $makeappx, $signtool) {
    if (-not (Test-Path -LiteralPath $tool)) {
        throw "Not found: $tool. Pass -SdkBin with the Windows SDK bin\<version>\x64 folder."
    }
}

Write-Host '== cargo build --release'
# Run from the crate folder: cargo reads .cargo\config.toml (the static C runtime) from the current
# directory, not from --manifest-path.
Push-Location -LiteralPath $crate
try {
    cargo build --release
    $meta = cargo metadata --format-version 1 --no-deps | ConvertFrom-Json
}
finally {
    Pop-Location
}
$target = $meta.target_directory
$exe = Join-Path $target 'release\hwtest.exe'
if (-not (Test-Path -LiteralPath $exe)) {
    throw "cargo build finished but $exe is missing"
}

$msixDir = Join-Path $target 'msix'
$layout = Join-Path $msixDir 'layout'
$assets = Join-Path $layout 'Assets'
$msix = Join-Path $msixDir 'hwtest.msix'

Write-Host "== staging $layout"
# A loose registration (register-loose.ps1) runs from this folder: uninstall it first, or this fails on locked files.
if (Test-Path -LiteralPath $layout) {
    Remove-Item -LiteralPath $layout -Recurse -Force
}
New-Item -ItemType Directory -Path $assets -Force | Out-Null
Copy-Item -LiteralPath $exe -Destination $layout
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'AppxManifest.xml') -Destination $layout

# Placeholder logos at the sizes the manifest names (scale-100), generated so the repo stays text-only.
Add-Type -AssemblyName System.Drawing
$logos = [ordered]@{ 'StoreLogo.png' = 50; 'Square150x150Logo.png' = 150; 'Square44x44Logo.png' = 44 }
foreach ($name in $logos.Keys) {
    $size = $logos[$name]
    $bitmap = [System.Drawing.Bitmap]::new($size, $size)
    try {
        $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
        try {
            $graphics.Clear([System.Drawing.Color]::FromArgb(255, 0, 120, 212))
        }
        finally {
            $graphics.Dispose()
        }
        $bitmap.Save((Join-Path $assets $name), [System.Drawing.Imaging.ImageFormat]::Png)
    }
    finally {
        $bitmap.Dispose()
    }
}
Get-ChildItem -LiteralPath $layout -Recurse -File | ForEach-Object { Write-Host "   $($_.FullName.Substring($layout.Length + 1)) ($($_.Length) bytes)" }

Write-Host "== makeappx pack -> $msix"
& $makeappx pack /o /h SHA256 /d $layout /p $msix

switch ($PSCmdlet.ParameterSetName) {
    'Store' {
        Write-Host "== signtool sign with Cert:\CurrentUser\My\$CertThumbprint"
        & $signtool sign /fd SHA256 /sha1 $CertThumbprint $msix
    }
    'Pfx' {
        Write-Host "== signtool sign with $PfxPath"
        if ($PfxPassword) {
            # signtool only takes the password on its command line; acceptable for a throwaway test certificate.
            $plain = [System.Net.NetworkCredential]::new('', $PfxPassword).Password
            & $signtool sign /fd SHA256 /a /f $PfxPath /p $plain $msix
        }
        else {
            & $signtool sign /fd SHA256 /a /f $PfxPath $msix
        }
    }
    default {
        Write-Host ''
        Write-Host "UNSIGNED: $msix"
        Write-Host 'Windows will not install it as is. Either register the loose layout (register-loose.ps1, Developer'
        Write-Host 'Mode, no certificate), or create a test certificate (new-test-cert.ps1) and run this script again'
        Write-Host 'with -CertThumbprint. See README.md.'
        return
    }
}
Write-Host ''
Write-Host "SIGNED: $msix"
