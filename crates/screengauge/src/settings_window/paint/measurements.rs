//! The Measurements block's rows' controls laid out (v1 spec WPC-296, Settings; WPC-320): the default tolerance's − and
//! + around its number, and the switch of "Also show logical pixels".

use screengauge_core::settings::Options;

use super::Rows;
use crate::settings_window::controls::{self, Control, Laid};

impl Rows<'_> {
    /// The default tolerance's − and + around the number `options` hold, at the right of the row from `top` to
    /// `bottom`, ending at `right`.
    pub(super) fn default_tolerance(
        &self,
        options: &Options,
        (right, top, bottom): (i32, i32, i32),
    ) -> Laid {
        controls::stepper(
            &self.canvas,
            options.default_tolerance.get(),
            right,
            top,
            bottom,
        )
    }

    /// "Also show logical pixels"' switch, on as `options` hold it.
    pub(super) fn logical_pixels(
        &self,
        options: &Options,
        (right, top, bottom): (i32, i32, i32),
    ) -> Laid {
        controls::switch(
            &self.canvas,
            Control::LogicalPixels,
            options.also_show_logical_pixels,
            right,
            top,
            bottom,
        )
    }
}
