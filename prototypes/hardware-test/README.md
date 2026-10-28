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
