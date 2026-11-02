//! The Snip's card drawn (v1 spec WPC-296, Settings; WPC-322): its two rows' labels and line, the choice of
//! destination, the switch, the folder's line with its path cut in the middle to fit and its "Change…" button, and the
//! note. Where each part goes is `snip_card`'s; the controls are `controls`'.

use std::cell::RefCell;

use screengauge_core::session::toolbar::scaled;
use screengauge_core::settings::Options;
use windows::Win32::Foundation::{E_FAIL, RECT};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_CLUSTER_METRICS, DWRITE_FONT_WEIGHT_NORMAL, IDWriteTextFormat,
};
use windows::core::Error;

use super::{Placed, Rows, draw_text, place, px_i, text, to_i32, width_of};
use crate::settings_window::controls::{self, Control, Laid};
use crate::settings_window::layout::{self, Page, Span};
use crate::settings_window::snip_card::{self, DESTINATIONS};

/// The path's text, made while the card is laid out and drawn once its box is: the text goes over the box.
pub(super) struct PathText {
    placed: Placed,
    /// Whether it is in the ink, as a folder is, or in the dim color, as "No folder chosen" is.
    dim: bool,
}

/// What the Snip's card gives the page: the controls it laid out, and its path's text.
pub(super) struct Snip {
    pub laid: Vec<Laid>,
    pub path: PathText,
}

impl Rows<'_> {
    /// Draws the card's labels, its line and its note, which nothing is drawn over, and lays out its controls and its
    /// path's text for after their surfaces. `page` is the page as it is drawn, the card's top on it.
    pub(super) fn snip(&self, options: &Options, page: &Page) -> windows::core::Result<Snip> {
        let on = self.on;
        let dpi = on.dpi;
        let fonts = on.fonts;
        let top = snip_card::top_of(page)
            .ok_or_else(|| Error::new(E_FAIL, "the page has no Snip block"))?;
        let parts = snip_card::parts(top, snip_card::note_shown(options));
        let rows = |span: Span| -> windows::core::Result<(i32, i32)> {
            Ok((
                to_i32(scaled(span.top, dpi))?,
                to_i32(scaled(span.bottom, dpi))?,
            ))
        };
        let inner = to_i32(scaled(
            layout::SIDE + layout::CARD_BORDER + layout::ROW_PADDING,
            dpi,
        ))?;
        let right = to_i32(on.width)? - inner;
        let (destination, save, field) = (
            rows(parts.destination)?,
            rows(parts.save)?,
            rows(parts.field)?,
        );

        self.row_label(snip_card::DESTINATION_LABEL, inner, destination)?;
        self.separator(to_i32(on.width)?, save.0)?;
        self.row_label(snip_card::SAVE_LABEL, inner, save)?;
        if let Some(note) = parts.note {
            let (top, bottom) = rows(note)?;
            let line = place(
                text(fonts, snip_card::NOTE, &self.note, None)?,
                px_i(inner),
                px_i(top),
                px_i(bottom - top),
            )?;
            draw_text(on.target, &line, &self.dim);
        }

        let mut choices = Vec::new();
        for (destination, word) in DESTINATIONS {
            let width = width_of(&text(fonts, word, &self.segment, None)?)?;
            choices.push((
                Control::SnipDestination(destination),
                word,
                width,
                options.snip_destination == destination,
            ));
        }
        let button = controls::button_place(
            &self.canvas,
            width_of(&text(fonts, snip_card::CHANGE_WORD, &self.segment, None)?)?,
            right,
            field.0,
            field.1,
        );
        let gap = to_i32(scaled(snip_card::FIELD_GAP, dpi))?;
        let path = controls::path_place(&self.canvas, inner, button.left - gap, field.0, field.1);
        let laid = vec![
            controls::choice(&self.canvas, &choices, right, destination.0, destination.1),
            controls::switch(
                &self.canvas,
                Control::SaveToDefaultFolder,
                options.default_folder.is_on(),
                right,
                save.0,
                save.1,
            ),
            Laid::Path { place: path },
            Laid::Button {
                control: Control::ChangeFolder,
                place: button,
                word: snip_card::CHANGE_WORD,
            },
        ];
        let path = self.path_text(options, path)?;
        Ok(Snip { laid, path })
    }

    /// Writes the path's text over its box, once the controls' surfaces are drawn.
    pub(super) fn write_path(&self, path: &PathText) {
        let brush = if path.dim { &self.dim } else { &self.ink };
        draw_text(self.on.target, &path.placed, brush);
    }

    /// A row's label at `left`, centered in the row from top to bottom, as the other rows' are.
    fn row_label(
        &self,
        words: &str,
        left: i32,
        (top, bottom): (i32, i32),
    ) -> windows::core::Result<()> {
        let line = layout::LABEL_LINE * self.scale;
        let at = px_i(top) + (px_i(bottom - top) - line) / 2.0;
        let label = place(
            text(self.on.fonts, words, &self.label, None)?,
            px_i(left),
            at,
            line,
        )?;
        draw_text(self.on.target, &label, &self.ink);
        Ok(())
    }

    /// The lengths, in UTF-16 units, of the clusters `words` lays out in `format`: the letters DirectWrite keeps whole.
    fn cluster_lengths(
        &self,
        words: &str,
        format: &IDWriteTextFormat,
    ) -> windows::core::Result<Vec<u16>> {
        let layout = text(self.on.fonts, words, format, None)?;
        // A cluster is at least one unit, so there are never more clusters than units.
        let units = words.encode_utf16().count();
        let mut metrics = vec![DWRITE_CLUSTER_METRICS::default(); units];
        let mut count = 0;
        // SAFETY: fills a buffer with a place for every cluster there can be, and the count it gives.
        unsafe { layout.GetClusterMetrics(Some(&mut metrics), &mut count) }?;
        let count = usize::try_from(count)
            .map_err(|_| Error::new(E_FAIL, "more clusters than the text has units"))?;
        Ok(metrics
            .iter()
            .take(count)
            .map(|cluster| cluster.length)
            .collect())
    }

    /// The text of the box at `place`: the folder's path cut in its middle to fit, or "No folder chosen" in italics and
    /// the dim color when no folder is kept.
    fn path_text(&self, options: &Options, field: RECT) -> windows::core::Result<PathText> {
        let fonts = self.on.fonts;
        let size = snip_card::PATH_TEXT * self.scale;
        let padding = to_i32(scaled(snip_card::PATH_PADDING, self.on.dpi))?;
        let room = px_i((field.right - field.left) - 2 * padding);
        let left = px_i(field.left + padding);
        let (words, format, dim) = match options.default_folder.folder() {
            Some(folder) => {
                let full = folder.as_path().to_string_lossy();
                let format = fonts.text_format(DWRITE_FONT_WEIGHT_NORMAL, size)?;
                // A failure to measure stops the cut at once, and comes back after it, never lost.
                let failed = RefCell::new(None);
                let points = snip_card::cut_points(&full, &self.cluster_lengths(&full, &format)?);
                let cut = snip_card::middle_cut(&full, &points, |candidate| {
                    match text(fonts, candidate, &format, None).and_then(|text| width_of(&text)) {
                        Ok(width) => width <= room,
                        Err(e) => {
                            failed.borrow_mut().get_or_insert(e);
                            true
                        }
                    }
                });
                if let Some(e) = failed.into_inner() {
                    return Err(e);
                }
                (cut, format, false)
            }
            None => (
                snip_card::NO_FOLDER.to_owned(),
                fonts.italic_format(DWRITE_FONT_WEIGHT_NORMAL, size)?,
                true,
            ),
        };
        let placed = place(
            text(fonts, &words, &format, None)?,
            left,
            px_i(field.top),
            px_i(field.bottom - field.top),
        )?;
        Ok(PathText { placed, dim })
    }
}
