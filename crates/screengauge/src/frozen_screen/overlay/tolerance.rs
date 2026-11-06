//! The tolerance indicator: "Tolerance", the tolerance as a number and a bar of ten segments on a small panel above and
//! to the left of the cursor, which follows the cursor (the prototype's variant A, WPC-279), or below and to the left of
//! it in Dimensions' horizontal mode and wherever above it would cover the live label (the owner's choices in WPC-309).
//! Where it goes needs no Windows, so [`size`], [`place_for`] and [`lit_segments`] are tested without a monitor; the
//! rest draws it onto the overlay's render target.
//!
//! The prototype's markup is `Tolerance <b>N</b> <span class="meter">...</span>` in a flex row with an 8px gap. The
//! word, the number and the bar are separate flex items, so the spaces between them collapse and the room between them
//! is the gap alone (measured in Chromium: the word is 47.54px wide with or without its trailing space, and the number
//! starts 8px after it).

use screengauge_core::session::{Measurement, Point};
use screengauge_core::settings::Tolerance;
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F;
use windows::Win32::Graphics::Direct2D::{
    D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_ROUNDED_RECT,
    D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE, ID2D1RenderTarget,
};
use windows::Win32::Graphics::DirectWrite::{DWRITE_FONT_WEIGHT, DWRITE_FONT_WEIGHT_BLACK};

use super::floating;
use super::geometry::{self, Size, scaled};
use super::{Look, Overlay, dot, px};

// Every size below is in logical pixels, which `scaled` turns into physical pixels for the monitor's scale.

/// The indicator's word. Everything ScreenGauge shows is in English (WPC-293).
const WORD: &str = "Tolerance";

/// The panel's corner radius.
const RADIUS: u32 = 9;
/// The room between the panel's border and what it holds, at the sides and above and below.
const PAD_X: u32 = 9;
const PAD_Y: u32 = 6;
/// The text's size, and the height of its line (CSS `line-height: 1`).
pub const TEXT: u32 = 11;
const LINE: u32 = 11;
/// The room between the word, the number and the bar.
const GAP: u32 = 8;

/// The number's weight. The prototype's `<b>` sits in a parent of weight 600, and CSS `bolder` from 600 is 900. Measured
/// in Chromium on Windows 11 for WPC-309: the `<b>` computes to 900, and a "0" in it is 6.594px wide at 11px, the same
/// as at 800 and 900 and unlike Bold's 6.333 at 700, so Chromium draws it in Segoe UI Black. DirectWrite also has a
/// Black face for 900 (a test checks it is a real face, not a simulated bolding). Black has no tabular figures: its "1"
/// is narrower than its "0" with or without the feature, so the number's width, and so the panel's, follows its digits
/// in the prototype too.
pub const NUMBER_WEIGHT: DWRITE_FONT_WEIGHT = DWRITE_FONT_WEIGHT_BLACK;

/// The bar: this many segments, each this wide and tall with rounded corners of this radius, this far apart. The
/// segments are centered in the text's line.
const SEGMENTS: i32 = 10;
const SEGMENT_WIDTH: u32 = 4;
const SEGMENT_HEIGHT: u32 = 10;
const SEGMENT_RADIUS: u32 = 1;
const SEGMENT_GAP: u32 = 2;
/// How much of the panel's ink shows in a segment that is not lit (CSS `opacity: .22`). A lit segment is the accent at
/// full strength.
const UNLIT_OPACITY: f32 = 0.22;

/// How far the panel sits from the cursor: to its left and above it, with nothing that moves it to the other side.
const REACH: u32 = 14;

/// How much of the notice's shadow this panel casts. The shadow's layers are fitted to a shadow box 15px tall (the
/// notice's 31px less the spread of 8 on each side); this panel is 25px tall, so its box is 9px, and a lower box blurs
/// to a fainter shadow. Fitted to CSS's true blur of that box (a gaussian of sigma 14 in the dark palette and 13 in the
/// light one, down the panel's middle), the layers want 0.67 and 0.69 of their strength, which brings the shadow's
/// error from about 0.03 (dark) and 0.02 (light) of opacity at full strength to under 0.01. Unfitted, the shadow would
/// be a third too dark.
const SHADOW_STRENGTH: f32 = 0.68;

/// The width of the bar: its segments and the gaps between them, each scaled on its own, so the bar is as wide as the
/// segments drawn. Scaling the sum in one go would give a different number at some scales (73 against 77 at 125%).
fn bar_width(dpi: u32) -> i32 {
    SEGMENTS * scaled(SEGMENT_WIDTH, dpi) + (SEGMENTS - 1) * scaled(SEGMENT_GAP, dpi)
}

/// How far the bar's top is below the top of the text's line: the line's spare room shared equally, which is half a
/// pixel at 100%, rounded up as Chromium does when it puts the box on the pixel grid.
fn bar_offset(dpi: u32) -> i32 {
    (scaled(LINE, dpi) - scaled(SEGMENT_HEIGHT, dpi) + 1) / 2
}

/// The panel around a word `word_width` pixels wide and a number `number_width` pixels wide: the border, the padding,
/// the word, a gap, the number, a gap, the bar and the padding.
pub fn size(word_width: i32, number_width: i32, dpi: u32) -> Size {
    let ring = scaled(geometry::RING, dpi);
    Size {
        width: ring
            + scaled(PAD_X, dpi)
            + word_width
            + scaled(GAP, dpi)
            + number_width
            + scaled(GAP, dpi)
            + bar_width(dpi)
            + scaled(PAD_X, dpi)
            + ring,
        height: ring + scaled(PAD_Y, dpi) + scaled(LINE, dpi) + scaled(PAD_Y, dpi) + ring,
    }
}

/// Where the panel goes above the cursor, as in the prototype: its rectangle in the virtual screen, right and bottom one
/// past its last pixel. Its right side is `REACH` left of `at` and its bottom `REACH` above it, and it never goes to the
/// other side of the cursor: where it would stick out it slides inside, `floating::MARGIN` from the monitor's sides;
/// should the monitor be smaller than the panel, it starts that far inside the monitor's left and top sides.
pub fn place(at: Point, size: Size, monitor: RECT, dpi: u32) -> RECT {
    let reach = scaled(REACH, dpi);
    floating::inside_monitor(
        at.x - reach - size.width,
        at.y - reach - size.height,
        size,
        monitor,
        dpi,
    )
}

/// Where the panel goes with `live` measured on its monitor and its label at `label`, so the panel doesn't hide the
/// number the wheel is changing (the owner's choices in WPC-309). Above the cursor, by [`place`], unless:
///
/// - Dimensions measures along the cursor's row alone with its label above the line: then the panel takes the line's
///   other side, below the cursor, wherever the cursor is along the row;
/// - the panel above would cover the label, as near a monitor's side, where the label mirrors: then it goes below.
///
/// Below, its top is `REACH` under `at` and its right side as ever. Where the monitor has no room for it there, or the
/// label is there too, it stays above.
pub fn place_for(
    at: Point,
    size: Size,
    live: Option<&Measurement>,
    label: Option<RECT>,
    monitor: RECT,
    dpi: u32,
) -> RECT {
    let above = place(at, size, monitor, dpi);
    let top = at.y + scaled(REACH, dpi);
    let below = RECT {
        top,
        bottom: top + size.height,
        ..above
    };
    let covers = |panel: &RECT| label.is_some_and(|label| overlap(panel, &label));
    let room_below = below.bottom <= monitor.bottom - scaled(floating::MARGIN, dpi);
    let along_row = live.is_some_and(|live| geometry::label_above_row(live, monitor, dpi));
    if room_below && (along_row || covers(&above)) && !covers(&below) {
        below
    } else {
        above
    }
}

/// Whether the rectangles `a` and `b`, right and bottom one past their last pixels, share a pixel.
fn overlap(a: &RECT, b: &RECT) -> bool {
    a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom
}

// The bar divides by the highest tolerance; should the calibration ever make it 0, this stops the build.
const _: () = assert!(
    Tolerance::MAX.get() > 0,
    "the highest tolerance must be above 0"
);

/// How many of the bar's segments are lit for `tolerance`: its share of the highest tolerance in tenths, rounded with
/// halves up, as the prototype's `Math.round(tol / TOL_MAX * 10)`, in whole numbers.
pub fn lit_segments(tolerance: Tolerance) -> i32 {
    let (tolerance, max) = (i32::from(tolerance.get()), i32::from(Tolerance::MAX.get()));
    (tolerance * SEGMENTS * 2 + max) / (2 * max)
}

impl Overlay {
    /// Draws the tolerance indicator for `tolerance`, for the cursor being at `at` with `live` measured there: the
    /// floating panel, "Tolerance", the number and the bar.
    pub(super) fn show_tolerance(
        &self,
        target: &ID2D1RenderTarget,
        at: Point,
        tolerance: Tolerance,
        live: Option<&Measurement>,
    ) -> windows::core::Result<()> {
        let word = self.typeset_plain(WORD, &self.word_format)?;
        let number = self.typeset(&tolerance.get().to_string(), &self.number_format)?;
        // Where the live label is, laid out as `draw` lays it out.
        let label = match live {
            Some(live) => self.label(live, Look::Live)?.map(|placed| placed.pill),
            None => None,
        };
        let panel = place_for(
            at,
            size(word.width, number.width, self.dpi()),
            live,
            label,
            self.monitor,
            self.dpi(),
        );
        self.draw_floating_panel(target, &panel, RADIUS, SHADOW_STRENGTH)?;

        let ring = scaled(geometry::RING, self.dpi());
        let window = self.in_window(&panel);
        let content_left = window.left + px(ring + scaled(PAD_X, self.dpi()));
        let content_top = window.top + px(ring + scaled(PAD_Y, self.dpi()));

        // Both texts sit in their line the way CSS does it, as the notice's text does: the line's extra room is shared
        // equally above and below. They share one baseline, on a whole pixel so the letters are crisp: the word's own
        // places it, and the number is drawn at its own distance from it, should its face have another ascent.
        let baseline = word.baseline_in(content_top, px(scaled(LINE, self.dpi())));
        let number_left = content_left + px(word.width + scaled(GAP, self.dpi()));
        let bar_left = number_left + px(number.width + scaled(GAP, self.dpi()));
        let texts = [
            word.on_baseline(content_left, baseline),
            number.on_baseline(number_left, baseline),
        ];
        // SAFETY: drawing calls on a live render target with arguments that outlive them. Out of BeginDraw and EndDraw,
        // Direct2D only fails the frame at EndDraw.
        unsafe {
            // Grayscale, not ClearType: color fringes would show on the see-through panel.
            target.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
            // The brush keeps the opacity of whatever drew with it last.
            self.surfaces.ink.SetOpacity(1.0);
            for text in &texts {
                target.DrawTextLayout(
                    dot(text.left, text.top).point,
                    &text.layout,
                    &self.surfaces.ink,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                );
            }
        }

        let bar_top = content_top + px(bar_offset(self.dpi()));
        self.draw_bar(target, bar_left, bar_top, lit_segments(tolerance));
        Ok(())
    }

    /// The bar with its top left corner at (`left`, `top`) in the window, `lit` of its segments from the left in the
    /// accent and the rest in the ink, faint. The segments are whole pixels, so their straight sides are crisp; their
    /// corners are antialiased.
    fn draw_bar(&self, target: &ID2D1RenderTarget, left: f32, top: f32, lit: i32) {
        let (width, height) = (
            scaled(SEGMENT_WIDTH, self.dpi()),
            scaled(SEGMENT_HEIGHT, self.dpi()),
        );
        let step = width + scaled(SEGMENT_GAP, self.dpi());
        let radius = px(scaled(SEGMENT_RADIUS, self.dpi()));
        // SAFETY: drawing calls on a live render target with arguments that outlive them. Out of BeginDraw and EndDraw,
        // Direct2D only fails the frame at EndDraw.
        unsafe {
            target.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
            // The brush keeps this opacity, so whatever draws with it next sets its own first.
            self.surfaces.ink.SetOpacity(UNLIT_OPACITY);
            for index in 0..SEGMENTS {
                let x = left + px(index * step);
                let segment = D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F {
                        left: x,
                        top,
                        right: x + px(width),
                        bottom: top + px(height),
                    },
                    radiusX: radius,
                    radiusY: radius,
                };
                let brush = if index < lit {
                    &self.surfaces.accent
                } else {
                    &self.surfaces.ink
                };
                target.FillRoundedRectangle(&segment, brush);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use screengauge_core::session::{Label, Measured, Span};

    fn point(x: i32, y: i32) -> Point {
        Point { x, y }
    }

    /// A second monitor to the right of the main one and below it, so nothing assumes the origin.
    const MONITOR: RECT = RECT {
        left: 1920,
        top: 100,
        right: 3840,
        bottom: 1180,
    };

    /// "Tolerance" and a number of two digits, as wide as they are at 100%.
    const PANEL: Size = Size {
        width: 156,
        height: 25,
    };

    fn rect(left: i32, top: i32, right: i32, bottom: i32) -> RECT {
        RECT {
            left,
            top,
            right,
            bottom,
        }
    }

    #[test]
    fn the_panel_is_the_border_the_padding_the_word_the_number_and_the_bar() {
        // 1 + 9 + 48 + 8 + 14 + 8 + (10 * 4 + 9 * 2) + 9 + 1 wide, and 1 + 6 + 11 + 6 + 1 tall.
        assert_eq!(
            size(48, 14, 96),
            Size {
                width: 156,
                height: 25
            }
        );
        // Each size is scaled on its own: at 125%, 1 + 11 + 48 + 10 + 14 + 10 + (10 * 5 + 9 * 3) + 11 + 1 wide, and
        // 1 + 8 + 14 + 8 + 1 tall. The bar is 77 wide, not the 73 that scaling its 58 in one go gives.
        assert_eq!(
            size(48, 14, 120),
            Size {
                width: 183,
                height: 32
            }
        );
        // At 150%: 2 + 14 + 48 + 12 + 14 + 12 + (10 * 6 + 9 * 3) + 14 + 2 and 2 + 9 + 17 + 9 + 2.
        assert_eq!(
            size(48, 14, 144),
            Size {
                width: 205,
                height: 39
            }
        );
        // At 175%: 2 + 16 + 48 + 14 + 14 + 14 + (10 * 7 + 9 * 4) + 16 + 2 and 2 + 11 + 19 + 11 + 2.
        assert_eq!(
            size(48, 14, 168),
            Size {
                width: 232,
                height: 45
            }
        );
    }

    #[test]
    fn a_wider_number_makes_a_wider_panel_by_the_same_amount() {
        assert_eq!(size(48, 14, 96).width - size(48, 7, 96).width, 7);
        assert_eq!(size(48, 14, 168).width - size(48, 7, 168).width, 7);
    }

    #[test]
    fn the_bar_is_centered_in_the_line_rounding_half_a_pixel_up() {
        // 100%: a line of 11 and a bar of 10 leave a pixel, shared as 0.5 and rounded to 1.
        assert_eq!(bar_offset(96), 1);
        // 125%: 14 and 13; 150%: 17 and 15; 175%: 19 and 18.
        assert_eq!(bar_offset(120), 1);
        assert_eq!(bar_offset(144), 1);
        assert_eq!(bar_offset(168), 1);
    }

    /// What Dimensions measures along row `y` only, from `x0` to `x1`: the horizontal mode.
    fn along_row(x0: i32, x1: i32, y: i32) -> Measurement {
        Measurement {
            monitor: 0,
            measured: Measured::Spans {
                horizontal: Some(Span {
                    first: point(x0, y),
                    last: point(x1, y),
                }),
                vertical: None,
            },
            label: Label::default(),
        }
    }

    /// What Dimensions measures along column `x` only, from `y0` to `y1`: the vertical mode.
    fn along_column(y0: i32, y1: i32, x: i32) -> Measurement {
        Measurement {
            monitor: 0,
            measured: Measured::Spans {
                horizontal: None,
                vertical: Some(Span {
                    first: point(x, y0),
                    last: point(x, y1),
                }),
            },
            label: Label::default(),
        }
    }

    /// What Dimensions measures in the Cross mode: `row`'s span and `column`'s.
    fn cross(row: Measurement, column: Measurement) -> Measurement {
        Measurement {
            measured: Measured::Spans {
                horizontal: row.horizontal(),
                vertical: column.vertical(),
            },
            ..row
        }
    }

    /// Where the live label of `live` goes, for a text 60 pixels wide.
    fn label_of(live: &Measurement, dpi: u32) -> Option<RECT> {
        geometry::place_label(live, geometry::pill_size(60, dpi), MONITOR, dpi)
    }

    #[test]
    fn where_it_would_cover_a_label_mirrored_by_the_monitors_side_the_indicator_goes_below_the_cursor()
     {
        // Near the monitor's right side the vertical mode's label goes to the left of the line, centered on the span;
        // with the cursor 40 below the span's middle, the panel above the cursor would cover it (owner's choice in
        // WPC-309: the panel moves away from the label when there is room).
        let at = point(3830, 640);
        let live = along_column(500, 700, 3830);
        let label = label_of(&live, 96);
        let label_rect = label.expect("a span");
        assert!(overlap(&place(at, PANEL, MONITOR, 96), &label_rect));
        let placed = place_for(at, PANEL, Some(&live), label, MONITOR, 96);
        assert_eq!((placed.top, placed.right), (640 + 14, 3830 - 14));
        assert!(!overlap(&placed, &label_rect));
    }

    #[test]
    fn in_the_horizontal_mode_the_indicator_stays_below_wherever_the_cursor_is_along_the_row() {
        // A long span's label sits in the span's middle, far from a cursor near one end, where the panel above would
        // cover nothing; it stays below all the same, so it doesn't jump from side to side as the cursor moves along.
        let at = point(2990, 600);
        let live = along_row(2000, 2999, 600);
        let label = label_of(&live, 96);
        assert!(!overlap(
            &place(at, PANEL, MONITOR, 96),
            &label.expect("a span")
        ));
        assert_eq!(
            place_for(at, PANEL, Some(&live), label, MONITOR, 96).top,
            600 + 14
        );
    }

    #[test]
    fn where_neither_side_is_free_of_the_label_the_indicator_stays_above_the_cursor() {
        // In the Cross mode's bottom right corner the label mirrors above and to the left of the crossing, and there is
        // no room below; 10 under the monitor's top the horizontal mode's label goes below the line, where the panel
        // below would cover it too. Both keep the prototype's place.
        let corner = cross(along_row(3700, 3839, 1170), along_column(1100, 1179, 3830));
        let top = along_row(2476, 2524, MONITOR.top + 10);
        for (at, live) in [
            (point(3830, 1170), corner),
            (point(2500, MONITOR.top + 10), top),
        ] {
            assert_eq!(
                place_for(at, PANEL, Some(&live), label_of(&live, 96), MONITOR, 96),
                place(at, PANEL, MONITOR, 96),
                "{live:?}"
            );
        }
    }

    #[test]
    fn with_the_horizontal_modes_label_above_the_line_the_indicator_goes_below_the_cursor() {
        // The label sits above the row the cursor is on, where the panel would cover it, so the panel takes the line's
        // other side: its right side 14 left of the cursor, as ever, and its top 14 below it.
        let at = point(2500, 600);
        let live = along_row(2476, 2524, 600);
        assert_eq!(
            place_for(at, PANEL, Some(&live), label_of(&live, 96), MONITOR, 96),
            rect(2500 - 14 - 156, 600 + 14, 2500 - 14, 600 + 14 + 25)
        );
        let panel = size(48, 14, 144);
        let placed = place_for(at, panel, Some(&live), label_of(&live, 144), MONITOR, 144);
        assert_eq!((placed.right, placed.top), (2500 - 21, 600 + 21), "at 150%");
    }

    #[test]
    fn with_any_other_measurement_or_none_the_indicator_stays_above_the_cursor() {
        let at = point(2500, 600);
        let above = place(at, PANEL, MONITOR, 96);
        let column = along_column(576, 624, 2500);
        let cross = cross(along_row(2476, 2524, 600), column.clone());
        for live in [None, Some(&column), Some(&cross)] {
            let label = live.and_then(|live| label_of(live, 96));
            assert_eq!(
                place_for(at, PANEL, live, label, MONITOR, 96),
                above,
                "{live:?}"
            );
        }
    }

    #[test]
    fn near_the_monitors_top_the_horizontal_label_goes_below_the_line_and_the_indicator_stays_above()
     {
        // 10 below the monitor's top there is no room above the line for the label, which goes below it; the panel
        // keeps its own place, slid inside the monitor.
        let at = point(2500, MONITOR.top + 10);
        let live = along_row(2476, 2524, MONITOR.top + 10);
        assert_eq!(
            place_for(at, PANEL, Some(&live), label_of(&live, 96), MONITOR, 96),
            place(at, PANEL, MONITOR, 96)
        );
    }

    #[test]
    fn with_no_room_below_the_cursor_the_indicator_stays_above() {
        let at = point(2500, MONITOR.bottom - 30);
        let live = along_row(2476, 2524, MONITOR.bottom - 30);
        assert_eq!(
            place_for(at, PANEL, Some(&live), label_of(&live, 96), MONITOR, 96),
            place(at, PANEL, MONITOR, 96)
        );
        // With just room for it, 14 below the cursor and 2 above the monitor's bottom, it goes below.
        let at = point(2500, MONITOR.bottom - 2 - 25 - 14);
        let live = along_row(2476, 2524, at.y);
        assert_eq!(
            place_for(at, PANEL, Some(&live), label_of(&live, 96), MONITOR, 96).top,
            at.y + 14
        );
    }

    #[test]
    fn the_indicator_is_above_and_to_the_left_of_the_cursor() {
        let at = point(2500, 600);
        // Its right side 14 left of the cursor, and its bottom side 14 above it.
        assert_eq!(
            place(at, PANEL, MONITOR, 96),
            rect(2500 - 14 - 156, 600 - 14 - 25, 2500 - 14, 600 - 14)
        );
        // The distance scales: 14 is 18 at 125% and 25 at 175%, and the panel is another size.
        let panel = size(48, 14, 120);
        assert_eq!(
            place(at, panel, MONITOR, 120),
            rect(
                2500 - 18 - panel.width,
                600 - 18 - panel.height,
                2500 - 18,
                600 - 18
            )
        );
        let panel = size(48, 14, 168);
        let placed = place(at, panel, MONITOR, 168);
        assert_eq!(
            (placed.right, placed.bottom),
            (2500 - 25, 600 - 25),
            "at 175%"
        );
    }

    #[test]
    fn the_indicator_stays_2_pixels_inside_the_monitors_left_side() {
        // The cursor so near the left side that the panel would stick out: it stops 2 from it, not 14 from the cursor.
        let near_left = place(point(MONITOR.left + 10, 600), PANEL, MONITOR, 96);
        assert_eq!(near_left.left, MONITOR.left + 2);
        assert_eq!(near_left.right, MONITOR.left + 2 + PANEL.width);
        // With the cursor 2 + 156 + 14 from the side the panel's left side is exactly 2 inside. One pixel nearer, it is
        // slid back to 2; one pixel further, it moves with the cursor.
        let fits = MONITOR.left + 2 + PANEL.width + 14;
        assert_eq!(
            place(point(fits, 600), PANEL, MONITOR, 96).left,
            MONITOR.left + 2
        );
        assert_eq!(
            place(point(fits - 1, 600), PANEL, MONITOR, 96).left,
            MONITOR.left + 2
        );
        assert_eq!(
            place(point(fits + 1, 600), PANEL, MONITOR, 96).left,
            MONITOR.left + 3
        );
    }

    #[test]
    fn the_indicator_stays_2_pixels_inside_the_monitors_top_and_never_goes_below_the_cursor() {
        // Near the top it does not move to the other side of the cursor, as the notice does: it stays above, clamped.
        let near_top = place(point(2500, MONITOR.top + 10), PANEL, MONITOR, 96);
        assert_eq!(near_top.top, MONITOR.top + 2);
        assert_eq!(near_top.bottom, MONITOR.top + 2 + PANEL.height);
        // The cursor at the monitor's very top, and past it.
        for y in [MONITOR.top, MONITOR.top - 50] {
            assert_eq!(
                place(point(2500, y), PANEL, MONITOR, 96).top,
                MONITOR.top + 2
            );
        }
    }

    #[test]
    fn the_indicator_stays_2_pixels_inside_the_monitors_right_and_bottom_sides() {
        // To the left of the cursor, it is past the right side only when the cursor is far past it.
        let past_right = place(point(MONITOR.right + 300, 600), PANEL, MONITOR, 96);
        assert_eq!(past_right.right, MONITOR.right - 2);
        // And past the bottom, which a cursor on a monitor below this one is.
        let past_bottom = place(point(2500, MONITOR.bottom + 200), PANEL, MONITOR, 96);
        assert_eq!(past_bottom.bottom, MONITOR.bottom - 2);
        // The cursor in the monitor's last pixel: the panel is above and to its left, well inside.
        let corner = place(
            point(MONITOR.right - 1, MONITOR.bottom - 1),
            PANEL,
            MONITOR,
            96,
        );
        assert_eq!(
            corner,
            rect(3839 - 14 - 156, 1179 - 14 - 25, 3839 - 14, 1179 - 14)
        );
    }

    #[test]
    fn the_indicator_stays_inside_the_monitor_at_every_scale() {
        for dpi in [96, 120, 144, 168] {
            let panel = size(48, 14, dpi);
            let margin = scaled(floating::MARGIN, dpi);
            for at in [
                point(MONITOR.left, MONITOR.top),
                point(MONITOR.right - 1, MONITOR.top),
                point(MONITOR.left, MONITOR.bottom - 1),
                point(MONITOR.right - 1, MONITOR.bottom - 1),
                point(2500, 600),
                point(MONITOR.right + 100, 600),
                point(MONITOR.left - 300, 600),
                point(2500, MONITOR.top - 50),
                point(2500, MONITOR.bottom + 200),
            ] {
                let placed = place(at, panel, MONITOR, dpi);
                assert!(
                    placed.left >= MONITOR.left + margin
                        && placed.top >= MONITOR.top + margin
                        && placed.right <= MONITOR.right - margin
                        && placed.bottom <= MONITOR.bottom - margin,
                    "{placed:?} for {at:?} at {dpi} DPI"
                );
                assert_eq!(placed.right - placed.left, panel.width);
                assert_eq!(placed.bottom - placed.top, panel.height);
            }
        }
    }

    #[test]
    fn a_monitor_smaller_than_the_panel_gets_the_panel_at_its_top_left() {
        // 100 by 20 pixels: no room for a panel of 156 by 25. This must not panic, and the panel starts inside the
        // monitor's corner rather than past it.
        let tiny = rect(100, 100, 200, 120);
        let placed = place(point(150, 110), PANEL, tiny, 96);
        assert_eq!((placed.left, placed.top), (tiny.left + 2, tiny.top + 2));
    }

    fn tolerance(value: u8) -> Tolerance {
        Tolerance::new(value).expect("within the limits")
    }

    #[test]
    fn the_bar_lights_the_tolerances_share_of_the_highest_in_tenths() {
        // For the highest tolerance, 100 (WPC-332): a segment for each 10, from 5 up.
        for (value, lit) in [
            (0, 0),
            (4, 0),
            (5, 1),
            (6, 1),
            (15, 2),
            (50, 5),
            (94, 9),
            (95, 10),
            (100, 10),
        ] {
            assert_eq!(lit_segments(tolerance(value)), lit, "tolerance {value}");
        }
    }

    #[test]
    fn the_bar_follows_javascripts_rounding_with_halves_up_for_every_tolerance() {
        // `Math.round` sends halves up, as `f64::round` does for these values, none of them below zero.
        let max = Tolerance::MAX.get();
        for value in Tolerance::MIN.get()..=max {
            let by_float = (f64::from(value) / f64::from(max) * 10.0).round();
            assert_eq!(
                f64::from(lit_segments(tolerance(value))),
                by_float,
                "tolerance {value}"
            );
        }
    }

    #[test]
    fn the_highest_tolerance_the_settings_allow_lights_the_whole_bar() {
        assert_eq!(lit_segments(Tolerance::MAX), SEGMENTS);
        assert_eq!(lit_segments(Tolerance::MIN), 0);
    }
}
