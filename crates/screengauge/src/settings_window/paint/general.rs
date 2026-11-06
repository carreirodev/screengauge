//! The General block's rows' controls laid out (v1 spec WPC-296, Settings): the global shortcut's field with Reset
//! beside it (WPC-323), the switch of "Start with Windows" (WPC-326), and the theme's choice (WPC-320).

use screengauge_core::settings::Options;
use windows::Win32::Graphics::DirectWrite::IDWriteTextLayout;

use super::{Rows, Shown, text, width_of};
use crate::settings_window::controls::{self, Control, LabelKind, Laid, THEMES};
use crate::settings_window::recorder_field;

impl Rows<'_> {
    /// The global shortcut's field and Reset at the right of the row from `top` to `bottom`, ending at `right`. The
    /// field comes back; Reset goes into `laid`.
    pub(super) fn shortcut(
        &self,
        shown: &Shown,
        measure: &impl Fn(&str, LabelKind) -> windows::core::Result<f32>,
        (right, top, bottom): (i32, i32, i32),
        laid: &mut Vec<Laid>,
    ) -> windows::core::Result<Laid> {
        let (field, reset) =
            recorder_field::lay(&shown.recorder, self.on.dpi, measure, right, top, bottom)?;
        laid.push(Laid::Button {
            control: Control::ShortcutReset,
            place: reset,
            word: recorder_field::RESET,
        });
        Ok(Laid::Shortcut(field))
    }

    /// "Start with Windows"' switch, on as Windows says.
    pub(super) fn start_with_windows(
        &self,
        shown: &Shown,
        (right, top, bottom): (i32, i32, i32),
    ) -> Laid {
        controls::switch(
            &self.canvas,
            Control::StartWithWindows,
            shown.start_with_windows,
            right,
            top,
            bottom,
        )
    }

    /// The theme's choice, with the theme `options` hold lit.
    pub(super) fn theme(
        &self,
        options: &Options,
        (right, top, bottom): (i32, i32, i32),
    ) -> windows::core::Result<Laid> {
        let mut choices = Vec::new();
        for (theme, word) in THEMES {
            let width = width_of(&self.segment_text(word)?)?;
            choices.push((Control::Theme(theme), word, width, options.theme == theme));
        }
        Ok(controls::choice(&self.canvas, &choices, right, top, bottom))
    }

    /// `word` laid out in the font of a choice's segment.
    pub(super) fn segment_text(&self, word: &str) -> windows::core::Result<IDWriteTextLayout> {
        text(self.on.fonts, word, &self.segment, None)
    }
}
