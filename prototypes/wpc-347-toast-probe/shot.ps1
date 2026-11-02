# One-off (WPC-304): captures the bottom-right corner of the primary monitor (where Windows notifications appear), in
# physical pixels, to a PNG.
param([string]$Out, [int]$Width = 1500, [int]$Height = 900)
Add-Type -TypeDefinition @'
using System;
using System.Drawing;
using System.Runtime.InteropServices;
public static class Shot {
    [DllImport("user32.dll")] static extern bool SetProcessDpiAwarenessContext(IntPtr v);
    [DllImport("user32.dll")] static extern int GetSystemMetrics(int i);
    public static string Take(string path, int w, int h) {
        SetProcessDpiAwarenessContext(new IntPtr(-4));
        int sw = GetSystemMetrics(0), sh = GetSystemMetrics(1);
        var bmp = new Bitmap(w, h);
        using (var g = Graphics.FromImage(bmp)) g.CopyFromScreen(sw - w, sh - h, 0, 0, new Size(w, h));
        bmp.Save(path);
        return "primary " + sw + "x" + sh + ", captured " + w + "x" + h;
    }
}
'@ -ReferencedAssemblies System.Drawing.Common, System.Drawing.Primitives, System.Private.Windows.GdiPlus, System.Private.Windows.Core, System.Runtime.InteropServices
[Shot]::Take($Out, $Width, $Height)
