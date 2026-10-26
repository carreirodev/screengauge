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

## Tools

**Dimensions**:
The tool that measures the distance from the cursor to the nearest edges, horizontally, vertically, or both at once (cross).
_Avoid_: ruler, distance tool

**Bounds**:
The tool that shrinks a roughly drawn rectangle until it fits the content inside it.
_Avoid_: fit, auto-size

**Inspect**:
The magnifier that shows the area around the cursor enlarged, with the cursor's position and the pixel's color.
_Avoid_: zoom, magnifier, lupa

**Snip**:
The tool that captures an area of the screen sized exactly to its content.
_Avoid_: screenshot, capture, print
