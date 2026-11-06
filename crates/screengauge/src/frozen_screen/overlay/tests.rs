//! What the overlay's tests share: a bitmap to draw on, the window they draw in, Dimensions' measurements to draw, and
//! what their pixels should be.

use std::ptr;

use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_FEATURE_LEVEL_DEFAULT, D2D1_RENDER_TARGET_PROPERTIES,
    D2D1_RENDER_TARGET_TYPE_SOFTWARE, D2D1_RENDER_TARGET_USAGE_NONE, D2D1CreateFactory,
    ID2D1Factory,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_WICPixelFormat32bppPBGRA, IWICBitmap, IWICImagingFactory,
    WICBitmapCacheOnDemand,
};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};

use super::*;
use crate::com::TestCom;
use screengauge_core::session::Measured;

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
        measured: Measured::Spans {
            horizontal: Some(span((x0, y), (x1, y))),
            vertical: None,
        },
        label: physical_only(label),
    }
}

pub(super) fn vertical(y0: i32, y1: i32, x: i32, label: &str) -> Measurement {
    Measurement {
        monitor: 0,
        measured: Measured::Spans {
            horizontal: None,
            vertical: Some(span((x, y0), (x, y1))),
        },
        label: physical_only(label),
    }
}

/// The Cross: a horizontal span along row `y` and a vertical one along column `x`.
pub(super) fn cross(h: (i32, i32), v: (i32, i32), x: i32, y: i32, label: &str) -> Measurement {
    Measurement {
        monitor: 0,
        measured: Measured::Spans {
            horizontal: Some(span((h.0, y), (h.1, y))),
            vertical: Some(span((x, v.0), (x, v.1))),
        },
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
    if let Some(h) = m.horizontal() {
        cyan |= on_line(h.first.x, h.last.x, h.first.y, k, x, y);
        outline |= beside_line(h.first.x, h.last.x, h.first.y, k, x, y);
    }
    if let Some(v) = m.vertical() {
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

pub(super) const INK: [u8; 3] = [0x04, 0x16, 0x1b];

pub(super) const SCALES: [u32; 4] = [96, 120, 144, 168];

/// `m` with `logical` as the logical part of its label.
pub(super) fn with_logical(mut m: Measurement, logical: &str) -> Measurement {
    m.label.logical = Some(logical.to_owned());
    m
}

/// The width of `text` laid out in `format`, in whole pixels, as the label counts it.
pub(super) fn text_width(overlay: &Overlay, text: &str, format: &IDWriteTextFormat) -> i32 {
    overlay.typeset(text, format).expect("a text").width
}
