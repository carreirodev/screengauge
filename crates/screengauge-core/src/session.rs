//! The session core: everything that happens between calling ScreenGauge and closing the tools (v1 spec WPC-296,
//! Modules). It is the spec's test point, chosen by the owner on 2026-10-29.
//!
//! The Windows layer opens a [`Session`] with the frozen screen, the options and the remembered state, hands it every
//! [`Event`] while the tools are open, draws its [`Scene`] and carries out the [`Effect`]s it returns. The session
//! never calls Windows and never reads the clock: the local time comes with each event.

mod bounds;
mod color;
mod dimensions;
mod event;
mod frozen_screen;
mod magnified_view;
mod output;
mod pixels;
mod snip_file;
pub mod toolbar;
mod units;

pub use event::{Event, Held, Input, Key, LocalTime, ToolbarButton};
pub use frozen_screen::{Colors, Image, Photo, Point, Rect, Scale};
pub use output::{
    ApproximateColor, ColorText, Coordinate, Cursor, DrawnRectangle, Effect, Label, MagnifiedView,
    Measured, Measurement, Message, Notice, NoticeLine, NoticeSide, PixelColor, SaveTo, Scene,
    Sign, SnipArea, Span, ToleranceIndicator, Toolbar,
};
pub use snip_file::{FileNames, png_path};

use std::collections::VecDeque;
use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

use crate::settings::{
    ColorFormat, DefaultFolder, DimensionsMode, Folder, Magnification, Options, Remembered,
    SnipDestination, Tolerance, Tool, ToolbarPlace,
};

/// How long a notice stays on screen: a little over a second, the approved prototype's 1.3 s (v1 spec WPC-296, Como as
/// medidas aparecem, "o tempo exato se ajusta na construção").
const NOTICE_LASTS: Duration = Duration::from_millis(1300);

/// How long the tolerance indicator stays after the mouse wheel's last notch (v1 spec WPC-296, Tolerance).
const TOLERANCE_INDICATOR_LASTS: Duration = Duration::from_secs(1);

/// How long the magnification indicator stays after the mouse wheel's last notch (v1 spec WPC-296, Inspect).
const MAGNIFICATION_INDICATOR_LASTS: Duration = Duration::from_secs(1);

/// How far the mouse wheel turns in a notch, in the units of [`Input::Wheel`]: Windows' WHEEL_DELTA.
const NOTCH: i32 = 120;

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
    /// Where the tool measures: the cursor's place, or, while the cursor is on the toolbar and the tool pauses, the last
    /// place it had off the toolbar, so the measurement stays as it was (the owner's choice in WPC-311). `None` while
    /// the cursor hasn't been off the toolbar since the tools opened.
    measured_at: Option<Point>,
    /// The tolerance Dimensions, Bounds and Snip share: the default tolerance from Settings, until the mouse wheel
    /// changes it. It lasts until the tools close and is never kept.
    tolerance: Tolerance,
    /// "Also show logical pixels" in Settings: whether a label on a monitor whose scale isn't 100% shows them.
    also_show_logical_pixels: bool,
    /// "Color format" in Settings: how Inspect shows and copies the color.
    color_format: ColorFormat,
    /// "Destination" in Settings: where Snip's image goes.
    snip_destination: SnipDestination,
    /// "Save to default folder" in Settings, and the folder.
    default_folder: DefaultFolder,
    /// Snip's image on its way to a file, from the release until the file is written or Save As is cancelled. `None`
    /// the rest of the time.
    saving: Option<Saving>,
    /// The copies asked for that the clipboard hasn't answered yet, oldest first. The Windows layer answers each in the
    /// order asked, before it carries out the next effect, so this holds them only for a moment: one, or a few should
    /// several clicks come before the Windows layer gets to the first copy.
    copies: VecDeque<WaitingCopy>,
    /// The remembered state as the tools opened, and as they will leave it: the Windows layer keeps it only when the
    /// two differ.
    remembered_at_opening: Remembered,
    remembered: Remembered,
    /// What the active tool measures now: Dimensions around the cursor, Bounds the content of the rectangle being drawn.
    live: Option<Measurement>,
    /// The area Snip takes the image of, while it draws a rectangle that holds some.
    snip_area: Option<SnipArea>,
    /// The drag Bounds or Snip is drawing a rectangle with, from the press until the button comes up.
    drag: Option<bounds::Drag>,
    /// Whether the button is still down after Esc cancelled the drag it was drawing. Until it comes up, the press is
    /// still the tool's: another Esc, or the repeats of one held down, cancel nothing more and close nothing, and the
    /// toolbar doesn't react (v1 spec WPC-296, Fechar as ferramentas).
    held_after_cancel: bool,
    /// The pinned measurements, oldest first.
    pinned: Vec<Measurement>,
    /// The notice on screen, and when it goes.
    notice: Option<TimedNotice>,
    /// Until when the tolerance indicator shows, when it does.
    tolerance_indicator_until: Option<Duration>,
    /// Until when the magnified view shows the magnification in its corner, when it does.
    magnification_indicator_until: Option<Duration>,
    /// How far the mouse wheel has turned towards its next notch, in Windows' units: forward above zero, back below it.
    wheel_turned: i32,
    /// Until when a press is the second click of the double-click that called ScreenGauge.
    second_click_until: Option<Duration>,
    /// The monitor the toolbar is on: the current monitor as the tools opened, for as long as they are open.
    toolbar_monitor: usize,
    /// Where the toolbar's parts are on that monitor: at the remembered place as the tools opened, and wherever a drag
    /// takes it.
    toolbar_layout: toolbar::Layout,
    /// How the toolbar is held while it is dragged by its background (WPC-312). `None` the rest of the time.
    toolbar_grab: Option<Grab>,
    /// Whether the toolbar is minimized to its small button. Every call opens it whole, so it is never kept.
    toolbar_minimized: bool,
    /// Set by the effect that closes the tools, after which events do nothing.
    closed: bool,
}

/// How the toolbar is held while it is dragged.
#[derive(Debug, Clone, Copy)]
struct Grab {
    /// How far right and down of the toolbar's corner the press was: the cursor keeps holding the toolbar there.
    offset: Point,
    /// The toolbar's place at the press. Each move works the place out from it again, so an axis the toolbar comes back
    /// to its starting pixel on keeps the share it had, and the remembered state changes only when the toolbar moved.
    place_at_press: ToolbarPlace,
}

/// Snip's image on its way to a file (v1 spec WPC-296, Snip).
#[derive(Debug)]
struct Saving {
    stage: Stage,
    ending: Ending,
}

/// What the end of the Snip's file path goes by, whichever step ends it: Save As cancelled or failing to open, or the
/// file written or not.
#[derive(Debug, Clone, Copy)]
struct Ending {
    /// What became of the image's copy to the clipboard, which the notice then says too.
    copy: ImageCopy,
    /// The monitor the image was taken on, where the notice goes should the cursor be on none.
    monitor: usize,
    /// Whether the monitors changed meanwhile: the tools then close once the file is written or Save As is cancelled,
    /// never with Save As open over them, and without a notice.
    close_after: bool,
}

/// What became of the Snip's copy to the clipboard, which the notice about its file tells of too (WPC-352).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ImageCopy {
    /// Destination is File: no copy was asked for.
    NotAsked,
    /// Destination is Both, and the clipboard hasn't answered yet. Can't last until the file's notice: the copy is
    /// asked for first, and answered before the file is written or Save As opens. Should it, the notice says nothing
    /// of the copy, as with File, rather than something that may be false.
    Waiting,
    Answered(CopyAnswer),
}

impl ImageCopy {
    /// The clipboard's answer, once it came.
    fn answer(self) -> Option<CopyAnswer> {
        match self {
            ImageCopy::Answered(answer) => Some(answer),
            ImageCopy::NotAsked | ImageCopy::Waiting => None,
        }
    }
}

/// How the clipboard answered a copy: [`Input::Copied`] or [`Input::NotCopied`] (WPC-352).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CopyAnswer {
    Made,
    /// Windows refused the clipboard, which holds what it held before.
    Refused,
}

impl CopyAnswer {
    /// What the notice of a copy alone says: "Copied" or "Couldn't copy.".
    fn message(self) -> Message {
        match self {
            CopyAnswer::Made => Message::Copied,
            CopyAnswer::Refused => Message::CouldntCopy,
        }
    }
}

/// How the Windows layer answered the Snip's file: [`Input::Saved`] or [`Input::NotSaved`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FileAnswer {
    Saved,
    /// No file could be written, and none was left behind.
    NotSaved,
}

/// A copy the session asked for, until the clipboard answers it (WPC-352).
#[derive(Debug)]
enum WaitingCopy {
    /// A measurement's or a color's, or the Snip's image with Destination at Clipboard: once answered, "Copied" or
    /// "Couldn't copy." shows at `at`, where the cursor was as the copy was asked for, on `monitor`, on the `side`
    /// of `at` it had then (WPC-367).
    Notice {
        at: Point,
        monitor: usize,
        side: NoticeSide,
    },
    /// The Snip's image with Destination at Both: the answer waits in [`Saving`] for the file's, and one notice says
    /// both.
    SnipImage,
}

#[derive(Debug)]
enum Stage {
    /// Save As is open, choosing where `image` goes: it is already taken. `aside` while another program is in front.
    SaveAs { image: Image, aside: bool },
    /// The file is being written. `folder` is the folder Save As chose, which the next Save As opens in once the file
    /// is written there.
    Writing { folder: Option<Folder> },
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
        // The cursor is always on a monitor; should it not be, the toolbar goes on the first one.
        let toolbar_monitor = frozen_screen::monitor_at(&frozen_screen, cursor).unwrap_or(0);
        let toolbar_layout = frozen_screen
            .get(toolbar_monitor)
            .map(|photo| toolbar::layout_on(photo.rect, photo.scale, remembered.toolbar_place))
            .ok_or(Refused::NoMonitor)?;
        let mut session = Session {
            frozen_screen,
            cursor,
            measured_at: None,
            tolerance: options.default_tolerance,
            also_show_logical_pixels: options.also_show_logical_pixels,
            color_format: options.color_format,
            snip_destination: options.snip_destination,
            default_folder: options.default_folder,
            saving: None,
            copies: VecDeque::new(),
            remembered_at_opening: remembered.clone(),
            remembered,
            live: None,
            snip_area: None,
            drag: None,
            held_after_cancel: false,
            pinned: Vec::new(),
            notice: None,
            tolerance_indicator_until: None,
            magnification_indicator_until: None,
            wheel_turned: 0,
            second_click_until,
            toolbar_monitor,
            toolbar_layout,
            toolbar_grab: None,
            toolbar_minimized: false,
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
        // Whatever the event, a notice or the tolerance indicator whose time is up goes, so neither outstays its time
        // should a wake be lost.
        if self
            .notice
            .as_ref()
            .is_some_and(|timed| timed.until <= event.uptime)
        {
            self.notice = None;
        }
        if self
            .tolerance_indicator_until
            .is_some_and(|until| until <= event.uptime)
        {
            self.tolerance_indicator_until = None;
        }
        if self
            .magnification_indicator_until
            .is_some_and(|until| until <= event.uptime)
        {
            self.magnification_indicator_until = None;
        }
        let alt = match event.input {
            Input::KeyDown(Key::Alt) => true,
            Input::KeyUp(Key::Alt) => false,
            _ => event.held.alt,
        };
        self.follow_alt(alt);
        // Exiting closes the tools in every state, so it comes before Save As's, which keeps them open otherwise.
        if event.input == Input::Exit {
            self.saving = None;
            return self.close();
        }
        // The clipboard's answer counts in every state, Save As's included: with Destination at Both it comes while the
        // Snip's image is on its way to a file.
        match event.input {
            Input::Copied => return self.copy_answered(CopyAnswer::Made, event.uptime),
            Input::NotCopied => return self.copy_answered(CopyAnswer::Refused, event.uptime),
            _ => {}
        }
        if self.saving.is_some() {
            return self.while_saving(event);
        }
        match event.input {
            // The second click of the double-click that called ScreenGauge does nothing, on the toolbar as on the tool
            // (WPC-308): it would act on a button the user never aimed at.
            Input::Toolbar(_) if self.is_second_click(event.uptime) => Vec::new(),
            // Esc with the button held while Bounds or Snip draws a rectangle cancels only that drag, and closes nothing
            // for as long as the button stays down (v1 spec WPC-296, Fechar as ferramentas).
            Input::KeyDown(Key::Esc) if self.tool_holds_the_press() => self.cancel_drag(),
            // Esc without a drag closes the tools whatever tool is active (v1 spec WPC-296, derived rule 3), and so do
            // the toolbar's close button and the global shortcut pressed again. Alt+F4 closes them as a whole, never one
            // monitor's window alone.
            Input::KeyDown(Key::Esc)
            | Input::Toolbar(ToolbarButton::Close)
            | Input::GlobalShortcut
            | Input::CloseAsked => self.close(),
            // Another program in front closes them as Esc does, so the user is never left on a frozen screen behind it,
            // and so does any change of monitors Windows reports, so nothing is measured with the wrong arrangement
            // (v1 spec WPC-296, Closing the tools).
            Input::OtherProgramInFront | Input::MonitorsChanged => self.close(),
            // The gear closes the tools, and Settings opens once the screen is back and the file written: Settings is a
            // window, never a tool, so the last tool stays the one that was active (v1 spec WPC-296, Settings).
            Input::Toolbar(ToolbarButton::Settings) => {
                let mut effects = self.close();
                effects.push(Effect::OpenSettings);
                effects
            }
            // Only the global shortcut closes the tools: calling ScreenGauge another way while they are open changes
            // nothing (v1 spec WPC-296, "abrir pelo menu Iniciar com a toolbar na tela não muda nada"), and the photo
            // isn't taken again.
            Input::Called => Vec::new(),
            // 1 to 4 are the tools in the toolbar's order, from any monitor (v1 spec WPC-296, Trocar de ferramenta).
            Input::KeyDown(Key::Digit1) => self.switch_tool(Tool::Dimensions),
            Input::KeyDown(Key::Digit2) => self.switch_tool(Tool::Bounds),
            Input::KeyDown(Key::Digit3) => self.switch_tool(Tool::Inspect),
            Input::KeyDown(Key::Digit4) => self.switch_tool(Tool::Snip),
            Input::Toolbar(ToolbarButton::Tool(tool)) => self.switch_tool(tool),
            Input::Toolbar(ToolbarButton::Mode(mode)) => self.switch_mode(mode),
            Input::Toolbar(ToolbarButton::Minimize) => self.minimize(true),
            Input::Toolbar(ToolbarButton::Restore) => self.minimize(false),
            Input::KeyDown(Key::H) => self.switch_mode(DimensionsMode::Horizontal),
            Input::KeyDown(Key::V) => self.switch_mode(DimensionsMode::Vertical),
            Input::KeyDown(Key::C) => self.switch_mode(DimensionsMode::Cross),
            Input::KeyDown(Key::Left) => self.step(-1, 0, event.held),
            Input::KeyDown(Key::Right) => self.step(1, 0, event.held),
            Input::KeyDown(Key::Up) => self.step(0, -1, event.held),
            Input::KeyDown(Key::Down) => self.step(0, 1, event.held),
            Input::Move => {
                self.move_cursor(event.cursor);
                Vec::new()
            }
            // A press on the toolbar is the toolbar's, never the tool's, which pauses there: on a button the Windows
            // layer makes it the click of that button, and on the background it picks the toolbar up to drag it.
            Input::Press => match self.toolbar_hit(event.cursor) {
                Some(toolbar::Hit::Background) => self.grab(event.cursor, event.uptime),
                Some(toolbar::Hit::Button(_)) => Vec::new(),
                None => self.press(event.held, event.uptime),
            },
            Input::Release => self.release(event.held, event.time, event.uptime),
            Input::KeyDown(Key::Backspace | Key::Delete) => self.unpin(),
            Input::Wheel(delta) => self.turn_wheel(delta, event.cursor, event.uptime),
            // What was due has gone above. A timer may also come a little early: what was due then stays, and the
            // session asks again for the moment left. Either way it asks for whatever is due next.
            Input::Wake => self.wake().into_iter().collect(),
            _ => Vec::new(),
        }
    }

    /// What a press off the toolbar does with the active tool: Dimensions pins what it measures, Bounds and Snip start a
    /// drag, and Inspect copies the color.
    fn press(&mut self, held: Held, uptime: Duration) -> Vec<Effect> {
        match self.remembered.last_tool {
            Tool::Dimensions => self.pin(held, uptime),
            Tool::Bounds | Tool::Snip => {
                self.start_drag(held.alt, uptime);
                Vec::new()
            }
            Tool::Inspect => self.copy_color(held, uptime),
        }
    }

    /// Copies the color of the pixel at the magnified view's center, the one under the cursor at the click; the tool stays
    /// as it was, and nothing is pinned (v1 spec WPC-296, Inspect). Once the clipboard answers, "Copied" or "Couldn't
    /// copy." shows (WPC-352). It copies the format the readout shows first, or with Shift held the other, even when it
    /// doesn't show. The second click of the double-click
    /// that called ScreenGauge copies nothing, as it pins nothing in Dimensions (WPC-308).
    fn copy_color(&mut self, held: Held, uptime: Duration) -> Vec<Effect> {
        if self.is_second_click(uptime) {
            return Vec::new();
        }
        // Before the cursor has been off the toolbar there is nothing to copy.
        let Some(at) = self.measured_at else {
            return Vec::new();
        };
        let Some((monitor, photo)) = frozen_screen::photo_at(&self.frozen_screen, at) else {
            // Can't happen: the tool measures where the cursor is, and the cursor is always on a monitor.
            return Vec::new();
        };
        let texts = color::texts(pixels::color(photo, at), self.color_format);
        let text = if held.shift { texts.other } else { texts.first };
        self.ask_for_copy(Effect::CopyText(text), monitor)
    }

    /// Starts drawing a rectangle from where the session last put the cursor, on the monitor there, as Dimensions' click
    /// pins what it measures there, exactly as drawn while `exact`, with Alt held. The second click of the double-click
    /// that called ScreenGauge starts none (WPC-308).
    fn start_drag(&mut self, exact: bool, uptime: Duration) {
        if self.is_second_click(uptime) {
            return;
        }
        self.drag = frozen_screen::monitor_at(&self.frozen_screen, self.cursor)
            .map(|monitor| bounds::Drag::new(self.cursor, monitor, exact));
        self.measure();
    }

    /// Has the drag Bounds or Snip is drawing measure exactly the drawn rectangle while `alt` is held, and shrink it to the
    /// content otherwise. Alt counts as each event has it, so pressing or letting go of it in the middle of the drag
    /// changes the preview at once, and the release goes by the key as the button comes up (v1 spec WPC-296, Bounds).
    /// Only a change measures again: Alt held down repeats, and a shrink may read a whole monitor.
    fn follow_alt(&mut self, alt: bool) {
        if self.drag.as_mut().is_some_and(|drag| drag.follow_alt(alt)) {
            self.measure();
        }
    }

    /// Whether the button is held for the tool: Bounds or Snip draws a rectangle, or Esc cancelled the one it drew and the
    /// button hasn't come up since.
    fn tool_holds_the_press(&self) -> bool {
        self.drag.is_some() || self.held_after_cancel
    }

    /// What the button coming up does: it lets go of the dragged toolbar, or ends the drag Bounds or Snip was drawing,
    /// or, after Esc cancelled that drag, does nothing.
    fn release(&mut self, held: Held, time: LocalTime, uptime: Duration) -> Vec<Effect> {
        self.held_after_cancel = false;
        match self.drag.take() {
            Some(drag) => self.finish_drag(drag, held, time, uptime),
            None => self.let_go(),
        }
    }

    /// Drops the drag Bounds or Snip was drawing, with its preview, and nothing else: the tools stay open with every
    /// pinned measurement, and the button coming up afterwards pins, copies and takes nothing.
    fn cancel_drag(&mut self) -> Vec<Effect> {
        self.drag = None;
        self.held_after_cancel = true;
        self.measure();
        Vec::new()
    }

    /// Pins what Bounds measured as `drag` ended, exactly as the preview showed it, and copies it, its logical pixels
    /// with Shift held as the button comes up, whatever it was held for before (v1 spec WPC-296, Bounds). The drawn
    /// rectangle goes, there are no handles to adjust it, and the tool is ready for the next drag. With nothing but the
    /// reference color in the drawn rectangle, nothing is pinned or copied, the clipboard stays as it was, and the
    /// notice "No content found" says so. A drag that never went past the drag distance was a click, and does nothing
    /// at all, with or without Alt, and without a notice.
    ///
    /// Snip instead takes the image of what its preview showed, at `time`, and sends it where Destination says, pinning
    /// nothing, and Shift changes nothing (v1 spec WPC-296, Snip); with nothing but the reference color there is no image,
    /// and the notice says so in Snip's words.
    fn finish_drag(
        &mut self,
        drag: bounds::Drag,
        held: Held,
        time: LocalTime,
        uptime: Duration,
    ) -> Vec<Effect> {
        let measured = self.live.take();
        let snipped = self.snip_area.take();
        self.measure();
        if drag.is_click() {
            return Vec::new();
        }
        let tool = self.remembered.last_tool;
        match (tool, measured, snipped) {
            (Tool::Snip, _, Some(area)) => self.take_image(&area, time),
            (Tool::Snip, _, None) | (_, None, _) => {
                let monitor = self.notice_monitor(drag.monitor);
                self.show(Message::NoContentFound(tool), monitor, uptime)
                    .into_iter()
                    .collect()
            }
            (_, Some(measurement), _) => self.pin_and_copy(measurement, held.shift),
        }
    }

    /// Takes the image of `area`, from the frozen screen and never from the screen as it is now, at `time`, and sends it
    /// where Destination says (v1 spec WPC-296, Snip). The clipboard, with Clipboard or Both, gets it at once, each image
    /// in the place of the last. The file, with File or Both, goes to the default folder under the automatic name when
    /// "Save to default folder" is on, with no window, and otherwise wherever Save As, opened in the folder it last saved
    /// to, says. The notice waits for the clipboard's answer, and for the file too when there is one.
    fn take_image(&mut self, area: &SnipArea, time: LocalTime) -> Vec<Effect> {
        let Some(image) = self
            .frozen_screen
            .get(area.monitor)
            .and_then(|photo| pixels::crop(photo, area.rect))
        else {
            // Can't happen: the area lies on the monitor the drag began on, one of the frozen screen's.
            return Vec::new();
        };
        let mut effects = Vec::new();
        let copy = match self.snip_destination {
            SnipDestination::Clipboard => {
                return self.ask_for_copy(Effect::CopyImage(image), area.monitor);
            }
            SnipDestination::File => ImageCopy::NotAsked,
            SnipDestination::Both => {
                effects.push(Effect::CopyImage(image.clone()));
                self.copies.push_back(WaitingCopy::SnipImage);
                ImageCopy::Waiting
            }
        };
        let names = FileNames::at(time);
        let (stage, effect) = match &self.default_folder {
            DefaultFolder::On(folder) => (
                Stage::Writing { folder: None },
                Effect::WritePng {
                    to: SaveTo::DefaultFolder {
                        folder: folder.clone(),
                        names,
                    },
                    image,
                },
            ),
            DefaultFolder::Off(_) => (
                Stage::SaveAs {
                    image,
                    aside: false,
                },
                Effect::OpenSaveAs {
                    folder: self.remembered.last_save_as_folder.clone(),
                    file_name: names.first(),
                },
            ),
        };
        self.saving = Some(Saving {
            stage,
            ending: Ending {
                copy,
                monitor: area.monitor,
                close_after: false,
            },
        });
        effects.push(effect);
        effects
    }

    /// What `event` does while Snip's image is on its way to a file (v1 spec WPC-296, Snip). With Save As open, nothing
    /// the user does over the frozen screen counts, so no other drag starts, and Esc is Save As's alone. Another program
    /// coming to the front sends the frozen screen and Save As behind it, to wait, and Save As's button brings them back
    /// (the owner's choice in WPC-318); so does the global shortcut, which never closes the tools while Save As is open,
    /// as that would lose the image, and so does calling ScreenGauge from its tray icon or the Start menu. A change of monitors closes the tools once the file is done with.
    fn while_saving(&mut self, event: Event) -> Vec<Effect> {
        match event.input {
            Input::SaveAsClosed(chosen) => self.save_as_closed(chosen, event.cursor, event.uptime),
            Input::SaveAsFailed => self.save_as_failed(event.uptime),
            Input::Saved => self.written(FileAnswer::Saved, event.uptime),
            Input::NotSaved => self.written(FileAnswer::NotSaved, event.uptime),
            Input::OtherProgramInFront => self.step_aside(true),
            Input::BackInFront | Input::GlobalShortcut | Input::Called => self.step_aside(false),
            Input::MonitorsChanged => {
                if let Some(saving) = self.saving.as_mut() {
                    saving.ending.close_after = true;
                }
                Vec::new()
            }
            Input::Wake => self.wake().into_iter().collect(),
            _ => Vec::new(),
        }
    }

    /// Sends the frozen screen and Save As behind the program in front when `aside`, or brings them back otherwise; a
    /// step they have already taken does nothing again, and so does either while the file is being written.
    fn step_aside(&mut self, aside: bool) -> Vec<Effect> {
        let Some(Saving {
            stage: Stage::SaveAs { aside: now, .. },
            ..
        }) = self.saving.as_mut()
        else {
            return Vec::new();
        };
        if *now == aside {
            return Vec::new();
        }
        *now = aside;
        vec![if aside {
            Effect::StepAside
        } else {
            Effect::ComeBack
        }]
    }

    /// Save As closed with the cursor at `cursor`: the image goes to `chosen`, always as a `.png`, or, cancelled, goes
    /// nowhere, no file is made, and only a copy asked for says what became of it: "Copied" or "Couldn't copy.".
    fn save_as_closed(
        &mut self,
        chosen: Option<PathBuf>,
        cursor: Point,
        uptime: Duration,
    ) -> Vec<Effect> {
        let Some(Saving {
            stage: Stage::SaveAs { image, aside },
            ending,
        }) = self.saving.take()
        else {
            return Vec::new();
        };
        let mut effects = Vec::new();
        // Can't happen: Save As is answered in front. Should it be, the frozen screen comes back before anything else.
        if aside {
            effects.push(Effect::ComeBack);
        }
        self.move_cursor(cursor);
        match chosen {
            Some(chosen) => {
                let path = png_path(&chosen);
                let folder = path.parent().and_then(Folder::new);
                self.saving = Some(Saving {
                    stage: Stage::Writing { folder },
                    ending,
                });
                effects.push(Effect::WritePng {
                    to: SaveTo::Chosen(path),
                    image,
                });
            }
            None => effects.extend(self.end_saving(ending, None, uptime)),
        }
        effects
    }

    /// The file was written, or couldn't be, and none was left, as `file` says. A folder Save As chose is the one it
    /// opens in next, once a file was written there, even as the tools close.
    fn written(&mut self, file: FileAnswer, uptime: Duration) -> Vec<Effect> {
        let Some(Saving {
            stage: Stage::Writing { folder },
            ending,
        }) = self.saving.take()
        else {
            return Vec::new();
        };
        if file == FileAnswer::Saved
            && let Some(folder) = folder
        {
            self.remembered.last_save_as_folder = Some(folder);
        }
        self.end_saving(ending, Some(file), uptime)
    }

    /// Windows couldn't open Save As: no file is written, as when one couldn't be.
    fn save_as_failed(&mut self, uptime: Duration) -> Vec<Effect> {
        let Some(Saving {
            stage: Stage::SaveAs { .. },
            ending,
        }) = self.saving.take()
        else {
            return Vec::new();
        };
        self.end_saving(ending, Some(FileAnswer::NotSaved), uptime)
    }

    /// Ends the Snip's file path, whichever step ends it: `file` says whether the file was written, and is `None` when
    /// Save As was cancelled. With the monitors changed meanwhile, the tools close without a notice. Otherwise they stay
    /// open, the options as they were, and the notice says what became of the file and, with Destination at Both, of
    /// the image's copy to the clipboard; after a cancel, only a copy asked for says what became of it, "Copied" or
    /// "Couldn't copy.", and nothing shows with File.
    fn end_saving(
        &mut self,
        ending: Ending,
        file: Option<FileAnswer>,
        uptime: Duration,
    ) -> Vec<Effect> {
        if ending.close_after {
            return self.close();
        }
        let message = match file {
            Some(file) => Some(file_notice(file, ending.copy)),
            None => ending.copy.answer().map(CopyAnswer::message),
        };
        let Some(message) = message else {
            return Vec::new();
        };
        let monitor = self.notice_monitor(ending.monitor);
        self.show(message, monitor, uptime).into_iter().collect()
    }

    /// Pins what Dimensions measures, as the user sees it at the click, and copies it, its logical pixels with Shift
    /// held; Dimensions goes on measuring (v1 spec WPC-296, Dimensions). A click with nothing measured does nothing, and
    /// so does the second click of the double-click that called ScreenGauge (WPC-308).
    fn pin(&mut self, held: Held, uptime: Duration) -> Vec<Effect> {
        if self.is_second_click(uptime) {
            return Vec::new();
        }
        let Some(measurement) = self.live.clone() else {
            return Vec::new();
        };
        self.pin_and_copy(measurement, held.shift)
    }

    /// Pins `measurement` and copies its physical pixels, or its logical pixels when `logical`, whether its label shows
    /// them or not, which is what Dimensions' click and Bounds' release both copy (v1 spec WPC-296, Dimensions and
    /// Bounds). Once the clipboard answers, "Copied" or "Couldn't copy." shows where the cursor is, and the measurement
    /// stays pinned either way (WPC-352).
    fn pin_and_copy(&mut self, measurement: Measurement, logical: bool) -> Vec<Effect> {
        let Some(photo) = self.frozen_screen.get(measurement.monitor) else {
            // Can't happen: the tools measure on monitors of the frozen screen.
            return Vec::new();
        };
        let text = units::clipboard_text(&measurement.lengths(), photo.scale, logical);
        let monitor = measurement.monitor;
        self.pinned.push(measurement);
        self.ask_for_copy(Effect::CopyText(text), monitor)
    }

    /// Asks for `copy`, a [`Effect::CopyText`] or a [`Effect::CopyImage`], whose notice waits for the clipboard's answer and then shows where the cursor is now, on its
    /// monitor, or on `measured_on`, the monitor of what was copied, should it be on none. Nothing says "Copied" before
    /// the copy was made (WPC-352).
    fn ask_for_copy(&mut self, copy: Effect, measured_on: usize) -> Vec<Effect> {
        self.copies.push_back(WaitingCopy::Notice {
            at: self.cursor,
            monitor: self.notice_monitor(measured_on),
            side: self.notice_side(),
        });
        vec![copy]
    }

    /// The clipboard answered the oldest copy still waiting, as `answer` says. A measurement's, a color's or the Snip's
    /// image to the clipboard alone shows "Copied" or "Couldn't copy." where the copy was asked for; the Snip's image
    /// with Destination at Both waits for its file, whose notice says both.
    fn copy_answered(&mut self, answer: CopyAnswer, uptime: Duration) -> Vec<Effect> {
        match self.copies.pop_front() {
            Some(WaitingCopy::Notice { at, monitor, side }) => self
                .show_at(answer.message(), at, monitor, side, uptime)
                .into_iter()
                .collect(),
            Some(WaitingCopy::SnipImage) => {
                if let Some(saving) = self.saving.as_mut()
                    && saving.ending.copy == ImageCopy::Waiting
                {
                    saving.ending.copy = ImageCopy::Answered(answer);
                }
                Vec::new()
            }
            // Can't happen: the Windows layer answers only the copies the session asked for.
            None => Vec::new(),
        }
    }

    /// Picks the toolbar up by its background at `at`, to drag it (v1 spec WPC-296, Toolbar; WPC-312). The second click of
    /// the double-click that called ScreenGauge picks nothing up, as it does nothing anywhere else: a toolbar left at the
    /// bottom right may sit over the tray icon.
    fn grab(&mut self, at: Point, uptime: Duration) -> Vec<Effect> {
        if !self.is_second_click(uptime) {
            let corner = self.toolbar_corner();
            self.toolbar_grab = Some(Grab {
                offset: Point {
                    x: at.x.saturating_sub(corner.x),
                    y: at.y.saturating_sub(corner.y),
                },
                place_at_press: self.remembered.toolbar_place,
            });
        }
        Vec::new()
    }

    /// Moves the held toolbar so the cursor keeps holding it where it was picked up, as near as the margins of its own
    /// monitor let it go: it never leaves that monitor (v1 spec WPC-296, Toolbar), and stops 14 logical pixels short
    /// of each side, where the remembered place puts it at the next call (the owner's choice in WPC-312). The place it
    /// takes is the remembered state's, kept when the tools close, even by Esc in the middle of the drag: the toolbar
    /// stays where the user had it (the owner's choice in WPC-312).
    fn drag_toolbar(&mut self) {
        let Some(grab) = self.toolbar_grab else {
            return;
        };
        let Some(photo) = self.frozen_screen.get(self.toolbar_monitor) else {
            // Can't happen: the toolbar's monitor is one of the frozen screen's.
            return;
        };
        let wanted = Point {
            x: self.cursor.x.saturating_sub(grab.offset.x),
            y: self.cursor.y.saturating_sub(grab.offset.y),
        };
        let place = toolbar::place_nearest(photo.rect, photo.scale, wanted, grab.place_at_press);
        self.remembered.toolbar_place = place;
        self.toolbar_layout = toolbar::layout_on(photo.rect, photo.scale, place);
    }

    /// Lets the dragged toolbar go where it is, when the button comes up or the Windows layer loses the mouse. Should the
    /// toolbar have stopped at a margin short of the cursor, the tool measures where the cursor is at once. With
    /// nothing held, it does nothing.
    fn let_go(&mut self) -> Vec<Effect> {
        if self.toolbar_grab.take().is_some() {
            self.measure();
        }
        Vec::new()
    }

    /// The toolbar's top left corner, which its minimized button shares.
    fn toolbar_corner(&self) -> Point {
        Point {
            x: self.toolbar_layout.toolbar.left,
            y: self.toolbar_layout.toolbar.top,
        }
    }

    /// Whether a click when Windows has been running `uptime` is the second click of the double-click that called
    /// ScreenGauge, as on its tray icon (WPC-308).
    fn is_second_click(&self, uptime: Duration) -> bool {
        self.second_click_until.is_some_and(|until| uptime < until)
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

    /// The monitor a notice where the cursor is goes on: the cursor's. The cursor is always on a monitor, which the drag
    /// of Bounds may have left for another; should it be on none, the notice goes on `measured_on`, the monitor of what
    /// the notice is about.
    fn notice_monitor(&self, measured_on: usize) -> usize {
        frozen_screen::monitor_at(&self.frozen_screen, self.cursor).unwrap_or(measured_on)
    }

    /// Shows `message` where the cursor is, on `monitor`, in place of any notice still on screen, until it is due to go,
    /// and asks for the next wake, which is the tolerance indicator's when that goes first.
    fn show(&mut self, message: Message, monitor: usize, uptime: Duration) -> Option<Effect> {
        self.show_at(message, self.cursor, monitor, self.notice_side(), uptime)
    }

    /// Which side of the cursor a notice shown now goes: below it where Inspect's magnified view is above it, near the
    /// monitor's bottom, so the notice doesn't hide the color just copied, and above it everywhere else (the owner's
    /// choice in WPC-367).
    fn notice_side(&self) -> NoticeSide {
        match self.magnified_view() {
            Some(view) if view.panel.top < view.center.y => NoticeSide::Below,
            _ => NoticeSide::Above,
        }
    }

    /// Shows `message` at `at`, on `monitor`, on `side` of `at`, as [`Session::show`] does where the cursor is.
    fn show_at(
        &mut self,
        message: Message,
        at: Point,
        monitor: usize,
        side: NoticeSide,
        uptime: Duration,
    ) -> Option<Effect> {
        // Can't happen: Windows' uptime is milliseconds since it started, nowhere near Duration's limit.
        let until = uptime.checked_add(NOTICE_LASTS)?;
        self.notice = Some(TimedNotice {
            notice: Notice {
                message,
                at,
                monitor,
                side,
            },
            until,
        });
        self.wake()
    }

    /// Turns the mouse wheel by `delta` at `at`. A precision touchpad turns it a part of a notch at a time, so the parts add
    /// up until they make a notch, and turning the other way starts counting again. Each notch is a step: of the
    /// magnification with Inspect active, forward for a level higher (v1 spec WPC-296, Inspect), and of the tolerance
    /// Dimensions, Bounds and Snip share with any other tool, forward for a step up, measured again at once (v1 spec
    /// WPC-296, Tolerance). Any turn shows what it steps, the magnification in the view's corner or the tolerance
    /// indicator, until a second after the wheel stops, even a turn past a limit that changes nothing (the owner's choices
    /// in WPC-309 and WPC-315). Turned at `at` on the toolbar, where the tool pauses, the wheel does nothing (the owner's
    /// choice in WPC-311).
    fn turn_wheel(&mut self, delta: i32, at: Point, uptime: Duration) -> Vec<Effect> {
        if self.pauses_at(at) {
            return Vec::new();
        }
        if self.wheel_turned.signum() == -delta.signum() {
            self.wheel_turned = 0;
        }
        let turned = self.wheel_turned.saturating_add(delta);
        let notches = turned / NOTCH;
        self.wheel_turned = turned % NOTCH;
        // Can't happen: Windows' uptime is milliseconds since it started, nowhere near Duration's limit. Should it, neither
        // the magnification indicator nor the tolerance indicator shows.
        if self.remembered.last_tool == Tool::Inspect {
            self.remembered.magnification = stepped(
                self.remembered.magnification,
                notches,
                Magnification::up,
                Magnification::down,
            );
            self.magnification_indicator_until = uptime.checked_add(MAGNIFICATION_INDICATOR_LASTS);
        } else {
            if notches != 0 {
                self.step_tolerance(notches);
            }
            self.tolerance_indicator_until = uptime.checked_add(TOLERANCE_INDICATOR_LASTS);
        }
        self.wake().into_iter().collect()
    }

    /// Moves the tolerance `notches` steps, up when they are above zero and down when below, stopping at its limits,
    /// and measures again.
    fn step_tolerance(&mut self, notches: i32) {
        self.tolerance = stepped(self.tolerance, notches, Tolerance::up, Tolerance::down);
        self.measure();
    }

    /// The wake for the soonest of what is on screen only for a while, the notice, the tolerance indicator and the
    /// magnification indicator: the one timer the Windows layer keeps serves them all, so the session
    /// always asks for the next moment one of them goes.
    fn wake(&self) -> Option<Effect> {
        let notice = self.notice.as_ref().map(|timed| timed.until);
        [
            notice,
            self.tolerance_indicator_until,
            self.magnification_indicator_until,
        ]
        .into_iter()
        .flatten()
        .min()
        .map(Effect::WakeAt)
    }

    /// Makes `tool` the active tool, by its key or its button on the toolbar, and the next call starts with it (v1 spec
    /// WPC-296, Trocar de ferramenta). Switching isn't closing: the frozen screen, the pinned measurements and the
    /// tolerance the wheel set all stay, and the new tool works on them from where the cursor is. Dimensions and Bounds
    /// measure, Inspect shows its magnified view, and Snip waits for a drag.
    fn switch_tool(&mut self, tool: Tool) -> Vec<Effect> {
        // A drag Bounds or Snip was drawing is dropped when the tool changes, even to the other of the two: the new tool
        // has no use for it, and the button it holds is the new tool's no more. A Snip drag must never end as a Bounds one
        // that pins and copies, nor the other way around.
        if tool != self.remembered.last_tool {
            self.drag = None;
            self.held_after_cancel = false;
        }
        self.remembered.last_tool = tool;
        self.measure();
        Vec::new()
    }

    /// Dimensions measures in `mode` from now on, and the next call starts in it, by its key or its button on the
    /// toolbar. The keys do nothing with another tool active, as the mode buttons show only with Dimensions on (v1 spec
    /// WPC-296, Toolbar).
    fn switch_mode(&mut self, mode: DimensionsMode) -> Vec<Effect> {
        if !self.dimensions_is_active() {
            return Vec::new();
        }
        self.remembered.dimensions_mode = mode;
        self.measure();
        Vec::new()
    }

    /// Shrinks the toolbar to its small button in the same place, or brings it back whole (v1 spec WPC-296, Toolbar). The
    /// tool goes on measuring: where the rest of the toolbar was is the tool's again, so with the cursor left there it
    /// measures at once, and the toolbar coming back over the cursor pauses it.
    fn minimize(&mut self, minimized: bool) -> Vec<Effect> {
        self.toolbar_minimized = minimized;
        self.measure();
        Vec::new()
    }

    /// What `at`, in physical pixels of the virtual screen, is on in the toolbar as it is now: `None` off it. Minimized,
    /// the toolbar is its small button alone; whole, H, V and C count only while they show. The Windows layer asks this
    /// which button a click is on, and the tool pauses wherever it isn't `None`.
    pub fn toolbar_hit(&self, at: Point) -> Option<toolbar::Hit> {
        if self.toolbar_minimized {
            self.toolbar_layout.hit_minimized(at)
        } else {
            self.toolbar_layout.hit(at, self.dimensions_is_active())
        }
    }

    /// Whether the tool pauses with the cursor at `at`: on the toolbar, and anywhere while the toolbar is dragged, as the
    /// cursor may leave it where it stops at a margin, the other monitor included (the owner's choice in WPC-312). While
    /// Bounds or Snip draws a rectangle, nowhere: the rectangle may go over the toolbar, which the photo doesn't have.
    /// Nor while the button that drew a rectangle Esc cancelled is still down: the press is still the tool's.
    fn pauses_at(&self, at: Point) -> bool {
        !self.tool_holds_the_press()
            && (self.toolbar_grab.is_some() || self.toolbar_hit(at).is_some())
    }

    /// Puts the cursor at `to`, the dragged toolbar following it, notes how far the drag Bounds or Snip is drawing has
    /// gone from its start, and measures there. The arrow keys come here too, so they nudge a held toolbar a pixel at a
    /// time, or ten with Shift (the owner's choice in WPC-312), and count towards the drag distance as the mouse going
    /// that far would (the owner's choice in WPC-314).
    fn move_cursor(&mut self, to: Point) {
        self.cursor = to;
        self.drag_toolbar();
        if let Some(drag) = self.drag.as_mut() {
            drag.went_to(to);
        }
        self.measure();
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
        self.move_cursor(to);
        vec![Effect::MoveCursor(to)]
    }

    /// Whether Dimensions is the active tool. The active tool is the remembered last tool: the one the tools opened
    /// with, until a key or the toolbar switches it.
    fn dimensions_is_active(&self) -> bool {
        self.remembered.last_tool == Tool::Dimensions
    }

    /// Measures again: Dimensions around the cursor, or, with the cursor on the toolbar, where the tool paused, so a mode
    /// switched from the toolbar shows at once where the user was measuring (v1 spec WPC-296, Toolbar; the owner's
    /// choice in WPC-311); Bounds the content of the rectangle being drawn, up to the cursor, and Snip the area it would
    /// take the image of. The photo has no toolbar and no pinned measurement in it, so the tools measure through both.
    /// Inspect's magnified view is worked out from where the tool measures whenever the scene is asked for.
    fn measure(&mut self) {
        if !self.pauses_at(self.cursor) {
            self.measured_at = Some(self.cursor);
        }
        self.live = match self.remembered.last_tool {
            Tool::Dimensions => self.measured_at.and_then(|at| {
                dimensions::measure(
                    &self.frozen_screen,
                    at,
                    self.remembered.dimensions_mode,
                    self.tolerance,
                    self.also_show_logical_pixels,
                )
            }),
            // Until the drag counts, the preview shows nothing, not even the drawn rectangle, as the button coming up then
            // does nothing (the owner's choice in WPC-314).
            Tool::Bounds => self.drag.filter(|drag| !drag.is_click()).and_then(|drag| {
                bounds::measure(
                    &self.frozen_screen,
                    drag,
                    self.cursor,
                    self.tolerance,
                    self.also_show_logical_pixels,
                )
            }),
            Tool::Inspect | Tool::Snip => None,
        };
        self.snip_area = match self.remembered.last_tool {
            Tool::Snip => self.drag.filter(|drag| !drag.is_click()).and_then(|drag| {
                let photo = self.frozen_screen.get(drag.monitor)?;
                let rect = bounds::area(photo, drag, self.cursor, self.tolerance)?;
                Some(SnipArea {
                    monitor: drag.monitor,
                    rect,
                    label: units::label(&[rect.width, rect.height], photo.scale, false),
                })
            }),
            Tool::Dimensions | Tool::Bounds | Tool::Inspect => None,
        };
    }

    /// Closes the tools, erasing every pinned measurement, the notice, the tolerance indicator and the magnified view's
    /// magnification indicator, and has the remembered state kept when the session changed it, the magnification the
    /// wheel left included (v1 spec WPC-296, Fechar as ferramentas). Closing comes first, so the screen is back before
    /// the settings file is written. The tolerance the mouse wheel set is never kept: the next call starts at the
    /// default tolerance in Settings.
    fn close(&mut self) -> Vec<Effect> {
        self.closed = true;
        self.toolbar_grab = None;
        self.drag = None;
        self.held_after_cancel = false;
        self.pinned.clear();
        self.notice = None;
        self.tolerance_indicator_until = None;
        self.magnification_indicator_until = None;
        let mut effects = vec![Effect::Close];
        if self.remembered != self.remembered_at_opening {
            effects.push(Effect::SaveRemembered(self.remembered.clone()));
        }
        effects
    }

    /// The tolerance indicator where the cursor is, while it shows.
    fn tolerance_indicator(&self) -> Option<ToleranceIndicator> {
        self.tolerance_indicator_until?;
        let monitor = frozen_screen::monitor_at(&self.frozen_screen, self.cursor)?;
        Some(ToleranceIndicator {
            tolerance: self.tolerance,
            at: self.cursor,
            monitor,
        })
    }

    /// The rectangle Bounds or Snip is drawing, dashed while the button is held once the drag counts, and without Alt.
    fn drawn_rectangle(&self) -> Option<DrawnRectangle> {
        let drag = self.drag.filter(|drag| drag.shows_dashes())?;
        let Some(photo) = self.frozen_screen.get(drag.monitor) else {
            // Can't happen: a drag begins on a monitor of the frozen screen.
            return None;
        };
        Some(DrawnRectangle {
            monitor: drag.monitor,
            rect: bounds::drawn_rectangle(photo, drag, self.cursor),
        })
    }

    /// Inspect's magnified view where the tool measures: around the cursor, or, while the cursor pauses on the toolbar,
    /// around the last place it had off it, as every tool pauses there (the owner's choice in WPC-311; the prototype hid
    /// the view instead). `None` with another tool active, or before the cursor has been off the toolbar.
    fn magnified_view(&self) -> Option<MagnifiedView> {
        if self.remembered.last_tool != Tool::Inspect {
            return None;
        }
        let center = self.measured_at?;
        let (monitor, photo) = frozen_screen::photo_at(&self.frozen_screen, center)?;
        Some(magnified_view::magnified_view(
            photo,
            monitor,
            center,
            self.remembered.magnification,
            self.magnification_indicator_until.is_some(),
            magnified_view::ReadoutOptions {
                also_show_logical_pixels: self.also_show_logical_pixels,
                color_format: self.color_format,
            },
        ))
    }

    /// What to draw now.
    pub fn scene(&self) -> Scene<'_> {
        Scene {
            frozen_screen: &self.frozen_screen,
            cursor: if self.pauses_at(self.cursor) {
                Cursor::Arrow
            } else {
                Cursor::Cross
            },
            live: self.live.as_ref(),
            drawn_rectangle: self.drawn_rectangle(),
            snip_area: self.snip_area.clone(),
            pinned: &self.pinned,
            magnified_view: self.magnified_view(),
            tolerance_indicator: self.tolerance_indicator(),
            notice: self.notice.as_ref().map(|timed| &timed.notice),
            toolbar: Toolbar {
                monitor: self.toolbar_monitor,
                corner: self.toolbar_corner(),
                active: self.remembered.last_tool,
                mode: self
                    .dimensions_is_active()
                    .then_some(self.remembered.dimensions_mode),
                minimized: self.toolbar_minimized,
                // While the toolbar is held, or the button is held for Bounds or Snip, no button reacts, whatever the cursor
                // slides over.
                hovered: match self.toolbar_hit(self.cursor) {
                    Some(toolbar::Hit::Button(button))
                        if self.toolbar_grab.is_none() && !self.tool_holds_the_press() =>
                    {
                        Some(button)
                    }
                    Some(_) | None => None,
                },
            },
        }
    }
}

/// What the notice says once the Snip's `file` was written, or couldn't be, and of the image's `copy` to the clipboard
/// with Destination at Both (v1 spec WPC-296, Snip; WPC-352).
fn file_notice(file: FileAnswer, copy: ImageCopy) -> Message {
    match (file, copy.answer()) {
        (FileAnswer::Saved, None) => Message::Saved,
        (FileAnswer::Saved, Some(CopyAnswer::Made)) => Message::CopiedAndSaved,
        (FileAnswer::Saved, Some(CopyAnswer::Refused)) => Message::CouldntCopyButSaved,
        (FileAnswer::NotSaved, None) => Message::CouldntSave,
        (FileAnswer::NotSaved, Some(CopyAnswer::Made)) => Message::CouldntSaveButCopied,
        (FileAnswer::NotSaved, Some(CopyAnswer::Refused)) => Message::CouldntSaveOrCopy,
    }
}

/// `value` moved `notches` steps, by `up` when they are above zero and by `down` when below, as the mouse wheel moves the
/// tolerance and the magnification a step a notch. It stops at the first step that changes nothing: a limit, where the
/// rest of the notches change nothing either.
fn stepped<T: Copy + PartialEq>(value: T, notches: i32, up: fn(T) -> T, down: fn(T) -> T) -> T {
    let step = if notches > 0 { up } else { down };
    let mut value = value;
    for _ in 0..notches.unsigned_abs() {
        let next = step(value);
        if next == value {
            break;
        }
        value = next;
    }
    value
}
