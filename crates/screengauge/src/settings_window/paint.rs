//! Settings drawn onto any Direct2D render target at 96 DPI, so a test can render it on a bitmap and count pixels: the
//! window's background, each block's heading and card, the rows with their labels, notes, messages (`row_message`) and
//! controls (`controls`, `recorder_field`), the Snip's card (`snip`), and the footer (`footer`). The colors are
//! `look`'s, which the tray icon's menu shares; the sizes come from `layout`, scaled for the monitor's `dpi`.

use screengauge_core::session::Point;
use screengauge_core::session::toolbar::scaled;
use screengauge_core::settings::Options;
use windows::Win32::Foundation::{E_FAIL, RECT};
use windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F;
use windows::Win32::Graphics::Direct2D::{
    D2D1_ANTIALIAS_MODE_ALIASED, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
    D2D1_BITMAP_INTERPOLATION_MODE_NEAREST_NEIGHBOR, D2D1_ROUNDED_RECT, ID2D1Factory,
    ID2D1RenderTarget, ID2D1SolidColorBrush, ID2D1StrokeStyle,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FONT_STYLE_ITALIC, DWRITE_FONT_WEIGHT, DWRITE_FONT_WEIGHT_BOLD,
    DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_TEXT_METRICS,
    DWRITE_TEXT_RANGE, IDWriteTextFormat, IDWriteTextLayout, IDWriteTextLayout1,
};
use windows::core::{Error, Interface};

use super::controls::{self, Control, Label, LabelKind, Laid, Pointer, RowEnd};
use super::footer::{self, Link, Status};
use super::layout::{self, Grown, Page, Row};
use super::recorder_field::{self, View};
use super::row_message::{self, Message};
use super::snip_card;
use crate::graphics::{Fonts, d2d_color, solid_brush};
use crate::log;
use crate::look::{Palette, background, dpi_scale, palette, px, write};
use crate::relief::{self, Canvas};
use crate::theme::Mode;
use crate::typeset::Typeset;

mod general;
mod inspect;
mod measurements;
mod snip;

/// The headings' letter spacing (`letter-spacing: .09em`), as a fraction of their size.
const HEADING_SPACING: f32 = 0.09;

/// A text laid out on one line, and where it goes.
pub(super) struct Placed {
    layout: IDWriteTextLayout,
    pub left: f32,
    /// The top of its layout box, set so the baseline falls on a whole pixel.
    pub top: f32,
    pub width: f32,
    pub baseline: f32,
}

/// What a click can act on, as drawn: the footer's links and each part of a control, in the target's pixels.
#[derive(Debug, Clone, PartialEq)]
pub struct Drawn {
    pub links: Vec<(Link, RECT)>,
    pub controls: Vec<(Control, RECT)>,
}

/// What the page is drawn with: the target and the factory it was made with, the fonts, the palette, the window's
/// scale, and the client area's width in pixels.
pub struct Surface<'a> {
    pub target: &'a ID2D1RenderTarget,
    pub factory: &'a ID2D1Factory,
    pub fonts: &'a Fonts,
    /// The round stroke the footer's signs are drawn with, made once for the window.
    pub stroke: &'a ID2D1StrokeStyle,
    pub mode: Mode,
    pub dpi: u32,
    pub width: u32,
}

/// What Settings shows apart from the options: the global shortcut recorder's field, whether "Start with Windows" is on
/// as Windows says, the messages under rows' labels with how much each grows its row, while they show, and what "Check
/// for updates"' line says, from its click on.
#[derive(Debug, Clone, PartialEq)]
pub struct Shown {
    pub recorder: View,
    /// Whether "Start with Windows"' switch shows on (`StartWithWindows::is_on`).
    pub start_with_windows: bool,
    pub messages: Vec<Message>,
    pub grown: Vec<Grown>,
    pub update: Option<Status>,
}

/// Draws Settings on `on`, with `options` as they are now, the controls reacting to `pointer`, and what `shown` says,
/// and returns where a click acts.
pub fn draw(
    on: &Surface,
    options: &Options,
    pointer: Pointer,
    shown: &Shown,
) -> windows::core::Result<Drawn> {
    let palette = palette(on.mode);
    // The page grows while a message shows under a row's label, and the blocks below move down with it. The Snip's
    // card is shorter without its note, and the window as tall as the page with it.
    let page = layout::with_status(
        snip_card::fitted(layout::page(&shown.grown), snip_card::note_shown(options)),
        shown.update.is_some(),
    );
    let height = px(scaled(page.height, on.dpi));
    let width = px(on.width);
    background(on.target, palette, width, height)?;
    cards(on.target, on.fonts, palette, &page, on.dpi, width)?;
    let controls = Rows::new(on, palette)?.draw(options, pointer, shown, &page)?;
    let links = footer::draw(on, palette, page.footer, shown.update)?;
    Ok(Drawn { links, controls })
}

/// What the rows are drawn with.
struct Rows<'a> {
    on: &'a Surface<'a>,
    /// A canvas on the window's own target, for the − and + signs that shrink with their button.
    canvas: Canvas<'a>,
    edge: ID2D1SolidColorBrush,
    ink: ID2D1SolidColorBrush,
    dim: ID2D1SolidColorBrush,
    label: IDWriteTextFormat,
    note: IDWriteTextFormat,
    segment: IDWriteTextFormat,
    scale: f32,
}

impl<'a> Rows<'a> {
    fn new(on: &'a Surface<'a>, palette: &Palette) -> windows::core::Result<Rows<'a>> {
        let scale = dpi_scale(on.dpi);
        Ok(Rows {
            on,
            canvas: Canvas::new(on.target, on.factory, on.dpi, Point { x: 0, y: 0 })?,
            edge: solid_brush(on.target, palette.edge)?,
            ink: solid_brush(on.target, palette.ink)?,
            dim: solid_brush(on.target, palette.dim)?,
            label: on
                .fonts
                .text_format(DWRITE_FONT_WEIGHT_NORMAL, layout::LABEL_TEXT * scale)?,
            note: on
                .fonts
                .text_format(DWRITE_FONT_WEIGHT_NORMAL, layout::NOTE_TEXT * scale)?,
            segment: on
                .fonts
                .text_format(DWRITE_FONT_WEIGHT_SEMI_BOLD, controls::SEGMENT_TEXT * scale)?,
            scale,
        })
    }

    /// The choice at the row's `end`: a segment for each of `options` with its word, the one equal to `chosen` lit,
    /// and `control` naming what a click on each acts on.
    fn choice<T: Copy + PartialEq>(
        &self,
        options: &[(T, &'static str)],
        chosen: T,
        control: fn(T) -> Control,
        end: RowEnd,
    ) -> windows::core::Result<Laid> {
        let mut segments = Vec::with_capacity(options.len());
        for &(option, word) in options {
            let width = width_of(&self.segment_text(word)?)?;
            segments.push((control(option), word, width, option == chosen));
        }
        Ok(controls::choice(&self.canvas, &segments, end))
    }

    /// `word` laid out in the font of a choice's segment.
    fn segment_text(&self, word: &str) -> windows::core::Result<IDWriteTextLayout> {
        text(self.on.fonts, word, &self.segment, None)
    }

    /// Each row's band, line above it, label, note, message and control, and where its control's parts are, on `page`.
    fn draw(
        &self,
        options: &Options,
        pointer: Pointer,
        shown: &Shown,
        page: &Page,
    ) -> windows::core::Result<Vec<(Control, RECT)>> {
        let dpi = self.on.dpi;
        let width = to_i32(self.on.width)?;
        let inner = to_i32(scaled(layout::ROW_INSET, dpi))?;
        let right = width - inner;
        let measure = measurer(self.on.fonts, self.scale);
        let mut laid = Vec::new();
        for row in layout::ROWS {
            let placed = layout::place_row(row, &shown.grown);
            let top = to_i32(scaled(placed.span.top, dpi))?;
            let bottom = to_i32(scaled(placed.span.bottom, dpi))?;
            let message = shown
                .messages
                .iter()
                .find(|message| message.row == row)
                .copied();
            if let Some(message) = message
                && message.band
            {
                let card = page
                    .blocks
                    .iter()
                    .find(|block| {
                        block.card.top <= placed.span.top && placed.span.bottom <= block.card.bottom
                    })
                    .map(|block| block.card)
                    .ok_or_else(|| Error::new(E_FAIL, "a row outside every card"))?;
                row_message::band(
                    self.on.target,
                    self.on.factory,
                    self.on.mode,
                    card,
                    placed.span,
                    dpi,
                    width,
                )?;
            }
            if placed.line_above {
                self.separator(width, top)?;
            }
            self.texts(row, message.as_ref(), px_i(inner), top, bottom)?;
            // Each block's controls are laid out in the block's own file.
            let place = RowEnd { right, top, bottom };
            let control = match row {
                Row::GlobalShortcut => self.shortcut(shown, &measure, place, &mut laid)?,
                Row::StartWithWindows => self.start_with_windows(shown, place),
                Row::Theme => self.theme(options, place)?,
                Row::DefaultTolerance => self.default_tolerance(options, place),
                Row::AlsoShowLogicalPixels => self.logical_pixels(options, place),
                Row::ColorFormat => self.color_format(options, place)?,
            };
            laid.push(control);
        }
        let snip::Snip {
            laid: snip_laid,
            path,
        } = self.snip(options, page)?;
        laid.extend(snip_laid);
        self.surfaces(&laid, pointer, page)?;
        for control in &laid {
            for label in controls::labels(control, self.on.mode, pointer) {
                self.label(&label)?;
            }
        }
        self.write_path(&path);
        Ok(laid.iter().flat_map(Laid::hits).collect())
    }

    /// The 1-pixel line between two rows of a card, across the card inside its border, just above `top`.
    fn separator(&self, width: i32, top: i32) -> windows::core::Result<()> {
        let border = to_i32(scaled(1, self.on.dpi).max(1))?;
        let inset = to_i32(scaled(layout::SIDE, self.on.dpi))? + border;
        let line = D2D_RECT_F {
            left: px_i(inset),
            top: px_i(top - border),
            right: px_i(width - inset),
            bottom: px_i(top),
        };
        let target = self.on.target;
        // SAFETY: drawing calls on a live render target, with arguments that outlive them. The line is on whole pixels.
        unsafe {
            target.SetAntialiasMode(D2D1_ANTIALIAS_MODE_ALIASED);
            target.FillRectangle(&line, &self.edge);
            target.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
        }
        Ok(())
    }

    /// `row`'s label, its note under it and the message under them, at `left`, all together centered in the row from
    /// `top` to `bottom`.
    fn texts(
        &self,
        row: Row,
        message: Option<&Message>,
        left: f32,
        top: i32,
        bottom: i32,
    ) -> windows::core::Result<()> {
        let fonts = self.on.fonts;
        let label_line = layout::LABEL_LINE * self.scale;
        let note_line = layout::NOTE_LINE * self.scale;
        let gap = layout::NOTE_GAP * self.scale;
        let above_message = match row.note() {
            Some(_) => label_line + gap + note_line,
            None => label_line,
        };
        let message_height = match message {
            Some(message) => Message::height(message.lines(fonts)?, self.scale),
            None => 0.0,
        };
        let height = above_message + message_height;
        let line = px_i(top) + (px_i(bottom - top) - height) / 2.0;
        let label = place(
            text(fonts, row.label(), &self.label, None)?,
            left,
            line,
            label_line,
        )?;
        draw_text(self.on.target, &label, &self.ink);
        if let Some(note) = row.note() {
            let note = place(
                text(fonts, note, &self.note, None)?,
                left,
                line + label_line + gap,
                note_line,
            )?;
            draw_text(self.on.target, &note, &self.dim);
        }
        if let Some(message) = message {
            message.draw(
                self.on.target,
                fonts,
                self.on.mode,
                left,
                line + above_message,
                self.scale,
            )?;
        }
        Ok(())
    }

    /// The controls' surfaces, painted with their shadows on a transparent layer the size of the page, which is then
    /// laid over it: faint shadow layers painted straight over the page's opaque pixels would round away (`relief`).
    fn surfaces(&self, laid: &[Laid], pointer: Pointer, page: &Page) -> windows::core::Result<()> {
        let target = self.on.target;
        let width = to_i32(self.on.width)?;
        let height = to_i32(scaled(page.height, self.on.dpi))?;
        let layer = relief::transparent_layer(target, width, height)?;
        let canvas = Canvas::new(&layer, self.on.factory, self.on.dpi, Point { x: 0, y: 0 })?;
        // SAFETY: the layer is ours, and ended below whatever happens in between. Premultiplied transparent: nothing
        // under the shadows.
        unsafe {
            layer.BeginDraw();
            layer.Clear(Some(&d2d_color([0, 0, 0], 0.0)));
            layer.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
        }
        let painted = laid
            .iter()
            .try_for_each(|control| controls::paint(&canvas, control, self.on.mode, pointer));
        // SAFETY: ends the BeginDraw above.
        let ended = unsafe { layer.EndDraw(None, None) };
        match (painted, ended) {
            (Ok(()), Ok(())) => {}
            (Err(e), Ok(())) | (Ok(()), Err(e)) => return Err(e),
            // Both failed: the painting's is the cause, and the other is logged, not lost.
            (Err(e), Err(also)) => {
                log::failure("Settings: ending the controls' layer", &also);
                return Err(e);
            }
        }
        // SAFETY: no arguments; the bitmap comes back as an owned interface.
        let bitmap = unsafe { layer.GetBitmap() }?;
        // SAFETY: a drawing call on a live render target, with arguments that outlive it. One pixel of the layer is one
        // of the page.
        unsafe {
            target.DrawBitmap(
                &bitmap,
                None,
                1.0,
                D2D1_BITMAP_INTERPOLATION_MODE_NEAREST_NEIGHBOR,
                None,
            );
        }
        Ok(())
    }

    /// A control's word or sign, centered in its box, shrunk with a held button.
    fn label(&self, label: &Label) -> windows::core::Result<()> {
        let layout = word(self.on.fonts, &label.text, label.kind, self.scale)?;
        let room = px_i(label.place.right - label.place.left);
        let left = px_i(label.place.left) + ((room - width_of(&layout)?) / 2.0).round();
        let placed = place(
            layout,
            left,
            px_i(label.place.top),
            px_i(label.place.bottom - label.place.top),
        )?;
        let brush = solid_brush(self.on.target, label.ink)?;
        let write = || {
            draw_text(self.on.target, &placed, &brush);
            Ok(())
        };
        if label.shrinks {
            self.canvas
                .in_motion(&label.place, controls::pressed(), write)
        } else {
            write()
        }
    }
}

/// The font of a control's word: its weight, its size in logical pixels, and whether it is italic.
fn font_of(kind: LabelKind) -> (DWRITE_FONT_WEIGHT, f32, bool) {
    match kind {
        LabelKind::Segment => (DWRITE_FONT_WEIGHT_SEMI_BOLD, controls::SEGMENT_TEXT, false),
        LabelKind::Number => (DWRITE_FONT_WEIGHT_BOLD, controls::NUMBER_TEXT, false),
        LabelKind::Sign => (DWRITE_FONT_WEIGHT_BOLD, controls::SIGN_TEXT, false),
        LabelKind::Keycap => (
            DWRITE_FONT_WEIGHT_SEMI_BOLD,
            recorder_field::KEYCAP_TEXT,
            false,
        ),
        LabelKind::Plus => (DWRITE_FONT_WEIGHT_NORMAL, recorder_field::PLUS_TEXT, false),
        LabelKind::Waiting => (
            DWRITE_FONT_WEIGHT_NORMAL,
            recorder_field::WAITING_TEXT,
            true,
        ),
    }
}

/// A control's `words` laid out on one line in the font of `kind`, at `scale`.
fn word(
    fonts: &Fonts,
    words: &str,
    kind: LabelKind,
    scale: f32,
) -> windows::core::Result<IDWriteTextLayout> {
    let (weight, size, italic) = font_of(kind);
    let format = fonts.text_format(weight, size * scale)?;
    let layout = text(fonts, words, &format, None)?;
    if italic {
        let length = u32::try_from(words.encode_utf16().count())
            .map_err(|_| Error::new(E_FAIL, "the text is too long"))?;
        // SAFETY: the range is the whole text.
        unsafe {
            layout.SetFontStyle(
                DWRITE_FONT_STYLE_ITALIC,
                DWRITE_TEXT_RANGE {
                    startPosition: 0,
                    length,
                },
            )
        }?;
    }
    Ok(layout)
}

/// How wide a control's word is, in pixels at `scale`, in the font of its kind: what the global shortcut's field is
/// laid out with.
pub fn measurer(
    fonts: &Fonts,
    scale: f32,
) -> impl Fn(&str, LabelKind) -> windows::core::Result<f32> + '_ {
    move |words, kind| width_of(&word(fonts, words, kind, scale)?)
}

/// A position in pixels as the float Direct2D takes.
fn px_i(pixels: i32) -> f32 {
    relief::px(pixels)
}

/// A size in pixels as a coordinate. Sizes in the window are a few thousand pixels at most.
pub fn to_i32(pixels: u32) -> windows::core::Result<i32> {
    i32::try_from(pixels).map_err(|_| Error::new(E_FAIL, "the page is too large to draw"))
}

/// Each block's card with its 1-pixel border, and its heading above it.
fn cards(
    target: &ID2D1RenderTarget,
    fonts: &Fonts,
    palette: &Palette,
    page: &Page,
    dpi: u32,
    width: f32,
) -> windows::core::Result<()> {
    let card = solid_brush(target, palette.card)?;
    let edge = solid_brush(target, palette.edge)?;
    let dim = solid_brush(target, palette.dim)?;
    let side = px(scaled(layout::SIDE, dpi));
    let radius = px(scaled(layout::CARD_RADIUS, dpi));
    let border = px(scaled(1, dpi).max(1));
    let size = layout::HEADING_LINE * dpi_scale(dpi);
    let format = fonts.text_format(DWRITE_FONT_WEIGHT_BOLD, size)?;
    for placed in &page.blocks {
        let outer = D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F {
                left: side,
                top: px(scaled(placed.card.top, dpi)),
                right: width - side,
                bottom: px(scaled(placed.card.bottom, dpi)),
            },
            radiusX: radius,
            radiusY: radius,
        };
        // The border runs inside the card's edge, as CSS draws it with `box-sizing: border-box`.
        let half = border / 2.0;
        let stroke = D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F {
                left: outer.rect.left + half,
                top: outer.rect.top + half,
                right: outer.rect.right - half,
                bottom: outer.rect.bottom - half,
            },
            radiusX: radius - half,
            radiusY: radius - half,
        };
        // SAFETY: drawing calls on a live render target, with arguments that outlive them.
        unsafe {
            target.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
            target.FillRoundedRectangle(&outer, &card);
            target.DrawRoundedRectangle(&stroke, &edge, border, None);
        }
        let heading = place(
            text(fonts, placed.block.heading(), &format, Some(size))?,
            side + px(scaled(layout::HEADING_INDENT, dpi)),
            px(scaled(placed.heading, dpi)),
            size,
        )?;
        draw_text(target, &heading, &dim);
    }
    Ok(())
}

/// `words` laid out on one line in `format`, spaced as a heading when `spacing` gives its font size.
pub(super) fn text(
    fonts: &Fonts,
    words: &str,
    format: &IDWriteTextFormat,
    spacing: Option<f32>,
) -> windows::core::Result<IDWriteTextLayout> {
    let text: Vec<u16> = words.encode_utf16().collect();
    let length =
        u32::try_from(text.len()).map_err(|_| Error::new(E_FAIL, "the text is too long"))?;
    // SAFETY: the layout copies the text and keeps its own reference to the format. A width and height past any text
    // here keep it on one line.
    let layout = unsafe {
        fonts
            .factory
            .CreateTextLayout(&text, format, 4096.0, 4096.0)
    }?;
    if let Some(size) = spacing {
        // Letter spacing came with the layout's second version, in every Windows ScreenGauge runs on.
        let spaced: IDWriteTextLayout1 = layout.cast()?;
        // SAFETY: the range is the whole text. CSS adds the spacing after each letter.
        unsafe {
            spaced.SetCharacterSpacing(
                0.0,
                HEADING_SPACING * size,
                0.0,
                DWRITE_TEXT_RANGE {
                    startPosition: 0,
                    length,
                },
            )
        }?;
    }
    Ok(layout)
}

pub(super) fn width_of(layout: &IDWriteTextLayout) -> windows::core::Result<f32> {
    let mut metrics = DWRITE_TEXT_METRICS::default();
    // SAFETY: fills a local.
    unsafe { layout.GetMetrics(&mut metrics) }?;
    Ok(metrics.widthIncludingTrailingWhitespace)
}

/// `layout` with its left side at `left`, centered in a line of `line_height` from `line` down, as CSS centers a text
/// in its line box, with the baseline moved to the nearest whole pixel so the letters stay crisp.
pub(super) fn place(
    layout: IDWriteTextLayout,
    left: f32,
    line: f32,
    line_height: f32,
) -> windows::core::Result<Placed> {
    // The width a following piece is placed after, trailing spaces included and not rounded: the typeset's whole width
    // would move it.
    let width = width_of(&layout)?;
    let typeset = Typeset::of(layout)?;
    let baseline = typeset.baseline;
    let placed = typeset.in_line(left, line, line_height);
    Ok(Placed {
        layout: placed.layout,
        left: placed.left,
        top: placed.top,
        width,
        baseline,
    })
}

pub(super) fn draw_text(target: &ID2D1RenderTarget, placed: &Placed, brush: &ID2D1SolidColorBrush) {
    write(target, &placed.layout, placed.left, placed.top, brush);
}

#[cfg(test)]
mod tests;
