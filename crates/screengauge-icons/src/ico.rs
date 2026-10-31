//! `.ico` files: a directory of square pictures, each at its own size.
//!
//! Pictures under 256 pixels are 32-bit bitmaps (BITMAPINFOHEADER, rows from the bottom, then the 1-bit AND mask),
//! which every Windows component reads; the 256-pixel picture is a PNG, as Windows has stored large icons since
//! Vista, so the file stays small.

use windows::Win32::Foundation::E_FAIL;
use windows::core::Error;

use crate::image::{Image, Rgba, widen};
use crate::png;

const DIRECTORY: usize = 6;
const ENTRY: usize = 16;
const BITMAP_HEADER: u32 = 40;

/// One picture as the file stores it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub size: u32,
    pub bit_count: u16,
    pub data: Vec<u8>,
}

impl Entry {
    pub fn is_png(&self) -> bool {
        self.data.starts_with(b"\x89PNG")
    }

    /// The picture, decoded.
    pub fn image(&self) -> Result<Image, String> {
        if self.is_png() {
            png::decode(&self.data).map_err(|e| format!("{} px PNG: {e}", self.size))
        } else {
            from_bitmap(self.size, &self.data)
        }
    }
}

/// The `.ico` file holding `images`, in the order given; each must be square, 256 pixels at most.
pub fn write(images: &[Image]) -> windows::core::Result<Vec<u8>> {
    let entries: Vec<Entry> = images
        .iter()
        .map(|image| {
            assert_eq!(image.width(), image.height(), "icons are square");
            assert!(image.width() <= 256, "an .ico holds 256 pixels at most");
            let data = if image.width() == 256 {
                png::encode(image)?
            } else {
                to_bitmap(image)
            };
            Ok(Entry {
                size: image.width(),
                bit_count: 32,
                data,
            })
        })
        .collect::<windows::core::Result<_>>()?;
    let count = u16::try_from(entries.len())
        .map_err(|_| Error::new(E_FAIL, "an .ico holds at most 65535 pictures"))?;
    let mut file = Vec::new();
    file.extend_from_slice(&[0, 0]);
    file.extend_from_slice(&1u16.to_le_bytes());
    file.extend_from_slice(&count.to_le_bytes());
    let mut offset = DIRECTORY + ENTRY * entries.len();
    for entry in &entries {
        // A size of 256 is written as 0.
        let side = u8::try_from(entry.size).unwrap_or(0);
        file.extend_from_slice(&[side, side, 0, 0]);
        file.extend_from_slice(&1u16.to_le_bytes());
        file.extend_from_slice(&entry.bit_count.to_le_bytes());
        file.extend_from_slice(&length_of(entry.data.len())?.to_le_bytes());
        file.extend_from_slice(&length_of(offset)?.to_le_bytes());
        offset += entry.data.len();
    }
    for entry in &entries {
        file.extend_from_slice(&entry.data);
    }
    Ok(file)
}

/// The pictures in an `.ico` file, in its order.
pub fn read(file: &[u8]) -> Result<Vec<Entry>, String> {
    let u16_at = |at: usize| -> Result<u16, String> {
        file.get(at..at + 2)
            .map(|b| u16::from_le_bytes([b[0], b[1]]))
            .ok_or_else(|| format!("the file ends at byte {at}"))
    };
    let u32_at = |at: usize| -> Result<u32, String> {
        file.get(at..at + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .ok_or_else(|| format!("the file ends at byte {at}"))
    };
    if u16_at(0)? != 0 || u16_at(2)? != 1 {
        return Err("not an .ico file".into());
    }
    let count = usize::from(u16_at(4)?);
    (0..count)
        .map(|i| {
            let at = DIRECTORY + ENTRY * i;
            let side = *file.get(at).ok_or("the directory is cut short")?;
            let size = if side == 0 { 256 } else { u32::from(side) };
            let bytes = widen(u32_at(at + 8)?);
            let start = widen(u32_at(at + 12)?);
            let data = file
                .get(start..start + bytes)
                .ok_or_else(|| format!("the {size} px picture lies outside the file"))?
                .to_vec();
            Ok(Entry {
                size,
                bit_count: u16_at(at + 6)?,
                data,
            })
        })
        .collect()
}

fn to_bitmap(image: &Image) -> Vec<u8> {
    let size = image.width();
    let mask_row = mask_row_bytes(size);
    let pixels = size * size * 4;
    let mut data = Vec::new();
    data.extend_from_slice(&BITMAP_HEADER.to_le_bytes());
    data.extend_from_slice(&size.to_le_bytes());
    // The height counts the color rows and the mask rows.
    data.extend_from_slice(&(size * 2).to_le_bytes());
    data.extend_from_slice(&1u16.to_le_bytes());
    data.extend_from_slice(&32u16.to_le_bytes());
    data.extend_from_slice(&0u32.to_le_bytes());
    data.extend_from_slice(&(pixels + mask_row * size).to_le_bytes());
    data.extend_from_slice(&[0; 16]);
    for y in (0..size).rev() {
        for x in 0..size {
            let p = image.pixel(x, y);
            data.extend_from_slice(&[p.b, p.g, p.r, p.a]);
        }
    }
    // The AND mask: a set bit where the picture is fully transparent.
    for y in (0..size).rev() {
        let mut row = vec![0u8; widen(mask_row)];
        for x in 0..size {
            if image.pixel(x, y).a == 0 {
                row[widen(x / 8)] |= 0x80 >> (x % 8);
            }
        }
        data.extend_from_slice(&row);
    }
    data
}

fn from_bitmap(size: u32, data: &[u8]) -> Result<Image, String> {
    let field = |at: usize| -> Result<u32, String> {
        data.get(at..at + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .ok_or_else(|| format!("the {size} px bitmap is cut short"))
    };
    if field(0)? != BITMAP_HEADER || field(4)? != size || field(8)? != size * 2 {
        return Err(format!(
            "the {size} px bitmap's header doesn't match its size"
        ));
    }
    if data.get(14..16) != Some(&32u16.to_le_bytes()[..]) {
        return Err(format!("the {size} px bitmap isn't 32-bit"));
    }
    let start = widen(BITMAP_HEADER);
    let row = widen(size) * 4;
    let mut image = Image::new(size, size);
    for y in 0..size {
        let from = start + widen(size - 1 - y) * row;
        let line = data
            .get(from..from + row)
            .ok_or_else(|| format!("the {size} px bitmap is cut short"))?;
        for (x, &[b, g, r, a]) in (0..size).zip(line.as_chunks::<4>().0) {
            image.set_pixel(x, y, Rgba { r, g, b, a });
        }
    }
    Ok(image)
}

/// Mask rows are 1 bit per pixel, padded to 4 bytes.
fn mask_row_bytes(size: u32) -> u32 {
    size.div_ceil(32) * 4
}

fn length_of(bytes: usize) -> windows::core::Result<u32> {
    u32::try_from(bytes).map_err(|_| Error::new(E_FAIL, "an .ico file must stay below 4 GB"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn marked(size: u32) -> Image {
        let mut image = Image::new(size, size);
        // Top left corner only, so a picture stored upside down or mirrored fails.
        image.set_pixel(0, 0, Rgba::hex(0xF97316));
        image.set_pixel(1, 0, Rgba::hex(0x7C2D12).with_opacity(0.5));
        image
    }

    #[test]
    fn pictures_come_back_as_they_went_in() {
        let images = [marked(16), marked(20), marked(256)];
        let entries = read(&write(&images).expect("write")).expect("read");
        assert_eq!(
            entries.iter().map(|e| e.size).collect::<Vec<_>>(),
            [16, 20, 256]
        );
        for (entry, image) in entries.iter().zip(&images) {
            assert_eq!(&entry.image().expect("decode"), image, "{}", entry.size);
        }
    }

    #[test]
    fn only_the_256_pixel_picture_is_a_png() {
        let entries = read(&write(&[marked(48), marked(256)]).expect("write")).expect("read");
        assert!(!entries[0].is_png());
        assert!(entries[1].is_png());
    }

    #[test]
    fn the_mask_marks_the_transparent_pixels() {
        let bitmap = to_bitmap(&marked(16));
        let mask_start = 40 + 16 * 16 * 4;
        // The top row is the last mask row; its first two pixels are opaque, the rest transparent.
        let top = &bitmap[mask_start + 15 * 4..mask_start + 16 * 4];
        assert_eq!(top, [0b0011_1111, 0xFF, 0, 0]);
    }
}
