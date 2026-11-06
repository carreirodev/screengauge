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
#[cfg(test)]
mod label_tests;
#[cfg(test)]
mod line_tests;
mod magnified_view;
#[cfg(test)]
mod magnified_view_readout_tests;
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
#[cfg(test)]
mod tests;
mod tolerance;
#[cfg(test)]
mod tolerance_tests;
pub mod toolbar;
mod toolbar_cache;
mod toolbar_icons;
mod toolbar_paint;
#[cfg(test)]
mod toolbar_reaction_tests;
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
    DWRITE_FONT_WEIGHT, DWRITE_FONT_WEIGHT_MEDIUM, DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_TEXT_RANGE,
    IDWriteTextFormat, IDWriteTextLayout, IDWriteTypography,
};
use windows::core::Error;

use crate::graphics::{Color, Fonts, color, solid_brush};
use crate::log;
use crate::relief::{self, dot, in_surface, px, ring, rounded_at};
use crate::theme::Mode;
use crate::typeset::{PlacedText, Typeset};
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
    /// The monitor's scale: everything is laid out at its DPI (`dpi`), and the toolbar's parts at the scale itself,
    /// wherever the scene puts it.
    scale: Scale,
    /// The window's monitor in the virtual screen, which is where the window's pixel (0, 0) is.
    monitor: RECT,
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

/// What a label draws after its physical text when it has a logical part.
struct PlacedLogical {
    /// The bar between the two parts, in the window's own pixels.
    bar: D2D_RECT_F,
    text: PlacedText,
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
                magnified_view::MAGNIFICATION_INDICATOR_WEIGHT,
                magnified_view::MAGNIFICATION_INDICATOR_TEXT,
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
        })
    }

    /// The DPI of the monitor's scale, which every size here is scaled to.
    fn dpi(&self) -> u32 {
        self.scale.dpi()
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
            parts.push(match measurement.content() {
                Some(content) => bounds::parts(content),
                None => geometry::parts(measurement, self.dpi()),
            });
            fills.extend(measurement.content().map(|content| (content, look)));
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
                    geometry::columns(physical.width, typeset.width, self.dpi()),
                    typeset,
                )
            });

        let text_width = after
            .as_ref()
            .map_or(physical.width, |(columns, _)| columns.width);
        let (size, inset) = match look {
            Look::Live => (geometry::pill_size(text_width, self.dpi()), 0),
            Look::Pinned => (
                geometry::pinned_size(text_width, self.dpi()),
                geometry::border(self.dpi()).strip,
            ),
        };
        let placed = match measurement.content() {
            Some(content) => Some(bounds::place_label(content, size, self.monitor, self.dpi())),
            None => geometry::place_label(measurement, size, self.monitor, self.dpi()),
        };
        let Some(pill) = placed else {
            return Ok(None);
        };

        // The text sits so the middle of its digits is the middle of the pill, on a whole pixel: the layout's own
        // height counts room for letters that drop below the line, which a number never has.
        let font = px(scaled(geometry::TEXT, self.dpi()));
        let window = self.in_window(&pill);
        let middle = (window.top + window.bottom) / 2.0;
        let baseline = whole((middle + self.fonts.cap_height * font / 2.0).round());
        let text_left = window.left + px(inset + scaled(geometry::PAD_X, self.dpi()));
        let logical = after.map(|(columns, text)| {
            // The bar is placed in the virtual screen like the pill and then moved into the window. Its bottom side is
            // measured from the baseline, which is the same one for both parts.
            let left = pill.left + inset + scaled(geometry::PAD_X, self.dpi()) + columns.bar;
            let bar = geometry::bar(left, baseline + self.monitor.top, self.dpi());
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
        let drop = scaled(geometry::SHADOW_DROP, self.dpi());
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
            let shape = self.rounded(pill, scaled(grow, self.dpi()), drop);
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
        let border = geometry::border(self.dpi());
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
        self.shaped(pill, scaled(geometry::RADIUS, self.dpi()), grow, down)
    }

    /// `rect` with corners of `radius` pixels, grown by `grow` pixels on every side (or smaller, should it be negative)
    /// and moved `down` pixels. The corners grow with it, as a CSS shadow's do.
    fn shaped(&self, rect: &RECT, radius: i32, grow: i32, down: i32) -> D2D1_ROUNDED_RECT {
        // None of these shapes is shrunk past half its height, so none needs the toolbar's clamp.
        rounded_at(self.window_origin(), rect, radius, grow, down, false)
    }
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
