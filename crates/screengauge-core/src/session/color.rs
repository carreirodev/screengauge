//! The color Inspect shows and copies (v1 spec WPC-296, Inspect): the exact pixel under the cursor, in HEX and in RGB.

use crate::settings::ColorFormat;

/// A color's two texts in Color format's order. The readout and the clipboard both take their text from here, so what
/// is copied is always what is on screen.
pub(super) struct Texts {
    /// The one the readout shows first and the click copies: HEX, or RGB when Color format is RGB.
    pub(super) first: String,
    /// The other one, which Shift + click copies whether it shows or not.
    pub(super) other: String,
    /// Whether the readout shows `other` under `first`: with Color format at Both, HEX above RGB (the owner's choice in
    /// WPC-316).
    pub(super) other_shows: bool,
}

/// `color`'s texts in `format`.
pub(super) fn texts(color: [u8; 3], format: ColorFormat) -> Texts {
    let (first, other) = match format {
        ColorFormat::Hex | ColorFormat::Both => (hex(color), rgb(color)),
        ColorFormat::Rgb => (rgb(color), hex(color)),
    };
    Texts {
        first,
        other,
        other_shows: format == ColorFormat::Both,
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
