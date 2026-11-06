//! The Inspect block's row's control laid out (v1 spec WPC-296, Settings; WPC-320): the color format's choice.

use screengauge_core::settings::Options;

use super::Rows;
use crate::settings_window::controls::{COLOR_FORMATS, Control, Laid, RowEnd};

impl Rows<'_> {
    /// The color format's choice, with the format `options` hold lit, at the row's `end`.
    pub(super) fn color_format(
        &self,
        options: &Options,
        end: RowEnd,
    ) -> windows::core::Result<Laid> {
        self.choice(
            &COLOR_FORMATS,
            options.color_format,
            Control::ColorFormat,
            end,
        )
    }
}
