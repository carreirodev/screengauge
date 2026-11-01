//! The frozen screen's pixels, and the one rule that tells content from the reference color, which Dimensions and
//! Bounds share, so the calibration (WPC-332) changes it in one place.

use super::{Photo, Point};
use crate::settings::Tolerance;

/// The pixels of row `y` from column `left`, `count` of them, which must all be on the monitor: four bytes each, as
/// [`Image`](super::Image) holds them.
pub(super) fn row(photo: &Photo, y: i32, left: i32, count: u32) -> &[[u8; 4]] {
    // Every pixel is on the monitor, so both differences are places in the photo. u32 to usize is lossless on x64, the
    // only target ScreenGauge builds for.
    let column = left.abs_diff(photo.rect.left) as usize;
    let line = y.abs_diff(photo.rect.top) as usize;
    let start = line * photo.rect.width as usize + column;
    &photo.image.bgra().as_chunks::<4>().0[start..start + count as usize]
}

/// The red, green and blue of the pixel at `point`, which must be on the monitor.
pub(super) fn color(photo: &Photo, point: Point) -> [u8; 3] {
    rgb(row(photo, point.y, point.x, 1)[0])
}

/// The red, green and blue of a pixel as the photo holds it. The fourth byte says nothing.
pub(super) fn rgb([blue, green, red, _]: [u8; 4]) -> [u8; 3] {
    [red, green, blue]
}

/// A reference color with the tolerance it is compared at, which tells content from the same area: a pixel differs
/// from it when one of its red, green or blue differs from the reference color's by more than three times the
/// tolerance. The prototype's rule, provisional until the calibration (WPC-332). Worked out once, for the millions of
/// pixels a drag of Bounds may compare with it at each move.
#[derive(Debug, Clone, Copy)]
pub(super) struct Reference {
    /// The lowest red, green and blue a pixel can have and still not differ.
    low: [u8; 3],
    /// The highest.
    high: [u8; 3],
}

impl Reference {
    /// `color` as the reference color, compared at `tolerance`.
    pub(super) fn new(color: [u8; 3], tolerance: Tolerance) -> Reference {
        // At most 150, as the highest tolerance is 50: a u8, kept from going past 255 all the same.
        let limit = tolerance.get().saturating_mul(3);
        Reference {
            low: color.map(|channel| channel.saturating_sub(limit)),
            high: color.map(|channel| channel.saturating_add(limit)),
        }
    }

    /// Whether `pixel` differs from the reference color by more than the tolerance: one of its red, green or blue lies
    /// outside the range.
    pub(super) fn differs(self, pixel: [u8; 3]) -> bool {
        // Every channel is compared, without stopping at the first that differs, so the comparisons run side by side.
        (0..3).fold(false, |differs, channel| {
            differs | (pixel[channel] < self.low[channel]) | (pixel[channel] > self.high[channel])
        })
    }
}
