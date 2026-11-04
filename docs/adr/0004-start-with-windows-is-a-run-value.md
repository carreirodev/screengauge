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
- The Store copy starts with its package's startup task instead, with the same rule of respecting Windows (WPC-336); see below.

## The Store copy: the package's startup task

A packaged app's `Run` value would hold the path inside `WindowsApps`, whose folder name carries the package's version, so it would break at the first Store update. The Store copy declares a startup task in its manifest instead (`desktop:StartupTask`, ID `ScreenGauge`, off), and never writes the `Run` value; not knowing which copy it is, ScreenGauge says so and changes nothing. Turning the option on asks Windows to enable the task, turning it off disables it, and the switch shows the task's state, which Windows reads at each call: `Enabled` is on, `Disabled` off, and `DisabledByUser`, the user's choice in Task Manager or Settings > Apps > Startup, is the same "turned off in Windows" as `StartupApproved`'s odd byte, with the same amber message. Windows never enables a task the user disabled, so the rule needs nothing more of ScreenGauge. A group policy's `DisabledByPolicy` shows as turned off in Windows too, and `EnabledByPolicy` as on.

The task starts `ScreenGauge.exe` with the manifest's `uap10:Parameters`, `--sign-in`, as the `Run` value does. Should Windows not pass them, the Store copy also asks Windows how it started (`AppInstance.GetActivatedEventArgs`), and a start by the startup task is a start at sign-in; DebugView shows both answers. Windows runs the task only once the package's app has been opened at least once.
