# The installer copy starts with Windows from a `Run` value, and reads the user's choice in Windows without writing it

"Start with Windows" in the installer copy is a value named `ScreenGauge` in the signed-in user's `Software\Microsoft\Windows\CurrentVersion\Run` key, holding the running `.exe`'s path in quotes and `--sign-in`. Windows starts every program listed there at sign-in, and lists each one in Task Manager's Startup apps and in Settings > Apps > Startup, where the user can turn it off. Started with `--sign-in`, ScreenGauge stays hidden with its tools closed, and exits in silence should a copy already run. Turning the option on in ScreenGauge writes the value, turning it off deletes it. Decided in WPC-326.

The user's choice in Task Manager or in Windows' Settings is the value of the same name in `Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run`. Windows doesn't document that key; read on the owner's computer (Windows 11 build 26300), an entry turned off holds 12 bytes starting with `03` and the time it was turned off, and one left on or turned on again `02` and zeros. ScreenGauge reads an odd first byte as off, and never writes the key, so the choice made in Windows is never overridden: listed there and turned off, the switch and the tray menu show it off, and a click on either only says where to turn it on. The rule is the core's (`screengauge_core::settings::StartWithWindows`).

Windows holds what happens at sign-in, so it is what the switch shows; the settings file's `start_with_windows` only follows the clicks made in ScreenGauge, and is never written to Windows at startup, which would put back a value the user removed there.

## Considered Options

- **A shortcut in the Startup folder**: Windows shows and respects it the same way (`StartupApproved\StartupFolder`), but it is a file the user can see and move, and making a `.lnk` needs COM's shell link interfaces for what one registry value does.
- **A scheduled task at log on**: it can start sooner, but Task Manager and Settings > Apps > Startup don't list it, so the user couldn't turn it off where Windows says to, and the spec's rule of respecting that choice would have nothing to read.
- **Writing `StartupApproved` to turn it on again from ScreenGauge**: it would override the user's choice in Windows, which the spec forbids.

## Consequences

- Not listed, ScreenGauge counts as off whatever `StartupApproved` kept: Windows shows that choice only for a listed program. Turning it on lists it; should Windows still have it off, the switch stays off, the amber message shows, and Settings > Apps > Startup now lists ScreenGauge where the user can turn it on.
- The value holds the path of the `.exe` that wrote it. The installer copy always runs from the same folder, so an update keeps it right; a build run from elsewhere that turns the option on points it at itself.
- The uninstaller must delete the `Run` value (WPC-337); `StartupApproved` is Windows' and stays.
- The Store copy starts with its package's startup task instead, with the same rule of respecting Windows (WPC-336).
