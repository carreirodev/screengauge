# ScreenGauge

A desktop tool for measuring and inspecting what is on the computer screen, pixel by pixel.

## Language

**ScreenGauge**:
The screen-measurement app this repository builds, written from scratch.
_Avoid_: Pixmeter (the name until 2026-10-28), Pixler (the name until 2026-10-26), SightLine (the toolbar design's former name)

**Rooler**:
Pete Blois's 2015 Windows screen-measurement app, used only as a reference for which features exist.
_Avoid_: the old version, ScreenGauge v0

## Measurement

**Physical pixel**:
The smallest point Windows draws on a monitor at its current resolution.
_Avoid_: real pixel, raw pixel, device pixel

**Scale**:
The percentage Windows sets for each monitor to enlarge text and controls (100%, 125%, 150%, and so on).
_Avoid_: DPI, zoom, scaling factor

**Logical pixel**:
A length in physical pixels divided by the scale of the monitor it is on; the size Windows and app designs give an element before scaling.
_Avoid_: DIP, device-independent pixel, effective pixel, scaled pixel

**Current monitor**:
The monitor under the cursor; the only monitor a tool measures or captures on at any moment.
_Avoid_: active monitor, main monitor (Windows' primary monitor, a different thing)

**Reference color**:
The color a tool compares every pixel with to tell background from content: the color under the cursor in Dimensions, and the color where the drag began in Bounds and Snip.
_Avoid_: background color, base color, sample color

**Tolerance**:
How much a pixel's color may differ from the reference color and still count as the same area.
_Avoid_: threshold, sensitivity, pixel difference

**Edge**:
The first pixel, moving away from the cursor along its row or column, whose color differs from the reference color by more than the tolerance.
_Avoid_: border, boundary

**Drawn rectangle**:
The rectangle the user drags out with the mouse, exactly as drawn, before any shrinking to the content.
_Avoid_: rough rectangle, selection, marquee

**Content**:
Every pixel inside a drawn rectangle whose color differs from the reference color by more than the tolerance, whether it forms one object or several.
_Avoid_: object, element

**Pinned measurement**:
A measurement left fixed on screen, by a click in Dimensions or by releasing the drag in Bounds, while the tool keeps measuring; it lasts until the tools close, even when the user switches to another tool.
_Avoid_: frozen measurement (freezing is for the screen, a different thing), persistent measurement, annotation

## Using ScreenGauge

**Toolbar**:
The floating bar with one button per tool that appears whenever ScreenGauge is called, with the active tool's button highlighted.
_Avoid_: floating bar, palette, dock, menu

**Active tool**:
The one tool that has the mouse at a given moment; calling ScreenGauge makes the last one used active again.
_Avoid_: current tool (the current monitor is a different thing), selected tool

**Global shortcut**:
The one key combination that calls ScreenGauge from any program.
_Avoid_: hotkey, keyboard shortcut (the keys inside a tool are a different thing)

**Tray icon**:
ScreenGauge's icon in the Windows notification area, next to the clock; clicking it calls ScreenGauge like the global shortcut.
_Avoid_: taskbar icon, system tray icon

**Hidden icons panel**:
The Windows panel that opens from the arrow next to the clock and holds the notification-area icons not shown directly on the taskbar; the tray icon may sit there or on the taskbar itself.
_Avoid_: overflow, tray flyout, hidden icon menu (the Windows setting that turns the panel on or off)

**Settings**:
The window, separate from the tools, where the user changes ScreenGauge's options.
_Avoid_: preferences, options, Settings tool

**Remembered state**:
What ScreenGauge keeps between calls without showing it in Settings, such as the last tool used and the toolbar's place; it lives in the settings file and goes back to its defaults with it.
_Avoid_: history, session state, last session

**Frozen screen**:
The still image of every monitor, taken all at once when ScreenGauge is called, that stays on screen in place of the live screen until the tools close; it is what the tools measure and capture.
_Avoid_: snapshot, screenshot (the Snip's image is a different thing), still

**Notice**:
A short message ScreenGauge draws on screen itself, next to the cursor or in the magnified view, such as Copied, No content found or Approximate color.
_Avoid_: toast, popup, alert, notification (a Windows notification is the system's message in the corner of the screen, a different thing)

## Tools

**Dimensions**:
The tool that measures the span of pixels around the cursor, from the nearest edge on one side to the nearest edge on the other, horizontally, vertically, or both at once (cross).
_Avoid_: ruler, distance tool

**Cross**:
The Dimensions mode that measures width and height at once.
_Avoid_: spacing, crosshair

**Bounds**:
The tool that shrinks a drawn rectangle until it fits the content inside it, never past what was drawn.
_Avoid_: fit, auto-size

**Inspect**:
The tool that shows a magnified view of the area around the cursor, with the cursor's position and the color of the pixel under it.
_Avoid_: zoom, magnifier, lupa

**Magnified view**:
The enlarged picture of the pixels around the cursor that Inspect shows, with the pixel under the cursor always at its center.
_Avoid_: loupe, zoom window

**Magnification**:
How many times larger each physical pixel appears in the magnified view.
_Avoid_: zoom, scale (the monitor's scale is a different thing)

**Snip**:
The tool that takes an image of the content a drawn rectangle shrinks to, or of the drawn rectangle itself when shrinking is off.
_Avoid_: screenshot, capture, print

## Color

**HDR**:
Windows' high dynamic range mode, turned on or off for each monitor in Windows settings, which brightens the colors of SDR content on that monitor.
_Avoid_: HD Color, Advanced Color (Windows' broader name, which also covers some SDR monitors)

**SDR content**:
Whatever ordinary apps draw in standard colors, such as windows, web pages and pictures; its exact color is what ScreenGauge reports, with HDR on or off.
_Avoid_: normal content, standard content

**HDR content**:
Video, games or pictures brighter or more saturated than any standard color, so they have no exact standard color to report.
_Avoid_: HDR image, HDR video (both are only examples)

**Approximate color**:
Inspect's notice that the color shown may differ from the one the app drew, because HDR is on and ScreenGauge can't undo its brightening on that monitor.
_Avoid_: wrong color, HDR warning

## Distribution

**Store copy**:
ScreenGauge as installed from the Microsoft Store, which keeps it up to date.
_Avoid_: Store version (a version is a number), MSIX build, packaged app

**Installer copy**:
ScreenGauge as installed by the installer downloaded from the project's GitHub releases.
_Avoid_: GitHub version, desktop version, portable version (there is none)

## Identity

**App icon**:
The set square that stands for ScreenGauge in Start, on the taskbar, in the installed apps list and, as the tray icon, next to the clock.
_Avoid_: logo (it could mean the wordmark too), Store icon (a different drawing of the same set square)

**Store icon**:
The app icon drawn with thickness and reflection, shown only on ScreenGauge's Microsoft Store page.
_Avoid_: Store logo, tile icon

**Store art**:
The wide picture at the top of ScreenGauge's Microsoft Store page.
_Avoid_: banner, hero image, wide image

**Wordmark**:
The name ScreenGauge written as a mark, with "Screen" light and "Gauge" bold, as at the top of the README.
_Avoid_: logo, logotype
