//! Snip drawn over a window's photo while the button is held (v1 spec WPC-296, Snip; the approved prototype's variant A,
//! WPC-279, `drawResult` for `snip`): cyan corners just outside the area the image is taken of, with no fill, so what
//! the image will hold shows untouched, and above their top left corner a cyan pill with a camera and the image's size.
//! Where each goes is worked out here in whole pixels of the virtual screen, with no Windows call, and the corners are
//! drawn with aliased fills, so each is crisp.

use screengauge_core::session::{Rect, SnipArea};
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Direct2D::{
    D2D1_ANTIALIAS_MODE_ALIASED, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, ID2D1RenderTarget,
};

use super::geometry::{self, Size, scaled};
use super::palette::{Color, color};
use super::toolbar_icons::CAMERA;
use super::{Look, Overlay, PlacedLabel, PlacedText, px, whole};

/// How thick each corner's arms are, and how far outside the image they lie, in physical pixels whatever the scale (the
/// prototype's `fillRect(…, arm, 2)` from `r.x - 2`).
const THICKNESS: i32 = 2;
/// How long each arm is, from the corner's outer pixel, in logical pixels: `14 * dpr`. An image too small for that has
/// arms half its width or height, and never shorter than `SHORTEST_ARM` physical pixels.
const ARM: u32 = 14;
const SHORTEST_ARM: i32 = 3;
/// The gap between the camera and the size on the label (`gap: 5px`), in logical pixels.
const CAMERA_GAP: u32 = 5;
/// The gap between the label's bottom and the corners' top, in logical pixels.
const LABEL_GAP: u32 = 6;
/// Where the label goes when the monitor has no room above the corners: this far right of their top left corner and
/// this far down, inside them, over the image (`translate(4px, 6px)`).
const INSIDE_RIGHT: u32 = 4;
const INSIDE_DOWN: u32 = 6;

/// The label's ink, on the cyan, the live label's (#04161b), which the camera is stroked in too.
const INK: Color = color(0x04, 0x16, 0x1b, 1.0);

/// The cyan corners around `area`, for a monitor of `dpi`: at each of the four corners an arm along the row and one
/// along the column, each `THICKNESS` pixels thick, lying just outside the image, so not one of its pixels is covered.
/// Each rectangle's right and bottom are one past its last pixel. The arms of a corner overlap at its outer pixels,
/// which are the same cyan.
pub fn corners(area: Rect, dpi: u32) -> Vec<RECT> {
    let half = |length: u32| i32::try_from(length / 2).unwrap_or(i32::MAX);
    let arm = scaled(ARM, dpi)
        .min(half(area.width))
        .min(half(area.height))
        .max(SHORTEST_ARM);
    let (left, top) = (area.left - THICKNESS, area.top - THICKNESS);
    // One past the outer pixels on the right and at the bottom.
    let (right, bottom) = (area.right() + 1 + THICKNESS, area.bottom() + 1 + THICKNESS);
    let rect = |left, top, right, bottom| RECT {
        left,
        top,
        right,
        bottom,
    };
    vec![
        rect(left, top, left + arm, top + THICKNESS),
        rect(left, top, left + THICKNESS, top + arm),
        rect(right - arm, top, right, top + THICKNESS),
        rect(right - THICKNESS, top, right, top + arm),
        rect(left, bottom - THICKNESS, left + arm, bottom),
        rect(left, bottom - arm, left + THICKNESS, bottom),
        rect(right - arm, bottom - THICKNESS, right, bottom),
        rect(right - THICKNESS, bottom - arm, right, bottom),
    ]
}

/// Where the label of `area` goes: its rectangle in the virtual screen, right and bottom one past its last pixel. Its
/// left side is on the corners' left column and its bottom `LABEL_GAP` above their top row; where the monitor has no
/// room above, it goes inside the corners at their top left, over the image. Where it would still stick out of
/// `monitor`, it slides inside.
pub fn place_label(area: Rect, label: Size, monitor: RECT, dpi: u32) -> RECT {
    let (corner_x, corner_y) = (area.left - THICKNESS, area.top - THICKNESS);
    let above = corner_y - scaled(LABEL_GAP, dpi) - label.height;
    let (left, top) = if above >= monitor.top {
        (corner_x, above)
    } else {
        (
            corner_x + scaled(INSIDE_RIGHT, dpi),
            corner_y + scaled(INSIDE_DOWN, dpi),
        )
    };
    let left = geometry::inside(left, label.width, monitor.left, monitor.right);
    let top = geometry::inside(top, label.height, monitor.top, monitor.bottom);
    RECT {
        left,
        top,
        right: left + label.width,
        bottom: top + label.height,
    }
}

/// Snip's label, ready to draw: the live pill with the size on it, and where the camera goes before the size.
struct SnipLabel {
    label: PlacedLabel,
    /// The camera's square, as tall as the pill, so the camera is centered in it.
    camera: RECT,
}

impl Overlay {
    /// Draws what Snip shows while the button is held over what `target` holds, between its BeginDraw and EndDraw, over
    /// every measurement and the drawn rectangle: the corners, then the label's shadow, its pill and size, and the
    /// camera.
    pub fn draw_snip(
        &self,
        target: &ID2D1RenderTarget,
        area: &SnipArea,
    ) -> windows::core::Result<()> {
        self.draw_corners(target, area.rect);
        let snip = self.snip_label(area)?;
        self.draw_shadow(target, &snip.label)?;
        self.draw_pill(target, &snip.label);
        self.draw_icon(target, &CAMERA, &snip.camera, INK)
    }

    /// The cyan corners around `area`.
    pub(super) fn draw_corners(&self, target: &ID2D1RenderTarget, area: Rect) {
        // SAFETY: drawing calls on a live render target with arguments that outlive them. Out of BeginDraw and EndDraw,
        // Direct2D only fails the frame at EndDraw. Aliased fills of rectangles on whole pixels cover exactly those
        // pixels, and the target is at 96 DPI, so one unit is one physical pixel.
        unsafe {
            target.SetAntialiasMode(D2D1_ANTIALIAS_MODE_ALIASED);
            for corner in corners(area, self.dpi) {
                target.FillRectangle(&self.in_window(&corner), &self.cyan);
            }
            target.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
        }
    }

    /// The label's pill, its size laid out after the camera and the gap, and the camera's square. The pill is the live
    /// label's, as wide as its padding, the camera, the gap and the size.
    fn snip_label(&self, area: &SnipArea) -> windows::core::Result<SnipLabel> {
        let text = self.typeset(&area.label.physical, &self.format)?;
        let camera = scaled(CAMERA.size, self.dpi);
        let before_text = camera + scaled(CAMERA_GAP, self.dpi);
        let size = geometry::pill_size(before_text + text.width, self.dpi);
        let pill = place_label(area.rect, size, self.monitor, self.dpi);
        // The text sits as on every label: the middle of its digits on the middle of the pill, on a whole pixel.
        let font = px(scaled(geometry::TEXT, self.dpi));
        let window = self.in_window(&pill);
        let middle = (window.top + window.bottom) / 2.0;
        let baseline = whole((middle + self.fonts.cap_height * font / 2.0).round());
        let pad = scaled(geometry::PAD_X, self.dpi);
        let camera_left = pill.left + pad;
        Ok(SnipLabel {
            label: PlacedLabel {
                look: Look::Live,
                pill,
                physical: PlacedText {
                    left: window.left + px(pad + before_text),
                    top: px(baseline) - text.baseline,
                    layout: text.layout,
                },
                logical: None,
            },
            camera: RECT {
                left: camera_left,
                top: pill.top,
                right: camera_left + camera,
                bottom: pill.bottom,
            },
        })
    }
}
