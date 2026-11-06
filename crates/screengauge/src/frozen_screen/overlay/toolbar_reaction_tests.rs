//! Render tests of the toolbar reacting to the mouse, and of the minimized toolbar (WPC-311), on a WIC bitmap like the
//! rest of `toolbar_tests`, whose prototype values and helpers they use.

use screengauge_core::settings::{DimensionsMode, Tool};

use super::geometry::scaled;
use super::panel_tests::MODES;
use super::tests::{BACKGROUND, Canvas, SCALES, near};
use super::toolbar_paint;
use super::toolbar_tests::{
    CLOSE_RED, Drawn, ICONS, Stops, TOOLBAR_MONITOR, blend, bounds, canvas, down, draw,
    frames_held, gradient_at, ink_in, placed, sample_point, scene, tokens, toolbar_layout,
};
use super::*;
use crate::relief::palette as toolbar_palette;
use screengauge_core::session::toolbar::Hit;
use screengauge_core::session::{Toolbar, ToolbarButton};

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
            .draw_toolbar(target, &placed(toolbar, dpi), held)
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
