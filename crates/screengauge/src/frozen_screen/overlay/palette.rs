//! The colors of ScreenGauge's own surfaces, the pinned labels' panel and the notice, in the light and the dark
//! palette: the prototype's theme tokens (WPC-279), where the two that are `oklch` there are converted to sRGB. The
//! measurement's line and the live label are cyan in both and are not here.

use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;
use windows::Win32::Graphics::Direct2D::{ID2D1RenderTarget, ID2D1SolidColorBrush};

use crate::theme::Mode;

/// How many rounded rectangles the notice's shadow is made of.
pub const SHADOW_LAYERS: usize = 10;

/// An sRGB color, with the opacity CSS gives it in `rgba()`.
#[derive(Clone, Copy)]
struct Color {
    rgb: [u8; 3],
    alpha: f32,
}

const fn color(r: u8, g: u8, b: u8, alpha: f32) -> Color {
    Color {
        rgb: [r, g, b],
        alpha,
    }
}

/// One mode's colors.
struct Palette {
    /// `--panel`: the pinned labels' and the notice's background, a little see-through.
    panel: Color,
    /// `--panel-ink`: the text on the panel.
    ink: Color,
    /// `--panel-border`.
    border: Color,
    /// `--accent`: the notice's ok sign.
    accent: Color,
    /// `--warn`: the notice's alert sign.
    warn: Color,
    /// The 1px highlight inside the notice's top border (the inset part of `--panel-shadow`).
    highlight: Color,
    /// The notice's shadow, which the opacities below are laid over.
    shadow: Color,
    /// The opacity of each layer of the notice's shadow, from the innermost to the outermost.
    shadow_opacities: [f32; SHADOW_LAYERS],
}

// The notice's shadow is CSS `0 10px 28px -8px`: a gaussian blur with a sigma of 14px (CSS's blur of 28px) of a box 15px
// tall (the notice's 31px less the spread of 8 on each side), at an opacity of .75 in the dark palette and .45 in the
// light one. Its layers are rounded rectangles 4px bigger each than the one before, and each one's opacity is what makes
// the layers laid over each other add up to the gaussian's strength in the middle of its 4px band, reading it from the
// outermost layer in; the last band beyond them is clear.
const DARK: Palette = Palette {
    panel: color(26, 27, 31, 0.94),
    ink: color(0xe9, 0xeb, 0xef, 1.0),
    border: color(255, 255, 255, 0.1),
    // oklch(0.8 0.13 200)
    accent: color(0x28, 0xd6, 0xdf, 1.0),
    // oklch(0.82 0.14 80)
    warn: color(0xf3, 0xb9, 0x4c, 1.0),
    highlight: color(255, 255, 255, 0.08),
    shadow: color(0, 0, 0, 1.0),
    shadow_opacities: [
        0.064, 0.060, 0.059, 0.051, 0.040, 0.028, 0.018, 0.011, 0.006, 0.006,
    ],
};

const LIGHT: Palette = Palette {
    panel: color(250, 249, 247, 0.96),
    ink: color(0x1d, 0x1c, 0x1a, 1.0),
    border: color(70, 60, 45, 0.16),
    // oklch(0.55 0.15 235)
    accent: color(0x00, 0x7c, 0xbb, 1.0),
    // oklch(0.58 0.15 60)
    warn: color(0xb8, 0x60, 0x00, 1.0),
    highlight: color(255, 255, 255, 1.0),
    shadow: color(70, 60, 45, 1.0),
    shadow_opacities: [
        0.034, 0.033, 0.033, 0.029, 0.023, 0.017, 0.011, 0.007, 0.004, 0.003,
    ],
};

/// The brushes of one mode's palette, made for one render target. A render target that is lost takes its brushes with
/// it, so these are made again with everything else.
pub struct Surfaces {
    pub panel: ID2D1SolidColorBrush,
    /// Each use sets its opacity first, like the live label's ink: the text is in full ink, the bar and the logical
    /// part fainter.
    pub ink: ID2D1SolidColorBrush,
    pub border: ID2D1SolidColorBrush,
    pub accent: ID2D1SolidColorBrush,
    pub warn: ID2D1SolidColorBrush,
    pub highlight: ID2D1SolidColorBrush,
    /// Each layer of the notice's shadow sets its opacity first.
    pub shadow: ID2D1SolidColorBrush,
    pub shadow_opacities: [f32; SHADOW_LAYERS],
}

impl Surfaces {
    pub fn new(target: &ID2D1RenderTarget, mode: Mode) -> windows::core::Result<Surfaces> {
        let palette = match mode {
            Mode::Light => LIGHT,
            Mode::Dark => DARK,
        };
        let brush = |Color {
                         rgb: [r, g, b],
                         alpha,
                     }: Color| {
            let color = D2D1_COLOR_F {
                r: f32::from(r) / 255.0,
                g: f32::from(g) / 255.0,
                b: f32::from(b) / 255.0,
                a: alpha,
            };
            // SAFETY: `color` outlives the call, which keeps nothing of it.
            unsafe { target.CreateSolidColorBrush(&color, None) }
        };
        Ok(Surfaces {
            panel: brush(palette.panel)?,
            ink: brush(palette.ink)?,
            border: brush(palette.border)?,
            accent: brush(palette.accent)?,
            warn: brush(palette.warn)?,
            highlight: brush(palette.highlight)?,
            shadow: brush(palette.shadow)?,
            shadow_opacities: palette.shadow_opacities,
        })
    }
}
