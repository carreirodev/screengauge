//! Settings drawn onto any Direct2D render target at 96 DPI, so a test can render it on a bitmap and count pixels: the
//! window's background, each block's heading and empty card, and the footer with the version, the license and the
//! "Project page" link. The colors are the prototype's theme tokens for `#settings` (WPC-279), where the `oklch` ones are
//! converted to sRGB; the sizes come from `layout`, scaled for the monitor's `dpi`.

use screengauge_core::session::toolbar::scaled;
use windows::Win32::Foundation::{E_FAIL, RECT};
use windows::Win32::Graphics::Direct2D::Common::{D2D_RECT_F, D2D1_GRADIENT_STOP};
use windows::Win32::Graphics::Direct2D::{
    D2D1_ANTIALIAS_MODE_ALIASED, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_DRAW_TEXT_OPTIONS_NONE,
    D2D1_ELLIPSE, D2D1_EXTEND_MODE_CLAMP, D2D1_GAMMA_2_2, D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES,
    D2D1_ROUNDED_RECT, D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE, ID2D1RenderTarget, ID2D1SolidColorBrush,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FONT_WEIGHT_BOLD, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_LINE_METRICS, DWRITE_TEXT_METRICS,
    DWRITE_TEXT_RANGE, IDWriteTextFormat, IDWriteTextLayout, IDWriteTextLayout1,
};
use windows::core::{Error, Interface};

use super::layout::{self, Page, Span};
use crate::graphics::{Color, Fonts, color, d2d_color, solid_brush};
use crate::theme::Mode;

/// The footer's left text: the version this exe was built as, and the license.
pub const VERSION: &str = concat!("Version ", env!("CARGO_PKG_VERSION"));
pub const LICENSE: &str = "Open source (MIT)";
pub const LINK: &str = "Project page";

/// The headings' letter spacing (`letter-spacing: .09em`), as a fraction of their size.
const HEADING_SPACING: f32 = 0.09;

/// How far below the baseline the link's underline starts (`text-underline-offset: 2px`).
const UNDERLINE_OFFSET: u32 = 2;

/// One mode's colors.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    /// `--win-bg`: the window's background, a gradient from the top to the bottom.
    pub top: Color,
    pub bottom: Color,
    /// `--card` and `--card-edge`: the cards' background and border, and the footer's line.
    pub card: Color,
    pub edge: Color,
    /// `--panel-dim`: the headings and the footer's texts.
    pub dim: Color,
    /// `--link`.
    pub link: Color,
}

pub const DARK: Palette = Palette {
    top: color(0x25, 0x27, 0x2c, 1.0),
    bottom: color(0x1b, 0x1c, 0x20, 1.0),
    card: color(255, 255, 255, 0.035),
    edge: color(255, 255, 255, 0.07),
    dim: color(0x9a, 0xa0, 0xaa, 1.0),
    // oklch(0.82 0.12 200)
    link: color(0x4c, 0xdb, 0xe3, 1.0),
};

pub const LIGHT: Palette = Palette {
    top: color(0xf8, 0xf7, 0xf4, 1.0),
    bottom: color(0xee, 0xec, 0xe8, 1.0),
    card: color(255, 255, 255, 0.75),
    edge: color(70, 60, 45, 0.13),
    dim: color(0x6b, 0x68, 0x62, 1.0),
    // oklch(0.5 0.15 240)
    link: color(0x00, 0x6a, 0xaf, 1.0),
};

pub fn palette(mode: Mode) -> &'static Palette {
    match mode {
        Mode::Light => &LIGHT,
        Mode::Dark => &DARK,
    }
}

/// A text laid out on one line, and where it goes.
struct Placed {
    layout: IDWriteTextLayout,
    left: f32,
    /// The top of its layout box, set so the baseline falls on a whole pixel.
    top: f32,
    width: f32,
    baseline: f32,
}

/// Draws Settings, `width` × `height` pixels at `dpi`, in `mode`, and returns where the "Project page" link is, in the
/// target's pixels, which is where a click opens it.
pub fn draw(
    target: &ID2D1RenderTarget,
    fonts: &Fonts,
    mode: Mode,
    dpi: u32,
    width: u32,
) -> windows::core::Result<RECT> {
    let palette = palette(mode);
    let page = layout::page();
    let height = px(scaled(page.height, dpi));
    let width = px(width);
    background(target, palette, width, height)?;
    cards(target, fonts, palette, &page, dpi, width)?;
    footer(target, fonts, palette, page.footer, dpi, width)
}

/// The window's gradient, from its top to its bottom. Colors blend in sRGB, as CSS does.
fn background(
    target: &ID2D1RenderTarget,
    palette: &Palette,
    width: f32,
    height: f32,
) -> windows::core::Result<()> {
    let stops = [
        D2D1_GRADIENT_STOP {
            position: 0.0,
            color: d2d_color(palette.top.rgb, palette.top.alpha),
        },
        D2D1_GRADIENT_STOP {
            position: 1.0,
            color: d2d_color(palette.bottom.rgb, palette.bottom.alpha),
        },
    ];
    // SAFETY: the stops outlive the call, which copies them.
    let collection = unsafe {
        target.CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)
    }?;
    let mut line = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES::default();
    line.endPoint.Y = height;
    // SAFETY: the line and the collection outlive the call, which keeps its own reference to the collection.
    let brush = unsafe { target.CreateLinearGradientBrush(&line, None, &collection) }?;
    let all = D2D_RECT_F {
        left: 0.0,
        top: 0.0,
        right: width,
        bottom: height,
    };
    // SAFETY: a drawing call on a live render target, with arguments that outlive it.
    unsafe { target.FillRectangle(&all, &brush) };
    Ok(())
}

/// Each block's card with its 1-pixel border, and its heading above it.
fn cards(
    target: &ID2D1RenderTarget,
    fonts: &Fonts,
    palette: &Palette,
    page: &Page,
    dpi: u32,
    width: f32,
) -> windows::core::Result<()> {
    let card = solid_brush(target, palette.card)?;
    let edge = solid_brush(target, palette.edge)?;
    let dim = solid_brush(target, palette.dim)?;
    let side = px(scaled(layout::SIDE, dpi));
    let radius = px(scaled(layout::CARD_RADIUS, dpi));
    let border = px(scaled(1, dpi).max(1));
    let size = layout::HEADING_LINE * dpi_scale(dpi);
    let format = fonts.text_format(DWRITE_FONT_WEIGHT_BOLD, size)?;
    for placed in &page.blocks {
        let outer = D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F {
                left: side,
                top: px(scaled(placed.card.top, dpi)),
                right: width - side,
                bottom: px(scaled(placed.card.bottom, dpi)),
            },
            radiusX: radius,
            radiusY: radius,
        };
        // The border runs inside the card's edge, as CSS draws it with `box-sizing: border-box`.
        let half = border / 2.0;
        let stroke = D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F {
                left: outer.rect.left + half,
                top: outer.rect.top + half,
                right: outer.rect.right - half,
                bottom: outer.rect.bottom - half,
            },
            radiusX: radius - half,
            radiusY: radius - half,
        };
        // SAFETY: drawing calls on a live render target, with arguments that outlive them.
        unsafe {
            target.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
            target.FillRoundedRectangle(&outer, &card);
            target.DrawRoundedRectangle(&stroke, &edge, border, None);
        }
        let heading = place(
            text(fonts, placed.block.heading(), &format, Some(size))?,
            side + px(scaled(layout::HEADING_INDENT, dpi)),
            px(scaled(placed.heading, dpi)),
            size,
        )?;
        draw_text(target, &heading, &dim);
    }
    Ok(())
}

/// The footer's line, the version and the license on the left, and the link on the right, which comes back.
fn footer(
    target: &ID2D1RenderTarget,
    fonts: &Fonts,
    palette: &Palette,
    span: Span,
    dpi: u32,
    width: f32,
) -> windows::core::Result<RECT> {
    let edge = solid_brush(target, palette.edge)?;
    let dim = solid_brush(target, palette.dim)?;
    let link = solid_brush(target, palette.link)?;
    let top = px(scaled(span.top, dpi));
    let border = px(scaled(1, dpi).max(1));
    let rule = D2D_RECT_F {
        left: 0.0,
        top,
        right: width,
        bottom: top + border,
    };
    // SAFETY: drawing calls on a live render target, with arguments that outlive them. The rule is on whole pixels.
    unsafe {
        target.SetAntialiasMode(D2D1_ANTIALIAS_MODE_ALIASED);
        target.FillRectangle(&rule, &edge);
        target.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
    }
    let size = px(scaled(layout::FOOTER_TEXT, dpi));
    let format = fonts.text_format(DWRITE_FONT_WEIGHT_NORMAL, size)?;
    // The texts' line starts below the rule and the padding.
    let line = top + border + px(scaled(layout::FOOTER_ABOVE, dpi));
    let line_height = layout::FOOTER_LINE * dpi_scale(dpi);
    let gap = px(scaled(layout::FOOTER_GAP, dpi));
    let side = px(scaled(layout::SIDE, dpi));
    let mut left = side;
    for words in [VERSION, "·", LICENSE] {
        let placed = place(text(fonts, words, &format, None)?, left, line, line_height)?;
        draw_text(target, &placed, &dim);
        left = (placed.left + placed.width + gap).round();
    }
    let layout = text(fonts, LINK, &format, None)?;
    let width_of_link = width_of(&layout)?;
    let placed = place(
        layout,
        // Down to a whole pixel, so the link never reaches into the margin.
        (width - side - width_of_link).floor(),
        line,
        line_height,
    )?;
    draw_text(target, &placed, &link);
    // The underline is 1 pixel thick up to 175% and grows with the scale beyond, as a browser rounds it.
    let underline_top = placed.top + placed.baseline + px(scaled(UNDERLINE_OFFSET, dpi));
    let underline = D2D_RECT_F {
        left: placed.left,
        top: underline_top,
        right: placed.left + placed.width,
        bottom: underline_top + px((dpi / 96).max(1)),
    };
    // SAFETY: drawing calls on a live render target, with arguments that outlive them.
    unsafe {
        target.SetAntialiasMode(D2D1_ANTIALIAS_MODE_ALIASED);
        target.FillRectangle(&underline, &link);
        target.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
    }
    Ok(RECT {
        left: whole(placed.left),
        top: whole(line.floor()),
        right: whole((placed.left + placed.width).ceil()),
        bottom: whole((line + line_height).ceil()),
    })
}

/// `words` laid out on one line in `format`, spaced as a heading when `spacing` gives its font size.
fn text(
    fonts: &Fonts,
    words: &str,
    format: &IDWriteTextFormat,
    spacing: Option<f32>,
) -> windows::core::Result<IDWriteTextLayout> {
    let text: Vec<u16> = words.encode_utf16().collect();
    let length =
        u32::try_from(text.len()).map_err(|_| Error::new(E_FAIL, "the text is too long"))?;
    // SAFETY: the layout copies the text and keeps its own reference to the format. A width and height past any text
    // here keep it on one line.
    let layout = unsafe {
        fonts
            .factory
            .CreateTextLayout(&text, format, 4096.0, 4096.0)
    }?;
    if let Some(size) = spacing {
        // Letter spacing came with the layout's second version, in every Windows ScreenGauge runs on.
        let spaced: IDWriteTextLayout1 = layout.cast()?;
        // SAFETY: the range is the whole text. CSS adds the spacing after each letter.
        unsafe {
            spaced.SetCharacterSpacing(
                0.0,
                HEADING_SPACING * size,
                0.0,
                DWRITE_TEXT_RANGE {
                    startPosition: 0,
                    length,
                },
            )
        }?;
    }
    Ok(layout)
}

fn width_of(layout: &IDWriteTextLayout) -> windows::core::Result<f32> {
    let mut metrics = DWRITE_TEXT_METRICS::default();
    // SAFETY: fills a local.
    unsafe { layout.GetMetrics(&mut metrics) }?;
    Ok(metrics.widthIncludingTrailingWhitespace)
}

/// `layout` with its left side at `left`, centered in a line of `line_height` from `line` down, as CSS centers a text
/// in its line box, with the baseline moved to the nearest whole pixel so the letters stay crisp.
fn place(
    layout: IDWriteTextLayout,
    left: f32,
    line: f32,
    line_height: f32,
) -> windows::core::Result<Placed> {
    let width = width_of(&layout)?;
    let mut metrics = [DWRITE_LINE_METRICS::default()];
    let mut lines = 0;
    // SAFETY: fills a one-line buffer, and fails should the text need more.
    unsafe { layout.GetLineMetrics(Some(&mut metrics), &mut lines) }?;
    let [metrics] = metrics;
    let baseline = (line + (line_height - metrics.height) / 2.0 + metrics.baseline).round();
    Ok(Placed {
        layout,
        left,
        top: baseline - metrics.baseline,
        width,
        baseline: metrics.baseline,
    })
}

fn draw_text(target: &ID2D1RenderTarget, placed: &Placed, brush: &ID2D1SolidColorBrush) {
    // The point type Direct2D takes, which windows 0.62 takes from a crate it doesn't re-export, as in the overlay.
    let mut origin = D2D1_ELLIPSE::default().point;
    origin.X = placed.left;
    origin.Y = placed.top;
    // SAFETY: drawing calls on a live render target, with arguments that outlive them.
    unsafe {
        target.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
        target.DrawTextLayout(origin, &placed.layout, brush, D2D1_DRAW_TEXT_OPTIONS_NONE);
    }
}

/// How many pixels a logical pixel is at `dpi`, for the sizes that are not whole numbers.
fn dpi_scale(dpi: u32) -> f32 {
    // A DPI is a few hundred at most, which a float holds exactly, so the cast loses nothing.
    dpi as f32 / 96.0
}

/// A whole number of pixels as the float Direct2D takes. Sizes here are a few thousand pixels at most, far below 2^24,
/// up to which `f32` holds every whole number.
fn px(pixels: u32) -> f32 {
    debug_assert!(pixels < 1 << 24, "{pixels} pixels");
    pixels as f32
}

/// A position already rounded to a whole number of pixels, as Windows takes a rectangle. Positions in the window are a
/// few thousand pixels at most, far inside `i32`.
// TODO: place the link in whole pixels without going through a float, so no float comes back to `i32` here.
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
        D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_PIXEL_FORMAT,
    };
    use windows::Win32::Graphics::Direct2D::{
        D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_FEATURE_LEVEL_DEFAULT,
        D2D1_RENDER_TARGET_PROPERTIES, D2D1_RENDER_TARGET_TYPE_SOFTWARE,
        D2D1_RENDER_TARGET_USAGE_NONE, D2D1CreateFactory, ID2D1Factory,
    };
    use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
    use windows::Win32::Graphics::Imaging::{
        CLSID_WICImagingFactory, GUID_WICPixelFormat32bppPBGRA, IWICImagingFactory,
        WICBitmapCacheOnDemand,
    };
    use windows::Win32::System::Com::{
        CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
        CoUninitialize,
    };

    use super::*;

    /// The page as drawn, as rows of [r, g, b] pixels, and where the link went.
    struct Drawn {
        width: usize,
        pixels: Vec<[u8; 3]>,
        link: RECT,
    }

    impl Drawn {
        fn at(&self, x: i32, y: i32) -> [u8; 3] {
            let (x, y) = (
                usize::try_from(x).expect("x"),
                usize::try_from(y).expect("y"),
            );
            self.pixels[y * self.width + x]
        }
    }

    /// Draws the page in software on a bitmap of its size at `dpi`, with no window, monitor or graphics card. COM,
    /// which the bitmap needs, is started for the test's thread and ended once everything made with it is dropped.
    fn drawn(mode: Mode, dpi: u32) -> Drawn {
        let width = scaled(layout::WIDTH, dpi);
        let height = scaled(layout::page().height, dpi);
        let _com = Com::start();
        {
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
            let fonts = Fonts::new().expect("the fonts");
            // SAFETY: BeginDraw and EndDraw around the drawing, on a live target.
            unsafe { target.BeginDraw() };
            let link = draw(&target, &fonts, mode, dpi, width).expect("the page is drawn");
            // SAFETY: ends the BeginDraw above.
            unsafe { target.EndDraw(None, None) }.expect("the drawing is finished");
            let stride = width * 4;
            let mut bytes = vec![0; usize::try_from(stride * height).expect("a small bitmap")];
            // SAFETY: no rectangle copies the whole bitmap, into a buffer of exactly its size.
            unsafe { bitmap.CopyPixels(ptr::null(), stride, &mut bytes) }
                .expect("the pixels are read");
            Drawn {
                width: usize::try_from(width).expect("a small bitmap"),
                // Blue, green, red and alpha; every pixel of the page is opaque.
                pixels: bytes
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|&[b, g, r, _]| [r, g, b])
                    .collect(),
                link,
            }
        }
    }

    /// COM for the test's thread, which the bitmap needs, ended when this drops, after every object made with it: also
    /// when a check fails and unwinds.
    struct Com;

    impl Com {
        fn start() -> Com {
            // SAFETY: starts COM for this thread, undone once by the drop of the Com it returns.
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

    fn near(pixel: [u8; 3], color: Color, within: u8) -> bool {
        pixel
            .iter()
            .zip(color.rgb)
            .all(|(&have, want)| have.abs_diff(want) <= within)
    }

    fn brightness(pixel: [u8; 3]) -> i32 {
        pixel.iter().map(|&channel| i32::from(channel)).sum()
    }

    fn whole_px(logical: u32, dpi: u32) -> i32 {
        i32::try_from(scaled(logical, dpi)).expect("a small page")
    }

    /// The owner's monitors' scales, and Full HD at 150%.
    const DPIS: [u32; 3] = [120, 144, 168];

    #[test]
    fn the_background_runs_from_the_top_color_to_the_bottom_one() {
        for mode in [Mode::Dark, Mode::Light] {
            for dpi in DPIS {
                let page = drawn(mode, dpi);
                let bottom = whole_px(layout::page().height, dpi) - 1;
                assert!(near(page.at(2, 0), palette(mode).top, 1), "{mode:?} {dpi}");
                assert!(
                    near(page.at(2, bottom), palette(mode).bottom, 1),
                    "{mode:?} {dpi}"
                );
            }
        }
    }

    #[test]
    fn the_link_is_on_the_right_of_the_footer_and_underlined_in_the_link_color() {
        for mode in [Mode::Dark, Mode::Light] {
            for dpi in DPIS {
                let page = drawn(mode, dpi);
                let link = page.link;
                let footer = layout::page().footer;
                // It ends 16 logical pixels in from the right, inside the footer.
                let right = whole_px(layout::WIDTH - layout::SIDE, dpi);
                assert!(
                    (right - 1..=right).contains(&link.right),
                    "{mode:?} {dpi}: {link:?}"
                );
                assert!(link.left > right / 2, "{mode:?} {dpi}: {link:?}");
                let (top, bottom) = (whole_px(footer.top, dpi), whole_px(footer.bottom, dpi));
                assert!(
                    top < link.top && link.bottom < bottom,
                    "{mode:?} {dpi}: {link:?}"
                );
                // A row below the text is the link's color all along: the underline.
                let underlined = (link.top..bottom).any(|y| {
                    (link.left + 1..link.right - 1)
                        .all(|x| near(page.at(x, y), palette(mode).link, 2))
                });
                assert!(underlined, "{mode:?} {dpi}: no underline in {link:?}");
            }
        }
    }

    #[test]
    fn the_footer_s_texts_on_the_left_are_drawn_in_the_dim_color() {
        for mode in [Mode::Dark, Mode::Light] {
            let page = drawn(mode, 144);
            let link = page.link;
            let texts = (link.top..link.bottom)
                .any(|y| (24..420).any(|x| near(page.at(x, y), palette(mode).dim, 24)));
            assert!(texts, "{mode:?}");
        }
    }

    #[test]
    fn the_cards_are_lighter_than_the_background_and_the_footer_has_its_line() {
        let dpi = 144;
        for mode in [Mode::Dark, Mode::Light] {
            let page = drawn(mode, dpi);
            for placed in layout::page().blocks {
                // Inside the card, past its border and corner, and beside it on the same row.
                let y = whole_px(placed.card.top + 20, dpi);
                let inside = page.at(whole_px(layout::SIDE + 30, dpi), y);
                let beside = page.at(2, y);
                assert!(
                    brightness(inside) > brightness(beside),
                    "{mode:?} {:?}: {inside:?} beside {beside:?}",
                    placed.block
                );
            }
            // The footer's 1-pixel line differs from the background just above it.
            let line = whole_px(layout::page().footer.top, dpi);
            assert_ne!(page.at(300, line), page.at(300, line - 1), "{mode:?}");
        }
    }
}
