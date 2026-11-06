//! The colors of ScreenGauge's own surfaces, the pinned labels' panel, the notice and the magnified view, in the light
//! and the dark palette: the prototype's theme tokens (WPC-279), where the two that are `oklch` there are converted to
//! sRGB. The measurement's line and the live label are cyan in both and are not here.

use windows::Win32::Graphics::Direct2D::{ID2D1RenderTarget, ID2D1SolidColorBrush};

use crate::graphics::{Color, color, solid_brush};
use crate::relief::palette::{Shadow, shadow};
use crate::theme::Mode;

/// One mode's colors.
struct Palette {
    /// `--panel`: the pinned labels' and the notice's background, a little see-through.
    panel: Color,
    /// `--panel-ink`: the text on the panel.
    ink: Color,
    /// `--panel-dim`: the text on the panel that names what the ink gives, such as the magnified view's X and Y.
    dim: Color,
    /// `--panel-border`.
    border: Color,
    /// `--accent`: the notice's ok sign.
    accent: Color,
    /// `--warn`: the notice's alert sign.
    warn: Color,
    /// The 1px highlight inside the notice's top border (the inset part of `--panel-shadow`).
    highlight: Color,
    /// `--blank`: what the magnified view shows where its enlargement reaches past the monitor.
    blank: Color,
    /// The floating panels' shadow (the outer part of `--panel-shadow`), which `relief` blurs as it blurs the
    /// toolbar's.
    shadow: Shadow,
}

const DARK: Palette = Palette {
    panel: color(26, 27, 31, 0.94),
    ink: color(0xe9, 0xeb, 0xef, 1.0),
    dim: color(0x9a, 0xa0, 0xaa, 1.0),
    border: color(255, 255, 255, 0.1),
    // oklch(0.8 0.13 200)
    accent: color(0x28, 0xd6, 0xdf, 1.0),
    // oklch(0.82 0.14 80)
    warn: color(0xf3, 0xb9, 0x4c, 1.0),
    highlight: color(255, 255, 255, 0.08),
    blank: color(0x2a, 0x2c, 0x31, 1.0),
    // CSS `0 10px 28px -8px rgba(0,0,0,.75)`.
    shadow: shadow(10, 28, -8, color(0, 0, 0, 0.75)),
};

const LIGHT: Palette = Palette {
    panel: color(250, 249, 247, 0.96),
    ink: color(0x1d, 0x1c, 0x1a, 1.0),
    dim: color(0x6b, 0x68, 0x62, 1.0),
    border: color(70, 60, 45, 0.16),
    // oklch(0.55 0.15 235)
    accent: color(0x00, 0x7c, 0xbb, 1.0),
    // oklch(0.58 0.15 60)
    warn: color(0xb8, 0x60, 0x00, 1.0),
    highlight: color(255, 255, 255, 1.0),
    blank: color(0xd9, 0xd6, 0xd0, 1.0),
    // CSS `0 10px 26px -8px rgba(70,60,45,.45)`.
    shadow: shadow(10, 26, -8, color(70, 60, 45, 0.45)),
};

/// The brushes of one mode's palette, made for one render target. A render target that is lost takes its brushes with
/// it, so these are made again with everything else.
pub struct Surfaces {
    pub panel: ID2D1SolidColorBrush,
    /// Each use sets its opacity first, like the live label's ink: the text is in full ink, the bar and the logical
    /// part fainter.
    pub ink: ID2D1SolidColorBrush,
    pub dim: ID2D1SolidColorBrush,
    pub border: ID2D1SolidColorBrush,
    pub accent: ID2D1SolidColorBrush,
    pub warn: ID2D1SolidColorBrush,
    pub highlight: ID2D1SolidColorBrush,
    pub blank: ID2D1SolidColorBrush,
    pub shadow: Shadow,
}

impl Surfaces {
    pub fn new(target: &ID2D1RenderTarget, mode: Mode) -> windows::core::Result<Surfaces> {
        let palette = match mode {
            Mode::Light => LIGHT,
            Mode::Dark => DARK,
        };
        let brush = |color: Color| solid_brush(target, color);
        Ok(Surfaces {
            panel: brush(palette.panel)?,
            ink: brush(palette.ink)?,
            dim: brush(palette.dim)?,
            border: brush(palette.border)?,
            accent: brush(palette.accent)?,
            warn: brush(palette.warn)?,
            highlight: brush(palette.highlight)?,
            blank: brush(palette.blank)?,
            shadow: palette.shadow,
        })
    }
}
