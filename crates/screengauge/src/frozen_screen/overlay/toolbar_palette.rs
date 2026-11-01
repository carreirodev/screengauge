//! The toolbar's colors and shadows in the light and the dark palette: the prototype's theme tokens (WPC-279, variant
//! A: `--grp-*`, `--btn-*`, `--on-*`, `--sbtn-*` and `--ink*`), where the `oklch` colors are converted to sRGB. Written
//! as the CSS lists them, so a shadow list reads top to bottom as the stylesheet does: the first is painted on top.
//! Sizes are in logical pixels, which `toolbar_paint` scales for the monitor.

use super::palette::{Color, color};
use crate::theme::Mode;

/// An opaque color, as CSS writes `#rrggbb`.
const fn rgb(r: u8, g: u8, b: u8) -> Color {
    color(r, g, b, 1.0)
}

/// A color stop of a gradient that runs from the top of its box to the bottom (`linear-gradient(180deg, ...)`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stop {
    /// Where it is along the box's height: 0 at the top, 1 at the bottom.
    pub at: f32,
    pub color: Color,
}

const fn stop(at: f32, color: Color) -> Stop {
    Stop { at, color }
}

/// A `box-shadow` with no sideways offset, which none of the toolbar's has: it falls `y` down, blurs by `blur` and is
/// `spread` bigger than its box on every side (smaller, should it be negative).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shadow {
    pub y: i32,
    pub blur: u32,
    pub spread: i32,
    pub color: Color,
}

const fn shadow(y: i32, blur: u32, spread: i32, color: Color) -> Shadow {
    Shadow {
        y,
        blur,
        spread,
        color,
    }
}

/// How one box is painted. CSS paints a box's outer shadows first, the last listed lowest, then its background (the
/// base color, then the gradient over it), then its border, then its inset shadows, again the last listed lowest.
#[derive(Debug, Clone, Copy)]
pub struct Surface {
    pub base: Option<Color>,
    pub gradient: &'static [Stop],
    pub border: Option<Color>,
    /// In the order of the stylesheet, so the painter goes through them backwards.
    pub outer: &'static [Shadow],
    pub inset: &'static [Shadow],
}

/// One mode's colors.
#[derive(Debug)]
pub struct Palette {
    /// `.grp`: the panel behind a group of buttons.
    pub group: Surface,
    /// `.tbtn` and `.mbtn`: a tool's or a mode's button.
    pub button: Surface,
    /// `.on::before`: what the lit button's own box is painted over with.
    pub lit: Surface,
    /// `.sbtn`: the gear, minimize and close.
    pub small: Surface,
    /// `--ink`, `--ink-on` (the lit button's) and `--sbtn-ink`: the icons.
    pub ink: Color,
    pub ink_on: Color,
    pub small_ink: Color,
}

/// `.sbtn`'s `box-shadow`, which the themes don't change.
const SMALL_INSET: [Shadow; 1] = [shadow(1, 1, 0, color(255, 255, 255, 0.2))];
const SMALL_OUTER: [Shadow; 1] = [shadow(3, 6, 0, color(0, 0, 0, 0.35))];

const DARK: Palette = Palette {
    group: Surface {
        base: Some(color(24, 25, 29, 0.86)),
        gradient: &[
            stop(0.0, color(255, 255, 255, 0.09)),
            stop(1.0, color(255, 255, 255, 0.02)),
        ],
        border: Some(color(255, 255, 255, 0.08)),
        outer: &[shadow(18, 40, -12, color(0, 0, 0, 0.8))],
        inset: &[
            shadow(1, 0, 0, color(255, 255, 255, 0.12)),
            shadow(-8, 16, 0, color(0, 0, 0, 0.35)),
        ],
    },
    button: Surface {
        base: None,
        gradient: &[
            stop(0.0, rgb(0x55, 0x5a, 0x64)),
            stop(0.55, rgb(0x2a, 0x2d, 0x33)),
            stop(1.0, rgb(0x1a, 0x1b, 0x1f)),
        ],
        border: None,
        outer: &[shadow(4, 8, 0, color(0, 0, 0, 0.55))],
        inset: &[
            shadow(1, 1, 0, color(255, 255, 255, 0.25)),
            shadow(-3, 6, 0, color(0, 0, 0, 0.5)),
        ],
    },
    lit: Surface {
        base: None,
        gradient: &[
            // oklch(0.92 0.1 195)
            stop(0.0, rgb(0x8e, 0xfa, 0xf9)),
            // oklch(0.74 0.15 205)
            stop(0.5, rgb(0x00, 0xc4, 0xd7)),
            // oklch(0.5 0.14 220)
            stop(1.0, rgb(0x00, 0x73, 0x9a)),
        ],
        border: None,
        outer: &[
            // The glow: oklch(0.75 0.15 205 / .6)
            shadow(0, 22, 0, color(0x00, 0xc8, 0xdb, 0.6)),
            shadow(4, 8, 0, color(0, 0, 0, 0.5)),
        ],
        inset: &[
            shadow(1, 1, 0, color(255, 255, 255, 0.6)),
            shadow(-3, 6, 0, color(0, 0, 0, 0.3)),
        ],
    },
    small: Surface {
        base: None,
        gradient: &[
            stop(0.0, rgb(0x4a, 0x4e, 0x57)),
            stop(0.85, rgb(0x1e, 0x20, 0x24)),
        ],
        border: None,
        outer: &SMALL_OUTER,
        inset: &SMALL_INSET,
    },
    ink: rgb(0xc3, 0xc7, 0xcf),
    ink_on: rgb(0x06, 0x14, 0x1a),
    small_ink: rgb(0xae, 0xb3, 0xbd),
};

const LIGHT: Palette = Palette {
    group: Surface {
        base: Some(color(226, 224, 219, 0.8)),
        gradient: &[
            stop(0.0, color(255, 255, 255, 0.85)),
            stop(1.0, color(255, 255, 255, 0.45)),
        ],
        border: Some(color(255, 255, 255, 0.9)),
        outer: &[
            shadow(18, 36, -14, color(70, 60, 45, 0.45)),
            shadow(2, 4, 0, color(70, 60, 45, 0.12)),
        ],
        inset: &[shadow(-6, 14, 0, color(70, 60, 45, 0.08))],
    },
    button: Surface {
        base: None,
        gradient: &[
            stop(0.0, rgb(0xff, 0xff, 0xff)),
            stop(0.5, rgb(0xf1, 0xef, 0xeb)),
            stop(1.0, rgb(0xd6, 0xd2, 0xcb)),
        ],
        border: None,
        outer: &[shadow(4, 8, 0, color(70, 60, 45, 0.22))],
        inset: &[
            shadow(1, 1, 0, rgb(0xff, 0xff, 0xff)),
            shadow(-3, 5, 0, color(70, 60, 45, 0.15)),
        ],
    },
    lit: Surface {
        base: None,
        gradient: &[
            // oklch(0.85 0.1 220)
            stop(0.0, rgb(0x7d, 0xdd, 0xfb)),
            // oklch(0.62 0.16 235)
            stop(0.55, rgb(0x00, 0x92, 0xd7)),
            // oklch(0.45 0.15 245)
            stop(1.0, rgb(0x00, 0x58, 0xa1)),
        ],
        border: None,
        outer: &[
            // The glow: oklch(0.62 0.16 235 / .45)
            shadow(0, 18, 0, color(0x00, 0x92, 0xd7, 0.45)),
            shadow(4, 8, 0, color(40, 50, 80, 0.3)),
        ],
        inset: &[
            shadow(1, 1, 0, color(255, 255, 255, 0.6)),
            shadow(-3, 6, 0, color(0, 0, 0, 0.2)),
        ],
    },
    small: Surface {
        base: None,
        gradient: &[
            stop(0.0, rgb(0xff, 0xff, 0xff)),
            stop(0.9, rgb(0xdc, 0xd8, 0xd1)),
        ],
        border: None,
        outer: &SMALL_OUTER,
        inset: &SMALL_INSET,
    },
    ink: rgb(0x4b, 0x48, 0x43),
    ink_on: rgb(0xff, 0xff, 0xff),
    small_ink: rgb(0x5b, 0x58, 0x52),
};

/// The palette of `mode`.
pub fn palette(mode: Mode) -> &'static Palette {
    match mode {
        Mode::Dark => &DARK,
        Mode::Light => &LIGHT,
    }
}
