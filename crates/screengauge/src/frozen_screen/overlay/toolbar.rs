//! The toolbar (v1 spec WPC-296, Toolbar; WPC-310): the tools' buttons, then H, V and C in a room kept for them, then
//! the gear, minimize and close, each group on its own panel, at the top center of its monitor. The sizes are the
//! prototype's variant A (WPC-279, `#toolbar`, `.grp`, `.tbtn`, `.mbtn` and `.sbtn`), in logical pixels.
//!
//! Where the toolbar and each button are needs no Windows, so [`layout`] is tested without a monitor, and the frozen
//! screen window asks the same layout which button a click is on: what is drawn and what is clicked never disagree.

use screengauge_core::session::{Point, Rect, Scale, Toolbar, ToolbarButton};
use screengauge_core::settings::{DimensionsMode, Tool};
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Direct2D::ID2D1RenderTarget;

use super::Overlay;
use super::geometry::{self, scaled};

// Every size below is in logical pixels, which `scaled` turns into physical pixels for the monitor's scale.

/// How far the toolbar sits from its monitor's top side, and at least from its left and right sides.
const MARGIN: u32 = 14;
/// The room between two groups (`#toolbar`'s `gap`).
const GROUP_GAP: u32 = 12;
/// Each group's border (`.grp`'s `border: 1px`), which the painting of the panels shares.
pub(super) const RING: u32 = 1;
/// The tools' group (`.grp`): its padding and the room between its buttons, which are this wide and tall (`.tbtn`).
const TOOLS_PAD: u32 = 9;
const TOOLS_GAP: u32 = 10;
const TOOL: u32 = 46;
/// The modes' group (`.grp.modes`) and the gear's (`.grp.small`): their padding, the room between their buttons, and the
/// buttons' size (`.mbtn` and `.sbtn`).
const SMALL_PAD: u32 = 8;
const MODES_GAP: u32 = 6;
const CONTROLS_GAP: u32 = 8;
const SMALL: u32 = 34;

/// How a button is styled and which group it is in: the tools' big buttons, the modes (H, V and C), and the small buttons
/// that act on the toolbar itself (the gear, minimize and close).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ButtonKind {
    Tool,
    Mode,
    Control,
}

/// What `button` is. A button is listed here, and a new one fails to compile until it is, so the painting and the clicks
/// never style it by default.
pub(super) fn kind(button: ToolbarButton) -> ButtonKind {
    match button {
        ToolbarButton::Tool(_) => ButtonKind::Tool,
        ToolbarButton::Mode(_) => ButtonKind::Mode,
        // The restore button, which only the minimized toolbar has, is not drawn by this toolbar: it is small, and it is
        // not a mode, which is as much as the clicks need to know of it.
        ToolbarButton::Settings
        | ToolbarButton::Minimize
        | ToolbarButton::Restore
        | ToolbarButton::Close => ButtonKind::Control,
    }
}

/// Whether `toolbar` draws `button` lit: the active tool's, and the mode's while there is one.
pub(super) fn is_lit(toolbar: &Toolbar, button: ToolbarButton) -> bool {
    match button {
        ToolbarButton::Tool(tool) => tool == toolbar.active,
        ToolbarButton::Mode(mode) => toolbar.mode == Some(mode),
        ToolbarButton::Settings
        | ToolbarButton::Minimize
        | ToolbarButton::Restore
        | ToolbarButton::Close => false,
    }
}

/// The buttons, in the toolbar's order, left to right.
pub const BUTTONS: [ToolbarButton; 10] = [
    ToolbarButton::Tool(Tool::Dimensions),
    ToolbarButton::Tool(Tool::Bounds),
    ToolbarButton::Tool(Tool::Inspect),
    ToolbarButton::Tool(Tool::Snip),
    ToolbarButton::Mode(DimensionsMode::Horizontal),
    ToolbarButton::Mode(DimensionsMode::Vertical),
    ToolbarButton::Mode(DimensionsMode::Cross),
    ToolbarButton::Settings,
    ToolbarButton::Minimize,
    ToolbarButton::Close,
];

/// Where the toolbar's parts are, in physical pixels of the virtual screen, each rectangle's right and bottom one past
/// its last pixel.
#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    /// The whole toolbar, the room between its groups included: a click anywhere in it is the toolbar's, never the
    /// tool's.
    pub toolbar: RECT,
    /// The tools' group, as tall as the toolbar.
    pub tools: RECT,
    /// The room kept for H, V and C, whose group shows only while Dimensions is the active tool.
    pub modes: RECT,
    /// The gear, minimize and close's group.
    pub controls: RECT,
    /// Each button of [`BUTTONS`], in the same order.
    pub buttons: [(ToolbarButton, RECT); 10],
}

/// What a point on the toolbar is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    Button(ToolbarButton),
    /// The toolbar but no button: a group's panel or padding, the room between groups, or the empty room of H, V and C.
    Background,
}

/// Where the toolbar goes on `monitor`, whose scale is `dpi`: at its top center, `MARGIN` below its top side. On a
/// monitor too narrow for it and its margins, it starts `MARGIN` inside the monitor's left side, and on one narrower than
/// the toolbar itself, at the monitor's left side.
pub fn layout(monitor: RECT, dpi: u32) -> Layout {
    let s = |logical| scaled(logical, dpi);
    let ring = s(RING);
    // Each size is scaled on its own, so a group is exactly as wide as the parts drawn in it.
    let group = |pad: u32, gap: u32, button: u32| {
        let width = 2 * ring + 2 * s(pad) + 3 * s(button) + 2 * s(gap);
        let height = 2 * ring + 2 * s(pad) + s(button);
        (width, height)
    };
    let (tools_width, height) = {
        let (three, height) = group(TOOLS_PAD, TOOLS_GAP, TOOL);
        (three + s(TOOL) + s(TOOLS_GAP), height)
    };
    let (modes_width, small_height) = group(SMALL_PAD, MODES_GAP, SMALL);
    let (controls_width, _) = group(SMALL_PAD, CONTROLS_GAP, SMALL);
    let width = tools_width + s(GROUP_GAP) + modes_width + s(GROUP_GAP) + controls_width;

    let margin = s(MARGIN);
    let room = (monitor.right - monitor.left - width - 2 * margin).max(0);
    let left = (monitor.left + margin + room / 2)
        .min(monitor.right - width)
        .max(monitor.left);
    let top = monitor.top + margin;
    // The smaller groups are centered in the toolbar's height (CSS `align-items: center`), an odd pixel left below them.
    let small_top = top + (height - small_height + 1) / 2;

    let tools = sized(left, top, tools_width, height);
    let modes = sized(
        tools.right + s(GROUP_GAP),
        small_top,
        modes_width,
        small_height,
    );
    let controls = sized(
        modes.right + s(GROUP_GAP),
        small_top,
        controls_width,
        small_height,
    );
    let row = |group: &RECT, pad: u32, gap: u32, button: u32, index: i32| {
        let inset = ring + s(pad);
        let size = s(button);
        sized(
            group.left + inset + index * (size + s(gap)),
            group.top + inset,
            size,
            size,
        )
    };
    let tool = |index| row(&tools, TOOLS_PAD, TOOLS_GAP, TOOL, index);
    let mode = |index| row(&modes, SMALL_PAD, MODES_GAP, SMALL, index);
    let control = |index| row(&controls, SMALL_PAD, CONTROLS_GAP, SMALL, index);
    let places = [
        tool(0),
        tool(1),
        tool(2),
        tool(3),
        mode(0),
        mode(1),
        mode(2),
        control(0),
        control(1),
        control(2),
    ];
    let buttons = std::array::from_fn(|index| (BUTTONS[index], places[index]));
    Layout {
        toolbar: sized(left, top, width, height),
        tools,
        modes,
        controls,
        buttons,
    }
}

/// [`layout`] on a monitor as the frozen screen has it.
pub fn on_monitor(monitor: Rect, scale: Scale) -> windows::core::Result<Layout> {
    Ok(layout(geometry::bounds(monitor)?, scale.dpi()))
}

impl Layout {
    /// What `at`, in physical pixels of the virtual screen, is on: `None` off the toolbar. H, V and C count only when
    /// `modes_shown`; otherwise their room is the toolbar's background.
    pub fn hit(&self, at: Point, modes_shown: bool) -> Option<Hit> {
        if !contains(&self.toolbar, at) {
            return None;
        }
        let button = self
            .buttons
            .iter()
            .filter(|(button, _)| modes_shown || kind(*button) != ButtonKind::Mode)
            .find(|(_, place)| contains(place, at))
            .map(|&(button, _)| Hit::Button(button));
        Some(button.unwrap_or(Hit::Background))
    }
}

/// The rectangle at (`left`, `top`) that is `width` by `height`.
fn sized(left: i32, top: i32, width: i32, height: i32) -> RECT {
    RECT {
        left,
        top,
        right: left + width,
        bottom: top + height,
    }
}

fn contains(rect: &RECT, at: Point) -> bool {
    (rect.left..rect.right).contains(&at.x) && (rect.top..rect.bottom).contains(&at.y)
}

impl Overlay {
    /// Draws `toolbar` over the measurements and under the tolerance indicator and the notice, between BeginDraw and
    /// EndDraw, when it is on this window's monitor: each group's panel and its buttons, in the palette of the window,
    /// with the active tool's button and the mode's lit. They are drawn once on a bitmap that the next frames lay over the
    /// photo, until another tool or mode is lit (`toolbar_cache`).
    pub fn draw_toolbar(
        &self,
        target: &ID2D1RenderTarget,
        toolbar: &Toolbar,
    ) -> windows::core::Result<()> {
        let layout = layout(self.monitor, self.dpi);
        self.show_toolbar(target, &layout, toolbar)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 4K monitor right of the main one, as on the owner's desk.
    const MONITOR: RECT = RECT {
        left: 1920,
        top: 0,
        right: 1920 + 3840,
        bottom: 2160,
    };

    /// Every scale Windows offers.
    const DPIS: [u32; 10] = [96, 120, 144, 168, 192, 216, 240, 288, 336, 384];

    fn width(rect: &RECT) -> i32 {
        rect.right - rect.left
    }

    fn height(rect: &RECT) -> i32 {
        rect.bottom - rect.top
    }

    fn place(layout: &Layout, button: ToolbarButton) -> RECT {
        layout
            .buttons
            .iter()
            .find(|(each, _)| *each == button)
            .map(|&(_, place)| place)
            .expect("every button has a place")
    }

    #[test]
    fn at_100_percent_the_toolbar_is_the_prototypes_size_at_the_top_center() {
        let layout = layout(MONITOR, 96);
        // 234 + 12 + 132 + 12 + 136 by 66: `.grp` with four 46px buttons, and two with three 34px buttons.
        assert_eq!((width(&layout.toolbar), height(&layout.toolbar)), (526, 66));
        assert_eq!(layout.toolbar.top, 14);
        assert_eq!(
            layout.toolbar.left - MONITOR.left,
            MONITOR.right - layout.toolbar.right
        );
        assert_eq!(
            [&layout.tools, &layout.modes, &layout.controls].map(width),
            [234, 132, 136]
        );
        // The smaller groups are centered in the toolbar's 66px: 7px above and below.
        assert_eq!(
            [&layout.modes, &layout.controls].map(|group| group.top),
            [21, 21]
        );
    }

    #[test]
    fn the_buttons_are_in_the_toolbars_order_left_to_right_without_overlapping() {
        for dpi in DPIS {
            let layout = layout(MONITOR, dpi);
            assert_eq!(layout.buttons.map(|(button, _)| button), BUTTONS);
            for pair in layout.buttons.windows(2) {
                let ((_, before), (_, after)) = (pair[0], pair[1]);
                assert!(before.right < after.left, "{dpi}: {before:?} {after:?}");
            }
        }
    }

    #[test]
    fn each_button_sits_inside_its_group_after_the_border_and_the_padding() {
        let dpi = 96;
        let layout = layout(MONITOR, dpi);
        let dimensions = place(&layout, ToolbarButton::Tool(Tool::Dimensions));
        assert_eq!(
            (
                dimensions.left - layout.tools.left,
                dimensions.top - layout.tools.top
            ),
            (10, 10)
        );
        assert_eq!((width(&dimensions), height(&dimensions)), (46, 46));
        let snip = place(&layout, ToolbarButton::Tool(Tool::Snip));
        assert_eq!(layout.tools.right - snip.right, 10);
        let horizontal = place(&layout, ToolbarButton::Mode(DimensionsMode::Horizontal));
        let vertical = place(&layout, ToolbarButton::Mode(DimensionsMode::Vertical));
        assert_eq!(horizontal.left - layout.modes.left, 9);
        assert_eq!(vertical.left - horizontal.right, 6);
        let settings = place(&layout, ToolbarButton::Settings);
        let minimize = place(&layout, ToolbarButton::Minimize);
        let close = place(&layout, ToolbarButton::Close);
        assert_eq!((width(&close), height(&close)), (34, 34));
        assert_eq!(minimize.left - settings.right, 8);
        assert_eq!(layout.controls.right - close.right, 9);
    }

    #[test]
    fn every_size_follows_the_monitors_scale() {
        for dpi in DPIS {
            let layout = layout(MONITOR, dpi);
            let tool = place(&layout, ToolbarButton::Tool(Tool::Inspect));
            let small = place(&layout, ToolbarButton::Close);
            assert_eq!(width(&tool), scaled(46, dpi), "{dpi}");
            assert_eq!(width(&small), scaled(34, dpi), "{dpi}");
            assert_eq!(layout.toolbar.top - MONITOR.top, scaled(14, dpi), "{dpi}");
            // The groups and the gaps between them add up to the whole toolbar.
            assert_eq!(layout.modes.left - layout.tools.right, scaled(12, dpi));
            assert_eq!(layout.controls.left - layout.modes.right, scaled(12, dpi));
            assert_eq!(
                (layout.tools.left, layout.controls.right),
                (layout.toolbar.left, layout.toolbar.right)
            );
            // Centered: the room left and right differs by a pixel at most.
            let left = layout.toolbar.left - MONITOR.left;
            let right = MONITOR.right - layout.toolbar.right;
            assert!((left - right).abs() <= 1, "{dpi}: {left} {right}");
        }
    }

    #[test]
    fn the_toolbar_is_bigger_at_175_percent_than_at_125() {
        let small = layout(MONITOR, 120);
        let big = layout(MONITOR, 168);
        assert!(width(&big.toolbar) > width(&small.toolbar));
        assert_eq!(height(&small.toolbar), 2 + 2 * 11 + 58);
    }

    #[test]
    fn on_a_monitor_too_narrow_it_keeps_to_the_left_side() {
        let narrow = RECT {
            left: -800,
            top: -600,
            right: 0,
            bottom: 0,
        };
        let narrower_than_the_toolbar = layout(narrow, 168).toolbar;
        assert_eq!(narrower_than_the_toolbar.left, -800);
        assert_eq!(narrower_than_the_toolbar.top, -600 + scaled(14, 168));
        let just_too_narrow = RECT {
            right: -800 + 526 + 20,
            ..narrow
        };
        assert_eq!(layout(just_too_narrow, 96).toolbar.left, -800 + 14);
    }

    #[test]
    fn a_click_on_a_button_is_that_button() {
        let layout = layout(MONITOR, 144);
        for (button, place) in layout.buttons {
            let middle = Point {
                x: (place.left + place.right) / 2,
                y: (place.top + place.bottom) / 2,
            };
            assert_eq!(layout.hit(middle, true), Some(Hit::Button(button)));
            let corner = Point {
                x: place.right - 1,
                y: place.bottom - 1,
            };
            assert_eq!(layout.hit(corner, true), Some(Hit::Button(button)));
        }
    }

    #[test]
    fn hidden_modes_are_the_toolbars_background() {
        let layout = layout(MONITOR, 96);
        let vertical = place(&layout, ToolbarButton::Mode(DimensionsMode::Vertical));
        let at = Point {
            x: vertical.left + 3,
            y: vertical.top + 3,
        };
        assert_eq!(layout.hit(at, false), Some(Hit::Background));
        let close = place(&layout, ToolbarButton::Close);
        let at = Point {
            x: close.left,
            y: close.top,
        };
        assert_eq!(
            layout.hit(at, false),
            Some(Hit::Button(ToolbarButton::Close))
        );
    }

    #[test]
    fn the_room_between_buttons_and_groups_is_the_background_and_outside_is_nothing() {
        let layout = layout(MONITOR, 96);
        let gap_between_groups = Point {
            x: layout.tools.right + 2,
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
                x: layout.toolbar.right,
                y: layout.toolbar.top,
            },
            Point {
                x: layout.toolbar.left,
                y: layout.toolbar.bottom,
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
    fn the_monitors_place_comes_from_the_frozen_screen() {
        let monitor = Rect {
            left: 3840,
            top: 0,
            width: 1920,
            height: 1080,
        };
        let scale = Scale::from_dpi(120).expect("a scale");
        let from_frozen = on_monitor(monitor, scale).expect("a monitor's size");
        let from_rect = layout(
            RECT {
                left: 3840,
                top: 0,
                right: 5760,
                bottom: 1080,
            },
            120,
        );
        assert_eq!(from_frozen, from_rect);
    }
}
