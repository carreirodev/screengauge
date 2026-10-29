#Requires -Version 7.4
<#
.SYNOPSIS
One-off script for the WPC-289 hardware test: creates the self-signed certificate that signs hwtest.msix.

.DESCRIPTION
Creates a code-signing certificate whose subject equals the Publisher in AppxManifest.xml, in
Cert:\CurrentUser\My (no elevation needed), valid for three months, and exports its public part to
target\msix\hwtest-test.cer. It trusts nothing by itself: the last step it prints is the one elevated
command that makes Windows trust the certificate, plus the commands that remove it afterwards.
Run by the owner, never by an agent. See README.md in this folder.
#>
[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Set-StrictMode -Version Latest

$crate = Split-Path -Parent $PSScriptRoot
[xml] $manifest = Get-Content -LiteralPath (Join-Path $PSScriptRoot 'AppxManifest.xml') -Raw
$publisher = $manifest.Package.Identity.Publisher
Push-Location -LiteralPath $crate
try {
    $meta = cargo metadata --format-version 1 --no-deps | ConvertFrom-Json
}
finally {
    Pop-Location
}
$msixDir = Join-Path $meta.target_directory 'msix'
$cerPath = Join-Path $msixDir 'hwtest-test.cer'

$existing = @(Get-ChildItem -LiteralPath 'Cert:\CurrentUser\My' | Where-Object Subject -EQ $publisher)
if ($existing.Count -gt 0) {
    $cert = $existing[0]
    Write-Host "A certificate for '$publisher' already exists in Cert:\CurrentUser\My; reusing it, not creating another."
}
else {
    # The parameters Learn gives for MSIX signing: code-signing EKU (1.3.6.1.5.5.7.3.3), end entity (not a CA).
    $cert = New-SelfSignedCertificate -Type Custom -KeyUsage DigitalSignature `
        -CertStoreLocation 'Cert:\CurrentUser\My' `
        -TextExtension @('2.5.29.37={text}1.3.6.1.5.5.7.3.3', '2.5.29.19={text}') `
        -Subject $publisher -FriendlyName 'ScreenGauge hwtest MSIX test signing' `
        -NotAfter (Get-Date).AddMonths(3)
    Write-Host "Created a certificate for '$publisher' in Cert:\CurrentUser\My."
}
$thumb = $cert.Thumbprint

New-Item -ItemType Directory -Path $msixDir -Force | Out-Null
Export-Certificate -Cert $cert -FilePath $cerPath -Type CERT | Out-Null

Write-Host ''
Write-Host "Thumbprint: $thumb"
Write-Host "Public certificate: $cerPath"
Write-Host ''
Write-Host 'Next, sign the package (normal PowerShell):'
Write-Host "  pwsh -File `"$(Join-Path $PSScriptRoot 'make-msix.ps1')`" -CertThumbprint $thumb"
Write-Host ''
Write-Host 'Then the one elevated step: in a PowerShell opened with "Run as administrator", trust the certificate:'
Write-Host "  Import-Certificate -FilePath `"$cerPath`" -CertStoreLocation Cert:\LocalMachine\TrustedPeople"
Write-Host ''
Write-Host 'After the test, remove it again. Elevated:'
Write-Host "  Remove-Item -LiteralPath Cert:\LocalMachine\TrustedPeople\$thumb"
Write-Host 'Normal PowerShell (also deletes the private key):'
Write-Host "  Remove-Item -LiteralPath Cert:\CurrentUser\My\$thumb -DeleteKey"
