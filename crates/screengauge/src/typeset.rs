//! A line of text DirectWrite laid out, measured, and placed on a surface with its baseline on a whole pixel, so its
//! letters are crisp. The frozen screen's overlay places its texts with it (WPC-391), and Settings and the tray icon's
//! menu can too: nothing here knows where the surface is or what it shows.

use windows::Win32::Graphics::DirectWrite::{
    DWRITE_LINE_METRICS, DWRITE_TEXT_METRICS, IDWriteTextLayout,
};

/// A text laid out, with what is needed to place it.
pub struct Typeset {
    pub layout: IDWriteTextLayout,
    /// The text's width, in whole pixels: the room a box around it counts for it.
    pub width: i32,
    /// The distance from the layout's top to its baseline.
    pub baseline: f32,
    /// The line's height, which `in_line` centers in the line the text is placed in.
    pub line_height: f32,
}

/// A line of text with the place it is drawn at.
pub struct PlacedText {
    pub layout: IDWriteTextLayout,
    /// Where the text's layout starts, in the surface's own pixels. Its baseline is on a whole pixel.
    pub left: f32,
    pub top: f32,
}

impl Typeset {
    /// `layout`, measured.
    pub fn of(layout: IDWriteTextLayout) -> windows::core::Result<Typeset> {
        let mut metrics = DWRITE_TEXT_METRICS::default();
        // SAFETY: fills a local.
        unsafe { layout.GetMetrics(&mut metrics) }?;
        let mut line = [DWRITE_LINE_METRICS::default()];
        let mut lines = 0;
        // SAFETY: fills a one-line buffer, and fails should the text need more.
        unsafe { layout.GetLineMetrics(Some(&mut line), &mut lines) }?;
        let [line] = line;
        Ok(Typeset {
            layout,
            width: whole_up(metrics.width),
            baseline: line.baseline,
            line_height: line.height,
        })
    }

    /// Where the text's baseline goes for it to sit in a line `line` pixels tall whose top is at `top`, the way CSS puts
    /// it there: the line's extra room, above and below the text's own height, is shared equally. It is on a whole pixel,
    /// so the letters are crisp. Texts that share a line sit on the baseline one of them gets here (`on_baseline`).
    pub fn baseline_in(&self, top: f32, line: f32) -> f32 {
        (top + (line - self.line_height) / 2.0 + self.baseline).round()
    }

    /// The text starting at `left`, in a line `line` pixels tall whose top is at `top`, with its baseline where
    /// `baseline_in` puts it.
    pub fn in_line(self, left: f32, top: f32, line: f32) -> PlacedText {
        let baseline = self.baseline_in(top, line);
        self.on_baseline(left, baseline)
    }

    /// The text starting at `left`, with its baseline at `baseline`, which is on a whole pixel.
    pub fn on_baseline(self, left: f32, baseline: f32) -> PlacedText {
        PlacedText {
            top: baseline - self.baseline,
            left,
            layout: self.layout,
        }
    }
}

/// A measured width rounded up to whole pixels, so a box around the text never cuts into it. Widths are a few hundred
/// pixels at most, far inside `i32`, so the cast keeps the whole value; past `i32` it would saturate without a word,
/// which only the assertion watches for, and only in a debug build.
// TODO: convert with a checked float-to-integer conversion once the standard library has one, so a width past `i32`
// fails instead of saturating.
fn whole_up(width: f32) -> i32 {
    debug_assert!((0.0..1e6).contains(&width), "a text {width} pixels wide");
    width.ceil() as i32
}

#[cfg(test)]
mod tests {
    use windows::Win32::Graphics::DirectWrite::DWRITE_FONT_WEIGHT_SEMI_BOLD;

    use super::*;
    use crate::graphics::Fonts;

    /// "Copied" laid out in the labels' semibold, `size` pixels tall.
    fn copied(fonts: &Fonts, size: f32) -> Typeset {
        let format = fonts
            .text_format(DWRITE_FONT_WEIGHT_SEMI_BOLD, size)
            .expect("a format");
        let text: Vec<u16> = "Copied".encode_utf16().collect();
        // SAFETY: the layout copies the text and keeps its own reference to the format.
        let layout = unsafe {
            fonts
                .factory
                .CreateTextLayout(&text, &format, 4096.0, 4096.0)
        }
        .expect("a layout");
        Typeset::of(layout).expect("the metrics")
    }

    #[test]
    fn a_text_in_a_line_has_its_baseline_on_a_whole_pixel_and_its_own_line_centered_in_it() {
        let fonts = Fonts::new().expect("the fonts");
        for size in [11.0, 13.0, 15.0, 23.0] {
            for top in [10.0, 10.25, 10.5, 10.75] {
                for line in [17.0, 18.0, 20.0, 31.0] {
                    let typeset = copied(&fonts, size);
                    let (baseline, height) = (typeset.baseline, typeset.line_height);
                    let placed = typeset.in_line(3.5, top, line);
                    let context = format!("{size}px text in a {line}px line at {top}");
                    assert_eq!(placed.left, 3.5, "{context}");
                    assert_eq!((placed.top + baseline).fract(), 0.0, "{context}");
                    // CSS shares the line's spare room equally above and below the text; the whole pixel moves it by
                    // half a pixel at most.
                    let middle = placed.top + height / 2.0;
                    assert!(
                        (middle - (top + line / 2.0)).abs() <= 0.5,
                        "{context}: middle at {middle}"
                    );
                }
            }
        }
    }

    #[test]
    fn a_text_on_a_baseline_sits_on_it() {
        let fonts = Fonts::new().expect("the fonts");
        let typeset = copied(&fonts, 13.0);
        let baseline = typeset.baseline;
        let placed = typeset.on_baseline(40.0, 27.0);
        assert_eq!(placed.left, 40.0);
        assert_eq!(placed.top + baseline, 27.0);
    }
}
