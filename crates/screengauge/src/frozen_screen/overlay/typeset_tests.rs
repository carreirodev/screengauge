//! The labels' and the notices' text keeps the font's kerning while its digits stay tabular (WPC-354). `typeset_plain`
//! is the reference for a text with no digits: laid out with DirectWrite's default features, it matched Chromium in
//! WPC-309 ("Tolerance" at 11px, 47.54 pixels wide).

use super::tests::{Canvas, MONITOR, SCALES};
use super::*;

/// How wide `layout` is, in fractions of a pixel: whole pixels could hide a difference.
fn exact_width(layout: &IDWriteTextLayout) -> f32 {
    let mut metrics = DWRITE_TEXT_METRICS::default();
    // SAFETY: fills a local.
    unsafe { layout.GetMetrics(&mut metrics) }.expect("metrics");
    metrics.width
}

#[test]
fn text_without_digits_is_as_wide_as_with_the_default_features() {
    // "Copied" has no pair the font kerns; the others lost a fraction of a pixel to the tabular figures before WPC-354.
    let texts = [
        "Copied",
        "No content found \u{b7} Hold Alt to measure the rectangle",
        "Couldn't save.",
        "Tolerance",
    ];
    let canvas = Canvas::new(8, 8);
    for dpi in SCALES {
        canvas.render(MONITOR, dpi, |overlay, _| {
            for (name, format) in [
                ("physical", &overlay.format),
                ("logical", &overlay.logical_format),
            ] {
                for text in texts {
                    let label = exact_width(&overlay.layout(text, format).expect("a layout"));
                    let plain = exact_width(
                        &overlay
                            .typeset_plain(text, format)
                            .expect("a plain layout")
                            .layout,
                    );
                    assert!(
                        (label - plain).abs() < 0.001,
                        "{text:?} in the {name} format at {dpi} DPI: {label} wide as a label, {plain} plain"
                    );
                }
            }
        });
    }
}

#[test]
fn digits_stay_tabular() {
    let canvas = Canvas::new(8, 8);
    for dpi in SCALES {
        canvas.render(MONITOR, dpi, |overlay, _| {
            for (name, format) in [
                ("physical", &overlay.format),
                ("logical", &overlay.logical_format),
            ] {
                let width = |text| exact_width(&overlay.layout(text, format).expect("a layout"));
                let (ones, zeros) = (width("1111"), width("0000"));
                assert!(
                    (ones - zeros).abs() < 0.001,
                    "in the {name} format at {dpi} DPI: 1111 is {ones} wide, 0000 {zeros}"
                );
            }
        });
    }
}
