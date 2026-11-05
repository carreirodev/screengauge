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
                let width = |text| overlay.typeset(text, format).expect("a text").width;
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
    let canvas = Canvas::new(8, 8);
    for dpi in SCALES {
        canvas.render(MONITOR, dpi, |overlay, _| {
            for text in ["Copied", "Couldn't save."] {
                let label = overlay.typeset(text, &overlay.format).expect("a text");
                let measured = whole_up(exact_width(&label.layout));
                assert_eq!(label.width, measured, "{text:?} at {dpi} DPI");
            }
        });
    }
}

/// The label for `physical` and `logical`, live or pinned: its pill's size, and where its bar and its logical text start
/// from the pill's left side.
fn label_parts(
    canvas: &Canvas,
    dpi: u32,
    look: Look,
    physical: &str,
    logical: &str,
) -> (i32, i32, i32, f32) {
    let m = tests::with_logical(tests::horizontal(-400, -100, 350, physical), logical);
    let mut parts = None;
    canvas.render(MONITOR, dpi, |overlay, _| {
        let label = overlay.label(&m, look).expect("the label").expect("a pill");
        let after = label.logical.expect("a logical part");
        let left = label.pill.left - MONITOR.left;
        parts = Some((
            label.pill.right - label.pill.left,
            label.pill.bottom - label.pill.top,
            // The bar is in the window's own pixels, on whole pixels.
            whole(after.bar.left) - left,
            after.text.left - px(left),
        ));
    });
    let (width, height, bar, text) = parts.expect("the label was laid out");
    (width, height, bar, text)
}

#[test]
fn a_seven_leaves_the_label_as_it_was_live_and_pinned() {
    let canvas = Canvas::new(480, 320);
    // Both parts differ only in their 7s: a 7 in the physical part would move the bar and the logical text, and one in
    // the logical part would move the pill's right side.
    for dpi in [144, 168] {
        for look in [Look::Live, Look::Pinned] {
            let without = label_parts(&canvas, dpi, look, "240 \u{d7} 180", "140.1 \u{d7} 102.9");
            let with = label_parts(&canvas, dpi, look, "247 \u{d7} 187", "147.1 \u{d7} 107.9");
            assert_eq!(
                without, with,
                "{look:?} at {dpi} DPI: (width, height, bar, logical text)"
            );
        }
    }
}
