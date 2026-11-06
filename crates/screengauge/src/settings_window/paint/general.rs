//! The General block's rows' controls laid out (v1 spec WPC-296, Settings): the global shortcut's field with Reset
//! beside it (WPC-323), the switch of "Start with Windows" (WPC-326), and the theme's choice (WPC-320).

use screengauge_core::settings::Options;

use super::{Rows, Shown};
use crate::settings_window::controls::{self, Control, LabelKind, Laid, RowEnd, THEMES};
use crate::settings_window::recorder_field;

impl Rows<'_> {
    /// The global shortcut's field and Reset at the row's `end`. The field comes back; Reset goes into `laid`.
    pub(super) fn shortcut(
        &self,
        shown: &Shown,
        measure: &impl Fn(&str, LabelKind) -> windows::core::Result<f32>,
        end: RowEnd,
        laid: &mut Vec<Laid>,
    ) -> windows::core::Result<Laid> {
        let (field, reset) = recorder_field::lay(&shown.recorder, self.on.dpi, measure, end)?;
        laid.push(Laid::Button {
            control: Control::ShortcutReset,
            place: reset,
            word: recorder_field::RESET,
        });
        Ok(Laid::Shortcut(field))
    }

    /// "Start with Windows"' switch, on as Windows says.
    pub(super) fn start_with_windows(&self, shown: &Shown, end: RowEnd) -> Laid {
        controls::switch(
            &self.canvas,
            Control::StartWithWindows,
            shown.start_with_windows,
            end,
        )
    }

    /// The theme's choice, with the theme `options` hold lit.
    pub(super) fn theme(&self, options: &Options, end: RowEnd) -> windows::core::Result<Laid> {
        self.choice(&THEMES, options.theme, Control::Theme, end)
    }
}
