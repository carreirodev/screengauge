//! Raised surfaces painted the way the prototype's CSS paints a box (WPC-279): its outer shadows, its background, its
//! border and then its inset shadows, at the monitor's scale. The toolbar's buttons and panels (WPC-310) and Settings'
//! controls (WPC-320) are painted with it. A blurred shadow is layers of rounded rectangles, `shadow`'s; the colors are
//! `palette`'s.
//!
//! Faint layers drawn straight over opaque 8-bit pixels round away, so whoever paints shadows here paints them on a
//! transparent bitmap first, and lays that bitmap over the opaque surface (`toolbar_cache` measures the difference).

pub mod palette;
pub mod shadow;

use screengauge_core::session::{Point, toolbar};
use std::mem::ManuallyDrop;
use windows::Win32::Foundation::{E_FAIL, RECT};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_RECT_F, D2D_SIZE_F, D2D_SIZE_U, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_GRADIENT_STOP,
    D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_COMBINE_MODE_EXCLUDE,
    D2D1_COMPATIBLE_RENDER_TARGET_OPTIONS_NONE, D2D1_ELLIPSE, D2D1_EXTEND_MODE_CLAMP,
    D2D1_GAMMA_2_2, D2D1_LAYER_OPTIONS_NONE, D2D1_LAYER_PARAMETERS,
    D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES, D2D1_ROUNDED_RECT, ID2D1BitmapRenderTarget, ID2D1Brush,
    ID2D1Factory, ID2D1Geometry, ID2D1Layer, ID2D1LinearGradientBrush, ID2D1PathGeometry,
    ID2D1RenderTarget, ID2D1RoundedRectangleGeometry, ID2D1SolidColorBrush, ID2D1StrokeStyle,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::core::{Error, Interface};

use crate::graphics::{Color, color, d2d_color, solid_brush};
use palette::{Shadow, Stop, Surface};
use shadow::Profile;

/// How flat Direct2D makes the curves of a shape it combines with another: well under a pixel.
const FLATTENING: f32 = 0.1;

/// `logical` pixels on a monitor of `dpi` (96 is 100%), in whole physical pixels, halves up: the session core's
/// [`toolbar::scaled`], as a coordinate.
pub fn scaled(logical: u32, dpi: u32) -> i32 {
    // The core saturates the product, and divided by 96 it is at most 44 million, so the sign never flips.
    toolbar::scaled(logical, dpi).cast_signed()
}

/// `logical` pixels of the monitor's scale, whole, for a length that may be negative, as a shadow's spread and drop may.
pub fn signed(logical: i32, dpi: u32) -> i32 {
    logical.signum() * scaled(logical.unsigned_abs(), dpi)
}

/// The standard deviation, in physical pixels, of a CSS blur radius of `blur` logical pixels: half of it.
pub fn sigma(blur: u32, dpi: u32) -> f64 {
    f64::from(blur) * f64::from(dpi) / 192.0
}

/// A whole number of pixels as the float Direct2D takes. Positions are far below 2^24, up to which `f32` holds every
/// whole number, so nothing is lost.
// TODO(WPC-359): place shapes without going through a float, so this cast needs no assertion.
pub fn px(pixels: i32) -> f32 {
    debug_assert!(pixels.unsigned_abs() < 1 << 24, "{pixels} pixels");
    pixels as f32
}

/// An ellipse of no size at (`x`, `y`), for its `point`: the Vector2 Direct2D's calls take, which windows 0.62 takes from
/// the windows-numerics crate without re-exporting it, so this program can't name it, and may not add the crate.
pub fn dot(x: f32, y: f32) -> D2D1_ELLIPSE {
    let mut dot = D2D1_ELLIPSE::default();
    dot.point.X = x;
    dot.point.Y = y;
    dot
}

/// The rounded rectangle to stroke with a line `width` pixels wide so the stroke covers the ring between `outer` and the
/// same shape `width` smaller on every side: the line's middle runs half its width inside the outer edge.
pub fn ring(outer: &D2D1_ROUNDED_RECT, width: f32) -> D2D1_ROUNDED_RECT {
    let half = width / 2.0;
    D2D1_ROUNDED_RECT {
        rect: D2D_RECT_F {
            left: outer.rect.left + half,
            top: outer.rect.top + half,
            right: outer.rect.right - half,
            bottom: outer.rect.bottom - half,
        },
        radiusX: (outer.radiusX - half).max(0.0),
        radiusY: (outer.radiusY - half).max(0.0),
    }
}

/// `rect`, from the virtual screen to the pixels of a surface whose pixel (0, 0) is the virtual screen's `origin`: the
/// window's own, or a bitmap's.
pub fn in_surface(origin: Point, rect: &RECT) -> D2D_RECT_F {
    D2D_RECT_F {
        left: px(rect.left - origin.x),
        top: px(rect.top - origin.y),
        right: px(rect.right - origin.x),
        bottom: px(rect.bottom - origin.y),
    }
}

/// `rect` as a rounded rectangle in the pixels of the surface at `origin`, with corners of `radius` pixels, grown by
/// `grow` pixels on every side (or smaller, should it be negative) and moved `down` pixels. The corners grow with it, as
/// a CSS shadow's do.
///
/// `clamp` is for a shape that may be shrunk past nothing: the toolbar's shadow layers and inset holes shrink a group's
/// box by up to three deviations of the blur, which can pass half its height. Then a side that would cross the other
/// goes to the middle instead of turning the rectangle inside out, and a corner's radius is never negative. The
/// notice's, the tolerance indicator's and the labels' shapes never shrink that far, so they are left as they were.
pub fn rounded_at(
    origin: Point,
    rect: &RECT,
    radius: i32,
    grow: i32,
    down: i32,
    clamp: bool,
) -> D2D1_ROUNDED_RECT {
    let span = |near: i32, far: i32| {
        let (near_grown, far_grown) = (near - grow, far + grow);
        if !clamp || near_grown <= far_grown {
            (near_grown, far_grown)
        } else {
            let middle = near + (far - near) / 2;
            (middle, middle)
        }
    };
    let (left, right) = span(rect.left - origin.x, rect.right - origin.x);
    let (top, bottom) = span(rect.top - origin.y + down, rect.bottom - origin.y + down);
    let radius = if clamp {
        (radius + grow).max(0)
    } else {
        radius + grow
    };
    D2D1_ROUNDED_RECT {
        rect: D2D_RECT_F {
            left: px(left),
            top: px(top),
            right: px(right),
            bottom: px(bottom),
        },
        radiusX: px(radius),
        radiusY: px(radius),
    }
}

/// `shape` as a geometry, which can be cut out of another.
pub fn geometry_of(
    factory: &ID2D1Factory,
    shape: &D2D1_ROUNDED_RECT,
) -> windows::core::Result<ID2D1RoundedRectangleGeometry> {
    // SAFETY: `shape` outlives the call, which keeps nothing of it.
    unsafe { factory.CreateRoundedRectangleGeometry(shape) }
}

/// Fills `shape` in `brush` on `target` everywhere except where `hole` is.
pub fn fill_except(
    factory: &ID2D1Factory,
    target: &ID2D1RenderTarget,
    shape: &D2D1_ROUNDED_RECT,
    hole: &ID2D1RoundedRectangleGeometry,
    brush: &ID2D1SolidColorBrush,
) -> windows::core::Result<()> {
    let shape = geometry_of(factory, shape)?;
    let cut = excluding(factory, &shape, hole)?;
    // SAFETY: a drawing call on a live render target with arguments that outlive it. Out of BeginDraw and EndDraw,
    // Direct2D only fails the frame at EndDraw.
    unsafe { target.FillGeometry(&cut, brush, None::<&ID2D1Brush>) };
    Ok(())
}

/// `shape` less `hole`, as one geometry. Both are in the same pixels.
pub fn excluding(
    factory: &ID2D1Factory,
    shape: &ID2D1Geometry,
    hole: &ID2D1Geometry,
) -> windows::core::Result<ID2D1PathGeometry> {
    // SAFETY: no arguments.
    let cut = unsafe { factory.CreatePathGeometry() }?;
    // SAFETY: no arguments.
    let sink = unsafe { cut.Open() }?;
    // SAFETY: the geometries and the sink outlive the call, which keeps nothing of them. No transform: both are already
    // in the same pixels.
    unsafe { shape.CombineWithGeometry(hole, D2D1_COMBINE_MODE_EXCLUDE, None, FLATTENING, &sink) }?;
    // SAFETY: closes the sink opened above, once.
    unsafe { sink.Close() }?;
    Ok(cut)
}

/// Runs `paint` with what it draws on `target` gathered on a transparent layer of its own, which is then laid over what
/// the target holds: only inside `bounds`, in the target's pixels, and, when there is a `mask` (in the same pixels),
/// only inside it, with its edge antialiased.
pub fn in_layer<T>(
    target: &ID2D1RenderTarget,
    bounds: D2D_RECT_F,
    mask: Option<ID2D1Geometry>,
    paint: impl FnOnce() -> T,
) -> T {
    // `D2D1_LAYER_PARAMETERS::default()` is all zeros: no content, no opacity and a transform that squashes the mask to a
    // point, so everything the layer is meant to carry is set.
    let mut parameters = D2D1_LAYER_PARAMETERS {
        contentBounds: bounds,
        geometricMask: ManuallyDrop::new(mask),
        maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
        opacity: 1.0,
        layerOptions: D2D1_LAYER_OPTIONS_NONE,
        ..D2D1_LAYER_PARAMETERS::default()
    };
    // The mask is in the target's pixels already, so the transform leaves it where it is. The matrix is the windows
    // crate's own type from windows-numerics, which ScreenGauge doesn't name; the default's entries are all zero.
    parameters.maskTransform.M11 = 1.0;
    parameters.maskTransform.M22 = 1.0;
    // SAFETY: a drawing call on a live render target with arguments that outlive it. Out of BeginDraw and EndDraw,
    // Direct2D only fails the frame at EndDraw. The layer is managed by Direct2D (Windows 8 and later, and ScreenGauge
    // is Windows 11 only), and it is popped below whatever `paint` returns.
    unsafe { target.PushLayer(&parameters, None::<&ID2D1Layer>) };
    let painted = paint();
    // SAFETY: pops the layer pushed above, once.
    unsafe { target.PopLayer() };
    // The layer kept its own reference to the mask until the pop; this one is released here, once.
    drop(ManuallyDrop::into_inner(parameters.geometricMask));
    painted
}

/// A render target on a transparent bitmap of `width` by `height` pixels, compatible with `target`: what shadows are
/// painted on before they are laid over an opaque surface.
pub fn transparent_layer(
    target: &ID2D1RenderTarget,
    width: i32,
    height: i32,
) -> windows::core::Result<ID2D1BitmapRenderTarget> {
    let too_large = || Error::new(E_FAIL, "the layer's bitmap is too large");
    let pixels = D2D_SIZE_U {
        width: u32::try_from(width).map_err(|_| too_large())?,
        height: u32::try_from(height).map_err(|_| too_large())?,
    };
    // Both are the same number because the target is at 96 DPI, where a unit is a pixel.
    let units = D2D_SIZE_F {
        width: px(width),
        height: px(height),
    };
    let format = D2D1_PIXEL_FORMAT {
        format: DXGI_FORMAT_B8G8R8A8_UNORM,
        alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
    };
    // SAFETY: the sizes and the format outlive the call, which keeps nothing of them.
    unsafe {
        target.CreateCompatibleRenderTarget(
            Some(&units),
            Some(&pixels),
            Some(&format),
            D2D1_COMPATIBLE_RENDER_TARGET_OPTIONS_NONE,
        )
    }
}

/// An outer shadow of a box in physical pixels: how much it is grown, how far it drops, and its layers, which are
/// fitted across the box's short side after the spread.
pub struct OuterShadow {
    pub spread: i32,
    pub drop: i32,
    pub layers: shadow::Layers,
}

impl OuterShadow {
    /// `None` when the spread leaves nothing of the box.
    pub fn of(shadow: &Shadow, rect: &RECT, dpi: u32) -> Option<OuterShadow> {
        OuterShadow::in_bands(shadow, rect, dpi, shadow::FINE)
    }

    /// As `of`, with `bands` bands to each standard deviation of the blur (`shadow::layers_in_bands`).
    pub fn in_bands(shadow: &Shadow, rect: &RECT, dpi: u32, bands: f64) -> Option<OuterShadow> {
        let spread = signed(shadow.spread, dpi);
        let short = (rect.right - rect.left).min(rect.bottom - rect.top) + 2 * spread;
        if short <= 0 {
            return None;
        }
        let layers = shadow::layers_in_bands(
            sigma(shadow.blur, dpi),
            shadow.color.alpha,
            Profile::Strip(f64::from(short)),
            bands,
        );
        Some(OuterShadow {
            spread,
            drop: signed(shadow.y, dpi),
            layers,
        })
    }
}

/// How a box moves and shrinks, as its CSS `transform`: `scale` around its middle, then `down` logical pixels down (up
/// when below zero).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Motion {
    pub scale: f32,
    pub down: i32,
}

/// What paints raised surfaces on one render target at one scale. `origin` is the point of the virtual screen (or of
/// the window, for a window that isn't on the frozen screen) at the target's pixel (0, 0).
pub struct Canvas<'a> {
    pub target: &'a ID2D1RenderTarget,
    pub factory: &'a ID2D1Factory,
    pub dpi: u32,
    pub origin: Point,
    /// Recolored for every fill, so the layers of a shadow share one.
    pub brush: ID2D1SolidColorBrush,
}

impl<'a> Canvas<'a> {
    pub fn new(
        target: &'a ID2D1RenderTarget,
        factory: &'a ID2D1Factory,
        dpi: u32,
        origin: Point,
    ) -> windows::core::Result<Canvas<'a>> {
        Ok(Canvas {
            target,
            factory,
            dpi,
            origin,
            brush: solid_brush(target, color(0, 0, 0, 1.0))?,
        })
    }

    /// `logical` pixels of the monitor's scale, whole.
    pub fn scaled(&self, logical: u32) -> i32 {
        scaled(logical, self.dpi)
    }

    pub fn signed(&self, logical: i32) -> i32 {
        signed(logical, self.dpi)
    }

    /// `rect`, grown by `grow` pixels on every side and moved `down`, as a rounded rectangle in the target's own pixels.
    /// Shadow layers and holes shrink a box by up to three deviations of the blur, which may pass half its height, so
    /// what shrinks past nothing is clamped.
    pub fn shape(&self, rect: &RECT, radius: i32, grow: i32, down: i32) -> D2D1_ROUNDED_RECT {
        rounded_at(self.origin, rect, radius, grow, down, true)
    }

    /// Sets the shared brush to `rgb` at `alpha`.
    pub fn set(&self, rgb: [u8; 3], alpha: f32) {
        // SAFETY: the color outlives the call, which keeps nothing of it.
        unsafe { self.brush.SetColor(&d2d_color(rgb, alpha)) };
    }

    pub fn fill(&self, shape: &D2D1_ROUNDED_RECT, color: Color) {
        self.set(color.rgb, color.alpha);
        // SAFETY: a drawing call on a live render target with arguments that outlive it. Out of BeginDraw and EndDraw,
        // Direct2D only fails the frame at EndDraw.
        unsafe { self.target.FillRoundedRectangle(shape, &self.brush) };
    }

    /// Runs `paint` with the target moved and shrunk as `motion` says, around the middle of `place`, as CSS transforms
    /// a box and all it draws, shadows included. The target's own transform is put back afterwards, whatever `paint` did.
    pub fn in_motion(
        &self,
        place: &RECT,
        motion: Motion,
        paint: impl FnOnce() -> windows::core::Result<()>,
    ) -> windows::core::Result<()> {
        let area = in_surface(self.origin, place);
        let (x, y) = (
            (area.left + area.right) / 2.0,
            (area.top + area.bottom) / 2.0,
        );
        // The windows crate takes its matrix from windows-numerics, which ScreenGauge doesn't name as a dependency: the
        // target's own transform is a value of that type, and the move is a copy of it with every entry set.
        let mut before = Default::default();
        // SAFETY: fills a local.
        unsafe { self.target.GetTransform(&mut before) };
        let mut moving = before;
        // A point p goes to (x, y) + scale × (p − (x, y)), then `down` pixels down. Direct2D multiplies a row vector by
        // the matrix, so the last row is the move.
        moving.M11 = motion.scale;
        moving.M12 = 0.0;
        moving.M21 = 0.0;
        moving.M22 = motion.scale;
        moving.M31 = x * (1.0 - motion.scale);
        moving.M32 = y * (1.0 - motion.scale) + px(self.signed(motion.down));
        // The box's own motion first, then whatever the target was already doing.
        let moved_then_before = moving * before;
        // SAFETY: the matrix outlives the call, which copies it.
        unsafe { self.target.SetTransform(&moved_then_before) };
        let painted = paint();
        // SAFETY: as above.
        unsafe { self.target.SetTransform(&before) };
        painted
    }

    /// A box painted as CSS does: the outer shadows, the background, the border and the inset shadows, the last listed of
    /// each kind first. `ring_width` is the border's width and `radius` the corners', in logical pixels, and `see_through`
    /// is whether the shadow is cut out under the box, as it is for a background that lets what is under it show.
    pub fn surface(
        &self,
        surface: &Surface,
        rect: &RECT,
        radius: u32,
        ring_width: u32,
        see_through: bool,
    ) -> windows::core::Result<()> {
        let radius = self.scaled(radius);
        let ring_width = self.scaled(ring_width);
        let outer = self.shape(rect, radius, 0, 0);
        let hole = if see_through {
            Some(geometry_of(self.factory, &outer)?)
        } else {
            None
        };
        for shadow in surface.outer.iter().rev() {
            self.outer_shadow(shadow, rect, radius, hole.as_ref())?;
        }
        // The background reaches under the border; a gradient runs from the top of the padding box to its bottom.
        if let Some(base) = surface.base {
            self.fill(&outer, base);
        }
        if !surface.gradient.is_empty() {
            self.gradient(
                &outer,
                surface.gradient,
                outer.rect.top + px(ring_width),
                outer.rect.bottom - px(ring_width),
            )?;
        }
        if let Some(border) = surface.border {
            self.set(border.rgb, border.alpha);
            let width = px(ring_width);
            // SAFETY: a drawing call on a live render target with arguments that outlive it, as in `fill`.
            unsafe {
                self.target.DrawRoundedRectangle(
                    &ring(&outer, width),
                    &self.brush,
                    width,
                    None::<&ID2D1StrokeStyle>,
                );
            }
        }
        // Inset shadows are inside the border, in the padding box.
        let padding = RECT {
            left: rect.left + ring_width,
            top: rect.top + ring_width,
            right: rect.right - ring_width,
            bottom: rect.bottom - ring_width,
        };
        for shadow in surface.inset.iter().rev() {
            self.inset_shadow(shadow, &padding, (radius - ring_width).max(0))?;
        }
        Ok(())
    }

    /// The outer shadow `shadow` of the box `rect`, with corners of `radius` physical pixels, for a box painted straight
    /// over an opaque surface, as the frozen screen's labels and panels are: its layers, in `shadow::COARSE` bands, are
    /// painted on a transparent layer of their own, which is then laid over what the target holds, so faint layers don't
    /// round away (see the module's notes). `see_through` cuts the shadow out under the box, once, as the layer is laid.
    pub fn shadow_over(
        &self,
        shadow: &Shadow,
        rect: &RECT,
        radius: i32,
        see_through: bool,
    ) -> windows::core::Result<()> {
        let Some(outer) = OuterShadow::in_bands(shadow, rect, self.dpi, shadow::COARSE) else {
            return Ok(());
        };
        // Everything the layers cover, and a pixel more for their antialiased corners.
        let grown = outer.spread + outer.layers.reach() + 1;
        let reach = self.shape(rect, 0, grown, outer.drop).rect;
        let mask = if see_through {
            let hole = geometry_of(self.factory, &self.shape(rect, radius, 0, 0))?;
            // SAFETY: the rectangle outlives the call, which keeps nothing of it.
            let all = unsafe { self.factory.CreateRectangleGeometry(&reach) }?;
            Some(excluding(self.factory, &all, &hole)?.cast()?)
        } else {
            None
        };
        in_layer(self.target, reach, mask, || {
            self.outer_layers(&outer, shadow.color.rgb, rect, radius, None)
        })
    }

    /// An outer shadow of the box `rect`: the box moved down by the shadow's drop and grown by its spread, blurred.
    /// `hole`, when there is one, is cut out of every layer.
    fn outer_shadow(
        &self,
        shadow: &Shadow,
        rect: &RECT,
        radius: i32,
        hole: Option<&ID2D1RoundedRectangleGeometry>,
    ) -> windows::core::Result<()> {
        match OuterShadow::of(shadow, rect, self.dpi) {
            Some(outer) => self.outer_layers(&outer, shadow.color.rgb, rect, radius, hole),
            None => Ok(()),
        }
    }

    /// The layers of `outer`, a shadow in `rgb` of the box `rect`, filled on the target, with `hole` cut out of each.
    fn outer_layers(
        &self,
        outer: &OuterShadow,
        rgb: [u8; 3],
        rect: &RECT,
        radius: i32,
        hole: Option<&ID2D1RoundedRectangleGeometry>,
    ) -> windows::core::Result<()> {
        let (spread, drop) = (outer.spread, outer.drop);
        for layer in &outer.layers.layers {
            let grow = spread + layer.grow;
            // A layer that stays inside the box is under it, or cut out of it: never seen.
            if grow <= -drop.abs() {
                continue;
            }
            self.set(rgb, layer.opacity);
            let layer = self.shape(rect, radius, grow, drop);
            match hole {
                Some(hole) => fill_except(self.factory, self.target, &layer, hole, &self.brush)?,
                // SAFETY: a drawing call on a live render target with arguments that outlive it, as in `fill`.
                None => unsafe { self.target.FillRoundedRectangle(&layer, &self.brush) },
            }
        }
        Ok(())
    }

    /// An inset shadow of the box `padding`: the box less its own shape moved by the shadow's drop, smaller by its
    /// spread, blurred. The blur moves the hole's outline in and out, a layer at a time, and what is filled is always
    /// the box less the hole, so nothing reaches beyond the box.
    fn inset_shadow(
        &self,
        shadow: &Shadow,
        padding: &RECT,
        radius: i32,
    ) -> windows::core::Result<()> {
        let spread = self.signed(shadow.spread);
        let drop = self.signed(shadow.y);
        let layers = shadow::layers(
            sigma(shadow.blur, self.dpi),
            shadow.color.alpha,
            Profile::Edge,
        );
        let inside = self.shape(padding, radius, 0, 0);
        for layer in &layers.layers {
            // A layer's reach is how far into the hole the shadow goes: the hole is that much smaller.
            let hole = geometry_of(
                self.factory,
                &self.shape(padding, radius, -spread - layer.grow, drop),
            )?;
            self.set(shadow.color.rgb, layer.opacity);
            fill_except(self.factory, self.target, &inside, &hole, &self.brush)?;
        }
        Ok(())
    }

    /// `shape` filled with `stops` running from `top` to `bottom`, in the target's pixels. Colors blend in sRGB, as
    /// CSS does, and the first and last colors go on past the ends.
    pub fn gradient(
        &self,
        shape: &D2D1_ROUNDED_RECT,
        stops: &[Stop],
        top: f32,
        bottom: f32,
    ) -> windows::core::Result<()> {
        let brush = gradient(self.target, stops, shape.rect.left, top, bottom)?;
        // SAFETY: a drawing call on a live render target with arguments that outlive it, as in `fill`.
        unsafe { self.target.FillRoundedRectangle(shape, &brush) };
        Ok(())
    }
}

/// A brush for `target` of `stops` running down from `top` to `bottom` at `x`, in the target's pixels. Colors blend
/// in sRGB, as CSS does, and the first and last colors go on past the ends. Settings' and the tray icon's menu's
/// background is painted with it too (`look`).
pub fn gradient(
    target: &ID2D1RenderTarget,
    stops: &[Stop],
    x: f32,
    top: f32,
    bottom: f32,
) -> windows::core::Result<ID2D1LinearGradientBrush> {
    let stops: Vec<D2D1_GRADIENT_STOP> = stops
        .iter()
        .map(|stop| D2D1_GRADIENT_STOP {
            position: stop.at,
            color: d2d_color(stop.color.rgb, stop.color.alpha),
        })
        .collect();
    // SAFETY: the stops outlive the call, which copies them.
    let collection = unsafe {
        target.CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)
    }?;
    let mut line = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES::default();
    line.startPoint.X = x;
    line.startPoint.Y = top;
    line.endPoint.X = x;
    line.endPoint.Y = bottom;
    // SAFETY: the line and the collection outlive the call, which keeps its own reference to the collection.
    unsafe { target.CreateLinearGradientBrush(&line, None, &collection) }
}
