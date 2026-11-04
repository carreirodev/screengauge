//! The prototype's two signs (WPC-279): the check mark of something done, and the exclamation mark in a circle of
//! something that went wrong. The tools' notices show them before their text, and Settings' footer before the answer of
//! "Check for updates". Both are strokes with round caps and joins, drawn in the 24 by 24 units of their `<svg>`.

use screengauge_core::session::Sign;
use windows::Win32::Graphics::Direct2D::Common::{D2D1_FIGURE_BEGIN_HOLLOW, D2D1_FIGURE_END_OPEN};
use windows::Win32::Graphics::Direct2D::{
    D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_CAP_STYLE_ROUND, D2D1_DASH_STYLE_SOLID, D2D1_ELLIPSE,
    D2D1_LINE_JOIN_ROUND, D2D1_STROKE_STYLE_PROPERTIES, ID2D1Factory, ID2D1RenderTarget,
    ID2D1SolidColorBrush, ID2D1StrokeStyle,
};

/// The units of the signs' view box.
const UNITS: f32 = 24.0;

/// Round caps and joins, which the signs' strokes are drawn with.
pub fn round_stroke(factory: &ID2D1Factory) -> windows::core::Result<ID2D1StrokeStyle> {
    let round = D2D1_STROKE_STYLE_PROPERTIES {
        startCap: D2D1_CAP_STYLE_ROUND,
        endCap: D2D1_CAP_STYLE_ROUND,
        dashCap: D2D1_CAP_STYLE_ROUND,
        lineJoin: D2D1_LINE_JOIN_ROUND,
        miterLimit: 1.0,
        dashStyle: D2D1_DASH_STYLE_SOLID,
        dashOffset: 0.0,
    };
    // SAFETY: `round` outlives the call, which keeps nothing of it; a solid line has no dashes to give.
    unsafe { factory.CreateStrokeStyle(&round, None) }
}

/// What a sign is drawn with: the target, the factory that made it, and the round stroke.
pub struct Pen<'a> {
    pub target: &'a ID2D1RenderTarget,
    pub factory: &'a ID2D1Factory,
    pub stroke: &'a ID2D1StrokeStyle,
}

/// Draws `sign` with `pen` in `brush`, `side` pixels wide and tall, with its top left corner at (`left`, `top`). The
/// check mark is a 3-unit stroke, the exclamation mark and its circle 2.4.
pub fn draw(
    pen: &Pen,
    brush: &ID2D1SolidColorBrush,
    sign: Sign,
    left: f32,
    top: f32,
    side: f32,
) -> windows::core::Result<()> {
    let Pen {
        target,
        factory,
        stroke,
    } = *pen;
    let unit = side / UNITS;
    // A point given in the sign's units; the point type comes from a crate windows 0.62 doesn't re-export.
    let at = |x: f32, y: f32| {
        let mut point = D2D1_ELLIPSE::default().point;
        point.X = left + x * unit;
        point.Y = top + y * unit;
        point
    };
    // SAFETY: a drawing call on a live render target. Out of BeginDraw and EndDraw, Direct2D only fails the frame at
    // EndDraw.
    unsafe { target.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE) };
    match sign {
        Sign::Ok => {
            // SAFETY: no arguments.
            let path = unsafe { factory.CreatePathGeometry() }?;
            // SAFETY: no arguments.
            let sink = unsafe { path.Open() }?;
            // SAFETY: the points are plain values, and the figure is begun, extended and ended in order on the sink
            // this function opened and closes below.
            unsafe {
                sink.BeginFigure(at(5.0, 12.5), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLine(at(9.5, 17.0));
                sink.AddLine(at(19.0, 7.5));
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
            }
            // SAFETY: closes the sink opened above, once.
            unsafe { sink.Close() }?;
            // SAFETY: a drawing call on a live render target with arguments that outlive it; as above.
            unsafe { target.DrawGeometry(&path, brush, 3.0 * unit, stroke) };
        }
        Sign::Alert => {
            let width = 2.4 * unit;
            let circle = D2D1_ELLIPSE {
                point: at(12.0, 12.0),
                radiusX: 9.5 * unit,
                radiusY: 9.5 * unit,
            };
            // SAFETY: drawing calls on a live render target with arguments that outlive them; as above.
            unsafe {
                target.DrawEllipse(&circle, brush, width, stroke);
                target.DrawLine(at(12.0, 7.0), at(12.0, 13.0), brush, width, stroke);
                // The dot is a line a hundredth of a unit long: its round caps make it a dot as wide as the stroke.
                target.DrawLine(at(12.0, 17.0), at(12.01, 17.0), brush, width, stroke);
            }
        }
    }
    Ok(())
}
