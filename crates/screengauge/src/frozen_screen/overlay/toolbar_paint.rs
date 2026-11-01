//! Paints the toolbar (v1 spec WPC-296, Toolbar; WPC-310) onto a Direct2D render target the way the prototype's CSS does
//! (variant A of WPC-279: `.grp`, `.tbtn`, `.mbtn`, `.sbtn`), at the monitor's scale. Where each part goes is `toolbar`'s
//! layout; its colors are `toolbar_palette`'s; a blurred shadow is layers of rounded rectangles, `shadow`'s.
//!
//! CSS paints a box's outer shadows, its background, its border and then its inset shadows, and a button after another
//! one in order, so each button is painted whole and a later one covers an earlier one's glow. A group is painted whole
//! before the next, so one group's shadow falls over the edge of the one before it, as in a browser. An outer shadow is
//! never painted inside its own box: where the box is see-through, as the groups' panels are, it is cut out.
//!
//! The buttons react to the mouse as the prototype's do (WPC-311): a tool's button rises on `:hover` and sinks and
//! shrinks on `:active`; the gear's and minimize's icon brightens on `:hover`, the close button turns red, and the three
//! shrink on `:active`; H, V and C don't react. The minimized toolbar is `#tbMini`: a lit button with the active tool's
//! icon and a small arrow in its corner.
//!
//! Left out on purpose: `backdrop-filter: blur(14px)` of the panels, which the owner agreed to do without, and the
//! 0.12 s the prototype takes to move and shrink a button, which here happens at once.

use screengauge_core::session::toolbar::Hit;
use screengauge_core::session::{Point, Toolbar, ToolbarButton};
use screengauge_core::settings::Tool;
use windows::Win32::Foundation::{E_FAIL, RECT};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_RECT_F, D2D_SIZE_F, D2D1_FIGURE_BEGIN_HOLLOW, D2D1_FIGURE_END_CLOSED, D2D1_FIGURE_END_OPEN,
    D2D1_GRADIENT_STOP,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_ARC_SEGMENT, D2D1_ARC_SIZE_LARGE, D2D1_ARC_SIZE_SMALL,
    D2D1_ELLIPSE, D2D1_EXTEND_MODE_CLAMP, D2D1_GAMMA_2_2, D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES,
    D2D1_ROUNDED_RECT, D2D1_SWEEP_DIRECTION_CLOCKWISE, D2D1_SWEEP_DIRECTION_COUNTER_CLOCKWISE,
    ID2D1PathGeometry, ID2D1RenderTarget, ID2D1RoundedRectangleGeometry, ID2D1SolidColorBrush,
    ID2D1StrokeStyle,
};
use windows::core::Error;

use super::geometry::scaled;
use super::palette::{Color, color, d2d_color, solid_brush};
use super::shadow::{self, Profile};
use super::svg_path::{self, Figure, Pt, Segment};
use super::toolbar::{ButtonKind, Layout, RING, Reaction, is_lit, kind, reaction};
use super::toolbar_icons::{self, Icon, Shape, VIEW_BOX};
use super::toolbar_palette::{
    self, CLOSE_HOVER, CLOSE_INK_HOVER, OPEN_BOX, OPEN_INK, Palette, Shadow, Stop, Surface,
};
use super::{Overlay, dot, in_surface, px, ring, rounded_at};

// Every size below is in logical pixels, which `scaled` turns into physical pixels for the monitor's scale.

/// The corners of a group's panel (`.grp`), of a tool's button (`.tbtn`) and of the small buttons (`.mbtn`, `.sbtn`).
const GROUP_RADIUS: u32 = 20;
const TOOL_RADIUS: u32 = 13;
const SMALL_RADIUS: u32 = 10;
/// The minimized toolbar's button (`#tbMini`), and the box of the small arrow in its corner (`.chev`): its side, its
/// corners, and how far it is from the button's right and bottom sides.
const MINIMIZED_RADIUS: u32 = 15;
const OPEN_SIDE: u32 = 14;
const OPEN_RADIUS: u32 = 5;
const OPEN_INSET: u32 = 3;
/// How far a tool's button rises with the cursor on it (`.tbtn:hover`), and sinks while held down (`.tbtn:active`),
/// and how much a held tool's and small button shrink (`.tbtn:active` and `.sbtn:active`).
const LIFT: i32 = 2;
const SINK: i32 = 1;
const TOOL_PRESSED: f32 = 0.95;
const SMALL_PRESSED: f32 = 0.92;

/// How a kind of button is painted: its surface, and the radius of its corners.
fn style(palette: &Palette, kind: ButtonKind) -> (&Surface, u32) {
    match kind {
        ButtonKind::Tool => (&palette.button, TOOL_RADIUS),
        ButtonKind::Mode => (&palette.button, SMALL_RADIUS),
        ButtonKind::Control => (&palette.small, SMALL_RADIUS),
        ButtonKind::Minimized => (&palette.lit, MINIMIZED_RADIUS),
    }
}

/// How a button reacting to the mouse moves and shrinks, as its CSS `transform`: `scale` around its middle, then
/// `down` logical pixels down (up when below zero).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Motion {
    scale: f32,
    down: i32,
}

/// How a button of `kind` reacting as `reaction` moves, or `None` when it stays where it is.
fn motion(kind: ButtonKind, reaction: Reaction) -> Option<Motion> {
    match (kind, reaction) {
        (ButtonKind::Tool, Reaction::Hovered) => Some(Motion {
            scale: 1.0,
            down: -LIFT,
        }),
        (ButtonKind::Tool, Reaction::Pressed) => Some(Motion {
            scale: TOOL_PRESSED,
            down: SINK,
        }),
        (ButtonKind::Control, Reaction::Pressed) => Some(Motion {
            scale: SMALL_PRESSED,
            down: 0,
        }),
        _ => None,
    }
}

/// `logical` pixels of the monitor's scale, whole, for a length that may be negative, as a shadow's spread and drop may.
fn signed(logical: i32, dpi: u32) -> i32 {
    logical.signum() * scaled(logical.unsigned_abs(), dpi)
}

/// The standard deviation, in physical pixels, of a CSS blur radius of `blur` logical pixels: half of it.
fn sigma(blur: u32, dpi: u32) -> f64 {
    f64::from(blur) * f64::from(dpi) / 192.0
}

/// An outer shadow of a box in physical pixels: how much it is grown, how far it drops, and its layers, which are
/// fitted across the box's short side after the spread.
struct OuterShadow {
    spread: i32,
    drop: i32,
    layers: shadow::Layers,
}

impl OuterShadow {
    /// `None` when the spread leaves nothing of the box.
    fn of(shadow: &Shadow, rect: &RECT, dpi: u32) -> Option<OuterShadow> {
        let spread = signed(shadow.spread, dpi);
        let short = (rect.right - rect.left).min(rect.bottom - rect.top) + 2 * spread;
        if short <= 0 {
            return None;
        }
        let layers = shadow::layers(
            sigma(shadow.blur, dpi),
            shadow.color.alpha,
            Profile::Strip(f64::from(short)),
        );
        Some(OuterShadow {
            spread,
            drop: signed(shadow.y, dpi),
            layers,
        })
    }
}

/// How far the toolbar's shadows reach past its rectangle on each side, in physical pixels: nothing is drawn beyond.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) struct Margins {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

/// The reach of every outer shadow of every box of the toolbar at `layout`, whichever buttons are lit, hovered or held
/// down, whether or not H, V and C show, and of the minimized toolbar's button, so one bitmap holds them all.
pub(super) fn reach(palette: &Palette, layout: &Layout, dpi: u32) -> Margins {
    let mut margins = Margins::default();
    let toolbar = layout.toolbar;
    let mut take = |surfaces: &[&Surface], rect: &RECT| {
        for outer in surfaces
            .iter()
            .flat_map(|surface| surface.outer)
            .filter_map(|shadow| OuterShadow::of(shadow, rect, dpi))
        {
            // The shadow's outline is the box moved down by the drop and grown by the spread and the last layer.
            let grown = outer.spread + outer.layers.reach();
            margins.left = margins.left.max(toolbar.left - (rect.left - grown));
            margins.right = margins.right.max(rect.right + grown - toolbar.right);
            margins.top = margins
                .top
                .max(toolbar.top - (rect.top + outer.drop - grown));
            margins.bottom = margins
                .bottom
                .max(rect.bottom + outer.drop + grown - toolbar.bottom);
        }
    };
    for group in [&layout.tools, &layout.modes, &layout.controls] {
        take(&[&palette.group], group);
    }
    for (button, place) in &layout.buttons {
        let (surface, _) = style(palette, kind(*button));
        take(&[surface, &palette.lit, &CLOSE_HOVER], place);
        // A tool's button risen with the cursor on it casts its shadows from higher up. Held down, it shrinks, and sinks
        // by less than it shrinks, so its shadows stay inside those it casts at rest.
        if kind(*button) == ButtonKind::Tool {
            let lift = signed(LIFT, dpi);
            let risen = RECT {
                top: place.top - lift,
                bottom: place.bottom - lift,
                ..*place
            };
            take(&[surface, &palette.lit], &risen);
        }
    }
    let (minimized, _) = style(palette, ButtonKind::Minimized);
    take(&[minimized], &layout.minimized);
    margins
}

/// What paints on one render target. `origin` is the point of the virtual screen at the target's pixel (0, 0): the window's
/// monitor when the target is the window, or where a bitmap kept for the toolbar starts.
struct Painter<'a> {
    overlay: &'a Overlay,
    target: &'a ID2D1RenderTarget,
    palette: &'static Palette,
    origin: Point,
    /// Recolored for every fill, so the layers of a shadow share one.
    brush: ID2D1SolidColorBrush,
}

/// Where an icon's view box is on the target and how big a unit of it is, to turn its points into pixels.
struct Placement {
    left: f32,
    top: f32,
    unit: f32,
}

impl Placement {
    /// `p` in pixels, as the ellipse of no size Direct2D's calls take their points from (see `dot`).
    fn point(&self, p: Pt) -> D2D1_ELLIPSE {
        dot(self.left + p.x * self.unit, self.top + p.y * self.unit)
    }
}

impl Overlay {
    /// Paints `toolbar` at `layout` onto `target`, whose pixel (0, 0) is the virtual screen's `origin`: for each group
    /// its panel and then its buttons, the ones the scene lights lit and the one under the cursor reacting to it, with
    /// `held` what the left button is held down on. H, V and C are left out while `toolbar.mode` is `None`: their room
    /// stays as it was. Minimized, it is the minimized toolbar's button alone. Between BeginDraw and EndDraw.
    pub(super) fn paint_toolbar(
        &self,
        target: &ID2D1RenderTarget,
        layout: &Layout,
        toolbar: &Toolbar,
        held: Option<Hit>,
        origin: Point,
    ) -> windows::core::Result<()> {
        let painter = Painter {
            overlay: self,
            target,
            palette: toolbar_palette::palette(self.mode),
            origin,
            brush: solid_brush(target, color(0, 0, 0, 1.0))?,
        };
        // SAFETY: a drawing call on a live render target. Out of BeginDraw and EndDraw, Direct2D only fails the frame
        // at EndDraw. The corners need antialiasing, which the target may have been left without.
        unsafe { target.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE) };
        if toolbar.minimized {
            return painter.minimized(&layout.minimized, toolbar.active);
        }
        painter.group(layout, &layout.tools, toolbar, held, ButtonKind::Tool)?;
        if toolbar.mode.is_some() {
            painter.group(layout, &layout.modes, toolbar, held, ButtonKind::Mode)?;
        }
        painter.group(layout, &layout.controls, toolbar, held, ButtonKind::Control)
    }
}

impl Painter<'_> {
    fn dpi(&self) -> u32 {
        self.overlay.dpi
    }

    /// `logical` pixels of the monitor's scale, whole.
    fn scaled(&self, logical: u32) -> i32 {
        scaled(logical, self.dpi())
    }

    fn signed(&self, logical: i32) -> i32 {
        signed(logical, self.dpi())
    }

    fn sigma(&self, blur: u32) -> f64 {
        sigma(blur, self.dpi())
    }

    /// `rect`, grown by `grow` pixels on every side and moved `down`, as a rounded rectangle in the target's own pixels.
    /// Shadow layers and holes shrink a box by up to three deviations of the blur, which may pass half its height, so
    /// what shrinks past nothing is clamped.
    fn shape(&self, rect: &RECT, radius: i32, grow: i32, down: i32) -> D2D1_ROUNDED_RECT {
        rounded_at(self.origin, rect, radius, grow, down, true)
    }

    /// Sets the shared brush to `rgb` at `alpha`.
    fn set(&self, rgb: [u8; 3], alpha: f32) {
        // SAFETY: the color outlives the call, which keeps nothing of it.
        unsafe { self.brush.SetColor(&d2d_color(rgb, alpha)) };
    }

    fn fill(&self, shape: &D2D1_ROUNDED_RECT, color: Color) {
        self.set(color.rgb, color.alpha);
        // SAFETY: a drawing call on a live render target with arguments that outlive it. Out of BeginDraw and EndDraw,
        // Direct2D only fails the frame at EndDraw.
        unsafe { self.target.FillRoundedRectangle(shape, &self.brush) };
    }

    /// A group's panel, and then its buttons in the toolbar's order.
    fn group(
        &self,
        layout: &Layout,
        group: &RECT,
        toolbar: &Toolbar,
        held: Option<Hit>,
        members: ButtonKind,
    ) -> windows::core::Result<()> {
        // The panel is a little see-through, so what falls under it is cut out of its shadow.
        self.surface(&self.palette.group, group, GROUP_RADIUS, RING, true)?;
        for (button, place) in layout
            .buttons
            .iter()
            .filter(|(button, _)| kind(*button) == members)
        {
            let reacts = reaction(toolbar, held, *button);
            let lit = is_lit(toolbar, *button);
            match motion(members, reacts) {
                Some(motion) => {
                    self.in_motion(place, motion, || self.button(button, place, lit, reacts))?;
                }
                None => self.button(button, place, lit, reacts)?,
            }
        }
        Ok(())
    }

    /// One button: its own box and, when it is lit, the lit box over it, which hides it and shows its glow around it.
    /// With the cursor on it, or held down, the close button is red and the gear's and minimize's icon brighter.
    fn button(
        &self,
        button: &ToolbarButton,
        place: &RECT,
        lit: bool,
        reacts: Reaction,
    ) -> windows::core::Result<()> {
        let pointed = reacts != Reaction::Rest;
        let kind = kind(*button);
        let (style, radius) = style(self.palette, kind);
        let (surface, ink) = match (button, kind) {
            (ToolbarButton::Close, _) if pointed => (&CLOSE_HOVER, CLOSE_INK_HOVER),
            (_, ButtonKind::Control) if pointed => (style, self.palette.small_ink_hover),
            (_, ButtonKind::Control) => (style, self.palette.small_ink),
            (_, ButtonKind::Tool | ButtonKind::Mode) => (style, self.palette.ink),
            (_, ButtonKind::Minimized) => (style, self.palette.ink_on),
        };
        self.surface(surface, place, radius, 0, false)?;
        if lit {
            self.surface(&self.palette.lit, place, radius, 0, false)?;
        }
        // The icon is over the lit box, in the lit ink.
        let ink = if lit { self.palette.ink_on } else { ink };
        match toolbar_icons::icon_for(button) {
            Some(icon) => self.icon(icon, place, ink),
            None => Ok(()),
        }
    }

    /// The minimized toolbar's button at `place`, with `active` the active tool: lit, with the tool's icon in the middle
    /// and, in its bottom right corner, a small dark box with a white arrow pointing right.
    fn minimized(&self, place: &RECT, active: Tool) -> windows::core::Result<()> {
        let (surface, radius) = style(self.palette, ButtonKind::Minimized);
        self.surface(surface, place, radius, 0, false)?;
        if let Some(icon) = toolbar_icons::icon_for(&ToolbarButton::Tool(active)) {
            self.icon(icon, place, self.palette.ink_on)?;
        }
        let side = self.scaled(OPEN_SIDE);
        let inset = self.scaled(OPEN_INSET);
        let corner = RECT {
            left: place.right - inset - side,
            top: place.bottom - inset - side,
            right: place.right - inset,
            bottom: place.bottom - inset,
        };
        self.fill(
            &self.shape(&corner, self.scaled(OPEN_RADIUS), 0, 0),
            OPEN_BOX,
        );
        self.icon(&toolbar_icons::OPEN, &corner, OPEN_INK)
    }

    /// Runs `paint` with the target moved and shrunk as `motion` says, around the middle of `place`, as CSS transforms
    /// a box and all it draws, shadows included. The target's own transform is put back afterwards, whatever `paint` did.
    fn in_motion(
        &self,
        place: &RECT,
        motion: Motion,
        paint: impl FnOnce() -> windows::core::Result<()>,
    ) -> windows::core::Result<()> {
        let area = in_surface(self.origin, place);
        let (x, y) = (
            (area.left + area.right) / 2.0,
            (area.top + area.bottom) / 2.0,
        );
        // The windows crate takes its matrix from windows-numerics, which ScreenGauge doesn't name as a dependency: the
        // target's own transform is a value of that type, and the move is a copy of it with every entry set.
        let mut before = Default::default();
        // SAFETY: fills a local.
        unsafe { self.target.GetTransform(&mut before) };
        let mut moving = before;
        // A point p goes to (x, y) + scale × (p − (x, y)), then `down` pixels down. Direct2D multiplies a row vector by
        // the matrix, so the last row is the move.
        moving.M11 = motion.scale;
        moving.M12 = 0.0;
        moving.M21 = 0.0;
        moving.M22 = motion.scale;
        moving.M31 = x * (1.0 - motion.scale);
        moving.M32 = y * (1.0 - motion.scale) + px(self.signed(motion.down));
        // The button's own motion first, then whatever the target was already doing.
        let moved_then_before = moving * before;
        // SAFETY: the matrix outlives the call, which copies it.
        unsafe { self.target.SetTransform(&moved_then_before) };
        let painted = paint();
        // SAFETY: as above.
        unsafe { self.target.SetTransform(&before) };
        painted
    }

    /// `icon` in `ink`, centered in `place`: its view box at its CSS size, scaled for the monitor, with its top left
    /// corner on a whole pixel (a half pixel of room goes to the left and top), and its strokes round at their ends and
    /// corners.
    fn icon(&self, icon: &Icon, place: &RECT, ink: Color) -> windows::core::Result<()> {
        let side = self.scaled(icon.size);
        let corner = in_surface(self.origin, place);
        let at = Placement {
            left: corner.left + px((place.right - place.left - side + 1) / 2),
            top: corner.top + px((place.bottom - place.top - side + 1) / 2),
            unit: px(side) / VIEW_BOX,
        };
        self.set(ink.rgb, ink.alpha);
        for shape in icon.shapes {
            self.shape_of(icon, shape, &at)?;
        }
        Ok(())
    }

    /// One piece of `icon`, stroked at the icon's width.
    fn shape_of(&self, icon: &Icon, shape: &Shape, at: &Placement) -> windows::core::Result<()> {
        let width = icon.stroke * at.unit;
        let style = &self.overlay.stroke;
        match *shape {
            Shape::Path(d) => {
                let figures = svg_path::parse(d)
                    .map_err(|e| Error::new(E_FAIL, format!("a toolbar icon: {e}")))?;
                let geometry = self.path_geometry(&figures, at)?;
                // SAFETY: a drawing call on a live render target with arguments that outlive it. Out of BeginDraw and
                // EndDraw, Direct2D only fails the frame at EndDraw.
                unsafe {
                    self.target
                        .DrawGeometry(&geometry, &self.brush, width, style)
                };
            }
            Shape::Circle { cx, cy, r } => {
                let mut circle = at.point(Pt { x: cx, y: cy });
                circle.radiusX = r * at.unit;
                circle.radiusY = r * at.unit;
                // SAFETY: as above.
                unsafe { self.target.DrawEllipse(&circle, &self.brush, width, style) };
            }
            Shape::Rect {
                x,
                y,
                width: w,
                height: h,
                radius,
            } => {
                let corner = at.point(Pt { x, y });
                let far = at.point(Pt { x: x + w, y: y + h });
                let rounded = D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F {
                        left: corner.point.X,
                        top: corner.point.Y,
                        right: far.point.X,
                        bottom: far.point.Y,
                    },
                    radiusX: radius * at.unit,
                    radiusY: radius * at.unit,
                };
                // SAFETY: as above.
                unsafe {
                    self.target
                        .DrawRoundedRectangle(&rounded, &self.brush, width, style);
                }
            }
        }
        Ok(())
    }

    /// `figures`, in an icon's own units, as Direct2D geometry in the target's pixels. A figure no closepath ended is
    /// left open, so its ends get caps.
    fn path_geometry(
        &self,
        figures: &[Figure],
        at: &Placement,
    ) -> windows::core::Result<ID2D1PathGeometry> {
        // SAFETY: no arguments.
        let geometry = unsafe { self.overlay.factory.CreatePathGeometry() }?;
        // SAFETY: no arguments.
        let sink = unsafe { geometry.Open() }?;
        for figure in figures {
            // SAFETY: calls on the sink opened above, which is closed below, once. Each figure is begun and ended in
            // turn; Direct2D reports a misuse when the sink is closed.
            unsafe {
                sink.BeginFigure(at.point(figure.start).point, D2D1_FIGURE_BEGIN_HOLLOW);
                for segment in &figure.segments {
                    match *segment {
                        Segment::Line(to) => sink.AddLine(at.point(to).point),
                        Segment::Arc(arc) => sink.AddArc(&D2D1_ARC_SEGMENT {
                            point: at.point(arc.to).point,
                            size: D2D_SIZE_F {
                                width: arc.radii.x * at.unit,
                                height: arc.radii.y * at.unit,
                            },
                            rotationAngle: arc.rotation,
                            sweepDirection: if arc.sweep {
                                D2D1_SWEEP_DIRECTION_CLOCKWISE
                            } else {
                                D2D1_SWEEP_DIRECTION_COUNTER_CLOCKWISE
                            },
                            arcSize: if arc.large {
                                D2D1_ARC_SIZE_LARGE
                            } else {
                                D2D1_ARC_SIZE_SMALL
                            },
                        }),
                    }
                }
                sink.EndFigure(if figure.closed {
                    D2D1_FIGURE_END_CLOSED
                } else {
                    D2D1_FIGURE_END_OPEN
                });
            }
        }
        // SAFETY: closes the sink opened above, once.
        unsafe { sink.Close() }?;
        Ok(geometry)
    }

    /// A box painted as CSS does: the outer shadows, the background, the border and the inset shadows, the last listed of
    /// each kind first. `ring_width` is the border's width and `radius` the corners', in logical pixels, and `see_through`
    /// is whether the shadow is cut out under the box, as it is for a background that lets what is under it show.
    fn surface(
        &self,
        surface: &Surface,
        rect: &RECT,
        radius: u32,
        ring_width: u32,
        see_through: bool,
    ) -> windows::core::Result<()> {
        let radius = self.scaled(radius);
        let ring_width = self.scaled(ring_width);
        let outer = self.shape(rect, radius, 0, 0);
        let hole = if see_through {
            Some(self.overlay.geometry_of(&outer)?)
        } else {
            None
        };
        for shadow in surface.outer.iter().rev() {
            self.outer_shadow(shadow, rect, radius, hole.as_ref())?;
        }
        // The background reaches under the border; a gradient runs from the top of the padding box to its bottom.
        if let Some(base) = surface.base {
            self.fill(&outer, base);
        }
        if !surface.gradient.is_empty() {
            self.gradient(
                &outer,
                surface.gradient,
                outer.rect.top + px(ring_width),
                outer.rect.bottom - px(ring_width),
            )?;
        }
        if let Some(border) = surface.border {
            self.set(border.rgb, border.alpha);
            let width = px(ring_width);
            // SAFETY: a drawing call on a live render target with arguments that outlive it, as in `fill`.
            unsafe {
                self.target.DrawRoundedRectangle(
                    &ring(&outer, width),
                    &self.brush,
                    width,
                    None::<&ID2D1StrokeStyle>,
                );
            }
        }
        // Inset shadows are inside the border, in the padding box.
        let padding = RECT {
            left: rect.left + ring_width,
            top: rect.top + ring_width,
            right: rect.right - ring_width,
            bottom: rect.bottom - ring_width,
        };
        for shadow in surface.inset.iter().rev() {
            self.inset_shadow(shadow, &padding, (radius - ring_width).max(0))?;
        }
        Ok(())
    }

    /// An outer shadow of the box `rect`: the box moved down by the shadow's drop and grown by its spread, blurred.
    /// `hole`, when there is one, is cut out of every layer.
    fn outer_shadow(
        &self,
        shadow: &Shadow,
        rect: &RECT,
        radius: i32,
        hole: Option<&ID2D1RoundedRectangleGeometry>,
    ) -> windows::core::Result<()> {
        let Some(outer) = OuterShadow::of(shadow, rect, self.dpi()) else {
            return Ok(());
        };
        let (spread, drop) = (outer.spread, outer.drop);
        for layer in &outer.layers.layers {
            let grow = spread + layer.grow;
            // A layer that stays inside the box is under it, or cut out of it: never seen.
            if grow <= -drop.abs() {
                continue;
            }
            self.set(shadow.color.rgb, layer.opacity);
            let layer = self.shape(rect, radius, grow, drop);
            match hole {
                Some(hole) => self
                    .overlay
                    .fill_except(self.target, &layer, hole, &self.brush)?,
                // SAFETY: a drawing call on a live render target with arguments that outlive it, as in `fill`.
                None => unsafe { self.target.FillRoundedRectangle(&layer, &self.brush) },
            }
        }
        Ok(())
    }

    /// An inset shadow of the box `padding`: the box less its own shape moved by the shadow's drop, smaller by its
    /// spread, blurred. The blur moves the hole's outline in and out, a layer at a time, and what is filled is always
    /// the box less the hole, so nothing reaches beyond the box.
    fn inset_shadow(
        &self,
        shadow: &Shadow,
        padding: &RECT,
        radius: i32,
    ) -> windows::core::Result<()> {
        let spread = self.signed(shadow.spread);
        let drop = self.signed(shadow.y);
        let layers = shadow::layers(self.sigma(shadow.blur), shadow.color.alpha, Profile::Edge);
        let inside = self.shape(padding, radius, 0, 0);
        for layer in &layers.layers {
            // A layer's reach is how far into the hole the shadow goes: the hole is that much smaller.
            let hole = self.overlay.geometry_of(&self.shape(
                padding,
                radius,
                -spread - layer.grow,
                drop,
            ))?;
            self.set(shadow.color.rgb, layer.opacity);
            self.overlay
                .fill_except(self.target, &inside, &hole, &self.brush)?;
        }
        Ok(())
    }

    /// `shape` filled with `stops` running from `top` to `bottom`, in the target's pixels. Colors blend in sRGB, as
    /// CSS does, and the first and last colors go on past the ends.
    fn gradient(
        &self,
        shape: &D2D1_ROUNDED_RECT,
        stops: &[Stop],
        top: f32,
        bottom: f32,
    ) -> windows::core::Result<()> {
        let stops: Vec<D2D1_GRADIENT_STOP> = stops
            .iter()
            .map(|stop| D2D1_GRADIENT_STOP {
                position: stop.at,
                color: d2d_color(stop.color.rgb, stop.color.alpha),
            })
            .collect();
        // SAFETY: the stops outlive the call, which copies them.
        let collection = unsafe {
            self.target
                .CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)
        }?;
        let mut line = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES::default();
        line.startPoint.X = shape.rect.left;
        line.startPoint.Y = top;
        line.endPoint.X = shape.rect.left;
        line.endPoint.Y = bottom;
        // SAFETY: the line and the collection outlive the call, which keeps its own reference to the collection.
        let brush = unsafe {
            self.target
                .CreateLinearGradientBrush(&line, None, &collection)
        }?;
        // SAFETY: a drawing call on a live render target with arguments that outlive it, as in `fill`.
        unsafe { self.target.FillRoundedRectangle(shape, &brush) };
        Ok(())
    }
}
