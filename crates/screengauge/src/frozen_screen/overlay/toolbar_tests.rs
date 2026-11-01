//! Render tests of the toolbar on a WIC bitmap, like the notice's in `panel_tests`. The sizes and colors here are the
//! prototype's (variant A of WPC-279: `.grp`, `.tbtn`, `.mbtn`, `.sbtn` and the palettes' tokens), written out a second
//! time and scaled in the tests, not read back from the code that is being tested. The shadows are judged by the owner's
//! eye; these check where things are, what is lit, and what is not drawn.

use screengauge_core::settings::{DimensionsMode, Tool};

use super::geometry::{self, scaled};
use super::palette::d2d_color;
use super::panel_tests::{MODES, over};
use super::tests::{BACKGROUND, Canvas, Picture, SCALES, near};
use super::toolbar::Layout;
use super::*;
use super::{toolbar_paint, toolbar_palette};
use screengauge_core::session::toolbar::Hit;
use screengauge_core::session::{Point, Toolbar, ToolbarButton};

/// A window wide enough for the toolbar at 175% and tall enough for its shadow, over a monitor left of and below the main
/// one, so coordinates in the virtual screen and in the window are different numbers.
const TOOLBAR_MONITOR: Rect = Rect {
    left: -1280,
    top: 120,
    width: 1280,
    height: 440,
};

fn canvas() -> Canvas {
    Canvas::new(1280, 440)
}

/// A stop of a CSS gradient that runs from the top of its box to the bottom: where along the height, and the color.
type Stops = &'static [(f64, [u8; 3])];

/// One palette's colors as the prototype states them (`--btn-bg`, `--on-bg`, `--sbtn-bg`, `--grp-bg`, `--grp-border`),
/// written out here a second time so that a slip in the palette is caught.
struct Tokens {
    button: Stops,
    lit: Stops,
    small: Stops,
    /// The panel's base color and opacity, and the white gradient over it: its opacity at the top and at the bottom.
    group_base: ([u8; 3], f64),
    group_top: f64,
    group_bottom: f64,
    group_border: ([u8; 3], f64),
    /// `--ink`, `--ink-on` and `--sbtn-ink`.
    ink: [u8; 3],
    ink_on: [u8; 3],
    small_ink: [u8; 3],
    /// `--sbtn-ink-hover`.
    small_ink_hover: [u8; 3],
}

/// `.sbtn.close:hover`'s background, in both palettes: oklch(0.75 0.17 25), then oklch(0.5 0.17 25) from 85% down.
const CLOSE_RED: Stops = &[(0.0, [0xff, 0x7d, 0x76]), (0.85, [0xb0, 0x2a, 0x2d])];

fn tokens(mode: Mode) -> Tokens {
    match mode {
        Mode::Dark => Tokens {
            button: &[
                (0.0, [0x55, 0x5a, 0x64]),
                (0.55, [0x2a, 0x2d, 0x33]),
                (1.0, [0x1a, 0x1b, 0x1f]),
            ],
            // oklch(0.92 0.1 195), oklch(0.74 0.15 205), oklch(0.5 0.14 220)
            lit: &[
                (0.0, [0x8e, 0xfa, 0xf9]),
                (0.5, [0x00, 0xc4, 0xd7]),
                (1.0, [0x00, 0x73, 0x9a]),
            ],
            small: &[(0.0, [0x4a, 0x4e, 0x57]), (0.85, [0x1e, 0x20, 0x24])],
            group_base: ([24, 25, 29], 0.86),
            group_top: 0.09,
            group_bottom: 0.02,
            group_border: ([255, 255, 255], 0.08),
            ink: [0xc3, 0xc7, 0xcf],
            ink_on: [0x06, 0x14, 0x1a],
            small_ink: [0xae, 0xb3, 0xbd],
            small_ink_hover: [0xff, 0xff, 0xff],
        },
        Mode::Light => Tokens {
            button: &[
                (0.0, [0xff, 0xff, 0xff]),
                (0.5, [0xf1, 0xef, 0xeb]),
                (1.0, [0xd6, 0xd2, 0xcb]),
            ],
            // oklch(0.85 0.1 220), oklch(0.62 0.16 235), oklch(0.45 0.15 245)
            lit: &[
                (0.0, [0x7d, 0xdd, 0xfb]),
                (0.55, [0x00, 0x92, 0xd7]),
                (1.0, [0x00, 0x58, 0xa1]),
            ],
            small: &[(0.0, [0xff, 0xff, 0xff]), (0.9, [0xdc, 0xd8, 0xd1])],
            group_base: ([226, 224, 219], 0.8),
            group_top: 0.85,
            group_bottom: 0.45,
            group_border: ([255, 255, 255], 0.9),
            ink: [0x4b, 0x48, 0x43],
            ink_on: [0xff, 0xff, 0xff],
            small_ink: [0x5b, 0x58, 0x52],
            small_ink_hover: [0x11, 0x11, 0x11],
        },
    }
}

/// The color of `stops` at `t` of the way down, blended in sRGB as CSS does, and held at the ends.
fn gradient_at(stops: Stops, t: f64) -> [u8; 3] {
    let (first, last) = (stops[0], stops[stops.len() - 1]);
    if t <= first.0 {
        return first.1;
    }
    if t >= last.0 {
        return last.1;
    }
    for pair in stops.windows(2) {
        let ((from, a), (to, b)) = (pair[0], pair[1]);
        if t <= to {
            let k = (t - from) / (to - from);
            // A mix of two channels with weights adding up to 1 stays within 0 to 255, so the cast only drops the
            // fraction `round` already took away.
            // TODO: mix in whole numbers, so no float comes back to `u8`.
            return std::array::from_fn(|i| {
                (f64::from(a[i]) * (1.0 - k) + f64::from(b[i]) * k).round() as u8
            });
        }
    }
    last.1
}

/// `over`, for the tests' floats.
fn blend(color: [u8; 3], alpha: f64, under: [u8; 3]) -> [u8; 3] {
    // `over` takes the single-precision opacity the palette has; this is the CSS number, which fits.
    over(color, narrow(alpha), under)
}

/// A CSS opacity, 0 to 1, as the `f32` the drawing takes.
// TODO: keep the opacities in `f32` in the tests, so nothing narrows here.
fn narrow(alpha: f64) -> f32 {
    alpha as f32
}

/// A pixel by its place in the virtual screen, which must be in the window.
fn at(picture: &Picture, x: i32, y: i32) -> [u8; 3] {
    let column =
        usize::try_from(x - TOOLBAR_MONITOR.left).expect("a pixel right of the window's left side");
    let row = usize::try_from(y - TOOLBAR_MONITOR.top).expect("a pixel below the window's top");
    picture.at(column, row)
}

/// The toolbar drawn alone, and where its parts are.
struct Drawn {
    picture: Picture,
    layout: Layout,
    dpi: u32,
}

fn bounds() -> RECT {
    geometry::bounds(TOOLBAR_MONITOR).expect("the monitor")
}

fn scene(active: Tool, mode: Option<DimensionsMode>) -> Toolbar {
    Toolbar {
        monitor: 0,
        active,
        mode,
        minimized: false,
        hovered: None,
    }
}

/// Where the core puts the toolbar on the tests' monitor at `dpi`.
fn toolbar_layout(dpi: u32) -> Layout {
    Layout::of(TOOLBAR_MONITOR, Scale::from_dpi(dpi).expect("a scale")).expect("the monitor")
}

/// `toolbar` drawn over `BACKGROUND` in the palette of `mode` for a window of `dpi`.
fn draw(canvas: &Canvas, mode: Mode, dpi: u32, toolbar: &Toolbar) -> Drawn {
    draw_over(canvas, mode, dpi, toolbar, BACKGROUND)
}

/// As `draw`, over a photo that is all `photo`.
fn draw_over(canvas: &Canvas, mode: Mode, dpi: u32, toolbar: &Toolbar, photo: [u8; 3]) -> Drawn {
    let photo = d2d_color(photo, 1.0);
    let picture = canvas.render_in(mode, TOOLBAR_MONITOR, dpi, |overlay, target| {
        // SAFETY: a drawing call between BeginDraw and EndDraw on a live target.
        unsafe { target.Clear(Some(&photo)) };
        overlay
            .draw_toolbar(target, toolbar, None)
            .expect("the toolbar");
    });
    Drawn {
        picture,
        layout: toolbar_layout(dpi),
        dpi,
    }
}

impl Drawn {
    fn at(&self, x: i32, y: i32) -> [u8; 3] {
        at(&self.picture, x, y)
    }

    fn place(&self, button: ToolbarButton) -> RECT {
        self.layout
            .buttons
            .iter()
            .find(|(each, _)| *each == button)
            .map(|&(_, place)| place)
            .expect("every button has a place")
    }
}

/// Where a button's background is sampled: its middle row, a few pixels in from its left side, clear of the icon and of
/// the inset shadows along its edges.
fn sample_point(place: &RECT, dpi: u32) -> (i32, i32) {
    (
        place.left + scaled(6, dpi),
        place.top + (place.bottom - place.top) / 2,
    )
}

/// How far down `place` the pixel row `y` is, at its middle.
fn down(place: &RECT, y: i32) -> f64 {
    (f64::from(y) + 0.5 - f64::from(place.top)) / f64::from(place.bottom - place.top)
}

fn is_control(button: ToolbarButton) -> bool {
    matches!(
        button,
        ToolbarButton::Settings | ToolbarButton::Minimize | ToolbarButton::Close
    )
}

/// The buttons the scene lights: the active tool's, and the mode's while there is one.
fn lit_in(toolbar: &Toolbar) -> Vec<ToolbarButton> {
    let mut lit = vec![ToolbarButton::Tool(toolbar.active)];
    lit.extend(toolbar.mode.map(ToolbarButton::Mode));
    lit
}

/// The buttons drawn lit: those whose sample is nearer the lit gradient than the plain one.
fn drawn_lit(drawn: &Drawn, t: &Tokens) -> Vec<ToolbarButton> {
    let distance = |a: [u8; 3], b: [u8; 3]| -> i32 {
        a.iter()
            .zip(b)
            .map(|(&a, b)| i32::from(a.abs_diff(b)))
            .sum()
    };
    drawn
        .layout
        .buttons
        .iter()
        .filter(|&&(button, place)| {
            let (x, y) = sample_point(&place, drawn.dpi);
            let got = drawn.at(x, y);
            let plain = if is_control(button) {
                t.small
            } else {
                t.button
            };
            let t_down = down(&place, y);
            distance(got, gradient_at(t.lit, t_down)) < distance(got, gradient_at(plain, t_down))
        })
        .map(|&(button, _)| button)
        .collect()
}

#[test]
fn each_buttons_middle_is_its_gradient_and_the_lit_ones_are_the_lit_gradient() {
    let canvas = canvas();
    let toolbar = scene(Tool::Inspect, Some(DimensionsMode::Vertical));
    let lit = lit_in(&toolbar);
    for mode in MODES {
        let t = tokens(mode);
        for dpi in SCALES {
            let drawn = draw(&canvas, mode, dpi, &toolbar);
            for (button, place) in drawn.layout.buttons {
                let stops = if lit.contains(&button) {
                    t.lit
                } else if is_control(button) {
                    t.small
                } else {
                    t.button
                };
                let (x, y) = sample_point(&place, dpi);
                let wanted = gradient_at(stops, down(&place, y));
                assert!(
                    near(drawn.at(x, y), wanted, 4),
                    "{button:?} at ({x}, {y}) is {:?}, wanted {wanted:?}: {mode:?} at {dpi} DPI",
                    drawn.at(x, y)
                );
            }
        }
    }
}

#[test]
fn the_lit_buttons_follow_the_active_tool_and_the_mode() {
    let canvas = canvas();
    let cases = [
        scene(Tool::Dimensions, Some(DimensionsMode::Horizontal)),
        scene(Tool::Dimensions, Some(DimensionsMode::Vertical)),
        scene(Tool::Dimensions, Some(DimensionsMode::Cross)),
        scene(Tool::Bounds, None),
        scene(Tool::Inspect, None),
        scene(Tool::Snip, None),
    ];
    for mode in MODES {
        let t = tokens(mode);
        for dpi in [96, 144] {
            for toolbar in &cases {
                let drawn = draw(&canvas, mode, dpi, toolbar);
                let mut lit = drawn_lit(&drawn, &t);
                // The modes are drawn only while there is one.
                let mut wanted = lit_in(toolbar);
                lit.sort_by_key(|button| format!("{button:?}"));
                wanted.sort_by_key(|button| format!("{button:?}"));
                assert_eq!(lit, wanted, "{toolbar:?}: {mode:?} at {dpi} DPI");
            }
        }
    }
}

#[test]
fn a_lit_button_glows_onto_the_panel_beside_it() {
    let canvas = canvas();
    for mode in MODES {
        for dpi in SCALES {
            let lit = draw(&canvas, mode, dpi, &scene(Tool::Dimensions, None));
            let plain = draw(&canvas, mode, dpi, &scene(Tool::Snip, None));
            // Halfway across the gap between Dimensions and Bounds, at the buttons' middle height.
            let first = lit.place(ToolbarButton::Tool(Tool::Dimensions));
            let second = lit.place(ToolbarButton::Tool(Tool::Bounds));
            let x = (first.right + second.left) / 2;
            let y = (first.top + first.bottom) / 2;
            // Cyan on the panel: bluer than it is red by more than the plain panel is.
            let tint = |p: [u8; 3]| i32::from(p[2]) - i32::from(p[0]);
            assert!(
                tint(lit.at(x, y)) >= tint(plain.at(x, y)) + 10,
                "{:?} beside a lit button, {:?} beside a plain one: {mode:?} at {dpi} DPI",
                lit.at(x, y),
                plain.at(x, y)
            );
        }
    }
}

#[test]
fn a_hidden_modes_room_shows_the_photo_and_no_button() {
    let canvas = canvas();
    for mode in MODES {
        for dpi in SCALES {
            let drawn = draw(&canvas, mode, dpi, &scene(Tool::Inspect, None));
            let room = drawn.layout.modes;
            // The neighbors' shadows fade out before the room's middle third: there the photo is untouched.
            let third = (room.right - room.left) / 3;
            for y in room.top..room.bottom {
                for x in room.left + third..room.right - third {
                    assert!(
                        near(drawn.at(x, y), BACKGROUND, 1),
                        "({x}, {y}) is {:?} in the empty room: {mode:?} at {dpi} DPI",
                        drawn.at(x, y)
                    );
                }
            }
            // And where H, V and C would be, nothing of a button or a panel: the photo, a shade darker at most.
            for mode_button in [
                DimensionsMode::Horizontal,
                DimensionsMode::Vertical,
                DimensionsMode::Cross,
            ] {
                let place = drawn.place(ToolbarButton::Mode(mode_button));
                let (x, y) = (
                    (place.left + place.right) / 2,
                    (place.top + place.bottom) / 2,
                );
                assert!(
                    near(drawn.at(x, y), BACKGROUND, 12),
                    "{mode_button:?} at ({x}, {y}) is {:?} with no mode: {mode:?} at {dpi} DPI",
                    drawn.at(x, y)
                );
            }
        }
    }
}

#[test]
fn the_modes_panel_is_drawn_while_there_is_a_mode() {
    let canvas = canvas();
    for mode in MODES {
        for dpi in SCALES {
            let drawn = draw(
                &canvas,
                mode,
                dpi,
                &scene(Tool::Dimensions, Some(DimensionsMode::Cross)),
            );
            let room = drawn.layout.modes;
            // The room's middle, which the panel covers.
            let (x, y) = ((room.left + room.right) / 2, room.top + scaled(3, dpi));
            assert!(
                !near(drawn.at(x, y), BACKGROUND, 40),
                "({x}, {y}) is {:?}, the photo, with a mode: {mode:?} at {dpi} DPI",
                drawn.at(x, y)
            );
        }
    }
}

/// How far the shadows reach past the toolbar in the prototype's numbers, in logical pixels, to the left and right, above
/// and below: three deviations of the blur less the spread, and the drop below.
fn shadow_reach(mode: Mode) -> (u32, u32, u32) {
    match mode {
        // 0 18px 40px -12px: 3 * 20 - 12 to the sides, 18 less above and more below.
        Mode::Dark => (48, 30, 66),
        // 0 18px 36px -14px: 3 * 18 - 14.
        Mode::Light => (40, 22, 58),
    }
}

#[test]
fn nothing_is_drawn_beyond_the_toolbar_and_the_reach_of_its_shadows() {
    let canvas = canvas();
    for mode in MODES {
        let (sides, above, below) = shadow_reach(mode);
        for dpi in SCALES {
            let toolbar = scene(Tool::Dimensions, Some(DimensionsMode::Horizontal));
            let drawn = draw(&canvas, mode, dpi, &toolbar);
            // Each reach is scaled as the whole, with a few pixels for the parts being scaled one at a time.
            let slack = 3;
            let area = RECT {
                left: drawn.layout.toolbar.left - scaled(sides, dpi) - slack,
                top: drawn.layout.toolbar.top - scaled(above, dpi) - slack,
                right: drawn.layout.toolbar.right + scaled(sides, dpi) + slack,
                bottom: drawn.layout.toolbar.bottom + scaled(below, dpi) + slack,
            };
            let window = bounds();
            let mut inside_changed = 0;
            for y in window.top..window.bottom {
                for x in window.left..window.right {
                    let outside =
                        x < area.left || x >= area.right || y < area.top || y >= area.bottom;
                    let pixel = drawn.at(x, y);
                    if outside {
                        assert_eq!(
                            pixel, BACKGROUND,
                            "({x}, {y}) is drawn outside {area:?}: {mode:?} at {dpi} DPI"
                        );
                    } else if pixel != BACKGROUND {
                        inside_changed += 1;
                    }
                }
            }
            assert!(inside_changed > 5000, "{inside_changed} pixels drawn");
        }
    }
}

#[test]
fn each_group_panel_is_where_the_layout_puts_it() {
    let canvas = canvas();
    let toolbar = scene(Tool::Dimensions, Some(DimensionsMode::Horizontal));
    for mode in MODES {
        for dpi in SCALES {
            // Over black the dark palette's black shadow is invisible, so the panel's edge is all that shows.
            let drawn = draw_over(&canvas, mode, dpi, &toolbar, [0, 0, 0]);
            for group in [
                drawn.layout.tools,
                drawn.layout.modes,
                drawn.layout.controls,
            ] {
                let (x, y) = (
                    (group.left + group.right) / 2,
                    (group.top + group.bottom) / 2,
                );
                let green = |x, y| i32::from(drawn.at(x, y)[1]);
                // The panel's edge pixel against the pixel outside it, on each side, at least 12 levels apart.
                for (inside, outside, side) in [
                    (green(group.left, y), green(group.left - 1, y), "left"),
                    (green(group.right - 1, y), green(group.right, y), "right"),
                    (green(x, group.top), green(x, group.top - 1), "top"),
                    (green(x, group.bottom - 1), green(x, group.bottom), "bottom"),
                ] {
                    assert!(
                        (inside - outside).abs() >= 12,
                        "the {side} edge of {group:?}: {inside} against {outside}: {mode:?} at {dpi} DPI"
                    );
                }
            }
        }
    }
}

#[test]
fn a_panel_is_the_palettes_see_through_base_under_its_gradient_with_its_ring_around() {
    let canvas = canvas();
    // The last tool is lit, so the first group's left side and top have no glow over them.
    let toolbar = scene(Tool::Snip, None);
    for mode in MODES {
        let t = tokens(mode);
        for dpi in SCALES {
            let over_photo = draw(&canvas, mode, dpi, &toolbar);
            // A second photo, black, to show the panel lets it through.
            let over_black = draw_over(&canvas, mode, dpi, &toolbar, [0, 0, 0]);
            let group = over_photo.layout.tools;
            let ring = scaled(1, dpi);
            // The padding box runs from just inside the ring to just inside the ring at the bottom.
            let (top, bottom) = (group.top + ring, group.bottom - ring);
            let height = f64::from(bottom - top);
            // A row in the padding above the buttons, away from the shadows of the buttons and the group's edge, in
            // the middle of the group's width.
            let (x, y) = ((group.left + group.right) / 2, top + scaled(4, dpi));
            let along = (f64::from(y) + 0.5 - f64::from(top)) / height;
            let alpha = t.group_top + (t.group_bottom - t.group_top) * along;
            let expected = |under: [u8; 3]| {
                let base = blend(t.group_base.0, t.group_base.1, under);
                blend([255, 255, 255], alpha, base)
            };
            for (drawn, under, name) in [
                (&over_photo, BACKGROUND, "the photo"),
                (&over_black, [0, 0, 0], "black"),
            ] {
                assert!(
                    near(drawn.at(x, y), expected(under), 3),
                    "the panel over {name} at ({x}, {y}) is {:?}, wanted {:?}: {mode:?} at {dpi} DPI",
                    drawn.at(x, y),
                    expected(under)
                );
            }
            // It lets the photo through: the two differ by about the share of it the panel does not cover.
            let (a, b) = (over_photo.at(x, y), over_black.at(x, y));
            let shows = i32::from(a[0].abs_diff(b[0])) + i32::from(a[1].abs_diff(b[1]));
            assert!(
                shows >= 8,
                "the panel is opaque: {a:?} over the photo, {b:?} over black"
            );

            // The ring, on the left side at the group's middle height: the border over the panel, ring pixels wide.
            let middle = (group.top + group.bottom) / 2;
            let panel_there = {
                let along = (f64::from(middle) + 0.5 - f64::from(top)) / height;
                let alpha = t.group_top + (t.group_bottom - t.group_top) * along;
                blend(
                    [255, 255, 255],
                    alpha,
                    blend(t.group_base.0, t.group_base.1, BACKGROUND),
                )
            };
            let ring_color = blend(t.group_border.0, t.group_border.1, panel_there);
            for k in 0..ring {
                assert!(
                    near(over_photo.at(group.left + k, middle), ring_color, 3),
                    "ring column {k} is {:?}, wanted {ring_color:?}: {mode:?} at {dpi} DPI",
                    over_photo.at(group.left + k, middle)
                );
            }
        }
    }
}

#[test]
fn a_button_has_rounded_corners_and_a_shadow_that_falls_below_it() {
    let canvas = canvas();
    let toolbar = scene(Tool::Snip, None);
    for mode in MODES {
        for dpi in SCALES {
            let drawn = draw(&canvas, mode, dpi, &toolbar);
            let place = drawn.place(ToolbarButton::Tool(Tool::Bounds));
            let t = tokens(mode);
            let green = |x, y| i32::from(drawn.at(x, y)[1]);
            // The corner pixel is the panel, not the button's top color: the corner is round.
            let corner = drawn.at(place.left, place.top);
            let top_color = gradient_at(t.button, 0.0);
            assert!(
                !near(corner, top_color, 5),
                "{corner:?} at the corner, the button's own color is {top_color:?}: {mode:?} at {dpi} DPI"
            );
            // The drop: the panel just below the button is darker than just above it, which the shadow hardly reaches.
            let x = (place.left + place.right) / 2;
            let above = green(x, place.top - scaled(2, dpi));
            let below = green(x, place.bottom + scaled(2, dpi));
            assert!(
                above - below >= 8,
                "{above} above, {below} below: {mode:?} at {dpi} DPI"
            );
        }
    }
}

#[test]
fn drawing_the_same_scene_twice_gives_the_same_pixels() {
    let canvas = canvas();
    let toolbar = scene(Tool::Bounds, None);
    for mode in MODES {
        for dpi in [96, 168] {
            let first = draw(&canvas, mode, dpi, &toolbar);
            let second = draw(&canvas, mode, dpi, &toolbar);
            assert!(
                first.picture.pixels == second.picture.pixels,
                "{mode:?} at {dpi} DPI"
            );
        }
    }
}

/// Each icon's CSS size and what it covers of its 24 by 24 view box, stroke included (left, top, right, bottom), worked out
/// by hand from the prototype's SVGs: the extent of the points the strokes pass through, and half a stroke width more
/// all round, as the ends and corners are round. Inspect's magnifying glass is not centered on (12, 12), nor the gear's
/// outline quite inside its box.
const ICONS: [(ToolbarButton, u32, [f64; 4]); 10] = [
    (
        ToolbarButton::Tool(Tool::Dimensions),
        20,
        [1.5, 7.0, 22.5, 17.0],
    ),
    (
        ToolbarButton::Tool(Tool::Bounds),
        20,
        [3.0, 3.0, 21.0, 21.0],
    ),
    (
        ToolbarButton::Tool(Tool::Inspect),
        20,
        [4.0, 4.0, 21.0, 21.0],
    ),
    (ToolbarButton::Tool(Tool::Snip), 20, [3.0, 4.0, 21.0, 20.0]),
    (
        ToolbarButton::Mode(DimensionsMode::Horizontal),
        16,
        [2.9, 6.9, 21.1, 17.1],
    ),
    (
        ToolbarButton::Mode(DimensionsMode::Vertical),
        16,
        [6.9, 2.9, 17.1, 21.1],
    ),
    (
        ToolbarButton::Mode(DimensionsMode::Cross),
        16,
        [1.9, 1.9, 22.1, 22.1],
    ),
    (ToolbarButton::Settings, 16, [-0.05, -0.05, 24.05, 24.05]),
    (ToolbarButton::Minimize, 15, [7.7, 4.7, 16.3, 19.3]),
    (ToolbarButton::Close, 14, [4.6, 4.6, 19.4, 19.4]),
];

/// What the pixels of a box show of an icon's ink over its background.
struct Inked {
    /// The most ink any pixel has: how much of the way from the background to the ink its color is, along the line
    /// between them (0 for the background, 1 for full ink; a stroke that covers part of a pixel blends the two).
    most: f64,
    /// How far that pixel's color is from that line: nothing but the ink and the background is in it.
    off_the_line: f64,
    /// How many pixels are more than half ink.
    inky: usize,
    /// The box around the pixels that are more than a third ink: left, top, right and bottom, the last two one past.
    found: [i32; 4],
}

/// A whole number of pixels, rounded down. Icons are tens of pixels across, far inside `i32`.
// TODO: lay the box out in whole pixels without a float, so no float comes back to `i32` here.
fn floor(value: f64) -> i32 {
    value.floor() as i32
}

/// As `floor`, rounded up.
// TODO: as `floor`.
fn ceil(value: f64) -> i32 {
    value.ceil() as i32
}

/// The ink of color `ink` over the gradient `stops` in the pixels of `drawn` within `wanted`, the box the prototype puts
/// the icon in, and two pixels around it, in `button`'s place.
fn ink_in(drawn: &Drawn, place: &RECT, stops: Stops, ink: [u8; 3], wanted: [f64; 4]) -> Inked {
    let mut inked = Inked {
        most: f64::MIN,
        off_the_line: 0.0,
        inky: 0,
        found: [i32::MAX, i32::MAX, i32::MIN, i32::MIN],
    };
    for y in floor(wanted[1]) - 2..ceil(wanted[3]) + 2 {
        let background = gradient_at(stops, down(place, y));
        let along = |i: usize| f64::from(ink[i]) - f64::from(background[i]);
        let length: f64 = (0..3).map(|i| along(i) * along(i)).sum();
        for x in floor(wanted[0]) - 2..ceil(wanted[2]) + 2 {
            let pixel = drawn.at(x, y);
            let from = |i: usize| f64::from(pixel[i]) - f64::from(background[i]);
            let share = (0..3).map(|i| from(i) * along(i)).sum::<f64>() / length;
            if share > inked.most {
                inked.most = share;
                inked.off_the_line = (0..3)
                    .map(|i| (from(i) - share * along(i)).powi(2))
                    .sum::<f64>()
                    .sqrt();
            }
            if share > 0.35 {
                let [left, top, right, bottom] = inked.found;
                inked.found = [left.min(x), top.min(y), right.max(x + 1), bottom.max(y + 1)];
            }
            if share > 0.5 {
                inked.inky += 1;
            }
        }
    }
    inked
}

#[test]
fn each_icon_is_in_its_ink_centered_in_its_button_at_its_size() {
    let canvas = canvas();
    // Each tool and each mode is lit in one of these, and unlit in the others.
    let scenes = [
        scene(Tool::Dimensions, Some(DimensionsMode::Horizontal)),
        scene(Tool::Bounds, Some(DimensionsMode::Vertical)),
        scene(Tool::Inspect, Some(DimensionsMode::Cross)),
        scene(Tool::Snip, Some(DimensionsMode::Horizontal)),
    ];
    for mode in MODES {
        let t = tokens(mode);
        for dpi in SCALES {
            for toolbar in &scenes {
                let drawn = draw(&canvas, mode, dpi, toolbar);
                let lit = lit_in(toolbar);
                for (button, css_size, extent) in ICONS {
                    let place = drawn.place(button);
                    let is_lit = lit.contains(&button);
                    let (stops, ink) = match (is_lit, is_control(button)) {
                        (true, _) => (t.lit, t.ink_on),
                        (false, true) => (t.small, t.small_ink),
                        (false, false) => (t.button, t.ink),
                    };
                    let context = format!("{button:?} lit {is_lit}: {mode:?} at {dpi} DPI");

                    // Where the icon's view box is: centered in the button, at the prototype's CSS size scaled. (The
                    // drawing puts its corner on a whole pixel, which moves it by half a pixel at most.)
                    let side = f64::from(scaled(css_size, dpi));
                    let unit = side / 24.0;
                    let room_x = f64::from(place.right - place.left) - side;
                    let room_y = f64::from(place.bottom - place.top) - side;
                    let (left, top) = (
                        f64::from(place.left) + room_x / 2.0,
                        f64::from(place.top) + room_y / 2.0,
                    );
                    let wanted = [
                        left + extent[0] * unit,
                        top + extent[1] * unit,
                        left + extent[2] * unit,
                        top + extent[3] * unit,
                    ];

                    let inked = ink_in(&drawn, &place, stops, ink, wanted);
                    assert!(
                        inked.most >= 0.6 && inked.inky >= 8,
                        "the inkiest pixel is {:.2} ink and {} are over half: {context}",
                        inked.most,
                        inked.inky
                    );
                    // And it is the ink's color, over the button's: a stroke blends the two, nothing else.
                    assert!(
                        inked.off_the_line <= 12.0,
                        "the inkiest pixel is {:.1} off the ink-background line: {context}",
                        inked.off_the_line
                    );
                    let [left, top, right, bottom] = inked.found;
                    for (side_name, got, want) in [
                        ("left", f64::from(left), wanted[0]),
                        ("top", f64::from(top), wanted[1]),
                        ("right", f64::from(right), wanted[2]),
                        ("bottom", f64::from(bottom), wanted[3]),
                    ] {
                        assert!(
                            (got - want).abs() <= 1.5,
                            "the icon's {side_name} is at {got}, the prototype puts it at {want:.2}: {context}"
                        );
                    }
                }
            }
        }
    }
}

/// Clears the target to the test's photo, as the start of another frame does.
fn clear_to_photo(target: &ID2D1RenderTarget) {
    let photo = d2d_color(BACKGROUND, 1.0);
    // SAFETY: a drawing call between BeginDraw and EndDraw on a live target.
    unsafe { target.Clear(Some(&photo)) };
}

/// The scenes drawn in order, each over a cleared target as in a window's frames, with the pixels of the last and how many
/// times the toolbar was drawn on its bitmap after each.
fn frames(canvas: &Canvas, mode: Mode, dpi: u32, scenes: &[Toolbar]) -> (Picture, Vec<u32>) {
    let held: Vec<_> = scenes.iter().map(|toolbar| (*toolbar, None)).collect();
    frames_held(canvas, mode, dpi, &held)
}

/// As `frames`, each scene with what the left button is held down on.
fn frames_held(
    canvas: &Canvas,
    mode: Mode,
    dpi: u32,
    scenes: &[(Toolbar, Option<Hit>)],
) -> (Picture, Vec<u32>) {
    let mut fills = Vec::new();
    let picture = canvas.render_in(mode, TOOLBAR_MONITOR, dpi, |overlay, target| {
        for (frame, (toolbar, held)) in scenes.iter().enumerate() {
            if frame > 0 {
                clear_to_photo(target);
            }
            overlay
                .draw_toolbar(target, toolbar, *held)
                .expect("the toolbar");
            fills.push(overlay.toolbar_cache.borrow().fills);
        }
    });
    (picture, fills)
}

#[test]
fn a_second_frame_of_the_same_toolbar_lays_the_bitmap_again_and_draws_the_same_pixels() {
    let canvas = canvas();
    let toolbar = scene(Tool::Inspect, Some(DimensionsMode::Cross));
    for mode in MODES {
        for dpi in SCALES {
            let fresh = draw(&canvas, mode, dpi, &toolbar);
            let (picture, fills) = frames(&canvas, mode, dpi, &[toolbar, toolbar, toolbar]);
            // Drawn on the bitmap once, laid three times.
            assert_eq!(fills, [1, 1, 1], "{mode:?} at {dpi} DPI");
            assert!(
                picture.pixels == fresh.picture.pixels,
                "a cached frame differs from a fresh one: {mode:?} at {dpi} DPI"
            );
        }
    }
}

#[test]
fn lighting_another_tool_or_mode_draws_the_bitmap_again_and_going_back_draws_the_first_pixels() {
    let canvas = canvas();
    let first = scene(Tool::Dimensions, Some(DimensionsMode::Horizontal));
    let others = [
        scene(Tool::Dimensions, Some(DimensionsMode::Cross)),
        scene(Tool::Snip, None),
        scene(Tool::Bounds, None),
    ];
    for mode in MODES {
        for dpi in [96, 144] {
            let fresh_first = draw(&canvas, mode, dpi, &first);
            for other in &others {
                let fresh_other = draw(&canvas, mode, dpi, other);
                // Another scene over the same bitmap is what a fresh one is: nothing is left of the first.
                let (picture, fills) = frames(&canvas, mode, dpi, &[first, *other]);
                assert_eq!(fills, [1, 2], "{other:?}: {mode:?} at {dpi} DPI");
                assert!(
                    picture.pixels == fresh_other.picture.pixels,
                    "{other:?} after {first:?} is not drawn as it is alone: {mode:?} at {dpi} DPI"
                );
                // And back to the first, the bitmap drawn a third time.
                let (picture, fills) = frames(&canvas, mode, dpi, &[first, *other, first]);
                assert_eq!(fills, [1, 2, 3]);
                assert!(
                    picture.pixels == fresh_first.picture.pixels,
                    "{first:?} after {other:?} is not drawn as it is alone: {mode:?} at {dpi} DPI"
                );
            }
        }
    }
}

#[test]
fn the_cached_toolbar_is_where_and_what_it_is_when_every_layer_is_drawn_straight_over_the_photo() {
    let canvas = canvas();
    let toolbar = scene(Tool::Inspect, Some(DimensionsMode::Vertical));
    for mode in MODES {
        for dpi in SCALES {
            let cached = draw(&canvas, mode, dpi, &toolbar);
            let direct = canvas.render_in(mode, TOOLBAR_MONITOR, dpi, |overlay, target| {
                let layout = toolbar_layout(dpi);
                let corner = Point {
                    x: TOOLBAR_MONITOR.left,
                    y: TOOLBAR_MONITOR.top,
                };
                overlay
                    .paint_toolbar(target, &layout, &toolbar, None, corner)
                    .expect("the toolbar");
            });
            // How far apart the two are at their worst, and on average over the toolbar's own rectangle.
            let (mut most, mut total, mut count) = (0, 0u64, 0u64);
            let toolbar_area = toolbar_layout(dpi).toolbar;
            for y in toolbar_area.top..toolbar_area.bottom {
                for x in toolbar_area.left..toolbar_area.right {
                    let (a, b) = (at(&cached.picture, x, y), at(&direct, x, y));
                    let worst = a
                        .iter()
                        .zip(&b)
                        .map(|(&a, &b)| a.abs_diff(b))
                        .max()
                        .unwrap_or(0);
                    most = most.max(worst);
                    total += u64::from(worst);
                    count += 1;
                }
            }
            // Not the same pixels, and not meant to be: drawn straight over the photo, a faint layer rounds against the
            // photo's 8 bits, and one that would change a channel by less than half a level leaves it as it was, so a
            // shadow comes out too light in the channels that are small, and tinted (the photo's green and blue stay
            // put while its red darkens). On the transparent bitmap the layers add up in the alpha channel first, so the
            // shadows keep their proportions. Hence the tolerance, which a bitmap a pixel out of place would still break
            // at every edge: a panel or an icon is tens of levels from what is beside it.
            let mean = f64::from(u32::try_from(total).expect("a sum of levels"))
                / f64::from(u32::try_from(count).expect("a count of pixels"));
            assert!(
                most <= 40,
                "{most} levels apart at most: {mode:?} at {dpi} DPI"
            );
            assert!(
                mean <= 5.0,
                "{mean:.2} levels apart on average: {mode:?} at {dpi} DPI"
            );
        }
    }
}

#[test]
fn the_bitmap_has_room_for_the_shadows_and_not_much_more() {
    for mode in MODES {
        let (sides, above, below) = shadow_reach(mode);
        for dpi in SCALES {
            let place = toolbar_layout(dpi);
            let margins = toolbar_paint::reach(toolbar_palette::palette(mode), &place, dpi);
            // The prototype's reach is three deviations out; the layers stop where they add under half a level, a few
            // pixels short of that, so each side is within ten (scaled) pixels under it and, with the groups' own
            // sizes scaled one at a time, a few over.
            for (name, got, bound) in [
                ("left", margins.left, sides),
                ("right", margins.right, sides),
                ("top", margins.top, above),
                ("bottom", margins.bottom, below),
            ] {
                let bound = scaled(bound, dpi);
                assert!(
                    (bound - scaled(10, dpi)..=bound + 3).contains(&got),
                    "the {name} margin is {got}, the prototype's shadows reach {bound}: {mode:?} at {dpi} DPI"
                );
            }
        }
    }
}

/// Times whole frames (BeginDraw to EndDraw, which is where Direct2D rasterizes) on the WIC bitmap: an empty one, one
/// that draws the toolbar straight over the photo as every frame would without the cache, the first with the cache, which
/// makes the bitmap, one after another tool is lit, which draws it again, and one that only lays it. Run alone and in
/// release, where the numbers mean something:
/// `cargo test --release -p screengauge toolbar_draw_times -- --ignored --nocapture --test-threads=1`.
#[test]
#[ignore = "a timing, for reading"]
fn toolbar_draw_times() {
    use std::time::{Duration, Instant};

    let canvas = canvas();
    let median = |mut times: Vec<Duration>| {
        times.sort();
        times[times.len() / 2]
    };
    for mode in MODES {
        for dpi in [96, 168] {
            let scale = Scale::from_dpi(dpi).expect("a scale");
            let place = toolbar_layout(dpi);
            let corner = Point {
                x: TOOLBAR_MONITOR.left,
                y: TOOLBAR_MONITOR.top,
            };
            let (one, other) = (
                scene(Tool::Dimensions, Some(DimensionsMode::Horizontal)),
                scene(Tool::Snip, None),
            );
            let mut times = [(); 5].map(|()| Vec::new());
            for _ in 0..15 {
                let overlay =
                    Overlay::new(&canvas.target, &canvas.fonts, TOOLBAR_MONITOR, scale, mode)
                        .expect("the overlay");
                let mut frame = |slot: usize, draw: &dyn Fn(&Overlay, &ID2D1RenderTarget)| {
                    let start = Instant::now();
                    // SAFETY: drawing calls between BeginDraw and EndDraw on a live target.
                    unsafe { canvas.target.BeginDraw() };
                    clear_to_photo(&canvas.target);
                    draw(&overlay, &canvas.target);
                    // SAFETY: ends the BeginDraw above.
                    unsafe { canvas.target.EndDraw(None, None) }.expect("the frame ends");
                    times[slot].push(start.elapsed());
                };
                let show = |toolbar: &Toolbar| {
                    let toolbar = *toolbar;
                    move |overlay: &Overlay, target: &ID2D1RenderTarget| {
                        overlay
                            .draw_toolbar(target, &toolbar, None)
                            .expect("the toolbar");
                    }
                };
                frame(0, &|_, _| {});
                frame(1, &|overlay, target| {
                    overlay
                        .paint_toolbar(target, &place, &one, None, corner)
                        .expect("the toolbar");
                });
                frame(2, &show(&one));
                frame(3, &show(&other));
                frame(4, &show(&other));
            }
            let [empty, straight, first, changed, laid] = times.map(median);
            println!(
                "{mode:?} at {dpi} DPI, a frame: empty {empty:?}, toolbar straight over the photo {straight:?}, first with the cache {first:?}, another tool lit {changed:?}, cached {laid:?}"
            );
            assert!(
                laid < straight,
                "laying the bitmap is not cheaper than drawing it"
            );
        }
    }
}

#[test]
fn the_toolbar_draws_on_a_windows_own_render_target_which_ignores_alpha() {
    use windows::Win32::Graphics::Direct2D::Common::{
        D2D_SIZE_U, D2D1_ALPHA_MODE_IGNORE, D2D1_PIXEL_FORMAT,
    };
    use windows::Win32::Graphics::Direct2D::{
        D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_FEATURE_LEVEL_DEFAULT,
        D2D1_HWND_RENDER_TARGET_PROPERTIES, D2D1_PRESENT_OPTIONS_NONE,
        D2D1_RENDER_TARGET_PROPERTIES, D2D1_RENDER_TARGET_TYPE_DEFAULT,
        D2D1_RENDER_TARGET_USAGE_NONE, D2D1CreateFactory, ID2D1Factory,
    };
    use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, WINDOW_EX_STYLE, WS_POPUP,
    };
    use windows::core::w;

    // The frozen screen's windows draw on a target made like this one: hardware when there is, the pixel format's alpha
    // ignored. The bitmap the toolbar is kept on asks for premultiplied alpha itself, and must still be laid over that.
    // SAFETY: "STATIC" is a window class of Windows' own; the window is never shown, and destroyed below.
    let hwnd = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("STATIC"),
            w!("ScreenGauge toolbar test"),
            WS_POPUP,
            0,
            0,
            1280,
            440,
            None,
            None,
            None,
            None,
        )
    }
    .expect("a window");
    // SAFETY: a single-threaded factory used only on this thread.
    let factory: ID2D1Factory =
        unsafe { D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None) }
            .expect("the Direct2D factory");
    let properties = D2D1_RENDER_TARGET_PROPERTIES {
        r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
        pixelFormat: D2D1_PIXEL_FORMAT {
            format: DXGI_FORMAT_B8G8R8A8_UNORM,
            alphaMode: D2D1_ALPHA_MODE_IGNORE,
        },
        dpiX: 96.0,
        dpiY: 96.0,
        usage: D2D1_RENDER_TARGET_USAGE_NONE,
        minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
    };
    let window_properties = D2D1_HWND_RENDER_TARGET_PROPERTIES {
        hwnd,
        pixelSize: D2D_SIZE_U {
            width: 1280,
            height: 440,
        },
        presentOptions: D2D1_PRESENT_OPTIONS_NONE,
    };
    // SAFETY: both property structs outlive the call.
    let target = unsafe { factory.CreateHwndRenderTarget(&properties, &window_properties) }
        .expect("a window's render target");
    let fonts = Fonts::new().expect("the fonts");
    for mode in MODES {
        for dpi in [96, 168] {
            let scale = Scale::from_dpi(dpi).expect("a scale");
            let overlay =
                Overlay::new(&target, &fonts, TOOLBAR_MONITOR, scale, mode).expect("the overlay");
            // SAFETY: drawing calls between BeginDraw and EndDraw on a live target.
            unsafe {
                target.BeginDraw();
                clear_to_photo(&target);
            }
            // Drawn on the bitmap, laid again from it, drawn on it again for another tool, and laid.
            for toolbar in [
                scene(Tool::Dimensions, Some(DimensionsMode::Cross)),
                scene(Tool::Dimensions, Some(DimensionsMode::Cross)),
                scene(Tool::Snip, None),
                scene(Tool::Snip, None),
            ] {
                overlay
                    .draw_toolbar(&target, &toolbar, None)
                    .expect("the toolbar");
            }
            // SAFETY: ends the BeginDraw above.
            unsafe { target.EndDraw(None, None) }.expect("the frame ends");
            assert_eq!(
                overlay.toolbar_cache.borrow().fills,
                2,
                "{mode:?} at {dpi} DPI"
            );
        }
    }
    drop(target);
    // SAFETY: our own window, destroyed once, after what drew on it.
    unsafe { DestroyWindow(hwnd) }.expect("the window is destroyed");
}

/// How much of the way from the gradient `stops`' color at row `y` to `ink` the pixel (`x`, `y`) of `drawn` is, in
/// `place`: 0 for the button's own color, 1 for full ink.
fn ink_share(drawn: &Drawn, place: &RECT, stops: Stops, ink: [u8; 3], x: i32, y: i32) -> f64 {
    let background = gradient_at(stops, down(place, y));
    let pixel = drawn.at(x, y);
    let along = |i: usize| f64::from(ink[i]) - f64::from(background[i]);
    let from = |i: usize| f64::from(pixel[i]) - f64::from(background[i]);
    let length: f64 = (0..3).map(|i| along(i) * along(i)).sum();
    (0..3).map(|i| from(i) * along(i)).sum::<f64>() / length
}

/// A point of an icon in its view box, and whether a stroke passes through it.
type Probe = (f64, f64, bool);

/// Points of each icon, in its view box, that a stroke passes through (`true`) and points well clear of every stroke
/// (`false`), taken from the prototype's SVGs by hand: the middle of a side, the tip of a tooth, the middle of a slanted
/// line. Where a path is closed, one of its points is on the side only the closepath draws; where an arc bulges, one is
/// at the top of the bulge.
const PROBES: [(ToolbarButton, &[Probe]); 10] = [
    (
        ToolbarButton::Tool(Tool::Dimensions),
        // The frame's top, left and bottom sides, and the first and second tick.
        &[
            (12.0, 8.0, true),
            (2.5, 12.0, true),
            (12.0, 16.0, true),
            (6.5, 9.5, true),
            (10.5, 11.0, true),
        ],
    ),
    (
        ToolbarButton::Tool(Tool::Bounds),
        // The four corner brackets' sides, the small square's top, and the room inside it and between them.
        &[
            (4.0, 6.5, true),
            (6.5, 4.0, true),
            (20.0, 6.5, true),
            (17.5, 4.0, true),
            (4.0, 17.5, true),
            (6.5, 20.0, true),
            (20.0, 17.5, true),
            (17.5, 20.0, true),
            (12.0, 9.0, true),
            (12.0, 12.0, false),
        ],
    ),
    (
        ToolbarButton::Tool(Tool::Inspect),
        // The glass at its top, right, left and bottom, the handle's middle, and the room inside the glass.
        &[
            (11.0, 5.0, true),
            (17.0, 11.0, true),
            (5.0, 11.0, true),
            (11.0, 17.0, true),
            (17.75, 17.75, true),
            (11.0, 11.0, false),
        ],
    ),
    (
        ToolbarButton::Tool(Tool::Snip),
        // The camera's left side (only the closepath draws it), bottom, right side, top and a slope, the lens' top, and
        // the room inside the lens.
        &[
            (4.0, 13.5, true),
            (12.0, 19.0, true),
            (20.0, 13.5, true),
            (12.0, 5.0, true),
            (8.0, 6.5, true),
            (12.0, 9.5, true),
            (12.0, 13.0, false),
        ],
    ),
    (
        ToolbarButton::Mode(DimensionsMode::Horizontal),
        // The shaft, the arrowheads' tips and a side of each, and the room above and below the shaft.
        &[
            (12.0, 12.0, true),
            (4.0, 12.0, true),
            (6.0, 10.0, true),
            (18.0, 14.0, true),
            (12.0, 8.0, false),
            (12.0, 16.0, false),
        ],
    ),
    (
        ToolbarButton::Mode(DimensionsMode::Vertical),
        &[
            (12.0, 12.0, true),
            (12.0, 4.0, true),
            (10.0, 6.0, true),
            (14.0, 18.0, true),
            (8.0, 12.0, false),
            (16.0, 12.0, false),
        ],
    ),
    (
        ToolbarButton::Mode(DimensionsMode::Cross),
        &[
            (12.0, 12.0, true),
            (10.5, 4.5, true),
            (10.5, 19.5, true),
            (4.5, 10.5, true),
            (19.5, 10.5, true),
            (6.5, 6.5, false),
            (17.5, 17.5, false),
        ],
    ),
    (
        ToolbarButton::Settings,
        // The tips of the gear's four teeth, which are arcs, the top of its hub, and the room in the middle of the hub
        // and between the hub and the teeth's ring.
        &[
            (12.0, 1.0, true),
            (12.0, 23.0, true),
            (1.0, 12.0, true),
            (23.0, 12.0, true),
            (12.0, 9.0, true),
            (12.0, 12.0, false),
            (12.0, 6.5, false),
        ],
    ),
    (
        ToolbarButton::Minimize,
        &[
            (12.0, 9.0, true),
            (12.0, 15.0, true),
            (9.0, 12.0, true),
            (14.0, 12.0, false),
        ],
    ),
    (
        ToolbarButton::Close,
        &[
            (12.0, 12.0, true),
            (8.0, 8.0, true),
            (16.0, 16.0, true),
            (16.0, 8.0, true),
            (8.0, 16.0, true),
            (12.0, 7.0, false),
            (7.0, 12.0, false),
        ],
    ),
];

#[test]
fn each_icons_strokes_are_where_its_svg_draws_them() {
    let canvas = canvas();
    let scenes = [
        scene(Tool::Dimensions, Some(DimensionsMode::Horizontal)),
        scene(Tool::Snip, Some(DimensionsMode::Cross)),
    ];
    for mode in MODES {
        let t = tokens(mode);
        for dpi in SCALES {
            for toolbar in &scenes {
                let drawn = draw(&canvas, mode, dpi, toolbar);
                let lit = lit_in(toolbar);
                for ((button, css_size, _), (probed, probes)) in ICONS.iter().zip(PROBES) {
                    assert_eq!(*button, probed, "the probes are in the icons' order");
                    let place = drawn.place(*button);
                    let (stops, ink) = match (lit.contains(button), is_control(*button)) {
                        (true, _) => (t.lit, t.ink_on),
                        (false, true) => (t.small, t.small_ink),
                        (false, false) => (t.button, t.ink),
                    };
                    let side = f64::from(scaled(*css_size, dpi));
                    let unit = side / 24.0;
                    let left =
                        f64::from(place.left) + (f64::from(place.right - place.left) - side) / 2.0;
                    let top =
                        f64::from(place.top) + (f64::from(place.bottom - place.top) - side) / 2.0;
                    for &(vx, vy, stroked) in probes {
                        let (x, y) = (floor(left + vx * unit), floor(top + vy * unit));
                        let context = format!(
                            "{button:?} at view box ({vx}, {vy}), pixel ({x}, {y}): {mode:?} at {dpi} DPI"
                        );
                        if stroked {
                            // The pixel, or one beside it: the icon's corner is laid on a whole pixel, half a pixel from
                            // where this puts it.
                            let share = (-1..=1)
                                .flat_map(|dy| (-1..=1).map(move |dx| (x + dx, y + dy)))
                                .map(|(x, y)| ink_share(&drawn, &place, stops, ink, x, y))
                                .fold(f64::MIN, f64::max);
                            assert!(share >= 0.45, "no stroke there, {share:.2} ink: {context}");
                        } else {
                            let share = ink_share(&drawn, &place, stops, ink, x, y);
                            assert!(
                                share <= 0.3,
                                "ink where there is none, {share:.2}: {context}"
                            );
                        }
                    }
                }
            }
        }
    }
}

// The toolbar reacting to the mouse, and minimized (WPC-311).

/// `toolbar` with the cursor on `button`.
fn hovering(toolbar: Toolbar, button: ToolbarButton) -> Toolbar {
    Toolbar {
        hovered: Some(button),
        ..toolbar
    }
}

/// `toolbar` minimized.
fn minimized(toolbar: Toolbar) -> Toolbar {
    Toolbar {
        minimized: true,
        ..toolbar
    }
}

/// `toolbar` drawn over `BACKGROUND` with the left button held down on `held`.
fn draw_held(canvas: &Canvas, mode: Mode, dpi: u32, toolbar: &Toolbar, held: Option<Hit>) -> Drawn {
    let picture = canvas.render_in(mode, TOOLBAR_MONITOR, dpi, |overlay, target| {
        overlay
            .draw_toolbar(target, toolbar, held)
            .expect("the toolbar");
    });
    Drawn {
        picture,
        layout: toolbar_layout(dpi),
        dpi,
    }
}

/// Where the prototype puts the icon of `css_size` covering `extent` of its view box, centered in `place`, at `dpi`.
fn icon_box(place: &RECT, css_size: u32, extent: [f64; 4], dpi: u32) -> [f64; 4] {
    let side = f64::from(scaled(css_size, dpi));
    let unit = side / 24.0;
    let left = f64::from(place.left) + (f64::from(place.right - place.left) - side) / 2.0;
    let top = f64::from(place.top) + (f64::from(place.bottom - place.top) - side) / 2.0;
    [
        left + extent[0] * unit,
        top + extent[1] * unit,
        left + extent[2] * unit,
        top + extent[3] * unit,
    ]
}

/// The icon of `button` (one of `ICONS`) in `drawn`, in `place`, over `stops` in `ink`: inked, in that ink alone, and where
/// the prototype puts it, within a pixel and a half.
fn assert_icon(
    drawn: &Drawn,
    button: ToolbarButton,
    place: &RECT,
    stops: Stops,
    ink: [u8; 3],
    context: &str,
) {
    let (_, css_size, extent) = ICONS
        .into_iter()
        .find(|(each, _, _)| *each == button)
        .expect("an icon of the table");
    let wanted = icon_box(place, css_size, extent, drawn.dpi);
    let inked = ink_in(drawn, place, stops, ink, wanted);
    assert!(
        inked.most >= 0.6 && inked.inky >= 8,
        "the inkiest pixel is {:.2} ink and {} are over half: {context}",
        inked.most,
        inked.inky
    );
    assert!(
        inked.off_the_line <= 12.0,
        "the inkiest pixel is {:.1} off the ink-background line: {context}",
        inked.off_the_line
    );
    let [left, top, right, bottom] = inked.found;
    for (got, want) in [
        (left, wanted[0]),
        (top, wanted[1]),
        (right, wanted[2]),
        (bottom, wanted[3]),
    ] {
        assert!(
            (f64::from(got) - want).abs() <= 1.5,
            "the icon is at {:?}, the prototype puts it at {wanted:.2?}: {context}",
            inked.found
        );
    }
}

#[test]
fn a_tools_button_rises_two_pixels_with_the_cursor_on_it() {
    let canvas = canvas();
    let toolbar = scene(Tool::Dimensions, None);
    for mode in MODES {
        for dpi in SCALES {
            let rest = draw(&canvas, mode, dpi, &toolbar);
            let lift = scaled(2, dpi);
            let radius = scaled(13, dpi);
            // The lit button and a plain one.
            for button in [
                ToolbarButton::Tool(Tool::Dimensions),
                ToolbarButton::Tool(Tool::Bounds),
            ] {
                let hovered = draw(&canvas, mode, dpi, &hovering(toolbar, button));
                let place = rest.place(button);
                // The whole button, clear of its rounded corners, is where it was, two (scaled) pixels higher: its
                // gradient runs down it, so a button that rose by another amount, or not at all, is a different color.
                for y in place.top + 1..place.bottom - 1 {
                    for x in place.left + radius..place.right - radius {
                        assert!(
                            near(hovered.at(x, y - lift), rest.at(x, y), 1),
                            "{button:?} at ({x}, {y}) is {:?} risen and {:?} at rest: {mode:?} at {dpi} DPI",
                            hovered.at(x, y - lift),
                            rest.at(x, y)
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn a_held_tools_button_shrinks_and_sinks_where_it_is_and_only_while_the_cursor_is_on_it() {
    let canvas = canvas();
    let button = ToolbarButton::Tool(Tool::Inspect);
    let toolbar = scene(Tool::Snip, None);
    let held = Some(Hit::Button(button));
    for mode in MODES {
        for dpi in [96, 168] {
            let rest = draw(&canvas, mode, dpi, &toolbar);
            let hovered = draw(&canvas, mode, dpi, &hovering(toolbar, button));
            let pressed = draw_held(&canvas, mode, dpi, &hovering(toolbar, button), held);
            let context = format!("{mode:?} at {dpi} DPI");
            assert!(
                pressed.picture.pixels != hovered.picture.pixels,
                "{context}"
            );
            // What changed is the button and its shadow, nothing around them.
            let place = rest.place(button);
            let reach = scaled(16, dpi);
            let mut on_the_button = 0;
            let window = bounds();
            for y in window.top..window.bottom {
                for x in window.left..window.right {
                    if near(pressed.at(x, y), rest.at(x, y), 1) {
                        continue;
                    }
                    let near_it = x >= place.left - reach
                        && x < place.right + reach
                        && y >= place.top - reach
                        && y < place.bottom + reach;
                    assert!(
                        near_it,
                        "({x}, {y}) changed, away from {place:?}: {context}"
                    );
                    on_the_button += 1;
                }
            }
            assert!(on_the_button > 0, "nothing changed: {context}");
            // Held but moved off it, it looks at rest, as a Windows button does.
            let off = draw_held(&canvas, mode, dpi, &toolbar, held);
            assert!(off.picture.pixels == rest.picture.pixels, "{context}");
        }
    }
}

#[test]
fn with_the_cursor_on_them_the_gear_and_minimize_brighten_and_close_turns_red() {
    let canvas = canvas();
    let toolbar = scene(Tool::Bounds, None);
    for mode in MODES {
        let t = tokens(mode);
        for dpi in SCALES {
            for button in [ToolbarButton::Settings, ToolbarButton::Minimize] {
                let drawn = draw(&canvas, mode, dpi, &hovering(toolbar, button));
                let place = drawn.place(button);
                let context = format!("{button:?}: {mode:?} at {dpi} DPI");
                // Its own gradient, the brighter ink on it.
                let (x, y) = sample_point(&place, dpi);
                assert!(
                    near(drawn.at(x, y), gradient_at(t.small, down(&place, y)), 4),
                    "{context}"
                );
                assert_icon(&drawn, button, &place, t.small, t.small_ink_hover, &context);
            }
            let drawn = draw(&canvas, mode, dpi, &hovering(toolbar, ToolbarButton::Close));
            let place = drawn.place(ToolbarButton::Close);
            let context = format!("Close: {mode:?} at {dpi} DPI");
            let (x, y) = sample_point(&place, dpi);
            let red = gradient_at(CLOSE_RED, down(&place, y));
            assert!(
                near(drawn.at(x, y), red, 4),
                "{:?}, wanted {red:?}: {context}",
                drawn.at(x, y)
            );
            assert_icon(
                &drawn,
                ToolbarButton::Close,
                &place,
                CLOSE_RED,
                [255, 255, 255],
                &context,
            );
        }
    }
}

#[test]
fn h_v_and_c_do_not_react_to_the_cursor() {
    let canvas = canvas();
    let toolbar = scene(Tool::Dimensions, Some(DimensionsMode::Cross));
    for mode in MODES {
        let rest = draw(&canvas, mode, 120, &toolbar);
        for each in [
            DimensionsMode::Horizontal,
            DimensionsMode::Vertical,
            DimensionsMode::Cross,
        ] {
            let button = ToolbarButton::Mode(each);
            let hovered = draw(&canvas, mode, 120, &hovering(toolbar, button));
            let held = draw_held(
                &canvas,
                mode,
                120,
                &hovering(toolbar, button),
                Some(Hit::Button(button)),
            );
            assert!(
                hovered.picture.pixels == rest.picture.pixels,
                "{each:?}: {mode:?}"
            );
            assert!(
                held.picture.pixels == rest.picture.pixels,
                "{each:?}: {mode:?}"
            );
        }
    }
}

#[test]
fn the_minimized_toolbar_is_its_lit_button_alone_with_the_tools_icon_and_a_small_arrow() {
    let canvas = canvas();
    for mode in MODES {
        let t = tokens(mode);
        for dpi in SCALES {
            for active in [Tool::Inspect, Tool::Snip] {
                let toolbar = minimized(scene(active, None));
                let drawn = draw(&canvas, mode, dpi, &toolbar);
                let place = drawn.layout.minimized;
                let context = format!("{active:?}: {mode:?} at {dpi} DPI");
                assert_eq!(
                    (place.left, place.top, place.right - place.left),
                    (
                        drawn.layout.toolbar.left,
                        drawn.layout.toolbar.top,
                        scaled(52, dpi)
                    ),
                    "{context}"
                );
                // Cyan, with the active tool's icon in the lit ink in its middle.
                let (x, y) = sample_point(&place, dpi);
                assert!(
                    near(drawn.at(x, y), gradient_at(t.lit, down(&place, y)), 4),
                    "{context}"
                );
                assert_icon(
                    &drawn,
                    ToolbarButton::Tool(active),
                    &place,
                    t.lit,
                    t.ink_on,
                    &context,
                );
                // The arrow's box, 14 pixels 3 in from the right and bottom sides: the cyan darkened, a white arrow in it.
                let side = scaled(14, dpi);
                let inset = scaled(3, dpi);
                let (right, bottom) = (place.right - inset, place.bottom - inset);
                let (left, top) = (right - side, bottom - side);
                let y = top + side / 2;
                let shaded = blend([0, 0, 0], 0.35, gradient_at(t.lit, down(&place, y)));
                assert!(
                    near(drawn.at(left + scaled(2, dpi), y), shaded, 6),
                    "{:?} in the arrow's box, wanted {shaded:?}: {context}",
                    drawn.at(left + scaled(2, dpi), y)
                );
                let whitest = (top..bottom)
                    .flat_map(|y| (left..right).map(move |x| (x, y)))
                    .map(|(x, y)| drawn.at(x, y).into_iter().min().unwrap_or(0))
                    .max()
                    .unwrap_or(0);
                assert!(
                    whitest >= 200,
                    "the whitest pixel of the arrow's box is {whitest}: {context}"
                );
                // And nothing else of the toolbar: where Snip and close were is the photo.
                for gone in [ToolbarButton::Tool(Tool::Snip), ToolbarButton::Close] {
                    let was = drawn.place(gone);
                    let (x, y) = ((was.left + was.right) / 2, (was.top + was.bottom) / 2);
                    assert!(
                        near(drawn.at(x, y), BACKGROUND, 1),
                        "{gone:?} at ({x}, {y}): {context}"
                    );
                }
                // The cursor on it changes nothing.
                let hovered = draw(
                    &canvas,
                    mode,
                    dpi,
                    &hovering(toolbar, ToolbarButton::Restore),
                );
                assert!(hovered.picture.pixels == drawn.picture.pixels, "{context}");
            }
        }
    }
}

#[test]
fn hovering_holding_or_minimizing_draws_the_bitmap_again_and_back_draws_the_first_pixels() {
    let canvas = canvas();
    let rest = scene(Tool::Dimensions, Some(DimensionsMode::Cross));
    let gear = hovering(rest, ToolbarButton::Settings);
    let held = Some(Hit::Button(ToolbarButton::Settings));
    for mode in MODES {
        let fresh = draw(&canvas, mode, 144, &rest);
        let (picture, fills) = frames_held(
            &canvas,
            mode,
            144,
            &[
                (rest, None),
                (gear, None),
                (gear, None),
                (gear, held),
                (minimized(rest), None),
                (rest, None),
            ],
        );
        assert_eq!(fills, [1, 2, 2, 3, 4, 5], "{mode:?}");
        assert!(picture.pixels == fresh.picture.pixels, "{mode:?}");
    }
}

#[test]
fn the_bitmap_cuts_off_no_shadow_of_a_button_risen_held_red_or_minimized() {
    let canvas = canvas();
    let lit = scene(Tool::Dimensions, Some(DimensionsMode::Horizontal));
    let dimensions = ToolbarButton::Tool(Tool::Dimensions);
    let scenes = [
        (hovering(lit, dimensions), None),
        (hovering(lit, dimensions), Some(Hit::Button(dimensions))),
        (hovering(lit, ToolbarButton::Close), None),
        (
            hovering(lit, ToolbarButton::Close),
            Some(Hit::Button(ToolbarButton::Close)),
        ),
        (minimized(lit), None),
    ];
    for mode in MODES {
        for dpi in SCALES {
            let layout = toolbar_layout(dpi);
            let margins = toolbar_paint::reach(toolbar_palette::palette(mode), &layout, dpi);
            let bitmap = RECT {
                left: layout.toolbar.left - margins.left,
                top: layout.toolbar.top - margins.top,
                right: layout.toolbar.right + margins.right,
                bottom: layout.toolbar.bottom + margins.bottom,
            };
            let window = bounds();
            for (toolbar, held) in &scenes {
                let drawn = draw_held(&canvas, mode, dpi, toolbar, *held);
                // The bitmap's outermost pixels that are in the window: every shadow has faded out before them, or the
                // bitmap would show a hard edge there.
                let rows = (bitmap.top.max(window.top)..bitmap.bottom)
                    .flat_map(|y| [(bitmap.left, y), (bitmap.right - 1, y)]);
                let bottom = (bitmap.left..bitmap.right).map(|x| (x, bitmap.bottom - 1));
                let top = (bitmap.top >= window.top)
                    .then_some(bitmap.left..bitmap.right)
                    .into_iter()
                    .flatten()
                    .map(|x| (x, bitmap.top));
                for (x, y) in rows.chain(bottom).chain(top) {
                    assert!(
                        near(drawn.at(x, y), BACKGROUND, 3),
                        "({x}, {y}) on the bitmap's edge is {:?}: {toolbar:?} held {held:?}, {mode:?} at {dpi} DPI",
                        drawn.at(x, y)
                    );
                }
            }
        }
    }
}

#[test]
fn a_held_small_button_shrinks_around_its_middle_where_it_is() {
    let canvas = canvas();
    let toolbar = scene(Tool::Snip, None);
    for mode in MODES {
        for dpi in [96, 168] {
            for button in [ToolbarButton::Settings, ToolbarButton::Close] {
                let hovered = draw(&canvas, mode, dpi, &hovering(toolbar, button));
                let pressed = draw_held(
                    &canvas,
                    mode,
                    dpi,
                    &hovering(toolbar, button),
                    Some(Hit::Button(button)),
                );
                let context = format!("{button:?}: {mode:?} at {dpi} DPI");
                let place = hovered.place(button);
                // Shrunk to 0.92 around its middle, it neither rises nor sinks: its middle row, left of the icon, is the
                // same color, and its top edge is no longer the button's.
                let (x, y) = (
                    (place.left + place.right) / 2,
                    (place.top + place.bottom) / 2,
                );
                let side = place.bottom - place.top;
                let edge_row = place.top + scaled(1, dpi);
                assert!(
                    near(
                        pressed.at(x - side * 3 / 8, y),
                        hovered.at(x - side * 3 / 8, y),
                        6
                    ),
                    "{context}"
                );
                assert!(
                    !near(pressed.at(x, edge_row), hovered.at(x, edge_row), 6),
                    "the top edge at ({x}, {edge_row}) is {:?} pressed and {:?} hovered: {context}",
                    pressed.at(x, edge_row),
                    hovered.at(x, edge_row)
                );
                // And nothing changed away from it and its shadow.
                let reach = scaled(12, dpi);
                let window = bounds();
                for y in window.top..window.bottom {
                    for x in window.left..window.right {
                        let changed = !near(pressed.at(x, y), hovered.at(x, y), 1);
                        let near_it = x >= place.left - reach
                            && x < place.right + reach
                            && y >= place.top - reach
                            && y < place.bottom + reach;
                        assert!(!changed || near_it, "({x}, {y}) changed: {context}");
                    }
                }
            }
        }
    }
}
