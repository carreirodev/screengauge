//! Render tests of the magnified view's readout, its coordinates and its color with the swatch and the Approximate
//! color notice, on a WIC bitmap like the rest of `magnified_view_tests`, whose photo and fixtures they use.

use screengauge_core::session::{Coordinate, Label, MagnifiedView};
use screengauge_core::settings::Magnification;
use windows::Win32::Graphics::Direct2D::ID2D1Bitmap;

use super::magnified_view_tests::{Fixture, SCREEN, at, dim, ints, photo, pixels, render};
use super::panel_tests::{MODES, inside, over, rect, tokens};
use super::tests::{BACKGROUND, Canvas, Picture, near, text_width};
use super::*;

/// The widths of one coordinate's texts, each laid out again on its own, in whole pixels: the key, the physical number
/// and, when there is one, the logical number (0 when there is none).
#[derive(Clone, Copy)]
struct Widths {
    key: i32,
    physical: i32,
    logical: i32,
}

/// A view drawn, with what the tests know about its readout from outside the drawing code.
struct Drawn {
    picture: Picture,
    view: MagnifiedView,
    mode: Mode,
    dpi: u32,
    x: Widths,
    y: Widths,
}

/// `view` drawn in `mode` at `dpi`, with the widths of its readout's texts.
fn draw_view(
    canvas: &Canvas,
    photo: &ID2D1Bitmap,
    mode: Mode,
    dpi: u32,
    view: MagnifiedView,
) -> Drawn {
    let mut found = None;
    let picture = canvas.render_in(mode, SCREEN, dpi, |overlay, target| {
        overlay
            .draw_magnified_view(target, &view, photo)
            .expect("the magnified view");
        let widths = |key: &str, coordinate: &Coordinate| Widths {
            key: overlay
                .typeset_plain(key, &overlay.logical_format)
                .expect("the key")
                .width,
            physical: text_width(overlay, &coordinate.label.physical, &overlay.format),
            logical: coordinate
                .label
                .logical
                .as_deref()
                .map_or(0, |text| text_width(overlay, text, &overlay.logical_format)),
        };
        found = Some((widths("X", &view.x), widths("Y", &view.y)));
    });
    let (x, y) = found.expect("the view was drawn");
    Drawn {
        picture,
        view,
        mode,
        dpi,
        x,
        y,
    }
}

impl Drawn {
    fn sized(&self, logical: u32) -> i32 {
        scaled(logical, self.dpi)
    }

    /// The readout: under the enlargement, which holds the lines and the padding around them and nothing else. A few
    /// pixels in from the panel's border and its rounded corners, where the border's own antialiasing would look like
    /// ink.
    fn readout(&self) -> RECT {
        let (panel, enlargement) = (ints(self.view.panel), ints(self.view.enlargement));
        let side = self.sized(1) + self.sized(3);
        let below = self.sized(1) + self.sized(6);
        rect(
            panel.left + side,
            enlargement.bottom,
            panel.right - side,
            panel.bottom - below,
        )
    }

    fn panel_color(&self) -> [u8; 3] {
        tokens(self.mode).panel_over(BACKGROUND)
    }

    /// The pixels of `area` that differ from the panel by more than `by` levels of some channel: ink.
    fn ink(&self, area: RECT, by: u8) -> Vec<(i32, i32)> {
        let panel = self.panel_color();
        pixels(area)
            .filter(|&(x, y)| !near(at(&self.picture, x, y), panel, by))
            .collect()
    }

    /// How far the strongest pixel of `area` is from the panel, in levels of green: the ink at full strength is as far
    /// as the palette's ink is from the panel, and fainter ink is nearer.
    fn contrast(&self, area: RECT) -> u8 {
        let panel = self.panel_color()[1];
        pixels(area)
            .map(|(x, y)| at(&self.picture, x, y)[1].abs_diff(panel))
            .max()
            .expect("an area with pixels")
    }

    /// A coordinate's line and widths: X's, or Y's.
    fn of(&self, y: bool) -> (RECT, Widths) {
        if y {
            (ints(self.view.y.line), self.y)
        } else {
            (ints(self.view.x.line), self.x)
        }
    }

    /// Where the parts of a coordinate's line start, as the prototype's flex row lays them out: the key's box, at least
    /// 14 wide, a gap, the physical number, a gap and a margin, the bar, a margin and a gap, the logical number. Each
    /// size is scaled on its own.
    fn columns(&self, y: bool) -> (i32, i32, i32) {
        let (line, widths) = self.of(y);
        let physical = line.left + widths.key.max(self.sized(14)) + self.sized(8);
        let bar = physical + widths.physical + self.sized(8) + self.sized(7);
        let logical = bar + self.sized(1) + self.sized(7) + self.sized(8);
        (physical, bar, logical)
    }

    /// The ink of the one text `blanked` blanks: the readout's pixels that change when the view is drawn again as
    /// `blanked`. Checks there is some, and that all of it is inside `line`, the text's own line; `what` names the text
    /// in a failure.
    fn blanked_ink(
        &self,
        canvas: &Canvas,
        photo: &ID2D1Bitmap,
        blanked: MagnifiedView,
        line: RECT,
        what: &str,
        context: &str,
    ) -> Vec<(i32, i32)> {
        let without = draw_view(canvas, photo, self.mode, self.dpi, blanked);
        let changed: Vec<(i32, i32)> = pixels(self.readout())
            .filter(|&(x, y)| at(&self.picture, x, y) != at(&without.picture, x, y))
            .collect();
        assert!(changed.len() > 30, "{} pixels, {context}", changed.len());
        for &(x, y) in &changed {
            assert!(
                inside(&line, x, y),
                "{what} at ({x}, {y}) outside its line {line:?}, {context}"
            );
        }
        changed
    }
}

/// The widest realistic readout: a monitor 3840 by 2560 at 150% or 175%, the cursor in the far corner, on a light gray.
/// At 100% there is no logical part. `#DDDDDD` is the widest HEX in Segoe UI semibold, and every RGB of three
/// three-digit channels is as wide as `255, 255, 255`, the digits being tabular.
fn widest(dpi: u32, magnification: Magnification) -> Fixture {
    Fixture {
        labels: [("3839", "2559.3"), ("2159", "1439.3")],
        rgb: [0xdd, 0xdd, 0xdd],
        color: ("#DDDDDD", Some("221, 221, 221")),
        ..Fixture::new(dpi, magnification)
    }
}

/// The color's swatch and its lines, where its ink goes.
fn color_parts(view: &MagnifiedView) -> Vec<RECT> {
    let color = &view.color;
    [
        Some(color.swatch),
        Some(color.first.line),
        color.second.as_ref().map(|second| second.line),
    ]
    .into_iter()
    .flatten()
    .map(ints)
    .collect()
}

#[test]
fn the_colors_texts_stay_inside_their_lines_for_the_widest_values_in_every_color_format() {
    let canvas = Canvas::new(640, 640);
    let photo = photo(&canvas);
    // Color format at Both, HEX and RGB, at 32x, the narrowest readout: at 120 DPI its 5 pixels a side are 160 physical
    // pixels, 128 logical.
    let formats = [
        ("#DDDDDD", Some("221, 221, 221")),
        ("#DDDDDD", None),
        ("221, 221, 221", None),
    ];
    for mode in MODES {
        for dpi in [96, 120, 144, 168] {
            for color in formats {
                let fixture = Fixture {
                    color,
                    ..widest(dpi, Magnification::X32)
                };
                let drawn = draw_view(&canvas, &photo, mode, dpi, fixture.view());
                let context = format!("{color:?}, {mode:?} at {dpi} DPI");
                let readout = drawn.readout();
                let swatch = ints(drawn.view.color.swatch);
                // Each text is what changes when it is blanked, and it is inside its own line, which starts after the
                // swatch and ends before the readout does.
                for second in [false, true] {
                    let mut blanked = drawn.view.clone();
                    let text = if second {
                        match blanked.color.second.as_mut() {
                            Some(text) => text,
                            None => continue,
                        }
                    } else {
                        &mut blanked.color.first
                    };
                    let line = ints(text.line);
                    text.text = " ".to_owned();
                    let changed =
                        drawn.blanked_ink(&canvas, &photo, blanked, line, "text", &context);
                    let left = changed.iter().map(|&(x, _)| x).min().expect("ink");
                    let right = changed.iter().map(|&(x, _)| x).max().expect("ink");
                    assert!(
                        left >= swatch.right && right < line.right && line.right <= readout.right,
                        "ink from column {left} to {right}, swatch {swatch:?}, line {line:?}, readout {readout:?}, \
                         {context}"
                    );
                }
            }
        }
    }
}

#[test]
fn the_hex_is_in_the_ink_and_the_rgb_under_it_fainter() {
    let canvas = Canvas::new(640, 640);
    let photo = photo(&canvas);
    for mode in MODES {
        for dpi in [96, 144] {
            let drawn = draw_view(
                &canvas,
                &photo,
                mode,
                dpi,
                Fixture::new(dpi, Magnification::X8).view(),
            );
            let context = format!("{mode:?} at {dpi} DPI");
            let color = &drawn.view.color;
            let strong = drawn.contrast(ints(color.first.line));
            let second = color.second.as_ref().expect("RGB under HEX");
            let faint = drawn.contrast(ints(second.line));
            // HEX reaches the ink, as the position's numbers do; RGB is at CSS opacity .62.
            let ink_distance = tokens(mode).ink[1].abs_diff(drawn.panel_color()[1]);
            assert!(
                strong.abs_diff(ink_distance) <= 6,
                "{strong} against {ink_distance}, {context}"
            );
            let ratio = f32::from(faint) / f32::from(strong);
            assert!(
                (0.5..0.72).contains(&ratio),
                "RGB is {ratio} of HEX, {context}"
            );
            // With RGB alone, it is in the ink.
            let alone = draw_view(
                &canvas,
                &photo,
                mode,
                dpi,
                Fixture {
                    color: ("30, 144, 255", None),
                    ..Fixture::new(dpi, Magnification::X8)
                }
                .view(),
            );
            let alone_contrast = alone.contrast(ints(alone.view.color.first.line));
            assert!(
                alone_contrast.abs_diff(ink_distance) <= 6,
                "{alone_contrast} against {ink_distance}, {context}"
            );
        }
    }
}

#[test]
fn the_readouts_texts_stay_inside_their_lines_and_the_panel_for_the_widest_numbers() {
    let canvas = Canvas::new(640, 640);
    let photo = photo(&canvas);
    // 32x is the narrowest enlargement, so the narrowest readout. 100% has the lines side by side, the others one under
    // the other.
    for mode in MODES {
        for dpi in [96, 120, 144, 168] {
            let fixture = widest(dpi, Magnification::X32);
            let drawn = draw_view(&canvas, &photo, mode, dpi, fixture.view());
            let context = format!("{mode:?} at {dpi} DPI");
            let (x_line, y_line) = (ints(drawn.view.x.line), ints(drawn.view.y.line));
            let color = color_parts(&drawn.view);
            let readout = drawn.readout();
            // Everything in the readout is in one of the two lines or in the color's row, all inside the panel's
            // border.
            let ink = drawn.ink(readout, 8);
            assert!(ink.len() > 100, "{} ink pixels, {context}", ink.len());
            for &(x, y) in &ink {
                assert!(
                    inside(&x_line, x, y)
                        || inside(&y_line, x, y)
                        || color.iter().any(|part| inside(part, x, y)),
                    "ink at ({x}, {y}) outside the lines {x_line:?} and {y_line:?} and the color's {color:?}, \
                     {context}"
                );
            }
            // Each coordinate's number and bar are inside its own line: what changes when its label is blanked.
            for (y_axis, line) in [(false, x_line), (true, y_line)] {
                let mut blanked = drawn.view.clone();
                let coordinate = if y_axis {
                    &mut blanked.y
                } else {
                    &mut blanked.x
                };
                coordinate.label = Label {
                    physical: " ".to_owned(),
                    logical: None,
                };
                let what = if y_axis { "Y's text" } else { "X's text" };
                let changed = drawn.blanked_ink(&canvas, &photo, blanked, line, what, &context);
                // A logical part, when there is one, ends before the line does.
                if drawn.view.x.label.logical.is_some() {
                    let right = changed.iter().map(|&(x, _)| x).max().expect("ink");
                    assert!(
                        right < line.right,
                        "ink to column {right}, line {line:?}, {context}"
                    );
                }
            }
            // Side by side, the lines don't meet, and neither do the texts.
            if drawn.view.x.label.logical.is_none() {
                assert!(
                    x_line.right < y_line.left,
                    "{x_line:?} and {y_line:?}, {context}"
                );
                assert!(
                    ink.iter().any(|&(x, _)| x >= y_line.left),
                    "Y has no ink, {context}"
                );
                assert!(
                    ink.iter().any(|&(x, _)| x < x_line.right),
                    "X has no ink, {context}"
                );
            }
        }
    }
}

#[test]
fn the_parts_of_a_line_sit_a_key_box_a_gap_a_bar_and_a_gap_apart() {
    let canvas = Canvas::new(640, 640);
    let photo = photo(&canvas);
    for mode in MODES {
        for dpi in [120, 144, 168] {
            let drawn = draw_view(
                &canvas,
                &photo,
                mode,
                dpi,
                Fixture::new(dpi, Magnification::X16).view(),
            );
            let ink = tokens(mode).ink;
            for y_axis in [false, true] {
                let (line, widths) = drawn.of(y_axis);
                let (physical, bar, logical) = drawn.columns(y_axis);
                let context = format!("{mode:?} at {dpi} DPI, line {y_axis}");
                let rows = |left: i32, right: i32| rect(left, line.top, right, line.bottom);
                // The key is first, and the room up to the number, which is the key's box and a gap, is empty after it.
                assert!(
                    !drawn
                        .ink(rows(line.left, line.left + widths.key), 20)
                        .is_empty(),
                    "{context}"
                );
                let gap = drawn.ink(rows(line.left + widths.key + 1, physical - 1), 8);
                assert!(
                    gap.is_empty(),
                    "{} ink pixels between the key and the number, {context}",
                    gap.len()
                );
                // The number starts there, and ends before the gap that leads to the bar, where the bar is.
                assert!(
                    !drawn
                        .ink(rows(physical, physical + drawn.sized(6)), 20)
                        .is_empty(),
                    "{context}"
                );
                let gap = drawn.ink(rows(physical + widths.physical, bar), 8);
                assert!(
                    gap.is_empty(),
                    "{} ink pixels between the number and the bar, {context}",
                    gap.len()
                );
                // The bar is a pixel of the ink at 40%, as wide and as tall as the prototype's, scaled, in the middle
                // of the line, with the panel on every side.
                let (width, height) = (drawn.sized(1), drawn.sized(11));
                let top = line.top + (line.bottom - line.top - height + 1) / 2;
                let faint = over(ink, 0.4, drawn.panel_color());
                for (x, y) in pixels(rect(bar, top, bar + width, top + height)) {
                    assert!(
                        near(at(&drawn.picture, x, y), faint, 2),
                        "the bar at ({x}, {y}) is {:?}, wanted {faint:?}, {context}",
                        at(&drawn.picture, x, y)
                    );
                }
                let around = [
                    (bar - 1, top + 2),
                    (bar + width, top + 2),
                    (bar, top - 1),
                    (bar, top + height),
                ];
                for (x, y) in around {
                    assert!(
                        near(at(&drawn.picture, x, y), drawn.panel_color(), 1),
                        "({x}, {y}) beside the bar is {:?}, {context}",
                        at(&drawn.picture, x, y)
                    );
                }
                // The logical number follows the bar and a margin and a gap.
                let gap = drawn.ink(rows(bar + width, logical), 8);
                assert!(
                    gap.is_empty(),
                    "{} ink pixels between the bar and the logical number, {context}",
                    gap.len()
                );
                assert!(
                    !drawn
                        .ink(rows(logical, logical + widths.logical), 20)
                        .is_empty(),
                    "{context}"
                );
            }
        }
    }
}

#[test]
fn the_logical_part_is_fainter_than_the_physical_part_and_the_key_is_dim() {
    let canvas = Canvas::new(640, 640);
    let photo = photo(&canvas);
    for mode in MODES {
        let drawn = draw_view(
            &canvas,
            &photo,
            mode,
            144,
            Fixture::new(144, Magnification::X16).view(),
        );
        let t = tokens(mode);
        let context = format!("{mode:?}");
        for y_axis in [false, true] {
            let (line, widths) = drawn.of(y_axis);
            let (physical, _, logical) = drawn.columns(y_axis);
            let rows = |left: i32, right: i32| rect(left, line.top, right, line.bottom);
            let strong = drawn.contrast(rows(physical, physical + widths.physical));
            let faint = drawn.contrast(rows(logical, logical + widths.logical));
            let key = drawn.contrast(rows(line.left, line.left + widths.key));
            // The physical number reaches the ink all through; the logical one at CSS opacity .62 and the bar at .4.
            let ink_distance = t.ink[1].abs_diff(drawn.panel_color()[1]);
            assert!(
                strong.abs_diff(ink_distance) <= 6,
                "{strong} against {ink_distance}, {context}"
            );
            let ratio = f32::from(faint) / f32::from(strong);
            assert!(
                (0.5..0.72).contains(&ratio),
                "the logical part is {ratio} of the physical one, {context}"
            );
            // The key is in the dim color: between the two, the ink's own contrast being the greater.
            let dim_distance = dim(mode)[1].abs_diff(drawn.panel_color()[1]);
            assert!(
                key.abs_diff(dim_distance) <= 14,
                "{key} against {dim_distance}, {context}"
            );
            assert!(
                key + 20 < strong,
                "the key at {key} is not dimmer than the number at {strong}, {context}"
            );
        }
    }
}

#[test]
fn the_texts_of_a_line_share_a_baseline_and_sit_in_the_middle_of_the_line() {
    let canvas = Canvas::new(640, 640);
    let photo = photo(&canvas);
    for mode in MODES {
        for dpi in [96, 144, 168] {
            // At 100% the lines are side by side and have no logical part: the key and the number share a baseline.
            let drawn = draw_view(
                &canvas,
                &photo,
                mode,
                dpi,
                Fixture::new(dpi, Magnification::X16).view(),
            );
            for y_axis in [false, true] {
                let (line, widths) = drawn.of(y_axis);
                let (physical, _, logical) = drawn.columns(y_axis);
                let context = format!("{mode:?} at {dpi} DPI, line {y_axis}");
                let extent = |left: i32, right: i32| {
                    let rows: Vec<i32> = drawn
                        .ink(rect(left, line.top, right, line.bottom), 40)
                        .iter()
                        .map(|&(_, y)| y)
                        .collect();
                    (
                        *rows.iter().min().expect("strong ink"),
                        *rows.iter().max().expect("strong ink"),
                    )
                };
                let (key_top, key_bottom) = extent(line.left, line.left + widths.key);
                let (number_top, number_bottom) = extent(physical, physical + widths.physical);
                // Capital letters and digits stand on the same line, the same height or so.
                assert_eq!(key_bottom, number_bottom, "{context}");
                assert!(
                    key_top.abs_diff(number_top) <= 1,
                    "{key_top} against {number_top}, {context}"
                );
                if widths.logical > 0 {
                    let (_, logical_bottom) = extent(logical, logical + widths.logical);
                    assert_eq!(
                        logical_bottom, number_bottom,
                        "the logical number's baseline, {context}"
                    );
                }
                // In the middle of the line, as CSS centers a line of text: a digit has room above it and below, within
                // what the font's ascent and descent leave, which is a pixel or two more above than below.
                let (above, below) = (number_top - line.top, line.bottom - 1 - number_bottom);
                assert!(
                    above > 0 && below > 0,
                    "{above} above and {below} below, {context}"
                );
                assert!(
                    above.abs_diff(below) <= u32::try_from(drawn.sized(2) + 1).expect("a size"),
                    "{above} above and {below} below, {context}"
                );
            }
        }
    }
}

#[test]
fn the_swatch_is_exactly_the_pixels_color_inside_a_ring_of_the_border_with_round_corners() {
    let canvas = Canvas::new(640, 640);
    let photo = photo(&canvas);
    for mode in MODES {
        for dpi in [96, 120, 144, 168] {
            let fixture = Fixture::new(dpi, Magnification::X8);
            let view = fixture.view();
            let picture = render(&canvas, &photo, mode, dpi, &view);
            let context = format!("{mode:?} at {dpi} DPI");
            // `.sw`: 22 a side, its ring of 1 inside it, in `--panel-border` over the swatch's own color, as CSS paints
            // a border over the box's background, and corners of 6.
            let swatch = ints(view.color.swatch);
            assert_eq!(swatch.right - swatch.left, scaled(22, dpi), "{context}");
            let (ring, radius) = (scaled(1, dpi), scaled(6, dpi));
            let tokens = tokens(mode);
            let ring_color = over(tokens.border.0, tokens.border.1, fixture.rgb);
            for (x, y) in pixels(swatch) {
                let across = (x - swatch.left).min(swatch.right - 1 - x);
                let down = (y - swatch.top).min(swatch.bottom - 1 - y);
                if across < radius && down < radius {
                    continue;
                }
                let found = at(&picture, x, y);
                if across < ring || down < ring {
                    assert!(
                        near(found, ring_color, 1),
                        "ring pixel ({x}, {y}) is {found:?}, wanted {ring_color:?}, {context}"
                    );
                } else {
                    // Pixel-exact: the color the user copies is the one they see.
                    assert_eq!(found, fixture.rgb, "pixel ({x}, {y}), {context}");
                }
            }
            // The corner's own pixel is past the round corner: the panel shows there.
            let panel = tokens.panel_over(BACKGROUND);
            let corner = at(&picture, swatch.left, swatch.top);
            assert!(
                near(corner, panel, 2),
                "corner {corner:?}, panel {panel:?}, {context}"
            );
        }
    }
}

#[test]
fn the_approximate_color_notice_is_amber_and_stays_inside_its_sign_and_lines_on_the_narrowest_readouts()
 {
    let canvas = Canvas::new(640, 640);
    let photo = photo(&canvas);
    // At 32x the readout is narrowest: at 120 DPI its 5 pixels a side are 160 physical pixels, 128 logical, and its
    // text 120 of them. 8x is the first magnification.
    for mode in MODES {
        let warn = tokens(mode).warn;
        for dpi in [96, 120, 144, 168] {
            for magnification in [Magnification::X32, Magnification::X8] {
                let fixture = Fixture {
                    approximate: true,
                    ..widest(dpi, magnification)
                };
                let drawn = draw_view(&canvas, &photo, mode, dpi, fixture.view());
                let context = format!("{mode:?} at {dpi} DPI and {magnification:?}");
                let readout = drawn.readout();
                let notice = drawn.view.approximate_color.clone().expect("the notice");
                let sign = ints(notice.sign);
                let lines = notice.lines.clone().map(|line| ints(line.line));
                // Leaving the notice out of the same panel changes the sign and the lines alone.
                let mut without = drawn.view.clone();
                without.approximate_color = None;
                let without = draw_view(&canvas, &photo, mode, dpi, without);
                let changed: Vec<(i32, i32)> = pixels(readout)
                    .filter(|&(x, y)| at(&drawn.picture, x, y) != at(&without.picture, x, y))
                    .collect();
                for &(x, y) in &changed {
                    assert!(
                        inside(&sign, x, y) || lines.iter().any(|line| inside(line, x, y)),
                        "ink at ({x}, {y}) outside the sign {sign:?} and the lines {lines:?}, {context}"
                    );
                }
                let amber = |area: &RECT, changed: &[(i32, i32)]| {
                    changed.iter().any(|&(x, y)| {
                        inside(area, x, y) && near(at(&drawn.picture, x, y), warn, 12)
                    })
                };
                assert!(
                    amber(&sign, &changed),
                    "no amber in the sign {sign:?}, {context}"
                );
                // Each line's text is what changes when it alone is blanked: inside its own line, which starts after
                // the sign and ends before the readout does, and amber.
                for (index, line) in lines.iter().enumerate() {
                    let what = notice.lines[index].text;
                    let mut blanked = drawn.view.clone();
                    if let Some(notice) = blanked.approximate_color.as_mut() {
                        notice.lines[index].text = " ";
                    }
                    let without = draw_view(&canvas, &photo, mode, dpi, blanked);
                    let changed: Vec<(i32, i32)> = pixels(readout)
                        .filter(|&(x, y)| at(&drawn.picture, x, y) != at(&without.picture, x, y))
                        .collect();
                    assert!(
                        changed.len() > 30,
                        "{} pixels of {what:?}, {context}",
                        changed.len()
                    );
                    for &(x, y) in &changed {
                        assert!(
                            inside(line, x, y),
                            "{what:?} at ({x}, {y}) outside its line {line:?}, {context}"
                        );
                    }
                    let left = changed.iter().map(|&(x, _)| x).min().expect("ink");
                    let right = changed.iter().map(|&(x, _)| x).max().expect("ink");
                    assert!(
                        left >= sign.right && right < line.right && line.right <= readout.right,
                        "{what:?} from column {left} to {right}, sign {sign:?}, line {line:?}, readout {readout:?}, {context}"
                    );
                    assert!(amber(line, &changed), "no amber in {what:?}, {context}");
                }
            }
        }
    }
}
