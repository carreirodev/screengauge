#Requires -Version 7.4
<#
.SYNOPSIS
Creates the self-signed certificate that signs a test build of the Store copy's package (WPC-334).

.DESCRIPTION
Creates a code-signing certificate whose subject is the Publisher in AppxManifest.xml, in Cert:\CurrentUser\My (no
administrator needed), valid for three months, and exports its public part to target\msix\ScreenGauge-test.cer. It
trusts nothing by itself: it prints the one command, run as administrator, that makes Windows trust the certificate,
and the commands that remove it afterwards. Run by the owner. See README.md.

The Store never sees this certificate: Microsoft signs the package that is submitted to it.
#>
[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Set-StrictMode -Version Latest

$repo = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
[xml] $manifest = Get-Content -LiteralPath (Join-Path $PSScriptRoot 'AppxManifest.xml') -Raw
$publisher = $manifest.Package.Identity.Publisher
Push-Location -LiteralPath $repo
try {
    $meta = cargo metadata --format-version 1 --no-deps | ConvertFrom-Json
}
finally {
    Pop-Location
}
$msixDir = Join-Path $meta.target_directory 'msix'
$cerPath = Join-Path $msixDir 'ScreenGauge-test.cer'

$existing = @(Get-ChildItem -LiteralPath 'Cert:\CurrentUser\My' |
        Where-Object { $_.Subject -eq $publisher -and $_.NotAfter -gt (Get-Date) })
if ($existing.Count -gt 0) {
    $cert = $existing[0]
    Write-Host "A certificate for '$publisher' is already in Cert:\CurrentUser\My; it is used again."
}
else {
    # The parameters Microsoft Learn gives for MSIX signing: code-signing use (1.3.6.1.5.5.7.3.3), not a CA.
    $cert = New-SelfSignedCertificate -Type Custom -KeyUsage DigitalSignature `
        -CertStoreLocation 'Cert:\CurrentUser\My' `
        -TextExtension @('2.5.29.37={text}1.3.6.1.5.5.7.3.3', '2.5.29.19={text}') `
        -Subject $publisher -FriendlyName 'ScreenGauge MSIX test signing' `
        -NotAfter (Get-Date).AddMonths(3)
    Write-Host "Created a certificate for '$publisher' in Cert:\CurrentUser\My."
}
$thumbprint = $cert.Thumbprint

New-Item -ItemType Directory -Path $msixDir -Force | Out-Null
Export-Certificate -Cert $cert -FilePath $cerPath -Type CERT | Out-Null

Write-Host ''
Write-Host "Thumbprint: $thumbprint"
Write-Host "Public certificate: $cerPath"
Write-Host ''
Write-Host 'Next, in a PowerShell opened with "Run as administrator", trust it:'
Write-Host "  Import-Certificate -FilePath `"$cerPath`" -CertStoreLocation Cert:\LocalMachine\TrustedPeople"
Write-Host ''
Write-Host 'Then build the package signed with it:'
Write-Host "  pwsh -File `"$(Join-Path $PSScriptRoot 'make-msix.ps1')`" -CertThumbprint $thumbprint"
Write-Host ''
Write-Host 'To remove it after the tests, as administrator:'
Write-Host "  Remove-Item -LiteralPath Cert:\LocalMachine\TrustedPeople\$thumbprint"
Write-Host 'and in a normal PowerShell, which deletes the private key too:'
Write-Host "  Remove-Item -LiteralPath Cert:\CurrentUser\My\$thumbprint -DeleteKey"
