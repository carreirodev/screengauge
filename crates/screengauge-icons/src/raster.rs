//! Draws layers of shapes into a picture with Direct2D, on the CPU through a WIC bitmap, the same anti-aliasing and
//! gradients ScreenGauge draws with on screen.

use std::cell::Cell;

use windows::Win32::Foundation::E_FAIL;
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_FIGURE_BEGIN_FILLED, D2D1_FIGURE_END_CLOSED,
    D2D1_FILL_MODE_ALTERNATE, D2D1_GRADIENT_STOP, D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_CAP_STYLE_FLAT, D2D1_DASH_STYLE_SOLID, D2D1_EXTEND_MODE_CLAMP,
    D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_FEATURE_LEVEL_DEFAULT, D2D1_GAMMA_2_2,
    D2D1_LINE_JOIN_ROUND, D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES, D2D1_RENDER_TARGET_PROPERTIES,
    D2D1_RENDER_TARGET_TYPE_SOFTWARE, D2D1_RENDER_TARGET_USAGE_NONE, D2D1_STROKE_STYLE_PROPERTIES,
    D2D1CreateFactory, ID2D1Brush, ID2D1Factory, ID2D1PathGeometry, ID2D1RenderTarget,
    ID2D1StrokeStyle,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_WICPixelFormat32bppPBGRA, IWICImagingFactory,
    WICBitmapCacheOnLoad, WICBitmapLockRead, WICRect,
};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
};
use windows::core::{Error, Interface};

use crate::art::{Layer, Paint, Point, Shape};
use crate::image::{Image, Rgba, unit_to_byte, widen};

/// Premultiplied RGBA, one f32 per channel from 0 to 1, rows from the top.
type Premultiplied = Vec<[f32; 4]>;

/// Draws `layers` on a transparent `size` × `size` picture: each layer is drawn, blurred when it asks, then laid
/// over the ones before.
pub fn render(layers: &[Layer], size: u32) -> windows::core::Result<Image> {
    let mut picture = vec![[0.0; 4]; widen(size) * widen(size)];
    for layer in layers {
        let mut drawn = draw(&layer.shapes, size)?;
        if layer.blur > 0.0 {
            drawn = blur(&drawn, size, layer.blur);
        }
        for (below, above) in picture.iter_mut().zip(&drawn) {
            *below = over(*above, *below);
        }
    }
    let rgba = picture.iter().flat_map(|p| straight(*p)).collect();
    Image::from_rgba(size, size, rgba)
        .ok_or_else(|| Error::new(E_FAIL, "the picture has the wrong number of bytes"))
}

/// Makes COM ready on this thread, once. WIC needs it; the multithreaded apartment is shared by every thread.
pub fn ensure_com() -> windows::core::Result<()> {
    thread_local! {
        static READY: Cell<bool> = const { Cell::new(false) };
    }
    if READY.get() {
        return Ok(());
    }
    // SAFETY: no reserved pointer. COM stays initialized until the thread ends, which is what the generator and the
    // tests need.
    unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.ok()?;
    READY.set(true);
    Ok(())
}

pub fn wic() -> windows::core::Result<IWICImagingFactory> {
    ensure_com()?;
    // SAFETY: a registered in-process COM class, created with no outer object.
    unsafe { CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER) }
}

/// Draws `shapes` with Direct2D into a premultiplied picture.
fn draw(shapes: &[Shape], size: u32) -> windows::core::Result<Premultiplied> {
    let wic = wic()?;
    // SAFETY: a new bitmap that this function owns, in the format Direct2D draws into.
    let bitmap = unsafe {
        wic.CreateBitmap(
            size,
            size,
            &GUID_WICPixelFormat32bppPBGRA,
            WICBitmapCacheOnLoad,
        )
    }?;
    // SAFETY: no options; the factory is used on this thread only.
    let factory: ID2D1Factory =
        unsafe { D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None) }?;
    // At 96 DPI one Direct2D unit is one pixel. A WIC bitmap is always drawn on the CPU.
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
    // SAFETY: `bitmap` is a live WIC bitmap in a format Direct2D supports; `properties` outlives the call.
    let target = unsafe { factory.CreateWicBitmapRenderTarget(&bitmap, &properties) }?;
    let round = round_joins(&factory)?;

    // SAFETY: drawing calls on a live render target, between BeginDraw and EndDraw; every geometry and brush comes
    // from the same factory and target.
    unsafe {
        target.BeginDraw();
        target.Clear(Some(&color_f(Rgba::TRANSPARENT)));
    }
    for shape in shapes {
        match shape {
            Shape::Fill { figures, paint } => {
                let geometry = geometry(&factory, figures)?;
                let brush = brush(&target, paint)?;
                // SAFETY: as above.
                unsafe { target.FillGeometry(&geometry, &brush, None::<&ID2D1Brush>) };
            }
            Shape::Stroke {
                figures,
                width,
                paint,
            } => {
                let geometry = geometry(&factory, figures)?;
                let brush = brush(&target, paint)?;
                // SAFETY: as above.
                unsafe { target.DrawGeometry(&geometry, &brush, *width, &round) };
            }
        }
    }
    // SAFETY: ends the BeginDraw above.
    unsafe { target.EndDraw(None, None) }?;

    let whole = WICRect {
        X: 0,
        Y: 0,
        Width: i32::try_from(size).map_err(|_| Error::new(E_FAIL, "the picture is too large"))?,
        Height: i32::try_from(size).map_err(|_| Error::new(E_FAIL, "the picture is too large"))?,
    };
    // SAFETY: reads the whole bitmap, which nothing else holds now that drawing ended.
    let lock = unsafe { bitmap.Lock(&whole, WICBitmapLockRead.0.cast_unsigned()) }?;
    let stride = widen(
        // SAFETY: a live lock.
        unsafe { lock.GetStride() }?,
    );
    let mut length = 0;
    let mut data = std::ptr::null_mut();
    // SAFETY: a live lock fills both outputs.
    unsafe { lock.GetDataPointer(&mut length, &mut data) }?;
    if data.is_null() || widen(length) < stride * widen(size) {
        return Err(Error::new(E_FAIL, "the bitmap lock returned too few bytes"));
    }
    // SAFETY: the lock grants `length` readable bytes at `data` until it is dropped, after this copy.
    let bytes = unsafe { std::slice::from_raw_parts(data, widen(length)) };
    let mut pixels = Vec::with_capacity(widen(size) * widen(size));
    for row in bytes.chunks_exact(stride).take(widen(size)) {
        for &[b, g, r, a] in row.as_chunks::<4>().0.iter().take(widen(size)) {
            pixels.push([r, g, b, a].map(|channel| f32::from(channel) / 255.0));
        }
    }
    Ok(pixels)
}

/// The stroke style SVG's `stroke-linejoin="round"` asks for.
fn round_joins(factory: &ID2D1Factory) -> windows::core::Result<ID2D1StrokeStyle> {
    let properties = D2D1_STROKE_STYLE_PROPERTIES {
        startCap: D2D1_CAP_STYLE_FLAT,
        endCap: D2D1_CAP_STYLE_FLAT,
        dashCap: D2D1_CAP_STYLE_FLAT,
        lineJoin: D2D1_LINE_JOIN_ROUND,
        miterLimit: 4.0,
        dashStyle: D2D1_DASH_STYLE_SOLID,
        dashOffset: 0.0,
    };
    // SAFETY: `properties` outlives the call; no dashes.
    unsafe { factory.CreateStrokeStyle(&properties, None) }
}

/// Closed figures as one geometry, filled by the even-odd rule.
fn geometry(
    factory: &ID2D1Factory,
    figures: &[Vec<Point>],
) -> windows::core::Result<ID2D1PathGeometry> {
    // SAFETY: a new geometry from a live factory.
    let geometry = unsafe { factory.CreatePathGeometry() }?;
    // SAFETY: an open geometry accepts figures until Close.
    let sink = unsafe { geometry.Open() }?;
    // SAFETY: a live sink, before Close.
    unsafe { sink.SetFillMode(D2D1_FILL_MODE_ALTERNATE) };
    // Direct2D takes points as `windows_numerics::Vector2`, which the windows crate doesn't re-export; naming it would
    // take a new direct dependency, so a zero point is taken from a struct field that holds one.
    let zero = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES::default().startPoint;
    let vector = |p: &Point| {
        let mut v = zero;
        v.X = p.x;
        v.Y = p.y;
        v
    };
    for figure in figures {
        let points: Vec<_> = figure.iter().map(vector).collect();
        let Some((first, rest)) = points.split_first() else {
            continue;
        };
        // SAFETY: a live sink, before Close; each figure is begun and ended.
        unsafe {
            sink.BeginFigure(*first, D2D1_FIGURE_BEGIN_FILLED);
            sink.AddLines(rest);
            sink.EndFigure(D2D1_FIGURE_END_CLOSED);
        }
    }
    // SAFETY: closes the sink opened above.
    unsafe { sink.Close() }?;
    Ok(geometry)
}

fn brush(target: &ID2D1RenderTarget, paint: &Paint) -> windows::core::Result<ID2D1Brush> {
    match paint {
        Paint::Solid(color) => {
            // SAFETY: a live target; the color outlives the call.
            let brush = unsafe { target.CreateSolidColorBrush(&color_f(*color), None) }?;
            brush.cast()
        }
        Paint::Linear { from, to, stops } => {
            let stops: Vec<D2D1_GRADIENT_STOP> = stops
                .iter()
                .map(|(position, color)| D2D1_GRADIENT_STOP {
                    position: *position,
                    color: color_f(*color),
                })
                .collect();
            // SVG mixes gradient colors in sRGB, which Direct2D calls gamma 2.2.
            // SAFETY: a live target; the stops are copied by the call.
            let collection = unsafe {
                target.CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)
            }?;
            let mut line = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES::default();
            line.startPoint.X = from.x;
            line.startPoint.Y = from.y;
            line.endPoint.X = to.x;
            line.endPoint.Y = to.y;
            // SAFETY: a live target; `line` outlives the call.
            let brush = unsafe { target.CreateLinearGradientBrush(&line, None, &collection) }?;
            brush.cast()
        }
    }
}

fn color_f(color: Rgba) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: f32::from(color.r) / 255.0,
        g: f32::from(color.g) / 255.0,
        b: f32::from(color.b) / 255.0,
        a: f32::from(color.a) / 255.0,
    }
}

/// `above` laid over `below`, both premultiplied.
fn over(above: [f32; 4], below: [f32; 4]) -> [f32; 4] {
    let keep = 1.0 - above[3];
    [0, 1, 2, 3].map(|i| above[i] + below[i] * keep)
}

/// A premultiplied pixel as straight RGBA bytes.
fn straight(p: [f32; 4]) -> [u8; 4] {
    let alpha = p[3];
    if alpha <= 0.0 {
        return [0; 4];
    }
    [
        unit_to_byte(p[0] / alpha),
        unit_to_byte(p[1] / alpha),
        unit_to_byte(p[2] / alpha),
        unit_to_byte(alpha),
    ]
}

/// A Gaussian blur with standard deviation `sigma` pixels, one direction after the other; what falls off the edge
/// is lost, as when SVG blurs inside its filter region.
fn blur(pixels: &Premultiplied, size: u32, sigma: f32) -> Premultiplied {
    let radius = (sigma * 3.0).ceil();
    let kernel: Vec<f32> = {
        let raw: Vec<f32> = (0..=radius_steps(radius) * 2)
            .map(|i| {
                let d = step_f32(i) - radius;
                (-(d * d) / (2.0 * sigma * sigma)).exp()
            })
            .collect();
        let total: f32 = raw.iter().sum();
        raw.iter().map(|w| w / total).collect()
    };
    let n = widen(size);
    let reach = radius_steps(radius);
    let pass = |source: &Premultiplied, horizontal: bool| -> Premultiplied {
        let mut out = vec![[0.0; 4]; n * n];
        for y in 0..n {
            for x in 0..n {
                let mut sum = [0.0; 4];
                for (i, weight) in kernel.iter().enumerate() {
                    let along = if horizontal { x } else { y };
                    let Some(at) = (along + i).checked_sub(reach).filter(|&at| at < n) else {
                        continue;
                    };
                    let (sx, sy) = if horizontal { (at, y) } else { (x, at) };
                    let p = source[sy * n + sx];
                    for c in 0..4 {
                        sum[c] += p[c] * weight;
                    }
                }
                out[y * n + x] = sum;
            }
        }
        out
    };
    pass(&pass(pixels, true), false)
}

/// A blur radius in whole pixels, as a count. The radius is small and already whole, so the cast is exact.
fn radius_steps(radius: f32) -> usize {
    debug_assert!((0.0..1e6).contains(&radius) && radius.fract() == 0.0);
    radius as usize
}

/// A kernel index as a distance. Kernels are a few dozen taps, which f32 holds exactly.
fn step_f32(i: usize) -> f32 {
    debug_assert!(i < 1 << 24);
    i as f32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::art::Layer;

    fn square(color: Rgba, from: f32, to: f32) -> Shape {
        Shape::Fill {
            figures: vec![vec![
                Point { x: from, y: from },
                Point { x: to, y: from },
                Point { x: to, y: to },
                Point { x: from, y: to },
            ]],
            paint: Paint::Solid(color),
        }
    }

    #[test]
    fn a_shape_on_whole_pixels_has_sharp_edges() {
        let red = Rgba::hex(0xFF0000);
        let picture = render(
            &[Layer {
                shapes: vec![square(red, 2.0, 6.0)],
                blur: 0.0,
            }],
            8,
        )
        .expect("render");
        assert_eq!(picture.pixel(2, 2), red);
        assert_eq!(picture.pixel(5, 5), red);
        assert_eq!(picture.pixel(1, 2), Rgba::TRANSPARENT);
        assert_eq!(picture.pixel(6, 5), Rgba::TRANSPARENT);
    }

    #[test]
    fn a_later_layer_lies_over_an_earlier_one() {
        let (red, blue) = (Rgba::hex(0xFF0000), Rgba::hex(0x0000FF));
        let layer = |shape| Layer {
            shapes: vec![shape],
            blur: 0.0,
        };
        let picture = render(
            &[layer(square(red, 0.0, 4.0)), layer(square(blue, 2.0, 4.0))],
            4,
        )
        .expect("render");
        assert_eq!(picture.pixel(0, 0), red);
        assert_eq!(picture.pixel(3, 3), blue);
    }

    #[test]
    fn a_blur_spreads_and_keeps_the_total() {
        let picture = render(
            &[Layer {
                shapes: vec![square(Rgba::hex(0x000000), 7.0, 9.0)],
                blur: 1.5,
            }],
            16,
        )
        .expect("render");
        let total: u32 = picture.pixels().map(|p| u32::from(p.a)).sum();
        // Four opaque pixels' worth of alpha, give or take rounding.
        assert!((1000..=1040).contains(&total), "{total}");
        assert!(picture.pixel(5, 8).a > 0);
        assert!(picture.pixel(8, 8).a < 255);
    }
}
