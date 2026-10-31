//! Pictures and colors as the icon files hold them: straight (not premultiplied) RGBA, rows from the top.

/// One color with its opacity, not premultiplied.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba {
    /// An opaque color from `0xRRGGBB`.
    pub const fn hex(rgb: u32) -> Rgba {
        let [_, r, g, b] = rgb.to_be_bytes();
        Rgba { r, g, b, a: 255 }
    }

    pub const TRANSPARENT: Rgba = Rgba {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    };

    /// The same color at `opacity` (0 to 1) of its own opacity.
    pub fn with_opacity(self, opacity: f32) -> Rgba {
        Rgba {
            a: unit_to_byte(f32::from(self.a) / 255.0 * opacity),
            ..self
        }
    }
}

/// A square or rectangular picture.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Image {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

impl Image {
    /// A fully transparent picture.
    pub fn new(width: u32, height: u32) -> Image {
        Image {
            width,
            height,
            rgba: vec![0; pixel_count(width, height) * 4],
        }
    }

    /// A picture from its RGBA bytes, or `None` when there aren't exactly four per pixel.
    pub fn from_rgba(width: u32, height: u32, rgba: Vec<u8>) -> Option<Image> {
        (rgba.len() == pixel_count(width, height) * 4).then_some(Image {
            width,
            height,
            rgba,
        })
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    /// The pixels, four bytes each (R, G, B, A), rows from the top.
    pub fn rgba(&self) -> &[u8] {
        &self.rgba
    }

    pub fn pixels(&self) -> impl Iterator<Item = Rgba> + '_ {
        self.rgba
            .as_chunks::<4>()
            .0
            .iter()
            .map(|&[r, g, b, a]| Rgba { r, g, b, a })
    }

    pub fn pixel(&self, x: u32, y: u32) -> Rgba {
        let i = self.index(x, y);
        Rgba {
            r: self.rgba[i],
            g: self.rgba[i + 1],
            b: self.rgba[i + 2],
            a: self.rgba[i + 3],
        }
    }

    pub fn set_pixel(&mut self, x: u32, y: u32, color: Rgba) {
        let i = self.index(x, y);
        self.rgba[i..i + 4].copy_from_slice(&[color.r, color.g, color.b, color.a]);
    }

    /// This picture centered on a transparent canvas of `width` × `height`, which must be at least as large.
    pub fn centered_on(&self, width: u32, height: u32) -> Image {
        assert!(width >= self.width && height >= self.height);
        let mut canvas = Image::new(width, height);
        let (left, top) = ((width - self.width) / 2, (height - self.height) / 2);
        for y in 0..self.height {
            for x in 0..self.width {
                canvas.set_pixel(left + x, top + y, self.pixel(x, y));
            }
        }
        canvas
    }

    fn index(&self, x: u32, y: u32) -> usize {
        assert!(
            x < self.width && y < self.height,
            "({x}, {y}) is outside {}×{}",
            self.width,
            self.height
        );
        (widen(y) * widen(self.width) + widen(x)) * 4
    }
}

const _: () = assert!(size_of::<usize>() >= size_of::<u32>());

/// A u32 as a usize. The assert above makes the cast lossless.
pub fn widen(value: u32) -> usize {
    value as usize
}

fn pixel_count(width: u32, height: u32) -> usize {
    widen(width) * widen(height)
}

/// A value from 0 to 1 as a byte, rounded: the clamp makes the cast lose only the fraction it rounds away.
pub fn unit_to_byte(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_reads_red_green_blue() {
        assert_eq!(
            Rgba::hex(0xF97316),
            Rgba {
                r: 0xF9,
                g: 0x73,
                b: 0x16,
                a: 255
            }
        );
    }

    #[test]
    fn centering_leaves_an_even_margin() {
        let mut dot = Image::new(2, 2);
        dot.set_pixel(0, 0, Rgba::hex(0xFFFFFF));
        let canvas = dot.centered_on(6, 4);
        assert_eq!(canvas.pixel(2, 1), Rgba::hex(0xFFFFFF));
        assert_eq!(canvas.pixels().filter(|p| p.a > 0).count(), 1);
    }

    #[test]
    fn rgba_of_the_wrong_length_is_refused() {
        assert!(Image::from_rgba(2, 2, vec![0; 15]).is_none());
        assert!(Image::from_rgba(2, 2, vec![0; 16]).is_some());
    }
}
