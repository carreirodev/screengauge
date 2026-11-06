//! For tests: drawings made in software on a bitmap, with no window, monitor or graphics card, read back as pixels.
//! Settings', the tray icon's menu's and the frozen screen's overlay's pixel tests all draw on it.

use std::ptr;

use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_FEATURE_LEVEL_DEFAULT, D2D1_RENDER_TARGET_PROPERTIES,
    D2D1_RENDER_TARGET_TYPE_SOFTWARE, D2D1_RENDER_TARGET_USAGE_NONE, D2D1CreateFactory,
    ID2D1Factory, ID2D1RenderTarget,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_WICPixelFormat32bppPBGRA, IWICBitmap, IWICImagingFactory,
    WICBitmapCacheOnDemand,
};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};

use crate::com::TestCom;

/// What was drawn, as rows of [r, g, b] pixels, and as a PNG would hold it: blue, green, red and alpha.
pub struct Picture {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<[u8; 3]>,
    pub bgra: Vec<u8>,
}

impl Picture {
    pub fn at(&self, x: i32, y: i32) -> [u8; 3] {
        let x = usize::try_from(x).expect("x inside the picture");
        let y = usize::try_from(y).expect("y inside the picture");
        let width = usize::try_from(self.width).expect("a small picture");
        self.pixels[y * width + x]
    }

    /// Writes it as a PNG file at `path`, for a person to look at.
    pub fn write_png(self, path: &std::path::Path) {
        let image = screengauge_core::session::Image::new(self.width, self.height, self.bgra)
            .expect("the bitmap's size");
        // On a thread of its own, which the encoder starts COM on in its own way.
        let png = std::thread::spawn(move || crate::png::encode(&image))
            .join()
            .expect("the encoder's thread")
            .expect("the picture as a PNG");
        std::fs::write(path, png).expect("the PNG");
    }
}

/// Whether every channel of `pixel` is within `within` of `color`'s.
pub fn near(pixel: [u8; 3], color: [u8; 3], within: u8) -> bool {
    pixel
        .iter()
        .zip(color)
        .all(|(&have, want)| have.abs_diff(want) <= within)
}

/// A `width` by `height` bitmap at 96 DPI, so one unit is one pixel, with a render target that draws on it in software.
/// It can be drawn on again, and read back after each drawing. COM, which the bitmap needs, is started for the test's
/// thread and ended once everything made with it is dropped.
pub struct Canvas {
    pub target: ID2D1RenderTarget,
    pub factory: ID2D1Factory,
    bitmap: IWICBitmap,
    pub width: u32,
    pub height: u32,
    /// Last, so it drops last.
    _com: TestCom,
}

impl Canvas {
    pub fn new(width: u32, height: u32) -> Canvas {
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
            factory,
            bitmap,
            width,
            height,
            _com: com,
        }
    }

    /// Draws with `draw` between BeginDraw and EndDraw, and reads the bitmap back.
    pub fn draw(&self, draw: impl FnOnce(&ID2D1RenderTarget, &ID2D1Factory)) -> Picture {
        // SAFETY: BeginDraw and EndDraw around the drawing, on a live target.
        unsafe { self.target.BeginDraw() };
        draw(&self.target, &self.factory);
        // SAFETY: ends the BeginDraw above.
        unsafe { self.target.EndDraw(None, None) }.expect("the drawing is finished");
        self.read()
    }

    /// The bitmap as it is now.
    pub fn read(&self) -> Picture {
        let stride = self.width * 4;
        let mut bgra = vec![0; usize::try_from(stride * self.height).expect("a small bitmap")];
        // SAFETY: no rectangle copies the whole bitmap, into a buffer of exactly its size.
        unsafe { self.bitmap.CopyPixels(ptr::null(), stride, &mut bgra) }
            .expect("the pixels are read");
        let pixels = bgra
            .as_chunks::<4>()
            .0
            .iter()
            .map(|&[b, g, r, _]| [r, g, b])
            .collect();
        Picture {
            width: self.width,
            height: self.height,
            pixels,
            bgra,
        }
    }
}

/// Draws with `draw` on a fresh `width` by `height` canvas, and reads it back.
pub fn drawn(
    width: u32,
    height: u32,
    draw: impl FnOnce(&ID2D1RenderTarget, &ID2D1Factory),
) -> Picture {
    Canvas::new(width, height).draw(draw)
}
