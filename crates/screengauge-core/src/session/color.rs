//! The color Inspect shows and copies (v1 spec WPC-296, Inspect): the exact pixel under the cursor, in HEX and in RGB.

use crate::settings::ColorFormat;

/// `color`'s two texts in `format`'s order. The first is the one the strip shows first and the click copies: HEX, or
/// RGB when Color format is RGB. The second is the other, which the strip shows under the first only with Both, and
/// which Shift + click copies whether it shows or not. The strip and the clipboard both take their text from here, so
/// what is copied is always what is on screen.
pub(super) fn texts(color: [u8; 3], format: ColorFormat) -> [String; 2] {
    match format {
        ColorFormat::Hex | ColorFormat::Both => [hex(color), rgb(color)],
        ColorFormat::Rgb => [rgb(color), hex(color)],
    }
}

/// `#1E90FF`: a hash, then red, green and blue in two uppercase hex digits each, with no transparency.
fn hex([red, green, blue]: [u8; 3]) -> String {
    format!("#{red:02X}{green:02X}{blue:02X}")
}

/// `30, 144, 255`: red, green and blue in decimal, a comma and a space between them.
fn rgb([red, green, blue]: [u8; 3]) -> String {
    format!("{red}, {green}, {blue}")
}
