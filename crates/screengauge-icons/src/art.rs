//! The drawings: the app icon and the Store icon as shapes in pixels, for a given size.
//!
//! Both are the set square of WPC-291, on Microsoft's 48 × 48 icon grid, as the prototype on the branch
//! `prototype/app-icon` drew it (`prototypes/app-icon/index.html`, shape D, palette 1, strong outline, and for the
//! Store icon the option labelled "3D 2 · Acrílico", acrylic). One change in the app icon, measured in WPC-304: the
//! hole's outline is `HOLE_OUTLINE`, lighter than the outer one, and a little thicker, so that at least half of every
//! size has a 3:1 contrast on the dark taskbar as well as on the light one (`contrast`). The approved drawing passed on
//! the light taskbar only.

use crate::image::Rgba;

/// The brand colors of WPC-291.
pub mod color {
    use crate::image::Rgba;

    /// The light end of the orange, the top left of the set square.
    pub const ORANGE_LIGHT: Rgba = Rgba::hex(0xFED7AA);
    /// Two steps of the orange between its light end and its middle, for the hand-drawn sizes' gradient.
    pub const ORANGE_STEP_1: Rgba = Rgba::hex(0xFDBA74);
    pub const ORANGE_STEP_2: Rgba = Rgba::hex(0xFB923C);
    /// The middle of the orange, the bottom right of the set square.
    pub const ORANGE: Rgba = Rgba::hex(0xF97316);
    /// The dark end of the orange, the Store icon's thickness.
    pub const ORANGE_DARK: Rgba = Rgba::hex(0x9A3412);
    /// The strong outline, darker than the orange's dark end, which holds the orange on a light taskbar.
    pub const OUTLINE: Rgba = Rgba::hex(0x7C2D12);
    /// The hole's outline: an orange dark enough for 3:1 on a light taskbar and light enough for 3:1 on a dark one.
    pub const HOLE_OUTLINE: Rgba = Rgba::hex(0xC2410C);
    /// The dark end of the graphite, the measuring marks.
    pub const GRAPHITE: Rgba = Rgba::hex(0x1A1C20);
    pub const WHITE: Rgba = Rgba::hex(0xFFFFFF);
    pub const BLACK: Rgba = Rgba::hex(0x000000);
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

const fn pt(x: f32, y: f32) -> Point {
    Point { x, y }
}

/// How a shape is colored.
#[derive(Clone, Debug, PartialEq)]
pub enum Paint {
    Solid(Rgba),
    /// A gradient along the line from `from` to `to`, mixed in sRGB as SVG mixes it, each stop at an offset from 0
    /// to 1.
    Linear {
        from: Point,
        to: Point,
        stops: Vec<(f32, Rgba)>,
    },
}

/// One drawing operation, in pixels.
#[derive(Clone, Debug, PartialEq)]
pub enum Shape {
    /// Fills closed figures; where figures overlap, the even-odd rule decides, so a figure inside another is a hole.
    Fill {
        figures: Vec<Vec<Point>>,
        paint: Paint,
    },
    /// Strokes closed figures with round joins, centered on the edge as SVG strokes it.
    Stroke {
        figures: Vec<Vec<Point>>,
        width: f32,
        paint: Paint,
    },
}

/// Shapes drawn together, then blurred by `blur` pixels (a Gaussian's standard deviation; 0 for none), then laid
/// over the layers before.
#[derive(Clone, Debug, PartialEq)]
pub struct Layer {
    pub shapes: Vec<Shape>,
    pub blur: f32,
}

/// The set square's outer edge, its right angle at the bottom left.
const OUTER: [Point; 3] = [pt(5.0, 5.0), pt(5.0, 43.0), pt(43.0, 43.0)];
/// The triangular hole in the middle.
const HOLE: [Point; 3] = [pt(12.0, 21.0), pt(12.0, 36.0), pt(27.0, 36.0)];
/// The strong outline: the prototype's 1.2 grid units, 1.8 times thicker.
const OUTLINE_WIDTH: f32 = 2.16;
/// The hole's outline, thicker than the outer one: its orange passes 3:1 on both taskbars, so more of it lifts the
/// light taskbar's share without lowering the dark one's.
const HOLE_OUTLINE_WIDTH: f32 = 2.6;
/// Where the outline's outer edge reaches on each side: the drawing's visible box, from `VISIBLE_MIN` to
/// `VISIBLE_MAX` both ways.
const VISIBLE_MIN: f32 = 5.0 - OUTLINE_WIDTH / 2.0;
const VISIBLE_MAX: f32 = 43.0 + OUTLINE_WIDTH / 2.0;

/// The app icon at `size` × `size` pixels.
///
/// The drawing is scaled so that the set square's left and bottom edges fall exactly on pixel boundaries, which keeps
/// them sharp, and the measuring marks are rounded to whole pixels. Up to 36 pixels the marks are fewer and thicker,
/// as in the prototype. The tray icon's sizes, 16 to 32 pixels, are drawn pixel by pixel instead (`pixel_art`).
pub fn app_icon(size: u32) -> Vec<Layer> {
    let scale = grid_scale(size);
    // The visible box's edges, rounded to whole pixels; the scale and offset map the grid onto them exactly.
    let first = (VISIBLE_MIN * scale).round();
    let last = (VISIBLE_MAX * scale).round();
    let k = (last - first) / (VISIBLE_MAX - VISIBLE_MIN);
    let at = |p: Point| {
        pt(
            first + (p.x - VISIBLE_MIN) * k,
            first + (p.y - VISIBLE_MIN) * k,
        )
    };
    let body = vec![OUTER.map(at).to_vec(), HOLE.map(at).to_vec()];
    let marks = if size <= 36 { Marks::FEW } else { Marks::MANY };
    let shapes = vec![
        Shape::Fill {
            figures: body,
            paint: orange_gradient(at, 1.0),
        },
        Shape::Stroke {
            figures: vec![OUTER.map(at).to_vec()],
            width: OUTLINE_WIDTH * k,
            paint: Paint::Solid(color::OUTLINE),
        },
        Shape::Stroke {
            figures: vec![HOLE.map(at).to_vec()],
            width: HOLE_OUTLINE_WIDTH * k,
            paint: Paint::Solid(color::HOLE_OUTLINE),
        },
        Shape::Fill {
            figures: marks
                .rects()
                .into_iter()
                .map(|[a, b]| snapped_rect(at(a), at(b)))
                .collect(),
            paint: Paint::Solid(color::GRAPHITE),
        },
    ];
    vec![Layer { shapes, blur: 0.0 }]
}

/// The Store icon at `size` × `size` pixels: the set square as a piece of orange acrylic, with thickness, a
/// reflection and a soft shadow, seen from the front. The grid is shown from −4 to 52, so the thickness and the
/// shadow fit.
pub fn store_icon(size: u32) -> Vec<Layer> {
    const FIRST: f32 = -4.0;
    const SPAN: f32 = 56.0;
    const DEPTH: u8 = 6;
    let k = px(size) / SPAN;
    let at = |p: Point| pt((p.x - FIRST) * k, (p.y - FIRST) * k);
    let shifted = |dx: f32, dy: f32| {
        move |p: Point| {
            at(Point {
                x: p.x + dx,
                y: p.y + dy,
            })
        }
    };
    let body = |map: &dyn Fn(Point) -> Point| vec![OUTER.map(map).to_vec(), HOLE.map(map).to_vec()];

    // The shadow under the piece, blurred by 1.6 grid units.
    let shadow = Layer {
        shapes: vec![Shape::Fill {
            figures: vec![ellipse(at(pt(25.0, 48.5)), 20.0 * k, 2.2 * k)],
            paint: Paint::Solid(color::BLACK.with_opacity(0.32)),
        }],
        blur: 1.6 * k,
    };

    let mut shapes = Vec::new();
    // The thickness: copies of the piece stepping down and right, the farthest in the darkest orange.
    for step in (1..=DEPTH).rev() {
        let edge = if step == DEPTH {
            color::OUTLINE
        } else {
            color::ORANGE_DARK
        };
        let offset = f32::from(step);
        let map = shifted(offset * 0.35, offset * 0.6);
        shapes.push(Shape::Fill {
            figures: body(&map),
            paint: Paint::Solid(edge),
        });
        shapes.push(Shape::Stroke {
            figures: body(&map),
            width: 1.2 * k,
            paint: Paint::Solid(edge),
        });
    }
    // The front face, very slightly see-through, with a thin edge.
    shapes.push(Shape::Fill {
        figures: body(&at),
        paint: orange_gradient(at, 0.96),
    });
    shapes.push(Shape::Stroke {
        figures: body(&at),
        width: 0.6 * k,
        paint: Paint::Solid(color::ORANGE_DARK),
    });
    shapes.push(Shape::Fill {
        figures: Marks::MANY
            .rects()
            .into_iter()
            .map(|[a, b]| rect(at(a), at(b)))
            .collect(),
        paint: Paint::Solid(color::GRAPHITE),
    });
    // The reflection: white fading out from the top left corner, and a bright line along the left edge.
    shapes.push(Shape::Fill {
        figures: body(&at),
        paint: Paint::Linear {
            from: at(pt(0.0, 0.0)),
            to: at(pt(48.0, 48.0)),
            stops: vec![
                (0.0, color::WHITE.with_opacity(0.5)),
                (0.45, color::WHITE.with_opacity(0.0)),
            ],
        },
    });
    shapes.push(Shape::Fill {
        figures: vec![
            [pt(5.0, 5.0), pt(5.7, 5.7), pt(5.7, 43.0), pt(5.0, 43.0)]
                .map(at)
                .to_vec(),
        ],
        paint: Paint::Solid(color::WHITE.with_opacity(0.55)),
    });

    vec![shadow, Layer { shapes, blur: 0.0 }]
}

/// The measuring marks along the left and bottom edges.
struct Marks {
    /// Grid units between two marks.
    step: usize,
    /// How thick each mark is, in grid units.
    width: f32,
}

impl Marks {
    /// Up to 36 pixels: every 8 grid units, thicker.
    const FEW: Marks = Marks {
        step: 8,
        width: 1.8,
    };
    /// Above 36 pixels: every 4 grid units.
    const MANY: Marks = Marks {
        step: 4,
        width: 1.1,
    };

    /// Each mark as its top left and bottom right corners, in grid units. Marks alternate long and short.
    fn rects(&self) -> Vec<[Point; 2]> {
        let length = |i: usize| if i.is_multiple_of(2) { 4.5 } else { 2.8 };
        let half = self.width / 2.0;
        let left = (11..=39u8).step_by(self.step).enumerate().map(|(i, y)| {
            let y = f32::from(y);
            [pt(5.0, y - half), pt(5.0 + length(i), y + half)]
        });
        let bottom = (9..=37u8).step_by(self.step).enumerate().map(|(i, x)| {
            let x = f32::from(x);
            [pt(x - half, 43.0 - length(i)), pt(x + half, 43.0)]
        });
        left.chain(bottom).collect()
    }
}

/// The set square's orange, light at the top left corner and the middle orange at the bottom right, at `opacity`.
fn orange_gradient(at: impl Fn(Point) -> Point, opacity: f32) -> Paint {
    Paint::Linear {
        from: at(pt(5.0, 5.0)),
        to: at(pt(43.0, 43.0)),
        stops: vec![
            (0.0, color::ORANGE_LIGHT.with_opacity(opacity)),
            (1.0, color::ORANGE.with_opacity(opacity)),
        ],
    }
}

/// How many pixels one grid unit is at `size`.
fn grid_scale(size: u32) -> f32 {
    px(size) / 48.0
}

/// A pixel count as a coordinate. Icon sizes are at most a few hundred pixels, which f32 holds exactly.
fn px(size: u32) -> f32 {
    debug_assert!(size < 1 << 24);
    size as f32
}

fn rect(a: Point, b: Point) -> Vec<Point> {
    vec![a, pt(b.x, a.y), b, pt(a.x, b.y)]
}

/// A rectangle with its edges on whole pixels: each side rounded to whole pixels, at least one, around the same center.
/// Rounding each edge on its own would make equal marks one or two pixels thick depending on where they fall.
fn snapped_rect(a: Point, b: Point) -> Vec<Point> {
    let snap = |from: f32, to: f32| {
        let length = (to - from).round().max(1.0);
        let start = ((from + to - length) / 2.0).round();
        (start, start + length)
    };
    let (left, right) = snap(a.x, b.x);
    let (top, bottom) = snap(a.y, b.y);
    rect(pt(left, top), pt(right, bottom))
}

/// An ellipse as a polygon fine enough to look round at any icon size.
fn ellipse(center: Point, radius_x: f32, radius_y: f32) -> Vec<Point> {
    const SIDES: u16 = 180;
    (0..SIDES)
        .map(|i| {
            let angle = f32::from(i) / f32::from(SIDES) * std::f32::consts::TAU;
            pt(
                center.x + radius_x * angle.cos(),
                center.y + radius_y * angle.sin(),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn outer_edge(size: u32) -> (f32, f32) {
        let layers = app_icon(size);
        let Shape::Stroke { figures, width, .. } = &layers[0].shapes[1] else {
            panic!("the second shape is the outer outline")
        };
        let left = figures[0][0].x - width / 2.0;
        let bottom = figures[0][1].y + width / 2.0;
        (left, bottom)
    }

    #[test]
    fn the_set_squares_left_and_bottom_edges_fall_on_whole_pixels() {
        for size in [28, 30, 32, 36, 40, 48, 64, 96, 256] {
            let (left, bottom) = outer_edge(size);
            assert!((left - left.round()).abs() < 1e-4, "{size}: left {left}");
            assert!(
                (bottom - bottom.round()).abs() < 1e-4,
                "{size}: bottom {bottom}"
            );
            assert!(
                left >= 0.0 && bottom <= px(size),
                "{size}: {left}..{bottom}"
            );
        }
    }

    #[test]
    fn the_set_square_stays_centered() {
        for size in [28, 30, 32, 36, 40, 48, 64, 96, 256] {
            let (left, bottom) = outer_edge(size);
            assert!(
                (left - (px(size) - bottom)).abs() <= 1.0,
                "{size}: {left}..{bottom}"
            );
        }
    }

    #[test]
    fn marks_are_whole_pixels_and_fewer_up_to_36() {
        for (size, count) in [(28, 8), (36, 8), (40, 16), (256, 16)] {
            let layers = app_icon(size);
            let Shape::Fill { figures, .. } = &layers[0].shapes[3] else {
                panic!("the fourth shape is the marks")
            };
            assert_eq!(figures.len(), count, "{size}");
            for corner in figures.iter().flatten() {
                assert_eq!(corner.x, corner.x.round(), "{size}");
                assert_eq!(corner.y, corner.y.round(), "{size}");
            }
        }
    }

    #[test]
    fn the_store_icon_fits_its_canvas() {
        for layer in store_icon(300) {
            for shape in layer.shapes {
                let (Shape::Fill { figures, .. } | Shape::Stroke { figures, .. }) = shape;
                for p in figures.iter().flatten() {
                    assert!((0.0..=300.0).contains(&p.x) && (0.0..=300.0).contains(&p.y));
                }
            }
        }
    }
}
