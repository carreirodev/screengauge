//! What the session core gives the Windows layer: the scene to draw and the effects to carry out.

use std::path::PathBuf;
use std::time::Duration;

use super::{FileNames, Image, Photo, Point, Rect, ToolbarButton};
use crate::settings::{DimensionsMode, Folder, Magnification, Remembered, Tolerance, Tool};

/// What the Windows layer draws while the tools are open.
#[derive(Debug, Clone, PartialEq)]
pub struct Scene<'a> {
    /// Every monitor's photo, drawn pixel for pixel over its monitor, in the frozen screen's order. Nothing marks the
    /// screen as frozen.
    pub frozen_screen: &'a [Photo],
    /// The cursor over the frozen screen: the normal arrow on the toolbar, and a thin cross everywhere else (v1 spec
    /// WPC-296, Toolbar).
    pub cursor: Cursor,
    /// What the active tool measures right now, drawn over the photo of its monitor, over the pinned measurements:
    /// Dimensions around the cursor, or Bounds the content of the rectangle being drawn. `None` while no tool measures.
    pub live: Option<&'a Measurement>,
    /// The rectangle Bounds or Snip is drawing while the button is held, drawn under every measurement on its monitor.
    /// `None` the rest of the time.
    pub drawn_rectangle: Option<DrawnRectangle>,
    /// The area Snip takes the image of as the button comes up, while it is held, drawn over the drawn rectangle and the
    /// pinned measurements on its monitor. `None` the rest of the time, and while the drawn rectangle holds no content.
    pub snip_area: Option<SnipArea>,
    /// The pinned measurements, oldest first, each drawn over the photo of the monitor it was measured on, whichever
    /// monitor the cursor is on. Their labels are in the theme's colors, the live one's in cyan, so the one that still
    /// changes stands out (v1 spec WPC-296, Como as medidas aparecem).
    pub pinned: &'a [Measurement],
    /// Inspect's magnified view, drawn over the measurements on its monitor and under the toolbar, as the prototype
    /// stacks them (WPC-279). `None` while another tool is active, and while the cursor hasn't been off the toolbar since
    /// the tools opened.
    pub magnified_view: Option<MagnifiedView>,
    /// The tolerance indicator, drawn over the measurements on the cursor's monitor and under the notice. `None` once
    /// the mouse wheel hasn't changed the tolerance for a second.
    pub tolerance_indicator: Option<ToleranceIndicator>,
    /// The notice on screen, drawn over everything else on its monitor. `None` when there is none.
    pub notice: Option<&'a Notice>,
    /// The toolbar, on screen the whole time the tools are open, drawn over the measurements on its monitor and under
    /// the tolerance indicator and the notice.
    pub toolbar: Toolbar,
}

/// The rectangle the user drags out in Bounds, exactly as drawn, before any shrinking to the content: dashed in black
/// and white on screen while the button is held (v1 spec WPC-296, Como as medidas aparecem).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DrawnRectangle {
    /// The monitor the drag began on, as an index in the frozen screen's order. The rectangle never leaves it.
    pub monitor: usize,
    /// Its pixels, both corners included, in physical pixels of the virtual screen.
    pub rect: Rect,
}

/// What Snip shows while the button is held (v1 spec WPC-296, Snip): cyan corners around the area it takes the image of,
/// with no fill, and a cyan label with a camera and the image's size, in physical pixels alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnipArea {
    /// The monitor the drag began on, as an index in the frozen screen's order. The area never leaves it.
    pub monitor: usize,
    /// The image's pixels, both corners included, in physical pixels of the virtual screen: the content of the drawn
    /// rectangle, or with Alt the drawn rectangle itself.
    pub rect: Rect,
    /// The image's size, `100 × 40`. It never has a logical part: an image is in physical pixels.
    pub label: Label,
}

/// The toolbar (v1 spec WPC-296, Toolbar): the tools' buttons with the active tool's lit, then H, V and C in a room kept
/// for them, then the gear, minimize and close; or, minimized, one small cyan button with the active tool's icon and a
/// small arrow, in the same place. Its parts are where [`toolbar::layout`](super::toolbar::layout) puts them from its
/// corner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Toolbar {
    /// The monitor it is on, as an index in the frozen screen's order: the current monitor as the tools opened. It stays
    /// there while the cursor goes to another monitor, where the tool then measures, and while it is dragged.
    pub monitor: usize,
    /// Its top left corner, in physical pixels of the virtual screen, which the minimized toolbar's button shares: at the
    /// remembered place on its monitor as the tools open, and wherever a drag takes it inside that monitor (WPC-312).
    pub corner: Point,
    /// The active tool, whose button is lit. There is always one.
    pub active: Tool,
    /// Dimensions' mode, whose button is lit, while Dimensions is the active tool. `None` with any other tool: then H, V
    /// and C don't show, and their room stays empty so no other button moves.
    pub mode: Option<DimensionsMode>,
    /// Whether it is minimized to its small button. Every call opens it whole.
    pub minimized: bool,
    /// The button the cursor is on, which reacts to it as the prototype's buttons do to hovering. `None` with the cursor
    /// off every button.
    pub hovered: Option<ToolbarButton>,
}

/// What the mouse wheel has just made the tolerance (v1 spec WPC-296, Tolerance): "Tolerance N" and a bar of ten
/// segments, as many of them lit as the tolerance's share of the highest one, on a small panel to the left of the
/// cursor: above it, or below it in Dimensions' horizontal mode and wherever above it would hide the measurement's
/// label (the owner's choices in WPC-309). It follows the cursor, and goes a second after the wheel stops. It is no
/// notice: it says what a value is now, not that something was done.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToleranceIndicator {
    /// The tolerance now: the number Settings' − / + shows for the default tolerance.
    pub tolerance: Tolerance,
    /// Where the cursor is, in physical pixels of the virtual screen.
    pub at: Point,
    /// The monitor the cursor is on, as an index in the frozen screen's order.
    pub monitor: usize,
}

/// Inspect's magnified view (v1 spec WPC-296, Inspect; the prototype's variant A, WPC-279): a panel a little below and
/// to the right of the cursor, or on the other side of it near the monitor's limits, never leaving the current monitor.
/// It holds a square of the frozen screen's pixels around the cursor, each enlarged to a square of the magnification's
/// side without smoothing, and under it a readout with the cursor's position and the color of the pixel under it. Every
/// rectangle is in physical pixels of the virtual screen, worked out in the session core from sizes in logical pixels,
/// so nothing in it depends on a font.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MagnifiedView {
    /// The current monitor, as an index in the frozen screen's order. Everything below is inside it.
    pub monitor: usize,
    /// The panel, its border included: the palette's floating panel, with corners of 14 logical pixels.
    pub panel: Rect,
    /// Where the enlarged pixels go, inside the panel: a square `source.width` times the magnification a side, its
    /// corners rounded by 9 logical pixels.
    pub enlargement: Rect,
    /// The frozen screen's pixels the enlargement stands for: an odd number of them a side, so the pixel under the
    /// cursor is the exact middle one. Near the current monitor's limit part of it is past that limit: the enlargement
    /// is plain and empty there, in the palette's blank color, with no grid and no pixel of the monitor beside it.
    pub source: Rect,
    /// The part of `source` on the current monitor, which is what the enlargement draws, each pixel enlarged to a
    /// square of the magnification's side. Never empty: the pixel under the cursor is on it.
    pub shown: Rect,
    /// The pixel under the cursor, in the middle of `source`, outlined in black and then white in the enlargement.
    pub center: Point,
    pub magnification: Magnification,
    /// Whether a 1-pixel grid parts the enlarged pixels: from 8× up. It covers `shown` alone.
    pub grid: bool,
    /// Whether the magnification shows, as `8×`, in the enlargement's top left corner: for a second after the mouse
    /// wheel turns, even when it turned past a limit and changed nothing (the owner's choice in WPC-315).
    pub magnification_indicator: bool,
    /// The cursor's position on the readout under the enlargement: how far it is from the current monitor's left side.
    pub x: Coordinate,
    /// How far it is from the current monitor's top.
    pub y: Coordinate,
    /// The color of the pixel under the cursor, on the readout under its position.
    pub color: PixelColor,
    /// The Approximate color notice under the color, while the view is on a monitor whose photo didn't come through
    /// the exact path. `None` on every other monitor, where the readout ends with the color.
    pub approximate_color: Option<ApproximateColor>,
}

/// The Approximate color notice on the magnified view's readout (v1 spec WPC-296, Inspect and Colors and HDR): the
/// alert sign in the warning's amber, and beside it "Approximate color" above "(HDR on)", semibold, 11 logical pixels
/// tall and in the amber too (the prototype's `.hdr`). It is the one notice that stays, for as long as the view is on
/// that monitor, and Inspect works as ever: the click still copies the color. The text takes two lines whatever the
/// magnification, since it doesn't fit one at every magnification and scale (the owner's choice in WPC-328), so on that
/// monitor the panel keeps its height as the mouse wheel turns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApproximateColor {
    /// The alert sign's square, 13 logical pixels a side, at the readout's left and centered beside the two lines: the
    /// notices' exclamation mark in a circle.
    pub sign: Rect,
    /// "Approximate color", then "(HDR on)" under it, after the sign.
    pub lines: [NoticeLine; 2],
}

/// One line of the Approximate color notice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoticeLine {
    /// What it says. ScreenGauge is in English only (WPC-293).
    pub text: &'static str,
    /// Where it goes: the line's height, starting at the text's left, after the sign.
    pub line: Rect,
}

/// The color of the pixel under the cursor on the magnified view's readout (v1 spec WPC-296, Inspect): a swatch of it
/// at the readout's left, and beside it its HEX, its RGB, or its HEX above its RGB, as Color format says. HEX, RGB and
/// the swatch don't fit one line of the readout at every magnification and scale, so with both formats HEX and RGB take
/// a line each, whatever the color, and the panel keeps its size as the cursor moves (the owner's choice in WPC-316).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PixelColor {
    /// Its red, green and blue, exactly as the frozen screen has the pixel: never averaged with its neighbors.
    pub rgb: [u8; 3],
    /// The swatch, filled with the color: a square of 22 logical pixels with a 1-pixel ring inside it in the palette's
    /// border color and corners of 6, centered in the color's row.
    pub swatch: Rect,
    /// What the click copies, in the ink: `#1E90FF`, or `30, 144, 255` with Color format at RGB.
    pub first: ColorText,
    /// `30, 144, 255` under the HEX, fainter, with Color format at Both, which Shift + click copies. `None` with HEX or
    /// RGB alone, when Shift + click copies the other format all the same.
    pub second: Option<ColorText>,
}

/// One of the color's texts on the magnified view's readout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColorText {
    /// `#1E90FF` or `30, 144, 255`, exactly what the click or Shift + click copies.
    pub text: String,
    /// Where it goes: the line's height, starting at the text's left, after the swatch.
    pub line: Rect,
}

/// One of the cursor's coordinates on the magnified view's readout: `X` or `Y`, the physical pixels, and, after a thin
/// bar and fainter, the logical pixels, by the rule of Units (v1 spec WPC-296, Inspect).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Coordinate {
    /// Counted from the current monitor's left side (X) or top (Y), so it is never below zero, whichever monitor it
    /// is: the monitor's first column is 0.
    pub label: Label,
    /// Where its text goes on the readout: the line's height, starting at the text's left. X and Y share a line, side
    /// by side, when the label has no logical part, and take one line each, X above Y, when it has (the owner's choice
    /// in WPC-315: both side by side don't fit the readout).
    pub line: Rect,
}

/// A short message ScreenGauge draws on screen itself (v1 spec WPC-296, Como as medidas aparecem): small, with an ok or
/// an alert sign before its text, above and to the right of where the cursor was when it appeared, or below it where
/// the magnified view is above it (WPC-367). It goes by itself a moment later.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Notice {
    pub message: Message,
    /// Where the cursor was when the notice appeared, in physical pixels of the virtual screen. It stays there while
    /// the cursor moves on, kept inside its monitor.
    pub at: Point,
    /// The monitor it is drawn on, as an index in the frozen screen's order.
    pub monitor: usize,
    /// Which side of `at` it goes, decided where the cursor was, as `at` is.
    pub side: NoticeSide,
}

/// Which side of where the cursor was a notice goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoticeSide {
    /// Above, as the spec says (v1 spec WPC-296, Como as medidas aparecem), or below where the monitor's top leaves no
    /// room above.
    Above,
    /// Below: Inspect's magnified view went above the cursor, near the monitor's bottom, and the notice takes the place
    /// the view would have had, so it doesn't hide the color just copied (the owner's choice in WPC-367).
    Below,
}

/// What a notice says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Message {
    /// A copy to the clipboard was made.
    Copied,
    /// A drawn rectangle held nothing but the reference color, with the tool that drew it: Bounds pinned and copied
    /// nothing (v1 spec WPC-296, Bounds), and Snip took no image, so the clipboard stays as it was (v1 spec WPC-296,
    /// Snip). It says how to take the drawn rectangle itself, in the tool's words, since nothing else on screen tells
    /// of Alt (the owner's choice in WPC-314).
    NoContentFound(Tool),
    /// The Snip's image was written to a file, with Destination at File.
    Saved,
    /// The Snip's image went to the clipboard and was written to a file, with Destination at Both.
    CopiedAndSaved,
    /// The Snip's file couldn't be written, and no file was left behind, with Destination at File.
    CouldntSave,
    /// The same with Destination at Both, where the image went to the clipboard all the same.
    CouldntSaveButCopied,
    /// Windows refused the clipboard, so the copy wasn't made and the clipboard holds what it held before (WPC-352). A
    /// measurement stays pinned all the same.
    CouldntCopy,
    /// With Destination at Both, the Snip's image was written to a file, but Windows refused the clipboard.
    CouldntCopyButSaved,
    /// With Destination at Both, neither the file nor the copy could be made.
    CouldntSaveOrCopy,
}

impl Message {
    /// The text on screen. ScreenGauge is in English only (WPC-293).
    pub fn text(self) -> &'static str {
        match self {
            Message::Copied => "Copied",
            Message::NoContentFound(Tool::Snip) => {
                "No content found · Hold Alt to snip the rectangle"
            }
            Message::NoContentFound(_) => "No content found · Hold Alt to measure the rectangle",
            Message::Saved => "Saved",
            Message::CopiedAndSaved => "Copied and saved",
            Message::CouldntSave => "Couldn't save.",
            Message::CouldntSaveButCopied => "Couldn't save. The image was copied.",
            Message::CouldntCopy => "Couldn't copy.",
            Message::CouldntCopyButSaved => "Couldn't copy. The image was saved.",
            Message::CouldntSaveOrCopy => "Couldn't save or copy.",
        }
    }

    pub fn sign(self) -> Sign {
        match self {
            Message::Copied | Message::Saved | Message::CopiedAndSaved => Sign::Ok,
            Message::NoContentFound(_)
            | Message::CouldntSave
            | Message::CouldntSaveButCopied
            | Message::CouldntCopy
            | Message::CouldntCopyButSaved
            | Message::CouldntSaveOrCopy => Sign::Alert,
        }
    }
}

/// The icon before a notice's text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sign {
    /// A check mark: what the user asked for was done.
    Ok,
    /// An exclamation mark in a circle: it wasn't, as with "No content found".
    Alert,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cursor {
    /// Windows' thin cross, everywhere but on the toolbar.
    Cross,
    /// Windows' normal arrow, on the toolbar, where the tool pauses.
    Arrow,
}

/// What a tool measures, on one monitor: Dimensions' spans or Bounds' content, which share the pinned measurements.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Measurement {
    /// The monitor it was measured on, as an index in the frozen screen's order.
    pub monitor: usize,
    pub measured: Measured,
    /// What its label says.
    pub label: Label,
}

/// What a measurement holds: spans or content, never both.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Measured {
    /// Dimensions' spans around the cursor along its row, its column or both (Cross).
    Spans {
        /// The span along the cursor's row, in Dimensions' horizontal and Cross modes.
        horizontal: Option<Span>,
        /// The span along the cursor's column, in Dimensions' vertical and Cross modes.
        vertical: Option<Span>,
    },
    /// Bounds' content: the smallest rectangle that holds the content of a drawn rectangle, each side on a pixel of it,
    /// in physical pixels of the virtual screen.
    Content(Rect),
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
    /// The span along the cursor's row, in Dimensions' horizontal and Cross modes. `None` in Bounds.
    pub fn horizontal(&self) -> Option<Span> {
        match self.measured {
            Measured::Spans { horizontal, .. } => horizontal,
            Measured::Content(_) => None,
        }
    }

    /// The span along the cursor's column, in Dimensions' vertical and Cross modes. `None` in Bounds.
    pub fn vertical(&self) -> Option<Span> {
        match self.measured {
            Measured::Spans { vertical, .. } => vertical,
            Measured::Content(_) => None,
        }
    }

    /// Bounds' content. `None` in Dimensions.
    pub fn content(&self) -> Option<Rect> {
        match self.measured {
            Measured::Content(content) => Some(content),
            Measured::Spans { .. } => None,
        }
    }

    /// How many pixels each span has, or the content's width and height, width before height, as the label shows them.
    pub(crate) fn lengths(&self) -> Vec<u32> {
        match self.measured {
            Measured::Spans {
                horizontal,
                vertical,
            } => [horizontal, vertical]
                .into_iter()
                .flatten()
                .map(Span::length)
                .collect(),
            Measured::Content(content) => vec![content.width, content.height],
        }
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

/// Where the Snip's PNG file goes (v1 spec WPC-296, Snip).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaveTo {
    /// A new file in the default folder, under the first of `names` no file there has yet: an earlier file is never
    /// replaced. A folder that no longer exists is never made again: the file then isn't written.
    DefaultFolder { folder: Folder, names: FileNames },
    /// Exactly this path, chosen in Save As, replacing the file there, which Windows has already asked about.
    Chosen(PathBuf),
}

/// Something the Windows layer does for the session, in the order the session returns them.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    /// Put this text on the clipboard, and hand the session [`Input::Copied`](super::Input::Copied), or
    /// [`Input::NotCopied`](super::Input::NotCopied) when Windows refused the clipboard, before the next effect: the
    /// notice waits for that answer, so it never says "Copied" for a copy that wasn't made (WPC-352).
    CopyText(String),
    /// Put this image on the clipboard, and answer as for [`Effect::CopyText`].
    CopyImage(Image),
    /// Write this image as a PNG file, where `to` says, and hand the session [`Input::Saved`](super::Input::Saved), or
    /// [`Input::NotSaved`](super::Input::NotSaved) when no file could be written, leaving none behind.
    WritePng { to: SaveTo, image: Image },
    /// Open the Save As window, over the frozen screen, in `folder` (`None` is Pictures, and so is a folder that no
    /// longer exists), suggesting `file_name` and taking PNG files alone. Its closing comes back as
    /// [`Input::SaveAsClosed`](super::Input::SaveAsClosed).
    OpenSaveAs {
        folder: Option<Folder>,
        file_name: String,
    },
    /// Another program came to the front while Save As is open: the frozen screen and Save As go behind it, as any
    /// program's windows would, and wait as they are, with nothing lost (the owner's choice in WPC-318). The button
    /// Save As has on the taskbar and in Alt+Tab brings them back, which comes as
    /// [`Input::BackInFront`](super::Input::BackInFront).
    StepAside,
    /// Bring the frozen screen and Save As back in front, over every program, as they were before they stepped aside.
    ComeBack,
    /// Open the Settings window, at the center of the current monitor, or bring it to the front as it is when it is
    /// open already. It comes after [`Effect::Close`]: the gear closes the tools first.
    OpenSettings,
    /// Put the mouse cursor at this point, in physical pixels of the virtual screen, as the arrow keys do. The session
    /// has already measured there; the move Windows then reports changes nothing more.
    MoveCursor(Point),
    /// Hand the session [`Input::Wake`](super::Input::Wake) once Windows has been running this long
    /// ([`Event::uptime`](super::Event::uptime)): the moment something on screen goes by itself, such as a notice or
    /// the tolerance indicator. A later `WakeAt` takes the place of an earlier one still waiting, so the session always
    /// asks for the soonest of them, and once woken asks for the next.
    WakeAt(Duration),
    /// Close the tools: the frozen screen goes, its photos are erased, and the mouse goes back to the programs. The
    /// session takes no more events.
    Close,
    /// Keep this remembered state in the settings file.
    SaveRemembered(Remembered),
}
