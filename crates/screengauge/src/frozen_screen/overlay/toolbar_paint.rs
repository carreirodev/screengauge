//! Paints the toolbar (v1 spec WPC-296, Toolbar; WPC-310) onto a Direct2D render target the way the prototype's CSS does
//! (variant A of WPC-279: `.grp`, `.tbtn`, `.mbtn`, `.sbtn`), at the monitor's scale. Where each part goes is `toolbar`'s
//! layout; its colors are `toolbar_palette`'s; a blurred shadow is layers of rounded rectangles, `shadow`'s.
//!
//! CSS paints a box's outer shadows, its background, its border and then its inset shadows, and a button after another
//! one in order, so each button is painted whole and a later one covers an earlier one's glow. A group is painted whole
//! before the next, so one group's shadow falls over the edge of the one before it, as in a browser. An outer shadow is
//! never painted inside its own box: where the box is see-through, as the groups' panels are, it is cut out.
//!
//! Left out on purpose: `backdrop-filter: blur(14px)` of the panels, which the owner agreed to do without, and the
//! reactions to the mouse hovering and pressing (WPC-311).

use screengauge_core::session::{Toolbar, ToolbarButton};
use windows::Win32::Foundation::{E_FAIL, RECT};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_RECT_F, D2D_SIZE_F, D2D1_BEZIER_SEGMENT, D2D1_COLOR_F, D2D1_FIGURE_BEGIN_HOLLOW,
    D2D1_FIGURE_END_CLOSED, D2D1_FIGURE_END_OPEN, D2D1_GRADIENT_STOP,
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
use super::shadow::{self, Profile};
use super::svg_path::{self, Figure, Pt, Segment};
use super::toolbar::Layout;
use super::toolbar_icons::{self, Icon, Shape, VIEW_BOX};
use super::toolbar_palette::{self, Palette, Rgba, Shadow, Stop, Surface};
use super::{Overlay, dot, px, ring};

// Every size below is in logical pixels, which `scaled` turns into physical pixels for the monitor's scale.

/// The corners of a group's panel (`.grp`), of a tool's button (`.tbtn`) and of the small buttons (`.mbtn`, `.sbtn`).
const GROUP_RADIUS: u32 = 20;
const TOOL_RADIUS: u32 = 13;
const SMALL_RADIUS: u32 = 10;
/// A group's border (`.grp`'s `border: 1px`), which the layout leaves room for.
const BORDER: u32 = 1;

/// A rounded rectangle of `rect`, grown by `grow` pixels on every side (smaller, should it be negative) and moved `down`
/// pixels, its corners growing with it as a CSS shadow's do. Shrunk past nothing it is a line, never turned inside out.
fn shape(rect: &RECT, radius: i32, grow: i32, down: i32) -> D2D1_ROUNDED_RECT {
    let (left, right) = squeeze(rect.left, rect.right, grow);
    let (top, bottom) = squeeze(rect.top + down, rect.bottom + down, grow);
    let radius = px((radius + grow).max(0));
    D2D1_ROUNDED_RECT {
        rect: D2D_RECT_F {
            left: px(left),
            top: px(top),
            right: px(right),
            bottom: px(bottom),
        },
        radiusX: radius,
        radiusY: radius,
    }
}

/// The span from `near` to `far` grown by `grow` at both ends; shrunk so far that the ends would cross, the middle.
fn squeeze(near: i32, far: i32, grow: i32) -> (i32, i32) {
    let (near_grown, far_grown) = (near - grow, far + grow);
    if near_grown <= far_grown {
        (near_grown, far_grown)
    } else {
        let middle = near + (far - near) / 2;
        (middle, middle)
    }
}

/// `color` as Direct2D takes it, with `alpha` for its opacity.
fn d2d(rgb: [u8; 3], alpha: f32) -> D2D1_COLOR_F {
    let [r, g, b] = rgb;
    D2D1_COLOR_F {
        r: f32::from(r) / 255.0,
        g: f32::from(g) / 255.0,
        b: f32::from(b) / 255.0,
        a: alpha,
    }
}

/// What paints on one render target. `origin` is the point of the virtual screen at the target's pixel (0, 0): the window's
/// monitor when the target is the window, or where a bitmap kept for the toolbar starts.
struct Painter<'a> {
    overlay: &'a Overlay,
    target: &'a ID2D1RenderTarget,
    palette: &'static Palette,
    origin: (i32, i32),
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
    /// its panel and then its buttons, the ones the scene lights lit. H, V and C are left out while `toolbar.mode` is
    /// `None`: their room stays as it was. Between BeginDraw and EndDraw.
    pub(super) fn paint_toolbar(
        &self,
        target: &ID2D1RenderTarget,
        layout: &Layout,
        toolbar: &Toolbar,
        origin: (i32, i32),
    ) -> windows::core::Result<()> {
        // SAFETY: the color outlives the call, which keeps nothing of it.
        let brush = unsafe { target.CreateSolidColorBrush(&d2d([0, 0, 0], 1.0), None) }?;
        let painter = Painter {
            overlay: self,
            target,
            palette: toolbar_palette::palette(self.mode),
            origin,
            brush,
        };
        // SAFETY: a drawing call on a live render target. Out of BeginDraw and EndDraw, Direct2D only fails the frame
        // at EndDraw. The corners need antialiasing, which the target may have been left without.
        unsafe { target.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE) };
        let is_tool = |button: &ToolbarButton| matches!(button, ToolbarButton::Tool(_));
        let is_mode = |button: &ToolbarButton| matches!(button, ToolbarButton::Mode(_));
        let is_control = |button: &ToolbarButton| !is_tool(button) && !is_mode(button);
        painter.group(layout, &layout.tools, toolbar, is_tool)?;
        if toolbar.mode.is_some() {
            painter.group(layout, &layout.modes, toolbar, is_mode)?;
        }
        painter.group(layout, &layout.controls, toolbar, is_control)
    }
}

impl Painter<'_> {
    fn dpi(&self) -> u32 {
        self.overlay.dpi
    }

    /// `logical` pixels of the monitor's scale, whole.
    fn s(&self, logical: u32) -> i32 {
        scaled(logical, self.dpi())
    }

    /// As `s`, for a length that may be negative, as a shadow's spread and drop may.
    fn signed(&self, logical: i32) -> i32 {
        logical.signum() * scaled(logical.unsigned_abs(), self.dpi())
    }

    /// The standard deviation, in physical pixels, of a CSS blur radius of `blur` logical pixels: half of it.
    fn sigma(&self, blur: u32) -> f64 {
        f64::from(blur) * f64::from(self.dpi()) / 192.0
    }

    /// `rect`, from the virtual screen to the target's own pixels.
    fn local(&self, rect: &RECT) -> RECT {
        RECT {
            left: rect.left - self.origin.0,
            top: rect.top - self.origin.1,
            right: rect.right - self.origin.0,
            bottom: rect.bottom - self.origin.1,
        }
    }

    /// Sets the shared brush to `rgb` at `alpha`.
    fn set(&self, rgb: [u8; 3], alpha: f32) {
        // SAFETY: the color outlives the call, which keeps nothing of it.
        unsafe { self.brush.SetColor(&d2d(rgb, alpha)) };
    }

    fn fill(&self, shape: &D2D1_ROUNDED_RECT, color: Rgba) {
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
        member: impl Fn(&ToolbarButton) -> bool,
    ) -> windows::core::Result<()> {
        // The panel is a little see-through, so what falls under it is cut out of its shadow.
        self.surface(
            &self.palette.group,
            &self.local(group),
            GROUP_RADIUS,
            BORDER,
            true,
        )?;
        for (button, place) in layout.buttons.iter().filter(|(button, _)| member(button)) {
            let lit = match button {
                ToolbarButton::Tool(tool) => *tool == toolbar.active,
                ToolbarButton::Mode(mode) => toolbar.mode == Some(*mode),
                _ => false,
            };
            self.button(button, &self.local(place), lit)?;
        }
        Ok(())
    }

    /// One button: its own box and, when it is lit, the lit box over it, which hides it and shows its glow around it.
    fn button(&self, button: &ToolbarButton, place: &RECT, lit: bool) -> windows::core::Result<()> {
        let (surface, radius) = match button {
            ToolbarButton::Tool(_) => (&self.palette.button, TOOL_RADIUS),
            ToolbarButton::Mode(_) => (&self.palette.button, SMALL_RADIUS),
            _ => (&self.palette.small, SMALL_RADIUS),
        };
        self.surface(surface, place, radius, 0, false)?;
        if lit {
            self.surface(&self.palette.lit, place, radius, 0, false)?;
        }
        // The icon is over the lit box, in the lit ink.
        let ink = match (lit, button) {
            (true, _) => self.palette.ink_on,
            (false, ToolbarButton::Tool(_) | ToolbarButton::Mode(_)) => self.palette.ink,
            (false, _) => self.palette.small_ink,
        };
        self.icon(button, place, ink)
    }

    /// The icon of `button` in `ink`, centered in `place`: its view box at its CSS size, scaled for the monitor, with its
    /// top left corner on a whole pixel (a half pixel of room goes to the left and top), and its strokes round at their
    /// ends and corners.
    fn icon(&self, button: &ToolbarButton, place: &RECT, ink: Rgba) -> windows::core::Result<()> {
        let Some(icon) = toolbar_icons::icon_for(button) else {
            return Ok(());
        };
        let side = self.s(icon.size);
        let at = Placement {
            left: px(place.left + (place.right - place.left - side + 1) / 2),
            top: px(place.top + (place.bottom - place.top - side + 1) / 2),
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
                        Segment::Cubic { c1, c2, to } => sink.AddBezier(&D2D1_BEZIER_SEGMENT {
                            point1: at.point(c1).point,
                            point2: at.point(c2).point,
                            point3: at.point(to).point,
                        }),
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
        let radius = self.s(radius);
        let ring_width = self.s(ring_width);
        let outer = shape(rect, radius, 0, 0);
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
                px(rect.top + ring_width),
                px(rect.bottom - ring_width),
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
        let spread = self.signed(shadow.spread);
        let drop = self.signed(shadow.y);
        let short = (rect.right - rect.left).min(rect.bottom - rect.top) + 2 * spread;
        if short <= 0 {
            return Ok(());
        }
        let layers = shadow::layers(
            self.sigma(shadow.blur),
            shadow.color.alpha,
            Profile::Strip(f64::from(short)),
        );
        for layer in &layers.layers {
            let grow = spread + layer.grow;
            // A layer that stays inside the box is under it, or cut out of it: never seen.
            if grow <= -drop.abs() {
                continue;
            }
            self.set(shadow.color.rgb, layer.opacity);
            let layer = shape(rect, radius, grow, drop);
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
        let inside = shape(padding, radius, 0, 0);
        for layer in &layers.layers {
            // A layer's reach is how far into the hole the shadow goes: the hole is that much smaller.
            let hole =
                self.overlay
                    .geometry_of(&shape(padding, radius, -spread - layer.grow, drop))?;
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
                color: d2d(stop.color.rgb, stop.color.alpha),
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
