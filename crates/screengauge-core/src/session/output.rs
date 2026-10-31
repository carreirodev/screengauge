//! What the session core gives the Windows layer: the scene to draw and the effects to carry out.

use std::path::PathBuf;

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
    /// What Dimensions measures around the cursor right now, drawn over the photo of its monitor. `None` while no tool
    /// measures.
    pub live: Option<&'a Measurement>,
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
    /// What the label says, in physical pixels: `24` for one span, `240 × 180` (width × height) for Cross.
    pub label: String,
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
    /// Close the tools: the frozen screen goes, its photos are erased, and the mouse goes back to the programs. The
    /// session takes no more events.
    Close,
    /// Keep this remembered state in the settings file.
    SaveRemembered(Remembered),
}
