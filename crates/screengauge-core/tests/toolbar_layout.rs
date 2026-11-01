//! Layer 1 tests of where the toolbar is (v1 spec WPC-296, Toolbar; WPC-310 and WPC-311): its size and place at each
//! scale, each button's place, the minimized toolbar's button, and what a point is on. The sizes are the prototype's
//! (variant A of WPC-279: `#toolbar`, `.grp`, `.tbtn`, `.mbtn`, `.sbtn` and `#tbMini`), written out here a second time.

use screengauge_core::session::toolbar::{BUTTONS, Hit, Layout, layout, scaled};
use screengauge_core::session::{Point, Rect, Scale, ToolbarButton};
use screengauge_core::settings::{DimensionsMode, Tool};

/// A 4K monitor right of the main one, as on the owner's desk.
const MONITOR: Rect = Rect {
    left: 1920,
    top: 0,
    width: 3840,
    height: 2160,
};

/// Every scale Windows offers.
const DPIS: [u32; 10] = [96, 120, 144, 168, 192, 216, 240, 288, 336, 384];

fn at(dpi: u32) -> Layout {
    layout(MONITOR, Scale::from_dpi(dpi).expect("a real scale"))
}

/// The column one past `rect`'s right side.
fn end(rect: Rect) -> i32 {
    rect.right() + 1
}

/// The row one past `rect`'s bottom side.
fn below(rect: Rect) -> i32 {
    rect.bottom() + 1
}

/// `logical` pixels at `dpi`, as a coordinate.
fn s(logical: u32, dpi: u32) -> i32 {
    i32::try_from(scaled(logical, dpi)).expect("a small size")
}

fn size(rect: Rect) -> (u32, u32) {
    (rect.width, rect.height)
}

fn place(layout: &Layout, button: ToolbarButton) -> Rect {
    layout
        .buttons
        .iter()
        .find(|(each, _)| *each == button)
        .map(|&(_, place)| place)
        .expect("every button has a place")
}

fn middle(rect: Rect) -> Point {
    Point {
        x: rect.left + (end(rect) - rect.left) / 2,
        y: rect.top + (below(rect) - rect.top) / 2,
    }
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
fn at_100_percent_the_toolbar_is_the_prototypes_size_at_the_top_center() {
    let layout = at(96);
    // 234 + 12 + 132 + 12 + 136 by 66: `.grp` with four 46px buttons, and two with three 34px buttons.
    assert_eq!(size(layout.toolbar), (526, 66));
    assert_eq!(layout.toolbar.top, 14);
    assert_eq!(
        layout.toolbar.left - MONITOR.left,
        end(MONITOR) - end(layout.toolbar)
    );
    assert_eq!(
        [layout.tools, layout.modes, layout.controls].map(|group| group.width),
        [234, 132, 136]
    );
    // The smaller groups are centered in the toolbar's 66px: 7px above and below.
    assert_eq!(
        [layout.modes, layout.controls].map(|group| group.top),
        [21, 21]
    );
}

#[test]
fn the_buttons_are_in_the_toolbars_order_left_to_right_without_overlapping() {
    for dpi in DPIS {
        let layout = at(dpi);
        assert_eq!(layout.buttons.map(|(button, _)| button), BUTTONS);
        for pair in layout.buttons.windows(2) {
            let ((_, before), (_, after)) = (pair[0], pair[1]);
            assert!(end(before) < after.left, "{dpi}: {before:?} {after:?}");
        }
    }
}

#[test]
fn each_button_sits_inside_its_group_after_the_border_and_the_padding() {
    let layout = at(96);
    let dimensions = place(&layout, ToolbarButton::Tool(Tool::Dimensions));
    assert_eq!(
        (
            dimensions.left - layout.tools.left,
            dimensions.top - layout.tools.top
        ),
        (10, 10)
    );
    assert_eq!(size(dimensions), (46, 46));
    let snip = place(&layout, ToolbarButton::Tool(Tool::Snip));
    assert_eq!(end(layout.tools) - end(snip), 10);
    let horizontal = place(&layout, ToolbarButton::Mode(DimensionsMode::Horizontal));
    let vertical = place(&layout, ToolbarButton::Mode(DimensionsMode::Vertical));
    assert_eq!(horizontal.left - layout.modes.left, 9);
    assert_eq!(vertical.left - end(horizontal), 6);
    let settings = place(&layout, ToolbarButton::Settings);
    let minimize = place(&layout, ToolbarButton::Minimize);
    let close = place(&layout, ToolbarButton::Close);
    assert_eq!(size(close), (34, 34));
    assert_eq!(minimize.left - end(settings), 8);
    assert_eq!(end(layout.controls) - end(close), 9);
}

#[test]
fn every_size_follows_the_monitors_scale() {
    for dpi in DPIS {
        let layout = at(dpi);
        let tool = place(&layout, ToolbarButton::Tool(Tool::Inspect));
        let small = place(&layout, ToolbarButton::Close);
        assert_eq!(tool.width, scaled(46, dpi), "{dpi}");
        assert_eq!(small.width, scaled(34, dpi), "{dpi}");
        assert_eq!(layout.toolbar.top - MONITOR.top, s(14, dpi), "{dpi}");
        // The groups and the gaps between them add up to the whole toolbar.
        assert_eq!(layout.modes.left - end(layout.tools), s(12, dpi));
        assert_eq!(layout.controls.left - end(layout.modes), s(12, dpi));
        assert_eq!(
            (layout.tools.left, end(layout.controls)),
            (layout.toolbar.left, end(layout.toolbar))
        );
        // Centered: the room left and right differs by a pixel at most.
        let left = layout.toolbar.left - MONITOR.left;
        let right = end(MONITOR) - end(layout.toolbar);
        assert!((left - right).abs() <= 1, "{dpi}: {left} {right}");
    }
}

#[test]
fn the_toolbar_is_bigger_at_175_percent_than_at_125() {
    let small = at(120);
    let big = at(168);
    assert!(big.toolbar.width > small.toolbar.width);
    assert_eq!(small.toolbar.height, 2 + 2 * 11 + 58);
}

#[test]
fn on_a_monitor_too_narrow_it_keeps_to_the_left_side() {
    let narrow = Rect {
        left: -800,
        top: -600,
        width: 800,
        height: 600,
    };
    let at_175 = Scale::from_dpi(168).expect("a real scale");
    let narrower_than_the_toolbar = layout(narrow, at_175).toolbar;
    assert_eq!(narrower_than_the_toolbar.left, -800);
    assert_eq!(narrower_than_the_toolbar.top, -600 + s(14, 168));
    let just_too_narrow = Rect {
        width: 526 + 20,
        ..narrow
    };
    assert_eq!(
        layout(just_too_narrow, Scale::HUNDRED_PERCENT).toolbar.left,
        -800 + 14
    );
}

#[test]
fn the_minimized_toolbars_button_is_52_pixels_at_the_toolbars_top_left_corner() {
    for dpi in DPIS {
        let layout = at(dpi);
        assert_eq!(
            layout.minimized,
            Rect {
                left: layout.toolbar.left,
                top: layout.toolbar.top,
                width: scaled(52, dpi),
                height: scaled(52, dpi),
            },
            "{dpi}"
        );
    }
}

#[test]
fn a_point_on_a_button_is_that_button() {
    let layout = at(144);
    for (button, place) in layout.buttons {
        assert_eq!(layout.hit(middle(place), true), Some(Hit::Button(button)));
        let corner = Point {
            x: place.right(),
            y: place.bottom(),
        };
        assert_eq!(layout.hit(corner, true), Some(Hit::Button(button)));
    }
}

#[test]
fn hidden_modes_are_the_toolbars_background() {
    let layout = at(96);
    let vertical = place(&layout, ToolbarButton::Mode(DimensionsMode::Vertical));
    let on_vertical = Point {
        x: vertical.left + 3,
        y: vertical.top + 3,
    };
    assert_eq!(layout.hit(on_vertical, false), Some(Hit::Background));
    let close = place(&layout, ToolbarButton::Close);
    let on_close = Point {
        x: close.left,
        y: close.top,
    };
    assert_eq!(
        layout.hit(on_close, false),
        Some(Hit::Button(ToolbarButton::Close))
    );
}

#[test]
fn the_room_between_buttons_and_groups_is_the_background_and_outside_is_nothing() {
    let layout = at(96);
    let gap_between_groups = Point {
        x: end(layout.tools) + 2,
        y: layout.toolbar.top + 30,
    };
    assert_eq!(layout.hit(gap_between_groups, true), Some(Hit::Background));
    let above_the_controls = Point {
        x: layout.controls.left + 20,
        y: layout.toolbar.top,
    };
    assert_eq!(layout.hit(above_the_controls, true), Some(Hit::Background));
    let padding = Point {
        x: layout.tools.left + 3,
        y: layout.tools.top + 3,
    };
    assert_eq!(layout.hit(padding, true), Some(Hit::Background));
    for outside in [
        Point {
            x: layout.toolbar.left - 1,
            y: layout.toolbar.top,
        },
        Point {
            x: end(layout.toolbar),
            y: layout.toolbar.top,
        },
        Point {
            x: layout.toolbar.left,
            y: below(layout.toolbar),
        },
        Point {
            x: layout.toolbar.left,
            y: layout.toolbar.top - 1,
        },
    ] {
        assert_eq!(layout.hit(outside, true), None, "{outside:?}");
    }
}

#[test]
fn on_the_minimized_toolbar_only_its_button_is_anything() {
    let layout = at(168);
    let restore = Some(Hit::Button(ToolbarButton::Restore));
    assert_eq!(layout.hit_minimized(middle(layout.minimized)), restore);
    let corner = Point {
        x: layout.minimized.right(),
        y: layout.minimized.bottom(),
    };
    assert_eq!(layout.hit_minimized(corner), restore);
    // Where the rest of the toolbar was, and just past the small button, is nothing.
    for off in [
        middle(place(&layout, ToolbarButton::Close)),
        Point {
            x: end(layout.minimized),
            y: layout.minimized.top,
        },
        Point {
            x: layout.minimized.left,
            y: below(layout.minimized),
        },
    ] {
        assert_eq!(layout.hit_minimized(off), None, "{off:?}");
    }
}
