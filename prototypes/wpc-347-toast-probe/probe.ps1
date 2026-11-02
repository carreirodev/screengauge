# PROTOTYPE, throwaway (WPC-347). Registry setup for toast-probe's three variants, under HKCU only.
#   a: AppUserModelId keys (DisplayName, IconUri), no CustomActivator
#   b: plus CustomActivator, no LocalServer32
#   c: plus LocalServer32 = "<toast-probe.exe>" cold c
# probe.ps1 register -Variant c [-Icon <png|ico path> | -Icon none]
# probe.ps1 unregister        removes every key and file the probe made, and the center's entries
# probe.ps1 check             lists what is left
param(
    [Parameter(Position = 0, Mandatory)] [ValidateSet('register', 'unregister', 'check')] [string] $Action,
    [ValidateSet('a', 'b', 'c')] [string] $Variant,
    [string] $Icon
)
$ErrorActionPreference = 'Stop'
$exe = Join-Path $PSScriptRoot 'target\release\toast-probe.exe'
$iconDir = Join-Path $env:LOCALAPPDATA 'ScreenGaugeProbe'
$variants = @{
    a = @{ Aumid = 'WPCSolutions.ScreenGaugeProbeA'; Clsid = '{D4C5E347-0A0A-4A7E-9F2C-5C1E0D347A01}' }
    b = @{ Aumid = 'WPCSolutions.ScreenGaugeProbeB'; Clsid = '{D4C5E347-0B0B-4A7E-9F2C-5C1E0D347B01}' }
    c = @{ Aumid = 'WPCSolutions.ScreenGaugeProbeC'; Clsid = '{D4C5E347-0C0C-4A7E-9F2C-5C1E0D347C01}' }
}
function Paths($v) {
    @(
        "HKCU:\Software\Classes\AppUserModelId\$($v.Aumid)",
        "HKCU:\Software\Classes\CLSID\$($v.Clsid)",
        "HKCU:\Software\Microsoft\Windows\CurrentVersion\Notifications\Settings\$($v.Aumid)"
    )
}

switch ($Action) {
    'register' {
        $v = $variants[$Variant]
        $key = "HKCU:\Software\Classes\AppUserModelId\$($v.Aumid)"
        New-Item -Path $key -Force | Out-Null
        New-ItemProperty -Path $key -Name DisplayName -PropertyType ExpandString -Value "ScreenGauge Probe $($Variant.ToUpper())" -Force | Out-Null
        if ($Icon -and $Icon -ne 'none') {
            New-Item -ItemType Directory -Force $iconDir | Out-Null
            # A new file name per icon, because the shell caches what it read for an AUMID.
            $copy = Join-Path $iconDir ("$($v.Aumid)-$([DateTime]::Now.ToString('HHmmss'))-" + (Split-Path $Icon -Leaf))
            Copy-Item $Icon $copy
            New-ItemProperty -Path $key -Name IconUri -PropertyType ExpandString -Value $copy -Force | Out-Null
        }
        if ($Variant -in 'b', 'c') {
            New-ItemProperty -Path $key -Name CustomActivator -PropertyType String -Value $v.Clsid -Force | Out-Null
        }
        if ($Variant -eq 'c') {
            $server = "HKCU:\Software\Classes\CLSID\$($v.Clsid)\LocalServer32"
            New-Item -Path $server -Force | Out-Null
            Set-Item -Path $server -Value "`"$exe`" cold c"
        }
        Get-ItemProperty $key | Select-Object DisplayName, IconUri, CustomActivator | Format-List
    }
    'unregister' {
        foreach ($k in $variants.Keys) {
            & $exe clear $k | Out-Null
            foreach ($p in Paths $variants[$k]) {
                if (Test-Path $p) { Remove-Item -Path $p -Recurse -Force; "removed $p" }
            }
        }
        if (Test-Path $iconDir) { Remove-Item $iconDir -Recurse -Force; "removed $iconDir" }
    }
    'check' {
        foreach ($k in $variants.Keys) {
            foreach ($p in Paths $variants[$k]) { if (Test-Path $p) { "left: $p" } }
        }
        if (Test-Path $iconDir) { "left: $iconDir" }
        'check done'
    }
}
