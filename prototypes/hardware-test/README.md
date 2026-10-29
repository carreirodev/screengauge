# hwtest

Throwaway program for the Pixmeter hardware tests: WPC-281 (two monitors with different scales) and WPC-289 (MSIX copy and HDR colors). It lives only on the `prototype/hardware-test` branch and is never merged into `main`. It is not the start of the app.

Build: `cargo build --release`. The manifest makes it Per-Monitor V2, and the C runtime is static.

Run `target/release/hwtest.exe <mode>`. Each mode logs what it measured to stdout and to `%HWTEST_OUT%` (default `%TEMP%\hwtest`).

| Mode | Checks |
| --- | --- |
| `info` | Monitors, DPI, cursor, virtual screen, as Per-Monitor V2 and as an unaware process would see them |
| `capture` | DXGI, GDI and Windows.Graphics.Capture on every monitor: size, time, pixel equality, yellow border |
| `measure` | A rectangle of known size on each monitor, captured and measured |
| `hotkey` | Registering `Win + Ctrl + M`, conflicts, and the key press |
| `recorder` | Recording a combination with the Win key without opening Start |
| `run` | Resident like Pixmeter: tray icon, hotkey, frozen screen, timings, closing rules |
| `hdr` | WPC-289: 1102 known colors painted on every monitor and read back through every capture path; the frozen photo shown back and compared in FP16; the frozen photo's time; `--watch-secs N` logs HDR and SDR white level changes |
| `package` | WPC-289: package identity, the startup task, where a settings file lands; see `packaging/README.md` |

`Win + Ctrl + M` is taken on Windows 11 (it opens Magnifier settings), so run the resident mode with a free combination:

```
hwtest.exe run --hotkey win+ctrl+shift+m --save-photo --auto-close-secs 30
```

Other `run` options: `--trigger-after-ms N` (opens the tools once without keys), `--once`, `--no-save`, `--activate-first` (on even triggers, the tray and Start paths take the foreground before the photo), `--present-vsync`, `--exit-after-secs N`. A left click on the tray icon opens the tools; a right click exits. The Start path needs a Start menu shortcut to `hwtest.exe run`.

For the WPC-286 consumption targets, `run` also reads environment variables: `HWTEST_NO_CHECK` skips the identical-check re-capture, `HWTEST_NO_POLL` drops the 1 s monitor poll, `HWTEST_REPEAT=N` re-triggers 4 s after each close until N calls (`HWTEST_FIRST_GAP_MS` sets a longer gap after the first call, for settled idle samples), and `HWTEST_KEEP_DXGI` then holds a warm DXGI session for the idle samples. Usage lines include the private working set, which should be the number Task Manager's Memory column shows (not compared side by side). The WPC-286 measurement:

```
HWTEST_NO_CHECK=1 HWTEST_NO_POLL=1 HWTEST_REPEAT=10 hwtest.exe run --hotkey win+ctrl+shift+m --no-save --trigger-after-ms 5000 --auto-close-secs 1 --exit-after-secs 245
```

`hotkey --wait-secs N` and `recorder --secs N` wait for real key presses. `capture --hold-secs N` keeps each Windows.Graphics.Capture session open so the yellow border can be seen.

## HDR (WPC-289)

Every DXGI capture (the frozen photo, `capture`, `measure`, `run`) now picks its format from `HWTEST_DXGI_FORMAT`:

- `auto` (default): on a monitor with HDR on, `DuplicateOutput1` in R16G16B16A16_FLOAT, converted back to 8 bits by dividing by the monitor's SDR white level (`DISPLAYCONFIG_SDR_WHITE_LEVEL`) and applying the curve; B8G8R8A8 elsewhere, as before.
- `bgra8`: always `DuplicateOutput1` in B8G8R8A8, so with HDR on Windows does the conversion itself.
- `fp16`: always FP16 and converted, also with HDR off (SDR white is then 80 nits, scRGB 1.0).
- `legacy`: the old `DuplicateOutput`.

`HWTEST_HDR_CURVE` picks the curve for the conversion: `srgb` (default, the piecewise sRGB curve) or `g22` (a pure 2.2 power).

`hdr` paints its patches with GDI (no color management), then reads them back with DXGI in B8G8R8A8, the old `DuplicateOutput` and FP16 (FP16 also redone with gamma 2.2 and with the white level measured from the white patch), GDI, and Windows.Graphics.Capture in B8G8R8A8 and FP16. Each `RESULT hdr-color` line says how many of the 1102 patches came back exact. Then it shows the frozen photo back full screen and compares the screen in FP16 with the live screen (`RESULT hdr-frozen`), and times three cold frozen photos per format (`RESULT hdr-photo-time`). Options: `--patch-px N` (12), `--no-wgc`, `--no-overlay`, `--no-timing`, `--hold-secs N`.

```
hwtest.exe hdr
```

```
hwtest.exe hdr --watch-secs 120
```

The second one only watches: it polls DisplayConfig and DXGI every 250 ms and logs each HDR or SDR white level change, plus the window messages that arrive (`WM_DISPLAYCHANGE`, `WM_SETTINGCHANGE` and others).

## MSIX copy (WPC-289)

`packaging/README.md` explains how to build the package, install it with a test certificate or through Developer Mode, run modes in the packaged copy through the `screengauge-hwtest.exe` alias, and remove everything afterwards.

The WPC-281 results are in that Linear issue's resolution comment.
