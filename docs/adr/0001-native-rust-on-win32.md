# Native Rust on Win32 and Direct2D, with no UI framework

ScreenGauge is written in Rust directly against the Windows APIs: Win32 windows, Direct2D and DirectWrite for drawing, and the Windows screen-capture APIs. It uses no UI framework and no web engine. ScreenGauge draws everything users see: the measurements on screen, the toolbar and the Settings window, light and dark themes included. Only the Windows file dialogs (Save As, choosing a folder) belong to the system. The project exists so that a single screen tool does not mean installing a suite like PowerToys and its runtimes, so "installs only ScreenGauge" outranks ready-made controls.

## Considered Options

- **Native core + WinUI 3 screens** (the PowerToys layout): the Windows 11 look and automatic theming come for free. It needs the 120 MB Windows App Runtime or a self-contained copy that Microsoft calls "significantly larger", adds C# and .NET as a second language, and each Windows App SDK line gets about a year of fixes. Rejected as the bloat the project avoids.
- **Tauri 2** (Rust + HTML/CSS in WebView2): light to install, since WebView2 ships with Windows. The owner wants no web engine in ScreenGauge, not even for Settings. Rejected.
- **C++ instead of Rust**: PowerToys' Screen Ruler is C++, so more reference code exists, but the C++ compiler does not reject memory-safety errors. The code will be AI-written and not reviewed line by line by the owner, so Rust's compile-time checks win. Microsoft's `windows` crates are below 1.0 and change often, so versions are pinned and upgraded on purpose.
- **Windows common controls for Settings**: less code, with keyboard and Narrator support built in. Windows documents dark mode only for the title bar, though, and the controls would look unlike the toolbar. Rejected.
- Electron, Flutter, WPF, WinForms, Avalonia and Qt were dropped by the research ticket WPC-266 (branch `research/windows-app-technologies`).

## Consequences

- Capture is a single shot taken when ScreenGauge is called (the frozen screen, decided in WPC-275), not a continuous stream. It uses the same Windows capture APIs, so it doesn't change this choice.
- Theming, hit-testing, focus and per-monitor scaling of ScreenGauge's own UI are ScreenGauge's code.
- The C runtime is linked statically (`-C target-feature=+crt-static`), so users need no Visual C++ Redistributable.
- Windows 11 and later only, so Windows 11 APIs such as the dark title bar (documented from build 22000) can be relied on.
- v1 has no Narrator support or full keyboard navigation in the toolbar and Settings. Adding it later means implementing UI Automation for hand-drawn UI.
- The hardware test WPC-281 ("Teste da tecnologia escolhida em dois monitores com escalas diferentes", 2026-10-28) confirmed the choice. On a 4K monitor at 175% and a Full HD at 125%, each on its own GPU, monitor bounds, the cursor and every capture API came in physical pixels. Known sizes measured exactly. The frozen screen matched the live screen pixel for pixel. DXGI Desktop Duplication captured both monitors with no yellow border. A 0.9 MB `.exe` with a static C runtime imported only Windows DLLs.
