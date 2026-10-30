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

/// A monitor's scale, kept as the DPI Windows reports for it (96 is 100%, 168 is 175%), so dividing by it stays exact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Scale {
    dpi: u32,
}

impl Scale {
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
