//! Where the toolbar is (v1 spec WPC-296, Toolbar; WPC-310, WPC-311): the tools' buttons, then H, V and C in a room
//! kept for them, then the gear, minimize and close, each group on its own panel, at the top center of its monitor; and
//! the small button the minimized toolbar becomes, at the toolbar's top left corner. The sizes are the prototype's
//! variant A (WPC-279, `#toolbar`, `.grp`, `.tbtn`, `.mbtn`, `.sbtn` and `#tbMini`), in logical pixels.
//!
//! The session knows from this where the cursor is over the toolbar, so the tool pauses there, and the Windows layer
//! draws the toolbar and finds the button a click is on from the same layout: what is drawn, what is clicked and where
//! the tool pauses never disagree.

use super::{Point, Rect, Scale, ToolbarButton};
use crate::settings::{DimensionsMode, Tool};

// Every size below is in logical pixels, which `scaled` turns into physical pixels for the monitor's scale.

/// How far the toolbar sits from its monitor's top side, and at least from its left and right sides.
const MARGIN: u32 = 14;
/// The room between two groups (`#toolbar`'s `gap`).
const GROUP_GAP: u32 = 12;
/// Each group's border (`.grp`'s `border: 1px`), which the painting of the panels shares.
pub const RING: u32 = 1;
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
/// The minimized toolbar's button (`#tbMini`), square.
const MINIMIZED: u32 = 52;

/// `logical` pixels on a monitor of `dpi` (96 is 100%), in whole physical pixels, halves up: how every size of what
/// ScreenGauge draws itself follows the monitor's scale.
pub fn scaled(logical: u32, dpi: u32) -> u32 {
    // Saturating, so a made-up size can't overflow; divided by 96, the result is at most about 44 million.
    logical.saturating_mul(dpi).saturating_add(48) / 96
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

/// Where the toolbar's parts are, in physical pixels of the virtual screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Layout {
    /// The whole toolbar, the room between its groups included: while it shows, the cursor anywhere in it is on the
    /// toolbar, never on the tool.
    pub toolbar: Rect,
    /// The tools' group, as tall as the toolbar.
    pub tools: Rect,
    /// The room kept for H, V and C, whose group shows only while Dimensions is the active tool.
    pub modes: Rect,
    /// The gear, minimize and close's group.
    pub controls: Rect,
    /// Each button of [`BUTTONS`], in the same order.
    pub buttons: [(ToolbarButton, Rect); BUTTONS.len()],
    /// The button the minimized toolbar becomes ([`ToolbarButton::Restore`]), in the same place: at the toolbar's top
    /// left corner. While the toolbar is minimized, it is all there is of it.
    pub minimized: Rect,
}

/// What a point on the toolbar is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    Button(ToolbarButton),
    /// The toolbar but no button: a group's panel or padding, the room between groups, or the empty room of H, V and C.
    Background,
}

/// Where the toolbar goes on `monitor`, at `scale`: at its top center, `MARGIN` below its top side. On a monitor too
/// narrow for it and its margins, it starts `MARGIN` inside the monitor's left side, and on one narrower than the
/// toolbar itself, at the monitor's left side.
pub fn layout(monitor: Rect, scale: Scale) -> Layout {
    let s = |logical| scaled(logical, scale.dpi());
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
    let room = monitor.width.saturating_sub(width + 2 * margin);
    let left = monitor.left.saturating_add_unsigned(margin + room / 2).min(
        monitor
            .left
            .saturating_add_unsigned(monitor.width.saturating_sub(width)),
    );
    let top = monitor.top.saturating_add_unsigned(margin);
    // The smaller groups are centered in the toolbar's height (CSS `align-items: center`), an odd pixel left below them.
    let small_top = top.saturating_add_unsigned(height.saturating_sub(small_height).div_ceil(2));

    let tools = sized(left, top, tools_width, height);
    let modes = sized(
        after(tools, s(GROUP_GAP)),
        small_top,
        modes_width,
        small_height,
    );
    let controls = sized(
        after(modes, s(GROUP_GAP)),
        small_top,
        controls_width,
        small_height,
    );
    let row = |group: Rect, pad: u32, gap: u32, button: u32, index: u32| {
        let inset = ring + s(pad);
        let size = s(button);
        sized(
            group
                .left
                .saturating_add_unsigned(inset + index * (size + s(gap))),
            group.top.saturating_add_unsigned(inset),
            size,
            size,
        )
    };
    let tool = |index| row(tools, TOOLS_PAD, TOOLS_GAP, TOOL, index);
    let mode = |index| row(modes, SMALL_PAD, MODES_GAP, SMALL, index);
    let control = |index| row(controls, SMALL_PAD, CONTROLS_GAP, SMALL, index);
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
        minimized: sized(left, top, s(MINIMIZED), s(MINIMIZED)),
    }
}

impl Layout {
    /// What `at`, in physical pixels of the virtual screen, is on, on the whole toolbar: `None` off it. H, V and C count
    /// only when `modes_shown`; otherwise their room is the toolbar's background.
    pub fn hit(&self, at: Point, modes_shown: bool) -> Option<Hit> {
        if !self.toolbar.contains(at) {
            return None;
        }
        let button = self
            .buttons
            .iter()
            .filter(|(button, _)| modes_shown || !matches!(button, ToolbarButton::Mode(_)))
            .find(|(_, place)| place.contains(at))
            .map(|&(button, _)| Hit::Button(button));
        Some(button.unwrap_or(Hit::Background))
    }

    /// What `at` is on, on the minimized toolbar: its button, or `None` off it.
    pub fn hit_minimized(&self, at: Point) -> Option<Hit> {
        self.minimized
            .contains(at)
            .then_some(Hit::Button(ToolbarButton::Restore))
    }
}

/// The rectangle at (`left`, `top`) that is `width` by `height`.
fn sized(left: i32, top: i32, width: u32, height: u32) -> Rect {
    Rect {
        left,
        top,
        width,
        height,
    }
}

/// The column `gap` pixels right of `rect`.
fn after(rect: Rect, gap: u32) -> i32 {
    rect.left.saturating_add_unsigned(rect.width + gap)
}
