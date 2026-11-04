# ScreenGauge's Microsoft Store listing

Every text pasted into Partner Center for ScreenGauge's Store page, one section per field, in English only (WPC-341).
Paste each section's text as it is, without its heading. [README.md](README.md) says where each field goes, and the
tests in `crates/screengauge/tests/store_listing.rs` hold these texts to Microsoft's limits and the spec's rules.

## Product name

ScreenGauge

## Short description

Measure anything on your screen, pixel by pixel. ScreenGauge freezes your monitors and gives you four tools: measure the gap between edges, fit a box around content, read the exact color of a pixel, and capture what you need as an image.

## Description

ScreenGauge is a free, open-source screen ruler for Windows 11. Press Win + Shift + X: it freezes everything on your monitors and opens a toolbar with four tools.

Dimensions measures the gap around the cursor, from the nearest edge on one side to the nearest edge on the other: horizontally, vertically, or both at once.

Bounds shrinks a rectangle you drag until it fits the content inside it.

Inspect magnifies the area around the cursor and shows the cursor's position and the exact color of the pixel under it, in HEX, RGB, or both.

Snip captures the content inside a rectangle you drag as a PNG image, to the clipboard, a file, or both.

Every number is in physical pixels, with logical pixels next to it when Windows scales the monitor. A click copies the number or the color, ready to paste into your design tool, your code or a bug report.

Pin measurements on the screen to compare them, move the cursor one pixel at a time with the arrow keys, and set with the mouse wheel how different a color must be to count as an edge. ScreenGauge works with several monitors, each at its own scale: each tool measures the monitor under the cursor.

ScreenGauge waits next to the clock until you need it. Open it with the shortcut, which you can change in Settings, or with a click on its icon. It can start with Windows, and its theme can be light, dark, or follow Windows.

ScreenGauge collects nothing and has no account. The Microsoft Store keeps it up to date.

## Product features

- Freeze every monitor with one shortcut, Win + Shift + X, and measure what is on the screen without it moving
- Dimensions: measure the gap between edges horizontally, vertically, or both at once
- Bounds: drag a rectangle and it shrinks to fit the content inside it
- Inspect: magnify the area under the cursor and read the exact color of a pixel in HEX or RGB
- Snip: capture a rectangle as a PNG image, to the clipboard, a file, or both
- Physical pixels, with logical pixels next to them on scaled monitors
- One click copies a measurement or a color
- Pin measurements on the screen to compare them
- Move the cursor one pixel at a time with the arrow keys
- Set with the mouse wheel how different a color must be to count as an edge
- Works with several monitors, each at its own scale
- Light and dark themes, or the one Windows uses
- Free and open source, with no account and no data collected

## Screenshots

1. Dimensions measures the gap between edges, horizontally and vertically, in physical and logical pixels.
2. Bounds shrinks the rectangle you drag until it fits the content inside it.
3. Inspect magnifies the pixels under the cursor and shows their exact color in HEX and RGB.
4. Snip captures the rectangle you drag as a PNG image, to the clipboard, a file, or both.
5. Settings: the shortcut, the theme, the tolerance, the color format and where Snips go.

## Privacy policy URL

https://wpcsolutions.com.br/screengauge/privacy/

## Website

https://github.com/carreirodev/screengauge

## Restricted capability: runFullTrust

ScreenGauge is a Win32 desktop app, a native program written in Rust on the Win32 API and packaged as MSIX. Its one executable, ScreenGauge.exe, runs as a full-trust desktop process (EntryPoint Windows.FullTrustApplication), which needs runFullTrust. It is the same program we publish with an installer on GitHub, and what it does needs a full-trust desktop process:

- It stays in the notification area (Shell_NotifyIcon) and registers a global hotkey (RegisterHotKey, Win + Shift + X by default), so the user can open its tools over any app.
- To freeze the screen and measure it, it captures every monitor with DXGI Desktop Duplication, or with GDI BitBlt where that fails. The images stay in memory and are discarded when the tools close; nothing is sent anywhere.
- While the user records a new shortcut in its Settings window, and only then, it installs a low-level keyboard hook (WH_KEYBOARD_LL), so a combination with the Windows key records without opening the Start menu. The hook is removed as soon as the recording stops.
- Its notifications use a packaged COM server (com:ExeServer with desktop:ToastNotificationActivation), so a click on one reaches ScreenGauge even when it is closed.
- When its icon is clicked inside the hidden icons panel, it hides that panel (ShowWindow) before capturing, so the panel is not in the frozen screen.

ScreenGauge collects no data, and the Store copy never goes to the internet: its Check for updates opens the app's Store page. The source code is public on GitHub, under the MIT license.
