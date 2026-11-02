//! The Snip's card, which has a shape of its own (v1 spec WPC-296, Settings; WPC-322): two rows that are not a share of
//! the card's height like the other blocks' (Destination, then "Save to default folder"), the line of the folder under
//! them, with its path and its "Change…" button, and, with the clipboard as the destination, the note under that line.
//! This is where each part goes in logical pixels, and the cut of a long path in its middle; `paint::snip` draws it.
//! The sizes are the prototype's stylesheet (`.srow`, `.fline`, `.fpath`, `.pbtn`, `.fnote`), measured in a browser for
//! WPC-319 as the card's height of 145.28 with the note.

use screengauge_core::settings::{Options, SnipDestination};

use super::layout::{Block, CARD_BORDER, Page, Span};

pub const DESTINATION_LABEL: &str = "Destination";
pub const SAVE_LABEL: &str = "Save to default folder";
pub const CHANGE_WORD: &str = "Change\u{2026}";
pub const NO_FOLDER: &str = "No folder chosen";
pub const NOTE: &str = "Only applies when the destination includes a file.";

/// The choices of "Destination", in the spec's order, with their words.
pub const DESTINATIONS: [(SnipDestination, &str); 3] = [
    (SnipDestination::Clipboard, "Clipboard"),
    (SnipDestination::File, "File"),
    (SnipDestination::Both, "Both"),
];

/// The first row, Destination (`.srow`'s `min-height`), and the second, the switch's, under the card's line, which the
/// first row's height includes: 42 each, the line taking one pixel of the second.
const FIRST_ROW: u32 = 42;
const SECOND_ROW: u32 = 41;

/// The folder's line: its path's box and the button have the same height (`.fpath` and `.pbtn`), with 9 pixels under
/// them (`.fline`'s padding) when the note isn't shown.
const FIELD: u32 = 30;
const BELOW_FIELD: u32 = 9;

/// The note's line (`.fnote`, 11.5 tall on a line-height of 1.3), the room between the path's box and its text (a
/// padding of 9 less a margin of 3), and the room under it. The prototype's 9 under it is 8 here: the card is 145 tall
/// in whole pixels with the note, against the 145.28 measured, and the pixel comes from there.
const NOTE_ABOVE: u32 = 6;
const NOTE_LINE: u32 = 15;
const NOTE_BELOW: u32 = 8;

/// The room between the path's box and the button (`.fline`'s `gap`), the path's room left and right of its text
/// (`.fpath`'s padding), the button's of its word (`.pbtn`'s padding), and their corners.
pub const FIELD_GAP: u32 = 8;
pub const PATH_PADDING: u32 = 10;
pub const BUTTON_PADDING: u32 = 11;
pub const PATH_RADIUS: u32 = 8;
pub const BUTTON_RADIUS: u32 = 9;
pub const FIELD_HEIGHT: u32 = FIELD;

/// The path's text (`.fpath`: `font-size: 12px`).
pub const PATH_TEXT: f32 = 12.0;

/// How much shorter the card is without the note: the room the note takes less the room it replaces.
const NOTE_ROOM: u32 = NOTE_ABOVE + NOTE_LINE + NOTE_BELOW - BELOW_FIELD;

/// Whether the note under the folder's line is shown: with the clipboard as the destination, where the folder only
/// waits for a destination that includes a file.
pub fn note_shown(options: &Options) -> bool {
    options.snip_destination == SnipDestination::Clipboard
}

/// Where the card's parts are on the page, in logical pixels from its top.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Parts {
    /// The Destination row, inside the card's border.
    pub destination: Span,
    /// The row of "Save to default folder", and the line above it.
    pub save: Span,
    pub line: Span,
    /// The path's box and the button, which share their top and bottom.
    pub field: Span,
    /// The note's line, when it is shown.
    pub note: Option<Span>,
    /// The card's bottom.
    pub bottom: u32,
}

/// The parts of a card whose top is at `top`, with the note or without it.
pub fn parts(top: u32, note: bool) -> Parts {
    let destination = Span {
        top: top + CARD_BORDER,
        bottom: top + CARD_BORDER + FIRST_ROW,
    };
    let line = Span {
        top: destination.bottom,
        bottom: destination.bottom + 1,
    };
    let save = Span {
        top: line.bottom,
        bottom: line.bottom + SECOND_ROW,
    };
    let field = Span {
        top: save.bottom,
        bottom: save.bottom + FIELD,
    };
    let (note, inside) = if note {
        let line = Span {
            top: field.bottom + NOTE_ABOVE,
            bottom: field.bottom + NOTE_ABOVE + NOTE_LINE,
        };
        (Some(line), line.bottom + NOTE_BELOW)
    } else {
        (None, field.bottom + BELOW_FIELD)
    };
    Parts {
        destination,
        save,
        line,
        field,
        note,
        bottom: inside + CARD_BORDER,
    }
}

/// Where the Snip's card begins on `page`, in logical pixels.
pub fn top_of(page: &Page) -> Option<u32> {
    page.blocks
        .iter()
        .find(|placed| placed.block == Block::Snip)
        .map(|placed| placed.card.top)
}

/// `page` with the Snip's card as tall as it is with or without its note. The page, and so the window, is as tall as
/// the card is with the note; without it the card is shorter and the footer stays at the bottom (the prototype's card
/// is just as tall as what it holds).
pub fn fitted(mut page: Page, note: bool) -> Page {
    if !note
        && let Some(snip) = page
            .blocks
            .iter_mut()
            .find(|placed| placed.block == Block::Snip)
    {
        snip.card.bottom -= NOTE_ROOM;
    }
    page
}

/// `path` cut in its middle with an ellipsis until `fits`, which says whether a text is narrow enough, accepts it; as
/// much of it is kept as fits, a third of the characters kept at the start (the drive) and the rest at the end (the
/// folder's own name, which says most). A path that fits is kept whole. When nothing of it fits with the ellipsis, the
/// ellipsis alone stays.
pub fn middle_cut(path: &str, fits: impl Fn(&str) -> bool) -> String {
    if fits(path) {
        return path.to_owned();
    }
    let chars: Vec<char> = path.chars().collect();
    let cut = |kept: usize| -> String {
        let head = kept / 3;
        let tail = kept - head;
        let mut text: String = chars[..head].iter().collect();
        text.push('\u{2026}');
        text.extend(&chars[chars.len() - tail..]);
        text
    };
    // The most characters kept that still fit: one fewer than all of them is already a cut. Fewer characters are never
    // wider, so the search halves what is left.
    let (mut fitting, mut over) = (0, chars.len());
    while over - fitting > 1 {
        let middle = fitting + (over - fitting) / 2;
        if fits(&cut(middle)) {
            fitting = middle;
        } else {
            over = middle;
        }
    }
    cut(fitting)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings_window::layout;

    /// Narrow enough when it has at most `width` characters: the cut's ellipsis counts as one.
    fn up_to(width: usize) -> impl Fn(&str) -> bool {
        move |text| text.chars().count() <= width
    }

    #[test]
    fn a_path_that_fits_is_kept_whole() {
        let path = r"C:\Users\Ana\Pictures";
        assert_eq!(middle_cut(path, up_to(path.chars().count())), path);
        assert_eq!(middle_cut(path, up_to(100)), path);
    }

    #[test]
    fn a_path_too_long_is_cut_in_the_middle_keeping_its_start_and_its_end() {
        let path = r"C:\Users\Ana\Pictures\Screenshots\Design Reviews\2026";
        let cut = middle_cut(path, up_to(24));
        assert_eq!(cut.chars().count(), 24, "{cut}");
        assert!(cut.starts_with(r"C:\U"), "{cut}");
        assert!(cut.ends_with(r"gn Reviews\2026"), "{cut}");
        assert_eq!(cut.matches('\u{2026}').count(), 1, "{cut}");
    }

    #[test]
    fn the_drive_stays_and_most_of_what_is_kept_is_the_end() {
        let path = r"D:\Projects\ScreenGauge\Exports\Snips\Final";
        let cut = middle_cut(path, up_to(20));
        let (head, tail) = cut.split_once('\u{2026}').expect("an ellipsis");
        assert!(head.starts_with(r"D:\"), "{cut}");
        assert!(tail.chars().count() > head.chars().count(), "{cut}");
        assert!(tail.ends_with("Final"), "{cut}");
    }

    #[test]
    fn a_wider_room_keeps_at_least_as_much_of_the_path() {
        let path = r"\\server\share\Design Reviews\2026\Q4\Buttons and icons";
        let mut last = 0;
        for width in 1..=path.chars().count() {
            let cut = middle_cut(path, up_to(width));
            let kept = cut.chars().filter(|&c| c != '\u{2026}').count();
            assert!(kept >= last, "{width}: {cut}");
            assert!(cut.chars().count() <= width.max(1), "{width}: {cut}");
            last = kept;
        }
    }

    #[test]
    fn with_no_room_for_anything_the_ellipsis_alone_stays() {
        assert_eq!(middle_cut(r"C:\Users\Ana", up_to(1)), "\u{2026}");
        assert_eq!(middle_cut(r"C:\Users\Ana", up_to(0)), "\u{2026}");
    }

    #[test]
    fn a_character_beyond_ascii_is_never_split() {
        let path = "C:\\Usu\u{e1}rios\\Jo\u{e3}o\\Imagens\\Capturas de tela\\\u{1f4f7} fotos";
        for width in 1..=path.chars().count() {
            // Slicing inside a character would have panicked; what is kept is whole characters of the path.
            let cut = middle_cut(path, up_to(width));
            assert!(
                cut.chars().all(|c| c == '\u{2026}' || path.contains(c)),
                "{cut}"
            );
        }
        let cut = middle_cut(path, up_to(20));
        assert!(cut.ends_with("fotos"), "{cut}");
    }

    #[test]
    fn a_path_of_one_character_that_does_not_fit_is_the_ellipsis() {
        assert_eq!(middle_cut("X", up_to(0)), "\u{2026}");
    }

    #[test]
    fn the_card_with_its_note_is_the_145_pixels_the_page_reserves() {
        let card = layout::page().blocks[3].card;
        let parts = parts(card.top, true);
        assert_eq!(parts.bottom, card.bottom);
        // The note's line ends 8 above the card's border.
        assert_eq!(
            parts.note.map(|note| note.bottom + 8 + 1),
            Some(card.bottom)
        );
    }

    #[test]
    fn the_rows_and_the_folder_s_line_follow_one_another_with_a_line_between_the_rows() {
        let parts = parts(100, true);
        // Destination: 42 inside the border; the line; the switch's row; the folder's line of 30.
        assert_eq!(
            parts.destination,
            Span {
                top: 101,
                bottom: 143
            }
        );
        assert_eq!(
            parts.line,
            Span {
                top: 143,
                bottom: 144
            }
        );
        assert_eq!(
            parts.save,
            Span {
                top: 144,
                bottom: 185
            }
        );
        assert_eq!(
            parts.field,
            Span {
                top: 185,
                bottom: 215
            }
        );
        assert_eq!(
            parts.note,
            Some(Span {
                top: 221,
                bottom: 236
            })
        );
        assert_eq!(parts.bottom, 245);
    }

    #[test]
    fn without_the_note_the_card_ends_9_pixels_under_the_folder_s_line() {
        let with = parts(100, true);
        let without = parts(100, false);
        assert_eq!(without.note, None);
        assert_eq!(without.field, with.field);
        assert_eq!(without.bottom, without.field.bottom + 9 + 1);
        // 20 pixels shorter than with it, which is what `fitted` takes off the reserved card.
        assert_eq!(with.bottom - without.bottom, 20);
    }

    #[test]
    fn only_the_clipboard_shows_the_note() {
        let mut options = Options::default();
        assert!(
            note_shown(&options),
            "the default destination is the clipboard"
        );
        options.snip_destination = SnipDestination::File;
        assert!(!note_shown(&options));
        options.snip_destination = SnipDestination::Both;
        assert!(!note_shown(&options));
    }

    #[test]
    fn fitting_the_page_shortens_only_the_snip_s_card_and_only_without_the_note() {
        let page = layout::page();
        assert_eq!(fitted(page.clone(), true), page);
        let shorter = fitted(page.clone(), false);
        assert_eq!(
            shorter.blocks[3].card.bottom,
            parts(page.blocks[3].card.top, false).bottom
        );
        assert_eq!(shorter.blocks[..3], page.blocks[..3]);
        // The footer, and so the window, stay where they were.
        assert_eq!(shorter.footer, page.footer);
        assert_eq!(shorter.height, page.height);
    }
}
