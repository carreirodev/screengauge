//! Where the toolbar is (v1 spec WPC-296, Toolbar; WPC-310, WPC-311, WPC-312): the tools' buttons, then H, V and C in a
//! room kept for them, then the gear, minimize and close, each group on its own panel; and the small button the
//! minimized toolbar becomes, at the toolbar's top left corner. The sizes are the prototype's variant A (WPC-279,
//! `#toolbar`, `.grp`, `.tbtn`, `.mbtn`, `.sbtn` and `#tbMini`), in logical pixels.
//!
//! Its place in its monitor is a [`ToolbarPlace`], the remembered state's: a share of the room the toolbar has to move
//! inside its margins on each axis, so the same place means the same spot, such as the bottom left, on a monitor of any
//! size or scale. [`corner`] turns a place into the toolbar's top left corner on a monitor, and [`place_nearest`] turns
//! the corner a drag wants back into a place.
//!
//! The session knows from this where the cursor is over the toolbar, so the tool pauses there, and the Windows layer
//! draws the toolbar and finds the button a click is on from the same layout: what is drawn, what is clicked and where
//! the tool pauses never disagree.

use super::{Point, Rect, Scale, ToolbarButton};
use crate::settings::{DimensionsMode, Tool, ToolbarPlace};

// Every size below is in logical pixels, which `scaled` turns into physical pixels for the monitor's scale.

/// How far the toolbar stays from each side of its monitor, wherever it is placed or dragged (the owner's choice in
/// WPC-312), unless the monitor is too small for it.
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

/// The groups' sizes at a scale, in physical pixels.
struct Sizes {
    /// The tools' group's width, the modes' group's and the gear's.
    tools_width: u32,
    modes_width: u32,
    controls_width: u32,
    /// The room between two groups.
    gap: u32,
    /// The tools' group's height, which is the toolbar's.
    height: u32,
    /// The height of the modes' group and of the gear's, which are centered in the toolbar's.
    small_height: u32,
}

impl Sizes {
    /// The prototype's sizes, scaled for `scale`.
    fn at(scale: Scale) -> Sizes {
        let s = |logical| scaled(logical, scale.dpi());
        let ring = s(RING);
        // Each size is scaled on its own, so a group is exactly as wide as the parts drawn in it.
        let group = |pad: u32, gap: u32, button: u32| {
            let width = 2 * ring + 2 * s(pad) + 3 * s(button) + 2 * s(gap);
            let height = 2 * ring + 2 * s(pad) + s(button);
            (width, height)
        };
        let (three_tools, height) = group(TOOLS_PAD, TOOLS_GAP, TOOL);
        let (modes_width, small_height) = group(SMALL_PAD, MODES_GAP, SMALL);
        let (controls_width, _) = group(SMALL_PAD, CONTROLS_GAP, SMALL);
        Sizes {
            tools_width: three_tools + s(TOOL) + s(TOOLS_GAP),
            modes_width,
            controls_width,
            gap: s(GROUP_GAP),
            height,
            small_height,
        }
    }

    /// The whole toolbar's width, the room between its groups included.
    fn width(&self) -> u32 {
        self.tools_width + self.gap + self.modes_width + self.gap + self.controls_width
    }
}

/// Where the toolbar's top left corner goes on `monitor`, at `scale`, for `place`: on each axis, `MARGIN` inside the
/// monitor's near side, plus the place's share of the room left inside both margins. The default place is the top
/// center, where the first call puts it. On a monitor too small for the toolbar and its margins, it is pushed inside:
/// it starts `MARGIN` inside the near side unless that takes it past the far side, and on a monitor smaller than the
/// toolbar itself it starts at the near side.
pub fn corner(monitor: Rect, scale: Scale, place: ToolbarPlace) -> Point {
    let (across, down) = Axis::of(monitor, scale);
    Point {
        x: across.place(place.x()),
        y: down.place(place.y()),
    }
}

/// Where the toolbar's parts are on `monitor`, at `scale`, for `place`: [`layout`] at the toolbar's [`corner`].
pub fn layout_on(monitor: Rect, scale: Scale, place: ToolbarPlace) -> Layout {
    layout(corner(monitor, scale, place), scale)
}

/// The place that puts the toolbar's top left corner on `monitor` as near `wanted` as it can go, at `scale`: inside
/// the monitor's margins, as [`corner`] puts it for that place. On an axis where the toolbar has no room to move, or
/// where its corner would stay on the same pixel, `current`'s share is kept, so a drag that ends where it began leaves
/// the remembered place as it was.
pub fn place_nearest(
    monitor: Rect,
    scale: Scale,
    wanted: Point,
    current: ToolbarPlace,
) -> ToolbarPlace {
    let (across, down) = Axis::of(monitor, scale);
    let x = across.share_nearest(wanted.x, current.x());
    let y = down.share_nearest(wanted.y, current.y());
    // Can't happen: each share is a whole number of pixels from 0 to the room divided by the room, or `current`'s own.
    ToolbarPlace::new(x, y).unwrap_or(current)
}

/// One of a monitor's two axes, as the toolbar moves along it.
struct Axis {
    /// Where the monitor starts along it: its left side across, its top side down.
    start: i32,
    /// How long the monitor is along it.
    length: u32,
    /// How long the toolbar is along it.
    size: u32,
    /// What the toolbar keeps from either end.
    margin: u32,
}

impl Axis {
    /// The axes of `monitor`, across and down, for the toolbar at `scale`.
    fn of(monitor: Rect, scale: Scale) -> (Axis, Axis) {
        let sizes = Sizes::at(scale);
        let margin = scaled(MARGIN, scale.dpi());
        let across = Axis {
            start: monitor.left,
            length: monitor.width,
            size: sizes.width(),
            margin,
        };
        let down = Axis {
            start: monitor.top,
            length: monitor.height,
            size: sizes.height,
            margin,
        };
        (across, down)
    }

    /// The room the toolbar has to move along the axis, inside both margins.
    fn room(&self) -> u32 {
        self.length
            .saturating_sub(self.size.saturating_add(self.margin.saturating_mul(2)))
    }

    /// The first pixel the toolbar can start at: `MARGIN` past the monitor's near side.
    fn first(&self) -> i32 {
        self.start.saturating_add_unsigned(self.margin)
    }

    /// Where the toolbar starts along the axis for `share` of the room, pushed inside a monitor too small for it.
    fn place(&self, share: f64) -> i32 {
        let placed = self
            .first()
            .saturating_add_unsigned(pixels(share, self.room()));
        placed.min(
            self.start
                .saturating_add_unsigned(self.length.saturating_sub(self.size)),
        )
    }

    /// The share of the room that starts the toolbar along the axis as near `wanted` as it can go: a start wanted before
    /// the margin goes to the margin, and one past the room to its end. `share` itself when the toolbar has no room to
    /// move, or would start on the pixel `share` already puts it at.
    fn share_nearest(&self, wanted: i32, share: f64) -> f64 {
        let room = self.room();
        if room == 0 {
            return share;
        }
        let moved = u32::try_from(wanted.saturating_sub(self.first()))
            .unwrap_or(0)
            .min(room);
        if moved == pixels(share, room) {
            share
        } else {
            f64::from(moved) / f64::from(room)
        }
    }
}

/// `share` of `room`, in whole pixels, a half rounded down: the centered toolbar leaves an odd pixel on its right, and
/// a share [`place_nearest`] made from a whole pixel comes back to that pixel, a hair over or under as it may be.
fn pixels(share: f64, room: u32) -> u32 {
    let whole = (share * f64::from(room) - 0.5)
        .ceil()
        .clamp(0.0, f64::from(room));
    // `as` drops nothing here: `whole` is a whole number from 0 to `room`, which a u32 holds. TODO: use a checked
    // float-to-integer conversion should the standard library get one.
    whole as u32
}

/// Where the toolbar's parts are with its top left corner at `corner`, at `scale`.
pub fn layout(corner: Point, scale: Scale) -> Layout {
    let s = |logical| scaled(logical, scale.dpi());
    let ring = s(RING);
    let sizes = Sizes::at(scale);
    let Point { x: left, y: top } = corner;
    // The smaller groups are centered in the toolbar's height (CSS `align-items: center`), an odd pixel left below them.
    let small_top =
        top.saturating_add_unsigned(sizes.height.saturating_sub(sizes.small_height).div_ceil(2));

    let tools = sized(left, top, sizes.tools_width, sizes.height);
    let modes = sized(
        after(tools, sizes.gap),
        small_top,
        sizes.modes_width,
        sizes.small_height,
    );
    let controls = sized(
        after(modes, sizes.gap),
        small_top,
        sizes.controls_width,
        sizes.small_height,
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
        toolbar: sized(left, top, sizes.width(), sizes.height),
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
