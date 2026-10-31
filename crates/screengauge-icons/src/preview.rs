//! Enlarged pictures of the small app icon sizes on the dark and light taskbars, for judging them pixel by pixel.

use std::fs;
use std::path::Path;

use crate::contrast::{DARK_TASKBAR, LIGHT_TASKBAR};
use crate::image::{Image, Rgba};
use crate::{assets, pixel_art, png};

/// How many screen pixels wide each icon pixel is drawn.
const PIXEL_SIDE: u32 = 12;
const GAP: u32 = 8;

/// Writes `tray-sizes.png` in `folder`: each tray size at real size and enlarged, on the dark taskbar on the left and
/// the light one on the right.
pub fn write(folder: &Path) -> Result<(), String> {
    let backgrounds = [DARK_TASKBAR, LIGHT_TASKBAR];
    let largest = pixel_art::SIZES[pixel_art::SIZES.len() - 1];
    let cell = GAP + largest + GAP + largest * PIXEL_SIDE + GAP;
    let row_height = largest * PIXEL_SIDE + 2 * GAP;
    let rows = u32::try_from(pixel_art::SIZES.len()).map_err(|e| e.to_string())?;
    let mut sheet = Image::new(cell * 2, row_height * rows);
    for (row, size) in (0..).zip(pixel_art::SIZES) {
        let icon = assets::app_icon(size)?;
        for (column, background) in (0..).zip(backgrounds) {
            let left = column * cell;
            let top = row * row_height;
            fill(&mut sheet, left, top, cell, row_height, background);
            draw(&mut sheet, &icon, left + GAP, top + GAP, 1, background);
            draw(
                &mut sheet,
                &icon,
                left + GAP + largest + GAP,
                top + GAP,
                PIXEL_SIDE,
                background,
            );
        }
    }
    fs::create_dir_all(folder).map_err(|e| format!("{}: {e}", folder.display()))?;
    let file = folder.join("tray-sizes.png");
    let bytes = png::encode(&sheet).map_err(|e| format!("encoding the preview: {e}"))?;
    fs::write(&file, bytes).map_err(|e| format!("{}: {e}", file.display()))
}

fn fill(sheet: &mut Image, left: u32, top: u32, width: u32, height: u32, color: Rgba) {
    for y in top..top + height {
        for x in left..left + width {
            sheet.set_pixel(x, y, color);
        }
    }
}

/// `icon` laid over `background`, each pixel a `side` × `side` square.
fn draw(sheet: &mut Image, icon: &Image, left: u32, top: u32, side: u32, background: Rgba) {
    for y in 0..icon.height() {
        for x in 0..icon.width() {
            let color = over(icon.pixel(x, y), background);
            fill(sheet, left + x * side, top + y * side, side, side, color);
        }
    }
}

fn over(front: Rgba, back: Rgba) -> Rgba {
    let alpha = u32::from(front.a);
    let mix = |f: u8, b: u8| {
        // A weighted average of two bytes, so it is a byte too; MAX is never reached by the fallback.
        let value = (u32::from(f) * alpha + u32::from(b) * (255 - alpha) + 127) / 255;
        u8::try_from(value).unwrap_or(u8::MAX)
    };
    Rgba {
        r: mix(front.r, back.r),
        g: mix(front.g, back.g),
        b: mix(front.b, back.b),
        a: 255,
    }
}
