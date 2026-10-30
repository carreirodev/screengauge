# Windows notifications are the tray icon's balloons, for now

ScreenGauge's Windows notifications are balloons on its own tray icon: `Shell_NotifyIconW` with `NIF_INFO`, which Windows 11 shows as one of its own notifications in the corner of the screen. The tray icon is there in the Store copy and the installer copy alike, so the balloon needs no registration, no shortcut and no new dependency (`Win32_UI_Shell` was already a feature of the `windows` crate), and its click comes back to the resident window as `NIN_BALLOONUSERCLICK` on the tray icon's callback. The text is the message alone, with no title. Decided in WPC-303; replacing it with Windows' standard notifications, which persist, is WPC-347, chosen by the owner on 2026-10-30.

## Considered Options

- **Windows' standard notifications (`Windows.UI.Notifications`, through the same `windows` crate)**: they stay in the notification center and wait out "Do not disturb". An unpackaged copy needs an AppUserModelID registered, through a Start menu shortcut the installer makes or in the registry, and a click from the notification center needs a COM activator, declared in the MSIX manifest for the Store copy. Neither the installer nor the package exists yet, and the open questions need a test of their own, so it went to WPC-347.
- **The Windows App SDK's `AppNotificationManager`**: it does the registration itself, but it is the 120 MB Windows App Runtime ADR 0001 rejects.
- **A message ScreenGauge draws itself**: the spec (WPC-296) and ADR 0001 make these messages Windows notifications, owned by the system.

## Consequences

Measured on the owner's computer with a one-off probe and the real build (Windows 11 build 26300, 2026-10-30):

- The notification shows the executable's name, "ScreenGauge.exe", with the generic icon: the `.exe` has no version resource yet. Giving it one, with the app icon (WPC-304), should show "ScreenGauge" (expected, not confirmed).
- It stays about 6 seconds and is **not kept** in the notification center (Win + N), even while ScreenGauge runs.
- A click delivers `NIN_BALLOONUSERCLICK` and nothing else, so it never calls ScreenGauge as a click on the icon would. Going away delivers `NIN_BALLOONTIMEOUT`, sometimes twice.
- With "Do not disturb" on, or with a full-screen window in front (`SHQueryUserNotificationState` says `QUNS_BUSY`), Windows drops it: `NIN_BALLOONTIMEOUT` arrives 40 ms after `NIN_BALLOONSHOW`, and nothing shows or is kept, with or without `NIF_REALTIME`. The frozen screen counts as such a window, so every notification waits while the tools are open and shows once they close.
- It shows with the tray icon inside the hidden icons panel too.
- A notification that can't show because the tray icon isn't there yet, as at sign-in, waits for the icon (`TaskbarCreated`), except "Couldn't capture the screen. Try again.", which means something only right away.
- `NIIF_RESPECT_QUIET_TIME` is never set: during the first hour after a new account signs in, it would drop the notification unshown.
- The Store copy is expected to behave the same, since the tray icon is the same; that is checked with the package (WPC-334).
