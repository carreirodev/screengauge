//! The toolbar on screen (v1 spec WPC-296, Toolbar; WPC-310 and WPC-311): where the session core's layout puts each of
//! its parts, as the rectangles Windows and the painting use, how each button is styled, and drawing it.
//!
//! Where the toolbar and each button are is the core's ([`screengauge_core::session::toolbar`]), which also says what a
//! point is on, so the window's clicks, the cursor's arrow, the pause of the tool and what is drawn here never disagree.

use screengauge_core::session::toolbar::{self as place, Hit};
use screengauge_core::session::{Rect, Scale, Toolbar, ToolbarButton};
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Direct2D::ID2D1RenderTarget;

use super::Overlay;
use super::geometry;

pub use place::RING;

/// How a button is styled and which group it is in: the tools' big buttons, the modes (H, V and C), the small buttons
/// that act on the toolbar itself (the gear, minimize and close), and the button the minimized toolbar becomes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ButtonKind {
    Tool,
    Mode,
    Control,
    Minimized,
}

/// What `button` is. A button is listed here, and a new one fails to compile until it is, so the painting never styles
/// it by default.
pub(super) fn kind(button: ToolbarButton) -> ButtonKind {
    match button {
        ToolbarButton::Tool(_) => ButtonKind::Tool,
        ToolbarButton::Mode(_) => ButtonKind::Mode,
        ToolbarButton::Settings | ToolbarButton::Minimize | ToolbarButton::Close => {
            ButtonKind::Control
        }
        ToolbarButton::Restore => ButtonKind::Minimized,
    }
}

/// Whether `toolbar` draws `button` lit: the active tool's, and the mode's while there is one. The minimized toolbar's
/// button is cyan whatever is active, as its own style.
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

/// How a button reacts to the mouse, as the prototype's do to `:hover` and `:active`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Reaction {
    Rest,
    /// The cursor is on it, and no button is held down.
    Hovered,
    /// It is held down and the cursor is on it, so letting go clicks it. Moved off it, it looks at rest again, as a
    /// Windows button does, and letting go does nothing (WPC-310).
    Pressed,
}

/// How `button` reacts when the cursor is on `toolbar.hovered` and `held` is what the left button went down on, while it
/// is held. While a button is held, the others don't react to the cursor.
pub(super) fn reaction(toolbar: &Toolbar, held: Option<Hit>, button: ToolbarButton) -> Reaction {
    if toolbar.hovered != Some(button) {
        return Reaction::Rest;
    }
    match held {
        None => Reaction::Hovered,
        Some(Hit::Button(pressed)) if pressed == button => Reaction::Pressed,
        Some(_) => Reaction::Rest,
    }
}

/// Where the toolbar's parts are, in physical pixels of the virtual screen, each rectangle's right and bottom one past
/// its last pixel: the core's [`place::Layout`], as Windows has rectangles.
#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    /// The whole toolbar, the room between its groups included.
    pub toolbar: RECT,
    /// The tools' group, as tall as the toolbar.
    pub tools: RECT,
    /// The room kept for H, V and C, whose group shows only while Dimensions is the active tool.
    pub modes: RECT,
    /// The gear, minimize and close's group.
    pub controls: RECT,
    /// Each button of [`BUTTONS`], in the same order.
    pub buttons: [(ToolbarButton, RECT); 10],
    /// The button the minimized toolbar becomes, at the toolbar's top left corner.
    pub minimized: RECT,
}

impl Layout {
    /// Where the core puts the toolbar on `monitor`, at `scale`.
    pub fn of(monitor: Rect, scale: Scale) -> windows::core::Result<Layout> {
        let core = place::layout(monitor, scale);
        let mut buttons = core.buttons.map(|(button, _)| (button, RECT::default()));
        for ((_, slot), (_, rect)) in buttons.iter_mut().zip(core.buttons) {
            *slot = geometry::bounds(rect)?;
        }
        Ok(Layout {
            toolbar: geometry::bounds(core.toolbar)?,
            tools: geometry::bounds(core.tools)?,
            modes: geometry::bounds(core.modes)?,
            controls: geometry::bounds(core.controls)?,
            buttons,
            minimized: geometry::bounds(core.minimized)?,
        })
    }
}

impl Overlay {
    /// Draws `toolbar` over the measurements and under the tolerance indicator and the notice, between BeginDraw and
    /// EndDraw, when it is on this window's monitor: each group's panel and its buttons, in the palette of the window,
    /// with the active tool's button and the mode's lit and the buttons reacting to the cursor, or the minimized
    /// toolbar's button alone. `held` is what on the toolbar the left button went down on, while it is held. They are
    /// drawn once on a bitmap that the next frames lay over the photo, until something on the toolbar changes
    /// (`toolbar_cache`).
    pub fn draw_toolbar(
        &self,
        target: &ID2D1RenderTarget,
        toolbar: &Toolbar,
        held: Option<Hit>,
    ) -> windows::core::Result<()> {
        self.show_toolbar(target, &self.toolbar_layout, toolbar, held)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use screengauge_core::settings::Tool;

    #[test]
    fn the_layout_is_the_cores_with_each_right_and_bottom_one_past_the_last_pixel() {
        let monitor = Rect {
            left: 3840,
            top: 0,
            width: 1920,
            height: 1080,
        };
        let scale = Scale::from_dpi(120).expect("a scale");
        let core = place::layout(monitor, scale);
        let layout = Layout::of(monitor, scale).expect("a monitor's size");
        let edges = |rect: Rect| (rect.left, rect.top, rect.right() + 1, rect.bottom() + 1);
        let sides = |rect: RECT| (rect.left, rect.top, rect.right, rect.bottom);
        assert_eq!(sides(layout.toolbar), edges(core.toolbar));
        assert_eq!(sides(layout.minimized), edges(core.minimized));
        for ((button, rect), (core_button, core_rect)) in
            layout.buttons.into_iter().zip(core.buttons)
        {
            assert_eq!((button, sides(rect)), (core_button, edges(core_rect)));
        }
    }

    fn toolbar(hovered: Option<ToolbarButton>) -> Toolbar {
        Toolbar {
            monitor: 0,
            active: Tool::Dimensions,
            mode: None,
            minimized: false,
            hovered,
        }
    }

    #[test]
    fn the_hovered_button_reacts_and_the_held_one_looks_pressed_only_while_the_cursor_is_on_it() {
        let inspect = ToolbarButton::Tool(Tool::Inspect);
        let close = ToolbarButton::Close;
        let on_inspect = toolbar(Some(inspect));
        assert_eq!(reaction(&on_inspect, None, inspect), Reaction::Hovered);
        assert_eq!(reaction(&on_inspect, None, close), Reaction::Rest);
        assert_eq!(
            reaction(&on_inspect, Some(Hit::Button(inspect)), inspect),
            Reaction::Pressed
        );
        // Held down on the close button and moved onto Inspect: neither reacts.
        assert_eq!(
            reaction(&on_inspect, Some(Hit::Button(close)), inspect),
            Reaction::Rest
        );
        assert_eq!(
            reaction(&toolbar(None), Some(Hit::Button(close)), close),
            Reaction::Rest
        );
        // Held down on the background: the buttons don't react while it is held.
        assert_eq!(
            reaction(&on_inspect, Some(Hit::Background), inspect),
            Reaction::Rest
        );
    }
}
