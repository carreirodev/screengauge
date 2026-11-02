//! What every surface ScreenGauge draws itself shares, the frozen screen's windows and Settings alike: the Direct2D
//! factory, DirectWrite's factory with what ScreenGauge needs to know about its font, and colors as CSS writes them.
//! The factories are made at their first use and kept until ScreenGauge exits.

use std::cell::RefCell;

use windows::Win32::Foundation::{E_FAIL, HWND};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_SIZE_U, D2D1_ALPHA_MODE_IGNORE, D2D1_COLOR_F, D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_FEATURE_LEVEL_DEFAULT,
    D2D1_HWND_RENDER_TARGET_PROPERTIES, D2D1_PRESENT_OPTIONS_NONE, D2D1_RENDER_TARGET_PROPERTIES,
    D2D1_RENDER_TARGET_TYPE_DEFAULT, D2D1_RENDER_TARGET_USAGE_NONE, D2D1CreateFactory,
    ID2D1Factory, ID2D1HwndRenderTarget, ID2D1RenderTarget, ID2D1SolidColorBrush,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_METRICS, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE,
    DWRITE_FONT_STYLE_ITALIC, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT,
    DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_WORD_WRAPPING_NO_WRAP, DWriteCreateFactory,
    IDWriteFactory, IDWriteFontFamily, IDWriteTextFormat,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::core::{BOOL, Error, PCWSTR, w};

/// ScreenGauge's one font, as the prototype's CSS names it.
pub const FAMILY: PCWSTR = w!("Segoe UI");

thread_local! {
    /// Made at the first call and kept until ScreenGauge exits, so nothing exists before the first call. The factory
    /// keeps the graphics device its windows draw with, and with it the graphics card's driver. On the owner's
    /// computer, a factory made for each call made every call about 350 ms slower, past the 250 ms target (WPC-286);
    /// kept, ScreenGauge at rest after four calls used 47 MB in Task Manager, under the 80 MB target.
    static FACTORY: RefCell<Option<ID2D1Factory>> = const { RefCell::new(None) };
    /// Made at the first call and kept until ScreenGauge exits, like the Direct2D factory.
    static FONTS: RefCell<Option<Fonts>> = const { RefCell::new(None) };
}

/// The kept Direct2D factory, made at the first call.
pub fn factory() -> windows::core::Result<ID2D1Factory> {
    FACTORY.with_borrow_mut(|kept| {
        if let Some(factory) = kept {
            return Ok(factory.clone());
        }
        // SAFETY: a single-threaded factory with default options, used only on this thread.
        let factory: ID2D1Factory =
            unsafe { D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None) }?;
        Ok(kept.insert(factory).clone())
    })
}

/// Releases the kept Direct2D factory, and the graphics device with it, as ScreenGauge exits, and DirectWrite's factory
/// too. Left to the end of the process, the release happened while Windows was already taking it down: the exit took 3
/// seconds and failed. Every window that draws is destroyed first.
pub fn release() {
    drop(FACTORY.with_borrow_mut(Option::take));
    drop(FONTS.with_borrow_mut(Option::take));
}

/// A render target of `size` for the opaque window `hwnd`, Settings or the tray icon's menu, at 96 DPI so one
/// Direct2D unit is one physical pixel at any scale.
pub fn window_target(hwnd: HWND, size: D2D_SIZE_U) -> windows::core::Result<ID2D1HwndRenderTarget> {
    let target_properties = D2D1_RENDER_TARGET_PROPERTIES {
        r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
        pixelFormat: D2D1_PIXEL_FORMAT {
            format: DXGI_FORMAT_B8G8R8A8_UNORM,
            alphaMode: D2D1_ALPHA_MODE_IGNORE,
        },
        dpiX: 96.0,
        dpiY: 96.0,
        usage: D2D1_RENDER_TARGET_USAGE_NONE,
        minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
    };
    let window_properties = D2D1_HWND_RENDER_TARGET_PROPERTIES {
        hwnd,
        pixelSize: size,
        presentOptions: D2D1_PRESENT_OPTIONS_NONE,
    };
    // SAFETY: both property structs outlive the call.
    unsafe { factory()?.CreateHwndRenderTarget(&target_properties, &window_properties) }
}

/// DirectWrite's factory and what ScreenGauge needs to know about its font.
#[derive(Clone)]
pub struct Fonts {
    pub factory: IDWriteFactory,
    /// The height of the font's capital letters, and of its digits, as a fraction of the font size.
    pub cap_height: f32,
}

impl Fonts {
    pub fn new() -> windows::core::Result<Fonts> {
        // SAFETY: no pointers; the factory comes back as an owned interface.
        let factory: IDWriteFactory = unsafe { DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED) }?;
        let family = font_family(&factory)?;
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

    /// The font in `weight`, `size` pixels tall, on one line whatever the room.
    pub fn text_format(
        &self,
        weight: DWRITE_FONT_WEIGHT,
        size: f32,
    ) -> windows::core::Result<IDWriteTextFormat> {
        self.styled_format(weight, DWRITE_FONT_STYLE_NORMAL, size)
    }

    /// The font in `weight`, italic, `size` pixels tall, on one line whatever the room.
    pub fn italic_format(
        &self,
        weight: DWRITE_FONT_WEIGHT,
        size: f32,
    ) -> windows::core::Result<IDWriteTextFormat> {
        self.styled_format(weight, DWRITE_FONT_STYLE_ITALIC, size)
    }

    fn styled_format(
        &self,
        weight: DWRITE_FONT_WEIGHT,
        style: DWRITE_FONT_STYLE,
        size: f32,
    ) -> windows::core::Result<IDWriteTextFormat> {
        // SAFETY: plain values and static strings.
        let format = unsafe {
            self.factory.CreateTextFormat(
                FAMILY,
                None,
                weight,
                style,
                DWRITE_FONT_STRETCH_NORMAL,
                size,
                w!("en-US"),
            )
        }?;
        // Every text ScreenGauge draws is one line, so the room it has is never the reason to break it.
        // SAFETY: plain value.
        unsafe { format.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP) }?;
        Ok(format)
    }
}

/// The installed family of the font.
pub fn font_family(factory: &IDWriteFactory) -> windows::core::Result<IDWriteFontFamily> {
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
    unsafe { collection.GetFontFamily(index) }
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

/// An sRGB color, with the opacity CSS gives it in `rgba()`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Color {
    pub rgb: [u8; 3],
    pub alpha: f32,
}

pub const fn color(r: u8, g: u8, b: u8, alpha: f32) -> Color {
    Color {
        rgb: [r, g, b],
        alpha,
    }
}

/// `color` laid at its opacity on the opaque `under`, as the screen shows it.
pub fn over(color: Color, under: [u8; 3]) -> [u8; 3] {
    let mut mixed = [0; 3];
    for ((out, top), bottom) in mixed.iter_mut().zip(color.rgb).zip(under) {
        let value = f32::from(bottom) + (f32::from(top) - f32::from(bottom)) * color.alpha;
        // A channel mixed from two channels stays between them, inside a byte, so the cast keeps the whole value.
        // TODO: mix in integers, so no float comes back to `u8` here.
        *out = value.round().clamp(0.0, 255.0) as u8;
    }
    mixed
}

/// `rgb` at `alpha`, as Direct2D takes a color.
pub fn d2d_color(rgb: [u8; 3], alpha: f32) -> D2D1_COLOR_F {
    let [r, g, b] = rgb;
    D2D1_COLOR_F {
        r: f32::from(r) / 255.0,
        g: f32::from(g) / 255.0,
        b: f32::from(b) / 255.0,
        a: alpha,
    }
}

/// A solid brush of `color` for `target`, which a render target that is lost takes with it.
pub fn solid_brush(
    target: &ID2D1RenderTarget,
    color: Color,
) -> windows::core::Result<ID2D1SolidColorBrush> {
    let color = d2d_color(color.rgb, color.alpha);
    // SAFETY: `color` outlives the call, which keeps nothing of it.
    unsafe { target.CreateSolidColorBrush(&color, None) }
}
