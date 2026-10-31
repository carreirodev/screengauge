//! The app icon at the tray icon's sizes, 16, 20, 24, 28 and 32 pixels (100% to 200%), drawn pixel by pixel in
//! `pixels/<size>.txt` (WPC-304): at those sizes a scaled drawing blurs, so each pixel is chosen by hand.
//!
//! A file has one line per row and one character per pixel, from the legend below. Lines starting with `;` are notes.

use crate::art::color;
use crate::image::{Image, Rgba};

/// The sizes drawn by hand.
pub const SIZES: [u32; 5] = [16, 20, 24, 28, 32];

/// What each character stands for.
pub const LEGEND: [(char, Rgba); 8] = [
    ('.', Rgba::TRANSPARENT),
    ('O', color::OUTLINE),
    ('h', color::HOLE_OUTLINE),
    ('l', color::ORANGE_LIGHT),
    ('p', color::ORANGE_STEP_1),
    ('q', color::ORANGE_STEP_2),
    ('f', color::ORANGE),
    ('t', color::GRAPHITE),
];

/// The hand-drawn picture at `size`, if that size is drawn by hand.
pub fn hand_tuned(size: u32) -> Option<Result<Image, String>> {
    let text = match size {
        16 => include_str!("../pixels/16.txt"),
        20 => include_str!("../pixels/20.txt"),
        24 => include_str!("../pixels/24.txt"),
        28 => include_str!("../pixels/28.txt"),
        32 => include_str!("../pixels/32.txt"),
        _ => return None,
    };
    Some(parse(size, text).map_err(|e| format!("pixels/{size}.txt: {e}")))
}

/// Reads a `size` × `size` picture from its text.
pub fn parse(size: u32, text: &str) -> Result<Image, String> {
    let rows: Vec<&str> = text
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.starts_with(';') && !line.is_empty())
        .collect();
    if rows.len() != crate::image::widen(size) {
        return Err(format!("{} rows, not {size}", rows.len()));
    }
    let mut image = Image::new(size, size);
    for (y, row) in (0..size).zip(&rows) {
        if row.chars().count() != crate::image::widen(size) {
            return Err(format!(
                "row {y} has {} pixels, not {size}",
                row.chars().count()
            ));
        }
        for (x, symbol) in (0..size).zip(row.chars()) {
            let color = LEGEND
                .iter()
                .find(|(c, _)| *c == symbol)
                .map(|(_, color)| *color)
                .ok_or_else(|| format!("row {y}, column {x}: '{symbol}' isn't in the legend"))?;
            image.set_pixel(x, y, color);
        }
    }
    Ok(image)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_picture_is_read_row_by_row() {
        let image = parse(2, "; a note\nO.\n.f\n").expect("parse");
        assert_eq!(image.pixel(0, 0), color::OUTLINE);
        assert_eq!(image.pixel(1, 0), Rgba::TRANSPARENT);
        assert_eq!(image.pixel(1, 1), color::ORANGE);
    }

    #[test]
    fn a_wrong_size_or_symbol_is_refused() {
        assert!(parse(2, "O.\n").is_err());
        assert!(parse(2, "O..\n.f\n").is_err());
        assert!(parse(2, "O.\n.z\n").is_err());
    }

    #[test]
    fn every_hand_drawn_size_reads() {
        for size in SIZES {
            let image = hand_tuned(size).expect("drawn by hand").expect("valid");
            assert_eq!(image.width(), size);
        }
    }
}
