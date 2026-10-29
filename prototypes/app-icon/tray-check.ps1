# PROTOTYPE, one-off for WPC-291: shows the set square in the real notification area, one icon per
# screengauge-*.ico in -Folder (built by make-ico.mjs), at the exact pixel size the taskbar uses.
# Right-click any of them and pick "Fechar" to close all; they also close after -Minutes.
# Usage: pwsh -NoProfile -File tray-check.ps1 -Folder <dir with the .ico files> [-Minutes 30]
param([string]$Folder, [int]$Minutes = 20)
$Icons = (Get-ChildItem -Path $Folder -Filter 'screengauge-*.ico' | Sort-Object Name).FullName

Add-Type -AssemblyName System.Windows.Forms, System.Drawing
Add-Type -Namespace Native -Name User32 -MemberDefinition @'
[DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern IntPtr FindWindow(string cls, string name);
[DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr hwnd);
'@

$taskbar = [Native.User32]::FindWindow('Shell_TrayWnd', $null)
$dpi = [Native.User32]::GetDpiForWindow($taskbar)
$size = [int][Math]::Round(16 * $dpi / 96)
"taskbar dpi $dpi -> tray icon $size x $size px"

$menu = New-Object System.Windows.Forms.ContextMenuStrip
$quit = $menu.Items.Add('Fechar')
$quit.add_Click({ [System.Windows.Forms.Application]::Exit() })

$shown = foreach ($path in $Icons) {
    $n = New-Object System.Windows.Forms.NotifyIcon
    $ico = New-Object System.Drawing.Icon($path, $size, $size)
    Write-Host "$([IO.Path]::GetFileName($path)): loaded frame $($ico.Width) x $($ico.Height)"
    $n.Icon = $ico
    $n.Text = "ScreenGauge (protótipo) - $([IO.Path]::GetFileNameWithoutExtension($path))"
    $n.ContextMenuStrip = $menu
    $n.Visible = $true
    $n
}

$timer = New-Object System.Windows.Forms.Timer
$timer.Interval = $Minutes * 60 * 1000
$timer.add_Tick({ [System.Windows.Forms.Application]::Exit() })
$timer.Start()
[System.Windows.Forms.Application]::Run()
foreach ($n in $shown) { if ($n -is [System.Windows.Forms.NotifyIcon]) { $n.Visible = $false; $n.Dispose() } }
'closed'
