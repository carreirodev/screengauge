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

`Win + Ctrl + M` is taken on Windows 11 (it opens Magnifier settings), so run the resident mode with a free combination:

```
hwtest.exe run --hotkey win+ctrl+shift+m --save-photo --auto-close-secs 30
```

Other `run` options: `--trigger-after-ms N` (opens the tools once without keys), `--once`, `--no-save`, `--activate-first` (on even triggers, the tray and Start paths take the foreground before the photo), `--present-vsync`, `--exit-after-secs N`. A left click on the tray icon opens the tools; a right click exits. The Start path needs a Start menu shortcut to `hwtest.exe run`.

`hotkey --wait-secs N` and `recorder --secs N` wait for real key presses. `capture --hold-secs N` keeps each Windows.Graphics.Capture session open so the yellow border can be seen.

The WPC-281 results are in that Linear issue's resolution comment.
