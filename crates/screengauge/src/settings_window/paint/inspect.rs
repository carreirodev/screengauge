//! The Inspect block's row's control laid out (v1 spec WPC-296, Settings; WPC-320): the color format's choice.

use screengauge_core::settings::Options;

use super::{Rows, width_of};
use crate::settings_window::controls::{self, COLOR_FORMATS, Control, Laid};

impl Rows<'_> {
    /// The color format's choice, with the format `options` hold lit, at the right of the row from `top` to `bottom`,
    /// ending at `right`.
    pub(super) fn color_format(
        &self,
        options: &Options,
        (right, top, bottom): (i32, i32, i32),
    ) -> windows::core::Result<Laid> {
        let mut choices = Vec::new();
        for (format, word) in COLOR_FORMATS {
            let width = width_of(&self.segment_text(word)?)?;
            let chosen = options.color_format == format;
            choices.push((Control::ColorFormat(format), word, width, chosen));
        }
        Ok(controls::choice(&self.canvas, &choices, right, top, bottom))
    }
}
