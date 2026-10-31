//! Units (v1 spec WPC-296, Unidades e vários monitores): every measurement is in physical pixels, with the logical
//! pixels beside it, the physical pixels divided by the scale of the monitor they were measured on. The rule is the same
//! for every tool.

use std::fmt;

use super::{Label, Scale};

/// A length in logical pixels, to the tenth.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct LogicalPixels {
    tenths: u64,
}

impl LogicalPixels {
    /// `physical` pixels on a monitor at `scale`, rounded to the nearest tenth, halves up (v1 spec WPC-296, derived
    /// rule 6). It is worked out in whole numbers, so no tenth is lost to a float and a half is exactly a half.
    pub(super) fn of(physical: u32, scale: Scale) -> LogicalPixels {
        let dpi = u64::from(scale.dpi());
        // physical × 96 / dpi logical pixels are physical × 960 / dpi tenths; adding half the divisor before dividing
        // rounds halves up. physical × 1920 is at most about 8 × 10^12, far inside u64.
        let tenths = (u64::from(physical) * 1920 + dpi) / (2 * dpi);
        LogicalPixels { tenths }
    }
}

/// One decimal place after a point, whatever the language, and a whole number without it: `100`, `114.3`, `0.6`.
impl fmt::Display for LogicalPixels {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (whole, tenth) = (self.tenths / 10, self.tenths % 10);
        if tenth == 0 {
            write!(f, "{whole}")
        } else {
            write!(f, "{whole}.{tenth}")
        }
    }
}

/// What a copy of `lengths`, measured on a monitor at `scale`, puts on the clipboard: the physical pixels, or the logical
/// ones when `logical`, whatever "Also show logical pixels" says. Only the numbers, without "px", width then height with
/// the letter x between them, so they paste into a numeric field: `24`, `24 x 180`, `114.3` (v1 spec WPC-296,
/// Dimensions). At 100% both are the same whole numbers.
pub(super) fn clipboard_text(lengths: &[u32], scale: Scale, logical: bool) -> String {
    lengths
        .iter()
        .map(|&length| {
            if logical {
                LogicalPixels::of(length, scale).to_string()
            } else {
                length.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" x ")
}

/// The label of `lengths` in physical pixels, measured on a monitor at `scale`: one length, or width then height, with
/// the multiplication sign between them. The logical pixels come only when the scale isn't 100% and
/// `also_show_logical_pixels` is on. The option only hides them on the label: [`LogicalPixels`] gives them whatever it
/// says, for what is copied (v1 spec WPC-296, "Shift + clique ... mesmo com 'Also show logical pixels' desligado").
pub(super) fn label(lengths: &[u32], scale: Scale, also_show_logical_pixels: bool) -> Label {
    let join = |parts: Vec<String>| parts.join(" × ");
    let shown = also_show_logical_pixels && scale != Scale::HUNDRED_PERCENT;
    let logical = shown.then(|| {
        join(
            lengths
                .iter()
                .map(|&length| LogicalPixels::of(length, scale).to_string())
                .collect(),
        )
    });
    Label {
        physical: join(lengths.iter().map(u32::to_string).collect()),
        logical,
    }
}
