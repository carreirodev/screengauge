//! Inspect's magnified view (v1 spec WPC-296, Inspect; the prototype's variant A, WPC-279): the floating panel, the
//! frozen screen's pixels enlarged without smoothing, the grid, the outline around the pixel under the cursor, the
//! magnification's tag and the strip with the cursor's position. Every rectangle comes from the session core's
//! [`MagnifiedView`]; this only draws them onto the overlay's render target.
//!
//! The prototype's `drawMag` and its CSS (`.panel`, `.magframe`, `.magtag`, `.info`) are the model. Where the core hands
//! over a rectangle, it is used as it is; the rest of the sizes below are the prototype's, in logical pixels, which
//! `scaled` turns into physical pixels for the monitor's scale, each one on its own.

use std::mem::ManuallyDrop;

use screengauge_core::session::{Coordinate, MagnifiedView};
use windows::Win32::Foundation::{E_FAIL, RECT};
use windows::Win32::Graphics::Direct2D::{
    D2D1_ANTIALIAS_MODE_ALIASED, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
    D2D1_BITMAP_INTERPOLATION_MODE_NEAREST_NEIGHBOR, D2D1_DRAW_TEXT_OPTIONS_NONE,
    D2D1_LAYER_OPTIONS_NONE, D2D1_LAYER_PARAMETERS, D2D1_ROUNDED_RECT,
    D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE, ID2D1Bitmap, ID2D1Geometry, ID2D1Layer, ID2D1RenderTarget,
};
use windows::Win32::Graphics::DirectWrite::{DWRITE_FONT_WEIGHT, DWRITE_FONT_WEIGHT_BOLD};
use windows::core::{Error, Interface};

use super::geometry::{self, scaled};
use super::{
    BAR_OPACITY, LOGICAL_OPACITY, Overlay, PlacedLogical, PlacedText, Typeset, dot, px, rounded_at,
};

// Every size below is in logical pixels, which `scaled` turns into physical pixels for the monitor's scale.

/// The panel's corner radius, and the frame's (CSS `.magframe { border-radius: 9px; overflow: hidden }`).
const PANEL_RADIUS: u32 = 14;
const FRAME_RADIUS: u32 = 9;

/// How much of the notice's shadow this panel casts. The notice's layers are fitted to a shadow box 15px tall (the
/// notice's 31px less the spread of 8 on each side), and this panel is 214 by 255 at the most, so its box, 198 by 239,
/// is as good as an edge as far as a blur of sigma 14 (dark) or 13 (light) can tell. A box that tall casts a shadow
/// stronger than the layers give at full strength, so the layers are made stronger, as the tolerance indicator's, which
/// is lower than the notice, makes them fainter (see `SHADOW_STRENGTH` in `tolerance`).
///
/// Fitted by least squares, the layers laid over each other as Direct2D lays them, against CSS's true blur of the box
/// (`0 10px 28px -8px` at .75 in the dark palette, `0 10px 26px -8px` at .45 in the light one, sigma half the blur),
/// read outward from the panel at 1 pixel steps, 0 to 30 pixels beyond it on the sides and above, and 40 below. The
/// answer does not depend on the panel's size, which is much taller than the blur. Dark and light want:
///
/// - down the middle, below the panel, which is how the tolerance indicator's 0.68 was found (the same reading gives its
///   0.67 and 0.70): 1.32 and 1.25;
/// - along the middle of a side: 1.17 and 1.04;
/// - above the panel: 1.03 and 0.80.
///
/// 1.25 is the one number for both palettes that is nearest to all three: the opacity it leaves is within 0.020 (dark)
/// and 0.014 (light) of CSS's, root mean square, below the panel, 0.012 and 0.012 on the sides and 0.007 above it. Not
/// closer, because the layers are the notice's: they fall off the way a box 15px tall's shadow does, a little slower
/// than an edge's, so no single number fits the panel's whole shadow. Right under the panel it is up to 0.06 of opacity
/// lighter than CSS's.
pub(super) const SHADOW_STRENGTH: f32 = 1.25;

/// How much of the grid's black shows over the pixels (CSS `rgba(0,0,0,.2)`).
const GRID_OPACITY: f32 = 0.2;

/// The tag: `.magtag`, `left: 6px; top: 6px`, a padding of 4px above and below and 6px at the sides, corners of 6px, a
/// background of black at .6, and its text in white, 700 weight and 11px with a line as tall as itself.
const TAG_INSET: u32 = 6;
const TAG_PAD_X: u32 = 6;
const TAG_PAD_Y: u32 = 4;
const TAG_RADIUS: u32 = 6;
const TAG_FILL_OPACITY: f32 = 0.6;
pub const TAG_TEXT: u32 = 11;
pub const TAG_WEIGHT: DWRITE_FONT_WEIGHT = DWRITE_FONT_WEIGHT_BOLD;

/// The strip's rows (`.info .row`): the key `X` or `Y` in a box at least this wide, then a gap, then the physical number.
/// A logical part follows after a gap and a margin, a bar `BAR_WIDTH` wide and `BAR_HEIGHT` tall, a margin and a gap.
const KEY_WIDTH: u32 = 14;
const GAP: u32 = 8;
const BAR_MARGIN: u32 = 7;
const BAR_WIDTH: u32 = 1;
const BAR_HEIGHT: u32 = 11;

/// Where the frame's parts go, in physical pixels of the virtual screen, right and bottom one past the last pixel.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Places {
    /// The magnification: how many physical pixels a side each enlarged pixel takes.
    factor: i32,
    /// The whole frame, where the blank color goes.
    frame: RECT,
    /// The part of the frame the monitor's pixels fill: each of the photo's pixels in `shown` is a square of the
    /// magnification's side here.
    drawn: RECT,
    /// The photo's pixels that are enlarged: the view's `shown`.
    shown: RECT,
    /// The enlarged pixel under the cursor.
    cell: RECT,
}

/// Where the parts of `view`'s frame go. Fails for a view so large that its pixels don't fit a coordinate.
fn places(view: &MagnifiedView) -> windows::core::Result<Places> {
    let too_large = || Error::new(E_FAIL, "the magnified view is too large to draw");
    let whole = |value: i64| i32::try_from(value).map_err(|_| too_large());
    let factor = whole(i64::from(view.magnification.factor()))?;
    let frame = geometry::bounds(view.frame)?;
    // Where the enlarged pixel of the frozen screen's pixel at `at` starts along an axis: its distance from the source's
    // first pixel, enlarged, from the frame's first pixel.
    let start = |frame: i32, source: i32, at: i32| {
        whole(i64::from(frame) + (i64::from(at) - i64::from(source)) * i64::from(factor))
    };
    let (source, shown) = (view.source, view.shown);
    let left = start(frame.left, source.left, shown.left)?;
    let top = start(frame.top, source.top, shown.top)?;
    let drawn = RECT {
        left,
        top,
        right: whole(i64::from(left) + i64::from(shown.width) * i64::from(factor))?,
        bottom: whole(i64::from(top) + i64::from(shown.height) * i64::from(factor))?,
    };
    let left = start(frame.left, source.left, view.center.x)?;
    let top = start(frame.top, source.top, view.center.y)?;
    let cell = RECT {
        left,
        top,
        right: whole(i64::from(left) + i64::from(factor))?,
        bottom: whole(i64::from(top) + i64::from(factor))?,
    };
    Ok(Places {
        factor,
        frame,
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
struct Columns {
    physical: i32,
    bar: i32,
    logical: i32,
}

fn columns(key: i32, physical: i32, dpi: u32) -> Columns {
    let physical_at = key.max(scaled(KEY_WIDTH, dpi)) + scaled(GAP, dpi);
    let bar = physical_at + physical + scaled(GAP, dpi) + scaled(BAR_MARGIN, dpi);
    Columns {
        physical: physical_at,
        bar,
        logical: bar + scaled(BAR_WIDTH, dpi) + scaled(BAR_MARGIN, dpi) + scaled(GAP, dpi),
    }
}

/// The tag, ready to draw.
struct PlacedTag {
    background: D2D1_ROUNDED_RECT,
    text: PlacedText,
}

/// One of the strip's lines, ready to draw: the key, the physical number, and the bar and the logical number when the
/// coordinate has them.
struct PlacedStrip {
    key: PlacedText,
    physical: PlacedText,
    logical: Option<PlacedLogical>,
}

impl Overlay {
    /// Draws `view` over the measurements and under the toolbar, between BeginDraw and EndDraw, enlarging the pixels of
    /// `photo`, the window's monitor's photo with its pixel (0, 0) at the monitor's top left corner.
    ///
    /// Everything that can fail is done before the first thing is drawn into the frame: a clip is pushed and popped
    /// with nothing between that could return early, so the frame is never left with a clip pushed.
    pub fn draw_magnified_view(
        &self,
        target: &ID2D1RenderTarget,
        view: &MagnifiedView,
        photo: &ID2D1Bitmap,
    ) -> windows::core::Result<()> {
        let places = places(view)?;
        let tag = if view.tag {
            Some(self.tag(&places.frame, places.factor)?)
        } else {
            None
        };
        let strips = [self.strip("X", &view.x)?, self.strip("Y", &view.y)?];
        let radius = scaled(FRAME_RADIUS, self.dpi);
        let mask: ID2D1Geometry = self
            .geometry_of(&self.shaped(&places.frame, radius, 0, 0))?
            .cast()?;

        self.draw_floating_panel(
            target,
            &geometry::bounds(view.panel)?,
            PANEL_RADIUS,
            SHADOW_STRENGTH,
        )?;

        let frame = self.in_window(&places.frame);
        // `D2D1_LAYER_PARAMETERS::default()` is all zeros: no content, no opacity and a transform that squashes the mask
        // to a point, so everything the layer is meant to carry is set.
        let mut parameters = D2D1_LAYER_PARAMETERS {
            contentBounds: frame,
            geometricMask: ManuallyDrop::new(Some(mask)),
            maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
            opacity: 1.0,
            layerOptions: D2D1_LAYER_OPTIONS_NONE,
            ..D2D1_LAYER_PARAMETERS::default()
        };
        // The mask is in the window's pixels already, so the transform leaves it where it is. The matrix is the windows
        // crate's own type from windows-numerics, which ScreenGauge doesn't name; the default's entries are all zero.
        parameters.maskTransform.M11 = 1.0;
        parameters.maskTransform.M22 = 1.0;
        // SAFETY: drawing calls on a live render target with arguments that outlive them. Out of BeginDraw and EndDraw,
        // Direct2D only fails the frame at EndDraw. The layer is managed by Direct2D (it is Windows 8 and later, and
        // ScreenGauge is Windows 11 only), and the pop below follows with nothing between that can return.
        unsafe { target.PushLayer(&parameters, None::<&ID2D1Layer>) };
        self.draw_frame(target, photo, &places, view);
        if let Some(tag) = &tag {
            self.draw_tag(target, tag);
        }
        // SAFETY: pops the layer pushed above, once.
        unsafe { target.PopLayer() };
        // The layer kept its own reference to the mask until the pop; this one is released here, once.
        drop(ManuallyDrop::into_inner(parameters.geometricMask));

        self.draw_strip(target, &strips);
        Ok(())
    }

    /// What the frame holds, clipped to its rounded corners by the layer the caller pushed: the blank color, the monitor's
    /// pixels enlarged, the grid and the outline of the pixel under the cursor.
    fn draw_frame(
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
            // Aliased, so the blank color covers exactly the frame's pixels: the layer rounds the corners.
            target.SetAntialiasMode(D2D1_ANTIALIAS_MODE_ALIASED);
            target.FillRectangle(&self.in_window(&places.frame), &self.surfaces.blank);
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
        // SAFETY: as above. Back to Direct2D's default, which the tag's corners need.
        unsafe { target.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE) };
    }

    /// The grid over `drawn`, the part of the frame that holds the monitor's pixels: a line one pixel wide in black at
    /// 20% on the first column of each enlarged pixel but the first, and the same along the rows. Where a vertical line
    /// and a horizontal one cross, the pixel is filled twice, as the prototype's canvas does.
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
        // Direct2D only fails the frame at EndDraw. Aliased fills of whole pixels, as in `draw_grid`. Each brush's opacity
        // is set before it is used, because the brushes keep the last one.
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

    /// The tag with the magnification, `factor`, laid out with its top left corner `TAG_INSET` inside `frame`'s.
    fn tag(&self, frame: &RECT, factor: i32) -> windows::core::Result<PlacedTag> {
        let text = self.typeset_plain(&format!("{factor}\u{d7}"), &self.tag_format)?;
        let (pad_x, pad_y) = (scaled(TAG_PAD_X, self.dpi), scaled(TAG_PAD_Y, self.dpi));
        let line = scaled(TAG_TEXT, self.dpi);
        let corner = RECT {
            left: frame.left + scaled(TAG_INSET, self.dpi),
            top: frame.top + scaled(TAG_INSET, self.dpi),
            right: 0,
            bottom: 0,
        };
        let area = RECT {
            right: corner.left + pad_x + text.width + pad_x,
            bottom: corner.top + pad_y + line + pad_y,
            ..corner
        };
        let window = self.in_window(&area);
        // The text sits in its line the way CSS does it, as the notice's does: the line's extra room is shared equally,
        // and the baseline is on a whole pixel.
        let line_top = window.top + px(pad_y);
        let baseline = (line_top + (px(line) - text.line_height) / 2.0 + text.baseline).round();
        Ok(PlacedTag {
            background: rounded_at(
                self.window_origin(),
                &area,
                scaled(TAG_RADIUS, self.dpi),
                0,
                0,
                false,
            ),
            text: PlacedText {
                left: window.left + px(pad_x),
                top: baseline - text.baseline,
                layout: text.layout,
            },
        })
    }

    /// The tag over the frame: a rounded rectangle of black at 60%, and the magnification in white on it.
    fn draw_tag(&self, target: &ID2D1RenderTarget, tag: &PlacedTag) {
        // SAFETY: drawing calls on a live render target with arguments that outlive them. Out of BeginDraw and EndDraw,
        // Direct2D only fails the frame at EndDraw. Each brush's opacity is set before it is used.
        unsafe {
            target.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
            self.shade.SetOpacity(TAG_FILL_OPACITY);
            target.FillRoundedRectangle(&tag.background, &self.shade);
            // Grayscale, not ClearType: color fringes would show on the see-through black.
            target.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
            self.white.SetOpacity(1.0);
            target.DrawTextLayout(
                dot(tag.text.left, tag.text.top).point,
                &tag.text.layout,
                &self.white,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
            );
        }
    }

    /// The strip's line for `coordinate`, whose key is `key` (`X` or `Y`), laid out in the coordinate's line. The key is
    /// medium and dim, the physical number semibold in the ink, and the logical one, after a bar, medium at
    /// `LOGICAL_OPACITY`, as in a pinned label. The texts share one baseline, on a whole pixel, which centers the
    /// physical number in the line the way CSS does it; the bar is centered in the line, as a flex item is.
    fn strip(&self, key: &str, coordinate: &Coordinate) -> windows::core::Result<PlacedStrip> {
        let label = &coordinate.label;
        // The key has no digits to align, so it is laid out without the tabular figures, which would take its kerning.
        let key = self.typeset_plain(key, &self.logical_format)?;
        let physical = self.typeset(&label.physical, &self.format)?;
        let logical = label
            .logical
            .as_deref()
            .map(|text| self.typeset(text, &self.logical_format))
            .transpose()?;
        let columns = columns(key.width, physical.width, self.dpi);

        let line = geometry::bounds(coordinate.line)?;
        let window = self.in_window(&line);
        let baseline = (window.top
            + (px(line.bottom - line.top) - physical.line_height) / 2.0
            + physical.baseline)
            .round();
        let at = |text: Typeset, along: i32| PlacedText {
            left: window.left + px(along),
            top: baseline - text.baseline,
            layout: text.layout,
        };
        let logical = logical.map(|text| {
            let height = scaled(BAR_HEIGHT, self.dpi);
            // Half the line's spare room above the bar, rounded up as Chromium puts a box on the pixel grid.
            let top = line.top + (line.bottom - line.top - height + 1) / 2;
            let left = line.left + columns.bar;
            let bar = RECT {
                left,
                top,
                right: left + scaled(BAR_WIDTH, self.dpi),
                bottom: top + height,
            };
            PlacedLogical {
                bar: self.in_window(&bar),
                text: at(text, columns.logical),
            }
        });
        Ok(PlacedStrip {
            key: at(key, 0),
            physical: at(physical, columns.physical),
            logical,
        })
    }

    /// The strip's lines: for each, the key in the dim ink, the physical number in the ink and, when there is one, the
    /// bar and the logical number fainter.
    fn draw_strip(&self, target: &ID2D1RenderTarget, strips: &[PlacedStrip]) {
        let origin = |text: &PlacedText| dot(text.left, text.top).point;
        // SAFETY: drawing calls on a live render target with arguments that outlive them. Out of BeginDraw and EndDraw,
        // Direct2D only fails the frame at EndDraw. Each brush's opacity is set before it is used, because the brushes
        // keep the last one.
        unsafe {
            // Grayscale, not ClearType: color fringes would show on the see-through panel.
            target.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
            for strip in strips {
                self.surfaces.dim.SetOpacity(1.0);
                target.DrawTextLayout(
                    origin(&strip.key),
                    &strip.key.layout,
                    &self.surfaces.dim,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                );
                self.surfaces.ink.SetOpacity(1.0);
                target.DrawTextLayout(
                    origin(&strip.physical),
                    &strip.physical.layout,
                    &self.surfaces.ink,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                );
                if let Some(logical) = &strip.logical {
                    // The bar is a fill of whole pixels, so it is exactly its width however the scale rounds; then the
                    // default comes back.
                    target.SetAntialiasMode(D2D1_ANTIALIAS_MODE_ALIASED);
                    self.surfaces.ink.SetOpacity(BAR_OPACITY);
                    target.FillRectangle(&logical.bar, &self.surfaces.ink);
                    target.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
                    self.surfaces.ink.SetOpacity(LOGICAL_OPACITY);
                    target.DrawTextLayout(
                        origin(&logical.text),
                        &logical.text.layout,
                        &self.surfaces.ink,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                    );
                }
            }
        }
    }
}
