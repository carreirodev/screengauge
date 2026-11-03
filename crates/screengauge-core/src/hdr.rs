//! The HDR conversion (v1 spec WPC-296, Modules; Colors and HDR): with HDR on, Windows composes the desktop in 16-bit
//! floating point scRGB, where 1.0 is 80 nits, and shows SDR content brightened to the monitor's SDR white level. This
//! undoes it: each channel is divided by the SDR white level and encoded again as 8-bit sRGB by the sRGB curve, which
//! WPC-289 measured exact where a 2.2 gamma misses dark tones by up to 9 levels. HDR content, brighter than SDR white
//! or below black, is cut channel by channel to the sRGB range.

use std::num::NonZeroU32;

use crate::session::Image;

/// A monitor's SDR white level, as Windows reports it in `DISPLAYCONFIG_SDR_WHITE_LEVEL`: in thousandths of 80 nits, so
/// 1000 is 80 nits and 3000 is 240.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SdrWhiteLevel {
    level: NonZeroU32,
}

impl SdrWhiteLevel {
    /// `None` for 0, which would make every color infinitely bright.
    pub fn from_windows(level: u32) -> Option<SdrWhiteLevel> {
        NonZeroU32::new(level).map(|level| SdrWhiteLevel { level })
    }

    /// What one unit of scRGB is in SDR content's linear light: 80 nits over the SDR white level.
    fn scale(self) -> f64 {
        1000.0 / f64::from(self.level.get())
    }
}

/// The photo of a monitor from its 16-bit desktop image: `height` rows of `width` pixels from the top, `pitch` bytes
/// apart in `rows`, each pixel the red, green, blue and alpha halves (IEEE 754, little-endian) of
/// `DXGI_FORMAT_R16G16B16A16_FLOAT`. The alpha says nothing, and every pixel comes out opaque. `None` when `rows` is too
/// short for that or the image too large to address.
pub fn convert(
    rows: &[u8],
    pitch: usize,
    width: u32,
    height: u32,
    white: SdrWhiteLevel,
) -> Option<Image> {
    let columns = usize::try_from(width).ok()?;
    let lines = usize::try_from(height).ok()?;
    let row_length = columns.checked_mul(8)?;
    let srgb = Table::new(white);
    let mut bgra = vec![0; columns.checked_mul(lines)?.checked_mul(4)?];
    // `max(1)` only keeps an image with no columns from asking for chunks of no bytes; it then has no rows to fill.
    for (line, out) in bgra.chunks_exact_mut((columns * 4).max(1)).enumerate() {
        let start = line.checked_mul(pitch)?;
        let pixels = rows.get(start..start.checked_add(row_length)?)?;
        for (out, pixel) in out
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(pixels.as_chunks::<8>().0)
        {
            let channel = |at: usize| srgb.byte(u16::from_le_bytes([pixel[at], pixel[at + 1]]));
            *out = [channel(4), channel(2), channel(0), u8::MAX];
        }
    }
    Image::new(width, height, bgra)
}

/// The sRGB byte of every half, worked out once per photo: a 4K photo has 25 million channels, and there are only 65536
/// halves.
struct Table {
    bytes: Vec<u8>,
}

impl Table {
    fn new(white: SdrWhiteLevel) -> Table {
        let scale = white.scale();
        let bounds = byte_bounds();
        Table {
            bytes: (0..=u16::MAX)
                .map(|half| {
                    let linear = half_value(half) * scale;
                    // The bytes whose lower bound the light reaches: the nearest byte, 0 below black, 255 above SDR
                    // white. NaN reaches no bound, so it is black. At most 255 bounds, so the count is a byte.
                    let byte = bounds.partition_point(|&bound| bound <= linear);
                    u8::try_from(byte).unwrap_or(u8::MAX)
                })
                .collect(),
        }
    }

    fn byte(&self, half: u16) -> u8 {
        self.bytes[usize::from(half)]
    }
}

/// Where each sRGB byte from 1 to 255 starts, in linear light: halfway between it and the byte below, through the sRGB
/// curve (IEC 61966-2-1), so a light rounds to its nearest byte with no float coming back to a byte. Rising, as the
/// curve does.
fn byte_bounds() -> Vec<f64> {
    (1..=255)
        .map(|byte| {
            let encoded = (f64::from(byte) - 0.5) / 255.0;
            if encoded <= 0.040_45 {
                encoded / 12.92
            } else {
                ((encoded + 0.055) / 1.055).powf(2.4)
            }
        })
        .collect()
}

/// An IEEE 754 half's value, exactly: every half is an f64.
fn half_value(half: u16) -> f64 {
    let sign = if half & 0x8000 == 0 { 1.0 } else { -1.0 };
    let exponent = (half >> 10) & 0x1f;
    let fraction = f64::from(half & 0x3ff);
    sign * match exponent {
        0 => fraction * 2f64.powi(-24),
        0x1f if fraction == 0.0 => f64::INFINITY,
        0x1f => f64::NAN,
        _ => (1024.0 + fraction) * 2f64.powi(i32::from(exponent) - 25),
    }
}
