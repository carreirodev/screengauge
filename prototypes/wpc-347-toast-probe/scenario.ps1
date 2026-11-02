# PROTOTYPE, throwaway (WPC-347). One test scenario end to end; run it hidden and read toast-probe.log afterwards:
#   Start-Process pwsh -WindowStyle Hidden -ArgumentList '-NoProfile -File scenario.ps1 -Name s1 -Shots <dir>' ...
param([Parameter(Mandatory)] [string] $Name, [string] $Shots = $env:TEMP)
$ErrorActionPreference = 'Stop'
Set-Location $PSScriptRoot
$exe = '.\target\release\toast-probe.exe'
function Mark($t) { & $exe mark-$t | Out-Null }
function Probe([string] $a) { Start-Process $exe -ArgumentList $a -WindowStyle Hidden -PassThru }
function Center { & pwsh -NoProfile -File .\center.ps1 @args }
function Shot($n) { & .\shot.ps1 -Out (Join-Path $Shots "$Name-$n.png") -Width 1500 -Height 2000 | Out-Null }
foreach ($v in 'a', 'b', 'c') { & $exe clear $v | Out-Null }
Mark "begin-$Name"

switch ($Name) {
    # App running with the COM activator (variant c): click on the popup.
    's1' { $p = Probe 'show c --com --wait 20 --tag s1'; Start-Sleep 2; Center -Action click -Text 'probe c s1'; $p.WaitForExit() }
    # App running with the COM activator: click from the center after the popup has gone.
    's2' { $p = Probe 'show c --com --wait 30 --tag s2'; Start-Sleep 12; Center -Action click -Center -Text 'probe c s2'; $p.WaitForExit() }
    # No CustomActivator (variant a), Activated event only: click on the popup.
    's3' { $p = Probe 'show a --wait 20 --tag s3'; Start-Sleep 2; Center -Action click -Text 'probe a s3'; $p.WaitForExit() }
    # No CustomActivator: click from the center after the popup has gone, app running.
    's4' { $p = Probe 'show a --wait 30 --tag s4'; Start-Sleep 12; Center -Action click -Center -Text 'probe a s4'; $p.WaitForExit() }
    # CustomActivator and a registered class object, but no LocalServer32 (variant b): click from the center.
    's5' { $p = Probe 'show b --com --wait 30 --tag s5'; Start-Sleep 12; Center -Action click -Center -Text 'probe b s5'; $p.WaitForExit() }
    # Variant b without a class object: click from the center, app running.
    's6' { $p = Probe 'show b --wait 30 --tag s6'; Start-Sleep 12; Center -Action click -Center -Text 'probe b s6'; $p.WaitForExit() }
    # App closed, full setup (variant c): click from the center; Windows should start "toast-probe.exe cold c".
    's7' { $p = Probe 'show c --wait 3 --tag s7'; $p.WaitForExit(); Start-Sleep 8; Center -Action click -Center -Text 'probe c s7'; Start-Sleep 25 }
    # App closed, no CustomActivator (variant a): click from the center.
    's8' { $p = Probe 'show a --wait 3 --tag s8'; $p.WaitForExit(); Start-Sleep 8; Center -Action click -Center -Text 'probe a s8'; Start-Sleep 10 }
    # App closed, variant c: click on the popup.
    's7p' { $p = Probe 'show c --wait 1 --tag s7p'; $p.WaitForExit(); Start-Sleep 1; Center -Action click -Text 'probe c s7p'; Start-Sleep 25 }
    # "Do not disturb" on: show, look, turn it off, look again, then list the center.
    's9' {
        Center -Action dnd -State on
        $p = Probe 'show c --com --wait 40 --tag s9'
        Start-Sleep 2; Shot 'dnd-on'
        Start-Sleep 8
        Center -Action dnd -State off
        Start-Sleep 1; Shot 'dnd-off-1s'
        Start-Sleep 3; Shot 'dnd-off-4s'
        Center -Action list -Text 'probe c s9' -Shot (Join-Path $Shots "$Name-center.png")
        $p.WaitForExit()
    }
    # A full-screen program in front: the notification is shown behind it, then the program closes.
    's10' {
        $fs = Start-Process pwsh -ArgumentList '-NoProfile -File .\fullscreen.ps1 -Seconds 14' -WindowStyle Hidden -PassThru
        Start-Sleep 3
        $p = Probe 'show c --com --wait 30 --tag s10'
        Start-Sleep 2; Shot 'fs-on'
        $fs.WaitForExit()
        Start-Sleep 3; Shot 'fs-off-3s'
        Center -Action list -Text 'probe c s10' -Shot (Join-Path $Shots "$Name-center.png")
        $p.WaitForExit()
    }
}
Mark "end-$Name"
