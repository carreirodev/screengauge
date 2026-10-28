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

For the WPC-286 consumption targets, `run` also reads environment variables: `HWTEST_NO_CHECK` skips the identical-check re-capture, `HWTEST_NO_POLL` drops the 1 s monitor poll, `HWTEST_REPEAT=N` re-triggers 4 s after each close until N calls, and `HWTEST_KEEP_DXGI` then holds a warm DXGI session for the idle samples. Usage lines include the private working set, the number Task Manager's Memory column shows. The WPC-286 measurement:

```
HWTEST_NO_CHECK=1 HWTEST_NO_POLL=1 HWTEST_REPEAT=10 hwtest.exe run --hotkey win+ctrl+shift+m --no-save --trigger-after-ms 5000 --auto-close-secs 1 --exit-after-secs 245
```

`hotkey --wait-secs N` and `recorder --secs N` wait for real key presses. `capture --hold-secs N` keeps each Windows.Graphics.Capture session open so the yellow border can be seen.

The WPC-281 results are in that Linear issue's resolution comment.
