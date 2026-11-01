//! The frozen screen as the session core receives it: one photo per monitor, with the monitor's place, size and scale.

use std::fmt;

/// A point in physical pixels of the virtual screen, where Windows' main monitor starts at (0, 0) and the others may
/// sit at negative coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

/// A rectangle in physical pixels of the virtual screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub width: u32,
    pub height: u32,
}

impl Rect {
    /// Whether `point` is one of the rectangle's pixels.
    pub fn contains(self, point: Point) -> bool {
        let (x, y) = (i64::from(point.x), i64::from(point.y));
        let (left, top) = (i64::from(self.left), i64::from(self.top));
        (left..left + i64::from(self.width)).contains(&x)
            && (top..top + i64::from(self.height)).contains(&y)
    }

    /// The rightmost column. Windows keeps the virtual screen in i32 coordinates, so for a monitor this never
    /// saturates; it only keeps a made-up rectangle from overflowing.
    pub fn right(self) -> i32 {
        self.left
            .saturating_add_unsigned(self.width)
            .saturating_sub(1)
    }

    /// The bottom row, as [`Rect::right`] is the rightmost column.
    pub fn bottom(self) -> i32 {
        self.top
            .saturating_add_unsigned(self.height)
            .saturating_sub(1)
    }

    /// The pixels the rectangle shares with `other`: none when they don't touch.
    pub(crate) fn overlap(self, other: Rect) -> Rect {
        let left = self.left.max(other.left);
        let top = self.top.max(other.top);
        let right = self
            .left
            .saturating_add_unsigned(self.width)
            .min(other.left.saturating_add_unsigned(other.width));
        let bottom = self
            .top
            .saturating_add_unsigned(self.height)
            .min(other.top.saturating_add_unsigned(other.height));
        Rect {
            left,
            top,
            width: right.saturating_sub(left).max(0).unsigned_abs(),
            height: bottom.saturating_sub(top).max(0).unsigned_abs(),
        }
    }

    /// The rectangle's pixel nearest to `point`: `point` itself when it is in the rectangle.
    pub fn nearest(self, point: Point) -> Point {
        // `max` keeps an empty rectangle from making `clamp` panic.
        Point {
            x: point.x.clamp(self.left, self.right().max(self.left)),
            y: point.y.clamp(self.top, self.bottom().max(self.top)),
        }
    }
}

/// A monitor's scale, kept as the DPI Windows reports for it (96 is 100%, 168 is 175%), so dividing by it stays exact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Scale {
    dpi: u32,
}

impl Scale {
    /// 100%, where a logical pixel is a physical pixel.
    pub const HUNDRED_PERCENT: Scale = Scale { dpi: 96 };

    /// `None` for 0, which no monitor has.
    pub fn from_dpi(dpi: u32) -> Option<Scale> {
        (dpi > 0).then_some(Scale { dpi })
    }

    pub fn dpi(self) -> u32 {
        self.dpi
    }
}

/// Whether a photo holds the colors the apps drew (v1 spec WPC-296, Colors and HDR).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Colors {
    /// As the apps drew them: HDR is off on that monitor, or its photo came through the exact HDR path.
    Exact,
    /// HDR is on and its brightening couldn't be undone, so Inspect shows the Approximate color notice there.
    Approximate,
}

/// One monitor's part of the frozen screen.
#[derive(Debug, Clone, PartialEq)]
pub struct Photo {
    /// The monitor's place and size. The image has its size.
    pub rect: Rect,
    pub scale: Scale,
    pub colors: Colors,
    pub image: Image,
}

/// The monitor `point` is on, as an index in `frozen_screen`'s order: the current monitor when `point` is the cursor.
/// `None` off every monitor.
pub(crate) fn monitor_at(frozen_screen: &[Photo], point: Point) -> Option<usize> {
    frozen_screen
        .iter()
        .position(|photo| photo.rect.contains(point))
}

/// An opaque 8-bit sRGB image: `height` rows of `width` pixels from the top, with no gap between rows. Each pixel is
/// four bytes, blue, green, red and one that says nothing: the order Windows captures and draws in, so the photo is
/// never converted on its way in or out.
#[derive(Clone, PartialEq, Eq)]
pub struct Image {
    width: u32,
    height: u32,
    bgra: Vec<u8>,
}

impl Image {
    /// `None` unless `bgra` holds exactly `height` rows of `width` pixels.
    pub fn new(width: u32, height: u32, bgra: Vec<u8>) -> Option<Image> {
        let length = usize::try_from(width)
            .ok()?
            .checked_mul(usize::try_from(height).ok()?)?
            .checked_mul(4)?;
        (bgra.len() == length).then_some(Image {
            width,
            height,
            bgra,
        })
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    /// The pixels, as [`Image`] describes them.
    pub fn bgra(&self) -> &[u8] {
        &self.bgra
    }
}

/// Only the size: a 4K photo's pixels are 33 MB of text.
impl fmt::Debug for Image {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Image({}x{})", self.width, self.height)
    }
}
