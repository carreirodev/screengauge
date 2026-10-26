# Pixmeter

A desktop tool for measuring and inspecting what is on the computer screen, pixel by pixel.

## Language

**Pixmeter**:
The screen-measurement app this repository builds, written from scratch.
_Avoid_: Pixler (the name until 2026-10-26), SightLine (the toolbar design's former name)

**Rooler**:
Pete Blois's 2015 Windows screen-measurement app, used only as a reference for which features exist.
_Avoid_: the old version, Pixmeter v0

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
The color a tool compares every pixel with to tell background from content: the color under the cursor in Dimensions, and the color where the drag began in Bounds.
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

## Using Pixmeter

**Toolbar**:
The floating bar with one button per tool that appears whenever Pixmeter is called, with the active tool's button highlighted.
_Avoid_: floating bar, palette, dock, menu

**Active tool**:
The one tool that has the mouse at a given moment; calling Pixmeter makes the last one used active again.
_Avoid_: current tool (the current monitor is a different thing), selected tool

**Global shortcut**:
The one key combination that calls Pixmeter from any program.
_Avoid_: hotkey, keyboard shortcut (the keys inside a tool are a different thing)

**Tray icon**:
Pixmeter's icon in the Windows notification area, next to the clock; clicking it calls Pixmeter like the global shortcut.
_Avoid_: taskbar icon, system tray icon

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
The tool that captures an area of the screen sized exactly to its content.
_Avoid_: screenshot, capture, print
