//! Render tests of the pinned labels, of measurements drawn over each other, and of the notice, on a WIC bitmap like
//! the ones in `overlay`'s own tests.

use screengauge_core::session::{Message, Notice, Point, Sign};

use super::notice;
use super::tests::{
    BACKGROUND, CYAN, Canvas, MONITOR, Picture, SCALES, cross, expected, horizontal, near, point,
    text_width, vertical, with_logical,
};
use super::*;

const MODES: [Mode; 2] = [Mode::Dark, Mode::Light];

/// One palette's colors as the prototype states them (`--panel`, `--panel-ink` and so on), written out here a second time
/// so that a slip in the palette is caught.
struct Tokens {
    panel: ([u8; 3], f32),
    ink: [u8; 3],
    border: ([u8; 3], f32),
    accent: [u8; 3],
    warn: [u8; 3],
    /// The 1px highlight inside the notice's top border.
    highlight: ([u8; 3], f32),
}

fn tokens(mode: Mode) -> Tokens {
    match mode {
        Mode::Dark => Tokens {
            panel: ([26, 27, 31], 0.94),
            ink: [0xe9, 0xeb, 0xef],
            border: ([255, 255, 255], 0.1),
            accent: [0x28, 0xd6, 0xdf],
            warn: [0xf3, 0xb9, 0x4c],
            highlight: ([255, 255, 255], 0.08),
        },
        Mode::Light => Tokens {
            panel: ([250, 249, 247], 0.96),
            ink: [0x1d, 0x1c, 0x1a],
            border: ([70, 60, 45], 0.16),
            accent: [0x00, 0x7c, 0xbb],
            warn: [0xb8, 0x60, 0x00],
            highlight: ([255, 255, 255], 1.0),
        },
    }
}

/// `color` at `alpha` over the opaque `under`.
fn over(color: [u8; 3], alpha: f32, under: [u8; 3]) -> [u8; 3] {
    std::array::from_fn(|i| {
        (f32::from(color[i]) * alpha + f32::from(under[i]) * (1.0 - alpha)).round() as u8
    })
}

impl Tokens {
    /// What the panel looks like over the test's background.
    fn panel_over(&self, under: [u8; 3]) -> [u8; 3] {
        over(self.panel.0, self.panel.1, under)
    }

    /// The panel over the background, and the ring over that: the ring's pixels.
    fn ring_over_panel(&self) -> [u8; 3] {
        over(self.border.0, self.border.1, self.panel_over(BACKGROUND))
    }
}

/// A pixel by its place in the virtual screen.
fn at(picture: &Picture, x: i32, y: i32) -> [u8; 3] {
    picture.at((x - MONITOR.left) as usize, (y - MONITOR.top) as usize)
}

/// The pixels of `area` as (x, y) in the virtual screen.
fn pixels(area: RECT) -> impl Iterator<Item = (i32, i32)> {
    (area.top..area.bottom).flat_map(move |y| (area.left..area.right).map(move |x| (x, y)))
}

fn rect(left: i32, top: i32, right: i32, bottom: i32) -> RECT {
    RECT {
        left,
        top,
        right,
        bottom,
    }
}

fn inside(pill: &RECT, x: i32, y: i32) -> bool {
    pill.left <= x && x < pill.right && pill.top <= y && y < pill.bottom
}

/// The measurements drawn as `draw` draws them on a window of `mode` and `dpi`, and the labels they got, pinned first
/// and the live one last.
fn render_scene(
    canvas: &Canvas,
    mode: Mode,
    dpi: u32,
    pinned: &[&Measurement],
    live: Option<&Measurement>,
) -> (Picture, Vec<PlacedLabel>) {
    let mut labels = Vec::new();
    let picture = canvas.render_in(mode, MONITOR, dpi, |overlay, target| {
        overlay
            .draw(target, pinned, live)
            .expect("the measurements");
        let looks = pinned
            .iter()
            .map(|&m| (m, Look::Pinned))
            .chain(live.map(|m| (m, Look::Live)));
        for (m, look) in looks {
            labels.extend(overlay.label(m, look).expect("a label"));
        }
    });
    (picture, labels)
}

/// The brightest green in `area` for a palette whose ink is lighter than its panel, the darkest for one whose ink is
/// darker: the pixel of a glyph that is covered all through.
fn strongest_green(picture: &Picture, area: RECT, mode: Mode) -> u8 {
    let greens = pixels(area).map(|(x, y)| at(picture, x, y)[1]);
    match mode {
        Mode::Dark => greens.max(),
        Mode::Light => greens.min(),
    }
    .expect("an area with pixels")
}

#[test]
fn a_pinned_label_is_the_panel_in_a_ring_with_a_cyan_strip_on_its_left() {
    let canvas = Canvas::new(480, 320);
    for mode in MODES {
        let t = tokens(mode);
        let panel = t.panel_over(BACKGROUND);
        let ring_color = t.ring_over_panel();
        for dpi in SCALES {
            let m = horizontal(-400, -301, 350, "240");
            let (picture, labels) = render_scene(&canvas, mode, dpi, &[&m], None);
            let pill = labels[0].pill;
            // The prototype's 3px strip and 1px ring, scaled: not read back from the geometry that is being tested.
            let (strip, ring) = (scaled(3, dpi), scaled(1, dpi));
            let middle = (pill.top + pill.bottom) / 2;
            let across = (pill.left + pill.right) / 2;
            let context = format!("{mode:?} at {dpi} DPI");

            // The strip: opaque cyan, as wide as scaled, over the left side's ring.
            for x in pill.left + 1..pill.left + strip {
                assert_eq!(at(&picture, x, middle), CYAN, "strip column {x}, {context}");
            }
            assert!(
                near(at(&picture, pill.left + strip, middle), panel, 1),
                "the column after the strip is the panel, {context}: {:?}",
                at(&picture, pill.left + strip, middle)
            );

            // The panel, as it composites over the photo, with no shadow showing through it.
            for (x, y) in [
                (pill.left + strip + 2, middle),
                (pill.right - ring - 2, middle),
                (across, pill.top + ring),
                (across, pill.bottom - ring - 1),
            ] {
                assert!(
                    near(at(&picture, x, y), panel, 1),
                    "({x}, {y}) is {:?}, the panel is {panel:?}, {context}",
                    at(&picture, x, y)
                );
            }

            // The ring: the border's color over the panel, as thick as scaled, on the top, bottom and right sides.
            for k in 0..ring {
                for (x, y, side) in [
                    (across, pill.top + k, "top"),
                    (across, pill.bottom - 1 - k, "bottom"),
                    (pill.right - 1 - k, middle, "right"),
                ] {
                    assert!(
                        near(at(&picture, x, y), ring_color, 2),
                        "the {side} ring at ({x}, {y}) is {:?}, wanted {ring_color:?}, {context}",
                        at(&picture, x, y)
                    );
                }
            }
        }
    }
}

#[test]
fn a_pinned_pill_is_the_live_pill_with_its_border_around_it_and_its_text_inside_the_border() {
    let canvas = Canvas::new(480, 320);
    for dpi in SCALES {
        let m = with_logical(horizontal(-400, -301, 350, "240"), "137.1");
        let scale = Scale::from_dpi(dpi).expect("a scale");
        let overlay = Overlay::new(&canvas.target, &canvas.fonts, MONITOR, scale, Mode::Dark)
            .expect("the overlay");
        let live = overlay.label(&m, Look::Live).expect("label").expect("pill");
        let pinned = overlay
            .label(&m, Look::Pinned)
            .expect("label")
            .expect("pill");
        let border = geometry::border(dpi);
        let wide = |l: &PlacedLabel| l.pill.right - l.pill.left;
        let tall = |l: &PlacedLabel| l.pill.bottom - l.pill.top;
        // 3 and 1 on the sides, 1 and 1 above and below, each scaled on its own.
        assert_eq!(
            wide(&pinned) - wide(&live),
            scaled(3, dpi) + scaled(1, dpi),
            "at {dpi} DPI"
        );
        assert_eq!(
            tall(&pinned) - tall(&live),
            2 * scaled(1, dpi),
            "at {dpi} DPI"
        );
        assert_eq!(border.strip + border.ring, scaled(3, dpi) + scaled(1, dpi));
        // The text starts a strip further in and a ring further down, relative to the pill's own corner. Both of a
        // label's texts and the bar move together.
        let from_corner = |l: &PlacedLabel| {
            (
                l.physical.left - px(l.pill.left),
                l.physical.top - px(l.pill.top),
            )
        };
        let (live_in, pinned_in) = (from_corner(&live), from_corner(&pinned));
        assert_eq!(pinned_in.0 - live_in.0, px(border.strip), "at {dpi} DPI");
        assert_eq!(pinned_in.1 - live_in.1, px(border.ring), "at {dpi} DPI");
        let live_logical = live.logical.expect("a logical part");
        let pinned_logical = pinned.logical.expect("a logical part");
        assert_eq!(
            (pinned_logical.bar.left - px(pinned.pill.left))
                - (live_logical.bar.left - px(live.pill.left)),
            px(border.strip),
            "the bar at {dpi} DPI"
        );
        assert_eq!(
            (pinned_logical.text.left - px(pinned.pill.left))
                - (live_logical.text.left - px(live.pill.left)),
            px(border.strip),
            "the logical text at {dpi} DPI"
        );
    }
}

#[test]
fn a_pinned_labels_text_is_in_the_panels_ink_with_the_bar_and_the_logical_part_fainter() {
    let canvas = Canvas::new(480, 320);
    for mode in MODES {
        let t = tokens(mode);
        let panel = t.panel_over(BACKGROUND);
        for dpi in SCALES {
            let m = with_logical(horizontal(-400, -301, 350, "240"), "137.1");
            let (picture, labels) = render_scene(&canvas, mode, dpi, &[&m], None);
            let label = &labels[0];
            let border = geometry::border(dpi);
            let context = format!("{mode:?} at {dpi} DPI");
            let logical = label.logical.as_ref().expect("a logical part");
            let window_to_screen =
                |x: f32, y: f32| (whole(x) + MONITOR.left, whole(y) + MONITOR.top);

            // The bar: the ink at 40% over the panel, on every pixel of it.
            let bar_color = over(t.ink, 0.4, panel);
            let (left, top) = window_to_screen(logical.bar.left, logical.bar.top);
            let (right, bottom) = window_to_screen(logical.bar.right, logical.bar.bottom);
            for (x, y) in pixels(rect(left, top, right, bottom)) {
                assert!(
                    near(at(&picture, x, y), bar_color, 3),
                    "bar pixel ({x}, {y}) is {:?}, wanted {bar_color:?}, {context}",
                    at(&picture, x, y)
                );
            }

            // The physical text: some pixel in full ink. The logical text: the same at 62%.
            let rows = |from_left: i32, to_right: i32| {
                rect(
                    from_left,
                    label.pill.top + border.ring,
                    to_right,
                    label.pill.bottom - border.ring,
                )
            };
            let (physical_left, _) = window_to_screen(label.physical.left, 0.0);
            let (bar_left, _) = window_to_screen(logical.bar.left, 0.0);
            let physical = strongest_green(&picture, rows(physical_left, bar_left), mode);
            assert!(
                physical.abs_diff(t.ink[1]) <= 3,
                "the physical text's strongest green is {physical}, the ink's is {}, {context}",
                t.ink[1]
            );
            let (logical_left, _) = window_to_screen(logical.text.left, 0.0);
            let faint = over(t.ink, 0.62, panel)[1];
            let logical_green = strongest_green(
                &picture,
                rows(
                    logical_left,
                    label.pill.right - border.ring - scaled(8, dpi),
                ),
                mode,
            );
            assert!(
                logical_green.abs_diff(faint) <= 3,
                "the logical text's strongest green is {logical_green}, 62% of the ink is {faint}, {context}"
            );
        }
    }
}

#[test]
fn the_live_pill_and_its_line_look_the_same_in_both_modes() {
    let canvas = Canvas::new(480, 320);
    for dpi in SCALES {
        let live = with_logical(horizontal(-400, -301, 350, "240"), "137.1");
        let (dark, _) = render_scene(&canvas, Mode::Dark, dpi, &[], Some(&live));
        let (light, _) = render_scene(&canvas, Mode::Light, dpi, &[], Some(&live));
        assert!(dark.pixels == light.pixels, "at {dpi} DPI");
        let cross = cross((-400, -301), (230, 480), -350, 360, "240 \u{d7} 180");
        let (dark, _) = render_scene(&canvas, Mode::Dark, dpi, &[], Some(&cross));
        let (light, _) = render_scene(&canvas, Mode::Light, dpi, &[], Some(&cross));
        assert!(dark.pixels == light.pixels, "the cross at {dpi} DPI");
    }
}

#[test]
fn each_label_has_full_ink_whichever_look_the_one_before_it_had() {
    // The brushes keep the opacity of the last thing drawn with them, and the overlay is used for every frame. A pinned
    // label with a logical part and then a live one leave both brushes at the logical text's opacity.
    let canvas = Canvas::new(480, 320);
    for mode in MODES {
        let t = tokens(mode);
        for dpi in [96, 168] {
            let pinned = with_logical(horizontal(-400, -301, 350, "24"), "13.7");
            let live = with_logical(horizontal(-400, -301, 250, "24"), "13.7");
            let later_pinned = horizontal(-400, -301, 400, "24");
            let later_live = horizontal(-400, -301, 300, "24");
            let mut labels = Vec::new();
            let picture = canvas.render_in(mode, MONITOR, dpi, |overlay, target| {
                overlay
                    .draw(target, &[&pinned], Some(&live))
                    .expect("the first frame");
                overlay
                    .draw(target, &[&later_pinned], Some(&later_live))
                    .expect("the second frame");
                for (m, look) in [(&later_pinned, Look::Pinned), (&later_live, Look::Live)] {
                    labels.extend(overlay.label(m, look).expect("a label"));
                }
            });
            let border = geometry::border(dpi);
            for label in &labels {
                let area = rect(
                    label.pill.left + border.strip + 4,
                    label.pill.top,
                    label.pill.right - 4,
                    label.pill.bottom,
                );
                let strongest = pixels(area)
                    .map(|(x, y)| at(&picture, x, y)[1])
                    .reduce(|a, b| match (label.look, mode) {
                        (Look::Live, _) | (_, Mode::Light) => a.min(b),
                        (Look::Pinned, Mode::Dark) => a.max(b),
                    })
                    .expect("pixels");
                let ink = match label.look {
                    Look::Live => 0x16,
                    Look::Pinned => t.ink[1],
                };
                assert!(
                    strongest.abs_diff(ink) <= 8,
                    "{:?} label, {mode:?} at {dpi} DPI: strongest green {strongest}, ink {ink}",
                    label.look
                );
            }
        }
    }
}

/// The scene of two measurements over each other: a pinned horizontal one along row 392, and the live vertical one
/// along column -300, which crosses it, with its label to the right of its line, where the pinned line passes just
/// below the label, in its shadow.
fn overlapping() -> (Measurement, Measurement) {
    (
        horizontal(-320, -200, 392, "120"),
        with_logical(vertical(330, 420, -300, "90"), "90.0"),
    )
}

#[test]
fn nothing_darkens_a_cyan_pixel_of_another_measurement_outside_the_pills() {
    let canvas = Canvas::new(480, 320);
    let (pinned, live) = overlapping();
    for mode in MODES {
        for dpi in SCALES {
            let k = scaled(4, dpi);
            let (picture, labels) = render_scene(&canvas, mode, dpi, &[&pinned], Some(&live));
            let pills: Vec<RECT> = labels.iter().map(|l| l.pill).collect();
            let mut under_a_shadow = 0;
            for (x, y) in pixels(rect(
                MONITOR.left,
                MONITOR.top,
                MONITOR.left + 480,
                MONITOR.top + 320,
            )) {
                let cyan = [&pinned, &live]
                    .iter()
                    .any(|m| expected(m, k, x, y) == CYAN);
                if !cyan || pills.iter().any(|p| inside(p, x, y)) {
                    continue;
                }
                assert_eq!(
                    at(&picture, x, y),
                    CYAN,
                    "pixel ({x}, {y}), {mode:?} at {dpi} DPI"
                );
                // Where a shadow falls: 2 for its drop and 6 for its blur around a pill.
                let reach = scaled(8, dpi);
                let grown = |p: &RECT| {
                    rect(
                        p.left - reach,
                        p.top - reach,
                        p.right + reach,
                        p.bottom + reach,
                    )
                };
                if pills.iter().any(|p| inside(&grown(p), x, y)) {
                    under_a_shadow += 1;
                }
            }
            // The scene is made for the test to mean something: cyan pixels do lie where a shadow falls.
            assert!(
                under_a_shadow > 10,
                "{under_a_shadow} cyan pixels in a shadow, {mode:?} at {dpi} DPI"
            );
        }
    }
}

#[test]
fn a_label_never_darkens_the_other_measurements_ticks_when_each_is_the_one_drawn_last() {
    // The same two measurements with their roles swapped, so the live one's shadow and outline are over the pinned
    // one's pixels and the other way round.
    let canvas = Canvas::new(480, 320);
    let (a, b) = overlapping();
    let (pinned, live) = (b, a);
    for dpi in SCALES {
        let k = scaled(4, dpi);
        let (picture, labels) = render_scene(&canvas, Mode::Dark, dpi, &[&pinned], Some(&live));
        let pills: Vec<RECT> = labels.iter().map(|l| l.pill).collect();
        for (x, y) in pixels(rect(
            MONITOR.left,
            MONITOR.top,
            MONITOR.left + 480,
            MONITOR.top + 320,
        )) {
            let cyan = [&pinned, &live]
                .iter()
                .any(|m| expected(m, k, x, y) == CYAN);
            if cyan && !pills.iter().any(|p| inside(p, x, y)) {
                assert_eq!(at(&picture, x, y), CYAN, "pixel ({x}, {y}) at {dpi} DPI");
            }
        }
    }
}

#[test]
fn the_live_pill_covers_a_pinned_one_at_the_same_place() {
    let canvas = Canvas::new(480, 320);
    for mode in MODES {
        let t = tokens(mode);
        for dpi in SCALES {
            let m = with_logical(horizontal(-400, -301, 350, "240"), "137.1");
            // The live label alone, and the same with a pinned label of the same measurement under it.
            let (alone, labels) = render_scene(&canvas, mode, dpi, &[], Some(&m));
            let live = labels[0].pill;
            let (both, labels) = render_scene(&canvas, mode, dpi, &[&m], Some(&m));
            let (pinned, over_it) = (labels[0].pill, labels[1].pill);
            assert_eq!(
                over_it, live,
                "the live pill's place doesn't depend on the other"
            );
            // The pinned pill is the larger, and sticks out of the live one on every side.
            assert!(
                pinned.left < live.left && pinned.right > live.right && pinned.top < live.top,
                "{pinned:?} against {live:?}, {mode:?} at {dpi} DPI"
            );
            // Away from the corners, where the live pill's antialiasing lets what is under it show, the pictures are
            // the same: nothing of the pinned pill shows through the live one, not its panel nor its text.
            let corner = scaled(geometry::RADIUS, dpi);
            let middle = rect(
                live.left + corner,
                live.top,
                live.right - corner,
                live.bottom,
            );
            for (x, y) in pixels(middle) {
                assert_eq!(
                    at(&both, x, y),
                    at(&alone, x, y),
                    "({x}, {y}), {mode:?} at {dpi} DPI"
                );
            }
            // And the pinned pill is there around it: its ring on the right is the ring's color.
            let y = (pinned.top + pinned.bottom) / 2;
            assert!(
                near(at(&both, pinned.right - 1, y), t.ring_over_panel(), 2),
                "{:?}, {mode:?} at {dpi} DPI",
                at(&both, pinned.right - 1, y)
            );
        }
    }
}

#[test]
fn a_later_pinned_label_is_over_an_earlier_one() {
    // Two labels at the same place, with different text: the second is over the first, and the first shows through its
    // see-through panel only by what the panel lets pass.
    let canvas = Canvas::new(480, 320);
    for dpi in [96, 168] {
        let first = horizontal(-400, -301, 350, "888");
        let second = horizontal(-400, -301, 350, "100");
        let (both, labels) = render_scene(&canvas, Mode::Dark, dpi, &[&first, &second], None);
        let (second_alone, _) = render_scene(&canvas, Mode::Dark, dpi, &[&second], None);
        let pill = labels[1].pill;
        // The same pill: the width of the text is the same for any three digits.
        assert_eq!(labels[0].pill, pill, "at {dpi} DPI");
        // Over the middle of the pill, the second label's pixels are those of its own, but for the 6% the first one's
        // panel and text let through under it: no pixel is further from the second's alone than that is.
        let corner = scaled(geometry::RADIUS, dpi);
        let middle = rect(
            pill.left + corner,
            pill.top,
            pill.right - corner,
            pill.bottom,
        );
        let mut most = 0;
        for (x, y) in pixels(middle) {
            let (a, b) = (at(&both, x, y), at(&second_alone, x, y));
            most = most.max(
                a.iter()
                    .zip(b)
                    .map(|(&a, b)| a.abs_diff(b))
                    .max()
                    .unwrap_or(0),
            );
        }
        assert!(most > 0, "the first label is under the second at {dpi} DPI");
        assert!(most <= 24, "{most} levels of difference at {dpi} DPI");
    }
}

/// The notice at `at` saying `text` after `sign`, drawn in `mode`, and where its panel is.
fn render_notice(
    canvas: &Canvas,
    mode: Mode,
    dpi: u32,
    at: Point,
    text: &str,
    sign: Sign,
) -> (Picture, RECT, i32) {
    let mut found = None;
    let picture = canvas.render_in(mode, MONITOR, dpi, |overlay, target| {
        overlay
            .show_notice(target, at, text, sign)
            .expect("the notice");
        let width = text_width(overlay, text, &overlay.format);
        let panel = notice::place(
            at,
            notice::size(width, dpi),
            geometry::bounds(MONITOR).expect("the monitor"),
            dpi,
        );
        found = Some((panel, width));
    });
    let (panel, width) = found.expect("the notice was drawn");
    (picture, panel, width)
}

#[test]
fn a_notice_is_the_panel_in_a_ring_with_a_highlight_inside_its_top() {
    let canvas = Canvas::new(480, 320);
    for mode in MODES {
        let t = tokens(mode);
        let panel_color = t.panel_over(BACKGROUND);
        let ring_color = t.ring_over_panel();
        let highlight = over(t.highlight.0, t.highlight.1, panel_color);
        for dpi in SCALES {
            let (picture, panel, _) =
                render_notice(&canvas, mode, dpi, point(-300, 400), "Copied", Sign::Ok);
            let context = format!("{mode:?} at {dpi} DPI");
            let ring = scaled(geometry::RING, dpi);
            let (middle, across) = (
                (panel.top + panel.bottom) / 2,
                (panel.left + panel.right) / 2,
            );
            let check = |x: i32, y: i32, wanted: [u8; 3], what: &str| {
                assert!(
                    near(at(&picture, x, y), wanted, 2),
                    "{what} at ({x}, {y}) is {:?}, wanted {wanted:?}, {context}",
                    at(&picture, x, y)
                );
            };
            for k in 0..ring {
                check(across, panel.top + k, ring_color, "the top ring");
                check(across, panel.bottom - 1 - k, ring_color, "the bottom ring");
                check(panel.left + k, middle, ring_color, "the left ring");
                check(panel.right - 1 - k, middle, ring_color, "the right ring");
                // The highlight is as thick as a pixel scaled, along the inside of the top ring; then the panel.
                check(
                    across,
                    panel.top + ring + k.min(scaled(1, dpi) - 1),
                    highlight,
                    "the highlight",
                );
            }
            check(
                across,
                panel.top + ring + scaled(1, dpi),
                panel_color,
                "below the highlight",
            );
            // Right of the text, in the padding: the panel with no shadow showing through it.
            check(panel.right - ring - 3, middle, panel_color, "the panel");
            // Outside the ring on every side there is the photo, a little darkened by the shadow below, and the ring is
            // nowhere else: one pixel beyond it is not its color.
            for (x, y) in [
                (across, panel.top - 1),
                (across, panel.bottom),
                (panel.left - 1, middle),
                (panel.right, middle),
            ] {
                assert!(
                    !near(at(&picture, x, y), ring_color, 6),
                    "({x}, {y}) is outside the notice but is {:?}, {context}",
                    at(&picture, x, y)
                );
            }
        }
    }
}

#[test]
fn a_notice_is_drawn_where_the_rule_puts_it() {
    // Worked out by hand at 100%: 14 to the right of the cursor and its bottom 14 above it, to the left when it would end
    // within 4 of the monitor's right side, and 18 below it when it would start within 4 of the monitor's top.
    let canvas = Canvas::new(480, 320);
    let t = tokens(Mode::Dark);
    let ring = t.ring_over_panel();
    let monitor = geometry::bounds(MONITOR).expect("the monitor");
    for (cursor, flips, below, name) in [
        (point(-300, 400), false, false, "above and to the right"),
        (point(-2, 400), true, false, "flipped to the left"),
        (point(-300, 205), false, true, "below"),
        (point(-2, 205), true, true, "left and below"),
    ] {
        let (picture, panel, width) =
            render_notice(&canvas, Mode::Dark, 96, cursor, "Copied", Sign::Ok);
        // The panel of "Copied" at 100% is 42 plus the text's width, and 31 tall.
        assert_eq!(panel.right - panel.left, 42 + width, "{name}");
        assert_eq!(panel.bottom - panel.top, 31, "{name}");
        let left = if flips {
            cursor.x - 14 - (42 + width)
        } else {
            cursor.x + 14
        };
        let top = if below {
            cursor.y + 18
        } else {
            cursor.y - 14 - 31
        };
        assert_eq!((panel.left, panel.top), (left, top), "{name}");
        assert!(
            panel.right <= monitor.right - 2 && panel.top >= monitor.top + 2,
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
                near(at(&picture, x, y), ring, 2),
                "{name}: ({x}, {y}) is {:?}",
                at(&picture, x, y)
            );
        }
        for (x, y) in [
            (across, panel.top - 1),
            (across, panel.bottom),
            (panel.left - 1, middle),
            (panel.right, middle),
        ] {
            assert!(
                !near(at(&picture, x, y), ring, 6),
                "{name}: ({x}, {y}) is {:?}",
                at(&picture, x, y)
            );
        }
    }
}

#[test]
fn the_public_notice_is_the_one_that_says_copied_with_a_check() {
    let canvas = Canvas::new(480, 320);
    for mode in MODES {
        let notice = Notice {
            message: Message::Copied,
            at: point(-300, 400),
            monitor: 0,
        };
        let drawn = canvas.render_in(mode, MONITOR, 120, |overlay, target| {
            overlay.draw_notice(target, &notice).expect("the notice");
        });
        let (by_hand, ..) = render_notice(&canvas, mode, 120, notice.at, "Copied", Sign::Ok);
        assert!(drawn.pixels == by_hand.pixels, "{mode:?}");
    }
}

#[test]
fn a_notices_sign_is_in_the_accent_for_ok_and_the_warning_color_for_an_alert() {
    let canvas = Canvas::new(480, 320);
    for mode in MODES {
        let t = tokens(mode);
        for dpi in SCALES {
            for (sign, color, other, name, least) in [
                (Sign::Ok, t.accent, t.warn, "ok", 5),
                (Sign::Alert, t.warn, t.accent, "alert", 14),
            ] {
                let (picture, panel, _) =
                    render_notice(&canvas, mode, dpi, point(-300, 400), "Copied", sign);
                let ring = scaled(geometry::RING, dpi);
                let left = panel.left + ring + scaled(9, dpi);
                let top = panel.top + ring + scaled(7, dpi);
                let side = scaled(13, dpi);
                let sign_box = rect(left, top, left + side, top + side);
                // A pixel is the sign's where it is within what 85% of the color over the panel leaves.
                let count = |wanted: [u8; 3]| {
                    pixels(sign_box)
                        .filter(|&(x, y)| near(at(&picture, x, y), wanted, 40))
                        .count()
                };
                let context = format!("{name} sign, {mode:?} at {dpi} DPI");
                assert!(
                    count(color) >= least,
                    "{} pixels in its color, {context}",
                    count(color)
                );
                assert_eq!(
                    count(other),
                    0,
                    "no pixel in the other sign's color, {context}"
                );
                // The strokes stay in their box: no pixel of the sign's color beside it, in the padding.
                let beside = rect(left - scaled(9, dpi) + ring, top, left, top + side);
                assert_eq!(
                    pixels(beside)
                        .filter(|&(x, y)| near(at(&picture, x, y), color, 40))
                        .count(),
                    0,
                    "{context}"
                );
            }
        }
    }
}

#[test]
fn a_notices_text_is_in_the_panels_ink_inside_its_line() {
    let canvas = Canvas::new(480, 320);
    for mode in MODES {
        let t = tokens(mode);
        let panel_color = t.panel_over(BACKGROUND);
        for dpi in SCALES {
            let (picture, panel, width) =
                render_notice(&canvas, mode, dpi, point(-300, 400), "Copied", Sign::Ok);
            let ring = scaled(geometry::RING, dpi);
            let text_left = panel.left + ring + scaled(9, dpi) + scaled(13, dpi) + scaled(7, dpi);
            let line_top = panel.top + ring + scaled(7, dpi);
            let line = rect(
                text_left,
                line_top,
                text_left + width,
                line_top + scaled(15, dpi),
            );
            let context = format!("{mode:?} at {dpi} DPI");
            // Some pixel of the text is in full ink.
            let strongest = strongest_green(&picture, line, mode);
            assert!(
                strongest.abs_diff(t.ink[1]) <= 3,
                "strongest green {strongest}, the ink's {}, {context}",
                t.ink[1]
            );
            // Every pixel that is clearly ink, in the text's columns of the panel's content rows and the padding above and
            // below them, is inside its line, but for the tail of the "p", which may hang a pixel or two out of it: CSS
            // lets a glyph go beyond its line box too, and the padding is there to take it.
            let line_and_tails = rect(
                line.left,
                line.top,
                line.right,
                line.bottom + scaled(2, dpi),
            );
            let tall = rect(
                text_left - 1,
                panel.top + ring,
                text_left + width + 1,
                panel.bottom - ring,
            );
            let inked: Vec<(i32, i32)> = pixels(tall)
                .filter(|&(x, y)| at(&picture, x, y)[1].abs_diff(panel_color[1]) > 60)
                .collect();
            assert!(!inked.is_empty(), "{context}");
            for &(x, y) in &inked {
                assert!(
                    inside(&line_and_tails, x, y),
                    "ink at ({x}, {y}) is outside the line {line:?}, {context}"
                );
            }
            // The text isn't pushed to one end of its line: the capitals and tall letters start within a few pixels of
            // its top.
            let first = inked.iter().map(|&(_, y)| y).min().expect("ink");
            let above = first - line.top;
            assert!(
                (0..=scaled(4, dpi)).contains(&above),
                "{above} rows above the ink, {context}"
            );
        }
    }
}

#[test]
fn a_notices_shadow_falls_below_it_and_stays_subtle() {
    let canvas = Canvas::new(480, 320);
    for mode in MODES {
        for dpi in [96, 168] {
            let (picture, panel, _) =
                render_notice(&canvas, mode, dpi, point(-300, 400), "Copied", Sign::Ok);
            let across = (panel.left + panel.right) / 2;
            let below = at(&picture, across, panel.bottom + scaled(4, dpi));
            let context = format!("{mode:?} at {dpi} DPI");
            assert!(below[0] < BACKGROUND[0], "{below:?} is darkened, {context}");
            assert!(below[0] > 120, "{below:?} is only a shadow, {context}");
            // It falls more than it rises: further below the panel than above it, it is still there.
            let far = scaled(10 + 4, dpi);
            assert!(
                at(&picture, across, panel.bottom + far)[0] < BACKGROUND[0],
                "{context}"
            );
            assert_eq!(
                at(&picture, across, panel.top - scaled(24, dpi)),
                BACKGROUND,
                "{context}"
            );
            // And past its reach, which is 10 + 36 - 8 = 38 below the panel, the photo is as it was.
            assert_eq!(
                at(&picture, across, panel.bottom + scaled(40, dpi)),
                BACKGROUND,
                "{context}"
            );
        }
    }
}
