//! Inspect's magnified view (v1 spec WPC-296, Inspect; the prototype's variant A, WPC-279): the floating panel, the
//! frozen screen's pixels enlarged without smoothing, the grid, the outline around the pixel under the cursor, the
//! magnification indicator and the readout with the cursor's position, the color of the pixel under it and, on a
//! monitor whose photo didn't come through the exact path, the Approximate color notice. Every rectangle comes from the
//! session core's [`MagnifiedView`]; this only draws them onto the overlay's render target.
//!
//! The prototype's `drawMag` and its CSS (`.panel`, `.magframe`, `.magtag`, `.info`, `.sw`, `.hdr`) are the
//! model. Where the core hands over a rectangle, it is used as it is; the rest of the sizes below are the prototype's,
//! in logical pixels, which `scaled` turns into physical pixels for the monitor's scale, each one on its own.

use screengauge_core::session::{
    ApproximateColor, ColorText, Coordinate, MagnifiedView, NoticeLine, PixelColor, Rect, Sign,
};
use windows::Win32::Foundation::{E_FAIL, RECT};
use windows::Win32::Graphics::Direct2D::{
    D2D1_ANTIALIAS_MODE_ALIASED, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
    D2D1_BITMAP_INTERPOLATION_MODE_NEAREST_NEIGHBOR, D2D1_DRAW_TEXT_OPTIONS_NONE,
    D2D1_ROUNDED_RECT, D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE, ID2D1Bitmap, ID2D1Geometry,
    ID2D1RenderTarget, ID2D1StrokeStyle,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FONT_WEIGHT, DWRITE_FONT_WEIGHT_BOLD, IDWriteTextFormat,
};
use windows::core::{Error, Interface};

use super::geometry::{self, scaled};
use super::{LOGICAL_OPACITY, Overlay, PlacedLogical, dot, px, ring, rounded_at};
use crate::graphics::d2d_color;
use crate::relief;
use crate::typeset::{PlacedText, Typeset};

// Every size below is in logical pixels, which `scaled` turns into physical pixels for the monitor's scale.

/// The panel's corner radius, and the enlargement's (CSS `.magframe { border-radius: 9px; overflow: hidden }`).
const PANEL_RADIUS: u32 = 14;
const ENLARGEMENT_RADIUS: u32 = 9;

/// How much of the grid's black shows over the pixels (CSS `rgba(0,0,0,.2)`).
const GRID_OPACITY: f32 = 0.2;

/// The magnification indicator: `.magtag`, `left: 6px; top: 6px`, a padding of 4px above and below and 6px at the
/// sides, corners of 6px, a background of black at .6, and its text in white, 700 weight and 11px with a line as tall
/// as itself.
const MAGNIFICATION_INDICATOR_INSET: u32 = 6;
const MAGNIFICATION_INDICATOR_PAD_X: u32 = 6;
const MAGNIFICATION_INDICATOR_PAD_Y: u32 = 4;
const MAGNIFICATION_INDICATOR_RADIUS: u32 = 6;
const MAGNIFICATION_INDICATOR_FILL_OPACITY: f32 = 0.6;
pub const MAGNIFICATION_INDICATOR_TEXT: u32 = 11;
pub const MAGNIFICATION_INDICATOR_WEIGHT: DWRITE_FONT_WEIGHT = DWRITE_FONT_WEIGHT_BOLD;

/// The readout's rows (`.info .row`): the key `X` or `Y` in a box at least this wide, then a gap, then the physical
/// number. A logical part follows after a gap and a margin, the label's bar (`geometry::BAR_WIDTH` wide and
/// `geometry::BAR_HEIGHT` tall), a margin and a gap.
const KEY_WIDTH: u32 = 14;
const GAP: u32 = 8;
const BAR_MARGIN: u32 = 7;

/// The swatch (`.sw`): corners of 6px, and a ring of 1px inside its side in `--panel-border`, over the swatch's own
/// color.
const SWATCH_RADIUS: u32 = 6;
const SWATCH_RING: u32 = 1;

/// The Approximate color notice's text (the prototype's `.hdr`): semibold, 11px, in `--warn`, as its sign is.
pub const APPROXIMATE_TEXT: u32 = 11;

/// Where the enlargement's parts go, in physical pixels of the virtual screen, right and bottom one past the last
/// pixel.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Places {
    /// The magnification: how many physical pixels a side each enlarged pixel takes.
    factor: i32,
    /// The whole enlargement, where the blank color goes.
    enlargement: RECT,
    /// The part of the enlargement the monitor's pixels fill: each of the photo's pixels in `shown` is a square of the
    /// magnification's side here.
    drawn: RECT,
    /// The photo's pixels that are enlarged: the view's `shown`.
    shown: RECT,
    /// The enlarged pixel under the cursor.
    cell: RECT,
}

/// Where the parts of `view`'s enlargement go. Fails for a view so large that its pixels don't fit a coordinate.
fn places(view: &MagnifiedView) -> windows::core::Result<Places> {
    let too_large = || Error::new(E_FAIL, "the magnified view is too large to draw");
    let whole = |value: i64| i32::try_from(value).map_err(|_| too_large());
    let factor = whole(i64::from(view.magnification.factor()))?;
    let enlargement = geometry::bounds(view.enlargement)?;
    // Where the enlarged pixel of the frozen screen's pixel at `at` starts along an axis: its distance from the
    // source's first pixel, enlarged, from the enlargement's first pixel.
    let start = |enlargement: i32, source: i32, at: i32| {
        whole(i64::from(enlargement) + (i64::from(at) - i64::from(source)) * i64::from(factor))
    };
    let (source, shown) = (view.source, view.shown);
    let left = start(enlargement.left, source.left, shown.left)?;
    let top = start(enlargement.top, source.top, shown.top)?;
    let drawn = RECT {
        left,
        top,
        right: whole(i64::from(left) + i64::from(shown.width) * i64::from(factor))?,
        bottom: whole(i64::from(top) + i64::from(shown.height) * i64::from(factor))?,
    };
    let left = start(enlargement.left, source.left, view.center.x)?;
    let top = start(enlargement.top, source.top, view.center.y)?;
    let cell = RECT {
        left,
        top,
        right: whole(i64::from(left) + i64::from(factor))?,
        bottom: whole(i64::from(top) + i64::from(factor))?,
    };
    Ok(Places {
        factor,
        enlargement,
        drawn,
        shown: geometry::bounds(shown)?,
        cell,
    })
}

/// The four sides of the one-pixel ring `distance` pixels outside `cell`: the pixels of `cell` grown by `distance` that
/// are not in `cell` grown by one less. The sides don't overlap.
fn ring_around(cell: &RECT, distance: i32) -> [RECT; 4] {
    let (left, top) = (cell.left - distance, cell.top - distance);
    let (right, bottom) = (cell.right + distance, cell.bottom + distance);
    [
        RECT {
            left,
            top,
            right,
            bottom: top + 1,
        },
        RECT {
            left,
            top: bottom - 1,
            right,
            bottom,
        },
        RECT {
            left,
            top: top + 1,
            right: left + 1,
            bottom: bottom - 1,
        },
        RECT {
            left: right - 1,
            top: top + 1,
            right,
            bottom: bottom - 1,
        },
    ]
}

/// Where the parts of a coordinate's line start, counted from the line's left side, for a key `key` pixels wide and a
/// physical number `physical` pixels wide. Every size is scaled on its own, as a browser lays each one out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LineColumns {
    physical: i32,
    bar: i32,
    logical: i32,
}

fn line_columns(key: i32, physical: i32, dpi: u32) -> LineColumns {
    let physical_at = key.max(scaled(KEY_WIDTH, dpi)) + scaled(GAP, dpi);
    let bar = physical_at + physical + scaled(GAP, dpi) + scaled(BAR_MARGIN, dpi);
    LineColumns {
        physical: physical_at,
        bar,
        logical: bar
            + scaled(geometry::BAR_WIDTH, dpi)
            + scaled(BAR_MARGIN, dpi)
            + scaled(GAP, dpi),
    }
}

/// The magnification indicator, ready to draw.
struct PlacedIndicator {
    background: D2D1_ROUNDED_RECT,
    text: PlacedText,
}

/// One of the readout's lines, ready to draw: the key, the physical number, and the bar and the logical number when the
/// coordinate has them.
struct PlacedLine {
    key: PlacedText,
    physical: PlacedText,
    logical: Option<PlacedLogical>,
}

/// The Approximate color notice, ready to draw: where its sign's top left corner goes in the window, and its two lines.
struct PlacedApproximate {
    sign: (f32, f32),
    lines: [PlacedText; 2],
}

/// The color's part of the readout, ready to draw: the swatch and its ring, and the texts beside it.
struct PlacedColor {
    swatch: D2D1_ROUNDED_RECT,
    ring: i32,
    rgb: [u8; 3],
    first: PlacedText,
    second: Option<PlacedText>,
}

impl Overlay {
    /// Draws `view` over the measurements and under the toolbar, between BeginDraw and EndDraw, enlarging the pixels of
    /// `photo`, the window's monitor's photo with its pixel (0, 0) at the monitor's top left corner.
    ///
    /// Everything that can fail is done before the first thing is drawn into the enlargement: a clip is pushed and
    /// popped with nothing between that could return early, so the enlargement is never left with a clip pushed.
    pub fn draw_magnified_view(
        &self,
        target: &ID2D1RenderTarget,
        view: &MagnifiedView,
        photo: &ID2D1Bitmap,
    ) -> windows::core::Result<()> {
        let places = places(view)?;
        let magnification_indicator = if view.magnification_indicator {
            Some(self.magnification_indicator(&places.enlargement, places.factor)?)
        } else {
            None
        };
        let lines = [self.line("X", &view.x)?, self.line("Y", &view.y)?];
        let color = self.color(&view.color)?;
        let approximate = view
            .approximate_color
            .as_ref()
            .map(|notice| self.approximate(notice))
            .transpose()?;
        let radius = scaled(ENLARGEMENT_RADIUS, self.dpi());
        let mask: ID2D1Geometry = self
            .geometry_of(&self.shaped(&places.enlargement, radius, 0, 0))?
            .cast()?;

        self.draw_floating_panel(target, &geometry::bounds(view.panel)?, PANEL_RADIUS)?;

        let enlargement = self.in_window(&places.enlargement);
        // The enlargement and the magnification indicator are clipped to the enlargement's rounded corners.
        relief::in_layer(target, enlargement, Some(mask), || {
            self.draw_enlargement(target, photo, &places, view);
            if let Some(magnification_indicator) = &magnification_indicator {
                self.draw_magnification_indicator(target, magnification_indicator);
            }
        });

        self.draw_readout(target, &lines);
        self.draw_color(target, &color);
        if let Some(approximate) = &approximate {
            self.draw_approximate(target, approximate)?;
        }
        Ok(())
    }

    /// What the enlargement holds, clipped to its rounded corners by the layer the caller pushed: the blank color, the
    /// monitor's pixels enlarged, the grid and the outline of the pixel under the cursor.
    fn draw_enlargement(
        &self,
        target: &ID2D1RenderTarget,
        photo: &ID2D1Bitmap,
        places: &Places,
        view: &MagnifiedView,
    ) {
        let drawn = self.in_window(&places.drawn);
        let shown = self.in_window(&places.shown);
        // SAFETY: drawing calls on a live render target with arguments that outlive them. Out of BeginDraw and EndDraw,
        // Direct2D only fails the frame at EndDraw. The photo is at 96 DPI, like the target, and both rectangles are on
        // whole pixels, the destination a whole multiple of the source's size, so nearest neighbor makes each source
        // pixel a square of exactly one color.
        unsafe {
            // Aliased, so the blank color covers exactly the enlargement's pixels: the layer rounds the corners.
            target.SetAntialiasMode(D2D1_ANTIALIAS_MODE_ALIASED);
            target.FillRectangle(&self.in_window(&places.enlargement), &self.surfaces.blank);
            target.DrawBitmap(
                photo,
                Some(&drawn),
                1.0,
                D2D1_BITMAP_INTERPOLATION_MODE_NEAREST_NEIGHBOR,
                Some(&shown),
            );
        }
        if view.grid {
            self.draw_grid(target, &places.drawn, places.factor);
        }
        self.draw_center_outline(target, &places.cell);
        // SAFETY: as above. Back to Direct2D's default, which the magnification indicator's corners need.
        unsafe { target.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE) };
    }

    /// The grid over `drawn`, the part of the enlargement that holds the monitor's pixels: a line one pixel wide in
    /// black at 20% on the first column of each enlarged pixel but the first, and the same along the rows. Where a
    /// vertical line and a horizontal one cross, the pixel is filled twice, as the prototype's canvas does.
    fn draw_grid(&self, target: &ID2D1RenderTarget, drawn: &RECT, step: i32) {
        // SAFETY: drawing calls on a live render target with arguments that outlive them. Out of BeginDraw and EndDraw,
        // Direct2D only fails the frame at EndDraw. Aliased fills of rectangles on whole pixels cover exactly those
        // pixels, and the target is at 96 DPI, so one unit is one physical pixel. The brush keeps this opacity, so
        // whatever draws with it next sets its own first.
        unsafe {
            target.SetAntialiasMode(D2D1_ANTIALIAS_MODE_ALIASED);
            self.shade.SetOpacity(GRID_OPACITY);
            let mut x = drawn.left.saturating_add(step);
            while x < drawn.right {
                let line = RECT {
                    left: x,
                    top: drawn.top,
                    right: x + 1,
                    bottom: drawn.bottom,
                };
                target.FillRectangle(&self.in_window(&line), &self.shade);
                x = x.saturating_add(step);
            }
            let mut y = drawn.top.saturating_add(step);
            while y < drawn.bottom {
                let line = RECT {
                    left: drawn.left,
                    top: y,
                    right: drawn.right,
                    bottom: y + 1,
                };
                target.FillRectangle(&self.in_window(&line), &self.shade);
                y = y.saturating_add(step);
            }
        }
    }

    /// The outline of the pixel under the cursor: a ring of opaque black pixels just outside `cell`, and a ring of
    /// opaque white ones just outside that (the prototype's two `strokeRect`s). One physical pixel thick whatever the
    /// scale, for it is the screen's pixels that are being told apart.
    fn draw_center_outline(&self, target: &ID2D1RenderTarget, cell: &RECT) {
        // SAFETY: drawing calls on a live render target with arguments that outlive them. Out of BeginDraw and EndDraw,
        // Direct2D only fails the frame at EndDraw. Aliased fills of whole pixels, as in `draw_grid`. Each brush's
        // opacity is set before it is used, because the brushes keep the last one.
        unsafe {
            target.SetAntialiasMode(D2D1_ANTIALIAS_MODE_ALIASED);
            self.shade.SetOpacity(1.0);
            for side in ring_around(cell, 1) {
                target.FillRectangle(&self.in_window(&side), &self.shade);
            }
            self.white.SetOpacity(1.0);
            for side in ring_around(cell, 2) {
                target.FillRectangle(&self.in_window(&side), &self.white);
            }
        }
    }

    /// The magnification indicator for `factor`, laid out with its top left corner `MAGNIFICATION_INDICATOR_INSET`
    /// inside `enlargement`'s.
    fn magnification_indicator(
        &self,
        enlargement: &RECT,
        factor: i32,
    ) -> windows::core::Result<PlacedIndicator> {
        let text = self.typeset_plain(
            &format!("{factor}\u{d7}"),
            &self.magnification_indicator_format,
        )?;
        let (pad_x, pad_y) = (
            scaled(MAGNIFICATION_INDICATOR_PAD_X, self.dpi()),
            scaled(MAGNIFICATION_INDICATOR_PAD_Y, self.dpi()),
        );
        let line = scaled(MAGNIFICATION_INDICATOR_TEXT, self.dpi());
        let corner = RECT {
            left: enlargement.left + scaled(MAGNIFICATION_INDICATOR_INSET, self.dpi()),
            top: enlargement.top + scaled(MAGNIFICATION_INDICATOR_INSET, self.dpi()),
            right: 0,
            bottom: 0,
        };
        let area = RECT {
            right: corner.left + pad_x + text.width + pad_x,
            bottom: corner.top + pad_y + line + pad_y,
            ..corner
        };
        let window = self.in_window(&area);
        Ok(PlacedIndicator {
            background: rounded_at(
                self.window_origin(),
                &area,
                scaled(MAGNIFICATION_INDICATOR_RADIUS, self.dpi()),
                0,
                0,
                false,
            ),
            // The text sits in its line the way CSS does it, as the notice's does.
            text: text.in_line(window.left + px(pad_x), window.top + px(pad_y), px(line)),
        })
    }

    /// The magnification indicator over the enlargement: a rounded rectangle of black at 60%, and the magnification in
    /// white on it.
    fn draw_magnification_indicator(
        &self,
        target: &ID2D1RenderTarget,
        magnification_indicator: &PlacedIndicator,
    ) {
        // SAFETY: drawing calls on a live render target with arguments that outlive them. Out of BeginDraw and EndDraw,
        // Direct2D only fails the frame at EndDraw. Each brush's opacity is set before it is used.
        unsafe {
            target.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
            self.shade.SetOpacity(MAGNIFICATION_INDICATOR_FILL_OPACITY);
            target.FillRoundedRectangle(&magnification_indicator.background, &self.shade);
            // Grayscale, not ClearType: color fringes would show on the see-through black.
            target.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
            self.white.SetOpacity(1.0);
            target.DrawTextLayout(
                dot(
                    magnification_indicator.text.left,
                    magnification_indicator.text.top,
                )
                .point,
                &magnification_indicator.text.layout,
                &self.white,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
            );
        }
    }

    /// The readout's line for `coordinate`, whose key is `key` (`X` or `Y`), laid out in the coordinate's line. The key
    /// is medium and dim, the physical number semibold in the ink, and the logical one, after a bar, medium at
    /// `LOGICAL_OPACITY`, as in a pinned label. The texts share one baseline, on a whole pixel, which centers the
    /// physical number in the line the way CSS does it; the bar is centered in the line, as a flex item is.
    fn line(&self, key: &str, coordinate: &Coordinate) -> windows::core::Result<PlacedLine> {
        let label = &coordinate.label;
        // The key has no digits to align, so it is laid out without the tabular figures, which would take its kerning.
        let key = self.typeset_plain(key, &self.logical_format)?;
        let physical = self.typeset(&label.physical, &self.format)?;
        let logical = label
            .logical
            .as_deref()
            .map(|text| self.typeset(text, &self.logical_format))
            .transpose()?;
        let columns = line_columns(key.width, physical.width, self.dpi());

        let line = geometry::bounds(coordinate.line)?;
        let window = self.in_window(&line);
        let baseline = physical.baseline_in(window.top, px(line.bottom - line.top));
        let at = |text: Typeset, along: i32| text.on_baseline(window.left + px(along), baseline);
        let logical = logical.map(|text| {
            let height = scaled(geometry::BAR_HEIGHT, self.dpi());
            // Half the line's spare room above the bar, rounded up as Chromium puts a box on the pixel grid.
            let top = line.top + (line.bottom - line.top - height + 1) / 2;
            let left = line.left + columns.bar;
            let bar = RECT {
                left,
                top,
                right: left + scaled(geometry::BAR_WIDTH, self.dpi()),
                bottom: top + height,
            };
            PlacedLogical {
                bar: self.in_window(&bar),
                text: at(text, columns.logical),
            }
        });
        Ok(PlacedLine {
            key: at(key, 0),
            physical: at(physical, columns.physical),
            logical,
        })
    }

    /// The readout's lines: for each, the key in the dim ink, the physical number in the ink and, when there is one,
    /// the bar and the logical number fainter, as a pinned label draws them.
    fn draw_readout(&self, target: &ID2D1RenderTarget, lines: &[PlacedLine]) {
        let origin = |text: &PlacedText| dot(text.left, text.top).point;
        for line in lines {
            // SAFETY: drawing calls on a live render target with arguments that outlive them. Out of BeginDraw and
            // EndDraw, Direct2D only fails the frame at EndDraw. Each brush's opacity is set before it is used, because
            // the brushes keep the last one.
            unsafe {
                // Grayscale, not ClearType: color fringes would show on the see-through panel.
                target.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
                self.surfaces.dim.SetOpacity(1.0);
                target.DrawTextLayout(
                    origin(&line.key),
                    &line.key.layout,
                    &self.surfaces.dim,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                );
                self.surfaces.ink.SetOpacity(1.0);
                target.DrawTextLayout(
                    origin(&line.physical),
                    &line.physical.layout,
                    &self.surfaces.ink,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                );
            }
            if let Some(logical) = &line.logical {
                self.draw_logical(target, logical, &self.surfaces.ink);
            }
        }
    }

    /// The color's swatch, its corners rounded, and its texts, each laid out in its line with the same tabular figures
    /// as the position, its baseline on a whole pixel, as a coordinate's number is. The first text is semibold, the
    /// prototype's `.v`, and the one under it medium, its `.dm`.
    fn color(&self, color: &PixelColor) -> windows::core::Result<PlacedColor> {
        let place = |text: &ColorText, format: &IDWriteTextFormat| {
            self.placed_in(self.typeset(&text.text, format)?, text.line)
        };
        Ok(PlacedColor {
            swatch: self.shaped(
                &geometry::bounds(color.swatch)?,
                scaled(SWATCH_RADIUS, self.dpi()),
                0,
                0,
            ),
            ring: scaled(SWATCH_RING, self.dpi()),
            rgb: color.rgb,
            first: place(&color.first, &self.format)?,
            second: color
                .second
                .as_ref()
                .map(|second| place(second, &self.logical_format))
                .transpose()?,
        })
    }

    /// The Approximate color notice: its sign's corner and its lines, each laid out in its line with its baseline on a
    /// whole pixel, as the color's texts are. The text has no digits to align, so it is laid out without the tabular
    /// figures.
    fn approximate(&self, notice: &ApproximateColor) -> windows::core::Result<PlacedApproximate> {
        let sign = self.in_window(&geometry::bounds(notice.sign)?);
        let place = |line: &NoticeLine| {
            self.placed_in(
                self.typeset_plain(line.text, &self.approximate_format)?,
                line.line,
            )
        };
        let [first, second] = &notice.lines;
        Ok(PlacedApproximate {
            sign: (sign.left, sign.top),
            lines: [place(first)?, place(second)?],
        })
    }

    /// `typeset` in `line`, its baseline on a whole pixel where it centers the text in the line the way CSS does it.
    fn placed_in(&self, typeset: Typeset, line: Rect) -> windows::core::Result<PlacedText> {
        let bounds = geometry::bounds(line)?;
        let window = self.in_window(&bounds);
        Ok(typeset.in_line(window.left, window.top, px(bounds.bottom - bounds.top)))
    }

    /// The Approximate color notice's alert sign, the notices' own, and its two lines beside it, all in the warning's
    /// amber.
    fn draw_approximate(
        &self,
        target: &ID2D1RenderTarget,
        approximate: &PlacedApproximate,
    ) -> windows::core::Result<()> {
        let (left, top) = approximate.sign;
        self.draw_sign(target, Sign::Alert, left, top)?;
        // SAFETY: drawing calls on a live render target with arguments that outlive them. Out of BeginDraw and EndDraw,
        // Direct2D only fails the frame at EndDraw. The warning's brush never changes its opacity.
        unsafe {
            // Grayscale, not ClearType: color fringes would show on the see-through panel.
            target.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
            for line in &approximate.lines {
                target.DrawTextLayout(
                    dot(line.left, line.top).point,
                    &line.layout,
                    &self.surfaces.warn,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                );
            }
        }
        Ok(())
    }

    /// The swatch in the pixel's color with the palette's border over its edge, then the first text in the ink and the
    /// one under it, when there is one, at `LOGICAL_OPACITY`, as the prototype's `.dm` and a label's logical part.
    fn draw_color(&self, target: &ID2D1RenderTarget, color: &PlacedColor) {
        let origin = |text: &PlacedText| dot(text.left, text.top).point;
        // SAFETY: drawing calls on a live render target with arguments that outlive them. Out of BeginDraw and EndDraw,
        // Direct2D only fails the frame at EndDraw. The swatch's color and the ink's opacity are set before they are
        // used, because those brushes keep the last one; the border's brush never changes.
        unsafe {
            // The straight sides are on whole pixels, so they are crisp; only the corners are antialiased.
            target.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
            self.swatch.SetColor(&d2d_color(color.rgb, 1.0));
            target.FillRoundedRectangle(&color.swatch, &self.swatch);
            target.DrawRoundedRectangle(
                &ring(&color.swatch, px(color.ring)),
                &self.surfaces.border,
                px(color.ring),
                None::<&ID2D1StrokeStyle>,
            );
            // Grayscale, not ClearType: color fringes would show on the see-through panel.
            target.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
            self.surfaces.ink.SetOpacity(1.0);
            target.DrawTextLayout(
                origin(&color.first),
                &color.first.layout,
                &self.surfaces.ink,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
            );
            if let Some(second) = &color.second {
                self.surfaces.ink.SetOpacity(LOGICAL_OPACITY);
                target.DrawTextLayout(
                    origin(second),
                    &second.layout,
                    &self.surfaces.ink,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use screengauge_core::session::{ColorText, Label, PixelColor, Point, Rect};
    use screengauge_core::settings::Magnification;

    use super::*;

    fn rect(left: i32, top: i32, width: u32, height: u32) -> Rect {
        Rect {
            left,
            top,
            width,
            height,
        }
    }

    /// An 8x view with the cursor at (60, 70) on a monitor whose left side is at 55, so the source, 21 pixels across,
    /// reaches 5 pixels past it.
    fn near_the_left_side() -> MagnifiedView {
        let line = rect(0, 0, 0, 0);
        let coordinate = Coordinate {
            label: Label::default(),
            line,
        };
        let color = PixelColor {
            rgb: [0, 0, 0],
            swatch: line,
            first: ColorText {
                text: String::new(),
                line,
            },
            second: None,
        };
        MagnifiedView {
            monitor: 0,
            panel: rect(104, 264, 182, 230),
            enlargement: rect(110, 270, 168, 168),
            source: rect(50, 60, 21, 21),
            shown: rect(55, 60, 16, 21),
            center: Point { x: 60, y: 70 },
            magnification: Magnification::X8,
            grid: true,
            magnification_indicator: false,
            x: coordinate.clone(),
            y: coordinate,
            color,
            approximate_color: None,
        }
    }

    fn area(rect: &RECT) -> i32 {
        (rect.right - rect.left) * (rect.bottom - rect.top)
    }

    #[test]
    fn the_monitors_part_of_the_enlargement_starts_where_the_source_reaches_the_monitor() {
        let places = places(&near_the_left_side()).expect("places");
        assert_eq!(places.factor, 8);
        // 5 pixels of the source are past the monitor's left side, so 5 enlarged pixels of the enlargement are blank;
        // the rest reaches the enlargement's right and bottom sides, 16 and 21 pixels enlarged.
        let enlargement = RECT {
            left: 110,
            top: 270,
            right: 278,
            bottom: 438,
        };
        assert_eq!(places.enlargement, enlargement);
        assert_eq!(
            places.drawn,
            RECT {
                left: 110 + 5 * 8,
                top: 270,
                right: 278,
                bottom: 438,
            }
        );
        // The photo's pixels it enlarges are `shown`, as they are in the virtual screen.
        assert_eq!(
            places.shown,
            RECT {
                left: 55,
                top: 60,
                right: 71,
                bottom: 81,
            }
        );
        // The pixel under the cursor is the middle one of the 21, the eleventh, 10 enlarged pixels from the
        // enlargement's left.
        assert_eq!(
            places.cell,
            RECT {
                left: 110 + 10 * 8,
                top: 270 + 10 * 8,
                right: 110 + 11 * 8,
                bottom: 270 + 11 * 8,
            }
        );
    }

    #[test]
    fn a_view_too_large_for_a_coordinate_is_an_error_and_not_a_wrong_picture() {
        let mut view = near_the_left_side();
        view.shown = rect(55, 60, u32::MAX, 21);
        assert!(places(&view).is_err());
    }

    #[test]
    fn a_ring_is_the_pixels_just_outside_the_cell_each_once() {
        let cell = RECT {
            left: 10,
            top: 20,
            right: 18,
            bottom: 28,
        };
        for (distance, pixels) in [(1, 10 * 10 - 8 * 8), (2, 12 * 12 - 10 * 10)] {
            let sides = ring_around(&cell, distance);
            assert_eq!(sides.iter().map(area).sum::<i32>(), pixels);
            for (n, side) in sides.iter().enumerate() {
                // One pixel thick, and outside the cell and inside the next ring out.
                assert_eq!(
                    (side.right - side.left).min(side.bottom - side.top),
                    1,
                    "{side:?}"
                );
                assert!(side.left >= cell.left - distance && side.right <= cell.right + distance);
                assert!(side.top >= cell.top - distance && side.bottom <= cell.bottom + distance);
                // No two sides share a pixel.
                for other in &sides[n + 1..] {
                    let shared = side.left < other.right
                        && other.left < side.right
                        && side.top < other.bottom
                        && other.top < side.bottom;
                    assert!(!shared, "{side:?} and {other:?}");
                }
            }
        }
    }

    #[test]
    fn the_parts_of_a_line_follow_each_other_by_the_prototypes_gaps_each_scaled_on_its_own() {
        // 100%: a key narrower than its box of 14, a gap of 8, the number, a gap of 8 and a margin of 7, a bar of 1, a
        // margin of 7 and a gap of 8.
        assert_eq!(
            line_columns(8, 40, 96),
            LineColumns {
                physical: 14 + 8,
                bar: 14 + 8 + 40 + 8 + 7,
                logical: 14 + 8 + 40 + 8 + 7 + 1 + 7 + 8,
            }
        );
        // 175%: 14 is 25, 8 is 14, 7 is 12 and 1 is 2, each on its own.
        assert_eq!(
            line_columns(18, 60, 168),
            LineColumns {
                physical: 25 + 14,
                bar: 25 + 14 + 60 + 14 + 12,
                logical: 25 + 14 + 60 + 14 + 12 + 2 + 12 + 14,
            }
        );
        // A key wider than its box widens it.
        assert_eq!(line_columns(30, 40, 96).physical, 30 + 8);
    }
}
