//! The toolbar's icons: the prototype's SVGs (WPC-279, `IC` in `app.js`), as they are written there. Each is a 24 by 24
//! view box with no fill and a stroke of the button's ink, round at its ends and corners, drawn at its own CSS size and
//! centered in its button. The paths are SVG path data, which `svg_path` reads; `toolbar_paint` draws them.

use screengauge_core::session::ToolbarButton;
use screengauge_core::settings::{DimensionsMode, Tool};

/// The side of every icon's view box, in the icons' own units.
pub const VIEW_BOX: f32 = 24.0;

/// One piece of an icon. A circle or a rounded rectangle could be written as a path with arcs, but SVG draws them as
/// themselves, and so does Direct2D.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Shape {
    Path(&'static str),
    Circle {
        cx: f32,
        cy: f32,
        r: f32,
    },
    Rect {
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        radius: f32,
    },
}

/// An icon: the CSS pixels its view box is shown at (the `width` and `height` of the `<svg>`), the width of its stroke in
/// view box units (its `stroke-width`), and what it is made of, painted in order.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Icon {
    pub size: u32,
    pub stroke: f32,
    pub shapes: &'static [Shape],
}

/// The ruler of Dimensions.
const DIMENSIONS: Icon = Icon {
    size: 20,
    stroke: 2.0,
    shapes: &[
        Shape::Rect {
            x: 2.5,
            y: 8.0,
            width: 19.0,
            height: 8.0,
            radius: 1.5,
        },
        Shape::Path("M6.5 8v3M10.5 8v4.5M14.5 8v3M18.5 8v4.5"),
    ],
};

/// Bounds: the corners of a frame around a small square.
const BOUNDS: Icon = Icon {
    size: 20,
    stroke: 2.0,
    shapes: &[
        Shape::Path("M4 9V4h5M20 9V4h-5M4 15v5h5M20 15v5h-5"),
        Shape::Rect {
            x: 9.0,
            y: 9.0,
            width: 6.0,
            height: 6.0,
            radius: 1.0,
        },
    ],
};

/// Inspect: a magnifying glass.
const INSPECT: Icon = Icon {
    size: 20,
    stroke: 2.0,
    shapes: &[
        Shape::Circle {
            cx: 11.0,
            cy: 11.0,
            r: 6.0,
        },
        Shape::Path("M15.5 15.5L20 20"),
    ],
};

/// Snip: a camera.
const SNIP: Icon = Icon {
    size: 20,
    stroke: 2.0,
    shapes: &[
        Shape::Path("M4 8h3l2-3h6l2 3h3v11H4z"),
        Shape::Circle {
            cx: 12.0,
            cy: 13.0,
            r: 3.5,
        },
    ],
};

/// H, V and C: arrows along the measurement's directions.
const HORIZONTAL: Icon = Icon {
    size: 16,
    stroke: 2.2,
    shapes: &[Shape::Path("M5 12h14M8 8l-4 4 4 4M16 8l4 4-4 4")],
};

const VERTICAL: Icon = Icon {
    size: 16,
    stroke: 2.2,
    shapes: &[Shape::Path("M12 5v14M8 8l4-4 4 4M8 16l4 4 4-4")],
};

const CROSS: Icon = Icon {
    size: 16,
    stroke: 2.2,
    shapes: &[Shape::Path(
        "M12 3v18M3 12h18M9 6l3-3 3 3M9 18l3 3 3-3M6 9l-3 3 3 3M18 9l3 3-3 3",
    )],
};

/// The gear of Settings.
const SETTINGS: Icon = Icon {
    size: 16,
    stroke: 2.1,
    shapes: &[
        Shape::Circle {
            cx: 12.0,
            cy: 12.0,
            r: 3.0,
        },
        Shape::Path(
            "M19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z",
        ),
    ],
};

/// Minimize: a chevron pointing left.
const MINIMIZE: Icon = Icon {
    size: 15,
    stroke: 2.6,
    shapes: &[Shape::Path("M15 6l-6 6 6 6")],
};

const CLOSE: Icon = Icon {
    size: 14,
    stroke: 2.8,
    shapes: &[Shape::Path("M6 6l12 12M18 6L6 18")],
};

/// The icon on `button`, or `None` for the button the minimized toolbar becomes, which this toolbar does not draw.
pub fn icon_for(button: &ToolbarButton) -> Option<&'static Icon> {
    Some(match button {
        ToolbarButton::Tool(Tool::Dimensions) => &DIMENSIONS,
        ToolbarButton::Tool(Tool::Bounds) => &BOUNDS,
        ToolbarButton::Tool(Tool::Inspect) => &INSPECT,
        ToolbarButton::Tool(Tool::Snip) => &SNIP,
        ToolbarButton::Mode(DimensionsMode::Horizontal) => &HORIZONTAL,
        ToolbarButton::Mode(DimensionsMode::Vertical) => &VERTICAL,
        ToolbarButton::Mode(DimensionsMode::Cross) => &CROSS,
        ToolbarButton::Settings => &SETTINGS,
        ToolbarButton::Minimize => &MINIMIZE,
        ToolbarButton::Close => &CLOSE,
        ToolbarButton::Restore => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::super::svg_path::{self, Segment};
    use super::super::toolbar::BUTTONS;
    use super::*;

    #[test]
    fn every_button_but_the_minimized_ones_has_an_icon_at_the_prototypes_size() {
        // The prototype's `sv(size, ..., stroke)` calls: size in CSS pixels, then the stroke width.
        let sizes = [
            (ToolbarButton::Tool(Tool::Dimensions), 20, 2.0),
            (ToolbarButton::Tool(Tool::Bounds), 20, 2.0),
            (ToolbarButton::Tool(Tool::Inspect), 20, 2.0),
            (ToolbarButton::Tool(Tool::Snip), 20, 2.0),
            (ToolbarButton::Mode(DimensionsMode::Horizontal), 16, 2.2),
            (ToolbarButton::Mode(DimensionsMode::Vertical), 16, 2.2),
            (ToolbarButton::Mode(DimensionsMode::Cross), 16, 2.2),
            (ToolbarButton::Settings, 16, 2.1),
            (ToolbarButton::Minimize, 15, 2.6),
            (ToolbarButton::Close, 14, 2.8),
        ];
        assert_eq!(sizes.map(|(button, _, _)| button), BUTTONS);
        for (button, size, stroke) in sizes {
            let icon = icon_for(&button).expect("an icon");
            assert_eq!((icon.size, icon.stroke), (size, stroke), "{button:?}");
        }
        assert_eq!(icon_for(&ToolbarButton::Restore), None);
    }

    #[test]
    fn every_path_of_every_icon_parses_into_the_figures_it_is_written_with() {
        // The `M` commands of each path, counted by eye in the SVG: one figure each.
        let figures = [
            (ToolbarButton::Tool(Tool::Dimensions), 4),
            (ToolbarButton::Tool(Tool::Bounds), 4),
            (ToolbarButton::Tool(Tool::Inspect), 1),
            (ToolbarButton::Tool(Tool::Snip), 1),
            (ToolbarButton::Mode(DimensionsMode::Horizontal), 3),
            (ToolbarButton::Mode(DimensionsMode::Vertical), 3),
            (ToolbarButton::Mode(DimensionsMode::Cross), 6),
            (ToolbarButton::Settings, 1),
            (ToolbarButton::Minimize, 1),
            (ToolbarButton::Close, 2),
        ];
        for (button, wanted) in figures {
            let icon = icon_for(&button).expect("an icon");
            let mut got = 0;
            for shape in icon.shapes {
                if let Shape::Path(d) = shape {
                    let parsed = svg_path::parse(d).unwrap_or_else(|e| panic!("{button:?}: {e}"));
                    got += parsed.len();
                    // Everything drawn is inside the view box, give or take the stroke: nothing is lost off its edge.
                    for figure in &parsed {
                        let mut points = vec![figure.start];
                        points.extend(figure.segments.iter().map(|segment| match segment {
                            Segment::Line(to) | Segment::Cubic { to, .. } => *to,
                            Segment::Arc(arc) => arc.to,
                        }));
                        for point in points {
                            assert!(
                                (0.0..=VIEW_BOX).contains(&point.x)
                                    && (0.0..=VIEW_BOX).contains(&point.y),
                                "{button:?}: {point:?}"
                            );
                        }
                    }
                }
            }
            assert_eq!(got, wanted, "{button:?}");
        }
    }

    #[test]
    fn the_shapes_are_inside_the_view_box() {
        for button in BUTTONS {
            let icon = icon_for(&button).expect("an icon");
            for shape in icon.shapes {
                match *shape {
                    Shape::Circle { cx, cy, r } => {
                        assert!(
                            cx - r >= 0.0
                                && cx + r <= VIEW_BOX
                                && cy - r >= 0.0
                                && cy + r <= VIEW_BOX
                        );
                    }
                    Shape::Rect {
                        x,
                        y,
                        width,
                        height,
                        radius,
                    } => {
                        assert!(
                            x >= 0.0 && x + width <= VIEW_BOX && y >= 0.0 && y + height <= VIEW_BOX
                        );
                        assert!(radius * 2.0 <= width.min(height));
                    }
                    Shape::Path(_) => {}
                }
            }
        }
    }
}
