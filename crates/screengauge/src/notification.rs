//! Windows notifications: the system's messages in the corner of the screen, for what goes wrong while no tool is open
//! (a notice is ScreenGauge's own message on the frozen screen, a different thing). They are the tray icon's balloons,
//! which Windows 11 shows as its own notifications (ADR 0003), and their click comes back to the resident window.

use std::cell::RefCell;

use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::Shell::{NIN_BALLOONHIDE, NIN_BALLOONTIMEOUT, NIN_BALLOONUSERCLICK};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_APP};

use crate::{log, tray};

/// Posted to the resident window to show the notifications waiting, once the messages posted before it are handled.
/// WM_APP + 3 is taken by the frozen screen's "another program in front" (WPC-302).
pub const SHOW_WAITING: u32 = WM_APP + 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Notification {
    /// The photo of some monitor failed through both paths, so the tools didn't open (v1 spec WPC-296, Frozen screen).
    CaptureFailed,
    /// The settings file was damaged, and ScreenGauge went back to the defaults (v1 spec WPC-296, Settings file).
    SettingsReset,
}

impl Notification {
    fn text(self) -> &'static str {
        match self {
            Notification::CaptureFailed => "Couldn't capture the screen. Try again.",
            Notification::SettingsReset => {
                "Your settings were damaged and have been reset. A copy was kept."
            }
        }
    }

    /// Whether it means something only right away: "Try again" is about the call just made, and shown later, once
    /// the taskbar is back, it would be about nothing.
    fn right_away_only(self) -> bool {
        matches!(self, Notification::CaptureFailed)
    }

    /// What its click does. The spec gives these two no action: Windows closes the notification, and that is all.
    fn clicked(self) {
        match self {
            Notification::CaptureFailed | Notification::SettingsReset => {}
        }
    }
}

thread_local! {
    /// Only the resident window's thread shows notifications and receives their events.
    static STATE: RefCell<State> = RefCell::new(State::default());
}

/// Shows `notification` now, or once the tools close. When the tray icon can't show it, it waits for the icon, unless
/// it means something only right away.
pub fn show(window: HWND, notification: Notification) {
    STATE.with_borrow_mut(|state| state.show(notification, balloon(window)));
}

/// Keeps `notification` until [`SHOW_WAITING`], the tools closing or the tray icon's return shows it.
pub fn wait(notification: Notification) {
    STATE.with_borrow_mut(|state| state.waiting.push(notification));
}

/// The frozen screen is on the monitors: notifications wait until [`tools_closed`].
pub fn tools_opened() {
    STATE.with_borrow_mut(State::tools_opened);
}

/// The frozen screen is gone: the notifications waiting show on `window`'s tray icon.
pub fn tools_closed(window: HWND) {
    STATE.with_borrow_mut(|state| state.tools_closed(balloon(window)));
}

/// Shows the notifications waiting once the messages already posted to `window` are handled: at startup, after the
/// call's photo, so a notification never lands in it; when the call opened the tools, after they close.
pub fn post_show_waiting(window: HWND) {
    // SAFETY: the resident window; the message carries nothing.
    if let Err(e) = unsafe { PostMessageW(Some(window), SHOW_WAITING, WPARAM(0), LPARAM(0)) } {
        log::failure("notification: posting the ones waiting", &e);
    }
}

/// Shows every notification waiting; those the tray icon can't show yet keep waiting.
pub fn show_waiting(window: HWND) {
    STATE.with_borrow_mut(|state| state.show_waiting(balloon(window)));
}

/// Takes a tray icon event (the low word of its callback's lParam) that concerns a notification.
pub fn on_tray_event(event: u32) {
    // The click runs once the state is no longer borrowed, since an action may show another notification.
    if let Some(clicked) = STATE.with_borrow_mut(|state| state.on_tray_event(event)) {
        clicked.clicked();
    }
}

/// The tray icon's balloon on `window`, for [`State`].
fn balloon(window: HWND) -> impl FnMut(Notification) -> windows::core::Result<()> {
    move |notification| tray::balloon(window, notification.text(), notification.right_away_only())
}

#[derive(Debug, Default)]
struct State {
    /// Waiting for the tray icon, for the startup call's photo or for the tools to close, in the order they came.
    waiting: Vec<Notification>,
    /// The last one Windows took, whose click comes back as NIN_BALLOONUSERCLICK.
    showing: Option<Notification>,
    /// The frozen screen covers the monitors. Windows drops a notification while a full-screen window is in front
    /// (SHQueryUserNotificationState says QUNS_BUSY), so none shows until the tools close.
    tools_open: bool,
}

impl State {
    fn tools_opened(&mut self) {
        self.tools_open = true;
    }

    fn tools_closed(&mut self, balloon: impl FnMut(Notification) -> windows::core::Result<()>) {
        self.tools_open = false;
        self.show_waiting(balloon);
    }

    fn show(
        &mut self,
        notification: Notification,
        mut balloon: impl FnMut(Notification) -> windows::core::Result<()>,
    ) {
        if self.tools_open {
            self.waiting.push(notification);
            return;
        }
        match balloon(notification) {
            Ok(()) => self.showing = Some(notification),
            Err(e) if notification.right_away_only() => {
                log::failure(&format!("notification: {notification:?} dropped"), &e);
            }
            Err(e) => {
                log::failure(
                    &format!("notification: {notification:?} waits for the tray icon"),
                    &e,
                );
                self.waiting.push(notification);
            }
        }
    }

    fn show_waiting(&mut self, mut balloon: impl FnMut(Notification) -> windows::core::Result<()>) {
        if self.tools_open {
            return;
        }
        for notification in std::mem::take(&mut self.waiting) {
            self.show(notification, &mut balloon);
        }
    }

    /// The notification clicked, when `event` is a click on one.
    fn on_tray_event(&mut self, event: u32) -> Option<Notification> {
        match event {
            NIN_BALLOONUSERCLICK => self.showing.take(),
            NIN_BALLOONTIMEOUT | NIN_BALLOONHIDE => {
                self.showing = None;
                None
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use windows::Win32::Foundation::E_FAIL;
    use windows::Win32::UI::Shell::NIN_SELECT;
    use windows::core::Error;

    use super::*;

    const EVERY_NOTIFICATION: [Notification; 2] =
        [Notification::CaptureFailed, Notification::SettingsReset];

    /// A tray icon that shows every notification, recording them.
    fn shown_into(
        shown: &mut Vec<Notification>,
    ) -> impl FnMut(Notification) -> windows::core::Result<()> {
        |notification| {
            shown.push(notification);
            Ok(())
        }
    }

    /// The tray icon isn't in the notification area, as before the taskbar exists.
    fn no_icon(_: Notification) -> windows::core::Result<()> {
        Err(Error::new(
            E_FAIL,
            "Shell_NotifyIconW(NIM_MODIFY) returned FALSE",
        ))
    }

    #[test]
    fn the_texts_are_the_specs() {
        let texts: Vec<_> = EVERY_NOTIFICATION.iter().map(|n| n.text()).collect();
        assert_eq!(
            texts,
            [
                "Couldn't capture the screen. Try again.",
                "Your settings were damaged and have been reset. A copy was kept.",
            ]
        );
    }

    #[test]
    fn every_text_fits_in_a_balloon() {
        for notification in EVERY_NOTIFICATION {
            assert!(
                tray::fits_in_a_balloon(notification.text()),
                "{notification:?}"
            );
        }
    }

    #[test]
    fn a_notification_shows_at_once_when_the_tray_icon_is_there() {
        let mut state = State::default();
        let mut shown = Vec::new();
        state.show(Notification::CaptureFailed, shown_into(&mut shown));
        assert_eq!(shown, [Notification::CaptureFailed]);
        assert!(state.waiting.is_empty());
    }

    #[test]
    fn a_capture_failure_the_tray_icon_cant_show_is_dropped() {
        // Later it would be about a call long gone.
        let mut state = State::default();
        state.show(Notification::CaptureFailed, no_icon);
        let mut shown = Vec::new();
        state.show_waiting(shown_into(&mut shown));
        assert_eq!(shown, []);
    }

    #[test]
    fn a_settings_reset_waits_for_the_tray_icon() {
        let mut state = State::default();
        state.show(Notification::SettingsReset, no_icon);
        state.show_waiting(no_icon);
        assert_eq!(state.waiting, [Notification::SettingsReset]);
        let mut shown = Vec::new();
        state.show_waiting(shown_into(&mut shown));
        assert_eq!(shown, [Notification::SettingsReset]);
        assert!(state.waiting.is_empty());
    }

    #[test]
    fn a_notification_waits_while_the_tools_are_open_and_shows_once_they_close() {
        // Windows drops a notification while the frozen screen covers the monitors, as for a full-screen program.
        let mut state = State::default();
        state.tools_opened();
        let mut shown = Vec::new();
        state.show(Notification::SettingsReset, shown_into(&mut shown));
        state.show_waiting(shown_into(&mut shown));
        assert_eq!(shown, []);
        state.tools_closed(shown_into(&mut shown));
        assert_eq!(shown, [Notification::SettingsReset]);
        assert!(state.waiting.is_empty());
    }

    #[test]
    fn notifications_waiting_show_in_the_order_they_came() {
        let mut state = State {
            waiting: vec![Notification::SettingsReset, Notification::CaptureFailed],
            ..State::default()
        };
        let mut shown = Vec::new();
        state.show_waiting(shown_into(&mut shown));
        assert_eq!(
            shown,
            [Notification::SettingsReset, Notification::CaptureFailed]
        );
    }

    #[test]
    fn a_click_comes_back_to_the_notification_shown() {
        let mut state = State::default();
        state.show(Notification::SettingsReset, |_| Ok(()));
        assert_eq!(state.on_tray_event(NIN_SELECT), None);
        assert_eq!(
            state.on_tray_event(NIN_BALLOONUSERCLICK),
            Some(Notification::SettingsReset)
        );
        // One click per notification.
        assert_eq!(state.on_tray_event(NIN_BALLOONUSERCLICK), None);
    }

    #[test]
    fn a_notification_that_went_away_takes_no_click() {
        for gone in [NIN_BALLOONTIMEOUT, NIN_BALLOONHIDE] {
            let mut state = State::default();
            state.show(Notification::CaptureFailed, |_| Ok(()));
            assert_eq!(state.on_tray_event(gone), None);
            assert_eq!(state.on_tray_event(NIN_BALLOONUSERCLICK), None, "{gone}");
        }
    }
}
