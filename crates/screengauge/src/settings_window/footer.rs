//! Settings' footer (`.sfoot`, WPC-279): its line on top, the version and the license on the left, and on the right
//! the "Project page" and "Check for updates" links. Once "Check for updates" is clicked in the installer copy, a line
//! under them, at the right (`.ustat`), says "Checking…" and then GitHub's answer (WPC-335).

use screengauge_core::session::Sign;
use screengauge_core::session::toolbar::scaled;
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F;
use windows::Win32::Graphics::Direct2D::{
    D2D1_ANTIALIAS_MODE_ALIASED, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, ID2D1RenderTarget,
    ID2D1SolidColorBrush,
};
use windows::Win32::Graphics::DirectWrite::{DWRITE_FONT_WEIGHT_NORMAL, IDWriteTextFormat};

use super::layout::{self, Span};
use super::paint::{Placed, Surface, draw_text, place, text, width_of};
use crate::graphics::solid_brush;
use crate::look::{self, Palette, dpi_scale, px, whole};
use crate::update_check::Answer;

/// The footer's left text: the version this exe was built as, and the license.
pub const VERSION: &str = concat!("Version ", env!("CARGO_PKG_VERSION"));
pub const LICENSE: &str = "Open source (MIT)";

/// What "Check for updates"' line says.
const CHECKING: &str = "Checking…";
const LATEST: &str = "You're on the latest version.";
const COULDNT_CHECK: &str = "Couldn't check. Check your connection and try again.";

/// How far below the baseline a link's underline starts (`text-underline-offset: 2px`).
const UNDERLINE_OFFSET: u32 = 2;

/// The sign before an answer (the prototype's `IC.ok` and `IC.bad`, 13 pixels).
const SIGN: u32 = 13;

/// The footer's links.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Link {
    ProjectPage,
    CheckForUpdates,
    /// After "Version X is available.": the latest release's page.
    Download,
}

impl Link {
    pub fn words(self) -> &'static str {
        match self {
            Link::ProjectPage => "Project page",
            Link::CheckForUpdates => "Check for updates",
            Link::Download => "Download",
        }
    }
}

/// What "Check for updates"' line shows, from the click on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// GitHub hasn't answered yet.
    Checking,
    Answered(Answer),
}

/// One piece of "Check for updates"' line, left to right.
enum Piece {
    Sign(Sign),
    Words(String),
    Link(Link),
}

impl Status {
    /// What the line holds, left to right, as the prototype has it.
    fn pieces(self) -> Vec<Piece> {
        match self {
            Status::Checking => vec![Piece::Words(CHECKING.to_string())],
            Status::Answered(Answer::Latest) => {
                vec![Piece::Sign(Sign::Ok), Piece::Words(LATEST.to_string())]
            }
            Status::Answered(Answer::Available(version)) => vec![
                Piece::Words(format!("Version {version} is available.")),
                Piece::Link(Link::Download),
            ],
            Status::Answered(Answer::CouldntCheck) => vec![
                Piece::Sign(Sign::Alert),
                Piece::Words(COULDNT_CHECK.to_string()),
            ],
        }
    }
}

/// Draws the footer in `span`, with `status`' line under its texts when it has one, and returns where its links are.
pub fn draw(
    on: &Surface,
    palette: &Palette,
    span: Span,
    status: Option<Status>,
) -> windows::core::Result<Vec<(Link, RECT)>> {
    let (target, fonts, dpi) = (on.target, on.fonts, on.dpi);
    let width = px(on.width);
    let edge = solid_brush(target, palette.edge)?;
    let dim = solid_brush(target, palette.dim)?;
    let link = solid_brush(target, palette.link)?;
    let top = px(scaled(span.top, dpi));
    let border = px(scaled(1, dpi).max(1));
    let rule = D2D_RECT_F {
        left: 0.0,
        top,
        right: width,
        bottom: top + border,
    };
    // SAFETY: drawing calls on a live render target, with arguments that outlive them. The rule is on whole pixels.
    unsafe {
        target.SetAntialiasMode(D2D1_ANTIALIAS_MODE_ALIASED);
        target.FillRectangle(&rule, &edge);
        target.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
    }
    let format = fonts.text_format(
        DWRITE_FONT_WEIGHT_NORMAL,
        px(scaled(layout::FOOTER_TEXT, dpi)),
    )?;
    // The texts' line starts below the rule and the padding.
    let line = top + border + px(scaled(layout::FOOTER_ABOVE, dpi));
    let line_height = layout::FOOTER_LINE * dpi_scale(dpi);
    let gap = px(scaled(layout::FOOTER_GAP, dpi));
    let side = px(scaled(layout::SIDE, dpi));
    let mut left = side;
    for words in [VERSION, "·", LICENSE] {
        let placed = place(text(fonts, words, &format, None)?, left, line, line_height)?;
        draw_text(target, &placed, &dim);
        left = (placed.left + placed.width + gap).round();
    }
    // On the right: "Project page", a dot, and "Check for updates".
    let mut links = right_aligned(
        on,
        &[
            Piece::Link(Link::ProjectPage),
            Piece::Words("·".to_string()),
            Piece::Link(Link::CheckForUpdates),
        ],
        &format,
        Line {
            top: line,
            height: line_height,
            gap,
        },
        &dim,
        &link,
    )?;
    if let Some(status) = status {
        // Under them, "Check for updates"' line, in the rows' ink.
        let ink = solid_brush(target, palette.ink)?;
        let line = Line {
            top: px(scaled(layout::status_line(span), dpi)),
            height: line_height,
            gap: px(scaled(layout::STATUS_GAP, dpi)),
        };
        links.extend(right_aligned(
            on,
            &status.pieces(),
            &format,
            line,
            &ink,
            &link,
        )?);
    }
    Ok(links)
}

/// A line of the footer: where it starts, how tall it is, and the room between its pieces.
#[derive(Clone, Copy)]
struct Line {
    top: f32,
    height: f32,
    gap: f32,
}

/// Draws `pieces` on `line`, ending at the footer's right side less its room, the words in `words` and the links in
/// `links`, a sign in the accent's color or the warning's, and returns where the links are. Each piece starts on a
/// whole pixel, so the last one never reaches into the margin.
fn right_aligned(
    on: &Surface,
    pieces: &[Piece],
    format: &IDWriteTextFormat,
    line: Line,
    words: &ID2D1SolidColorBrush,
    links: &ID2D1SolidColorBrush,
) -> windows::core::Result<Vec<(Link, RECT)>> {
    let (target, fonts, dpi) = (on.target, on.fonts, on.dpi);
    let mut found = Vec::new();
    let mut right = (px(on.width) - px(scaled(layout::SIDE, dpi))).floor();
    for piece in pieces.iter().rev() {
        let said = match piece {
            Piece::Sign(sign) => {
                let side = px(scaled(SIGN, dpi));
                let left = right - side;
                // Centered on the text's line, as `align-items: center` puts it.
                let top = (line.top + (line.height - side) / 2.0).round();
                let palette = look::palette(on.mode);
                let color = match sign {
                    Sign::Ok => palette.accent,
                    Sign::Alert => palette.warn,
                };
                let pen = crate::sign::Pen {
                    target,
                    factory: on.factory,
                    stroke: on.stroke,
                };
                crate::sign::draw(&pen, &solid_brush(target, color)?, *sign, left, top, side)?;
                right = left - line.gap;
                continue;
            }
            Piece::Words(said) => said.as_str(),
            Piece::Link(link) => link.words(),
        };
        let layout = text(fonts, said, format, None)?;
        let left = (right - width_of(&layout)?).floor();
        let placed = place(layout, left, line.top, line.height)?;
        match piece {
            Piece::Link(link) => {
                found.push((*link, underlined(target, &placed, links, line, dpi)?))
            }
            Piece::Words(_) | Piece::Sign(_) => draw_text(target, &placed, words),
        }
        right = placed.left - line.gap;
    }
    // Laid from the right; given back left to right.
    found.reverse();
    Ok(found)
}

/// Draws `placed` as a link, underlined in `brush`, and returns where a click on it acts: as tall as `line`, as wide as
/// its words.
fn underlined(
    target: &ID2D1RenderTarget,
    placed: &Placed,
    brush: &ID2D1SolidColorBrush,
    line: Line,
    dpi: u32,
) -> windows::core::Result<RECT> {
    draw_text(target, placed, brush);
    // The underline is 1 pixel thick up to 175% and grows with the scale beyond, as a browser rounds it.
    let underline_top = placed.top + placed.baseline + px(scaled(UNDERLINE_OFFSET, dpi));
    let underline = D2D_RECT_F {
        left: placed.left,
        top: underline_top,
        right: placed.left + placed.width,
        bottom: underline_top + px((dpi / 96).max(1)),
    };
    // SAFETY: drawing calls on a live render target, with arguments that outlive them.
    unsafe {
        target.SetAntialiasMode(D2D1_ANTIALIAS_MODE_ALIASED);
        target.FillRectangle(&underline, brush);
        target.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
    }
    Ok(RECT {
        left: whole(placed.left),
        top: whole(line.top.floor()),
        right: whole((placed.left + placed.width).ceil()),
        bottom: whole((line.top + line.height).ceil()),
    })
}
