//! The live measurement's label: its pill, its shadow, its text, and its logical part.

use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FONT_SIMULATIONS_NONE, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL,
};

use super::geometry::Size;
use super::tests::{
    BACKGROUND, CYAN, Canvas, INK, MONITOR, Picture, SCALES, cross, expected, horizontal, near,
    text_width, vertical, with_logical,
};
use super::*;
use crate::graphics::font_family;

/// The label alone, drawn as the whole measurement draws it, and where its pill went.
fn draw_label(overlay: &Overlay, target: &ID2D1RenderTarget, m: &Measurement) -> Option<RECT> {
    let label = overlay.label(m, Look::Live).expect("the label")?;
    overlay.draw_shadow(target, &label).expect("the shadow");
    overlay.draw_pill(target, &label);
    Some(label.pill)
}

/// The rows (top, bottom) and columns (left, right) of the text's pixels inside `pill`: those that are more than
/// half the text's near-black, which neither the cyan, nor the photo showing at the rounded corners, nor the shadow
/// is.
fn ink(picture: &Picture, pill: &RECT) -> ((i32, i32), (i32, i32)) {
    let (mut top, mut bottom, mut left, mut right) = (i32::MAX, i32::MIN, i32::MAX, i32::MIN);
    for y in pill.top..pill.bottom {
        for x in pill.left..pill.right {
            let (wx, wy) = ((x - MONITOR.left) as usize, (y - MONITOR.top) as usize);
            let [r, g, _] = picture.at(wx, wy);
            if r < 100 && g < 140 {
                (top, bottom) = (top.min(y), bottom.max(y));
                (left, right) = (left.min(x), right.max(x));
            }
        }
    }
    ((top, bottom), (left, right))
}

#[test]
fn the_pill_is_opaque_cyan_around_dark_centered_text() {
    let canvas = Canvas::new(480, 320);
    for (dpi, height) in [(96, 22), (120, 27), (168, 39)] {
        let m = horizontal(-400, -301, 350, "24");
        let mut pill = None;
        let picture = canvas.render(MONITOR, dpi, |overlay, target| {
            pill = draw_label(overlay, target, &m);
        });
        let pill = pill.expect("a pill");
        assert_eq!(pill.bottom - pill.top, height, "at {dpi} DPI");
        let at =
            |x: i32, y: i32| picture.at((x - MONITOR.left) as usize, (y - MONITOR.top) as usize);
        let middle = (pill.top + pill.bottom) / 2;
        // In the padding left of the text: opaque cyan, not blended with the photo.
        assert_eq!(at(pill.left + 3, middle), CYAN, "at {dpi} DPI");
        // The corner is rounded, so the pill's corner pixel is mostly the photo.
        assert_ne!(at(pill.left, pill.top), CYAN, "at {dpi} DPI");

        let ((top, bottom), (left, right)) = ink(&picture, &pill);
        assert!(top <= bottom, "the text has dark pixels at {dpi} DPI");
        // Digits as high as the room above and below them, to a pixel.
        let (above, below) = (top - pill.top, pill.bottom - 1 - bottom);
        assert!(
            (above - below).abs() <= 1,
            "{above} above, {below} below at {dpi} DPI"
        );
        // And about the padding to each side.
        let (before, after) = (left - pill.left, pill.right - 1 - right);
        assert!(
            (before - after).abs() <= 2,
            "{before} before, {after} after at {dpi} DPI"
        );
        assert!(
            before >= scaled(geometry::PAD_X, dpi) - 1,
            "{before} before at {dpi} DPI"
        );
    }
}

#[test]
fn the_shadow_is_subtle_and_stays_near_the_pill() {
    let canvas = Canvas::new(480, 320);
    let m = horizontal(-400, -301, 350, "24");
    let mut pill = None;
    let picture = canvas.render(MONITOR, 96, |overlay, target| {
        pill = draw_label(overlay, target, &m);
    });
    let pill = pill.expect("a pill");
    let at = |x: i32, y: i32| picture.at((x - MONITOR.left) as usize, (y - MONITOR.top) as usize);
    let centre = (pill.left + pill.right) / 2;
    // Just below the pill it darkens the photo, but by far less than the outline's 28%.
    let below = at(centre, pill.bottom + 1);
    assert!(below[0] < BACKGROUND[0], "{below:?}");
    assert!(below[0] > 150, "{below:?}");
    // Three deviations of the blur (CSS 7px, a deviation of 3.5) and a pixel for the antialiased corners beyond the
    // pill, and 2 further below for the drop, nothing is left: the blur's own reach, since WPC-393 put the pill's
    // shadow on `relief`'s blur. It reaches a few pixels further than the 5 fixed layers did (8 below and 6 beside),
    // which the owner approved on 2026-11-06 (WPC-393).
    let reach = 11 + 1;
    assert_eq!(at(centre, pill.bottom + 2 + reach), BACKGROUND);
    assert_eq!(
        at(pill.left - reach, (pill.top + pill.bottom) / 2),
        BACKGROUND
    );
}

#[test]
fn the_cross_label_reads_width_by_height() {
    let canvas = Canvas::new(480, 320);
    let m = cross((-350, -100), (230, 480), -230, 360, "240 \u{d7} 180");
    let mut pill = None;
    canvas.render(MONITOR, 96, |overlay, target| {
        pill = draw_label(overlay, target, &m);
    });
    let pill = pill.expect("a pill");
    // Below and to the right of the crossing, and wide enough for nine characters.
    assert_eq!((pill.left, pill.top), (-230 + 12, 360 + 12));
    assert!(pill.right - pill.left > 60, "{pill:?}");
}

#[test]
fn drawing_all_leaves_the_line_exact_beside_the_label() {
    let canvas = Canvas::new(480, 320);
    for dpi in [96, 168] {
        let m = horizontal(-400, -301, 350, "100");
        let k = scaled(4, dpi);
        let picture = canvas.render(MONITOR, dpi, |overlay, target| {
            overlay
                .draw(target, &[], Some(&m), None)
                .expect("the measurement");
        });
        for x in 80..180 {
            assert_eq!(picture.at(x, 150), CYAN, "column {x} at {dpi} DPI");
        }
        for y in 150 - k as usize..=150 + k as usize {
            assert_eq!(picture.at(80, y), CYAN, "tick row {y} at {dpi} DPI");
        }
        // The label is above the line, so the pixels below it are the photo.
        assert_eq!(picture.at(130, 150 + scaled(9, dpi) as usize), BACKGROUND);
    }
}

#[test]
fn the_label_stays_in_the_window_at_the_monitors_limit() {
    let canvas = Canvas::new(480, 320);
    // A short span in the corner: the label would stick out on two sides without sliding in.
    let m = horizontal(-480, -478, 202, "3");
    let mut pill = None;
    canvas.render(MONITOR, 168, |overlay, target| {
        pill = draw_label(overlay, target, &m);
    });
    let pill = pill.expect("a pill");
    assert!(pill.left >= MONITOR.left && pill.top >= MONITOR.top);
    assert!(pill.right <= MONITOR.left + 480 && pill.bottom <= MONITOR.top + 320);
}

#[test]
fn the_labels_digits_are_as_wide_as_each_other() {
    // Without tabular figures Segoe UI's 1 is 4.8 pixels wide at 100% and its 0 is 6.7, so the pill would jump as
    // the number changed. With them every digit but the 7 is as wide as the 0, and the 7 takes the 0's room
    // (WPC-386), so the label counts the same whole pixels for any digits, in the physical text's format and in the
    // logical text's. Four of each, as one digit alone rounds up to the same pixel however narrow its 7 is.
    let canvas = Canvas::new(8, 8);
    for dpi in SCALES {
        canvas.render(MONITOR, dpi, |overlay, _| {
            for (part, format) in [
                ("physical", &overlay.format),
                ("logical", &overlay.logical_format),
            ] {
                let width = |text: &str| text_width(overlay, text, format);
                let zeros = width("0000");
                for digit in '1'..='9' {
                    let digits = digit.to_string().repeat(4);
                    assert_eq!(
                        width(&digits),
                        zeros,
                        "{digits} in the {part} format at {dpi} DPI"
                    );
                }
            }
        });
    }
}

#[test]
fn the_font_reports_a_capital_height_a_digit_can_use() {
    let fonts = Fonts::new().expect("the fonts");
    assert!(
        (0.6..0.8).contains(&fonts.cap_height),
        "{}",
        fonts.cap_height
    );
}

#[test]
fn the_pill_grows_with_the_text() {
    // Three digits are wider than one, and the pill is as tall for both.
    let canvas = Canvas::new(480, 320);
    let size_of = |label: &str| -> Size {
        let m = horizontal(-400, -301, 350, label);
        let mut pill = None;
        canvas.render(MONITOR, 96, |overlay, target| {
            pill = draw_label(overlay, target, &m);
        });
        let pill = pill.expect("a pill");
        Size {
            width: pill.right - pill.left,
            height: pill.bottom - pill.top,
        }
    };
    let (one, three) = (size_of("1"), size_of("100"));
    assert_eq!(one.height, three.height);
    assert!(three.width > one.width + 10);
}

/// Every pixel the model says is cyan, in the picture of the whole measurement, line and label together.
fn check_cyan_after_the_label(canvas: &Canvas, m: &Measurement, dpi: u32) {
    let k = scaled(4, dpi);
    let picture = canvas.render(MONITOR, dpi, |overlay, target| {
        overlay
            .draw(target, &[], Some(m), None)
            .expect("the measurement");
    });
    for y in 0..canvas.height as usize {
        for x in 0..canvas.width as usize {
            let (vx, vy) = (x as i32 + MONITOR.left, y as i32 + MONITOR.top);
            if expected(m, k, vx, vy) == CYAN {
                assert_eq!(
                    picture.at(x, y),
                    CYAN,
                    "pixel ({vx}, {vy}) at {dpi} DPI for {m:?}"
                );
            }
        }
    }
}

#[test]
fn the_label_and_its_shadow_never_darken_a_cyan_pixel() {
    // Short spans put the ticks under the label's shadow, which reaches 2 + 6 pixels below the pill.
    // The labels with a logical part are wider, so the pill and its shadow reach further along the line.
    let canvas = Canvas::new(480, 320);
    let labels = [
        (-400, "1", None),
        (-391, "10", None),
        (-377, "24", None),
        (-400, "1", Some("1.1")),
        (-377, "24", Some("13.7")),
        (-377, "240 \u{d7} 180", Some("137.1 \u{d7} 102.9")),
    ];
    for dpi in [96, 120, 144, 168] {
        for (last, label, logical) in labels {
            let with = |m: Measurement| match logical {
                Some(logical) => with_logical(m, logical),
                None => m,
            };
            let horizontal = with(horizontal(-400, last, 350, label));
            check_cyan_after_the_label(&canvas, &horizontal, dpi);
            let vertical = with(vertical(300, last + 700, -300, label));
            check_cyan_after_the_label(&canvas, &vertical, dpi);
            let cross = with(cross((-400, last), (300, last + 700), -400, 350, label));
            check_cyan_after_the_label(&canvas, &cross, dpi);
        }
    }
}

/// The ink at `opacity` over the pill's cyan: what the bar and the logical text's solid pixels are. Direct2D
/// blends each byte of the target, so rounding the same way lands within a level of it.
fn ink_over_cyan(opacity: f32) -> [u8; 3] {
    std::array::from_fn(|i| {
        (f32::from(INK[i]) * opacity + f32::from(CYAN[i]) * (1.0 - opacity)).round() as u8
    })
}

/// One label drawn alone, with what the tests know about it from outside the drawing code: the widths of its two
/// texts, laid out again on their own, and the scale.
struct Drawn {
    picture: Picture,
    pill: RECT,
    dpi: u32,
    physical_width: i32,
    /// 0 for a label without a logical part.
    logical_width: i32,
}

impl Drawn {
    /// A pixel, by its place in the virtual screen.
    fn at(&self, x: i32, y: i32) -> [u8; 3] {
        self.picture
            .at((x - MONITOR.left) as usize, (y - MONITOR.top) as usize)
    }

    /// The physical text's first column: the pill's padding in.
    fn text_left(&self) -> i32 {
        self.pill.left + scaled(8, self.dpi)
    }

    /// The bar's first column, after the physical text and a gap.
    fn bar_left(&self) -> i32 {
        self.text_left() + self.physical_width + scaled(7, self.dpi)
    }

    /// The logical text's first column, after the bar and another gap.
    fn logical_left(&self) -> i32 {
        self.bar_left() + scaled(1, self.dpi) + scaled(7, self.dpi)
    }

    /// The pill's rows, over the columns `left` to `right`.
    fn rows(&self, left: i32, right: i32) -> RECT {
        RECT {
            left,
            top: self.pill.top,
            right,
            bottom: self.pill.bottom,
        }
    }

    /// The darkest green in the columns `left` to `right` of the pill's rows. The ink is the only dark green on
    /// the cyan, whose green is 211.
    fn darkest_green(&self, left: i32, right: i32) -> u8 {
        let area = self.rows(left, right);
        (area.top..area.bottom)
            .flat_map(|y| (area.left..area.right).map(move |x| (x, y)))
            .map(|(x, y)| self.at(x, y)[1])
            .min()
            .expect("columns")
    }

    /// The columns of the pill with a run of at least `rows` pixels near `color` from top to bottom. A glyph's
    /// partly covered pixels can have a bar's color, but no glyph is as tall as the bar.
    fn columns_with_run(&self, color: [u8; 3], rows: i32) -> Vec<i32> {
        (self.pill.left..self.pill.right)
            .filter(|&x| {
                let mut run = 0;
                (self.pill.top..self.pill.bottom).any(|y| {
                    run = if near(self.at(x, y), color, 2) {
                        run + 1
                    } else {
                        0
                    };
                    run >= rows
                })
            })
            .collect()
    }
}

fn draw_alone(canvas: &Canvas, m: &Measurement, dpi: u32) -> Drawn {
    let mut found = None;
    let picture = canvas.render(MONITOR, dpi, |overlay, target| {
        let pill = draw_label(overlay, target, m).expect("a pill");
        let physical = text_width(overlay, &m.label.physical, &overlay.format);
        let logical = m
            .label
            .logical
            .as_deref()
            .map_or(0, |text| text_width(overlay, text, &overlay.logical_format));
        found = Some((pill, physical, logical));
    });
    let (pill, physical_width, logical_width) = found.expect("the label was drawn");
    Drawn {
        picture,
        pill,
        dpi,
        physical_width,
        logical_width,
    }
}

#[test]
fn a_label_without_a_logical_part_is_its_text_in_its_pill_with_no_bar() {
    let canvas = Canvas::new(480, 320);
    for dpi in SCALES {
        let d = draw_alone(&canvas, &horizontal(-400, -301, 350, "24"), dpi);
        let size = geometry::pill_size(d.physical_width, dpi);
        assert_eq!(
            (d.pill.right - d.pill.left, d.pill.bottom - d.pill.top),
            (size.width, size.height),
            "at {dpi} DPI"
        );
        let bar = ink_over_cyan(0.4);
        assert_eq!(
            d.columns_with_run(bar, scaled(11, dpi)),
            Vec::<i32>::new(),
            "at {dpi} DPI"
        );
    }
}

#[test]
fn a_logical_part_widens_the_pill_by_the_gaps_the_bar_and_its_text_and_not_its_height() {
    let canvas = Canvas::new(480, 320);
    for dpi in SCALES {
        let alone = draw_alone(&canvas, &horizontal(-400, -301, 350, "24"), dpi);
        let both = draw_alone(
            &canvas,
            &with_logical(horizontal(-400, -301, 350, "24"), "13.7"),
            dpi,
        );
        let height = |d: &Drawn| d.pill.bottom - d.pill.top;
        let width = |d: &Drawn| d.pill.right - d.pill.left;
        assert_eq!(height(&both), height(&alone), "at {dpi} DPI");
        assert!(both.logical_width > 10, "{}", both.logical_width);
        assert_eq!(
            width(&both) - width(&alone),
            scaled(7, dpi) + scaled(1, dpi) + scaled(7, dpi) + both.logical_width,
            "at {dpi} DPI"
        );
    }
}

#[test]
fn the_bar_is_ink_at_40_percent_scaled_wide_and_tall_between_two_gaps_of_cyan() {
    let canvas = Canvas::new(480, 320);
    let bar = ink_over_cyan(0.4);
    for dpi in SCALES {
        let m = with_logical(horizontal(-400, -301, 350, "24"), "13.7");
        let d = draw_alone(&canvas, &m, dpi);
        let (gap, width, height) = (scaled(7, dpi), scaled(1, dpi), scaled(11, dpi));

        // Where the columns of the bar are, found from the pixels: after the physical text and a gap.
        let left = d.bar_left();
        assert_eq!(
            d.columns_with_run(bar, height),
            (left..left + width).collect::<Vec<_>>(),
            "the bar's columns at {dpi} DPI"
        );

        // The digits stand on the baseline, so the first row under them is it, and the bar's bottom side hangs a
        // pixel (scaled) below it. The rows of the digits are read from the physical text's columns alone.
        let ((_, last_digit_row), _) = ink(
            &d.picture,
            &d.rows(d.text_left(), d.text_left() + d.physical_width),
        );
        let bottom = last_digit_row + 1 + scaled(1, dpi);
        let bar_rows = bottom - height..bottom;
        for x in left - gap..left + width + gap {
            let in_bar = (left..left + width).contains(&x);
            for y in d.pill.top..d.pill.bottom {
                if in_bar && bar_rows.contains(&y) {
                    assert!(
                        near(d.at(x, y), bar, 2),
                        "bar pixel ({x}, {y}) is {:?}, wanted {bar:?}, at {dpi} DPI",
                        d.at(x, y)
                    );
                } else {
                    assert_eq!(
                        d.at(x, y),
                        CYAN,
                        "({x}, {y}) is next to the bar at {dpi} DPI, so it is cyan"
                    );
                }
            }
        }
    }
}

#[test]
fn the_logical_text_is_fainter_than_the_physical_text() {
    let canvas = Canvas::new(480, 320);
    // The ink's solid pixels at 62% over the cyan have this green; a glyph's edge pixels only come lighter.
    let faint = i32::from(ink_over_cyan(0.62)[1]);
    for dpi in SCALES {
        let m = with_logical(horizontal(-400, -301, 350, "24"), "13.7");
        let d = draw_alone(&canvas, &m, dpi);
        let physical = i32::from(d.darkest_green(d.text_left(), d.text_left() + d.physical_width));
        let logical =
            i32::from(d.darkest_green(d.logical_left(), d.logical_left() + d.logical_width));
        assert!(
            (i32::from(INK[1])..=i32::from(INK[1]) + 2).contains(&physical),
            "the physical text is full ink: green {physical} at {dpi} DPI"
        );
        // Some pixel of the logical text is solid, and a solid pixel is 62% of the ink, no more and no less.
        assert!(
            logical.abs_diff(faint) <= 2,
            "the logical text's darkest green is {logical}, 62% of the ink is {faint}, at {dpi} DPI"
        );
        assert!(
            logical > physical + 50,
            "{logical} against {physical} at {dpi} DPI"
        );
    }
}

#[test]
fn the_texts_start_a_padding_in_and_a_gap_a_bar_and_a_gap_apart() {
    // A glyph's side bearings keep pixels from showing a shift of one pixel in where a text starts, so this reads
    // where the label put each text. The expected places are worked out here from the scale alone.
    let canvas = Canvas::new(480, 320);
    for dpi in SCALES {
        let m = with_logical(horizontal(-400, -301, 350, "24"), "13.7");
        let mut found = None;
        canvas.render(MONITOR, dpi, |overlay, _| {
            let label = overlay
                .label(&m, Look::Live)
                .expect("the label")
                .expect("a pill");
            let physical_width = text_width(overlay, "24", &overlay.format);
            found = Some((label, physical_width));
        });
        let (label, w) = found.expect("the label was made");
        let logical = label.logical.expect("a logical part");
        let physical_left = px(label.pill.left - MONITOR.left + scaled(8, dpi));
        assert_eq!(label.physical.left, physical_left, "at {dpi} DPI");
        let bar_left = physical_left + px(w + scaled(7, dpi));
        assert_eq!(logical.bar.left, bar_left, "at {dpi} DPI");
        assert_eq!(logical.bar.right, bar_left + px(scaled(1, dpi)));
        assert_eq!(
            logical.text.left,
            bar_left + px(scaled(1, dpi) + scaled(7, dpi)),
            "at {dpi} DPI"
        );
    }
}

#[test]
fn the_logical_text_sits_on_the_physical_texts_baseline() {
    let canvas = Canvas::new(480, 320);
    for dpi in SCALES {
        // Both parts say 24, whose 2 stands on a flat base.
        let m = with_logical(horizontal(-400, -301, 350, "24"), "24");
        let d = draw_alone(&canvas, &m, dpi);
        let last_row = |left: i32, right: i32| {
            (d.pill.top..d.pill.bottom)
                .rev()
                .find(|&y| (left..right).any(|x| d.at(x, y)[1] < 125))
                .expect("a dark row")
        };
        assert_eq!(
            last_row(d.logical_left(), d.logical_left() + d.logical_width),
            last_row(d.text_left(), d.text_left() + d.physical_width),
            "at {dpi} DPI"
        );
    }
}

#[test]
fn a_label_drawn_after_one_with_a_logical_part_still_has_full_ink() {
    // The ink brush keeps the opacity of the last thing drawn with it, and the overlay is used for every frame.
    let canvas = Canvas::new(480, 320);
    for dpi in [96, 168] {
        let first = with_logical(horizontal(-400, -301, 350, "24"), "13.7");
        let second = horizontal(-400, -301, 250, "24");
        let mut pills = Vec::new();
        let picture = canvas.render(MONITOR, dpi, |overlay, target| {
            pills.extend(draw_label(overlay, target, &first));
            pills.extend(draw_label(overlay, target, &second));
        });
        let [_, pill] = pills[..] else {
            panic!("two pills, got {pills:?}");
        };
        let darkest = (pill.top..pill.bottom)
            .flat_map(|y| (pill.left + 8..pill.right - 8).map(move |x| (x, y)))
            .map(|(x, y)| picture.at((x - MONITOR.left) as usize, (y - MONITOR.top) as usize)[1])
            .min()
            .expect("pixels");
        assert!(
            darkest <= INK[1] + 8,
            "the second label's darkest green is {darkest} at {dpi} DPI"
        );
    }
}

#[test]
fn a_long_logical_part_keeps_the_label_inside_the_window_at_the_monitors_limit() {
    let canvas = Canvas::new(480, 320);
    // A short span in the top left corner, as in the test of the plain label, then the Cross in the bottom right
    // corner, whose label mirrors up and to the left.
    let corner = with_logical(horizontal(-480, -478, 202, "3"), "1234567.8");
    let cross = with_logical(
        cross((-120, -1), (400, 519), -1, 519, "240 \u{d7} 180"),
        "137.1 \u{d7} 102.9",
    );
    for dpi in SCALES {
        for m in [&corner, &cross] {
            let d = draw_alone(&canvas, m, dpi);
            let pill = d.pill;
            assert!(
                pill.left >= MONITOR.left && pill.top >= MONITOR.top,
                "{pill:?}"
            );
            assert!(
                pill.right <= MONITOR.left + 480 && pill.bottom <= MONITOR.top + 320,
                "{pill:?} at {dpi} DPI for {m:?}"
            );
            // The logical text ends inside the pill's padding: the pill was made for it.
            assert!(
                d.logical_left() + d.logical_width <= pill.right - scaled(8, dpi),
                "{pill:?} at {dpi} DPI for {m:?}"
            );
            let (_, (_, right)) = ink(&d.picture, &pill);
            assert!(
                right < pill.right - scaled(8, dpi) + 2,
                "{right} in {pill:?}"
            );
        }
    }
}

#[test]
fn the_logical_part_is_in_the_same_semibold_the_owner_approved() {
    // Segoe UI has no face at weight 500, and DirectWrite takes its semibold for it, with no simulated bolding: the
    // label the owner approved on screen. Should Windows ever bring a face at 500, the logical part would change
    // weight from what was approved: this fails then, so it is looked at again.
    let fonts = Fonts::new().expect("the fonts");
    let family = font_family(&fonts.factory).expect("the family");
    let face = |weight| {
        // SAFETY: plain values.
        unsafe {
            family.GetFirstMatchingFont(
                weight,
                DWRITE_FONT_STRETCH_NORMAL,
                DWRITE_FONT_STYLE_NORMAL,
            )
        }
        .expect("a face")
    };
    let (logical, physical) = (face(LOGICAL_WEIGHT), face(DWRITE_FONT_WEIGHT_SEMI_BOLD));
    // SAFETY: plain getters.
    let (logical_weight, physical_weight, simulations) = unsafe {
        (
            logical.GetWeight().0,
            physical.GetWeight().0,
            logical.GetSimulations(),
        )
    };
    assert_eq!(logical_weight, physical_weight);
    assert_eq!(simulations, DWRITE_FONT_SIMULATIONS_NONE);
}
