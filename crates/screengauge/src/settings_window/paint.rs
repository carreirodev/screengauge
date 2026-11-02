//! Settings drawn onto any Direct2D render target at 96 DPI, so a test can render it on a bitmap and count pixels: the
//! window's background, each block's heading and card, the rows of Measurements and Inspect with their labels, notes and
//! controls (`controls`), the Snip's card (`snip`), and the footer with the version, the license and the "Project page"
//! link. The colors are the prototype's theme tokens for `#settings` (WPC-279), where the `oklch` ones are converted to
//! sRGB; the sizes come from `layout`, scaled for the monitor's `dpi`.

use screengauge_core::session::Point;
use screengauge_core::session::toolbar::scaled;
use screengauge_core::settings::Options;
use windows::Win32::Foundation::{E_FAIL, RECT};
use windows::Win32::Graphics::Direct2D::Common::{D2D_RECT_F, D2D1_GRADIENT_STOP};
use windows::Win32::Graphics::Direct2D::{
    D2D1_ANTIALIAS_MODE_ALIASED, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
    D2D1_BITMAP_INTERPOLATION_MODE_NEAREST_NEIGHBOR, D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_ELLIPSE,
    D2D1_EXTEND_MODE_CLAMP, D2D1_GAMMA_2_2, D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES,
    D2D1_ROUNDED_RECT, D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE, ID2D1Factory, ID2D1RenderTarget,
    ID2D1SolidColorBrush,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FONT_WEIGHT, DWRITE_FONT_WEIGHT_BOLD, DWRITE_FONT_WEIGHT_NORMAL,
    DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_LINE_METRICS, DWRITE_TEXT_METRICS, DWRITE_TEXT_RANGE,
    IDWriteTextFormat, IDWriteTextLayout, IDWriteTextLayout1,
};
use windows::core::{Error, Interface};

use super::controls::{
    self, COLOR_FORMATS, Control, Label, LabelKind, Laid, Pointer, THEMES, dpi_scale,
};
use super::layout::{self, Page, Row, Span};
use super::snip_card;
use crate::graphics::{Color, Fonts, color, d2d_color, solid_brush};
use crate::log;
use crate::relief::{self, Canvas};
use crate::theme::Mode;

mod snip;

/// The footer's left text: the version this exe was built as, and the license.
pub const VERSION: &str = concat!("Version ", env!("CARGO_PKG_VERSION"));
pub const LICENSE: &str = "Open source (MIT)";
pub const LINK: &str = "Project page";

/// The headings' letter spacing (`letter-spacing: .09em`), as a fraction of their size.
const HEADING_SPACING: f32 = 0.09;

/// How far below the baseline the link's underline starts (`text-underline-offset: 2px`).
const UNDERLINE_OFFSET: u32 = 2;

/// One mode's colors.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    /// `--win-bg`: the window's background, a gradient from the top to the bottom.
    pub top: Color,
    pub bottom: Color,
    /// `--card` and `--card-edge`: the cards' background and border, and the footer's line.
    pub card: Color,
    pub edge: Color,
    /// `--panel-ink`: the rows' labels.
    pub ink: Color,
    /// `--panel-dim`: the headings, the rows' notes and the footer's texts.
    pub dim: Color,
    /// `--link`.
    pub link: Color,
}

pub const DARK: Palette = Palette {
    top: color(0x25, 0x27, 0x2c, 1.0),
    bottom: color(0x1b, 0x1c, 0x20, 1.0),
    card: color(255, 255, 255, 0.035),
    edge: color(255, 255, 255, 0.07),
    ink: controls::DARK.ink,
    dim: color(0x9a, 0xa0, 0xaa, 1.0),
    // oklch(0.82 0.12 200)
    link: color(0x4c, 0xdb, 0xe3, 1.0),
};

pub const LIGHT: Palette = Palette {
    top: color(0xf8, 0xf7, 0xf4, 1.0),
    bottom: color(0xee, 0xec, 0xe8, 1.0),
    card: color(255, 255, 255, 0.75),
    edge: color(70, 60, 45, 0.13),
    ink: controls::LIGHT.ink,
    dim: color(0x6b, 0x68, 0x62, 1.0),
    // oklch(0.5 0.15 240)
    link: color(0x00, 0x6a, 0xaf, 1.0),
};

pub fn palette(mode: Mode) -> &'static Palette {
    match mode {
        Mode::Light => &LIGHT,
        Mode::Dark => &DARK,
    }
}

/// A text laid out on one line, and where it goes.
struct Placed {
    layout: IDWriteTextLayout,
    left: f32,
    /// The top of its layout box, set so the baseline falls on a whole pixel.
    top: f32,
    width: f32,
    baseline: f32,
}

/// What a click can act on, as drawn: the "Project page" link and each part of a control, in the target's pixels.
#[derive(Debug, Clone, PartialEq)]
pub struct Drawn {
    pub link: RECT,
    pub controls: Vec<(Control, RECT)>,
}

/// What the page is drawn with: the target and the factory it was made with, the fonts, the palette, the window's
/// scale, and the client area's width in pixels.
pub struct Surface<'a> {
    pub target: &'a ID2D1RenderTarget,
    pub factory: &'a ID2D1Factory,
    pub fonts: &'a Fonts,
    pub mode: Mode,
    pub dpi: u32,
    pub width: u32,
}

/// Draws Settings on `on`, with `options` as they are now and the controls reacting to `pointer`, and returns where a
/// click acts.
pub fn draw(on: &Surface, options: &Options, pointer: Pointer) -> windows::core::Result<Drawn> {
    let palette = palette(on.mode);
    // The Snip's card is shorter without its note, and the window as tall as the page with it.
    let page = snip_card::fitted(layout::page(), snip_card::note_shown(options));
    let height = px(scaled(page.height, on.dpi));
    let width = px(on.width);
    background(on.target, palette, width, height)?;
    cards(on.target, on.fonts, palette, &page, on.dpi, width)?;
    let controls = Rows::new(on, palette)?.draw(options, pointer)?;
    let link = footer(on.target, on.fonts, palette, page.footer, on.dpi, width)?;
    Ok(Drawn { link, controls })
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

    /// Each row's line above it, label, note and control, and where its control's parts are.
    fn draw(
        &self,
        options: &Options,
        pointer: Pointer,
    ) -> windows::core::Result<Vec<(Control, RECT)>> {
        let dpi = self.on.dpi;
        let width = to_i32(self.on.width)?;
        let inner = to_i32(scaled(
            layout::SIDE + layout::CARD_BORDER + layout::ROW_PADDING,
            dpi,
        ))?;
        let right = width - inner;
        let mut laid = Vec::new();
        for row in layout::ROWS {
            let placed = layout::place_row(row);
            let top = to_i32(scaled(placed.span.top, dpi))?;
            let bottom = to_i32(scaled(placed.span.bottom, dpi))?;
            if placed.line_above {
                self.separator(width, top)?;
            }
            self.texts(row, px_i(inner), top, bottom)?;
            let canvas = &self.canvas;
            laid.push(match row {
                Row::Theme => {
                    let mut choices = Vec::new();
                    for (theme, word) in THEMES {
                        let width = width_of(&text(self.on.fonts, word, &self.segment, None)?)?;
                        choices.push((Control::Theme(theme), word, width, options.theme == theme));
                    }
                    controls::choice(canvas, &choices, right, top, bottom)
                }
                Row::DefaultTolerance => {
                    controls::stepper(canvas, options.default_tolerance.get(), right, top, bottom)
                }
                Row::AlsoShowLogicalPixels => controls::switch(
                    canvas,
                    Control::LogicalPixels,
                    options.also_show_logical_pixels,
                    right,
                    top,
                    bottom,
                ),
                Row::ColorFormat => {
                    let mut choices = Vec::new();
                    for (format, word) in COLOR_FORMATS {
                        let width = width_of(&text(self.on.fonts, word, &self.segment, None)?)?;
                        let chosen = options.color_format == format;
                        choices.push((Control::ColorFormat(format), word, width, chosen));
                    }
                    controls::choice(canvas, &choices, right, top, bottom)
                }
            });
        }
        let snip::Snip {
            laid: snip_laid,
            path,
        } = self.snip(options)?;
        laid.extend(snip_laid);
        self.surfaces(&laid, pointer)?;
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

    /// `row`'s label, and its note under it, at `left`, the two together centered in the row from `top` to `bottom`.
    fn texts(&self, row: Row, left: f32, top: i32, bottom: i32) -> windows::core::Result<()> {
        let fonts = self.on.fonts;
        let label_line = layout::LABEL_LINE * self.scale;
        let note_line = layout::NOTE_LINE * self.scale;
        let gap = layout::NOTE_GAP * self.scale;
        let height = match row.note() {
            Some(_) => label_line + gap + note_line,
            None => label_line,
        };
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
        Ok(())
    }

    /// The controls' surfaces, painted with their shadows on a transparent layer the size of the page, which is then
    /// laid over it: faint shadow layers painted straight over the page's opaque pixels would round away (`relief`).
    fn surfaces(&self, laid: &[Laid], pointer: Pointer) -> windows::core::Result<()> {
        let target = self.on.target;
        let width = to_i32(self.on.width)?;
        let height = to_i32(scaled(layout::page().height, self.on.dpi))?;
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
        let (weight, size): (DWRITE_FONT_WEIGHT, f32) = match label.kind {
            LabelKind::Segment => (DWRITE_FONT_WEIGHT_SEMI_BOLD, controls::SEGMENT_TEXT),
            LabelKind::Number => (DWRITE_FONT_WEIGHT_BOLD, controls::NUMBER_TEXT),
            LabelKind::Sign => (DWRITE_FONT_WEIGHT_BOLD, controls::SIGN_TEXT),
        };
        let format = self.on.fonts.text_format(weight, size * self.scale)?;
        let layout = text(self.on.fonts, &label.text, &format, None)?;
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

/// A position in pixels as the float Direct2D takes.
fn px_i(pixels: i32) -> f32 {
    relief::px(pixels)
}

/// A size in pixels as a coordinate. Sizes in the window are a few thousand pixels at most.
fn to_i32(pixels: u32) -> windows::core::Result<i32> {
    i32::try_from(pixels).map_err(|_| Error::new(E_FAIL, "the page is too large to draw"))
}

/// The window's gradient, from its top to its bottom. Colors blend in sRGB, as CSS does.
fn background(
    target: &ID2D1RenderTarget,
    palette: &Palette,
    width: f32,
    height: f32,
) -> windows::core::Result<()> {
    let stops = [
        D2D1_GRADIENT_STOP {
            position: 0.0,
            color: d2d_color(palette.top.rgb, palette.top.alpha),
        },
        D2D1_GRADIENT_STOP {
            position: 1.0,
            color: d2d_color(palette.bottom.rgb, palette.bottom.alpha),
        },
    ];
    // SAFETY: the stops outlive the call, which copies them.
    let collection = unsafe {
        target.CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)
    }?;
    let mut line = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES::default();
    line.endPoint.Y = height;
    // SAFETY: the line and the collection outlive the call, which keeps its own reference to the collection.
    let brush = unsafe { target.CreateLinearGradientBrush(&line, None, &collection) }?;
    let all = D2D_RECT_F {
        left: 0.0,
        top: 0.0,
        right: width,
        bottom: height,
    };
    // SAFETY: a drawing call on a live render target, with arguments that outlive it.
    unsafe { target.FillRectangle(&all, &brush) };
    Ok(())
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

/// The footer's line, the version and the license on the left, and the link on the right, which comes back.
fn footer(
    target: &ID2D1RenderTarget,
    fonts: &Fonts,
    palette: &Palette,
    span: Span,
    dpi: u32,
    width: f32,
) -> windows::core::Result<RECT> {
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
    let size = px(scaled(layout::FOOTER_TEXT, dpi));
    let format = fonts.text_format(DWRITE_FONT_WEIGHT_NORMAL, size)?;
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
    let layout = text(fonts, LINK, &format, None)?;
    let width_of_link = width_of(&layout)?;
    let placed = place(
        layout,
        // Down to a whole pixel, so the link never reaches into the margin.
        (width - side - width_of_link).floor(),
        line,
        line_height,
    )?;
    draw_text(target, &placed, &link);
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
        target.FillRectangle(&underline, &link);
        target.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
    }
    Ok(RECT {
        left: whole(placed.left),
        top: whole(line.floor()),
        right: whole((placed.left + placed.width).ceil()),
        bottom: whole((line + line_height).ceil()),
    })
}

/// `words` laid out on one line in `format`, spaced as a heading when `spacing` gives its font size.
fn text(
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

fn width_of(layout: &IDWriteTextLayout) -> windows::core::Result<f32> {
    let mut metrics = DWRITE_TEXT_METRICS::default();
    // SAFETY: fills a local.
    unsafe { layout.GetMetrics(&mut metrics) }?;
    Ok(metrics.widthIncludingTrailingWhitespace)
}

/// `layout` with its left side at `left`, centered in a line of `line_height` from `line` down, as CSS centers a text
/// in its line box, with the baseline moved to the nearest whole pixel so the letters stay crisp.
fn place(
    layout: IDWriteTextLayout,
    left: f32,
    line: f32,
    line_height: f32,
) -> windows::core::Result<Placed> {
    let width = width_of(&layout)?;
    let mut metrics = [DWRITE_LINE_METRICS::default()];
    let mut lines = 0;
    // SAFETY: fills a one-line buffer, and fails should the text need more.
    unsafe { layout.GetLineMetrics(Some(&mut metrics), &mut lines) }?;
    let [metrics] = metrics;
    let baseline = (line + (line_height - metrics.height) / 2.0 + metrics.baseline).round();
    Ok(Placed {
        layout,
        left,
        top: baseline - metrics.baseline,
        width,
        baseline: metrics.baseline,
    })
}

fn draw_text(target: &ID2D1RenderTarget, placed: &Placed, brush: &ID2D1SolidColorBrush) {
    // The point type Direct2D takes, which windows 0.62 takes from a crate it doesn't re-export, as in the overlay.
    let mut origin = D2D1_ELLIPSE::default().point;
    origin.X = placed.left;
    origin.Y = placed.top;
    // SAFETY: drawing calls on a live render target, with arguments that outlive them.
    unsafe {
        target.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
        target.DrawTextLayout(origin, &placed.layout, brush, D2D1_DRAW_TEXT_OPTIONS_NONE);
    }
}

/// A whole number of pixels as the float Direct2D takes. Sizes here are a few thousand pixels at most, far below 2^24,
/// up to which `f32` holds every whole number.
fn px(pixels: u32) -> f32 {
    debug_assert!(pixels < 1 << 24, "{pixels} pixels");
    pixels as f32
}

/// A position already rounded to a whole number of pixels, as Windows takes a rectangle. Positions in the window are a
/// few thousand pixels at most, far inside `i32`.
// TODO: place the link in whole pixels without going through a float, so no float comes back to `i32` here.
fn whole(position: f32) -> i32 {
    debug_assert!(
        position.fract() == 0.0 && position.abs() < 1e6,
        "{position}"
    );
    position as i32
}

#[cfg(test)]
mod tests {
    use std::ptr;

    use windows::Win32::Graphics::Direct2D::Common::{
        D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_PIXEL_FORMAT,
    };
    use windows::Win32::Graphics::Direct2D::{
        D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_FEATURE_LEVEL_DEFAULT,
        D2D1_RENDER_TARGET_PROPERTIES, D2D1_RENDER_TARGET_TYPE_SOFTWARE,
        D2D1_RENDER_TARGET_USAGE_NONE, D2D1CreateFactory, ID2D1Factory,
    };
    use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
    use windows::Win32::Graphics::Imaging::{
        CLSID_WICImagingFactory, GUID_WICPixelFormat32bppPBGRA, IWICImagingFactory,
        WICBitmapCacheOnDemand,
    };
    use windows::Win32::System::Com::{
        CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
        CoUninitialize,
    };

    use screengauge_core::settings::{ColorFormat, Theme};

    use super::*;

    mod snip;

    /// The page as drawn, as rows of [r, g, b] pixels, where the link went and where the controls' parts are, and the
    /// page as a PNG, for a look at it.
    struct Picture {
        width: usize,
        pixels: Vec<[u8; 3]>,
        link: RECT,
        controls: Vec<(Control, RECT)>,
        /// The page as a PNG would hold it: its height, and its pixels in blue, green, red and alpha.
        height: u32,
        bgra: Vec<u8>,
    }

    impl Picture {
        /// Where `control` is.
        fn place_of(&self, control: Control) -> RECT {
            self.controls
                .iter()
                .find(|(drawn, _)| *drawn == control)
                .map(|&(_, place)| place)
                .unwrap_or_else(|| panic!("{control:?} isn't drawn"))
        }

        fn at(&self, x: i32, y: i32) -> [u8; 3] {
            let (x, y) = (
                usize::try_from(x).expect("x"),
                usize::try_from(y).expect("y"),
            );
            self.pixels[y * self.width + x]
        }
    }

    /// The page with the default options and the mouse away.
    fn drawn(mode: Mode, dpi: u32) -> Picture {
        drawn_with(mode, dpi, &Options::default(), Pointer::default())
    }

    /// Draws the page in software on a bitmap of its size at `dpi`, with no window, monitor or graphics card. COM,
    /// which the bitmap needs, is started for the test's thread and ended once everything made with it is dropped.
    fn drawn_with(mode: Mode, dpi: u32, options: &Options, pointer: Pointer) -> Picture {
        let width = scaled(layout::WIDTH, dpi);
        let height = scaled(layout::page().height, dpi);
        let _com = Com::start();
        {
            // SAFETY: COM is started for this thread.
            let wic: IWICImagingFactory =
                unsafe { CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER) }
                    .expect("the WIC factory");
            // SAFETY: plain values, and a pixel format that outlives the call.
            let bitmap = unsafe {
                wic.CreateBitmap(
                    width,
                    height,
                    &GUID_WICPixelFormat32bppPBGRA,
                    WICBitmapCacheOnDemand,
                )
            }
            .expect("a WIC bitmap");
            // SAFETY: a single-threaded factory used only on this thread.
            let factory: ID2D1Factory =
                unsafe { D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None) }
                    .expect("the Direct2D factory");
            let properties = D2D1_RENDER_TARGET_PROPERTIES {
                r#type: D2D1_RENDER_TARGET_TYPE_SOFTWARE,
                pixelFormat: D2D1_PIXEL_FORMAT {
                    format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                },
                dpiX: 96.0,
                dpiY: 96.0,
                usage: D2D1_RENDER_TARGET_USAGE_NONE,
                minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
            };
            // SAFETY: the bitmap and the properties outlive the call.
            let target = unsafe { factory.CreateWicBitmapRenderTarget(&bitmap, &properties) }
                .expect("a render target on the bitmap");
            let fonts = Fonts::new().expect("the fonts");
            // SAFETY: BeginDraw and EndDraw around the drawing, on a live target.
            unsafe { target.BeginDraw() };
            let surface = Surface {
                target: &target,
                factory: &factory,
                fonts: &fonts,
                mode,
                dpi,
                width,
            };
            let drawn = draw(&surface, options, pointer).expect("the page is drawn");
            // SAFETY: ends the BeginDraw above.
            unsafe { target.EndDraw(None, None) }.expect("the drawing is finished");
            let stride = width * 4;
            let mut bytes = vec![0; usize::try_from(stride * height).expect("a small bitmap")];
            // SAFETY: no rectangle copies the whole bitmap, into a buffer of exactly its size.
            unsafe { bitmap.CopyPixels(ptr::null(), stride, &mut bytes) }
                .expect("the pixels are read");
            let pixels = bytes
                .as_chunks::<4>()
                .0
                .iter()
                .map(|&[b, g, r, _]| [r, g, b])
                .collect();
            Picture {
                width: usize::try_from(width).expect("a small bitmap"),
                // Blue, green, red and alpha; every pixel of the page is opaque.
                pixels,
                link: drawn.link,
                controls: drawn.controls,
                height,
                bgra: bytes,
            }
        }
    }

    /// COM for the test's thread, which the bitmap needs, ended when this drops, after every object made with it: also
    /// when a check fails and unwinds.
    struct Com;

    impl Com {
        fn start() -> Com {
            // SAFETY: starts COM for this thread, undone once by the drop of the Com it returns.
            unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }
                .ok()
                .expect("COM starts on a test thread");
            Com
        }
    }

    impl Drop for Com {
        fn drop(&mut self) {
            // SAFETY: pairs the CoInitializeEx in `start`.
            unsafe { CoUninitialize() };
        }
    }

    fn near(pixel: [u8; 3], color: Color, within: u8) -> bool {
        pixel
            .iter()
            .zip(color.rgb)
            .all(|(&have, want)| have.abs_diff(want) <= within)
    }

    fn brightness(pixel: [u8; 3]) -> i32 {
        pixel.iter().map(|&channel| i32::from(channel)).sum()
    }

    fn whole_px(logical: u32, dpi: u32) -> i32 {
        i32::try_from(scaled(logical, dpi)).expect("a small page")
    }

    /// The owner's monitors' scales, and Full HD at 150%.
    const DPIS: [u32; 3] = [120, 144, 168];

    /// The average color of the pixels inside `place`, `inset` pixels in from each side.
    fn average(page: &Picture, place: RECT, inset: i32) -> [f64; 3] {
        let mut sum = [0.0; 3];
        let mut count = 0.0;
        for y in place.top + inset..place.bottom - inset {
            for x in place.left + inset..place.right - inset {
                for (total, channel) in sum.iter_mut().zip(page.at(x, y)) {
                    *total += f64::from(channel);
                }
                count += 1.0;
            }
        }
        sum.map(|total| total / count)
    }

    /// How much bluer than red a box is on average: the cyan of a lit control is far bluer, the greys far less.
    fn cyan(page: &Picture, place: RECT) -> f64 {
        let [r, _, b] = average(page, place, 3);
        b - r
    }

    fn width_of_rect(place: RECT) -> i32 {
        place.right - place.left
    }

    fn height_of_rect(place: RECT) -> i32 {
        place.bottom - place.top
    }

    #[test]
    fn the_controls_are_where_the_prototype_puts_them_right_of_their_rows() {
        // Measured in the prototype at 100% for WPC-320, from the window's left: every control ends at 531, the card's
        // right side less its border and the row's padding; the switch is 42 × 24, the buttons 30 × 30, the number's box
        // 40 wide, and the choice 32 tall with its segments 26.
        for mode in [Mode::Dark, Mode::Light] {
            let page = drawn(mode, 96);
            let switch = page.place_of(Control::LogicalPixels);
            assert_eq!(
                (switch.right, width_of_rect(switch), height_of_rect(switch)),
                (531, 42, 24)
            );
            let plus = page.place_of(Control::ToleranceUp);
            let minus = page.place_of(Control::ToleranceDown);
            assert_eq!(
                (plus.right, width_of_rect(plus), height_of_rect(plus)),
                (531, 30, 30)
            );
            // − / + is 112 wide: two buttons, the number's 40 and two gaps of 6.
            assert_eq!(plus.right - minus.left, 112);
            let both = page.place_of(Control::ColorFormat(ColorFormat::Both));
            assert_eq!((both.right, height_of_rect(both)), (528, 26));
            let hex = page.place_of(Control::ColorFormat(ColorFormat::Hex));
            // The prototype's choice is 143.29 wide: 3 of padding either side and between, and each word with 10
            // either side.
            let choice = both.right + 3 - (hex.left - 3);
            assert!((142..=146).contains(&choice), "{choice}");
            // Each control is centered in its row.
            for control in [
                Control::LogicalPixels,
                Control::ToleranceUp,
                Control::ColorFormat(ColorFormat::Hex),
            ] {
                let place = page.place_of(control);
                let row = match control {
                    Control::LogicalPixels => layout::place_row(Row::AlsoShowLogicalPixels),
                    Control::ColorFormat(_) => layout::place_row(Row::ColorFormat),
                    _ => layout::place_row(Row::DefaultTolerance),
                };
                let above = place.top - i32::try_from(row.span.top).expect("small");
                let below = i32::try_from(row.span.bottom).expect("small") - place.bottom;
                assert!(
                    (above - below).abs() <= 1,
                    "{control:?}: {above} above, {below} below"
                );
            }
        }
    }

    #[test]
    fn the_controls_grow_with_the_scale() {
        for dpi in DPIS {
            let page = drawn(Mode::Dark, dpi);
            let switch = page.place_of(Control::LogicalPixels);
            assert_eq!(width_of_rect(switch), whole_px(42, dpi), "{dpi}");
            assert_eq!(height_of_rect(switch), whole_px(24, dpi), "{dpi}");
            let plus = page.place_of(Control::ToleranceUp);
            assert_eq!(width_of_rect(plus), whole_px(30, dpi), "{dpi}");
            assert_eq!(
                plus.right,
                i32::try_from(page.width).expect("small") - whole_px(29, dpi),
                "{dpi}"
            );
        }
    }

    #[test]
    fn the_switch_s_track_is_cyan_when_on_and_sunken_grey_when_off() {
        for mode in [Mode::Dark, Mode::Light] {
            let on = drawn(mode, 144);
            let off = drawn_with(
                mode,
                144,
                &Options {
                    also_show_logical_pixels: false,
                    ..Options::default()
                },
                Pointer::default(),
            );
            let track = on.place_of(Control::LogicalPixels);
            assert!(cyan(&on, track) > 60.0, "{mode:?}: {}", cyan(&on, track));
            assert!(
                cyan(&off, track).abs() < 25.0,
                "{mode:?}: {}",
                cyan(&off, track)
            );
        }
    }

    #[test]
    fn the_switch_s_knob_sits_right_when_on_and_left_when_off() {
        let dpi = 144;
        for mode in [Mode::Dark, Mode::Light] {
            let on = drawn(mode, dpi);
            let off = drawn_with(
                mode,
                dpi,
                &Options {
                    also_show_logical_pixels: false,
                    ..Options::default()
                },
                Pointer::default(),
            );
            let track = on.place_of(Control::LogicalPixels);
            let middle = (track.top + track.bottom) / 2;
            // The knob's middle, 3 + 9 logical pixels in from the side it sits on.
            let near_left = track.left + whole_px(12, dpi);
            let near_right = track.right - whole_px(12, dpi);
            // On, the knob is white over the cyan; off, it is the raised button over the sunken well.
            assert!(
                brightness(on.at(near_right, middle)) > brightness(on.at(near_left, middle)),
                "{mode:?} on"
            );
            assert_ne!(
                off.at(near_left, middle),
                off.at(near_right, middle),
                "{mode:?} off"
            );
            assert!(
                cyan(
                    &off,
                    RECT {
                        left: near_left - 4,
                        right: near_left + 4,
                        top: middle - 4,
                        bottom: middle + 4
                    }
                )
                .abs()
                    < 25.0
            );
        }
    }

    #[test]
    fn only_the_chosen_format_is_cyan() {
        for mode in [Mode::Dark, Mode::Light] {
            for (format, _) in COLOR_FORMATS {
                let page = drawn_with(
                    mode,
                    144,
                    &Options {
                        color_format: format,
                        ..Options::default()
                    },
                    Pointer::default(),
                );
                for (other, _) in COLOR_FORMATS {
                    let lit = cyan(&page, page.place_of(Control::ColorFormat(other)));
                    if other == format {
                        assert!(lit > 50.0, "{mode:?} {format:?}: {lit}");
                    } else {
                        assert!(lit.abs() < 25.0, "{mode:?} {format:?} {other:?}: {lit}");
                    }
                }
            }
        }
    }

    #[test]
    fn the_theme_s_choice_lights_the_theme_chosen_automatic_by_default() {
        for mode in [Mode::Dark, Mode::Light] {
            for (theme, _) in THEMES {
                let page = drawn_with(
                    mode,
                    144,
                    &Options {
                        theme,
                        ..Options::default()
                    },
                    Pointer::default(),
                );
                for (other, _) in THEMES {
                    let lit = cyan(&page, page.place_of(Control::Theme(other)));
                    if other == theme {
                        assert!(lit > 50.0, "{mode:?} {theme:?}: {lit}");
                    } else {
                        assert!(lit.abs() < 25.0, "{mode:?} {theme:?} {other:?}: {lit}");
                    }
                }
            }
        }
        let page = drawn(Mode::Light, 144);
        assert!(cyan(&page, page.place_of(Control::Theme(Theme::Automatic))) > 50.0);
    }

    #[test]
    fn the_theme_s_choice_is_in_general_s_last_row_at_the_right_like_the_others() {
        // Measured in the prototype at 100%: "Automatic · Light · Dark" is 181.67 wide and ends at 531.
        let page = drawn(Mode::Dark, 96);
        let automatic = page.place_of(Control::Theme(Theme::Automatic));
        let dark = page.place_of(Control::Theme(Theme::Dark));
        assert_eq!(dark.right + 3, 531);
        let width = dark.right + 3 - (automatic.left - 3);
        assert!((180..=184).contains(&width), "{width}");
        let row = layout::place_row(Row::Theme);
        assert!(automatic.top > i32::try_from(row.span.top).expect("small"));
        assert!(automatic.bottom < i32::try_from(row.span.bottom).expect("small"));
    }

    #[test]
    fn the_number_between_minus_and_plus_is_the_default_tolerance() {
        let dpi = 144;
        let mut previous: Option<Vec<[u8; 3]>> = None;
        for value in [2, 13] {
            let options = Options {
                default_tolerance: screengauge_core::settings::Tolerance::new(value)
                    .expect("a tolerance"),
                ..Options::default()
            };
            let page = drawn_with(Mode::Dark, dpi, &options, Pointer::default());
            let minus = page.place_of(Control::ToleranceDown);
            let plus = page.place_of(Control::ToleranceUp);
            // The number's box, between the two buttons: text in the light ink shows there.
            let number = RECT {
                left: minus.right,
                right: plus.left,
                ..minus
            };
            let inked = (number.top..number.bottom)
                .flat_map(|y| (number.left..number.right).map(move |x| (x, y)))
                .filter(|&(x, y)| near(page.at(x, y), controls::DARK.ink, 40))
                .count();
            assert!(inked > 20, "{value}: {inked} pixels of ink");
            let pixels: Vec<[u8; 3]> = (number.top..number.bottom)
                .flat_map(|y| (number.left..number.right).map(move |x| (x, y)))
                .map(|(x, y)| page.at(x, y))
                .collect();
            if let Some(previous) = previous {
                assert_ne!(previous, pixels, "2 and 13 look the same");
            }
            previous = Some(pixels);
        }
    }

    #[test]
    fn plus_brightens_with_the_cursor_on_it_and_shrinks_while_held() {
        let dpi = 144;
        let options = Options::default();
        let rest = drawn(Mode::Dark, dpi);
        let over = drawn_with(
            Mode::Dark,
            dpi,
            &options,
            Pointer {
                over: Some(Control::ToleranceUp),
                held: None,
            },
        );
        let held = drawn_with(
            Mode::Dark,
            dpi,
            &options,
            Pointer {
                over: Some(Control::ToleranceUp),
                held: Some(Control::ToleranceUp),
            },
        );
        let plus = rest.place_of(Control::ToleranceUp);
        let minus = rest.place_of(Control::ToleranceDown);
        let brightest = |page: &Picture, place: RECT| {
            (place.top..place.bottom)
                .flat_map(|y| (place.left..place.right).map(move |x| (x, y)))
                .map(|(x, y)| brightness(page.at(x, y)))
                .max()
                .unwrap_or(0)
        };
        // The sign goes white (#fff) from its grey (#aeb3bd); − stays as it was.
        assert!(brightest(&over, plus) > brightest(&rest, plus), "hover");
        assert_eq!(brightest(&over, minus), brightest(&rest, minus));
        // Held, the button is 0.92 of its size: its corner pixel shows the card behind it.
        let corner = (plus.left + 1, (plus.top + plus.bottom) / 2);
        assert_ne!(
            held.at(corner.0, corner.1),
            over.at(corner.0, corner.1),
            "held"
        );
    }

    #[test]
    fn measurements_rows_have_a_line_between_them_and_inspect_s_none() {
        let dpi = 144;
        for mode in [Mode::Dark, Mode::Light] {
            let page = drawn(mode, dpi);
            let row = layout::place_row(Row::AlsoShowLogicalPixels);
            let line = whole_px(row.span.top, dpi) - 1;
            let x = whole_px(200, dpi);
            assert_ne!(page.at(x, line), page.at(x, line - 2), "{mode:?}");
            let color = layout::place_row(Row::ColorFormat);
            let middle = whole_px((color.span.top + color.span.bottom) / 2, dpi);
            assert!(
                // Only the background's gradient changes from one pixel row to the next, by a level at most.
                (whole_px(color.span.top, dpi) + 2..middle - 8).all(|y| near(
                    page.at(x, y),
                    Color {
                        rgb: page.at(x, y + 1),
                        alpha: 1.0
                    },
                    1
                )),
                "{mode:?}: a line in Inspect's card"
            );
        }
    }

    #[test]
    fn each_row_s_label_is_in_the_ink_and_its_note_in_the_dim_color() {
        let dpi = 144;
        for mode in [Mode::Dark, Mode::Light] {
            let page = drawn(mode, dpi);
            let row = layout::place_row(Row::DefaultTolerance);
            let (top, bottom) = (whole_px(row.span.top, dpi), whole_px(row.span.bottom, dpi));
            let middle = (top + bottom) / 2;
            let left = whole_px(29, dpi);
            let count = |from: i32, to: i32, color: Color| {
                (from..to)
                    .flat_map(|y| (left..left + whole_px(150, dpi)).map(move |x| (x, y)))
                    .filter(|&(x, y)| near(page.at(x, y), color, 30))
                    .count()
            };
            assert!(count(top, middle, palette(mode).ink) > 50, "{mode:?} label");
            assert!(
                count(middle, bottom, palette(mode).dim) > 50,
                "{mode:?} note"
            );
        }
    }

    /// Writes the page in both modes and at the owner's scales to target\settings-pages, for a look at it.
    #[test]
    #[ignore = "writes PNG files for a person to look at"]
    fn write_the_pages_for_a_look() {
        let folder =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/settings-pages");
        std::fs::create_dir_all(&folder).expect("the folder");
        for mode in [Mode::Dark, Mode::Light] {
            for dpi in [96, 120, 144, 168] {
                let page = drawn(mode, dpi);
                let width = u32::try_from(page.width).expect("small");
                let image = screengauge_core::session::Image::new(width, page.height, page.bgra)
                    .expect("the bitmap's size");
                // On a thread of its own, which the encoder starts COM on in its own way.
                let png = std::thread::spawn(move || crate::png::encode(&image))
                    .join()
                    .expect("the encoder's thread")
                    .expect("the page as a PNG");
                std::fs::write(folder.join(format!("{mode:?}-{dpi}.png")), png).expect("the PNG");
            }
        }
    }

    #[test]
    fn the_background_runs_from_the_top_color_to_the_bottom_one() {
        for mode in [Mode::Dark, Mode::Light] {
            for dpi in DPIS {
                let page = drawn(mode, dpi);
                let bottom = whole_px(layout::page().height, dpi) - 1;
                assert!(near(page.at(2, 0), palette(mode).top, 1), "{mode:?} {dpi}");
                assert!(
                    near(page.at(2, bottom), palette(mode).bottom, 1),
                    "{mode:?} {dpi}"
                );
            }
        }
    }

    #[test]
    fn the_link_is_on_the_right_of_the_footer_and_underlined_in_the_link_color() {
        for mode in [Mode::Dark, Mode::Light] {
            for dpi in DPIS {
                let page = drawn(mode, dpi);
                let link = page.link;
                let footer = layout::page().footer;
                // It ends 16 logical pixels in from the right, inside the footer.
                let right = whole_px(layout::WIDTH - layout::SIDE, dpi);
                assert!(
                    (right - 1..=right).contains(&link.right),
                    "{mode:?} {dpi}: {link:?}"
                );
                assert!(link.left > right / 2, "{mode:?} {dpi}: {link:?}");
                let (top, bottom) = (whole_px(footer.top, dpi), whole_px(footer.bottom, dpi));
                assert!(
                    top < link.top && link.bottom < bottom,
                    "{mode:?} {dpi}: {link:?}"
                );
                // A row below the text is the link's color all along: the underline.
                let underlined = (link.top..bottom).any(|y| {
                    (link.left + 1..link.right - 1)
                        .all(|x| near(page.at(x, y), palette(mode).link, 2))
                });
                assert!(underlined, "{mode:?} {dpi}: no underline in {link:?}");
            }
        }
    }

    #[test]
    fn the_footer_s_texts_on_the_left_are_drawn_in_the_dim_color() {
        for mode in [Mode::Dark, Mode::Light] {
            let page = drawn(mode, 144);
            let link = page.link;
            let texts = (link.top..link.bottom)
                .any(|y| (24..420).any(|x| near(page.at(x, y), palette(mode).dim, 24)));
            assert!(texts, "{mode:?}");
        }
    }

    #[test]
    fn the_cards_are_lighter_than_the_background_and_the_footer_has_its_line() {
        let dpi = 144;
        for mode in [Mode::Dark, Mode::Light] {
            let page = drawn(mode, dpi);
            for placed in layout::page().blocks {
                // Inside the card, past its border and corner and above its rows' labels, and beside it on the same row.
                let y = whole_px(placed.card.top + 4, dpi);
                let inside = page.at(whole_px(layout::SIDE + 30, dpi), y);
                let beside = page.at(2, y);
                assert!(
                    brightness(inside) > brightness(beside),
                    "{mode:?} {:?}: {inside:?} beside {beside:?}",
                    placed.block
                );
            }
            // The footer's 1-pixel line differs from the background just above it.
            let line = whole_px(layout::page().footer.top, dpi);
            assert_ne!(page.at(300, line), page.at(300, line - 1), "{mode:?}");
        }
    }
}
