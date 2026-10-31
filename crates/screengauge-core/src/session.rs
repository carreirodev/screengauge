//! The session core: everything that happens between calling ScreenGauge and closing the tools (v1 spec WPC-296,
//! Modules). It is the spec's test point, chosen by the owner on 2026-10-29.
//!
//! The Windows layer opens a [`Session`] with the frozen screen, the options and the remembered state, hands it every
//! [`Event`] while the tools are open, draws its [`Scene`] and carries out the [`Effect`]s it returns. The session
//! never calls Windows and never reads the clock: the local time comes with each event.

mod dimensions;
mod event;
mod frozen_screen;
mod output;
mod units;

pub use event::{Event, Held, Input, Key, LocalTime, ToolbarButton};
pub use frozen_screen::{Colors, Image, Photo, Point, Rect, Scale};
pub use output::{Cursor, Effect, Label, Measurement, Message, Notice, Scene, Sign, Span};

use std::fmt;
use std::time::Duration;

use crate::settings::{DimensionsMode, Options, Remembered, Tolerance, Tool};

/// How long a notice stays on screen: a little over a second, the approved prototype's 1.3 s (v1 spec WPC-296, Como as
/// medidas aparecem, "o tempo exato se ajusta na construção").
const NOTICE_LASTS: Duration = Duration::from_millis(1300);

/// What the session opens with.
#[derive(Debug, Clone)]
pub struct Opening {
    /// The frozen screen: every monitor's photo, in the order the Windows layer shows them, or `None` for a monitor
    /// that neither capture path could photograph.
    pub frozen_screen: Vec<Option<Photo>>,
    /// Where the cursor is as the tools open, in physical pixels of the virtual screen.
    pub cursor: Point,
    /// The options in Settings as the tools open; changing Settings meanwhile doesn't reach this session.
    pub options: Options,
    pub remembered: Remembered,
    /// Until Windows has been running this long ([`Event::uptime`]), a press is taken for the second click of a
    /// double-click that called ScreenGauge, as on its tray icon, and does nothing: it would pin a measurement of the
    /// taskbar and copy it over what the user had copied. `None` when the global shortcut called it, after which a
    /// click counts at once (the owner's choice in WPC-308).
    pub second_click_until: Option<Duration>,
}

/// Why a session didn't open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refused {
    /// No photo at all: there is always a monitor.
    NoMonitor,
    /// The monitor at this index has no photo. All or nothing: the tools never open on part of the screen.
    NotCaptured(usize),
    /// The photo of the monitor at this index isn't the monitor's size.
    PhotoSize(usize),
}

impl fmt::Display for Refused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Refused::NoMonitor => write!(f, "the frozen screen has no monitor"),
            Refused::NotCaptured(monitor) => write!(f, "monitor {monitor} has no photo"),
            Refused::PhotoSize(monitor) => {
                write!(f, "the photo of monitor {monitor} isn't the monitor's size")
            }
        }
    }
}

impl std::error::Error for Refused {}

/// The tools, from the call that opens them to the effect that closes them.
#[derive(Debug)]
pub struct Session {
    frozen_screen: Vec<Photo>,
    /// Where the cursor is, as far as the session knows: the last event's place, or where the arrow keys put it.
    cursor: Point,
    /// The default tolerance from Settings, until the mouse wheel adjusts it (WPC-309).
    tolerance: Tolerance,
    /// "Also show logical pixels" in Settings: whether a label on a monitor whose scale isn't 100% shows them.
    also_show_logical_pixels: bool,
    /// The remembered state as the tools opened, and as they will leave it: the Windows layer keeps it only when the
    /// two differ.
    remembered_at_opening: Remembered,
    remembered: Remembered,
    /// What Dimensions measures around the cursor.
    live: Option<Measurement>,
    /// The pinned measurements, oldest first.
    pinned: Vec<Measurement>,
    /// The notice on screen, and when it goes.
    notice: Option<TimedNotice>,
    /// Until when a press is the second click of the double-click that called ScreenGauge.
    second_click_until: Option<Duration>,
    /// Set by the effect that closes the tools, after which events do nothing.
    closed: bool,
}

/// A notice, with the moment it goes: when Windows has been running `until`.
#[derive(Debug)]
struct TimedNotice {
    notice: Notice,
    until: Duration,
}

impl Session {
    pub fn open(opening: Opening) -> Result<Session, Refused> {
        let Opening {
            frozen_screen,
            cursor,
            options,
            remembered,
            second_click_until,
        } = opening;
        if frozen_screen.is_empty() {
            return Err(Refused::NoMonitor);
        }
        let frozen_screen = frozen_screen
            .into_iter()
            .enumerate()
            .map(|(monitor, photo)| photo.ok_or(Refused::NotCaptured(monitor)))
            .collect::<Result<Vec<Photo>, Refused>>()?;
        if let Some(monitor) = frozen_screen.iter().position(|photo| {
            (photo.image.width(), photo.image.height()) != (photo.rect.width, photo.rect.height)
        }) {
            return Err(Refused::PhotoSize(monitor));
        }
        let mut session = Session {
            frozen_screen,
            cursor,
            tolerance: options.default_tolerance,
            also_show_logical_pixels: options.also_show_logical_pixels,
            remembered_at_opening: remembered.clone(),
            remembered,
            live: None,
            pinned: Vec::new(),
            notice: None,
            second_click_until,
            closed: false,
        };
        session.measure();
        Ok(session)
    }

    /// What `event` makes happen.
    pub fn handle(&mut self, event: Event) -> Vec<Effect> {
        if self.closed {
            return Vec::new();
        }
        // Whatever the event, a notice whose time is up goes, so one never outstays its time should a wake be lost.
        if self
            .notice
            .as_ref()
            .is_some_and(|timed| timed.until <= event.uptime)
        {
            self.notice = None;
        }
        match event.input {
            // Esc closes the tools whatever tool is active (v1 spec WPC-296, derived rule 3), and so does the global
            // shortcut pressed again. Alt+F4 closes them as a whole, never one monitor's window alone.
            Input::KeyDown(Key::Esc) | Input::GlobalShortcut | Input::CloseAsked => self.close(),
            // Another program in front closes them as Esc does, so the user is never left on a frozen screen behind it,
            // and so does any change of monitors Windows reports, so nothing is measured with the wrong arrangement
            // (v1 spec WPC-296, Closing the tools).
            Input::OtherProgramInFront | Input::MonitorsChanged => self.close(),
            // Only the global shortcut closes the tools: calling ScreenGauge another way while they are open changes
            // nothing (v1 spec WPC-296, "abrir pelo menu Iniciar com a toolbar na tela não muda nada"), and the photo
            // isn't taken again.
            Input::Called => Vec::new(),
            Input::KeyDown(Key::H) => self.switch_mode(DimensionsMode::Horizontal),
            Input::KeyDown(Key::V) => self.switch_mode(DimensionsMode::Vertical),
            Input::KeyDown(Key::C) => self.switch_mode(DimensionsMode::Cross),
            Input::KeyDown(Key::Left) => self.step(-1, 0, event.held),
            Input::KeyDown(Key::Right) => self.step(1, 0, event.held),
            Input::KeyDown(Key::Up) => self.step(0, -1, event.held),
            Input::KeyDown(Key::Down) => self.step(0, 1, event.held),
            Input::Move => {
                self.cursor = event.cursor;
                self.measure();
                Vec::new()
            }
            Input::Press => self.pin(event.held, event.uptime),
            Input::KeyDown(Key::Backspace | Key::Delete) => self.unpin(),
            // A timer may come a little early: the notice then stays, and the session asks again for the moment left.
            Input::Wake => self
                .notice
                .as_ref()
                .map(|timed| vec![Effect::WakeAt(timed.until)])
                .unwrap_or_default(),
            _ => Vec::new(),
        }
    }

    /// Pins what Dimensions measures, as the user sees it at the click, and copies its physical pixels, or its logical
    /// pixels with Shift held, whether the label shows them or not; Dimensions goes on measuring, and the copy shows
    /// "Copied" where the click was (v1 spec WPC-296, Dimensions). A click with nothing measured does nothing, and so
    /// does the second click of the double-click that called ScreenGauge (WPC-308).
    fn pin(&mut self, held: Held, uptime: Duration) -> Vec<Effect> {
        if self.second_click_until.is_some_and(|until| uptime < until) {
            return Vec::new();
        }
        let Some(measurement) = self.live.clone() else {
            return Vec::new();
        };
        let Some(photo) = self.frozen_screen.get(measurement.monitor) else {
            // Can't happen: Dimensions measures on a monitor of the frozen screen.
            return Vec::new();
        };
        let text = units::clipboard_text(&measurement.lengths(), photo.scale, held.shift);
        let monitor = measurement.monitor;
        self.pinned.push(measurement);
        let mut effects = vec![Effect::CopyText(text)];
        effects.extend(self.show(Message::Copied, monitor, uptime));
        effects
    }

    /// Takes away the newest pinned measurement, as an undo. Dimensions' and Bounds' pins are one stack, and the keys
    /// work with either tool active, whichever tool pinned it; with Inspect or Snip active they do nothing (v1 spec
    /// WPC-296, derived rule 5).
    fn unpin(&mut self) -> Vec<Effect> {
        if matches!(self.remembered.last_tool, Tool::Dimensions | Tool::Bounds) {
            self.pinned.pop();
        }
        Vec::new()
    }

    /// Shows `message` where the cursor is, on `monitor`, in place of any notice still on screen, until it is due to go.
    fn show(&mut self, message: Message, monitor: usize, uptime: Duration) -> Option<Effect> {
        // Can't happen: Windows' uptime is milliseconds since it started, nowhere near Duration's limit.
        let until = uptime.checked_add(NOTICE_LASTS)?;
        self.notice = Some(TimedNotice {
            notice: Notice {
                message,
                at: self.cursor,
                monitor,
            },
            until,
        });
        Some(Effect::WakeAt(until))
    }

    /// Dimensions measures in `mode` from now on, and the next call starts in it. The keys do nothing with another
    /// tool active, as the mode buttons show only with Dimensions on (v1 spec WPC-296, Toolbar).
    fn switch_mode(&mut self, mode: DimensionsMode) -> Vec<Effect> {
        if !self.dimensions_is_active() {
            return Vec::new();
        }
        self.remembered.dimensions_mode = mode;
        self.measure();
        Vec::new()
    }

    /// Moves the cursor one pixel the arrow's way, or ten with Shift, and measures there (v1 spec WPC-296, Dimensions).
    /// The step goes on from where the session last put the cursor, never from the event's place: a key held down
    /// repeats, and a repeat may carry the cursor's place from before Windows carried out the last move. A step that
    /// would leave every monitor stops at the current monitor's limit.
    fn step(&mut self, right: i32, down: i32, held: Held) -> Vec<Effect> {
        let pixels = if held.shift { 10 } else { 1 };
        let ahead = Point {
            x: self.cursor.x.saturating_add(right * pixels),
            y: self.cursor.y.saturating_add(down * pixels),
        };
        let on_a_monitor = self
            .frozen_screen
            .iter()
            .any(|photo| photo.rect.contains(ahead));
        let to = match self
            .frozen_screen
            .iter()
            .find(|photo| photo.rect.contains(self.cursor))
        {
            Some(current) if !on_a_monitor => current.rect.nearest(ahead),
            _ => ahead,
        };
        if to == self.cursor {
            return Vec::new();
        }
        self.cursor = to;
        self.measure();
        vec![Effect::MoveCursor(to)]
    }

    /// Whether Dimensions is the active tool: the last one used, until the toolbar switches tools (WPC-310).
    fn dimensions_is_active(&self) -> bool {
        self.remembered.last_tool == Tool::Dimensions
    }

    /// Measures again around the cursor. Only Dimensions measures yet: the other tools come with their own tickets.
    fn measure(&mut self) {
        if !self.dimensions_is_active() {
            self.live = None;
            return;
        }
        self.live = dimensions::measure(
            &self.frozen_screen,
            self.cursor,
            self.remembered.dimensions_mode,
            self.tolerance,
            self.also_show_logical_pixels,
        );
    }

    /// Closes the tools, erasing every pinned measurement and the notice, and has the remembered state kept when the
    /// session changed it (v1 spec WPC-296, Fechar as ferramentas). Closing comes first, so the screen is back before the
    /// settings file is written.
    fn close(&mut self) -> Vec<Effect> {
        self.closed = true;
        self.pinned.clear();
        self.notice = None;
        let mut effects = vec![Effect::Close];
        if self.remembered != self.remembered_at_opening {
            effects.push(Effect::SaveRemembered(self.remembered.clone()));
        }
        effects
    }

    /// What to draw now.
    pub fn scene(&self) -> Scene<'_> {
        Scene {
            frozen_screen: &self.frozen_screen,
            cursor: Cursor::Cross,
            live: self.live.as_ref(),
            pinned: &self.pinned,
            notice: self.notice.as_ref().map(|timed| &timed.notice),
        }
    }
}
