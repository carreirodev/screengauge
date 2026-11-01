//! Layer 1 tests of the toolbar's place (v1 spec WPC-296, Toolbar; ticket WPC-312): the relative place the remembered
//! state keeps, carried to monitors of other sizes and scales, and the push inside a monitor too small for it, on
//! monitor arrangements made up here. Each expected corner is worked out by hand in its comment, from the prototype's
//! sizes (variant A of WPC-279) and its 14 logical pixels of margin.

use screengauge_core::session::toolbar::{corner, layout, place_nearest};
use screengauge_core::session::{Point, Rect, Scale};
use screengauge_core::settings::ToolbarPlace;

/// A 4K monitor right of the main one.
const FOUR_K: Rect = Rect {
    left: 1920,
    top: 0,
    width: 3840,
    height: 2160,
};

/// A Full HD monitor, Windows' main one.
const FULL_HD: Rect = Rect {
    left: 0,
    top: 0,
    width: 1920,
    height: 1080,
};

fn scale(dpi: u32) -> Scale {
    Scale::from_dpi(dpi).expect("a real scale")
}

fn place(x: f64, y: f64) -> ToolbarPlace {
    ToolbarPlace::new(x, y).expect("shares from 0 to 1")
}

/// `x`, `y` as a point.
fn point(x: i32, y: i32) -> Point {
    Point { x, y }
}

#[test]
fn the_first_call_puts_it_at_the_top_center() {
    // At 100% the toolbar is 526 × 66, so the 4K monitor leaves it 3840 - 526 - 2 × 14 = 3286 to move across: half of
    // it is 1643, after the 14 of margin.
    assert_eq!(
        corner(FOUR_K, scale(96), ToolbarPlace::default()),
        Point {
            x: 1920 + 14 + 1643,
            y: 14,
        }
    );
}

#[test]
fn at_175_percent_the_top_center_is_where_the_bigger_toolbar_is_centered() {
    // At 175% the toolbar is 930 × 117 and its margin 25 (24.5, rounded up): the owner's 4K monitor leaves it
    // 3840 - 930 - 50 = 2860, half of it 1430.
    assert_eq!(
        corner(
            Rect { left: 0, ..FOUR_K },
            scale(168),
            ToolbarPlace::default()
        ),
        point(25 + 1430, 25)
    );
}

#[test]
fn the_same_place_is_the_same_share_of_the_room_on_a_smaller_monitor() {
    let place = place(0.25, 0.5);
    // At 100% the 4K monitor leaves the 526 × 66 toolbar 3286 across and 2160 - 66 - 28 = 2066 down: a quarter of
    // 3286 is 821.5, whose half goes left, and half of 2066 is 1033.
    assert_eq!(
        corner(FOUR_K, scale(96), place),
        point(1920 + 14 + 821, 14 + 1033)
    );
    // Full HD leaves it 1920 - 554 = 1366 across, a quarter of it 341.5, and 1080 - 94 = 986 down, half of it 493.
    assert_eq!(corner(FULL_HD, scale(96), place), point(14 + 341, 14 + 493));
}

#[test]
fn the_bottom_left_stays_the_bottom_left_on_a_monitor_of_another_size_and_scale() {
    let bottom_left = place(0.0, 1.0);
    // At 175%: 930 × 117 with a margin of 25, so its bottom side is 25 above the 4K monitor's.
    assert_eq!(
        corner(FOUR_K, scale(168), bottom_left),
        point(1920 + 25, 2160 - 117 - 25)
    );
    // At 125%: 663 × 82 with a margin of 18 (17.5, rounded up).
    assert_eq!(
        corner(FULL_HD, scale(120), bottom_left),
        point(18, 1080 - 82 - 18)
    );
}

#[test]
fn the_bottom_right_keeps_the_margin_from_both_sides_on_a_monitor_left_of_and_above_the_main_one() {
    let left_above = Rect {
        left: -1920,
        top: -200,
        ..FULL_HD
    };
    assert_eq!(
        corner(left_above, scale(96), place(1.0, 1.0)),
        point(-1920 + 1920 - 526 - 14, -200 + 1080 - 66 - 14)
    );
    assert_eq!(
        corner(left_above, scale(96), place(1.0, 0.0)),
        point(-1920 + 1920 - 526 - 14, -200 + 14)
    );
}

#[test]
fn on_a_monitor_too_narrow_for_its_margins_it_is_pushed_inside_wherever_it_was() {
    // 530 wide at 100%: the 526 of the toolbar fit, but not its margins, so it is pushed back until its right side is
    // the monitor's, 4 from the left one.
    let narrow = Rect {
        left: 3840,
        top: 0,
        width: 530,
        height: 1080,
    };
    for x in [0.0, 0.5, 1.0] {
        assert_eq!(
            corner(narrow, scale(96), place(x, 0.0)),
            point(3840 + 4, 14),
            "{x}"
        );
    }
    // 800 wide at 175%, narrower than the 930 of the toolbar: it starts at the monitor's left side.
    let narrower = Rect {
        width: 800,
        ..narrow
    };
    for x in [0.0, 1.0] {
        assert_eq!(corner(narrower, scale(168), place(x, 1.0)).x, 3840, "{x}");
    }
}

#[test]
fn on_a_monitor_too_short_for_its_margins_it_is_pushed_inside_wherever_it_was() {
    // 70 tall at 100%: the 66 of the toolbar fit, but not its margins, so its bottom side is the monitor's.
    let short = Rect {
        left: 0,
        top: 1080,
        width: 1920,
        height: 70,
    };
    for y in [0.0, 1.0] {
        assert_eq!(corner(short, scale(96), place(0.5, y)).y, 1080 + 4, "{y}");
    }
    // 80 tall: room for the top margin, none for both, so it stays 14 below the top side.
    let less_short = Rect {
        height: 80,
        ..short
    };
    assert_eq!(corner(less_short, scale(96), place(0.5, 1.0)).y, 1080 + 14);
    // 40 tall, shorter than the toolbar: it starts at the monitor's top side.
    let shorter = Rect {
        height: 40,
        ..short
    };
    assert_eq!(corner(shorter, scale(96), place(0.5, 1.0)).y, 1080);
}

/// Every scale Windows offers.
const DPIS: [u32; 10] = [96, 120, 144, 168, 192, 216, 240, 288, 336, 384];

/// Monitors of made-up sizes, some leaving the toolbar an odd room to move in.
const MONITORS: [Rect; 4] = [
    FOUR_K,
    FULL_HD,
    Rect {
        left: -1366,
        top: -768,
        width: 1366,
        height: 768,
    },
    Rect {
        left: 7680,
        top: 333,
        width: 2561,
        height: 1441,
    },
];

#[test]
fn the_top_center_is_where_the_toolbar_was_before_it_could_be_dragged() {
    // Before WPC-312 the toolbar's left side was half the room, rounded down, past the margin, wherever the toolbar and
    // its margins fit; where they don't, the push tests above hold.
    for monitor in MONITORS {
        for dpi in DPIS {
            let at = corner(monitor, scale(dpi), ToolbarPlace::default());
            let width = layout(at, scale(dpi)).toolbar.width;
            let margin = i32::try_from((14 * dpi + 48) / 96).expect("a small margin");
            let room = i32::try_from(monitor.width).expect("a monitor's width")
                - i32::try_from(width).expect("the toolbar's width")
                - 2 * margin;
            if room < 0 {
                continue;
            }
            assert_eq!(
                at,
                point(monitor.left + margin + room / 2, monitor.top + margin),
                "{monitor:?} at {dpi}"
            );
        }
    }
}

#[test]
fn a_dragged_corner_comes_back_to_the_same_pixel_at_the_next_call() {
    for monitor in MONITORS {
        for dpi in DPIS {
            let scale = scale(dpi);
            let first = corner(monitor, scale, place(0.0, 0.0));
            let last = corner(monitor, scale, place(1.0, 1.0));
            // Every pixel across, and down the top, the middle and the bottom.
            for x in first.x..=last.x {
                for y in [first.y, (first.y + last.y) / 2, last.y] {
                    let wanted = point(x, y);
                    let place = place_nearest(monitor, scale, wanted, ToolbarPlace::default());
                    assert_eq!(
                        corner(monitor, scale, place),
                        wanted,
                        "{monitor:?} at {dpi}: {place:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn a_corner_wanted_past_the_margins_stops_at_them() {
    let scale = scale(96);
    let top_left = place_nearest(FULL_HD, scale, point(-500, -3), place(0.5, 0.5));
    assert_eq!(top_left, place(0.0, 0.0));
    assert_eq!(corner(FULL_HD, scale, top_left), point(14, 14));
    // Past the right and bottom margins, even on the next monitor.
    let bottom_right = place_nearest(FULL_HD, scale, point(2500, 1070), place(0.5, 0.5));
    assert_eq!(bottom_right, place(1.0, 1.0));
    assert_eq!(
        corner(FULL_HD, scale, bottom_right),
        point(1920 - 526 - 14, 1080 - 66 - 14)
    );
}

#[test]
fn a_corner_left_on_its_pixel_keeps_the_place_as_it_was() {
    // A third of Full HD's 1366 is 455.33…: the corner sits on pixel 455, which as a share is not a third.
    let third = place(1.0 / 3.0, 0.0);
    let at = corner(FULL_HD, scale(96), third);
    assert_eq!(at, point(14 + 455, 14));
    assert_eq!(place_nearest(FULL_HD, scale(96), at, third), third);
    // Moved across only, its share down stays the one it had.
    let across = place_nearest(FULL_HD, scale(96), point(at.x + 1, at.y), third);
    assert_eq!(across, place(456.0 / 1366.0, 0.0));
}

#[test]
fn on_an_axis_with_no_room_to_move_the_place_keeps_its_share() {
    // 530 wide: the toolbar can't move across, so its share across stays whatever the drag wants.
    let narrow = Rect {
        left: 0,
        top: 0,
        width: 530,
        height: 1080,
    };
    let kept = place(0.7, 0.0);
    let dragged = place_nearest(narrow, scale(96), point(-100, 1080 - 66 - 14), kept);
    assert_eq!(dragged, place(0.7, 1.0));
}
