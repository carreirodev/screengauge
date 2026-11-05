//! The labels' and the notices' text keeps the font's kerning while its digits stay tabular (WPC-354). `typeset_plain`
//! is the reference for a text with no digits: laid out with DirectWrite's default features, it matched Chromium in
//! WPC-309 ("Tolerance" at 11px, 47.54 pixels wide).

use super::tests::{Canvas, MONITOR, SCALES, horizontal, text_width, with_logical};
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

/// Without tabular figures a 1 is narrower than a 0: at 100%, "1111" is 19.31 pixels wide and "0000" 26.65.
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

/// Texts that differ only in their 7s, which Segoe UI semibold draws a third of a pixel narrower than its other digits
/// even with tabular figures (WPC-386; Chromium draws it as narrow). Rounded up to whole pixels, that third could take a
/// pixel off the box around the number.
const SEVENS: [(&str, &str); 3] = [
    ("10", "17"),
    ("1000", "1777"),
    ("240 \u{d7} 180", "247 \u{d7} 187"),
];

#[test]
fn a_seven_takes_as_much_room_as_any_other_digit() {
    let canvas = Canvas::new(8, 8);
    for dpi in SCALES {
        canvas.render(MONITOR, dpi, |overlay, _| {
            for (name, format) in [
                ("physical", &overlay.format),
                ("logical", &overlay.logical_format),
            ] {
                let width = |text| text_width(overlay, text, format);
                for (without, with) in SEVENS {
                    assert_eq!(
                        width(without),
                        width(with),
                        "{without:?} and {with:?} in the {name} format at {dpi} DPI"
                    );
                }
            }
        });
    }
}

#[test]
fn a_text_without_digits_keeps_its_width() {
    // As wide as laid out with the default features alone, in whole pixels, as before WPC-386.
    let canvas = Canvas::new(8, 8);
    for dpi in SCALES {
        canvas.render(MONITOR, dpi, |overlay, _| {
            for text in ["Copied", "Couldn't save."] {
                let plain = overlay
                    .typeset_plain(text, &overlay.format)
                    .expect("a plain text")
                    .width;
                assert_eq!(
                    text_width(overlay, text, &overlay.format),
                    plain,
                    "{text:?} at {dpi} DPI"
                );
            }
        });
    }
}

/// A label's pill's size, and where its bar and its logical text start from the pill's left side.
#[derive(Debug, PartialEq)]
struct LabelParts {
    width: i32,
    height: i32,
    bar: i32,
    logical_text: f32,
}

/// The parts of the label for `physical` and `logical`, live or pinned.
fn label_parts(canvas: &Canvas, dpi: u32, look: Look, physical: &str, logical: &str) -> LabelParts {
    let m = with_logical(horizontal(-400, -100, 350, physical), logical);
    let mut parts = None;
    canvas.render(MONITOR, dpi, |overlay, _| {
        let label = overlay.label(&m, look).expect("the label").expect("a pill");
        let after = label.logical.expect("a logical part");
        let left = label.pill.left - MONITOR.left;
        parts = Some(LabelParts {
            width: label.pill.right - label.pill.left,
            height: label.pill.bottom - label.pill.top,
            // The bar is in the window's own pixels, on whole pixels.
            bar: whole(after.bar.left) - left,
            logical_text: after.text.left - px(left),
        });
    });
    parts.expect("the label was laid out")
}

#[test]
fn a_seven_leaves_the_label_as_it_was_live_and_pinned() {
    let canvas = Canvas::new(480, 320);
    // Both parts differ only in their 7s: a 7 in the physical part would move the bar and the logical text, and one in
    // the logical part would move the pill's right side.
    for dpi in SCALES {
        for look in [Look::Live, Look::Pinned] {
            let without = label_parts(&canvas, dpi, look, "240 \u{d7} 180", "140.1 \u{d7} 102.9");
            let with = label_parts(&canvas, dpi, look, "247 \u{d7} 187", "147.1 \u{d7} 107.9");
            assert_eq!(without, with, "{look:?} at {dpi} DPI");
        }
    }
}
