# PROTOTYPE, throwaway (WPC-347). Drives the notification popup and the notification center with UI Automation and
# real mouse clicks. Run it hidden (Start-Process pwsh -WindowStyle Hidden): the Claude Code terminal takes the front
# when a tool prints, which closes the center.
#   center.ps1 -Action list  [-Shot <png>]           opens the center, lists entries whose text has -Text, closes it
#   center.ps1 -Action click -Text <t>               clicks the popup whose text has <t>
#   center.ps1 -Action click -Text <t> -Center       opens the center and clicks the entry whose text has <t>
#   center.ps1 -Action dnd -State on|off             sets "Do not disturb" with the center's own toggle
# The center doesn't show among UIA's top-level windows; it is found from the window under a point on the right edge.
# A synthetic Win+N did not open it from a hidden process, so it opens through the shell's ms-actioncenter: URI.
param(
    [Parameter(Mandatory)] [ValidateSet('list', 'click', 'dnd')] [string] $Action,
    [string] $Text = 'ScreenGauge probe',
    [switch] $Center,
    [ValidateSet('on', 'off')] [string] $State,
    [string] $Shot
)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class In {
    [StructLayout(LayoutKind.Sequential)] public struct P { public int x, y; }
    [StructLayout(LayoutKind.Sequential)] struct MI { public int dx, dy; public uint data, flags, time; public IntPtr extra; }
    [StructLayout(LayoutKind.Sequential)] struct INPUT { public uint type; public MI m; }
    [DllImport("user32.dll")] static extern uint SendInput(uint n, INPUT[] i, int size);
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr v);
    [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(P p);
    [DllImport("user32.dll")] public static extern IntPtr GetAncestor(IntPtr h, uint f);
    [DllImport("user32.dll")] public static extern int GetSystemMetrics(int i);
    [DllImport("user32.dll")] public static extern void keybd_event(byte v, byte s, uint f, UIntPtr e);
    public static IntPtr RootAt(int x, int y) { var p = new P(); p.x = x; p.y = y; return GetAncestor(WindowFromPoint(p), 2); }
    public static void Click(int x, int y) {
        SetCursorPos(x, y);
        var d = new INPUT(); d.type = 0; d.m.flags = 2;
        var u = new INPUT(); u.type = 0; u.m.flags = 4;
        SendInput(2, new[] { d, u }, Marshal.SizeOf(typeof(INPUT)));
    }
    public static void Esc() { keybd_event(0x1B, 0, 0, UIntPtr.Zero); keybd_event(0x1B, 0, 2, UIntPtr.Zero); }
}
'@
[In]::SetProcessDpiAwarenessContext([IntPtr]-4) | Out-Null
$A = [System.Windows.Automation.AutomationElement]
$T = [System.Windows.Automation.TreeScope]
$sw = [In]::GetSystemMetrics(0); $sh = [In]::GetSystemMetrics(1)

function All($root) { $A::FromHandle($root).FindAll($T::Descendants, [System.Windows.Automation.Condition]::TrueCondition) }
function Open-Center {
    Start-Process 'ms-actioncenter:'
    Start-Sleep -Milliseconds 2500
    [In]::RootAt($sw - 300, 700)
}
function Click-Element($e) {
    $r = $e.Current.BoundingRectangle
    # Left of centre, on the text, away from the close and "..." buttons on the right.
    $x = [int]($r.X + $r.Width * 0.3); $y = [int]($r.Y + $r.Height * 0.7)
    "click at $x,$y on [$($e.Current.Name)]"
    [In]::Click($x, $y)
}

switch ($Action) {
    'list' {
        $root = Open-Center
        if ($Shot) { & (Join-Path $PSScriptRoot 'shot.ps1') -Out $Shot -Width 1500 -Height 2000 | Out-Null }
        foreach ($e in (All $root)) {
            $n = $e.Current.Name
            if ($e.Current.ControlType.ProgrammaticName -in 'ControlType.ListItem', 'ControlType.Group' -and $n -like "*$Text*") { "entry: [$n]" }
            if ($n -like '*incomodar*' -or $n -like '*disturb*') {
                $state = try { $e.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Current.ToggleState } catch { '?' }
                "dnd: $state"
            }
        }
        [In]::Esc()
    }
    'dnd' {
        $root = Open-Center
        $btn = All $root | Where-Object { $_.Current.Name -like '*incomodar*' -or $_.Current.Name -like '*disturb*' } | Select-Object -First 1
        $toggle = $btn.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern)
        $want = if ($State -eq 'on') { 'On' } else { 'Off' }
        if ("$($toggle.Current.ToggleState)" -ne $want) { $toggle.Toggle(); Start-Sleep -Milliseconds 600 }
        "dnd now: $($toggle.Current.ToggleState)"
        [In]::Esc()
    }
    'click' {
        if ($Center) {
            $root = Open-Center
            $item = All $root | Where-Object { $_.Current.ControlType.ProgrammaticName -eq 'ControlType.ListItem' -and $_.Current.Name -like "*$Text*" } | Select-Object -First 1
        } else {
            # The popup sits in the bottom-right corner, above the taskbar.
            $root = [In]::RootAt($sw - 300, $sh - 170)
            $item = All $root | Where-Object { $_.Current.Name -like "*$Text*" } | Select-Object -First 1
        }
        if (-not $item) { "nothing to click with [$Text]"; if ($Center) { [In]::Esc() }; exit 1 }
        Click-Element $item
        Start-Sleep -Milliseconds 800
    }
}
