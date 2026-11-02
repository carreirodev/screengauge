//! A message under a row's label in Settings, in amber (`.srow .msg`), and the amber band across the whole row
//! (`.srow.warn`) for a message that needs it (v1 spec WPC-296, Settings; WPC-323). The global shortcut recorder's
//! refusals show here first; the shortcut busy at start (WPC-324) and "Start with Windows" turned off in Windows
//! (WPC-326) use the same piece.
//!
//! The message wraps in the room between the label's left and the row's control. While it shows, its row is as tall as
//! the label and the message's lines need (`.srow`'s `min-height: 42px` and `padding: 5px 12px`), and the card, the page
//! and the window grow as much ([`Grown`]); the window is its usual height again once the message goes.
//!
//! The colors are the prototype's `--warn` (WPC-279), the same the tools' notices use (`frozen_screen::overlay::palette`).

use screengauge_core::session::toolbar::scaled;
use windows::Win32::Foundation::{E_FAIL, RECT};
use windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F;
use windows::Win32::Graphics::Direct2D::{
    D2D1_ANTIALIAS_MODE_ALIASED, D2D1_ROUNDED_RECT, ID2D1Factory, ID2D1RenderTarget,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_LINE_METRICS, DWRITE_LINE_SPACING_METHOD_UNIFORM,
    DWRITE_TEXT_METRICS, DWRITE_WORD_WRAPPING_WRAP, IDWriteTextLayout,
};
use windows::core::Error;

use super::layout::{self, Grown, Row, Span};
use super::paint::to_i32;
use crate::graphics::{Color, Fonts, color, solid_brush};
use crate::relief::{self, px};
use crate::theme::Mode;

/// `--warn` in the dark and the light palette: oklch(0.82 0.14 80) and oklch(0.58 0.15 60).
pub const DARK_WARN: Color = color(0xf3, 0xb9, 0x4c, 1.0);
pub const LIGHT_WARN: Color = color(0xb8, 0x60, 0x00, 1.0);

pub fn warn(mode: Mode) -> Color {
    match mode {
        Mode::Dark => DARK_WARN,
        Mode::Light => LIGHT_WARN,
    }
}

/// How much of `--warn` the band is (`color-mix(in oklch, var(--warn) 14%, transparent)`), and its bar's width on the
/// row's left (`box-shadow: inset 3px 0 0 var(--warn)`).
const BAND: f32 = 0.14;
const BAR: u32 = 3;

/// Above the message, below the label or the note (`.msg`'s `margin-top`).
pub const GAP: f32 = 2.0;

/// A row's room above and below its label (`.srow`'s padding), and its least height (`min-height`).
const ROW_PADDING: f32 = 5.0;

/// The message's size (`.msg`: `font-size: 11.5px`, `font-weight: 600`, on `#settings`' line-height of 1.3): the note's.
const TEXT: f32 = layout::NOTE_TEXT;
const LINE: f32 = layout::NOTE_LINE;

/// A message showing under a row's label.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Message {
    pub row: Row,
    pub text: &'static str,
    /// Whether the whole row shows the amber band too.
    pub band: bool,
    /// The room for its lines, in logical pixels: from the label's left to the row's control, less the gap before it.
    pub width: f32,
}

impl Message {
    /// The message laid out at `scale`, wrapped in its room, its lines `#settings`' line-height apart, as CSS spaces
    /// them: each line's text centered in its line. Comes with its lines' count and the first line's baseline, from the
    /// layout's top.
    fn layout(&self, fonts: &Fonts, scale: f32) -> windows::core::Result<Laid> {
        let format = fonts.text_format(DWRITE_FONT_WEIGHT_SEMI_BOLD, TEXT * scale)?;
        let text: Vec<u16> = self.text.encode_utf16().collect();
        // Half a pixel more than the room, drawn at any scale but 100%: a line that fits at 100% fits at every scale,
        // though DirectWrite's widths grow with the scale only to a float's precision. The lines counted for the row's
        // height are then never fewer than the lines drawn.
        let room = self.width * scale + if scale > 1.0 { 0.5 } else { 0.0 };
        // SAFETY: the layout copies the text and keeps its own reference to the format.
        let layout = unsafe { fonts.factory.CreateTextLayout(&text, &format, room, 4096.0) }?;
        // SAFETY: plain value.
        unsafe { layout.SetWordWrapping(DWRITE_WORD_WRAPPING_WRAP) }?;
        let mut metrics = DWRITE_TEXT_METRICS::default();
        // SAFETY: fills a local.
        unsafe { layout.GetMetrics(&mut metrics) }?;
        let count = usize::try_from(metrics.lineCount)
            .map_err(|_| Error::new(E_FAIL, "a message of more lines than memory"))?;
        let mut lines = vec![DWRITE_LINE_METRICS::default(); count];
        let mut filled = 0;
        // SAFETY: fills a buffer of as many lines as the layout has, and a local.
        unsafe { layout.GetLineMetrics(Some(&mut lines), &mut filled) }?;
        let first = lines
            .first()
            .copied()
            .ok_or_else(|| Error::new(E_FAIL, "the message laid out on no line"))?;
        let line = LINE * scale;
        let baseline = (line - first.height) / 2.0 + first.baseline;
        // SAFETY: plain values.
        unsafe { layout.SetLineSpacing(DWRITE_LINE_SPACING_METHOD_UNIFORM, line, baseline) }?;
        Ok(Laid {
            layout,
            lines: metrics.lineCount,
            baseline,
        })
    }

    /// How many lines it takes, as laid out at 100%.
    pub fn lines(&self, fonts: &Fonts) -> windows::core::Result<u32> {
        Ok(self.layout(fonts, 1.0)?.lines)
    }

    /// How much its row grows to hold its lines under the label (and the note, should the row have one).
    pub fn growth(&self, fonts: &Fonts) -> windows::core::Result<Grown> {
        let lines = self.lines(fonts)?;
        let mut needed = 2.0 * ROW_PADDING + layout::LABEL_LINE + Message::height(lines, 1.0);
        if self.row.note().is_some() {
            needed += layout::NOTE_GAP + layout::NOTE_LINE;
        }
        let span = layout::place_row(self.row, None).span;
        // A row's height is a few hundred pixels, never below nothing.
        let needed = u32::try_from(super::controls::whole_up(needed)).unwrap_or(0);
        Ok(Grown {
            row: self.row,
            by: needed.saturating_sub(span.bottom - span.top),
        })
    }

    /// The height `lines` of a message take under the label at `scale`, the gap above them included.
    pub fn height(lines: u32, scale: f32) -> f32 {
        // A message has a few lines, which a u16 holds, and a float exactly.
        let lines = f32::from(u16::try_from(lines).unwrap_or(u16::MAX));
        (GAP + LINE * lines) * scale
    }

    /// Writes it with its left side at `left` and its top at `top`, in the target's pixels at `scale`: right below the
    /// label (or the note), the gap above it included.
    pub fn draw(
        &self,
        target: &ID2D1RenderTarget,
        fonts: &Fonts,
        mode: Mode,
        left: f32,
        top: f32,
        scale: f32,
    ) -> windows::core::Result<()> {
        let laid = self.layout(fonts, scale)?;
        let ink = solid_brush(target, warn(mode))?;
        // The first line's baseline on a whole pixel, as the labels' are, so the letters stay crisp.
        let top = top + GAP * scale;
        let top = (top + laid.baseline).round() - laid.baseline;
        super::paint::write(target, &laid.layout, left, top, &ink);
        Ok(())
    }
}

/// A message laid out.
struct Laid {
    layout: IDWriteTextLayout,
    lines: u32,
    /// The first line's baseline, from the layout's top.
    baseline: f32,
}

/// The amber band across the row `span` of a card `card` at `dpi`, on a page `width` pixels wide: `--warn` faint over
/// the whole row inside the card's border, with corners where the card's are, and a bar of `--warn` on its left. The
/// line above the row, should there be one, is drawn over it after.
pub fn band(
    target: &ID2D1RenderTarget,
    factory: &ID2D1Factory,
    mode: Mode,
    card: Span,
    span: Span,
    dpi: u32,
    width: i32,
) -> windows::core::Result<()> {
    let border = scaled(1, dpi).max(1);
    let side = to_i32(scaled(layout::SIDE, dpi))?;
    let inner_left = side + to_i32(border)?;
    let inner = D2D1_ROUNDED_RECT {
        rect: D2D_RECT_F {
            left: px(inner_left),
            top: px(to_i32(scaled(card.top, dpi) + border)?),
            right: px(width - inner_left),
            bottom: px(to_i32(scaled(card.bottom, dpi) - border)?),
        },
        radiusX: px(to_i32(scaled(layout::CARD_RADIUS, dpi) - border)?),
        radiusY: px(to_i32(scaled(layout::CARD_RADIUS, dpi) - border)?),
    };
    let row = RECT {
        left: inner_left,
        top: to_i32(scaled(span.top, dpi))?,
        right: width - inner_left,
        bottom: to_i32(scaled(span.bottom, dpi))?,
    };
    let warn = warn(mode);
    let tint = solid_brush(
        target,
        Color {
            alpha: BAND,
            ..warn
        },
    )?;
    let bar = solid_brush(target, warn)?;
    // The bar is the row's shape less the same shape moved right by its width: it follows a rounded corner.
    let moved = D2D1_ROUNDED_RECT {
        rect: D2D_RECT_F {
            left: inner.rect.left + px(relief::scaled(BAR, dpi)),
            right: inner.rect.right + px(relief::scaled(BAR, dpi)),
            ..inner.rect
        },
        ..inner
    };
    let hole = relief::geometry_of(factory, &moved)?;
    let clip = D2D_RECT_F {
        left: px(row.left),
        top: px(row.top),
        right: px(row.right),
        bottom: px(row.bottom),
    };
    // SAFETY: drawing calls on a live render target with arguments that outlive them; the clip is popped below.
    unsafe {
        target.PushAxisAlignedClip(&clip, D2D1_ANTIALIAS_MODE_ALIASED);
        target.FillRoundedRectangle(&inner, &tint);
    }
    let barred = relief::fill_except(factory, target, &inner, &hole, &bar);
    // SAFETY: pops the clip pushed above.
    unsafe { target.PopAxisAlignedClip() };
    barred
}
