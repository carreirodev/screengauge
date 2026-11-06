//! The Measurements block's rows' controls laid out (v1 spec WPC-296, Settings; WPC-320): the default tolerance's − and
//! + around its number, and the switch of "Also show logical pixels".

use screengauge_core::settings::Options;

use super::Rows;
use crate::settings_window::controls::{self, Control, Laid, RowEnd};

impl Rows<'_> {
    /// The default tolerance's − and + around the number `options` hold, at the row's `end`.
    pub(super) fn default_tolerance(&self, options: &Options, end: RowEnd) -> Laid {
        controls::stepper(&self.canvas, options.default_tolerance.get(), end)
    }

    /// "Also show logical pixels"' switch, on as `options` hold it.
    pub(super) fn logical_pixels(&self, options: &Options, end: RowEnd) -> Laid {
        controls::switch(
            &self.canvas,
            Control::LogicalPixels,
            options.also_show_logical_pixels,
            end,
        )
    }
}
