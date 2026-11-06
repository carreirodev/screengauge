//! What Settings and the tray icon's menu look like alike: the prototype's theme tokens for `#settings` (WPC-279), where
//! the `oklch` ones are converted to sRGB, the window's background, and the helpers both draw with at the monitor's
//! scale. Neither window owns it, so a change to one's looks shows here, where it changes the other too.

use windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F;
use windows::Win32::Graphics::Direct2D::{
    D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_ELLIPSE, D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE,
    ID2D1RenderTarget, ID2D1SolidColorBrush,
};
use windows::Win32::Graphics::DirectWrite::IDWriteTextLayout;

use crate::graphics::{Color, color};
use crate::relief;
use crate::relief::palette::Stop;
use crate::theme::Mode;

/// One mode's colors.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    /// `--win-bg`: the window's background, a gradient from the top to the bottom.
    pub top: Color,
    pub bottom: Color,
    /// `--card` and `--card-edge`: Settings' cards' background and border, and its footer's line; the menu's border
    /// and the tint of its lit row.
    pub card: Color,
    pub edge: Color,
    /// `--panel-ink`: the rows' labels.
    pub ink: Color,
    /// `--panel-dim`: the headings, the rows' notes and the footer's texts.
    pub dim: Color,
    /// `--link`.
    pub link: Color,
    /// `--accent`, as the tools' notices have it (`frozen_screen::overlay::palette`): the global shortcut's field while
    /// it records, and the check mark before "Check for updates"' "You're on the latest version.".
    pub accent: Color,
    /// `--warn`: the messages under rows' labels, their band, and the sign before "Couldn't check.".
    pub warn: Color,
}

pub const DARK: Palette = Palette {
    top: color(0x25, 0x27, 0x2c, 1.0),
    bottom: color(0x1b, 0x1c, 0x20, 1.0),
    card: color(255, 255, 255, 0.035),
    edge: color(255, 255, 255, 0.07),
    ink: color(0xe9, 0xeb, 0xef, 1.0),
    dim: color(0x9a, 0xa0, 0xaa, 1.0),
    // oklch(0.82 0.12 200)
    link: color(0x4c, 0xdb, 0xe3, 1.0),
    // oklch(0.8 0.13 200)
    accent: color(0x28, 0xd6, 0xdf, 1.0),
    // oklch(0.82 0.14 80)
    warn: color(0xf3, 0xb9, 0x4c, 1.0),
};

pub const LIGHT: Palette = Palette {
    top: color(0xf8, 0xf7, 0xf4, 1.0),
    bottom: color(0xee, 0xec, 0xe8, 1.0),
    card: color(255, 255, 255, 0.75),
    edge: color(70, 60, 45, 0.13),
    ink: color(0x1d, 0x1c, 0x1a, 1.0),
    dim: color(0x6b, 0x68, 0x62, 1.0),
    // oklch(0.5 0.15 240)
    link: color(0x00, 0x6a, 0xaf, 1.0),
    // oklch(0.55 0.15 235)
    accent: color(0x00, 0x7c, 0xbb, 1.0),
    // oklch(0.58 0.15 60)
    warn: color(0xb8, 0x60, 0x00, 1.0),
};

pub fn palette(mode: Mode) -> &'static Palette {
    match mode {
        Mode::Light => &LIGHT,
        Mode::Dark => &DARK,
    }
}

/// The window's gradient, from its top to its bottom, `width` by `height` pixels: the toolbar's own gradient
/// (`relief::gradient`), whose colors blend in sRGB, as CSS does.
pub fn background(
    target: &ID2D1RenderTarget,
    palette: &Palette,
    width: f32,
    height: f32,
) -> windows::core::Result<()> {
    let stops = [
        Stop {
            at: 0.0,
            color: palette.top,
        },
        Stop {
            at: 1.0,
            color: palette.bottom,
        },
    ];
    let brush = relief::gradient(target, &stops, 0.0, 0.0, height)?;
    let all = D2D_RECT_F {
        left: 0.0,
        top: 0.0,
        right: width,
        bottom: height,
    };
    // SAFETY: a drawing call on a live render target, with arguments that outlive it. A plain rectangle: a rounded one
    // of no radius comes out a level apart in places.
    unsafe { target.FillRectangle(&all, &brush) };
    Ok(())
}

/// How many pixels a logical pixel is at `dpi`.
pub fn dpi_scale(dpi: u32) -> f32 {
    // A DPI is a few hundred at most, which a float holds exactly, so the cast loses nothing.
    dpi as f32 / 96.0
}

/// A whole number of pixels as the float Direct2D takes. Sizes here are a few thousand pixels at most, far below 2^24,
/// up to which `f32` holds every whole number.
pub fn px(pixels: u32) -> f32 {
    debug_assert!(pixels < 1 << 24, "{pixels} pixels");
    pixels as f32
}

/// A position already rounded to a whole number of pixels, as Windows takes a rectangle. Positions in the window are a
/// few thousand pixels at most, far inside `i32`.
// TODO: place the link in whole pixels without going through a float, so no float comes back to `i32` here.
pub fn whole(position: f32) -> i32 {
    debug_assert!(
        position.fract() == 0.0 && position.abs() < 1e6,
        "{position}"
    );
    position as i32
}

/// Draws `layout` with its top left corner at (`left`, `top`), its letters smoothed in grey.
pub fn write(
    target: &ID2D1RenderTarget,
    layout: &IDWriteTextLayout,
    left: f32,
    top: f32,
    brush: &ID2D1SolidColorBrush,
) {
    // The point type Direct2D takes, which windows 0.62 takes from a crate it doesn't re-export, as in the overlay.
    let mut origin = D2D1_ELLIPSE::default().point;
    origin.X = left;
    origin.Y = top;
    // SAFETY: drawing calls on a live render target, with arguments that outlive them.
    unsafe {
        target.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
        target.DrawTextLayout(origin, layout, brush, D2D1_DRAW_TEXT_OPTIONS_NONE);
    }
}
