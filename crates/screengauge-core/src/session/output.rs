//! What the session core gives the Windows layer: the scene to draw and the effects to carry out.

use std::path::PathBuf;
use std::time::Duration;

use super::{Image, Photo, Point};
use crate::settings::{Folder, Remembered};

/// What the Windows layer draws while the tools are open.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Scene<'a> {
    /// Every monitor's photo, drawn pixel for pixel over its monitor, in the frozen screen's order. Nothing marks the
    /// screen as frozen.
    pub frozen_screen: &'a [Photo],
    /// The cursor over the frozen screen.
    pub cursor: Cursor,
    /// What Dimensions measures around the cursor right now, drawn over the photo of its monitor, over the pinned
    /// measurements. `None` while no tool measures.
    pub live: Option<&'a Measurement>,
    /// The pinned measurements, oldest first, each drawn over the photo of the monitor it was measured on, whichever
    /// monitor the cursor is on. Their labels are in the theme's colors, the live one's in cyan, so the one that still
    /// changes stands out (v1 spec WPC-296, Como as medidas aparecem).
    pub pinned: &'a [Measurement],
    /// The notice on screen, drawn over everything else on its monitor. `None` when there is none.
    pub notice: Option<&'a Notice>,
}

/// A short message ScreenGauge draws on screen itself (v1 spec WPC-296, Como as medidas aparecem): small, with an ok or
/// an alert sign before its text, above and to the right of where the cursor was when it appeared. It goes by itself a
/// moment later.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Notice {
    pub message: Message,
    /// Where the cursor was when the notice appeared, in physical pixels of the virtual screen. It stays there while
    /// the cursor moves on, kept inside its monitor.
    pub at: Point,
    /// The monitor it is drawn on, as an index in the frozen screen's order.
    pub monitor: usize,
}

/// What a notice says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Message {
    /// A copy to the clipboard was made.
    Copied,
}

impl Message {
    /// The text on screen. ScreenGauge is in English only (WPC-293).
    pub fn text(self) -> &'static str {
        match self {
            Message::Copied => "Copied",
        }
    }

    pub fn sign(self) -> Sign {
        match self {
            Message::Copied => Sign::Ok,
        }
    }
}

/// The icon before a notice's text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sign {
    /// A check mark: what the user asked for was done.
    Ok,
    /// An exclamation mark in a circle: it wasn't, as with "No content found" (WPC-314).
    Alert,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cursor {
    /// Windows' thin cross.
    Cross,
}

/// A Dimensions measurement: the span around the cursor along its row, its column or both (Cross), on one monitor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Measurement {
    /// The monitor it was measured on, as an index in the frozen screen's order.
    pub monitor: usize,
    /// The span along the cursor's row, in the horizontal and Cross modes.
    pub horizontal: Option<Span>,
    /// The span along the cursor's column, in the vertical and Cross modes.
    pub vertical: Option<Span>,
    /// What its label says.
    pub label: Label,
}

/// What a measurement's label says (v1 spec WPC-296, Como as medidas aparecem): the physical pixels, then, after a thin
/// bar and fainter, the logical pixels.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Label {
    /// In physical pixels: `24` for one span, `240 × 180` (width × height) for Cross.
    pub physical: String,
    /// The same lengths in logical pixels, with a point before the tenths: `13.7`, `137.1 × 102.9`. `None` when the
    /// monitor's scale is 100% or "Also show logical pixels" is off, and then the label has no bar either.
    pub logical: Option<String>,
}

/// Pixels in a straight run along one row or one column, from the first to the last, both included: where a
/// measurement's line is drawn. The edges at either end are outside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    /// The leftmost or topmost pixel, in physical pixels of the virtual screen.
    pub first: Point,
    /// The rightmost or bottommost pixel. It is `first` for a span of one pixel.
    pub last: Point,
}

impl Measurement {
    /// How many pixels each span has, width before height, as the label shows them.
    pub(crate) fn lengths(&self) -> Vec<u32> {
        [self.horizontal, self.vertical]
            .iter()
            .flatten()
            .map(|span| span.length())
            .collect()
    }
}

impl Span {
    /// How many pixels the span has: the number Dimensions shows for it.
    pub fn length(self) -> u32 {
        let along = (self.last.x - self.first.x).max(self.last.y - self.first.y);
        // A span's last pixel never comes before its first, so `along` is never negative.
        along.unsigned_abs() + 1
    }
}

/// Something the Windows layer does for the session, in the order the session returns them.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    /// Put this text on the clipboard.
    CopyText(String),
    /// Put this image on the clipboard.
    CopyImage(Image),
    /// Write this image as a PNG file at `path`.
    WritePng { path: PathBuf, image: Image },
    /// Open the Save As window in `folder` (`None` is Pictures), suggesting `file_name`. Its closing comes back as
    /// [`Input::SaveAsClosed`](super::Input::SaveAsClosed).
    OpenSaveAs {
        folder: Option<Folder>,
        file_name: String,
    },
    /// Open the Settings window.
    OpenSettings,
    /// Put the mouse cursor at this point, in physical pixels of the virtual screen, as the arrow keys do. The session
    /// has already measured there; the move Windows then reports changes nothing more.
    MoveCursor(Point),
    /// Hand the session [`Input::Wake`](super::Input::Wake) once Windows has been running this long
    /// ([`Event::uptime`](super::Event::uptime)): the moment something on screen goes by itself, such as a notice. A
    /// later `WakeAt` takes the place of an earlier one still waiting.
    WakeAt(Duration),
    /// Close the tools: the frozen screen goes, its photos are erased, and the mouse goes back to the programs. The
    /// session takes no more events.
    Close,
    /// Keep this remembered state in the settings file.
    SaveRemembered(Remembered),
}
