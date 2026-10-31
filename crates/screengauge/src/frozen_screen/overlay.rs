//! Dimensions' live measurement drawn over a window's photo: the cyan line with its ticks and outline, and the label in
//! a pill. Everything about where it goes is in `geometry`, which needs no Windows; this draws it onto any Direct2D
//! render target at 96 DPI, so a test can render it on a bitmap and count pixels.

mod geometry;

use std::cell::RefCell;

use screengauge_core::session::{Measurement, Rect, Scale};
use windows::Win32::Foundation::{E_FAIL, RECT};
use windows::Win32::Graphics::Direct2D::Common::{D2D_RECT_F, D2D1_COLOR_F};
use windows::Win32::Graphics::Direct2D::{
    D2D1_ANTIALIAS_MODE_ALIASED, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_DRAW_TEXT_OPTIONS_NONE,
    D2D1_ELLIPSE, D2D1_ROUNDED_RECT, D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE, ID2D1RenderTarget,
    ID2D1SolidColorBrush,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_FEATURE, DWRITE_FONT_FEATURE_TAG_TABULAR_FIGURES,
    DWRITE_FONT_METRICS, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL,
    DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_LINE_METRICS, DWRITE_TEXT_METRICS, DWRITE_TEXT_RANGE,
    DWRITE_WORD_WRAPPING_NO_WRAP, DWriteCreateFactory, IDWriteFactory, IDWriteTextFormat,
    IDWriteTextLayout, IDWriteTypography,
};
use windows::core::{BOOL, Error, PCWSTR, w};

use geometry::scaled;

/// The label's font. Its digits differ in width unless asked for tabular figures (a 1 is two pixels narrower than a 0
/// at this size), which the label does, so the pill keeps its width while the number keeps its length.
const FAMILY: PCWSTR = w!("Segoe UI");

/// How much of the outline's black shows over the photo.
const OUTLINE_OPACITY: f32 = 0.28;

/// The pill's shadow, approximating CSS `0 2px 7px rgba(0,0,0,.35)`: black rounded rectangles that grow by this many
/// logical pixels and show this much, laid over each other, so the shadow fades outward from the pill.
const SHADOW: [(u32, f32); 5] = [(0, 0.05), (1, 0.04), (2, 0.04), (4, 0.03), (6, 0.02)];

/// DirectWrite's factory and what the label needs to know about its font. Kept across calls, like the Direct2D factory,
/// so opening the tools doesn't pay for them again.
#[derive(Clone)]
pub struct Fonts {
    factory: IDWriteFactory,
    /// The height of the font's capital letters, and of its digits, as a fraction of the font size.
    cap_height: f32,
}

impl Fonts {
    fn new() -> windows::core::Result<Fonts> {
        // SAFETY: a shared factory, which DirectWrite shares with every other user in the process.
        let factory: IDWriteFactory = unsafe { DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED) }?;
        let mut collection = None;
        // SAFETY: fills `collection` with the installed fonts; the cached list is good enough.
        unsafe { factory.GetSystemFontCollection(&mut collection, false) }?;
        let collection = collection
            .ok_or_else(|| Error::new(E_FAIL, "DirectWrite returned no collection of fonts"))?;
        let (mut index, mut exists) = (0, BOOL::default());
        // SAFETY: both outputs are locals, and the name is a static string.
        unsafe { collection.FindFamilyName(FAMILY, &mut index, &mut exists) }?;
        if !exists.as_bool() {
            return Err(Error::new(E_FAIL, "Segoe UI is not installed"));
        }
        // SAFETY: `index` came from FindFamilyName just above.
        let family = unsafe { collection.GetFontFamily(index) }?;
        // SAFETY: plain values.
        let font = unsafe {
            family.GetFirstMatchingFont(
                DWRITE_FONT_WEIGHT_SEMI_BOLD,
                DWRITE_FONT_STRETCH_NORMAL,
                DWRITE_FONT_STYLE_NORMAL,
            )
        }?;
        let mut metrics = DWRITE_FONT_METRICS::default();
        // SAFETY: fills a local.
        unsafe { font.GetMetrics(&mut metrics) };
        if metrics.designUnitsPerEm == 0 {
            return Err(Error::new(E_FAIL, "Segoe UI reports an em of no units"));
        }
        Ok(Fonts {
            factory,
            cap_height: f32::from(metrics.capHeight) / f32::from(metrics.designUnitsPerEm),
        })
    }
}

thread_local! {
    /// Made at the first call and kept until ScreenGauge exits, like the Direct2D factory in the parent module.
    static FONTS: RefCell<Option<Fonts>> = const { RefCell::new(None) };
}

/// The kept fonts, made at the first call.
pub fn fonts() -> windows::core::Result<Fonts> {
    FONTS.with_borrow_mut(|kept| {
        if let Some(fonts) = kept {
            return Ok(fonts.clone());
        }
        Ok(kept.insert(Fonts::new()?).clone())
    })
}

/// Releases the kept fonts as ScreenGauge exits, together with the Direct2D factory.
pub fn release_fonts() {
    drop(FONTS.with_borrow_mut(Option::take));
}

/// What draws the live measurement on one monitor's window: brushes and a text format made for its render target and
/// its scale. A render target that is lost takes its brushes with it, so this is made again with everything else.
pub struct Overlay {
    cyan: ID2D1SolidColorBrush,
    /// Black. Each fill sets its opacity first: the outline's, then each layer of the shadow's.
    shade: ID2D1SolidColorBrush,
    ink: ID2D1SolidColorBrush,
    format: IDWriteTextFormat,
    /// Asks for tabular figures; applied to each label's text.
    figures: IDWriteTypography,
    fonts: Fonts,
    /// The window's monitor in the virtual screen, which is where the window's pixel (0, 0) is.
    monitor: RECT,
    dpi: u32,
}

impl Overlay {
    /// For a window over `monitor`, whose scale is `scale`, drawing on `target`.
    pub fn new(
        target: &ID2D1RenderTarget,
        fonts: &Fonts,
        monitor: Rect,
        scale: Scale,
    ) -> windows::core::Result<Overlay> {
        let dpi = scale.dpi();
        let brush = |[r, g, b]: [u8; 3]| {
            let color = D2D1_COLOR_F {
                r: f32::from(r) / 255.0,
                g: f32::from(g) / 255.0,
                b: f32::from(b) / 255.0,
                a: 1.0,
            };
            // SAFETY: `color` outlives the call, which keeps nothing of it.
            unsafe { target.CreateSolidColorBrush(&color, None) }
        };
        // SAFETY: plain values and static strings. A size that is a whole number of pixels keeps the digits crisp.
        let format = unsafe {
            fonts.factory.CreateTextFormat(
                FAMILY,
                None,
                DWRITE_FONT_WEIGHT_SEMI_BOLD,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                px(scaled(geometry::TEXT, dpi)),
                w!("en-US"),
            )
        }?;
        // The label is one line, so the window's width is never the reason to break it.
        // SAFETY: plain value.
        unsafe { format.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP) }?;
        // SAFETY: plain values.
        let figures = unsafe { fonts.factory.CreateTypography() }?;
        // SAFETY: plain values.
        unsafe {
            figures.AddFontFeature(DWRITE_FONT_FEATURE {
                nameTag: DWRITE_FONT_FEATURE_TAG_TABULAR_FIGURES,
                parameter: 1,
            })
        }?;
        Ok(Overlay {
            // #22d3ee, the same cyan whatever the Windows theme.
            cyan: brush([0x22, 0xd3, 0xee])?,
            shade: brush([0, 0, 0])?,
            // #04161b
            ink: brush([0x04, 0x16, 0x1b])?,
            format,
            figures,
            fonts: fonts.clone(),
            monitor: geometry::bounds(monitor)?,
            dpi,
        })
    }

    /// Draws `measurement` over what `target` holds, between its BeginDraw and EndDraw. Order matters: every outline
    /// first and every cyan pixel after, so no outline darkens a cyan pixel, then the label above both. A failure to
    /// lay the label out is returned, once the line is drawn.
    pub fn draw(
        &self,
        target: &ID2D1RenderTarget,
        measurement: &Measurement,
    ) -> windows::core::Result<()> {
        self.draw_lines(target, measurement);
        self.draw_label(target, measurement).map(|_| ())
    }

    /// The line, the ticks and the outline.
    fn draw_lines(&self, target: &ID2D1RenderTarget, measurement: &Measurement) {
        let parts = geometry::parts(measurement, self.dpi);
        // SAFETY: drawing calls on a live render target, between its BeginDraw and EndDraw. Aliased fills of
        // rectangles on whole pixels cover exactly those pixels, and the target is at 96 DPI, so one unit is one
        // physical pixel. DrawLine is never used: on whole coordinates it straddles two pixels.
        unsafe {
            target.SetAntialiasMode(D2D1_ANTIALIAS_MODE_ALIASED);
            self.shade.SetOpacity(OUTLINE_OPACITY);
            for outline in &parts.outline {
                target.FillRectangle(&self.in_window(outline), &self.shade);
            }
            for line in &parts.line {
                target.FillRectangle(&self.in_window(line), &self.cyan);
            }
            // Back to Direct2D's default, which the pill's corners need.
            target.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
        }
    }

    /// The pill with its shadow and text. Returns the pill's place in the virtual screen, `None` when the measurement
    /// has no span, so there is nothing to label.
    fn draw_label(
        &self,
        target: &ID2D1RenderTarget,
        measurement: &Measurement,
    ) -> windows::core::Result<Option<RECT>> {
        let layout = self.layout(&measurement.label)?;
        let mut metrics = DWRITE_TEXT_METRICS::default();
        // SAFETY: fills a local.
        unsafe { layout.GetMetrics(&mut metrics) }?;
        let mut line = [DWRITE_LINE_METRICS::default()];
        let mut lines = 0;
        // SAFETY: fills a one-line buffer, and fails should the label need more.
        unsafe { layout.GetLineMetrics(Some(&mut line), &mut lines) }?;
        let [line] = line;

        let size = geometry::pill_size(whole_up(metrics.width), self.dpi);
        let Some(pill) = geometry::place_label(measurement, size, self.monitor, self.dpi) else {
            return Ok(None);
        };

        // The text sits so the middle of its digits is the middle of the pill, on a whole pixel: the layout's own
        // height counts room for letters that drop below the line, which a number never has.
        let font = px(scaled(geometry::TEXT, self.dpi));
        let window = self.in_window(&pill);
        let middle = (window.top + window.bottom) / 2.0;
        let baseline = (middle + self.fonts.cap_height * font / 2.0).round();
        let left = window.left + px(scaled(geometry::PAD_X, self.dpi));
        // DrawTextLayout takes a Vector2, which windows 0.62 takes from the windows-numerics crate without
        // re-exporting it, and this program may not add the crate. Direct2D's ellipse holds one.
        let mut origin = D2D1_ELLIPSE::default().point;
        origin.X = left;
        origin.Y = baseline - line.baseline;

        // SAFETY: drawing calls on a live render target, between its BeginDraw and EndDraw. The shapes are on whole
        // pixels, so their straight edges are crisp; the corners are antialiased.
        unsafe {
            target.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
            let drop = scaled(geometry::SHADOW_DROP, self.dpi);
            for (grow, opacity) in SHADOW {
                self.shade.SetOpacity(opacity);
                let shape = self.rounded(&pill, scaled(grow, self.dpi), drop);
                target.FillRoundedRectangle(&shape, &self.shade);
            }
            target.FillRoundedRectangle(&self.rounded(&pill, 0, 0), &self.cyan);
            // Grayscale, not ClearType: color fringes would show on the cyan.
            target.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
            target.DrawTextLayout(origin, &layout, &self.ink, D2D1_DRAW_TEXT_OPTIONS_NONE);
        }
        Ok(Some(pill))
    }

    /// `label` laid out on one line in the label's font, with tabular figures.
    fn layout(&self, label: &str) -> windows::core::Result<IDWriteTextLayout> {
        let text: Vec<u16> = label.encode_utf16().collect();
        let length = u32::try_from(text.len())
            .map_err(|_| Error::new(E_FAIL, "the label is too long to lay out"))?;
        // SAFETY: the layout copies the text and keeps the format. A width past any label and no height limit keep it
        // on one line.
        let layout = unsafe {
            self.fonts
                .factory
                .CreateTextLayout(&text, &self.format, 4096.0, 4096.0)
        }?;
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
        Ok(layout)
    }

    /// `rect`, from the virtual screen to the window's own pixels.
    fn in_window(&self, rect: &RECT) -> D2D_RECT_F {
        D2D_RECT_F {
            left: px(rect.left - self.monitor.left),
            top: px(rect.top - self.monitor.top),
            right: px(rect.right - self.monitor.left),
            bottom: px(rect.bottom - self.monitor.top),
        }
    }

    /// The pill's rounded rectangle, grown by `grow` pixels on every side and moved `down` pixels.
    fn rounded(&self, pill: &RECT, grow: i32, down: i32) -> D2D1_ROUNDED_RECT {
        let mut rect = self.in_window(pill);
        rect.left -= px(grow);
        rect.right += px(grow);
        rect.top += px(down - grow);
        rect.bottom += px(down + grow);
        let radius = px(scaled(geometry::RADIUS, self.dpi) + grow);
        D2D1_ROUNDED_RECT {
            rect,
            radiusX: radius,
            radiusY: radius,
        }
    }
}

/// A whole number of pixels as the float Direct2D takes. Positions are far below 2^24, up to which `f32` holds every
/// whole number, so nothing is lost.
fn px(pixels: i32) -> f32 {
    pixels as f32
}

/// A measured width rounded up to whole pixels, so the pill never cuts into the text. Widths are a few hundred pixels
/// at most, far inside `i32`.
fn whole_up(width: f32) -> i32 {
    width.ceil() as i32
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
        CLSID_WICImagingFactory, GUID_WICPixelFormat32bppPBGRA, IWICBitmap, IWICImagingFactory,
        WICBitmapCacheOnDemand,
    };
    use windows::Win32::System::Com::{
        CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
        CoUninitialize,
    };

    use super::geometry::Size;
    use super::*;

    const CYAN: [u8; 3] = [0x22, 0xd3, 0xee];
    const BACKGROUND: [u8; 3] = [200, 120, 60];

    /// Tests run on worker threads, where COM isn't started, and WIC needs it. Started for the thread by the first
    /// canvas and ended by the last object dropping, so every COM object is released before COM goes.
    struct Com;

    impl Com {
        fn start() -> Com {
            // SAFETY: starts COM for this thread, undone once by the drop of the Com it returns. A thread that
            // already started it in the same way gets S_FALSE, which is success.
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

    /// A bitmap a render target draws on in software, so no monitor or graphics card is needed.
    struct Canvas {
        target: ID2D1RenderTarget,
        bitmap: IWICBitmap,
        fonts: Fonts,
        width: u32,
        height: u32,
        /// Last, so it drops last.
        _com: Com,
    }

    /// The canvas as rows of [r, g, b] pixels.
    struct Picture {
        width: usize,
        pixels: Vec<[u8; 3]>,
    }

    impl Picture {
        fn at(&self, x: usize, y: usize) -> [u8; 3] {
            self.pixels[y * self.width + x]
        }
    }

    impl Canvas {
        fn new(width: u32, height: u32) -> Canvas {
            let com = Com::start();
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

        /// Fills the canvas with `BACKGROUND`, lets `draw` draw over it for a window of `monitor` and `dpi`, and
        /// reads the result back.
        fn render(
            &self,
            monitor: Rect,
            dpi: u32,
            draw: impl FnOnce(&Overlay, &ID2D1RenderTarget),
        ) -> Picture {
            let scale = Scale::from_dpi(dpi).expect("a scale");
            let overlay =
                Overlay::new(&self.target, &self.fonts, monitor, scale).expect("the overlay");
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
    const MONITOR: Rect = Rect {
        left: -480,
        top: 200,
        width: 480,
        height: 320,
    };

    fn point(x: i32, y: i32) -> screengauge_core::session::Point {
        screengauge_core::session::Point { x, y }
    }

    fn span(first: (i32, i32), last: (i32, i32)) -> screengauge_core::session::Span {
        screengauge_core::session::Span {
            first: point(first.0, first.1),
            last: point(last.0, last.1),
        }
    }

    fn horizontal(x0: i32, x1: i32, y: i32, label: &str) -> Measurement {
        Measurement {
            monitor: 0,
            horizontal: Some(span((x0, y), (x1, y))),
            vertical: None,
            label: label.to_owned(),
        }
    }

    fn vertical(y0: i32, y1: i32, x: i32, label: &str) -> Measurement {
        Measurement {
            monitor: 0,
            horizontal: None,
            vertical: Some(span((x, y0), (x, y1))),
            label: label.to_owned(),
        }
    }

    /// The Cross: a horizontal span along row `y` and a vertical one along column `x`.
    fn cross(h: (i32, i32), v: (i32, i32), x: i32, y: i32, label: &str) -> Measurement {
        Measurement {
            monitor: 0,
            horizontal: Some(span((h.0, y), (h.1, y))),
            vertical: Some(span((x, v.0), (x, v.1))),
            label: label.to_owned(),
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
    fn expected(m: &Measurement, k: i32, x: i32, y: i32) -> [u8; 3] {
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

    fn near(actual: [u8; 3], wanted: [u8; 3], tolerance: u8) -> bool {
        actual
            .iter()
            .zip(wanted)
            .all(|(&a, w)| a.abs_diff(w) <= tolerance)
    }

    /// Checks every pixel of the window against the model, the line alone with no label.
    fn check_line(canvas: &Canvas, m: &Measurement, dpi: u32) {
        let k = scaled(4, dpi);
        let picture = canvas.render(MONITOR, dpi, |overlay, target| {
            overlay.draw_lines(target, m);
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
    fn a_line_at_the_monitor_edge_is_cut_not_moved() {
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
            overlay.draw_lines(target, &m)
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
                overlay.draw_lines(target, &m)
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
            overlay.draw_lines(target, &m)
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
            overlay.draw_lines(target, &m)
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
                overlay.draw_lines(target, &m)
            });
            // (-230, 360) in the window.
            assert_eq!(picture.at(250, 160), CYAN, "at {dpi} DPI");
        }
    }

    /// The rows (top, bottom) and columns (left, right) of the text's pixels inside `pill`: those that are more than half
    /// the text's near-black, which neither the cyan, nor the photo showing at the rounded corners, nor the shadow is.
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
                pill = overlay.draw_label(target, &m).expect("the label");
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
            pill = overlay.draw_label(target, &m).expect("the label");
        });
        let pill = pill.expect("a pill");
        let at =
            |x: i32, y: i32| picture.at((x - MONITOR.left) as usize, (y - MONITOR.top) as usize);
        let centre = (pill.left + pill.right) / 2;
        // Just below the pill it darkens the photo, but by far less than the outline's 28%.
        let below = at(centre, pill.bottom + 1);
        assert!(below[0] < BACKGROUND[0], "{below:?}");
        assert!(below[0] > 150, "{below:?}");
        // Eight pixels beyond the pill's bottom edge, 2 for the drop and 6 for the blur, nothing is left.
        assert_eq!(at(centre, pill.bottom + 9), BACKGROUND);
        assert_eq!(at(pill.left - 7, (pill.top + pill.bottom) / 2), BACKGROUND);
    }

    #[test]
    fn the_cross_label_reads_width_by_height() {
        let canvas = Canvas::new(480, 320);
        let m = cross((-350, -100), (230, 480), -230, 360, "240 \u{d7} 180");
        let mut pill = None;
        canvas.render(MONITOR, 96, |overlay, target| {
            pill = overlay.draw_label(target, &m).expect("the label");
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
                overlay.draw(target, &m).expect("the measurement");
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
    fn the_label_stays_in_the_window_at_the_monitors_edge() {
        let canvas = Canvas::new(480, 320);
        // A short span in the corner: the label would stick out on two sides without sliding in.
        let m = horizontal(-480, -478, 202, "3");
        let mut pill = None;
        canvas.render(MONITOR, 168, |overlay, target| {
            pill = overlay.draw_label(target, &m).expect("the label");
        });
        let pill = pill.expect("a pill");
        assert!(pill.left >= MONITOR.left && pill.top >= MONITOR.top);
        assert!(pill.right <= MONITOR.left + 480 && pill.bottom <= MONITOR.top + 320);
    }

    #[test]
    fn the_labels_digits_are_as_wide_as_each_other() {
        // Without tabular figures Segoe UI's 1 is 4.8 pixels wide at this size and its 0 is 6.7, so the pill would
        // jump as the number changed. With them the widths agree to a fraction of a pixel (the font's 7 is 0.2 narrower).
        let canvas = Canvas::new(8, 8);
        let overlay = Overlay::new(
            &canvas.target,
            &canvas.fonts,
            MONITOR,
            Scale::from_dpi(96).expect("a scale"),
        )
        .expect("the overlay");
        let widths: Vec<f32> = "0123456789"
            .chars()
            .map(|digit| {
                let layout = overlay.layout(&digit.to_string()).expect("a layout");
                let mut metrics = DWRITE_TEXT_METRICS::default();
                // SAFETY: fills a local.
                unsafe { layout.GetMetrics(&mut metrics) }.expect("metrics");
                metrics.width
            })
            .collect();
        let widest = widths.iter().copied().fold(f32::MIN, f32::max);
        let narrowest = widths.iter().copied().fold(f32::MAX, f32::min);
        assert!(widest - narrowest < 0.3, "{widths:?}");
        assert!(narrowest > 6.0, "{widths:?}");
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
                pill = overlay.draw_label(target, &m).expect("the label");
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
}
