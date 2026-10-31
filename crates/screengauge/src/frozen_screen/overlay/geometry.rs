//! Where Dimensions' live measurement lands, as whole-pixel rectangles in physical pixels of the virtual screen.
//! Nothing here calls Windows, so the pixel counts the owner checks by eye are tested without a monitor: [`parts`]
//! gives the line, its ticks and their outline, and [`place_label`] gives the label's place.

use screengauge_core::session::{Measurement, Rect, Span};
use windows::Win32::Foundation::{E_FAIL, RECT};
use windows::core::Error;

// Every size below is in logical pixels, which `scaled` turns into physical pixels for the monitor's scale.

/// How far a tick reaches to each side of the line.
const TICK: u32 = 4;
/// The gap between a horizontal line and its label, above or below.
const ABOVE_GAP: u32 = 9;
/// The gap between a vertical line and its label, to its right or left.
const SIDE_GAP: u32 = 10;
/// How far the Cross label's corner sits from the crossing, along each axis.
const CROSS_OFFSET: u32 = 12;
/// The label's text size, and the padding around the text inside the pill.
pub const TEXT: u32 = 12;
pub const PAD_X: u32 = 8;
const PAD_Y: u32 = 5;
/// The pill's corner radius, and how far its shadow falls.
pub const RADIUS: u32 = 7;
pub const SHADOW_DROP: u32 = 2;
/// Between the physical and the logical part of a label: a gap, a bar, and a gap again. The bar is this wide and tall,
/// and its bottom side hangs this far below the text's baseline (CSS `vertical-align: -1px` on an empty box).
const BAR_GAP: u32 = 7;
const BAR_WIDTH: u32 = 1;
const BAR_HEIGHT: u32 = 11;
const BAR_DROP: u32 = 1;
/// A pinned label's border (prototype CSS `border: 1px solid`, with `border-left: 3px solid` in cyan): a ring on its
/// top, right and bottom sides, and a wider strip on its left.
pub const RING: u32 = 1;
const STRIP: u32 = 3;

/// `logical` pixels on a monitor of `dpi` (96 is 100%), in whole physical pixels, halves up.
pub fn scaled(logical: u32, dpi: u32) -> i32 {
    // The saturated product divided by 96 is at most 44 million, so the sign never flips.
    (logical.saturating_mul(dpi).saturating_add(48) / 96).cast_signed()
}

/// A monitor's rectangle, with its right and bottom sides one past its last pixel, as Windows has it.
pub fn bounds(monitor: Rect) -> windows::core::Result<RECT> {
    let too_large = || Error::new(E_FAIL, "the monitor is too large to draw on");
    let width = i32::try_from(monitor.width).map_err(|_| too_large())?;
    let height = i32::try_from(monitor.height).map_err(|_| too_large())?;
    Ok(RECT {
        left: monitor.left,
        top: monitor.top,
        right: monitor.left.checked_add(width).ok_or_else(too_large)?,
        bottom: monitor.top.checked_add(height).ok_or_else(too_large)?,
    })
}

/// What to fill for one measurement, all in whole pixels of the virtual screen.
#[derive(Debug, PartialEq)]
pub struct Parts {
    /// The translucent outline: the pixels beside the line along its whole length and beside each tick, with no two
    /// rectangles overlapping, so no pixel is darkened twice where an outline meets another.
    pub outline: Vec<RECT>,
    /// The opaque cyan pixels: each span and the tick through its first and last pixel. These may overlap.
    pub line: Vec<RECT>,
}

/// The line, ticks and outline of `measurement`, for a monitor of `dpi`. A span is exactly its pixels, one pixel thick
/// whatever the scale; a tick is a perpendicular run of 2k + 1 pixels through the span's first pixel and another
/// through its last, where k is the tick's reach scaled.
pub fn parts(measurement: &Measurement, dpi: u32) -> Parts {
    let reach = scaled(TICK, dpi);
    let mut line = Vec::new();
    let mut outline = Vec::new();
    if let Some(Span { first, last }) = measurement.horizontal {
        add_span(
            first.x,
            last.x,
            first.y,
            reach,
            false,
            &mut line,
            &mut outline,
        );
    }
    if let Some(Span { first, last }) = measurement.vertical {
        add_span(
            first.y,
            last.y,
            first.x,
            reach,
            true,
            &mut line,
            &mut outline,
        );
    }
    Parts {
        outline: disjoint(&outline),
        line,
    }
}

/// Adds the pieces of a span along a row: its pixels run from x = `from` to `to` on row `across`. With `column`, every
/// rectangle swaps its axes, which makes it a span along a column instead.
fn add_span(
    from: i32,
    to: i32,
    across: i32,
    reach: i32,
    column: bool,
    line: &mut Vec<RECT>,
    outline: &mut Vec<RECT>,
) {
    let put = |into: &mut Vec<RECT>, left: i32, top: i32, right: i32, bottom: i32| {
        into.push(if column {
            RECT {
                left: top,
                top: left,
                right: bottom,
                bottom: right,
            }
        } else {
            RECT {
                left,
                top,
                right,
                bottom,
            }
        });
    };
    put(line, from, across, to + 1, across + 1);
    put(outline, from, across - 1, to + 1, across);
    put(outline, from, across + 1, to + 1, across + 2);
    // A span of one pixel has both ticks in the same place.
    for x in [from, to] {
        put(line, x, across - reach, x + 1, across + reach + 1);
        put(outline, x - 1, across - reach, x, across + reach + 1);
        put(outline, x + 1, across - reach, x + 2, across + reach + 1);
    }
}

/// The same pixels as `rects` cover together, as rectangles that don't overlap: the sides of every rectangle cut the
/// plane into cells, and each row of covered cells becomes one rectangle.
fn disjoint(rects: &[RECT]) -> Vec<RECT> {
    let cuts = |of: fn(&RECT) -> [i32; 2]| {
        let mut cuts: Vec<i32> = rects.iter().flat_map(of).collect();
        cuts.sort_unstable();
        cuts.dedup();
        cuts
    };
    let columns = cuts(|r| [r.left, r.right]);
    let rows = cuts(|r| [r.top, r.bottom]);
    let mut merged = Vec::new();
    for band in rows.windows(2) {
        let &[top, bottom] = band else { continue };
        // The run of covered cells being extended, as its left and right sides.
        let mut run: Option<(i32, i32)> = None;
        for cell in columns.windows(2) {
            let &[left, right] = cell else { continue };
            let covered = rects
                .iter()
                .any(|r| r.left <= left && right <= r.right && r.top <= top && bottom <= r.bottom);
            run = match (covered, run) {
                (true, Some((start, _))) => Some((start, right)),
                (true, None) => Some((left, right)),
                (false, Some((start, end))) => {
                    merged.push(RECT {
                        left: start,
                        top,
                        right: end,
                        bottom,
                    });
                    None
                }
                (false, None) => None,
            };
        }
        if let Some((start, end)) = run {
            merged.push(RECT {
                left: start,
                top,
                right: end,
                bottom,
            });
        }
    }
    merged
}

/// A size in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Size {
    pub width: i32,
    pub height: i32,
}

/// The pill around a label whose text is `text_width` pixels wide: the padding on every side of a text as tall as its
/// size. Each part is rounded on its own, so the pill is the same height whatever the digits say.
pub fn pill_size(text_width: i32, dpi: u32) -> Size {
    Size {
        width: text_width + 2 * scaled(PAD_X, dpi),
        height: scaled(TEXT, dpi) + 2 * scaled(PAD_Y, dpi),
    }
}

/// The widths of a pinned label's border, in physical pixels. Each is scaled on its own, as every other size is, so the
/// ring is the same width on its three sides whatever the scale, and the pinned pill is `strip + ring` wider and
/// `2 * ring` taller than the live pill around the same text. Scaling the sum in one go would give `scaled(4)` and
/// `scaled(2)`, which differ by a pixel at some scales (the width at 150%, 7 against 6; the height at 125%, 2 against 3,
/// and at 150%, 4 against 3) and would make the ring uneven.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Border {
    /// The cyan strip on the label's left side.
    pub strip: i32,
    /// The ring on its other three sides.
    pub ring: i32,
}

pub fn border(dpi: u32) -> Border {
    Border {
        strip: scaled(STRIP, dpi),
        ring: scaled(RING, dpi),
    }
}

/// The pinned pill around a label whose text is `text_width` pixels wide: the live pill's size, with the border added
/// outside its padding (CSS `border-box` with the padding inside the border).
pub fn pinned_size(text_width: i32, dpi: u32) -> Size {
    let live = pill_size(text_width, dpi);
    let border = border(dpi);
    Size {
        width: live.width + border.strip + border.ring,
        height: live.height + 2 * border.ring,
    }
}

/// Where the parts of a label that has a logical part go along its text, in pixels from the text's left side.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Columns {
    /// The bar's left side.
    pub bar: i32,
    /// The logical text's left side.
    pub logical: i32,
    /// The whole text: the physical part, the bar between its two gaps, and the logical part. This is what
    /// `pill_size` takes for such a label.
    pub width: i32,
}

/// The columns of a label whose physical text is `physical` pixels wide and logical text `logical` pixels wide. Both
/// widths are whole pixels, and so is every gap and the bar, so the bar and the logical text start on whole pixels.
pub fn columns(physical: i32, logical: i32, dpi: u32) -> Columns {
    let gap = scaled(BAR_GAP, dpi);
    let bar = physical + gap;
    let after_bar = bar + scaled(BAR_WIDTH, dpi) + gap;
    Columns {
        bar,
        logical: after_bar,
        width: after_bar + logical,
    }
}

/// The bar between a label's two parts: its left side is at `left`, and the text's baseline is on row `baseline`, the
/// first row under the digits. The bar's bottom side is `BAR_DROP` below the baseline and it rises `BAR_HEIGHT` from
/// there. Whole pixels, in whatever space `left` and `baseline` are in.
pub fn bar(left: i32, baseline: i32, dpi: u32) -> RECT {
    let bottom = baseline + scaled(BAR_DROP, dpi);
    RECT {
        left,
        top: bottom - scaled(BAR_HEIGHT, dpi),
        right: left + scaled(BAR_WIDTH, dpi),
        bottom,
    }
}

/// Where the pill goes: its rectangle in the virtual screen, right and bottom one past its last pixel, whole pixels so
/// the text and the pill's sides are crisp. `None` when the measurement has no span. The label always lies inside
/// `monitor`, because the window covering it cuts off whatever sticks out.
///
/// A horizontal span's label is centered on the span, `ABOVE_GAP` above the line, or below it when the monitor has no
/// room above. A vertical span's is centered on the span, `SIDE_GAP` right of the line, or left of it when there is no
/// room. The Cross label's corner sits `CROSS_OFFSET` below and right of the crossing. The spec doesn't say what
/// happens near the monitor's limit, so this follows the other two modes: with no room on the right the label mirrors
/// to the left of the crossing, its right side `CROSS_OFFSET` from it, and with no room below it mirrors above. Where
/// it would still stick out, because the monitor is smaller than the label, or the span is short next to the monitor's
/// limit, it slides inside.
pub fn place_label(
    measurement: &Measurement,
    label: Size,
    monitor: RECT,
    dpi: u32,
) -> Option<RECT> {
    let (above, side, offset) = (
        scaled(ABOVE_GAP, dpi),
        scaled(SIDE_GAP, dpi),
        scaled(CROSS_OFFSET, dpi),
    );
    let (left, top) = match (measurement.horizontal, measurement.vertical) {
        (Some(horizontal), Some(vertical)) => {
            let (x, y) = (vertical.first.x, horizontal.first.y);
            (
                if x + offset + label.width <= monitor.right {
                    x + offset
                } else {
                    x - offset - label.width
                },
                if y + offset + label.height <= monitor.bottom {
                    y + offset
                } else {
                    y - offset - label.height
                },
            )
        }
        (Some(horizontal), None) => {
            let y = horizontal.first.y;
            let over = y - above - label.height;
            (
                centered(horizontal.first.x, horizontal.last.x, label.width),
                if over >= monitor.top {
                    over
                } else {
                    y + 1 + above
                },
            )
        }
        (None, Some(vertical)) => {
            let x = vertical.first.x;
            let right_of = x + 1 + side;
            (
                if right_of + label.width <= monitor.right {
                    right_of
                } else {
                    x - side - label.width
                },
                centered(vertical.first.y, vertical.last.y, label.height),
            )
        }
        (None, None) => return None,
    };
    let left = inside(left, label.width, monitor.left, monitor.right);
    let top = inside(top, label.height, monitor.top, monitor.bottom);
    Some(RECT {
        left,
        top,
        right: left + label.width,
        bottom: top + label.height,
    })
}

/// Where a label `length` pixels long starts to be centered on the pixels `first` to `last`, whose middle is
/// (first + last + 1) / 2. Halves round up.
fn centered(first: i32, last: i32, length: i32) -> i32 {
    (first + last + 2 - length).div_euclid(2)
}

/// `start` moved the least that puts `length` pixels from it between `low` and `high`. Should they not fit, they start
/// at `low`.
fn inside(start: i32, length: i32, low: i32, high: i32) -> i32 {
    start.min(high - length).max(low)
}

#[cfg(test)]
mod tests {
    use screengauge_core::session::Point;

    use super::*;

    fn point(x: i32, y: i32) -> Point {
        Point { x, y }
    }

    fn span(first: (i32, i32), last: (i32, i32)) -> Span {
        Span {
            first: point(first.0, first.1),
            last: point(last.0, last.1),
        }
    }

    fn measurement(horizontal: Option<Span>, vertical: Option<Span>) -> Measurement {
        Measurement {
            monitor: 0,
            horizontal,
            vertical,
            label: screengauge_core::session::Label::default(),
        }
    }

    /// A horizontal measurement: `x0` to `x1` on row `y`.
    fn horizontal(x0: i32, x1: i32, y: i32) -> Measurement {
        measurement(Some(span((x0, y), (x1, y))), None)
    }

    /// A vertical measurement: `y0` to `y1` on column `x`.
    fn vertical(y0: i32, y1: i32, x: i32) -> Measurement {
        measurement(None, Some(span((x, y0), (x, y1))))
    }

    /// The Cross measurement around (`x`, `y`).
    fn cross(x0: i32, x1: i32, y0: i32, y1: i32, x: i32, y: i32) -> Measurement {
        measurement(Some(span((x0, y), (x1, y))), Some(span((x, y0), (x, y1))))
    }

    /// A second monitor to the right of the main one, so nothing assumes the origin.
    const MONITOR: RECT = RECT {
        left: 1920,
        top: 0,
        right: 3840,
        bottom: 1080,
    };

    const PILL: Size = Size {
        width: 40,
        height: 22,
    };

    fn rect(left: i32, top: i32, right: i32, bottom: i32) -> RECT {
        RECT {
            left,
            top,
            right,
            bottom,
        }
    }

    fn area(rects: &[RECT]) -> i64 {
        rects
            .iter()
            .map(|r| i64::from(r.right - r.left) * i64::from(r.bottom - r.top))
            .sum()
    }

    fn overlap(a: &RECT, b: &RECT) -> bool {
        a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom
    }

    #[test]
    fn scaled_rounds_halves_up_at_each_scale() {
        // 100%, 125%, 150%, 175%, 200%
        assert_eq!(
            [96, 120, 144, 168, 192].map(|dpi| scaled(4, dpi)),
            [4, 5, 6, 7, 8]
        );
        assert_eq!(
            [96, 120, 168].map(|dpi| scaled(22, dpi)),
            [22, 28, 39],
            "27.5 and 38.5 round up"
        );
        assert_eq!(scaled(5, 120), 6, "6.25");
        assert_eq!(scaled(5, 168), 9, "8.75");
    }

    #[test]
    fn bounds_end_one_past_the_last_pixel() {
        let monitor = Rect {
            left: -1920,
            top: 40,
            width: 1920,
            height: 1080,
        };
        let bounds = bounds(monitor).expect("fits");
        assert_eq!(
            (bounds.left, bounds.top, bounds.right, bounds.bottom),
            (-1920, 40, 0, 1120)
        );
        let huge = Rect {
            left: i32::MAX,
            top: 0,
            width: 10,
            height: 10,
        };
        assert!(super::bounds(huge).is_err());
    }

    #[test]
    fn a_horizontal_span_is_one_pixel_thick_from_first_to_last() {
        let parts = parts(&horizontal(100, 109, 50), 96);
        assert_eq!(parts.line[0], rect(100, 50, 110, 51));
    }

    #[test]
    fn a_vertical_span_is_one_pixel_wide_from_first_to_last() {
        let parts = parts(&vertical(200, 224, 70), 96);
        assert_eq!(parts.line[0], rect(70, 200, 71, 225));
    }

    #[test]
    fn ticks_run_from_minus_k_to_plus_k_through_both_end_pixels() {
        for (dpi, k) in [(96, 4), (120, 5), (168, 7)] {
            let parts = parts(&horizontal(100, 109, 50), dpi);
            assert_eq!(
                parts.line[1..],
                [
                    rect(100, 50 - k, 101, 50 + k + 1),
                    rect(109, 50 - k, 110, 50 + k + 1)
                ],
                "at {dpi} DPI"
            );
            let parts = super::parts(&vertical(200, 224, 70), dpi);
            assert_eq!(
                parts.line[1..],
                [
                    rect(70 - k, 200, 70 + k + 1, 201),
                    rect(70 - k, 224, 70 + k + 1, 225)
                ],
                "at {dpi} DPI"
            );
        }
    }

    #[test]
    fn the_line_stays_one_pixel_thick_at_every_scale() {
        for dpi in [96, 120, 144, 168, 192] {
            let parts = parts(&horizontal(100, 109, 50), dpi);
            assert_eq!(parts.line[0].bottom - parts.line[0].top, 1, "at {dpi} DPI");
        }
    }

    #[test]
    fn the_outline_lies_beside_the_line_and_each_tick() {
        let parts = parts(&horizontal(100, 109, 50), 96);
        // Rows above and below the line, and columns beside each tick.
        let covered = |x: i32, y: i32| {
            parts
                .outline
                .iter()
                .any(|r| r.left <= x && x < r.right && r.top <= y && y < r.bottom)
        };
        assert!((100..=109).all(|x| covered(x, 49) && covered(x, 51)));
        assert!((46..=54).all(|y| covered(99, y) && covered(101, y)));
        assert!((46..=54).all(|y| covered(108, y) && covered(110, y)));
        // Two rows away, past the end of a tick, and in the middle of the line there is none. (The pixels of the line
        // itself and of the ticks are cyan, drawn over the outline's rectangles.)
        assert!(!covered(105, 48) && !covered(105, 52));
        assert!(!covered(100, 45) && !covered(100, 55));
        assert!(!covered(98, 50) && !covered(111, 50));
    }

    #[test]
    fn no_outline_rectangles_overlap() {
        let measurements = [
            horizontal(100, 109, 50),
            // One and two pixels, where the ticks' outlines meet each other.
            horizontal(100, 100, 50),
            horizontal(100, 101, 50),
            horizontal(100, 102, 50),
            vertical(200, 224, 70),
            vertical(200, 200, 70),
            cross(100, 200, 40, 90, 150, 60),
            cross(150, 150, 60, 60, 150, 60),
        ];
        for dpi in [96, 120, 168] {
            for m in &measurements {
                let outline = parts(m, dpi).outline;
                for (i, a) in outline.iter().enumerate() {
                    for b in &outline[i + 1..] {
                        assert!(
                            !overlap(a, b),
                            "{a:?} overlaps {b:?} at {dpi} DPI for {m:?}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn disjoint_keeps_exactly_the_pixels_covered() {
        let rects = [
            rect(0, 0, 10, 1),
            rect(5, 0, 6, 8),
            rect(5, 0, 6, 8),
            rect(2, 7, 9, 9),
        ];
        let merged = disjoint(&rects);
        // Counted by hand: row 0 has 10 pixels, rows 1 to 6 have the column's 1 each, rows 7 and 8 have 7 each.
        assert_eq!(area(&merged), 10 + 6 + 7 * 2);
        for (i, a) in merged.iter().enumerate() {
            for b in &merged[i + 1..] {
                assert!(!overlap(a, b));
            }
        }
    }

    #[test]
    fn a_cross_has_both_spans_and_four_ticks() {
        let parts = parts(&cross(100, 200, 40, 90, 150, 60), 96);
        assert_eq!(parts.line.len(), 6);
        assert_eq!(parts.line[0], rect(100, 60, 201, 61));
        assert_eq!(parts.line[3], rect(150, 40, 151, 91));
    }

    #[test]
    fn nothing_is_drawn_without_a_span() {
        let none = measurement(None, None);
        assert_eq!(
            parts(&none, 96),
            Parts {
                outline: Vec::new(),
                line: Vec::new()
            }
        );
        assert_eq!(place_label(&none, PILL, MONITOR, 96), None);
    }

    #[test]
    fn the_pill_is_the_text_with_padding_on_every_side() {
        assert_eq!(
            pill_size(30, 96),
            Size {
                width: 46,
                height: 22
            }
        );
        // 15 px of text and 6 px of padding above and below at 125%; 8 px of padding sideways becomes 10.
        assert_eq!(
            pill_size(40, 120),
            Size {
                width: 60,
                height: 27
            }
        );
        // 21 px of text and 9 px of padding at 175%; 14 px sideways.
        assert_eq!(
            pill_size(50, 168),
            Size {
                width: 78,
                height: 39
            }
        );
    }

    #[test]
    fn the_pinned_borders_widths_scale_on_their_own() {
        // A strip of 3 and a ring of 1, at 100%, 125%, 150% and 175%: 4.5 and 1.5 round up at 150%.
        let widths = [96, 120, 144, 168].map(|dpi| {
            let border = border(dpi);
            (border.strip, border.ring)
        });
        assert_eq!(widths, [(3, 1), (4, 1), (5, 2), (5, 2)]);
    }

    #[test]
    fn the_pinned_pill_is_the_live_pill_with_the_border_around_its_padding() {
        for dpi in [96, 120, 144, 168] {
            let live = pill_size(37, dpi);
            let pinned = pinned_size(37, dpi);
            let border = border(dpi);
            // The strip and the ring on the right add to the width, the ring above and below to the height.
            assert_eq!(
                pinned.width - live.width,
                scaled(3, dpi) + scaled(1, dpi),
                "at {dpi} DPI"
            );
            assert_eq!(
                pinned.height - live.height,
                2 * scaled(1, dpi),
                "at {dpi} DPI"
            );
            assert_eq!(pinned.width - live.width, border.strip + border.ring);
        }
        // 3 + 1 wide and 1 + 1 tall at 100%.
        assert_eq!(
            pinned_size(30, 96),
            Size {
                width: 50,
                height: 24
            }
        );
    }

    #[test]
    fn the_bars_sizes_scale_with_the_monitor() {
        // The gap of 7, the bar's width of 1 and its height of 11, at 100%, 125%, 150% and 175%. A width of 1 rounds up
        // to 2 from 150%, where it is 1.5.
        assert_eq!(
            [96, 120, 144, 168].map(|dpi| scaled(BAR_GAP, dpi)),
            [7, 9, 11, 12]
        );
        assert_eq!(
            [96, 120, 144, 168].map(|dpi| scaled(BAR_WIDTH, dpi)),
            [1, 1, 2, 2]
        );
        assert_eq!(
            [96, 120, 144, 168].map(|dpi| scaled(BAR_HEIGHT, dpi)),
            [11, 14, 17, 19]
        );
        assert_eq!(
            [96, 120, 144, 168].map(|dpi| scaled(BAR_DROP, dpi)),
            [1, 1, 2, 2]
        );
    }

    #[test]
    fn the_bar_hangs_below_the_baseline_and_rises_its_height() {
        // Baseline on row 100: the digits' last row is 99. At 100% the bar covers rows 90 to 100, one below the digits.
        assert_eq!(bar(50, 100, 96), rect(50, 90, 51, 101));
        // 125%: 14 tall, 1 below, 1 wide.
        assert_eq!(bar(50, 100, 120), rect(50, 87, 51, 101));
        // 150%: 17 tall, 2 below, 2 wide.
        assert_eq!(bar(50, 100, 144), rect(50, 85, 52, 102));
        // 175%: 19 tall, 2 below, 2 wide.
        assert_eq!(bar(50, 100, 168), rect(50, 83, 52, 102));
    }

    #[test]
    fn the_logical_part_follows_the_physical_one_after_a_gap_a_bar_and_a_gap() {
        // 30 px of physical text and 20 of logical at 100%: the bar after 7, the logical text after 7 + 1 + 7 more.
        assert_eq!(
            columns(30, 20, 96),
            Columns {
                bar: 37,
                logical: 45,
                width: 65
            }
        );
        // 150%: the gaps are 11 and the bar 2.
        assert_eq!(
            columns(30, 20, 144),
            Columns {
                bar: 41,
                logical: 54,
                width: 74
            }
        );
    }

    #[test]
    fn a_label_with_a_logical_part_is_wider_and_not_taller() {
        for dpi in [96, 120, 144, 168] {
            let physical = pill_size(30, dpi);
            let both = pill_size(columns(30, 20, dpi).width, dpi);
            let added = 2 * scaled(BAR_GAP, dpi) + scaled(BAR_WIDTH, dpi) + 20;
            assert_eq!(both.width, physical.width + added, "at {dpi} DPI");
            assert_eq!(both.height, physical.height, "at {dpi} DPI");
        }
    }

    #[test]
    fn a_horizontal_label_is_centered_above_the_line() {
        // The span's pixels 2000 to 2099 have their middle at 2050; the line is on row 500.
        let label = place_label(&horizontal(2000, 2099, 500), PILL, MONITOR, 96).expect("a span");
        assert_eq!(
            label,
            rect(2030, 469, 2070, 491),
            "centered; 9 above row 500"
        );
        // An odd width puts the half pixel after the middle.
        let odd = Size {
            width: 41,
            height: 22,
        };
        let label = place_label(&horizontal(2000, 2099, 500), odd, MONITOR, 96).expect("a span");
        assert_eq!((label.left, label.right), (2030, 2071));
    }

    #[test]
    fn a_horizontal_label_goes_below_when_there_is_no_room_above() {
        // Row 30 leaves 30 - 9 = 21 pixels above, one short of 22.
        let label = place_label(&horizontal(2000, 2099, 30), PILL, MONITOR, 96).expect("a span");
        assert_eq!(
            label,
            rect(2030, 40, 2070, 62),
            "9 below the line's bottom side"
        );
        // The top is the limit: one more row and it fits above.
        let label = place_label(&horizontal(2000, 2099, 31), PILL, MONITOR, 96).expect("a span");
        assert_eq!(label.top, 0);
    }

    #[test]
    fn a_horizontal_label_slides_inside_the_monitor() {
        // A short span at the left limit, whose centered label would stick out.
        let label = place_label(&horizontal(1920, 1925, 500), PILL, MONITOR, 96).expect("a span");
        assert_eq!((label.left, label.right), (1920, 1960));
        // And at the right limit.
        let label = place_label(&horizontal(3834, 3839, 500), PILL, MONITOR, 96).expect("a span");
        assert_eq!((label.left, label.right), (3800, 3840));
    }

    #[test]
    fn a_vertical_label_is_centered_right_of_the_line() {
        let label = place_label(&vertical(400, 499, 2500), PILL, MONITOR, 96).expect("a span");
        assert_eq!(
            label,
            rect(2511, 439, 2551, 461),
            "10 right of the line's right side"
        );
    }

    #[test]
    fn a_vertical_label_goes_left_when_there_is_no_room_on_the_right() {
        // Column 3790: the label would end at 3791 + 10 + 40 = 3841, one past the monitor.
        let label = place_label(&vertical(400, 499, 3790), PILL, MONITOR, 96).expect("a span");
        assert_eq!(
            label,
            rect(3740, 439, 3780, 461),
            "10 left of the line's left side"
        );
    }

    #[test]
    fn a_vertical_label_stays_right_when_it_just_fits() {
        // Column 3789: the label's right side lands on 3840 exactly.
        let label = place_label(&vertical(400, 499, 3789), PILL, MONITOR, 96).expect("a span");
        assert_eq!((label.left, label.right), (3800, 3840));
    }

    #[test]
    fn a_vertical_label_slides_inside_the_monitor() {
        let label = place_label(&vertical(0, 5, 2500), PILL, MONITOR, 96).expect("a span");
        assert_eq!((label.top, label.bottom), (0, 22));
        let label = place_label(&vertical(1074, 1079, 2500), PILL, MONITOR, 96).expect("a span");
        assert_eq!((label.top, label.bottom), (1058, 1080));
    }

    #[test]
    fn a_cross_label_sits_below_and_right_of_the_crossing() {
        let m = cross(2000, 2100, 400, 500, 2050, 450);
        let label = place_label(&m, PILL, MONITOR, 96).expect("spans");
        assert_eq!(label, rect(2062, 462, 2102, 484));
    }

    #[test]
    fn a_cross_label_mirrors_left_with_no_room_on_the_right() {
        let m = cross(3700, 3800, 400, 500, 3790, 450);
        let label = place_label(&m, PILL, MONITOR, 96).expect("spans");
        // 3790 + 12 + 40 is past 3840, so the right side goes 12 left of the crossing.
        assert_eq!((label.left, label.right), (3738, 3778));
        assert_eq!((label.top, label.bottom), (462, 484));
    }

    #[test]
    fn a_cross_label_mirrors_above_with_no_room_below() {
        let m = cross(2000, 2100, 1000, 1079, 2050, 1070);
        let label = place_label(&m, PILL, MONITOR, 96).expect("spans");
        assert_eq!((label.left, label.right), (2062, 2102));
        assert_eq!(
            (label.top, label.bottom),
            (1036, 1058),
            "its bottom side 12 above"
        );
    }

    #[test]
    fn a_cross_label_mirrors_both_ways_in_the_corner() {
        let m = cross(3700, 3839, 1000, 1079, 3839, 1079);
        let label = place_label(&m, PILL, MONITOR, 96).expect("spans");
        assert_eq!(label, rect(3787, 1045, 3827, 1067));
    }

    #[test]
    fn the_gaps_follow_the_scale() {
        // The 9 above a line is 11 at 125% and 16 at 175%.
        for (dpi, height, gap) in [(120, 27, 11), (168, 39, 16)] {
            let size = Size { width: 60, height };
            let label =
                place_label(&horizontal(2000, 2099, 500), size, MONITOR, dpi).expect("a span");
            assert_eq!(label.bottom, 500 - gap, "at {dpi} DPI");
            assert_eq!(label.top, label.bottom - height, "at {dpi} DPI");
        }
        // The 10 beside a line is 13 and 18, and the Cross label's 12 is 15 and 21.
        for (dpi, side, corner) in [(120, 13, 15), (168, 18, 21)] {
            let size = Size {
                width: 60,
                height: 27,
            };
            let right = place_label(&vertical(400, 499, 2500), size, MONITOR, dpi).expect("a span");
            assert_eq!(right.left, 2501 + side, "at {dpi} DPI");
            let m = cross(2000, 2100, 400, 500, 2050, 450);
            let label = place_label(&m, size, MONITOR, dpi).expect("spans");
            assert_eq!(
                (label.left, label.top),
                (2050 + corner, 450 + corner),
                "at {dpi} DPI"
            );
        }
    }
}
