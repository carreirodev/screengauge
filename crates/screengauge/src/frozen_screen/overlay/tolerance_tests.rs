//! Render tests of the tolerance indicator on a WIC bitmap, like the notice's in `panel_tests`. The sizes here are the
//! prototype's (WPC-279), in logical pixels, scaled in the tests, and not read back from the code that is being tested.

use screengauge_core::session::{Point, Sign, ToleranceIndicator};
use screengauge_core::settings::Tolerance;
use windows::Win32::Graphics::DirectWrite::DWRITE_FONT_SIMULATIONS_NONE;

use super::panel_tests::{MODES, at, inside, over, pixels, rect, strongest_green, tokens};
use super::tests::{
    BACKGROUND, Canvas, MONITOR, Picture, SCALES, horizontal, near, point, text_width, with_logical,
};
use super::*;
use super::{notice, tolerance};

/// One indicator drawn alone, with what the tests know about it from outside the drawing code: the widths of its two
/// texts, laid out again on their own, and the scale.
struct Drawn {
    picture: Picture,
    panel: RECT,
    word: i32,
    number: i32,
    dpi: u32,
}

/// The prototype's sizes, scaled for `dpi` one at a time, as every size is.
struct Sizes {
    ring: i32,
    pad_x: i32,
    pad_y: i32,
    line: i32,
    gap: i32,
    segment: i32,
    height: i32,
    between: i32,
}

fn sizes(dpi: u32) -> Sizes {
    Sizes {
        ring: scaled(1, dpi),
        pad_x: scaled(9, dpi),
        pad_y: scaled(6, dpi),
        line: scaled(11, dpi),
        gap: scaled(8, dpi),
        segment: scaled(4, dpi),
        height: scaled(10, dpi),
        between: scaled(2, dpi),
    }
}

impl Drawn {
    fn sizes(&self) -> Sizes {
        sizes(self.dpi)
    }

    /// The bar's left and right sides: its right is the padding and the border from the panel's.
    fn bar(&self) -> (i32, i32) {
        let s = self.sizes();
        let right = self.panel.right - s.ring - s.pad_x;
        (right - (10 * s.segment + 9 * s.between), right)
    }

    /// The word's left side, and the number's left side: after the word and a gap.
    fn texts(&self) -> (i32, i32) {
        let s = self.sizes();
        let word = self.panel.left + s.ring + s.pad_x;
        (word, word + self.word + s.gap)
    }

    /// The top of the text's line.
    fn line_top(&self) -> i32 {
        let s = self.sizes();
        self.panel.top + s.ring + s.pad_y
    }

    /// The middle of segment `index` along the bar, and the row halfway down the bar's usual height.
    fn segment_middle(&self, index: i32) -> (i32, i32) {
        let s = self.sizes();
        let (left, _) = self.bar();
        (
            left + index * (s.segment + s.between) + s.segment / 2,
            self.line_top() + s.line / 2,
        )
    }
}

/// The indicator for `value` drawn in `mode` with the cursor at `cursor`.
fn render_indicator(canvas: &Canvas, mode: Mode, dpi: u32, cursor: Point, value: u8) -> Drawn {
    let value = Tolerance::new(value).expect("a tolerance");
    let mut found = None;
    let picture = canvas.render_in(mode, MONITOR, dpi, |overlay, target| {
        overlay
            .show_tolerance(target, cursor, value)
            .expect("the indicator");
        // The word has no digits to align, so it is laid out without the tabular figures, which would take its kerning.
        let word = overlay
            .typeset_plain("Tolerance", &overlay.indicator_format)
            .expect("the word")
            .width;
        let number = text_width(overlay, &value.get().to_string(), &overlay.number_format);
        found = Some((word, number));
    });
    let (word, number) = found.expect("the indicator was drawn");
    let panel = tolerance::place(
        cursor,
        tolerance::size(word, number, dpi),
        geometry::bounds(MONITOR).expect("the monitor"),
        dpi,
    );
    Drawn {
        picture,
        panel,
        word,
        number,
        dpi,
    }
}

/// The rows of the column `x` that differ clearly from `base`, as the first and the last, in the virtual screen.
fn extent(picture: &Picture, x: i32, from: i32, to: i32, base: [u8; 3]) -> Option<(i32, i32)> {
    let rows: Vec<i32> = (from..to)
        .filter(|&y| at(picture, x, y)[1].abs_diff(base[1]) > 20)
        .collect();
    Some((*rows.first()?, *rows.last()?))
}

/// An indicator in the middle of the window, with room all round for its shadow.
const CURSOR: Point = Point { x: -100, y: 450 };

#[test]
fn an_indicator_is_the_panel_in_a_ring_with_a_highlight_inside_its_top() {
    let canvas = Canvas::new(480, 320);
    for mode in MODES {
        let t = tokens(mode);
        let panel_color = t.panel_over(BACKGROUND);
        let ring_color = t.ring_over_panel();
        let highlight = over(t.highlight.0, t.highlight.1, panel_color);
        for dpi in SCALES {
            let d = render_indicator(&canvas, mode, dpi, CURSOR, 25);
            let (panel, s) = (d.panel, d.sizes());
            let context = format!("{mode:?} at {dpi} DPI");
            let (middle, across) = (
                (panel.top + panel.bottom) / 2,
                (panel.left + panel.right) / 2,
            );
            // The border, the padding and the line, each scaled: 25 tall at 100%.
            assert_eq!(
                panel.bottom - panel.top,
                s.ring + s.pad_y + s.line + s.pad_y + s.ring,
                "{context}"
            );
            let check = |x: i32, y: i32, wanted: [u8; 3], what: &str| {
                assert!(
                    near(at(&d.picture, x, y), wanted, 2),
                    "{what} at ({x}, {y}) is {:?}, wanted {wanted:?}, {context}",
                    at(&d.picture, x, y)
                );
            };
            for k in 0..s.ring {
                check(across, panel.top + k, ring_color, "the top ring");
                check(across, panel.bottom - 1 - k, ring_color, "the bottom ring");
                check(panel.left + k, middle, ring_color, "the left ring");
                check(panel.right - 1 - k, middle, ring_color, "the right ring");
                // The highlight is as thick as a pixel scaled, along the inside of the top ring; then the panel.
                check(
                    across,
                    panel.top + s.ring + k.min(scaled(1, dpi) - 1),
                    highlight,
                    "the highlight",
                );
            }
            check(
                across,
                panel.top + s.ring + scaled(1, dpi),
                panel_color,
                "below the highlight",
            );
            // Between the bar's end and the border, in the padding: the panel with no shadow showing through it.
            check(
                panel.right - s.ring - s.pad_x / 2,
                middle,
                panel_color,
                "the panel",
            );
            // Outside the ring on every side there is the photo, a little darkened by the shadow below, and the ring is
            // nowhere else: one pixel beyond it is not its color.
            for (x, y) in [
                (across, panel.top - 1),
                (across, panel.bottom),
                (panel.left - 1, middle),
                (panel.right, middle),
            ] {
                assert!(
                    !near(at(&d.picture, x, y), ring_color, 6),
                    "({x}, {y}) is outside the indicator but is {:?}, {context}",
                    at(&d.picture, x, y)
                );
            }
            // The corners are round: the corner pixel is the photo, and the ring is back along the top by the radius
            // of 9.
            assert!(
                !near(at(&d.picture, panel.left, panel.top), ring_color, 20),
                "{:?}, {context}",
                at(&d.picture, panel.left, panel.top)
            );
            check(
                panel.left + scaled(9, dpi) + 1,
                panel.top,
                ring_color,
                "the top ring past the corner",
            );
        }
    }
}

#[test]
fn an_indicator_is_drawn_where_the_rule_puts_it() {
    // Worked out by hand at 100%: its right side 14 left of the cursor and its bottom side 14 above it, 156 wide for a
    // word and a number of those widths, and slid to 2 inside the monitor's sides where that sticks out, never to the
    // other side of the cursor.
    let canvas = Canvas::new(480, 320);
    let t = tokens(Mode::Dark);
    let ring = t.ring_over_panel();
    for (cursor, name) in [
        (CURSOR, "above and to the left"),
        (point(-470, 450), "slid in from the left side"),
        (point(-300, 205), "slid in from the top"),
        (
            point(-2, 450),
            "near the right side, still left of the cursor",
        ),
        (point(-470, 205), "slid in at the corner"),
    ] {
        let d = render_indicator(&canvas, Mode::Dark, 96, cursor, 25);
        let panel = d.panel;
        // "Tolerance" and "25" make the panel 94 plus their widths wide, and 25 tall.
        assert_eq!(panel.right - panel.left, 94 + d.word + d.number, "{name}");
        assert_eq!(panel.bottom - panel.top, 25, "{name}");
        let monitor = geometry::bounds(MONITOR).expect("the monitor");
        let (right, bottom) = (cursor.x - 14, cursor.y - 14);
        let left = (right - (94 + d.word + d.number)).max(monitor.left + 2);
        let top = (bottom - 25).max(monitor.top + 2);
        assert_eq!((panel.left, panel.top), (left, top), "{name}");
        // Where the cursor is near the left side the panel slides over it: it does not go to the other side.
        assert!(
            panel.right <= monitor.right - 2 && panel.bottom <= monitor.bottom - 2,
            "{name}"
        );
        // The ring is at the rectangle's sides, and one pixel outside them it isn't.
        let across = (panel.left + panel.right) / 2;
        let middle = (panel.top + panel.bottom) / 2;
        for (x, y) in [
            (across, panel.top),
            (across, panel.bottom - 1),
            (panel.left, middle),
            (panel.right - 1, middle),
        ] {
            assert!(
                near(at(&d.picture, x, y), ring, 2),
                "{name}: ({x}, {y}) is {:?}",
                at(&d.picture, x, y)
            );
        }
        for (x, y) in [
            (across, panel.top - 1),
            (across, panel.bottom),
            (panel.left - 1, middle),
            (panel.right, middle),
        ] {
            assert!(
                !near(at(&d.picture, x, y), ring, 6),
                "{name}: ({x}, {y}) is {:?}",
                at(&d.picture, x, y)
            );
        }
    }
}

#[test]
fn the_bar_is_ten_segments_of_4_by_10_with_2_between_them_in_the_middle_of_the_line() {
    let canvas = Canvas::new(480, 320);
    for mode in MODES {
        let panel_color = tokens(mode).panel_over(BACKGROUND);
        for dpi in SCALES {
            for value in [2, 25, 50] {
                let d = render_indicator(&canvas, mode, dpi, CURSOR, value);
                let s = d.sizes();
                let context = format!("{mode:?} at {dpi} DPI, tolerance {value}");
                let (bar_left, bar_right) = d.bar();
                let line_top = d.line_top();
                let (mut tops, mut bottoms) = (Vec::new(), Vec::new());
                for index in 0..10 {
                    let (middle, row) = d.segment_middle(index);
                    // Its height, down the middle column: the line's rows and the padding beside it have nothing
                    // else in them.
                    let (top, bottom) = extent(
                        &d.picture,
                        middle,
                        d.panel.top + s.ring,
                        d.panel.bottom - s.ring,
                        panel_color,
                    )
                    .unwrap_or_else(|| panic!("segment {index} has pixels, {context}"));
                    assert_eq!(
                        bottom - top + 1,
                        s.height,
                        "segment {index} is 10 tall, scaled, {context}"
                    );
                    tops.push(top);
                    bottoms.push(bottom);
                    // Its width, along its middle row: the pixels before and after it are the panel's.
                    let left = bar_left + index * (s.segment + s.between);
                    let covered: Vec<i32> = (left - s.between..left + s.segment + s.between)
                        .filter(|&x| at(&d.picture, x, row)[1].abs_diff(panel_color[1]) > 20)
                        .collect();
                    assert_eq!(
                        (covered.first().copied(), covered.last().copied()),
                        (Some(left), Some(left + s.segment - 1)),
                        "segment {index} is 4 wide, scaled, and at its step, {context}"
                    );
                    assert_eq!(covered.len(), usize::try_from(s.segment).expect("a width"));
                }
                // All ten on the same rows.
                assert!(tops.iter().all(|&t| t == tops[0]), "{tops:?}, {context}");
                assert!(
                    bottoms.iter().all(|&b| b == bottoms[0]),
                    "{bottoms:?}, {context}"
                );
                // Centered in the line, the spare room above and below it equal but for the pixel that cannot be
                // shared, which goes below it: half a pixel rounds up.
                let above = tops[0] - line_top;
                let below = line_top + s.line - 1 - bottoms[0];
                assert!(
                    (0..=1).contains(&(above - below)),
                    "{above} above and {below} below, {context}"
                );
                // The last segment ends where the padding begins.
                assert_eq!(bar_right, d.panel.right - s.ring - s.pad_x, "{context}");
            }
        }
    }
}

#[test]
fn the_bar_lights_as_many_segments_in_the_accent_as_the_tolerance_asks_for_and_the_rest_faintly() {
    let canvas = Canvas::new(480, 320);
    for mode in MODES {
        let t = tokens(mode);
        let panel_color = t.panel_over(BACKGROUND);
        let faint = over(t.ink, 0.22, panel_color);
        for dpi in SCALES {
            // The prototype's table: the tolerance, and how many segments are lit with a highest of 50.
            for (value, lit) in [(0, 0), (2, 0), (3, 1), (25, 5), (47, 9), (48, 10), (50, 10)] {
                let d = render_indicator(&canvas, mode, dpi, CURSOR, value);
                let context = format!("{mode:?} at {dpi} DPI, tolerance {value}");
                for index in 0..10 {
                    let (x, y) = d.segment_middle(index);
                    let (wanted, what) = if index < lit {
                        (t.accent, "lit")
                    } else {
                        (faint, "unlit")
                    };
                    assert!(
                        near(at(&d.picture, x, y), wanted, 2),
                        "segment {index} should be {what}: it is {:?}, wanted {wanted:?}, {context}",
                        at(&d.picture, x, y)
                    );
                }
            }
        }
    }
}

#[test]
fn the_word_and_the_number_are_in_the_panels_ink_on_one_baseline_inside_the_line() {
    let canvas = Canvas::new(480, 320);
    for mode in MODES {
        let t = tokens(mode);
        let panel_color = t.panel_over(BACKGROUND);
        for dpi in SCALES {
            let d = render_indicator(&canvas, mode, dpi, CURSOR, 47);
            let s = d.sizes();
            let (word_left, number_left) = d.texts();
            let (bar_left, _) = d.bar();
            let line = rect(
                word_left,
                d.line_top(),
                bar_left - s.gap,
                d.line_top() + s.line,
            );
            let context = format!("{mode:?} at {dpi} DPI");
            let ink = |from: i32, to: i32| -> Vec<(i32, i32)> {
                pixels(rect(
                    from,
                    d.panel.top + s.ring,
                    to,
                    d.panel.bottom - s.ring,
                ))
                .filter(|&(x, y)| at(&d.picture, x, y)[1].abs_diff(panel_color[1]) > 60)
                .collect()
            };
            let word = ink(word_left, number_left - s.gap + 1);
            let number = ink(number_left - s.gap / 2, number_left + d.number);
            assert!(!word.is_empty() && !number.is_empty(), "{context}");
            // Some pixel of each is in full ink.
            for (name, from, to) in [
                ("word", word_left, number_left - s.gap),
                ("number", number_left, number_left + d.number),
            ] {
                let strongest =
                    strongest_green(&d.picture, rect(from, line.top, to, line.bottom), mode);
                assert!(
                    strongest.abs_diff(t.ink[1]) <= 3,
                    "the {name}'s strongest green is {strongest}, the ink's {}, {context}",
                    t.ink[1]
                );
            }
            // Neither has a letter or a digit that drops below its line, so all of both is inside it, and the pixels
            // between the texts and between the number and the bar are bare.
            for &(x, y) in word.iter().chain(&number) {
                assert!(
                    inside(&line, x, y),
                    "ink at ({x}, {y}) is outside the line {line:?}, {context}"
                );
            }
            // The word ends before the number begins, and the number before the bar.
            let word_right = word.iter().map(|&(x, _)| x).max().expect("ink");
            let number_first = number.iter().map(|&(x, _)| x).min().expect("ink");
            let number_last = number.iter().map(|&(x, _)| x).max().expect("ink");
            assert!(word_right < number_left, "{word_right}, {context}");
            assert!(number_first >= number_left, "{context}");
            assert!(number_last < bar_left, "{context}");
            // The gap between them is the prototype's 8 less the room the letters keep inside their advances.
            assert!(
                number_first - word_right <= s.gap + scaled(3, dpi),
                "{} between the texts, {context}",
                number_first - word_right
            );
            // One baseline: the last row of ink is the same for the word and the number.
            let bottom = |ink: &[(i32, i32)]| ink.iter().map(|&(_, y)| y).max().expect("ink");
            assert_eq!(
                bottom(&word),
                bottom(&number),
                "the word's ink ends at row {}, the number's at {}, {context}",
                bottom(&word),
                bottom(&number)
            );
            // At 100% the baseline is 10 below the top of the line: in Chromium the text's content area starts 2 above the
            // line and its ascent is 12 (a "Tolerance" at 11px is 14.67 tall at 1.5 with the line at 16.5). The digits sit
            // on it, so the last row of the number's ink is the line's tenth.
            if dpi == 96 {
                assert_eq!(bottom(&number), line.top + 9, "{context}");
            }
            // And the texts are not pushed to one end of the line: they start within a few pixels of its top, and
            // end well above its bottom, where a baseline sits.
            let first = word
                .iter()
                .chain(&number)
                .map(|&(_, y)| y)
                .min()
                .expect("ink");
            assert!(
                (0..=scaled(4, dpi)).contains(&(first - line.top)),
                "{} rows above the ink, {context}",
                first - line.top
            );
            assert!(
                (0..=scaled(3, dpi)).contains(&(line.bottom - 1 - bottom(&word))),
                "{} rows below the ink, {context}",
                line.bottom - 1 - bottom(&word)
            );
        }
    }
}

#[test]
fn an_indicators_shadow_falls_below_it_and_nothing_is_drawn_beyond_it() {
    let canvas = Canvas::new(480, 320);
    for mode in MODES {
        for dpi in SCALES {
            let d = render_indicator(&canvas, mode, dpi, CURSOR, 25);
            let context = format!("{mode:?} at {dpi} DPI");
            let across = (d.panel.left + d.panel.right) / 2;
            let below = at(&d.picture, across, d.panel.bottom + scaled(4, dpi));
            assert!(below[0] < BACKGROUND[0], "{below:?} is darkened, {context}");
            assert!(below[0] > 120, "{below:?} is only a shadow, {context}");
            // The shadow's reach: 36 less its spread of 8 beyond the panel on the sides and above it, and 10 further
            // below, and a pixel or two for the blur's last band. Beyond it the photo is as it was, to the last pixel.
            let reach = scaled(28, dpi) + 2;
            let drop = scaled(10, dpi);
            let reached = rect(
                d.panel.left - reach,
                d.panel.top - reach,
                d.panel.right + reach,
                d.panel.bottom + reach + drop,
            );
            let mut shadowed = 0;
            for (x, y) in pixels(rect(
                MONITOR.left,
                MONITOR.top,
                MONITOR.left + 480,
                MONITOR.top + 320,
            )) {
                if inside(&reached, x, y) {
                    shadowed += usize::from(at(&d.picture, x, y) != BACKGROUND);
                } else {
                    assert_eq!(
                        at(&d.picture, x, y),
                        BACKGROUND,
                        "({x}, {y}) is beyond the shadow, {context}"
                    );
                }
            }
            assert!(shadowed > 1000, "{shadowed} pixels drawn, {context}");
        }
    }
}

#[test]
fn the_indicators_shadow_is_fainter_than_the_notices_because_its_panel_is_lower() {
    // The same panel color and the same layers: only the strength of the shadow differs, and a panel 25 tall casts less
    // than one 31 tall. Measured below the panel, where the shadow is darkest.
    let canvas = Canvas::new(480, 320);
    for mode in MODES {
        for dpi in [96, 168] {
            let indicator = render_indicator(&canvas, mode, dpi, CURSOR, 25);
            let mut notice_panel = None;
            let notice_picture = canvas.render_in(mode, MONITOR, dpi, |overlay, target| {
                overlay
                    .show_notice(target, CURSOR, "Copied", Sign::Ok)
                    .expect("the notice");
                let width = text_width(overlay, "Copied", &overlay.format);
                notice_panel = Some(notice::place(
                    CURSOR,
                    notice::size(width, dpi),
                    geometry::bounds(MONITOR).expect("the monitor"),
                    dpi,
                ));
            });
            let notice_panel = notice_panel.expect("the notice was drawn");
            let darkness = |picture: &Picture, panel: &RECT| {
                let across = (panel.left + panel.right) / 2;
                let y = panel.bottom + scaled(4, dpi);
                i32::from(BACKGROUND[0]) - i32::from(at(picture, across, y)[0])
            };
            let (own, notices) = (
                darkness(&indicator.picture, &indicator.panel),
                darkness(&notice_picture, &notice_panel),
            );
            let context = format!("{mode:?} at {dpi} DPI");
            assert!(own > 0, "{own} levels of shadow, {context}");
            // Light mode's shadow is faint to begin with, so compare in whole levels, allowing the rounding.
            assert!(
                own < notices,
                "{own} against the notice's {notices}, {context}"
            );
        }
    }
}

#[test]
fn the_public_indicator_is_the_one_that_shows_the_tolerance_at_the_cursor() {
    let canvas = Canvas::new(480, 320);
    for mode in MODES {
        let indicator = ToleranceIndicator {
            tolerance: Tolerance::new(33).expect("a tolerance"),
            at: CURSOR,
            monitor: 0,
        };
        let drawn = canvas.render_in(mode, MONITOR, 120, |overlay, target| {
            overlay
                .draw_tolerance(target, &indicator)
                .expect("the indicator");
        });
        let by_hand = render_indicator(&canvas, mode, 120, CURSOR, 33);
        assert!(drawn.pixels == by_hand.picture.pixels, "{mode:?}");
    }
}

#[test]
fn the_ink_has_its_full_strength_whatever_opacity_the_brush_was_left_at_and_leaves_none_behind() {
    // The ink brush outlives a frame. A pinned label with a logical part leaves it at 62%, and the indicator's bar
    // leaves it at 22%. In the order of a frame the label, the indicator, another pinned label and a notice are each
    // in full ink all the same.
    let canvas = Canvas::new(480, 320);
    for mode in MODES {
        let t = tokens(mode);
        for dpi in [96, 168] {
            let first = with_logical(horizontal(-400, -301, 250, "24"), "13.7");
            let later = horizontal(-400, -301, 300, "24");
            let notice_at = point(-300, 340);
            let mut found = None;
            let picture = canvas.render_in(mode, MONITOR, dpi, |overlay, target| {
                overlay
                    .draw(target, &[&first], None)
                    .expect("the first label");
                overlay
                    .show_tolerance(target, CURSOR, Tolerance::MAX)
                    .expect("the indicator");
                overlay.draw(target, &[&later], None).expect("the label");
                overlay
                    .show_notice(target, notice_at, "Copied", Sign::Ok)
                    .expect("the notice");
                let word = overlay
                    .typeset_plain("Tolerance", &overlay.indicator_format)
                    .expect("the word")
                    .width;
                let number = text_width(overlay, "50", &overlay.number_format);
                let bounds = geometry::bounds(MONITOR).expect("the monitor");
                let copied = text_width(overlay, "Copied", &overlay.format);
                found = Some((
                    tolerance::place(CURSOR, tolerance::size(word, number, dpi), bounds, dpi),
                    word,
                    overlay.label(&later, Look::Pinned).expect("a label"),
                    notice::place(notice_at, notice::size(copied, dpi), bounds, dpi),
                ));
            });
            let (indicator, word, label, notice) = found.expect("drawn");
            let label = label.expect("a label");
            let ring = scaled(1, dpi);
            let border = geometry::border(dpi);
            let word_left = indicator.left + ring + scaled(9, dpi);
            let areas = [
                (
                    "the indicator",
                    rect(
                        word_left,
                        indicator.top + ring,
                        word_left + word,
                        indicator.bottom - ring,
                    ),
                ),
                (
                    "the label",
                    rect(
                        label.pill.left + border.strip + 4,
                        label.pill.top,
                        label.pill.right - 4,
                        label.pill.bottom,
                    ),
                ),
                (
                    "the notice",
                    rect(
                        notice.left + ring + scaled(9, dpi) + scaled(13, dpi) + scaled(7, dpi),
                        notice.top + ring,
                        notice.right - ring - scaled(11, dpi),
                        notice.bottom - ring,
                    ),
                ),
            ];
            for (name, area) in areas {
                let strongest = strongest_green(&picture, area, mode);
                assert!(
                    strongest.abs_diff(t.ink[1]) <= 3,
                    "{name}'s strongest green is {strongest}, the ink's {}, {mode:?} at {dpi} DPI",
                    t.ink[1]
                );
            }
        }
    }
}

#[test]
fn the_number_is_in_segoe_ui_black_and_the_texts_are_as_wide_as_chromium_laid_them_out() {
    // Chromium's numbers, measured on Windows 11 for WPC-309 at 11px: the prototype's `<b>` computes to weight 900, and
    // then a "0" is 6.594 wide, a "47" 13.427, a "25" or "50" 13.188; the word in semibold is 47.542.
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
    let black = face(tolerance::NUMBER_WEIGHT);
    // SAFETY: plain getters.
    let (weight, simulations) = unsafe { (black.GetWeight().0, black.GetSimulations()) };
    // A real face of weight 900, and not the bold (700) with a simulated bolding on it.
    assert_eq!(weight, 900);
    assert_eq!(simulations, DWRITE_FONT_SIMULATIONS_NONE);
    let canvas = Canvas::new(480, 320);
    let scale = Scale::from_dpi(96).expect("a scale");
    let overlay = Overlay::new(&canvas.target, &canvas.fonts, MONITOR, scale, Mode::Dark)
        .expect("the overlay");
    let width = |text: &str, format: &IDWriteTextFormat, tabular: bool| {
        let layout = overlay.layout_as(text, format, tabular).expect("a layout");
        let mut metrics = DWRITE_TEXT_METRICS::default();
        // SAFETY: fills a local.
        unsafe { layout.GetMetrics(&mut metrics) }.expect("metrics");
        metrics.width
    };
    for (text, format, tabular, chromium) in [
        ("Tolerance", &overlay.indicator_format, false, 47.542),
        ("0", &overlay.number_format, true, 6.594),
        ("25", &overlay.number_format, true, 13.188),
        ("47", &overlay.number_format, true, 13.427),
        ("50", &overlay.number_format, true, 13.188),
    ] {
        let ours = width(text, format, tabular);
        assert!(
            (ours - chromium).abs() < 0.1,
            "{text:?} is {ours} wide, Chromium's is {chromium}"
        );
    }
}
