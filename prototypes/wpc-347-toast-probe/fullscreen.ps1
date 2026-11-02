# PROTOTYPE, throwaway (WPC-347). A borderless full-screen window in front for -Seconds, standing in for a game or a
# full-screen video, so the probe can see what Windows does with a notification shown behind it.
param([int] $Seconds = 14)
Add-Type -AssemblyName System.Windows.Forms
$f = New-Object System.Windows.Forms.Form
$f.FormBorderStyle = 'None'
$f.WindowState = 'Maximized'
$f.TopMost = $true
$f.BackColor = [System.Drawing.Color]::DarkSlateGray
$t = New-Object System.Windows.Forms.Timer
$t.Interval = $Seconds * 1000
$t.add_Tick({ $f.Close() })
$t.Start()
$f.add_Shown({ $f.Activate() })
[void] $f.ShowDialog()
