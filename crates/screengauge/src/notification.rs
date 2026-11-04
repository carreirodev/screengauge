//! Windows notifications: the system's messages in the corner of the screen, for what goes wrong while no tool is open
//! (a notice is ScreenGauge's own message on the frozen screen, a different thing). They are Windows' standard
//! notifications (ADR 0003): each stays in the notification center until the user closes it or Windows takes it out,
//! and its click comes back to the resident window through the activator, with ScreenGauge running or not.

mod activator;
mod toast;

/// For the tests of the Store copy's manifest (`package.rs`), which must name the same class as the code registers.
#[cfg(test)]
pub use toast::STORE_COPY_CLSID;

use std::cell::RefCell;
use std::ffi::c_void;
use std::sync::atomic::{AtomicPtr, Ordering};
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use screengauge_core::settings::Shortcut;
use windows::Win32::Foundation::{E_FAIL, HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_APP};
use windows::core::Error;

use crate::{com, log, package, settings_file};

/// Posted to the resident window to show the notifications waiting, once the messages posted before it are handled.
/// WM_APP + 3 is taken by the frozen screen's "another program in front" (WPC-302).
pub const SHOW_WAITING: u32 = WM_APP + 4;

/// Posted to the resident window when Windows told something about a notification: its click, or that it went away.
/// WM_APP + 1 to + 11 are taken.
pub const EVENTS_READY: u32 = WM_APP + 12;

/// A notification Windows dismissed sooner than this after it was shown never appeared in the corner: with "Do not
/// disturb" on or a full-screen program in front, Windows dismisses it within about 10 ms and keeps it only in the
/// notification center, while one that appeared stays 6 to 9 s (WPC-347's test, 2026-11-02).
const APPEARED_AFTER: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Notification {
    /// The photo of some monitor failed through both paths, so the tools didn't open (v1 spec WPC-296, Frozen screen).
    CaptureFailed,
    /// The settings file was damaged, and ScreenGauge went back to the defaults (v1 spec WPC-296, Settings file).
    SettingsReset,
    /// Windows refused the global shortcut at startup: another program holds it, or Windows took it (WPC-324). Its
    /// click opens Settings, to choose another one.
    ShortcutBusy,
}

const EVERY_NOTIFICATION: [Notification; 3] = [
    Notification::CaptureFailed,
    Notification::SettingsReset,
    Notification::ShortcutBusy,
];

impl Notification {
    /// Its text as it shows now: the busy shortcut's names the global shortcut the settings file holds.
    fn text(self) -> windows::core::Result<String> {
        Ok(match self {
            Notification::CaptureFailed => "Couldn't capture the screen. Try again.".to_owned(),
            Notification::SettingsReset => {
                "Your settings were damaged and have been reset. A copy was kept.".to_owned()
            }
            Notification::ShortcutBusy => {
                let stored =
                    settings_file::current().map_err(|e| Error::new(E_FAIL, e.to_string()))?;
                shortcut_busy_text(stored.options.global_shortcut)
            }
        })
    }

    /// What names it to Windows, as its tag, and in its click's arguments.
    fn key(self) -> &'static str {
        match self {
            Notification::CaptureFailed => "capture-failed",
            Notification::SettingsReset => "settings-reset",
            Notification::ShortcutBusy => "shortcut-busy",
        }
    }

    /// The notification a click's arguments name, whichever process showed it.
    fn of_launch(launch: &str) -> Option<Notification> {
        let key = launch.split_once(':').map_or(launch, |(key, _)| key);
        EVERY_NOTIFICATION.into_iter().find(|n| n.key() == key)
    }

    /// Whether it means something only right away: "Try again" is about the call just made, and shown later, once
    /// Windows takes notifications again, it would be about nothing.
    fn right_away_only(self) -> bool {
        matches!(self, Notification::CaptureFailed)
    }

    /// Whether its click opens Settings. The spec gives the others no action: Windows closes the notification and
    /// ScreenGauge stays running, as when the click started it.
    pub fn opens_settings(self) -> bool {
        match self {
            Notification::ShortcutBusy => true,
            Notification::CaptureFailed | Notification::SettingsReset => false,
        }
    }
}

/// The busy shortcut's text for `shortcut` (v1 spec WPC-296; WPC-324).
fn shortcut_busy_text(shortcut: Shortcut) -> String {
    format!(
        "{shortcut} is already used by another program. Click to choose another one in Settings."
    )
}

/// Whether a notification appeared in the corner of the screen, or went only to the notification center.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Seen {
    InCorner,
    CenterOnly,
}

/// A notification as handed to Windows: `launch` is what its click gives back, unique to this showing.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Toast {
    notification: Notification,
    launch: String,
}

/// What Windows told about the notification shown with `launch`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Event {
    launch: String,
    what: What,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum What {
    /// Clicked, in the corner or in the notification center.
    Clicked,
    /// Gone from the corner, `after` it was handed to Windows.
    Dismissed { reason: Dismissal, after: Duration },
    /// Windows couldn't show it.
    Failed(String),
}

/// Why a notification left the corner of the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Dismissal {
    /// The user closed it.
    UserCanceled,
    /// ScreenGauge took it out.
    ApplicationHidden,
    /// Its time in the corner ran out, or it never got any.
    TimedOut,
}

/// What [`State`] asks of Windows.
trait Toasts {
    fn show(&mut self, toast: &Toast) -> windows::core::Result<()>;
    /// Takes `notification` out of the notification center.
    fn remove(&mut self, notification: Notification) -> windows::core::Result<()>;
}

/// Windows' own notifications, for the running ScreenGauge.
struct Windows;

impl Toasts for Windows {
    fn show(&mut self, toast: &Toast) -> windows::core::Result<()> {
        toast::show(toast)
    }

    fn remove(&mut self, notification: Notification) -> windows::core::Result<()> {
        toast::remove(notification)
    }
}

thread_local! {
    /// Only the resident window's thread shows notifications and handles their events. It is the main thread, which
    /// also opens the settings file before the resident window exists, so what that queues is here for the window.
    static STATE: RefCell<State> = RefCell::new(State::new(std::process::id()));
}

/// The resident window, which the events Windows tells on threads of its own are posted to.
static RESIDENT: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());

/// The events told and not yet handled by the resident window, in the order they came.
static EVENTS: Mutex<Vec<Event>> = Mutex::new(Vec::new());

/// Gets notifications ready as ScreenGauge starts, once the resident window `window` exists: registers ScreenGauge
/// with Windows and the activator with COM, so a click comes to this process, even when Windows started it for that
/// click. Without the activator, notifications still show, and their click in the notification center does nothing.
pub fn start(window: HWND) {
    RESIDENT.store(window.0, Ordering::Release);
    let copy = match package::this_copy() {
        Ok(copy) => copy,
        // Neither copy's class nor name can be told apart, so nothing is registered: the notifications then fail to
        // show, as they would without a name, and wait.
        Err(e) => {
            log::failure("notification: finding which copy this is", &e);
            return;
        }
    };
    let activator = com::ensure_started().and_then(|()| activator::register(toast::clsid(copy)));
    if let Err(e) = &activator {
        log::failure("notification: registering the activator", e);
    }
    if let Err(e) = toast::register(copy, activator.is_ok()) {
        log::failure("notification: registering ScreenGauge with Windows", &e);
    }
}

/// Takes the activator back as ScreenGauge exits. The registration in Windows stays, so a click on a notification
/// left in the notification center starts ScreenGauge.
pub fn stop() {
    if let Err(e) = activator::revoke() {
        log::failure("exit: revoking the notifications' activator", &e);
    }
}

/// Shows `notification` now, or once the tools close. When Windows refuses it, it waits for the next chance (see
/// [`show_waiting`]), unless it means something only right away.
pub fn show(notification: Notification) {
    STATE.with_borrow_mut(|state| state.show(notification, &mut Windows));
}

/// Keeps `notification` until [`SHOW_WAITING`] or the tools closing shows it.
pub fn wait(notification: Notification) {
    STATE.with_borrow_mut(|state| state.waiting.push(notification));
}

/// The frozen screen is on the monitors: notifications wait until [`tools_closed`].
pub fn tools_opened() {
    STATE.with_borrow_mut(State::tools_opened);
}

/// The frozen screen is gone: the notifications waiting show.
pub fn tools_closed() {
    STATE.with_borrow_mut(|state| state.tools_closed(&mut Windows));
}

/// A call captured the screen: "Couldn't capture the screen. Try again." leaves the notification center.
pub fn capture_succeeded() {
    STATE.with_borrow_mut(|state| state.capture_succeeded(&mut Windows));
}

/// Shows the notifications waiting once the messages already posted to `window` are handled: at startup, after the
/// call's photo, so a notification never lands in it; when the call opened the tools, after they close.
pub fn post_show_waiting(window: HWND) {
    // SAFETY: the resident window; the message carries nothing.
    if let Err(e) = unsafe { PostMessageW(Some(window), SHOW_WAITING, WPARAM(0), LPARAM(0)) } {
        log::failure("notification: posting the ones waiting", &e);
    }
}

/// Shows every notification waiting; those Windows refuses keep waiting. Comes at startup, when the tools close and
/// when the taskbar is created, as at sign-in.
pub fn show_waiting() {
    STATE.with_borrow_mut(|state| state.show_waiting(&mut Windows));
}

/// Whether `notification`, the latest shown, appeared in the corner of the screen or went only to the notification
/// center; `None` until Windows tells, or when none was shown.
// TODO(WPC-339): "already told" reads this; drop the expect then.
#[expect(
    dead_code,
    reason = "WPC-339 is its first reader; until then what it would say is logged"
)]
pub fn seen(notification: Notification) -> Option<Seen> {
    STATE.with_borrow(|state| state.seen(notification))
}

/// Handles what Windows told about notifications since the last time ([`EVENTS_READY`]), and returns the ones
/// clicked, in the order they were, for the resident window to do what their click asks.
pub fn on_events_ready() -> Vec<Notification> {
    let events = std::mem::take(&mut *EVENTS.lock().unwrap_or_else(PoisonError::into_inner));
    events
        .into_iter()
        .filter_map(|event| STATE.with_borrow_mut(|state| state.on_event(event)))
        .collect()
}

/// The global shortcut works now: the busy shortcut's notification no longer waits, and leaves the notification center,
/// should this process or an earlier ScreenGauge have left it there.
pub fn shortcut_free() {
    STATE.with_borrow_mut(|state| state.shortcut_free(&mut Windows));
}

/// Hands `event`, told on any thread, to the resident window.
fn report(event: Event) -> windows::core::Result<()> {
    EVENTS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push(event);
    let window = RESIDENT.load(Ordering::Acquire);
    // Never so far: the window is kept before the activator is registered and before any notification shows. A null
    // window would post to this thread, whose message would be lost.
    if window.is_null() {
        return Err(Error::new(E_FAIL, "no resident window to tell"));
    }
    // SAFETY: the resident window; the message carries nothing. Should it be gone, as at exit, the call fails, and the
    // event stays queued for nothing.
    unsafe { PostMessageW(Some(HWND(window)), EVENTS_READY, WPARAM(0), LPARAM(0)) }
}

/// The latest notification of a kind shown by this process, and what Windows told of it.
#[derive(Debug)]
struct Shown {
    toast: Toast,
    seen: Option<Seen>,
}

#[derive(Debug)]
struct State {
    /// This process, in each `launch`, so a click on a notification an earlier ScreenGauge showed is never taken for one
    /// of this process's.
    process: u32,
    /// How many notifications this process has shown.
    count: u64,
    /// Waiting for the startup call's photo, for the tools to close or for Windows to take them, in the order they came.
    waiting: Vec<Notification>,
    /// The latest of each kind shown.
    shown: Vec<Shown>,
    /// The frozen screen covers the monitors. Windows takes a full-screen window in front for a reason to keep
    /// notifications out of the corner (`QUNS_BUSY`), so none shows until the tools close.
    tools_open: bool,
    /// "Couldn't capture the screen. Try again." may be in the notification center, still to leave it: shown by this
    /// process, or by an earlier ScreenGauge, so true at startup. Taking out one that isn't there does nothing.
    capture_failed_in_center: bool,
}

impl State {
    fn new(process: u32) -> State {
        State {
            process,
            count: 0,
            waiting: Vec::new(),
            shown: Vec::new(),
            tools_open: false,
            capture_failed_in_center: true,
        }
    }

    fn tools_opened(&mut self) {
        self.tools_open = true;
    }

    fn tools_closed(&mut self, toasts: &mut impl Toasts) {
        self.tools_open = false;
        self.show_waiting(toasts);
    }

    fn show(&mut self, notification: Notification, toasts: &mut impl Toasts) {
        if self.tools_open {
            self.waiting.push(notification);
            return;
        }
        self.count += 1;
        let toast = Toast {
            notification,
            launch: format!("{}:{}.{}", notification.key(), self.process, self.count),
        };
        match toasts.show(&toast) {
            Ok(()) => {
                if notification == Notification::CaptureFailed {
                    self.capture_failed_in_center = true;
                }
                self.shown
                    .retain(|shown| shown.toast.notification != notification);
                self.shown.push(Shown { toast, seen: None });
            }
            Err(e) => self.refused(notification, &e),
        }
    }

    /// Windows didn't take `notification`: it waits for the next chance, unless it means something only right away.
    fn refused(&mut self, notification: Notification, error: &impl std::fmt::Display) {
        if notification.right_away_only() {
            log::failure(&format!("notification: {notification:?} dropped"), error);
        } else {
            log::failure(&format!("notification: {notification:?} waits"), error);
            self.waiting.push(notification);
        }
    }

    fn show_waiting(&mut self, toasts: &mut impl Toasts) {
        if self.tools_open {
            return;
        }
        for notification in std::mem::take(&mut self.waiting) {
            self.show(notification, toasts);
        }
    }

    fn capture_succeeded(&mut self, toasts: &mut impl Toasts) {
        if !std::mem::take(&mut self.capture_failed_in_center) {
            return;
        }
        if let Err(e) = toasts.remove(Notification::CaptureFailed) {
            log::failure("notification: taking the capture failure out", &e);
        }
    }

    fn shortcut_free(&mut self, toasts: &mut impl Toasts) {
        self.waiting
            .retain(|waiting| *waiting != Notification::ShortcutBusy);
        if let Err(e) = toasts.remove(Notification::ShortcutBusy) {
            log::failure("notification: taking the busy shortcut out", &e);
        }
    }

    fn seen(&self, notification: Notification) -> Option<Seen> {
        self.shown
            .iter()
            .find(|shown| shown.toast.notification == notification)?
            .seen
    }

    /// Takes `event`, and returns the notification clicked, when it is a click: one this process showed, or one an
    /// earlier ScreenGauge did, as when Windows started this process for the click.
    fn on_event(&mut self, event: Event) -> Option<Notification> {
        let shown = self
            .shown
            .iter_mut()
            .find(|shown| shown.toast.launch == event.launch);
        match event.what {
            What::Clicked => {
                if let Some(shown) = shown {
                    // Clicked before anything else was told: it was in the corner.
                    decide(shown, Seen::InCorner);
                }
                let clicked = Notification::of_launch(&event.launch);
                // Settings, which the click opens, says the shortcut is busy: one waiting, as when Windows started
                // ScreenGauge for the click and the startup found it busy again, would say it twice.
                if clicked == Some(Notification::ShortcutBusy) {
                    self.waiting
                        .retain(|waiting| *waiting != Notification::ShortcutBusy);
                }
                match clicked {
                    Some(notification) => {
                        log::note(&format!("notification: {notification:?} clicked"))
                    }
                    None => log::failure(
                        "notification: a click",
                        &format!("no notification is {:?}", event.launch),
                    ),
                }
                clicked
            }
            What::Dismissed { reason, after } => {
                let seen = match reason {
                    Dismissal::TimedOut if after < APPEARED_AFTER => Some(Seen::CenterOnly),
                    Dismissal::TimedOut | Dismissal::UserCanceled => Some(Seen::InCorner),
                    // Taken out by ScreenGauge: whatever it was, nothing more is learned.
                    Dismissal::ApplicationHidden => None,
                };
                if let (Some(shown), Some(seen)) = (shown, seen) {
                    decide(shown, seen);
                }
                None
            }
            What::Failed(error) => {
                if let Some(shown) = shown {
                    let notification = shown.toast.notification;
                    self.shown
                        .retain(|shown| shown.toast.launch != event.launch);
                    if notification == Notification::CaptureFailed {
                        self.capture_failed_in_center = false;
                    }
                    self.refused(notification, &error);
                }
                None
            }
        }
    }
}

/// Keeps `seen` for `shown`, unless Windows already told something of it: the first thing it tells decides, as a
/// notification gone to the notification center at once may still be clicked there later.
fn decide(shown: &mut Shown, seen: Seen) {
    if shown.seen.is_some() {
        return;
    }
    shown.seen = Some(seen);
    let notification = shown.toast.notification;
    log::note(&match seen {
        Seen::InCorner => format!("notification: {notification:?} appeared in the corner"),
        Seen::CenterOnly => {
            format!("notification: {notification:?} went only to the notification center")
        }
    });
}

#[cfg(test)]
mod tests {
    use screengauge_core::settings::{Key, Modifiers};
    use windows::Win32::Foundation::E_FAIL;
    use windows::core::Error;

    use super::*;

    /// Windows' notifications, recorded.
    #[derive(Default)]
    struct Recorded {
        shown: Vec<Toast>,
        removed: Vec<Notification>,
        /// Windows refuses every notification, as it might before the taskbar exists.
        refusing: bool,
    }

    impl Recorded {
        fn refusing() -> Recorded {
            Recorded {
                refusing: true,
                ..Recorded::default()
            }
        }

        fn notifications(&self) -> Vec<Notification> {
            self.shown.iter().map(|toast| toast.notification).collect()
        }

        fn launch_of_last(&self) -> String {
            self.shown
                .last()
                .expect("a notification shown")
                .launch
                .clone()
        }
    }

    impl Toasts for Recorded {
        fn show(&mut self, toast: &Toast) -> windows::core::Result<()> {
            if self.refusing {
                return Err(Error::new(E_FAIL, "ToastNotifier.Show failed"));
            }
            self.shown.push(toast.clone());
            Ok(())
        }

        fn remove(&mut self, notification: Notification) -> windows::core::Result<()> {
            self.removed.push(notification);
            Ok(())
        }
    }

    fn state() -> State {
        State::new(4321)
    }

    #[test]
    fn the_texts_are_the_specs() {
        // The busy shortcut's names the shortcut of the moment: see the_busy_shortcut_names_the_shortcut_of_the_moment.
        assert_eq!(
            Notification::CaptureFailed.text().expect("a fixed text"),
            "Couldn't capture the screen. Try again."
        );
        assert_eq!(
            Notification::SettingsReset.text().expect("a fixed text"),
            "Your settings were damaged and have been reset. A copy was kept."
        );
    }

    #[test]
    fn a_notification_shows_at_once() {
        let mut state = state();
        let mut windows = Recorded::default();
        state.show(Notification::CaptureFailed, &mut windows);
        assert_eq!(windows.notifications(), [Notification::CaptureFailed]);
        assert!(state.waiting.is_empty());
    }

    #[test]
    fn a_notification_waits_while_the_tools_are_open_and_shows_once_they_close() {
        // The frozen screen counts as a full-screen program: shown now, it would go silently to the notification center.
        let mut state = state();
        let mut windows = Recorded::default();
        state.tools_opened();
        state.show(Notification::SettingsReset, &mut windows);
        state.show_waiting(&mut windows);
        assert_eq!(windows.notifications(), []);
        state.tools_closed(&mut windows);
        assert_eq!(windows.notifications(), [Notification::SettingsReset]);
        assert!(state.waiting.is_empty());
    }

    #[test]
    fn notifications_waiting_show_in_the_order_they_came() {
        let mut state = state();
        state.waiting = vec![Notification::SettingsReset, Notification::CaptureFailed];
        let mut windows = Recorded::default();
        state.show_waiting(&mut windows);
        assert_eq!(
            windows.notifications(),
            [Notification::SettingsReset, Notification::CaptureFailed]
        );
    }

    #[test]
    fn a_capture_failure_windows_refuses_is_dropped() {
        // Later it would be about a call long gone.
        let mut state = state();
        state.show(Notification::CaptureFailed, &mut Recorded::refusing());
        let mut windows = Recorded::default();
        state.show_waiting(&mut windows);
        assert_eq!(windows.notifications(), []);
    }

    #[test]
    fn a_settings_reset_windows_refuses_waits_for_the_next_chance() {
        let mut state = state();
        state.show(Notification::SettingsReset, &mut Recorded::refusing());
        state.show_waiting(&mut Recorded::refusing());
        assert_eq!(state.waiting, [Notification::SettingsReset]);
        let mut windows = Recorded::default();
        state.show_waiting(&mut windows);
        assert_eq!(windows.notifications(), [Notification::SettingsReset]);
        assert!(state.waiting.is_empty());
    }

    /// The state once the first capture since startup has taken out what an earlier ScreenGauge may have left.
    fn state_after_first_capture() -> State {
        let mut state = state();
        state.capture_succeeded(&mut Recorded::default());
        state
    }

    #[test]
    fn the_first_capture_takes_out_one_an_earlier_screengauge_left() {
        // ScreenGauge exited, or Windows started it for a click, after a capture failed.
        let mut state = state();
        let mut windows = Recorded::default();
        state.capture_succeeded(&mut windows);
        state.capture_succeeded(&mut windows);
        assert_eq!(windows.removed, [Notification::CaptureFailed]);
    }

    #[test]
    fn the_capture_failure_leaves_the_notification_center_once_a_capture_succeeds() {
        let mut state = state_after_first_capture();
        let mut windows = Recorded::default();
        state.show(Notification::CaptureFailed, &mut windows);
        state.capture_succeeded(&mut windows);
        assert_eq!(windows.removed, [Notification::CaptureFailed]);
        // Once: the next captures have nothing to take out.
        state.capture_succeeded(&mut windows);
        assert_eq!(windows.removed, [Notification::CaptureFailed]);
    }

    #[test]
    fn a_capture_takes_nothing_out_when_no_capture_failed() {
        let mut state = state_after_first_capture();
        let mut windows = Recorded::default();
        state.show(Notification::SettingsReset, &mut windows);
        state.capture_succeeded(&mut windows);
        assert_eq!(windows.removed, []);
    }

    #[test]
    fn a_capture_failure_windows_refused_has_nothing_to_leave() {
        let mut state = state_after_first_capture();
        state.show(Notification::CaptureFailed, &mut Recorded::refusing());
        let mut windows = Recorded::default();
        state.capture_succeeded(&mut windows);
        assert_eq!(windows.removed, []);
    }

    /// `notification`, shown, and then what Windows told of it.
    fn seen_after(notification: Notification, told: What) -> Option<Seen> {
        let mut state = state();
        let mut windows = Recorded::default();
        state.show(notification, &mut windows);
        state.on_event(Event {
            launch: windows.launch_of_last(),
            what: told,
        });
        state.seen(notification)
    }

    fn dismissed(reason: Dismissal, after_ms: u64) -> What {
        What::Dismissed {
            reason,
            after: Duration::from_millis(after_ms),
        }
    }

    #[test]
    fn dismissed_at_once_it_went_only_to_the_notification_center() {
        // "Do not disturb" on, or a full-screen program in front: measured 9 ms after Show.
        assert_eq!(
            seen_after(
                Notification::SettingsReset,
                dismissed(Dismissal::TimedOut, 9)
            ),
            Some(Seen::CenterOnly)
        );
    }

    #[test]
    fn its_time_in_the_corner_ran_out_so_it_appeared() {
        // Measured between 6 and 9 s after Show.
        assert_eq!(
            seen_after(
                Notification::SettingsReset,
                dismissed(Dismissal::TimedOut, 6000)
            ),
            Some(Seen::InCorner)
        );
    }

    #[test]
    fn the_cut_between_the_two_is_one_second() {
        assert_eq!(
            seen_after(
                Notification::SettingsReset,
                dismissed(Dismissal::TimedOut, 999)
            ),
            Some(Seen::CenterOnly)
        );
        assert_eq!(
            seen_after(
                Notification::SettingsReset,
                dismissed(Dismissal::TimedOut, 1000)
            ),
            Some(Seen::InCorner)
        );
    }

    #[test]
    fn closed_by_the_user_or_clicked_before_anything_else_it_appeared() {
        assert_eq!(
            seen_after(
                Notification::SettingsReset,
                dismissed(Dismissal::UserCanceled, 2000)
            ),
            Some(Seen::InCorner)
        );
        assert_eq!(
            seen_after(Notification::SettingsReset, What::Clicked),
            Some(Seen::InCorner)
        );
    }

    #[test]
    fn taken_out_by_screengauge_it_tells_nothing() {
        assert_eq!(
            seen_after(
                Notification::CaptureFailed,
                dismissed(Dismissal::ApplicationHidden, 9)
            ),
            None
        );
    }

    #[test]
    fn nothing_is_known_until_windows_tells() {
        let mut state = state();
        state.show(Notification::SettingsReset, &mut Recorded::default());
        assert_eq!(state.seen(Notification::SettingsReset), None);
        assert_eq!(state.seen(Notification::CaptureFailed), None);
    }

    #[test]
    fn the_first_thing_windows_tells_decides() {
        // Gone to the notification center at once, then clicked there an hour later: it never appeared in the corner.
        let mut state = state();
        let mut windows = Recorded::default();
        state.show(Notification::SettingsReset, &mut windows);
        let launch = windows.launch_of_last();
        for what in [dismissed(Dismissal::TimedOut, 9), What::Clicked] {
            state.on_event(Event {
                launch: launch.clone(),
                what,
            });
        }
        assert_eq!(
            state.seen(Notification::SettingsReset),
            Some(Seen::CenterOnly)
        );
    }

    #[test]
    fn what_windows_tells_of_an_older_one_leaves_the_latest_alone() {
        let mut state = state();
        let mut windows = Recorded::default();
        state.show(Notification::CaptureFailed, &mut windows);
        let older = windows.launch_of_last();
        state.show(Notification::CaptureFailed, &mut windows);
        state.on_event(Event {
            launch: older,
            what: dismissed(Dismissal::TimedOut, 9),
        });
        assert_eq!(state.seen(Notification::CaptureFailed), None);
    }

    #[test]
    fn a_click_comes_back_to_the_notification_clicked() {
        let mut state = state();
        let mut windows = Recorded::default();
        state.show(Notification::SettingsReset, &mut windows);
        state.show(Notification::CaptureFailed, &mut windows);
        let clicked = state.on_event(Event {
            launch: windows.shown[0].launch.clone(),
            what: What::Clicked,
        });
        assert_eq!(clicked, Some(Notification::SettingsReset));
    }

    #[test]
    fn a_click_on_one_an_earlier_screengauge_showed_comes_back_too() {
        // ScreenGauge was closed, and Windows started it for the click.
        let mut state = state();
        let clicked = state.on_event(Event {
            launch: "settings-reset:999.3".to_string(),
            what: What::Clicked,
        });
        assert_eq!(clicked, Some(Notification::SettingsReset));
        assert_eq!(state.seen(Notification::SettingsReset), None);
    }

    #[test]
    fn going_away_is_no_click() {
        let mut state = state();
        let mut windows = Recorded::default();
        state.show(Notification::SettingsReset, &mut windows);
        let clicked = state.on_event(Event {
            launch: windows.launch_of_last(),
            what: dismissed(Dismissal::UserCanceled, 2000),
        });
        assert_eq!(clicked, None);
    }

    #[test]
    fn the_busy_shortcut_names_the_shortcut_of_the_moment() {
        assert_eq!(
            shortcut_busy_text(Shortcut::default()),
            "Win + Shift + X is already used by another program. Click to choose another one in Settings."
        );
        let ctrl_alt_win_f9 = Shortcut::new(
            Modifiers {
                win: true,
                ctrl: true,
                alt: true,
                shift: false,
            },
            Key::Function(9),
        )
        .expect("allowed");
        assert_eq!(
            shortcut_busy_text(ctrl_alt_win_f9),
            "Win + Ctrl + Alt + F9 is already used by another program. Click to choose another one in Settings."
        );
    }

    #[test]
    fn the_busy_shortcut_waits_for_the_next_chance_when_windows_refuses_it() {
        // At sign-in, before the taskbar exists: it still has to reach the user.
        let mut state = state();
        state.show(Notification::ShortcutBusy, &mut Recorded::refusing());
        assert_eq!(state.waiting, [Notification::ShortcutBusy]);
    }

    #[test]
    fn a_free_shortcut_takes_the_busy_one_out_of_the_center_and_of_the_waiting() {
        let mut state = state();
        let mut windows = Recorded::default();
        state.tools_opened();
        state.show(Notification::ShortcutBusy, &mut windows);
        state.show(Notification::SettingsReset, &mut windows);
        state.shortcut_free(&mut windows);
        assert_eq!(windows.removed, [Notification::ShortcutBusy]);
        state.tools_closed(&mut windows);
        assert_eq!(windows.notifications(), [Notification::SettingsReset]);
    }

    #[test]
    fn only_a_click_on_the_busy_shortcut_opens_settings() {
        assert!(Notification::ShortcutBusy.opens_settings());
        assert!(!Notification::CaptureFailed.opens_settings());
        assert!(!Notification::SettingsReset.opens_settings());
    }

    #[test]
    fn a_click_on_a_busy_shortcut_an_earlier_screengauge_showed_comes_back_too() {
        // ScreenGauge was closed, and Windows started it for the click: it opens Settings all the same.
        let mut state = state();
        let clicked = state.on_event(Event {
            launch: "shortcut-busy:999.1".to_string(),
            what: What::Clicked,
        });
        assert_eq!(clicked, Some(Notification::ShortcutBusy));
    }

    #[test]
    fn a_click_on_the_busy_shortcut_drops_the_one_waiting() {
        // ScreenGauge was closed, and Windows started it for the click: the startup found the shortcut still busy and
        // queued the notification again, but Settings, which the click opens, already says so.
        let mut state = state();
        state.waiting = vec![Notification::ShortcutBusy, Notification::SettingsReset];
        state.on_event(Event {
            launch: "shortcut-busy:999.1".to_string(),
            what: What::Clicked,
        });
        assert_eq!(state.waiting, [Notification::SettingsReset]);
    }

    #[test]
    fn a_settings_reset_windows_failed_to_show_waits_for_the_next_chance() {
        let mut state = state();
        let mut windows = Recorded::default();
        state.show(Notification::SettingsReset, &mut windows);
        state.on_event(Event {
            launch: windows.launch_of_last(),
            what: What::Failed("0x80070490".to_string()),
        });
        assert_eq!(state.waiting, [Notification::SettingsReset]);
    }
}
