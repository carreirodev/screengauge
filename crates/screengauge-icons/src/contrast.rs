//! Microsoft's contrast rule for app icons (Microsoft Learn, "Design guidelines for Windows app icons", read
//! 2026-10-31): "Make sure at least half of your icon passes a 3.0:1 contrast ratio on light and dark theme."
//!
//! Measured per picture: of the pixels at least half opaque, the share that, laid over a theme's background, differ
//! from that background by 3:1 or more (WCAG's contrast ratio).

use crate::image::{Image, Rgba};

/// The backgrounds an app icon sits on: the taskbar and the window background of Windows 11's dark and light themes,
/// as the WPC-291 prototype drew them.
pub const THEME_BACKGROUNDS: [(&str, Rgba); 4] = [
    ("dark taskbar", Rgba::hex(0x1C1C1C)),
    ("dark window", Rgba::hex(0x202020)),
    ("light taskbar", Rgba::hex(0xEEEFF1)),
    ("light window", Rgba::hex(0xF3F3F3)),
];

/// The share of `image`, from 0 to 1, that has at least 3:1 contrast against `background`.
pub fn passing_share(image: &Image, background: Rgba) -> f64 {
    let back = luminance(background.r, background.g, background.b);
    let (mut counted, mut passing) = (0u32, 0u32);
    for p in image.pixels().filter(|p| p.a >= 128) {
        let alpha = f64::from(p.a) / 255.0;
        let mix =
            |front: u8, behind: u8| f64::from(front) * alpha + f64::from(behind) * (1.0 - alpha);
        let front = linear_luminance(
            mix(p.r, background.r),
            mix(p.g, background.g),
            mix(p.b, background.b),
        );
        counted += 1;
        if ratio(front, back) >= 3.0 {
            passing += 1;
        }
    }
    if counted == 0 {
        return 0.0;
    }
    f64::from(passing) / f64::from(counted)
}

/// WCAG's contrast ratio between two relative luminances.
pub fn ratio(a: f64, b: f64) -> f64 {
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

/// WCAG's relative luminance of an sRGB color.
pub fn luminance(r: u8, g: u8, b: u8) -> f64 {
    linear_luminance(f64::from(r), f64::from(g), f64::from(b))
}

fn linear_luminance(r: f64, g: f64, b: f64) -> f64 {
    let channel = |value: f64| {
        let c = value / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lum(color: Rgba) -> f64 {
        luminance(color.r, color.g, color.b)
    }

    #[test]
    fn black_on_white_is_21_to_1() {
        let r = ratio(lum(Rgba::hex(0x000000)), lum(Rgba::hex(0xFFFFFF)));
        assert!((r - 21.0).abs() < 1e-9, "{r}");
    }

    #[test]
    fn the_middle_orange_fails_on_light_and_passes_on_dark() {
        // The values measured in the prototype for WPC-304.
        let orange = lum(Rgba::hex(0xF97316));
        assert!((ratio(orange, lum(Rgba::hex(0xEEEFF1))) - 2.44).abs() < 0.01);
        assert!((ratio(orange, lum(Rgba::hex(0x1C1C1C))) - 6.08).abs() < 0.01);
    }

    #[test]
    fn half_transparent_pixels_dont_count() {
        let mut image = Image::new(2, 1);
        image.set_pixel(0, 0, Rgba::hex(0x000000));
        image.set_pixel(1, 0, Rgba::hex(0xFFFFFF).with_opacity(0.4));
        assert_eq!(passing_share(&image, Rgba::hex(0xFFFFFF)), 1.0);
    }

    #[test]
    fn the_share_counts_passing_pixels() {
        let mut image = Image::new(4, 1);
        image.set_pixel(0, 0, Rgba::hex(0x000000));
        for x in 1..4 {
            image.set_pixel(x, 0, Rgba::hex(0xFAFAFA));
        }
        assert_eq!(passing_share(&image, Rgba::hex(0xFFFFFF)), 0.25);
    }
}
