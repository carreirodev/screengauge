//! The measurements drawn over a window's photo: Dimensions' cyan line with its ticks and outline, Bounds' drawn
//! rectangle and the content it shrinks to, and the label in a pill, for the pinned measurements and the live one; Snip's
//! corners and label; the toolbar, the tolerance indicator and the notice. Everything about where they go is in
//! `geometry`, `bounds`, `snip`, `toolbar`, `tolerance` and `notice`, which need no Windows; this draws them onto any
//! Direct2D render target at 96 DPI, so a test can render them on a bitmap and count pixels.

mod bounds;
#[cfg(test)]
mod bounds_tests;
mod floating;
mod geometry;
mod magnified_view;
#[cfg(test)]
mod magnified_view_tests;
mod notice;
#[cfg(test)]
mod notice_side_tests;
mod palette;
#[cfg(test)]
mod panel_tests;
mod snip;
#[cfg(test)]
mod snip_tests;
mod svg_path;
mod tolerance;
#[cfg(test)]
mod tolerance_tests;
pub mod toolbar;
mod toolbar_cache;
mod toolbar_icons;
mod toolbar_paint;
#[cfg(test)]
mod toolbar_tests;
#[cfg(test)]
mod typeset_tests;

use std::cell::RefCell;

use screengauge_core::session::{
    DrawnRectangle, Measurement, Notice, Point, Rect, Scale, ToleranceIndicator,
};
use windows::Win32::Foundation::{E_FAIL, RECT};
use windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F;
use windows::Win32::Graphics::Direct2D::{
    D2D1_ANTIALIAS_MODE_ALIASED, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_DRAW_TEXT_OPTIONS_NONE,
    D2D1_ROUNDED_RECT, D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE, ID2D1Factory, ID2D1RenderTarget,
    ID2D1RoundedRectangleGeometry, ID2D1SolidColorBrush, ID2D1StrokeStyle,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FONT_FEATURE, DWRITE_FONT_FEATURE_TAG_KERNING, DWRITE_FONT_FEATURE_TAG_TABULAR_FIGURES,
    DWRITE_FONT_WEIGHT, DWRITE_FONT_WEIGHT_MEDIUM, DWRITE_FONT_WEIGHT_SEMI_BOLD,
    DWRITE_LINE_METRICS, DWRITE_TEXT_METRICS, DWRITE_TEXT_RANGE, IDWriteTextFormat,
    IDWriteTextLayout, IDWriteTypography,
};
use windows::core::Error;

use crate::graphics::{Color, Fonts, color, solid_brush};
use crate::log;
use crate::relief::{self, dot, in_surface, px, ring, rounded_at};
use crate::theme::Mode;
use geometry::{Parts, scaled};
use palette::Surfaces;

/// The labels' font in `weight`, `size` logical pixels tall in a window of `dpi`, scaled to whole pixels, which keep
/// the digits crisp. Its digits differ in width unless asked for tabular figures (a 1 is two pixels narrower than a 0
/// at this size), which the label does, so the pill keeps its width while the number keeps its length.
fn text_format(
    fonts: &Fonts,
    weight: DWRITE_FONT_WEIGHT,
    size: u32,
    dpi: u32,
) -> windows::core::Result<IDWriteTextFormat> {
    fonts.text_format(weight, px(scaled(size, dpi)))
}

/// The live label's ink, on the cyan (#04161b): its text, and the camera on Snip's label.
const LIVE_INK: Color = color(0x04, 0x16, 0x1b, 1.0);

/// How much of the outline's black shows over the photo.
const OUTLINE_OPACITY: f32 = 0.28;

/// How much of the label's ink shows in the bar before the logical part, and in the logical part itself (CSS opacity
/// .4 and .62): fainter than the physical part, which is in full ink.
const BAR_OPACITY: f32 = 0.4;
const LOGICAL_OPACITY: f32 = 0.62;

/// The logical part's weight, the prototype's CSS 500. Segoe UI has no face at 500, and DirectWrite gives its semibold
/// for it, so the logical part is fainter by its opacity alone. Chromium does the same on Windows 11 (Segoe UI at 500
/// lays out exactly as wide as at 600 and wider than at 400, measured for WPC-307), and the owner approved this label
/// on screen in WPC-307.
const LOGICAL_WEIGHT: DWRITE_FONT_WEIGHT = DWRITE_FONT_WEIGHT_MEDIUM;

/// The pill's shadow, approximating CSS `0 2px 7px rgba(0,0,0,.35)`: black rounded rectangles that grow by this many
/// logical pixels and show this much, laid over each other, so the shadow fades outward from the pill.
const SHADOW: [(u32, f32); 5] = [(0, 0.05), (1, 0.04), (2, 0.04), (4, 0.03), (6, 0.02)];

/// What draws the measurements and the notice on one monitor's window: brushes and a text format made for its render
/// target and its scale. A render target that is lost takes its brushes with it, so this is made again with everything
/// else.
pub struct Overlay {
    cyan: ID2D1SolidColorBrush,
    /// Black. Each fill sets its opacity first: the outline's, the drawn rectangle's, then each layer of the shadow's.
    shade: ID2D1SolidColorBrush,
    /// The drawn rectangle's dashes.
    white: ID2D1SolidColorBrush,
    /// The magnified view's swatch. Each use sets its color first: the pixel's under the cursor.
    swatch: ID2D1SolidColorBrush,
    /// The live label's ink. Each use sets its opacity first, because the brush outlives a frame: full for the
    /// physical text, then the bar's and the logical text's.
    ink: ID2D1SolidColorBrush,
    /// The brushes of the palette the pinned labels and the notice are drawn in.
    surfaces: Surfaces,
    /// Makes the shapes that are cut out of others.
    factory: ID2D1Factory,
    /// Round caps and joins, for the strokes of the notice's sign.
    stroke: ID2D1StrokeStyle,
    /// The physical text's: Segoe UI semibold.
    format: IDWriteTextFormat,
    /// The logical text's: the same family, size and wrapping, asked for the medium weight.
    logical_format: IDWriteTextFormat,
    /// The tolerance indicator's texts: its word in the same semibold at its own size, and its number in the black
    /// face the prototype's `<b>` gets (see `tolerance`).
    word_format: IDWriteTextFormat,
    number_format: IDWriteTextFormat,
    /// The magnification indicator: bold, at its own size.
    magnification_indicator_format: IDWriteTextFormat,
    /// The magnified view's Approximate color notice: semibold, at its own size.
    approximate_format: IDWriteTextFormat,
    /// Asks for tabular figures, and for the kerning that asking for any feature would otherwise drop; applied to each
    /// label's and notice's text.
    figures: IDWriteTypography,
    fonts: Fonts,
    /// The palette the toolbar is drawn in; the pinned labels' and the notice's are in `surfaces`.
    mode: Mode,
    /// The toolbar, drawn on a bitmap that every frame lays over the photo. Lost with the rest of the overlay should the
    /// render target be.
    toolbar_cache: RefCell<toolbar_cache::ToolbarCache>,
    /// The monitor's scale, which the toolbar's parts are laid out at, wherever the scene puts it.
    scale: Scale,
    /// The window's monitor in the virtual screen, which is where the window's pixel (0, 0) is.
    monitor: RECT,
    dpi: u32,
}

/// Which of the two looks a label has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Look {
    /// The live measurement's: a cyan pill.
    Live,
    /// A pinned measurement's: the palette's panel, with a ring around it and a cyan strip on its left.
    Pinned,
}

/// A label ready to draw.
struct PlacedLabel {
    look: Look,
    /// The pill's place in the virtual screen.
    pill: RECT,
    physical: PlacedText,
    /// The bar and the logical text after the physical one. `None` when the measurement has no logical part, and then
    /// the label is the physical text alone.
    logical: Option<PlacedLogical>,
}

/// A line of text with the place it is drawn at.
struct PlacedText {
    layout: IDWriteTextLayout,
    /// Where the text's layout starts, in the window's own pixels. Its baseline is on a whole pixel.
    left: f32,
    top: f32,
}

/// What a label draws after its physical text when it has a logical part.
struct PlacedLogical {
    /// The bar between the two parts, in the window's own pixels.
    bar: D2D_RECT_F,
    text: PlacedText,
}

/// A text laid out, with what the label needs to place it.
struct Typeset {
    layout: IDWriteTextLayout,
    /// The text's width, in whole pixels: the room the box around it counts for it (see `Overlay::typeset`).
    width: i32,
    /// The distance from the layout's top to its baseline.
    baseline: f32,
    /// The line's height, which the notice centers in its own line.
    line_height: f32,
}

impl Typeset {
    /// Where the text's baseline goes for it to sit in a line `line` pixels tall whose top is at `top`, the way CSS puts
    /// it there: the line's extra room, above and below the text's own height, is shared equally. It is on a whole pixel,
    /// so the letters are crisp.
    fn baseline_in(&self, top: f32, line: f32) -> f32 {
        (top + (line - self.line_height) / 2.0 + self.baseline).round()
    }

    /// `layout`, measured.
    fn of(layout: IDWriteTextLayout) -> windows::core::Result<Typeset> {
        let mut metrics = DWRITE_TEXT_METRICS::default();
        // SAFETY: fills a local.
        unsafe { layout.GetMetrics(&mut metrics) }?;
        let mut line = [DWRITE_LINE_METRICS::default()];
        let mut lines = 0;
        // SAFETY: fills a one-line buffer, and fails should the label need more.
        unsafe { layout.GetLineMetrics(Some(&mut line), &mut lines) }?;
        let [line] = line;
        Ok(Typeset {
            layout,
            width: whole_up(metrics.width),
            baseline: line.baseline,
            line_height: line.height,
        })
    }
}

impl Overlay {
    /// For a window over `monitor`, whose scale is `scale`, drawing on `target`, with ScreenGauge's own surfaces (the
    /// pinned measurements' labels and the notices) in the palette of `mode`.
    pub fn new(
        target: &ID2D1RenderTarget,
        fonts: &Fonts,
        monitor: Rect,
        scale: Scale,
        mode: Mode,
    ) -> windows::core::Result<Overlay> {
        let dpi = scale.dpi();
        let brush = |r, g, b| solid_brush(target, color(r, g, b, 1.0));
        // SAFETY: no arguments.
        let figures = unsafe { fonts.factory.CreateTypography() }?;
        // Asking DirectWrite for a feature replaces its default ones, kerning among them, which would leave "Couldn't
        // save." a pixel wider than Chromium draws it. Kerning is the only default that changes these texts' width: with
        // it back, every label and notice is as wide as with no features asked for, measured at 100 to 175% for WPC-354,
        // ligatures and contextual alternates asked for too or not.
        for feature in [
            DWRITE_FONT_FEATURE_TAG_TABULAR_FIGURES,
            DWRITE_FONT_FEATURE_TAG_KERNING,
        ] {
            // SAFETY: plain values.
            unsafe {
                figures.AddFontFeature(DWRITE_FONT_FEATURE {
                    nameTag: feature,
                    parameter: 1,
                })
            }?;
        }
        // SAFETY: no arguments; the factory comes back as an owned interface.
        let factory = unsafe { target.GetFactory() }?;
        let stroke = crate::sign::round_stroke(&factory)?;
        Ok(Overlay {
            // #22d3ee, the same cyan whatever the Windows theme.
            cyan: brush(0x22, 0xd3, 0xee)?,
            shade: brush(0, 0, 0)?,
            white: brush(255, 255, 255)?,
            swatch: brush(0, 0, 0)?,
            ink: solid_brush(target, LIVE_INK)?,
            surfaces: Surfaces::new(target, mode)?,
            factory,
            stroke,
            format: text_format(fonts, DWRITE_FONT_WEIGHT_SEMI_BOLD, geometry::TEXT, dpi)?,
            logical_format: text_format(fonts, LOGICAL_WEIGHT, geometry::TEXT, dpi)?,
            word_format: text_format(fonts, DWRITE_FONT_WEIGHT_SEMI_BOLD, tolerance::TEXT, dpi)?,
            number_format: text_format(fonts, tolerance::NUMBER_WEIGHT, tolerance::TEXT, dpi)?,
            magnification_indicator_format: text_format(
                fonts,
                magnified_view::INDICATOR_WEIGHT,
                magnified_view::INDICATOR_TEXT,
                dpi,
            )?,
            approximate_format: text_format(
                fonts,
                DWRITE_FONT_WEIGHT_SEMI_BOLD,
                magnified_view::APPROXIMATE_TEXT,
                dpi,
            )?,
            figures,
            fonts: fonts.clone(),
            mode,
            toolbar_cache: RefCell::default(),
            scale,
            monitor: geometry::bounds(monitor)?,
            dpi,
        })
    }

    /// Draws the pinned measurements, oldest first, and then the live one over what `target` holds, between its
    /// BeginDraw and EndDraw, over the rectangle Bounds is drawing, `drawn`, when there is one here. The pinned labels
    /// are in the palette, with a cyan strip on their left; the live one is cyan, and over the others.
    ///
    /// Order matters, and it is across the measurements, not within each: the drawn rectangle; every content's fill;
    /// every label's shadow, which is faint and reaches toward the lines; every outline; every cyan pixel, so nothing
    /// darkens one, whichever measurement it belongs to; then the pills with their text. A pill never touches its own
    /// line, being further from it than a tick reaches, but it covers another measurement's. A label that can't be laid out doesn't stop the others; the
    /// first failure is returned once everything else is drawn, and any later one is logged.
    pub fn draw(
        &self,
        target: &ID2D1RenderTarget,
        pinned: &[&Measurement],
        live: Option<&Measurement>,
        drawn: Option<&DrawnRectangle>,
    ) -> windows::core::Result<()> {
        let mut first_failure = None;
        let mut keep = |result: windows::core::Result<()>| {
            if let Err(e) = result {
                if first_failure.is_none() {
                    first_failure = Some(e);
                } else {
                    log::failure("frozen screen: drawing a label", &e);
                }
            }
        };
        let mut parts = Vec::new();
        let mut fills = Vec::new();
        let mut labels = Vec::new();
        let pinned = pinned.iter().map(|&m| (m, Look::Pinned));
        for (measurement, look) in pinned.chain(live.map(|m| (m, Look::Live))) {
            parts.push(match measurement.content {
                Some(content) => bounds::parts(content),
                None => geometry::parts(measurement, self.dpi),
            });
            fills.extend(measurement.content.map(|content| (content, look)));
            match self.label(measurement, look) {
                Ok(Some(label)) => labels.push(label),
                // No span and no content, so nothing to label.
                Ok(None) => {}
                Err(e) => keep(Err(e)),
            }
        }
        if let Some(drawn) = drawn {
            self.draw_drawn_rectangle(target, drawn);
        }
        keep(self.draw_fills(target, &fills));
        for label in &labels {
            keep(self.draw_shadow(target, label));
        }
        self.draw_outlines(target, &parts);
        self.draw_cyan(target, &parts);
        for label in &labels {
            self.draw_pill(target, label);
        }
        first_failure.map_or(Ok(()), Err)
    }

    /// Draws `notice` over everything else, inside the window's monitor, between BeginDraw and EndDraw: the palette's
    /// panel with its border and shadow, the sign and the text, above and to the right of where it appeared.
    pub fn draw_notice(
        &self,
        target: &ID2D1RenderTarget,
        notice: &Notice,
    ) -> windows::core::Result<()> {
        self.show_notice(
            target,
            notice.at,
            notice.side,
            notice.message.text(),
            notice.message.sign(),
        )
    }

    /// Draws the tolerance indicator over the measurements and under any notice, inside the window's monitor, between
    /// BeginDraw and EndDraw: the palette's panel with its border and shadow, "Tolerance N" and the bar, to the left of
    /// the cursor and above it, or below it in the horizontal mode or where above it would hide the label of `live`,
    /// what Dimensions measures here.
    pub fn draw_tolerance(
        &self,
        target: &ID2D1RenderTarget,
        indicator: &ToleranceIndicator,
        live: Option<&Measurement>,
    ) -> windows::core::Result<()> {
        self.show_tolerance(target, indicator.at, indicator.tolerance, live)
    }

    /// The outlines of every measurement.
    fn draw_outlines(&self, target: &ID2D1RenderTarget, parts: &[Parts]) {
        // SAFETY: drawing calls on a live render target with arguments that outlive them. Out of BeginDraw and EndDraw,
        // Direct2D only fails the frame at EndDraw. Aliased fills of rectangles on whole pixels cover exactly those
        // pixels, and the target is at 96 DPI, so one unit is one physical pixel.
        unsafe {
            target.SetAntialiasMode(D2D1_ANTIALIAS_MODE_ALIASED);
            self.shade.SetOpacity(OUTLINE_OPACITY);
            for outline in parts.iter().flat_map(|parts| &parts.outline) {
                target.FillRectangle(&self.in_window(outline), &self.shade);
            }
            // Back to Direct2D's default, which the pills' corners need.
            target.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
        }
    }

    /// The line and the ticks of every measurement.
    fn draw_cyan(&self, target: &ID2D1RenderTarget, parts: &[Parts]) {
        // SAFETY: as in `draw_outlines`. DrawLine is never used: on whole coordinates it straddles two pixels.
        unsafe {
            target.SetAntialiasMode(D2D1_ANTIALIAS_MODE_ALIASED);
            for line in parts.iter().flat_map(|parts| &parts.line) {
                target.FillRectangle(&self.in_window(line), &self.cyan);
            }
            target.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
        }
    }

    /// The label's size and place, and where its text goes. `None` when the measurement has no span, so there is
    /// nothing to label. A pinned label is as wide and as tall as its border makes it, and its text starts inside the
    /// border, so it sits further right and down than a live label's at the same place.
    fn label(
        &self,
        measurement: &Measurement,
        look: Look,
    ) -> windows::core::Result<Option<PlacedLabel>> {
        let label = &measurement.label;
        let physical = self.typeset(&label.physical, &self.format)?;
        // The logical text, if the label has one, with where the bar and that text start along the label.
        let after = label
            .logical
            .as_deref()
            .map(|text| self.typeset(text, &self.logical_format))
            .transpose()?
            .map(|typeset| {
                (
                    geometry::columns(physical.width, typeset.width, self.dpi),
                    typeset,
                )
            });

        let text_width = after
            .as_ref()
            .map_or(physical.width, |(columns, _)| columns.width);
        let (size, inset) = match look {
            Look::Live => (geometry::pill_size(text_width, self.dpi), 0),
            Look::Pinned => (
                geometry::pinned_size(text_width, self.dpi),
                geometry::border(self.dpi).strip,
            ),
        };
        let placed = match measurement.content {
            Some(content) => Some(bounds::place_label(content, size, self.monitor, self.dpi)),
            None => geometry::place_label(measurement, size, self.monitor, self.dpi),
        };
        let Some(pill) = placed else {
            return Ok(None);
        };

        // The text sits so the middle of its digits is the middle of the pill, on a whole pixel: the layout's own
        // height counts room for letters that drop below the line, which a number never has.
        let font = px(scaled(geometry::TEXT, self.dpi));
        let window = self.in_window(&pill);
        let middle = (window.top + window.bottom) / 2.0;
        let baseline = whole((middle + self.fonts.cap_height * font / 2.0).round());
        let text_left = window.left + px(inset + scaled(geometry::PAD_X, self.dpi));
        let logical = after.map(|(columns, text)| {
            // The bar is placed in the virtual screen like the pill and then moved into the window. Its bottom side is
            // measured from the baseline, which is the same one for both parts.
            let left = pill.left + inset + scaled(geometry::PAD_X, self.dpi) + columns.bar;
            let bar = geometry::bar(left, baseline + self.monitor.top, self.dpi);
            PlacedLogical {
                bar: self.in_window(&bar),
                text: PlacedText {
                    left: text_left + px(columns.logical),
                    // Its own distance to the baseline: another face may have another ascent.
                    top: px(baseline) - text.baseline,
                    layout: text.layout,
                },
            }
        });
        Ok(Some(PlacedLabel {
            look,
            pill,
            physical: PlacedText {
                layout: physical.layout,
                left: text_left,
                top: px(baseline) - physical.baseline,
            },
            logical,
        }))
    }

    /// `text` laid out in `format` with tabular figures, and how wide it is and how far down its baseline is.
    ///
    /// Each 7 counts as wide as a 0. The labels' Segoe UI semibold draws its tabular 7 a third of a pixel narrower than
    /// its other digits (Chromium does the same), and rounded up to whole pixels that third could take a pixel off the
    /// box around the number as a 7 came in, the jump tabular figures are there to stop (WPC-386). The 7 is still drawn
    /// as the font draws it, from the real text's layout; only the room counted for it changes, so the text sits at the
    /// left of a room at most two thirds of a pixel wider than it. The other digits are left as they are: the tolerance
    /// indicator's Black face has no tabular figures, its 1 is narrower and its 4 wider than its 0 by design (see
    /// `tolerance::NUMBER_WEIGHT`), and its 7 is already a 0's width.
    fn typeset(&self, text: &str, format: &IDWriteTextFormat) -> windows::core::Result<Typeset> {
        let typeset = Typeset::of(self.layout(text, format)?)?;
        if !text.contains('7') {
            return Ok(typeset);
        }
        let room = Typeset::of(self.layout(&text.replace('7', "0"), format)?)?;
        Ok(Typeset {
            // Never narrower than the text itself, so the box never cuts into it.
            width: typeset.width.max(room.width),
            ..typeset
        })
    }

    /// As `typeset`, with DirectWrite's default features alone: for text with no digits to align.
    fn typeset_plain(
        &self,
        text: &str,
        format: &IDWriteTextFormat,
    ) -> windows::core::Result<Typeset> {
        Typeset::of(self.layout_as(text, format, false)?)
    }

    /// The pill's shadow, which the lines are drawn over. A pinned pill is a little see-through, and CSS never paints an
    /// outer shadow inside the box it belongs to, so there the shadow is cut out where the pill is; the live pill is
    /// opaque, so under it the shadow is not worth the trouble.
    fn draw_shadow(
        &self,
        target: &ID2D1RenderTarget,
        label: &PlacedLabel,
    ) -> windows::core::Result<()> {
        let drop = scaled(geometry::SHADOW_DROP, self.dpi);
        let pill = &label.pill;
        // SAFETY: a drawing call on a live render target. Out of BeginDraw and EndDraw, Direct2D only fails the frame
        // at EndDraw. The shapes are on whole pixels, so their straight sides are crisp; the corners are antialiased.
        unsafe { target.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE) };
        let cut_out = match label.look {
            Look::Live => None,
            Look::Pinned => Some(self.geometry_of(&self.rounded(pill, 0, 0))?),
        };
        for (grow, opacity) in SHADOW {
            // SAFETY: as above.
            unsafe { self.shade.SetOpacity(opacity) };
            let shape = self.rounded(pill, scaled(grow, self.dpi), drop);
            match &cut_out {
                None => {
                    // SAFETY: as above.
                    unsafe { target.FillRoundedRectangle(&shape, &self.shade) };
                }
                Some(hole) => self.fill_except(target, &shape, hole, &self.shade)?,
            }
        }
        Ok(())
    }

    /// The pill and the text on it: the physical part in full ink, then, when the label has one, the bar and the
    /// logical part fainter.
    fn draw_pill(&self, target: &ID2D1RenderTarget, label: &PlacedLabel) {
        let origin = |text: &PlacedText| dot(text.left, text.top).point;
        let ink = match label.look {
            Look::Live => {
                // SAFETY: drawing calls on a live render target with arguments that outlive them. Out of BeginDraw
                // and EndDraw, Direct2D only fails the frame at EndDraw.
                unsafe {
                    target.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
                    target.FillRoundedRectangle(&self.rounded(&label.pill, 0, 0), &self.cyan);
                }
                &self.ink
            }
            Look::Pinned => {
                self.draw_panel(target, &label.pill);
                &self.surfaces.ink
            }
        };
        // SAFETY: drawing calls on a live render target with arguments that outlive them, as for the background.
        unsafe {
            // Grayscale, not ClearType: color fringes would show on the cyan and on the see-through panel.
            target.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
            // The brushes keep the opacity of the last label's logical part, so the physical text sets it back.
            ink.SetOpacity(1.0);
            target.DrawTextLayout(
                origin(&label.physical),
                &label.physical.layout,
                ink,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
            );
        }
        if let Some(logical) = &label.logical {
            self.draw_logical(target, logical, ink);
        }
    }

    /// A label's logical part in `ink`: the bar at `BAR_OPACITY` and the logical text at `LOGICAL_OPACITY`. The brush keeps
    /// the last opacity, so whatever draws with it next sets its own first.
    fn draw_logical(
        &self,
        target: &ID2D1RenderTarget,
        logical: &PlacedLogical,
        ink: &ID2D1SolidColorBrush,
    ) {
        // SAFETY: drawing calls on a live render target with arguments that outlive them. Out of BeginDraw and EndDraw,
        // Direct2D only fails the frame at EndDraw.
        unsafe {
            // The bar is a fill of whole pixels, so it is exactly its width however the scale rounds; then the default
            // comes back.
            target.SetAntialiasMode(D2D1_ANTIALIAS_MODE_ALIASED);
            ink.SetOpacity(BAR_OPACITY);
            target.FillRectangle(&logical.bar, ink);
            target.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
            ink.SetOpacity(LOGICAL_OPACITY);
            target.DrawTextLayout(
                dot(logical.text.left, logical.text.top).point,
                &logical.text.layout,
                ink,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
            );
        }
    }

    /// A pinned label's background: the panel, a ring of the palette's border around it, and over the ring's left side
    /// a cyan strip. The strip is the pill's own rounded rectangle cut to its first columns, so its outer side follows
    /// the pill's corners.
    fn draw_panel(&self, target: &ID2D1RenderTarget, pill: &RECT) {
        let border = geometry::border(self.dpi);
        let outer = self.rounded(pill, 0, 0);
        let window = self.in_window(pill);
        let strip = D2D_RECT_F {
            right: window.left + px(border.strip),
            ..window
        };
        self.fill_panel(target, &outer, border.ring);
        // SAFETY: drawing calls on a live render target with arguments that outlive them. Out of BeginDraw and EndDraw,
        // Direct2D only fails the frame at EndDraw. The clip is popped right after it is pushed, with nothing between
        // that could return early, so the frame is never left with a clip pushed.
        unsafe {
            target.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
            // The strip's sides are on whole pixels, so an aliased clip loses nothing.
            target.PushAxisAlignedClip(&strip, D2D1_ANTIALIAS_MODE_ALIASED);
            target.FillRoundedRectangle(&outer, &self.cyan);
            target.PopAxisAlignedClip();
        }
    }

    /// The palette's panel filling `outer`, and a ring of the palette's border `ring_width` pixels wide inside its edge:
    /// the background of a pinned label and of the notice.
    fn fill_panel(&self, target: &ID2D1RenderTarget, outer: &D2D1_ROUNDED_RECT, ring_width: i32) {
        // SAFETY: drawing calls on a live render target with arguments that outlive them. Out of BeginDraw and EndDraw,
        // Direct2D only fails the frame at EndDraw.
        unsafe {
            target.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
            target.FillRoundedRectangle(outer, &self.surfaces.panel);
            target.DrawRoundedRectangle(
                &ring(outer, px(ring_width)),
                &self.surfaces.border,
                px(ring_width),
                None::<&ID2D1StrokeStyle>,
            );
        }
    }

    /// `shape` as a geometry, which can be cut out of another.
    fn geometry_of(
        &self,
        shape: &D2D1_ROUNDED_RECT,
    ) -> windows::core::Result<ID2D1RoundedRectangleGeometry> {
        relief::geometry_of(&self.factory, shape)
    }

    /// Fills `shape` in `brush` everywhere except where `hole` is.
    fn fill_except(
        &self,
        target: &ID2D1RenderTarget,
        shape: &D2D1_ROUNDED_RECT,
        hole: &ID2D1RoundedRectangleGeometry,
        brush: &ID2D1SolidColorBrush,
    ) -> windows::core::Result<()> {
        relief::fill_except(&self.factory, target, shape, hole, brush)
    }

    /// `label` laid out on one line in `format`, with tabular figures.
    fn layout(
        &self,
        label: &str,
        format: &IDWriteTextFormat,
    ) -> windows::core::Result<IDWriteTextLayout> {
        self.layout_as(label, format, true)
    }

    /// `label` laid out on one line in `format`, with tabular figures only if `tabular`. Either way it keeps the font's
    /// kerning, from DirectWrite's default features without them and from `figures` with them: the "To" of
    /// "Tolerance" kerns by a pixel at 11px, as it does in Chromium.
    fn layout_as(
        &self,
        label: &str,
        format: &IDWriteTextFormat,
        tabular: bool,
    ) -> windows::core::Result<IDWriteTextLayout> {
        let text: Vec<u16> = label.encode_utf16().collect();
        let length = u32::try_from(text.len())
            .map_err(|_| Error::new(E_FAIL, "the label is too long to lay out"))?;
        // SAFETY: the layout copies the text and keeps its own reference to the format. A width and height past any
        // label keep it on one line.
        let layout = unsafe {
            self.fonts
                .factory
                .CreateTextLayout(&text, format, 4096.0, 4096.0)
        }?;
        if tabular {
            // SAFETY: the range is the whole text.
            unsafe {
                layout.SetTypography(
                    &self.figures,
                    DWRITE_TEXT_RANGE {
                        startPosition: 0,
                        length,
                    },
                )
            }?;
        }
        Ok(layout)
    }

    /// The virtual screen's point at the window's own pixel (0, 0): its monitor's corner.
    fn window_origin(&self) -> Point {
        Point {
            x: self.monitor.left,
            y: self.monitor.top,
        }
    }

    /// `rect`, from the virtual screen to the window's own pixels.
    fn in_window(&self, rect: &RECT) -> D2D_RECT_F {
        in_surface(self.window_origin(), rect)
    }

    /// The pill's rounded rectangle, grown by `grow` pixels on every side and moved `down` pixels.
    fn rounded(&self, pill: &RECT, grow: i32, down: i32) -> D2D1_ROUNDED_RECT {
        self.shaped(pill, scaled(geometry::RADIUS, self.dpi), grow, down)
    }

    /// `rect` with corners of `radius` pixels, grown by `grow` pixels on every side (or smaller, should it be negative)
    /// and moved `down` pixels. The corners grow with it, as a CSS shadow's do.
    fn shaped(&self, rect: &RECT, radius: i32, grow: i32, down: i32) -> D2D1_ROUNDED_RECT {
        // None of these shapes is shrunk past half its height, so none needs the toolbar's clamp.
        rounded_at(self.window_origin(), rect, radius, grow, down, false)
    }
}

/// A measured width rounded up to whole pixels, so the pill never cuts into the text. Widths are a few hundred pixels
/// at most, far inside `i32`.
fn whole_up(width: f32) -> i32 {
    debug_assert!((0.0..1e6).contains(&width), "a label {width} pixels wide");
    width.ceil() as i32
}

/// A position that is already a whole number of pixels, as the integer geometry takes it. Positions in a window are
/// below a million pixels, far inside `i32`, so the conversion keeps every digit.
// TODO: place the baseline in whole pixels without going through a float, so no float comes back to `i32` here.
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
        D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_PIXEL_FORMAT,
    };
    use windows::Win32::Graphics::Direct2D::{
        D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_FEATURE_LEVEL_DEFAULT,
        D2D1_RENDER_TARGET_PROPERTIES, D2D1_RENDER_TARGET_TYPE_SOFTWARE,
        D2D1_RENDER_TARGET_USAGE_NONE, D2D1CreateFactory, ID2D1Factory,
    };
    use windows::Win32::Graphics::DirectWrite::{
        DWRITE_FONT_SIMULATIONS_NONE, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL,
    };
    use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
    use windows::Win32::Graphics::Imaging::{
        CLSID_WICImagingFactory, GUID_WICPixelFormat32bppPBGRA, IWICBitmap, IWICImagingFactory,
        WICBitmapCacheOnDemand,
    };
    use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};

    use super::geometry::Size;
    use super::*;
    use crate::com::TestCom;
    use crate::graphics::font_family;

    pub(super) const CYAN: [u8; 3] = [0x22, 0xd3, 0xee];
    pub(super) const BACKGROUND: [u8; 3] = [200, 120, 60];

    /// A bitmap a render target draws on in software, so no monitor or graphics card is needed.
    pub(super) struct Canvas {
        pub(super) target: ID2D1RenderTarget,
        bitmap: IWICBitmap,
        pub(super) fonts: Fonts,
        pub(super) width: u32,
        pub(super) height: u32,
        /// Last, so it drops last.
        _com: TestCom,
    }

    /// The canvas as rows of [r, g, b] pixels.
    pub(super) struct Picture {
        pub(super) width: usize,
        pub(super) pixels: Vec<[u8; 3]>,
    }

    impl Picture {
        pub(super) fn at(&self, x: usize, y: usize) -> [u8; 3] {
            self.pixels[y * self.width + x]
        }
    }

    impl Canvas {
        pub(super) fn new(width: u32, height: u32) -> Canvas {
            let com = TestCom::start();
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
            // Software, and 96 DPI like the windows' render targets: one unit is one pixel.
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
            Canvas {
                target,
                bitmap,
                fonts: Fonts::new().expect("the fonts"),
                width,
                height,
                _com: com,
            }
        }

        /// Fills the canvas with `BACKGROUND`, lets `draw` draw over it for a window of `monitor` and `dpi` in the
        /// dark palette, and reads the result back.
        pub(super) fn render(
            &self,
            monitor: Rect,
            dpi: u32,
            draw: impl FnOnce(&Overlay, &ID2D1RenderTarget),
        ) -> Picture {
            self.render_in(Mode::Dark, monitor, dpi, draw)
        }

        /// As `render`, in the palette of `mode`.
        pub(super) fn render_in(
            &self,
            mode: Mode,
            monitor: Rect,
            dpi: u32,
            draw: impl FnOnce(&Overlay, &ID2D1RenderTarget),
        ) -> Picture {
            let scale = Scale::from_dpi(dpi).expect("a scale");
            let overlay =
                Overlay::new(&self.target, &self.fonts, monitor, scale, mode).expect("the overlay");
            let [r, g, b] = BACKGROUND;
            let background = D2D1_COLOR_F {
                r: f32::from(r) / 255.0,
                g: f32::from(g) / 255.0,
                b: f32::from(b) / 255.0,
                a: 1.0,
            };
            // SAFETY: drawing calls between BeginDraw and EndDraw on a live target.
            unsafe {
                self.target.BeginDraw();
                self.target.Clear(Some(&background));
            }
            draw(&overlay, &self.target);
            // SAFETY: ends the BeginDraw above.
            unsafe { self.target.EndDraw(None, None) }.expect("the drawing is finished");
            self.read()
        }

        fn read(&self) -> Picture {
            let stride = self.width * 4;
            let mut bytes = vec![0; (stride * self.height) as usize];
            // SAFETY: no rectangle copies the whole bitmap, into a buffer of exactly its size.
            unsafe { self.bitmap.CopyPixels(ptr::null(), stride, &mut bytes) }
                .expect("the pixels are read");
            Picture {
                width: self.width as usize,
                // The bitmap is blue, green, red and alpha; every pixel here is opaque.
                pixels: bytes
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|&[b, g, r, _]| [r, g, b])
                    .collect(),
            }
        }
    }

    /// The window of the tests: 480 by 320 pixels, over a monitor to the left of and below the main one, so
    /// coordinates in the virtual screen and in the window are different numbers.
    pub(super) const MONITOR: Rect = Rect {
        left: -480,
        top: 200,
        width: 480,
        height: 320,
    };

    pub(super) fn point(x: i32, y: i32) -> screengauge_core::session::Point {
        screengauge_core::session::Point { x, y }
    }

    fn span(first: (i32, i32), last: (i32, i32)) -> screengauge_core::session::Span {
        screengauge_core::session::Span {
            first: point(first.0, first.1),
            last: point(last.0, last.1),
        }
    }

    /// A label in physical pixels alone, as at 100% or with "Also show logical pixels" off.
    fn physical_only(label: &str) -> screengauge_core::session::Label {
        screengauge_core::session::Label {
            physical: label.to_owned(),
            logical: None,
        }
    }

    pub(super) fn horizontal(x0: i32, x1: i32, y: i32, label: &str) -> Measurement {
        Measurement {
            monitor: 0,
            horizontal: Some(span((x0, y), (x1, y))),
            vertical: None,
            content: None,
            label: physical_only(label),
        }
    }

    pub(super) fn vertical(y0: i32, y1: i32, x: i32, label: &str) -> Measurement {
        Measurement {
            monitor: 0,
            horizontal: None,
            vertical: Some(span((x, y0), (x, y1))),
            content: None,
            label: physical_only(label),
        }
    }

    /// The Cross: a horizontal span along row `y` and a vertical one along column `x`.
    pub(super) fn cross(h: (i32, i32), v: (i32, i32), x: i32, y: i32, label: &str) -> Measurement {
        Measurement {
            monitor: 0,
            horizontal: Some(span((h.0, y), (h.1, y))),
            vertical: Some(span((x, v.0), (x, v.1))),
            content: None,
            label: physical_only(label),
        }
    }

    /// The model the owner checks by counting: what a pixel of the window must be, worked out from the span alone.
    /// A span along a row from `from` to `to` on row `row`, with ticks reaching `k`; the pixel is (`along`, `across`).
    fn on_line(from: i32, to: i32, row: i32, k: i32, along: i32, across: i32) -> bool {
        (across == row && (from..=to).contains(&along))
            || ((along == from || along == to) && (across - row).abs() <= k)
    }

    /// Beside the line along its whole length, or beside a tick along its whole length.
    fn beside_line(from: i32, to: i32, row: i32, k: i32, along: i32, across: i32) -> bool {
        ((across - row).abs() == 1 && (from..=to).contains(&along))
            || (((along - from).abs() == 1 || (along - to).abs() == 1) && (across - row).abs() <= k)
    }

    /// What a pixel is for a measurement's line: its cyan, its outline over the background, or the background.
    pub(super) fn expected(m: &Measurement, k: i32, x: i32, y: i32) -> [u8; 3] {
        let mut cyan = false;
        let mut outline = false;
        if let Some(h) = m.horizontal {
            cyan |= on_line(h.first.x, h.last.x, h.first.y, k, x, y);
            outline |= beside_line(h.first.x, h.last.x, h.first.y, k, x, y);
        }
        if let Some(v) = m.vertical {
            cyan |= on_line(v.first.y, v.last.y, v.first.x, k, y, x);
            outline |= beside_line(v.first.y, v.last.y, v.first.x, k, y, x);
        }
        if cyan {
            CYAN
        } else if outline {
            // 28% black over the background: once, never twice, where outlines meet.
            BACKGROUND.map(|c| (f32::from(c) * 0.72).round() as u8)
        } else {
            BACKGROUND
        }
    }

    pub(super) fn near(actual: [u8; 3], wanted: [u8; 3], tolerance: u8) -> bool {
        actual
            .iter()
            .zip(wanted)
            .all(|(&a, w)| a.abs_diff(w) <= tolerance)
    }

    /// The lines of `m` alone: its outline, then its cyan pixels.
    fn draw_lines(overlay: &Overlay, target: &ID2D1RenderTarget, m: &Measurement) {
        let parts = [geometry::parts(m, overlay.dpi)];
        overlay.draw_outlines(target, &parts);
        overlay.draw_cyan(target, &parts);
    }

    /// Checks every pixel of the window against the model, the line alone with no label.
    fn check_line(canvas: &Canvas, m: &Measurement, dpi: u32) {
        let k = scaled(4, dpi);
        let picture = canvas.render(MONITOR, dpi, |overlay, target| {
            draw_lines(overlay, target, m);
        });
        for y in 0..canvas.height as usize {
            for x in 0..canvas.width as usize {
                // The window's pixel is the virtual screen's pixel minus the monitor's corner.
                let (vx, vy) = (x as i32 + MONITOR.left, y as i32 + MONITOR.top);
                let wanted = expected(m, k, vx, vy);
                let tolerance = if wanted == CYAN || wanted == BACKGROUND {
                    0
                } else {
                    1
                };
                assert!(
                    near(picture.at(x, y), wanted, tolerance),
                    "pixel ({vx}, {vy}) at {dpi} DPI is {:?}, wanted {wanted:?}, for {m:?}",
                    picture.at(x, y)
                );
            }
        }
    }

    #[test]
    fn a_horizontal_line_is_its_pixels_in_cyan_with_the_outline_beside_them() {
        let canvas = Canvas::new(480, 320);
        for dpi in [96, 120, 168] {
            check_line(&canvas, &horizontal(-400, -301, 350, ""), dpi);
        }
    }

    #[test]
    fn a_vertical_line_is_its_pixels_in_cyan_with_the_outline_beside_them() {
        let canvas = Canvas::new(480, 320);
        for dpi in [96, 120, 168] {
            check_line(&canvas, &vertical(230, 480, -200, ""), dpi);
        }
    }

    #[test]
    fn a_cross_is_both_lines_without_darkening_where_they_meet() {
        let canvas = Canvas::new(480, 320);
        for dpi in [96, 120, 168] {
            check_line(
                &canvas,
                &cross((-350, -100), (230, 480), -230, 360, ""),
                dpi,
            );
        }
    }

    #[test]
    fn spans_of_one_two_and_three_pixels_keep_each_outline_pixel_single() {
        let canvas = Canvas::new(480, 320);
        for dpi in [96, 168] {
            for last in [-300, -299, -298] {
                check_line(&canvas, &horizontal(-300, last, 350, ""), dpi);
                check_line(&canvas, &vertical(300, last + 600, -300, ""), dpi);
            }
            check_line(
                &canvas,
                &cross((-300, -300), (350, 350), -300, 350, ""),
                dpi,
            );
        }
    }

    #[test]
    fn a_line_at_the_monitors_limit_is_cut_not_moved() {
        let canvas = Canvas::new(480, 320);
        // From the monitor's first column to its last, and down its first row.
        check_line(&canvas, &horizontal(-480, -1, 350, ""), 96);
        check_line(&canvas, &vertical(200, 519, -200, ""), 96);
        check_line(&canvas, &horizontal(-480, -1, 200, ""), 168);
    }

    #[test]
    fn the_junctions_are_darkened_once() {
        let canvas = Canvas::new(480, 320);
        let m = horizontal(-400, -301, 350, "");
        let picture = canvas.render(MONITOR, 96, |overlay, target| {
            draw_lines(overlay, target, &m)
        });
        // The window's pixel for (-400, 350) is (80, 150). The pixel diagonal to the span's first pixel is in the
        // outline of both the line and the tick.
        let once = BACKGROUND.map(|c| (f32::from(c) * 0.72).round() as u8);
        assert!(
            near(picture.at(81, 151), once, 1),
            "{:?}",
            picture.at(81, 151)
        );
        assert!(
            near(picture.at(79, 149), once, 1),
            "{:?}",
            picture.at(79, 149)
        );
        // The outline's own color is not twice 28%, which would be 52% of the background.
        assert!(picture.at(81, 151)[0] > 135);
    }

    #[test]
    fn a_tick_is_two_k_plus_one_pixels_long_at_every_scale() {
        let canvas = Canvas::new(480, 320);
        for (dpi, k) in [(96, 4), (120, 5), (168, 7)] {
            let m = horizontal(-400, -301, 350, "");
            let picture = canvas.render(MONITOR, dpi, |overlay, target| {
                draw_lines(overlay, target, &m)
            });
            // The first and last pixels' columns.
            for x in [80, 179] {
                let cyan = (0..320).filter(|&y| picture.at(x, y) == CYAN).count();
                assert_eq!(cyan, 2 * k + 1, "the tick at column {x}, {dpi} DPI");
            }
            // A column in the middle only has the line.
            assert_eq!((0..320).filter(|&y| picture.at(130, y) == CYAN).count(), 1);
        }
    }

    #[test]
    fn the_ticks_of_a_vertical_line_are_two_k_plus_one_pixels_wide() {
        let canvas = Canvas::new(480, 320);
        let m = vertical(230, 480, -200, "");
        let picture = canvas.render(MONITOR, 120, |overlay, target| {
            draw_lines(overlay, target, &m)
        });
        // Rows 30 and 280 of the window hold the first and last pixels.
        for y in [30, 280] {
            assert_eq!((0..480).filter(|&x| picture.at(x, y) == CYAN).count(), 11);
        }
        assert_eq!((0..480).filter(|&x| picture.at(x, 100) == CYAN).count(), 1);
    }

    #[test]
    fn the_line_is_one_pixel_thick_at_168_dpi_with_one_pixel_of_outline_each_side() {
        let canvas = Canvas::new(480, 320);
        let m = horizontal(-400, -301, 350, "");
        let picture = canvas.render(MONITOR, 168, |overlay, target| {
            draw_lines(overlay, target, &m)
        });
        // Column 130 of the window is in the middle of the span, and row 150 is its row.
        let column: Vec<[u8; 3]> = (0..320).map(|y| picture.at(130, y)).collect();
        assert_eq!(column.iter().filter(|&&p| p == CYAN).count(), 1);
        assert_eq!(column[150], CYAN);
        let shaded = BACKGROUND.map(|c| (f32::from(c) * 0.72).round() as u8);
        assert!(near(column[149], shaded, 1) && near(column[151], shaded, 1));
        assert_eq!(column[148], BACKGROUND, "two rows away is untouched");
        assert_eq!(column[152], BACKGROUND);
    }

    #[test]
    fn the_crossing_pixel_is_pure_cyan() {
        let canvas = Canvas::new(480, 320);
        for dpi in [96, 168] {
            let m = cross((-350, -100), (230, 480), -230, 360, "");
            let picture = canvas.render(MONITOR, dpi, |overlay, target| {
                draw_lines(overlay, target, &m)
            });
            // (-230, 360) in the window.
            assert_eq!(picture.at(250, 160), CYAN, "at {dpi} DPI");
        }
    }

    /// The label alone, drawn as the whole measurement draws it, and where its pill went.
    fn draw_label(overlay: &Overlay, target: &ID2D1RenderTarget, m: &Measurement) -> Option<RECT> {
        let label = overlay.label(m, Look::Live).expect("the label")?;
        overlay.draw_shadow(target, &label).expect("the shadow");
        overlay.draw_pill(target, &label);
        Some(label.pill)
    }

    /// The rows (top, bottom) and columns (left, right) of the text's pixels inside `pill`: those that are more than
    /// half the text's near-black, which neither the cyan, nor the photo showing at the rounded corners, nor the shadow
    /// is.
    fn ink(picture: &Picture, pill: &RECT) -> ((i32, i32), (i32, i32)) {
        let (mut top, mut bottom, mut left, mut right) = (i32::MAX, i32::MIN, i32::MAX, i32::MIN);
        for y in pill.top..pill.bottom {
            for x in pill.left..pill.right {
                let (wx, wy) = ((x - MONITOR.left) as usize, (y - MONITOR.top) as usize);
                let [r, g, _] = picture.at(wx, wy);
                if r < 100 && g < 140 {
                    (top, bottom) = (top.min(y), bottom.max(y));
                    (left, right) = (left.min(x), right.max(x));
                }
            }
        }
        ((top, bottom), (left, right))
    }

    #[test]
    fn the_pill_is_opaque_cyan_around_dark_centered_text() {
        let canvas = Canvas::new(480, 320);
        for (dpi, height) in [(96, 22), (120, 27), (168, 39)] {
            let m = horizontal(-400, -301, 350, "24");
            let mut pill = None;
            let picture = canvas.render(MONITOR, dpi, |overlay, target| {
                pill = draw_label(overlay, target, &m);
            });
            let pill = pill.expect("a pill");
            assert_eq!(pill.bottom - pill.top, height, "at {dpi} DPI");
            let at = |x: i32, y: i32| {
                picture.at((x - MONITOR.left) as usize, (y - MONITOR.top) as usize)
            };
            let middle = (pill.top + pill.bottom) / 2;
            // In the padding left of the text: opaque cyan, not blended with the photo.
            assert_eq!(at(pill.left + 3, middle), CYAN, "at {dpi} DPI");
            // The corner is rounded, so the pill's corner pixel is mostly the photo.
            assert_ne!(at(pill.left, pill.top), CYAN, "at {dpi} DPI");

            let ((top, bottom), (left, right)) = ink(&picture, &pill);
            assert!(top <= bottom, "the text has dark pixels at {dpi} DPI");
            // Digits as high as the room above and below them, to a pixel.
            let (above, below) = (top - pill.top, pill.bottom - 1 - bottom);
            assert!(
                (above - below).abs() <= 1,
                "{above} above, {below} below at {dpi} DPI"
            );
            // And about the padding to each side.
            let (before, after) = (left - pill.left, pill.right - 1 - right);
            assert!(
                (before - after).abs() <= 2,
                "{before} before, {after} after at {dpi} DPI"
            );
            assert!(
                before >= scaled(geometry::PAD_X, dpi) - 1,
                "{before} before at {dpi} DPI"
            );
        }
    }

    #[test]
    fn the_shadow_is_subtle_and_stays_near_the_pill() {
        let canvas = Canvas::new(480, 320);
        let m = horizontal(-400, -301, 350, "24");
        let mut pill = None;
        let picture = canvas.render(MONITOR, 96, |overlay, target| {
            pill = draw_label(overlay, target, &m);
        });
        let pill = pill.expect("a pill");
        let at =
            |x: i32, y: i32| picture.at((x - MONITOR.left) as usize, (y - MONITOR.top) as usize);
        let centre = (pill.left + pill.right) / 2;
        // Just below the pill it darkens the photo, but by far less than the outline's 28%.
        let below = at(centre, pill.bottom + 1);
        assert!(below[0] < BACKGROUND[0], "{below:?}");
        assert!(below[0] > 150, "{below:?}");
        // Eight pixels beyond the pill's bottom side, 2 for the drop and 6 for the blur, nothing is left.
        assert_eq!(at(centre, pill.bottom + 9), BACKGROUND);
        assert_eq!(at(pill.left - 7, (pill.top + pill.bottom) / 2), BACKGROUND);
    }

    #[test]
    fn the_cross_label_reads_width_by_height() {
        let canvas = Canvas::new(480, 320);
        let m = cross((-350, -100), (230, 480), -230, 360, "240 \u{d7} 180");
        let mut pill = None;
        canvas.render(MONITOR, 96, |overlay, target| {
            pill = draw_label(overlay, target, &m);
        });
        let pill = pill.expect("a pill");
        // Below and to the right of the crossing, and wide enough for nine characters.
        assert_eq!((pill.left, pill.top), (-230 + 12, 360 + 12));
        assert!(pill.right - pill.left > 60, "{pill:?}");
    }

    #[test]
    fn drawing_all_leaves_the_line_exact_beside_the_label() {
        let canvas = Canvas::new(480, 320);
        for dpi in [96, 168] {
            let m = horizontal(-400, -301, 350, "100");
            let k = scaled(4, dpi);
            let picture = canvas.render(MONITOR, dpi, |overlay, target| {
                overlay
                    .draw(target, &[], Some(&m), None)
                    .expect("the measurement");
            });
            for x in 80..180 {
                assert_eq!(picture.at(x, 150), CYAN, "column {x} at {dpi} DPI");
            }
            for y in 150 - k as usize..=150 + k as usize {
                assert_eq!(picture.at(80, y), CYAN, "tick row {y} at {dpi} DPI");
            }
            // The label is above the line, so the pixels below it are the photo.
            assert_eq!(picture.at(130, 150 + scaled(9, dpi) as usize), BACKGROUND);
        }
    }

    #[test]
    fn the_label_stays_in_the_window_at_the_monitors_limit() {
        let canvas = Canvas::new(480, 320);
        // A short span in the corner: the label would stick out on two sides without sliding in.
        let m = horizontal(-480, -478, 202, "3");
        let mut pill = None;
        canvas.render(MONITOR, 168, |overlay, target| {
            pill = draw_label(overlay, target, &m);
        });
        let pill = pill.expect("a pill");
        assert!(pill.left >= MONITOR.left && pill.top >= MONITOR.top);
        assert!(pill.right <= MONITOR.left + 480 && pill.bottom <= MONITOR.top + 320);
    }

    #[test]
    fn the_labels_digits_are_as_wide_as_each_other() {
        // Without tabular figures Segoe UI's 1 is 4.8 pixels wide at 100% and its 0 is 6.7, so the pill would jump as
        // the number changed. With them every digit but the 7 is as wide as the 0, and the 7 takes the 0's room
        // (WPC-386), so the label counts the same whole pixels for any digits, in the physical text's format and in the
        // logical text's. Four of each, as one digit alone rounds up to the same pixel however narrow its 7 is.
        let canvas = Canvas::new(8, 8);
        for dpi in SCALES {
            canvas.render(MONITOR, dpi, |overlay, _| {
                for (part, format) in [
                    ("physical", &overlay.format),
                    ("logical", &overlay.logical_format),
                ] {
                    let width = |text: &str| text_width(overlay, text, format);
                    let zeros = width("0000");
                    for digit in '1'..='9' {
                        let digits = digit.to_string().repeat(4);
                        assert_eq!(
                            width(&digits),
                            zeros,
                            "{digits} in the {part} format at {dpi} DPI"
                        );
                    }
                }
            });
        }
    }

    #[test]
    fn the_font_reports_a_capital_height_a_digit_can_use() {
        let fonts = Fonts::new().expect("the fonts");
        assert!(
            (0.6..0.8).contains(&fonts.cap_height),
            "{}",
            fonts.cap_height
        );
    }

    #[test]
    fn the_pill_grows_with_the_text() {
        // Three digits are wider than one, and the pill is as tall for both.
        let canvas = Canvas::new(480, 320);
        let size_of = |label: &str| -> Size {
            let m = horizontal(-400, -301, 350, label);
            let mut pill = None;
            canvas.render(MONITOR, 96, |overlay, target| {
                pill = draw_label(overlay, target, &m);
            });
            let pill = pill.expect("a pill");
            Size {
                width: pill.right - pill.left,
                height: pill.bottom - pill.top,
            }
        };
        let (one, three) = (size_of("1"), size_of("100"));
        assert_eq!(one.height, three.height);
        assert!(three.width > one.width + 10);
    }

    /// Every pixel the model says is cyan, in the picture of the whole measurement, line and label together.
    fn check_cyan_after_the_label(canvas: &Canvas, m: &Measurement, dpi: u32) {
        let k = scaled(4, dpi);
        let picture = canvas.render(MONITOR, dpi, |overlay, target| {
            overlay
                .draw(target, &[], Some(m), None)
                .expect("the measurement");
        });
        for y in 0..canvas.height as usize {
            for x in 0..canvas.width as usize {
                let (vx, vy) = (x as i32 + MONITOR.left, y as i32 + MONITOR.top);
                if expected(m, k, vx, vy) == CYAN {
                    assert_eq!(
                        picture.at(x, y),
                        CYAN,
                        "pixel ({vx}, {vy}) at {dpi} DPI for {m:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn the_label_and_its_shadow_never_darken_a_cyan_pixel() {
        // Short spans put the ticks under the label's shadow, which reaches 2 + 6 pixels below the pill.
        // The labels with a logical part are wider, so the pill and its shadow reach further along the line.
        let canvas = Canvas::new(480, 320);
        let labels = [
            (-400, "1", None),
            (-391, "10", None),
            (-377, "24", None),
            (-400, "1", Some("1.1")),
            (-377, "24", Some("13.7")),
            (-377, "240 \u{d7} 180", Some("137.1 \u{d7} 102.9")),
        ];
        for dpi in [96, 120, 144, 168] {
            for (last, label, logical) in labels {
                let with = |m: Measurement| match logical {
                    Some(logical) => with_logical(m, logical),
                    None => m,
                };
                let horizontal = with(horizontal(-400, last, 350, label));
                check_cyan_after_the_label(&canvas, &horizontal, dpi);
                let vertical = with(vertical(300, last + 700, -300, label));
                check_cyan_after_the_label(&canvas, &vertical, dpi);
                let cross = with(cross((-400, last), (300, last + 700), -400, 350, label));
                check_cyan_after_the_label(&canvas, &cross, dpi);
            }
        }
    }

    pub(super) const INK: [u8; 3] = [0x04, 0x16, 0x1b];

    pub(super) const SCALES: [u32; 4] = [96, 120, 144, 168];

    /// `m` with `logical` as the logical part of its label.
    pub(super) fn with_logical(mut m: Measurement, logical: &str) -> Measurement {
        m.label.logical = Some(logical.to_owned());
        m
    }

    /// The ink at `opacity` over the pill's cyan: what the bar and the logical text's solid pixels are. Direct2D
    /// blends each byte of the target, so rounding the same way lands within a level of it.
    fn ink_over_cyan(opacity: f32) -> [u8; 3] {
        std::array::from_fn(|i| {
            (f32::from(INK[i]) * opacity + f32::from(CYAN[i]) * (1.0 - opacity)).round() as u8
        })
    }

    /// The width of `text` laid out in `format`, in whole pixels, as the label counts it.
    pub(super) fn text_width(overlay: &Overlay, text: &str, format: &IDWriteTextFormat) -> i32 {
        overlay.typeset(text, format).expect("a text").width
    }

    /// One label drawn alone, with what the tests know about it from outside the drawing code: the widths of its two
    /// texts, laid out again on their own, and the scale.
    struct Drawn {
        picture: Picture,
        pill: RECT,
        dpi: u32,
        physical_width: i32,
        /// 0 for a label without a logical part.
        logical_width: i32,
    }

    impl Drawn {
        /// A pixel, by its place in the virtual screen.
        fn at(&self, x: i32, y: i32) -> [u8; 3] {
            self.picture
                .at((x - MONITOR.left) as usize, (y - MONITOR.top) as usize)
        }

        /// The physical text's first column: the pill's padding in.
        fn text_left(&self) -> i32 {
            self.pill.left + scaled(8, self.dpi)
        }

        /// The bar's first column, after the physical text and a gap.
        fn bar_left(&self) -> i32 {
            self.text_left() + self.physical_width + scaled(7, self.dpi)
        }

        /// The logical text's first column, after the bar and another gap.
        fn logical_left(&self) -> i32 {
            self.bar_left() + scaled(1, self.dpi) + scaled(7, self.dpi)
        }

        /// The pill's rows, over the columns `left` to `right`.
        fn rows(&self, left: i32, right: i32) -> RECT {
            RECT {
                left,
                top: self.pill.top,
                right,
                bottom: self.pill.bottom,
            }
        }

        /// The darkest green in the columns `left` to `right` of the pill's rows. The ink is the only dark green on
        /// the cyan, whose green is 211.
        fn darkest_green(&self, left: i32, right: i32) -> u8 {
            let area = self.rows(left, right);
            (area.top..area.bottom)
                .flat_map(|y| (area.left..area.right).map(move |x| (x, y)))
                .map(|(x, y)| self.at(x, y)[1])
                .min()
                .expect("columns")
        }

        /// The columns of the pill with a run of at least `rows` pixels near `color` from top to bottom. A glyph's
        /// partly covered pixels can have a bar's color, but no glyph is as tall as the bar.
        fn columns_with_run(&self, color: [u8; 3], rows: i32) -> Vec<i32> {
            (self.pill.left..self.pill.right)
                .filter(|&x| {
                    let mut run = 0;
                    (self.pill.top..self.pill.bottom).any(|y| {
                        run = if near(self.at(x, y), color, 2) {
                            run + 1
                        } else {
                            0
                        };
                        run >= rows
                    })
                })
                .collect()
        }
    }

    fn draw_alone(canvas: &Canvas, m: &Measurement, dpi: u32) -> Drawn {
        let mut found = None;
        let picture = canvas.render(MONITOR, dpi, |overlay, target| {
            let pill = draw_label(overlay, target, m).expect("a pill");
            let physical = text_width(overlay, &m.label.physical, &overlay.format);
            let logical = m
                .label
                .logical
                .as_deref()
                .map_or(0, |text| text_width(overlay, text, &overlay.logical_format));
            found = Some((pill, physical, logical));
        });
        let (pill, physical_width, logical_width) = found.expect("the label was drawn");
        Drawn {
            picture,
            pill,
            dpi,
            physical_width,
            logical_width,
        }
    }

    #[test]
    fn a_label_without_a_logical_part_is_its_text_in_its_pill_with_no_bar() {
        let canvas = Canvas::new(480, 320);
        for dpi in SCALES {
            let d = draw_alone(&canvas, &horizontal(-400, -301, 350, "24"), dpi);
            let size = geometry::pill_size(d.physical_width, dpi);
            assert_eq!(
                (d.pill.right - d.pill.left, d.pill.bottom - d.pill.top),
                (size.width, size.height),
                "at {dpi} DPI"
            );
            let bar = ink_over_cyan(0.4);
            assert_eq!(
                d.columns_with_run(bar, scaled(11, dpi)),
                Vec::<i32>::new(),
                "at {dpi} DPI"
            );
        }
    }

    #[test]
    fn a_logical_part_widens_the_pill_by_the_gaps_the_bar_and_its_text_and_not_its_height() {
        let canvas = Canvas::new(480, 320);
        for dpi in SCALES {
            let alone = draw_alone(&canvas, &horizontal(-400, -301, 350, "24"), dpi);
            let both = draw_alone(
                &canvas,
                &with_logical(horizontal(-400, -301, 350, "24"), "13.7"),
                dpi,
            );
            let height = |d: &Drawn| d.pill.bottom - d.pill.top;
            let width = |d: &Drawn| d.pill.right - d.pill.left;
            assert_eq!(height(&both), height(&alone), "at {dpi} DPI");
            assert!(both.logical_width > 10, "{}", both.logical_width);
            assert_eq!(
                width(&both) - width(&alone),
                scaled(7, dpi) + scaled(1, dpi) + scaled(7, dpi) + both.logical_width,
                "at {dpi} DPI"
            );
        }
    }

    #[test]
    fn the_bar_is_ink_at_40_percent_scaled_wide_and_tall_between_two_gaps_of_cyan() {
        let canvas = Canvas::new(480, 320);
        let bar = ink_over_cyan(0.4);
        for dpi in SCALES {
            let m = with_logical(horizontal(-400, -301, 350, "24"), "13.7");
            let d = draw_alone(&canvas, &m, dpi);
            let (gap, width, height) = (scaled(7, dpi), scaled(1, dpi), scaled(11, dpi));

            // Where the columns of the bar are, found from the pixels: after the physical text and a gap.
            let left = d.bar_left();
            assert_eq!(
                d.columns_with_run(bar, height),
                (left..left + width).collect::<Vec<_>>(),
                "the bar's columns at {dpi} DPI"
            );

            // The digits stand on the baseline, so the first row under them is it, and the bar's bottom side hangs a
            // pixel (scaled) below it. The rows of the digits are read from the physical text's columns alone.
            let ((_, last_digit_row), _) = ink(
                &d.picture,
                &d.rows(d.text_left(), d.text_left() + d.physical_width),
            );
            let bottom = last_digit_row + 1 + scaled(1, dpi);
            let bar_rows = bottom - height..bottom;
            for x in left - gap..left + width + gap {
                let in_bar = (left..left + width).contains(&x);
                for y in d.pill.top..d.pill.bottom {
                    if in_bar && bar_rows.contains(&y) {
                        assert!(
                            near(d.at(x, y), bar, 2),
                            "bar pixel ({x}, {y}) is {:?}, wanted {bar:?}, at {dpi} DPI",
                            d.at(x, y)
                        );
                    } else {
                        assert_eq!(
                            d.at(x, y),
                            CYAN,
                            "({x}, {y}) is next to the bar at {dpi} DPI, so it is cyan"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn the_logical_text_is_fainter_than_the_physical_text() {
        let canvas = Canvas::new(480, 320);
        // The ink's solid pixels at 62% over the cyan have this green; a glyph's edge pixels only come lighter.
        let faint = i32::from(ink_over_cyan(0.62)[1]);
        for dpi in SCALES {
            let m = with_logical(horizontal(-400, -301, 350, "24"), "13.7");
            let d = draw_alone(&canvas, &m, dpi);
            let physical =
                i32::from(d.darkest_green(d.text_left(), d.text_left() + d.physical_width));
            let logical =
                i32::from(d.darkest_green(d.logical_left(), d.logical_left() + d.logical_width));
            assert!(
                (i32::from(INK[1])..=i32::from(INK[1]) + 2).contains(&physical),
                "the physical text is full ink: green {physical} at {dpi} DPI"
            );
            // Some pixel of the logical text is solid, and a solid pixel is 62% of the ink, no more and no less.
            assert!(
                logical.abs_diff(faint) <= 2,
                "the logical text's darkest green is {logical}, 62% of the ink is {faint}, at {dpi} DPI"
            );
            assert!(
                logical > physical + 50,
                "{logical} against {physical} at {dpi} DPI"
            );
        }
    }

    #[test]
    fn the_texts_start_a_padding_in_and_a_gap_a_bar_and_a_gap_apart() {
        // A glyph's side bearings keep pixels from showing a shift of one pixel in where a text starts, so this reads
        // where the label put each text. The expected places are worked out here from the scale alone.
        let canvas = Canvas::new(480, 320);
        for dpi in SCALES {
            let m = with_logical(horizontal(-400, -301, 350, "24"), "13.7");
            let mut found = None;
            canvas.render(MONITOR, dpi, |overlay, _| {
                let label = overlay
                    .label(&m, Look::Live)
                    .expect("the label")
                    .expect("a pill");
                let physical_width = text_width(overlay, "24", &overlay.format);
                found = Some((label, physical_width));
            });
            let (label, w) = found.expect("the label was made");
            let logical = label.logical.expect("a logical part");
            let physical_left = px(label.pill.left - MONITOR.left + scaled(8, dpi));
            assert_eq!(label.physical.left, physical_left, "at {dpi} DPI");
            let bar_left = physical_left + px(w + scaled(7, dpi));
            assert_eq!(logical.bar.left, bar_left, "at {dpi} DPI");
            assert_eq!(logical.bar.right, bar_left + px(scaled(1, dpi)));
            assert_eq!(
                logical.text.left,
                bar_left + px(scaled(1, dpi) + scaled(7, dpi)),
                "at {dpi} DPI"
            );
        }
    }

    #[test]
    fn the_logical_text_sits_on_the_physical_texts_baseline() {
        let canvas = Canvas::new(480, 320);
        for dpi in SCALES {
            // Both parts say 24, whose 2 stands on a flat base.
            let m = with_logical(horizontal(-400, -301, 350, "24"), "24");
            let d = draw_alone(&canvas, &m, dpi);
            let last_row = |left: i32, right: i32| {
                (d.pill.top..d.pill.bottom)
                    .rev()
                    .find(|&y| (left..right).any(|x| d.at(x, y)[1] < 125))
                    .expect("a dark row")
            };
            assert_eq!(
                last_row(d.logical_left(), d.logical_left() + d.logical_width),
                last_row(d.text_left(), d.text_left() + d.physical_width),
                "at {dpi} DPI"
            );
        }
    }

    #[test]
    fn a_label_drawn_after_one_with_a_logical_part_still_has_full_ink() {
        // The ink brush keeps the opacity of the last thing drawn with it, and the overlay is used for every frame.
        let canvas = Canvas::new(480, 320);
        for dpi in [96, 168] {
            let first = with_logical(horizontal(-400, -301, 350, "24"), "13.7");
            let second = horizontal(-400, -301, 250, "24");
            let mut pills = Vec::new();
            let picture = canvas.render(MONITOR, dpi, |overlay, target| {
                pills.extend(draw_label(overlay, target, &first));
                pills.extend(draw_label(overlay, target, &second));
            });
            let [_, pill] = pills[..] else {
                panic!("two pills, got {pills:?}");
            };
            let darkest = (pill.top..pill.bottom)
                .flat_map(|y| (pill.left + 8..pill.right - 8).map(move |x| (x, y)))
                .map(|(x, y)| {
                    picture.at((x - MONITOR.left) as usize, (y - MONITOR.top) as usize)[1]
                })
                .min()
                .expect("pixels");
            assert!(
                darkest <= INK[1] + 8,
                "the second label's darkest green is {darkest} at {dpi} DPI"
            );
        }
    }

    #[test]
    fn a_long_logical_part_keeps_the_label_inside_the_window_at_the_monitors_limit() {
        let canvas = Canvas::new(480, 320);
        // A short span in the top left corner, as in the test of the plain label, then the Cross in the bottom right
        // corner, whose label mirrors up and to the left.
        let corner = with_logical(horizontal(-480, -478, 202, "3"), "1234567.8");
        let cross = with_logical(
            cross((-120, -1), (400, 519), -1, 519, "240 \u{d7} 180"),
            "137.1 \u{d7} 102.9",
        );
        for dpi in SCALES {
            for m in [&corner, &cross] {
                let d = draw_alone(&canvas, m, dpi);
                let pill = d.pill;
                assert!(
                    pill.left >= MONITOR.left && pill.top >= MONITOR.top,
                    "{pill:?}"
                );
                assert!(
                    pill.right <= MONITOR.left + 480 && pill.bottom <= MONITOR.top + 320,
                    "{pill:?} at {dpi} DPI for {m:?}"
                );
                // The logical text ends inside the pill's padding: the pill was made for it.
                assert!(
                    d.logical_left() + d.logical_width <= pill.right - scaled(8, dpi),
                    "{pill:?} at {dpi} DPI for {m:?}"
                );
                let (_, (_, right)) = ink(&d.picture, &pill);
                assert!(
                    right < pill.right - scaled(8, dpi) + 2,
                    "{right} in {pill:?}"
                );
            }
        }
    }

    #[test]
    fn the_logical_part_is_in_the_same_semibold_the_owner_approved() {
        // Segoe UI has no face at weight 500, and DirectWrite takes its semibold for it, with no simulated bolding: the
        // label the owner approved on screen. Should Windows ever bring a face at 500, the logical part would change
        // weight from what was approved: this fails then, so it is looked at again.
        let fonts = Fonts::new().expect("the fonts");
        let family = font_family(&fonts.factory).expect("the family");
        let face = |weight| {
            // SAFETY: plain values.
            unsafe {
                family.GetFirstMatchingFont(
                    weight,
                    DWRITE_FONT_STRETCH_NORMAL,
                    DWRITE_FONT_STYLE_NORMAL,
                )
            }
            .expect("a face")
        };
        let (logical, physical) = (face(LOGICAL_WEIGHT), face(DWRITE_FONT_WEIGHT_SEMI_BOLD));
        // SAFETY: plain getters.
        let (logical_weight, physical_weight, simulations) = unsafe {
            (
                logical.GetWeight().0,
                physical.GetWeight().0,
                logical.GetSimulations(),
            )
        };
        assert_eq!(logical_weight, physical_weight);
        assert_eq!(simulations, DWRITE_FONT_SIMULATIONS_NONE);
    }
}
