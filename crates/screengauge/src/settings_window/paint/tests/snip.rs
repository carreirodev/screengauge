//! The Snip's card drawn on a bitmap (WPC-322): where its controls are, what each state of the options shows, the path cut
//! to fit its box, and the note.

use std::path::Path;

use screengauge_core::settings::{DefaultFolder, Folder, Options, SnipDestination};

use super::*;
use crate::settings_window::snip_card;

const LONG_PATH: &str =
    r"C:\Users\Eduardo\Pictures\Screenshots\Design Reviews\2026\Q4\Buttons and icons\Final exports";

fn folder(path: &str) -> Folder {
    Folder::new(Path::new(path)).expect("an absolute path")
}

fn with(destination: SnipDestination, default_folder: DefaultFolder) -> Options {
    Options {
        snip_destination: destination,
        default_folder,
        ..Options::default()
    }
}

/// The Snip card's parts on the page for `options`, in logical pixels.
fn parts(options: &Options) -> snip_card::Parts {
    let top = snip_card::top_of(&layout::page(&[])).expect("the Snip block");
    snip_card::parts(top, snip_card::note_shown(options))
}

/// A box from `left` to `right`, and from the first logical `top` to the second.
fn rect(left: i32, right: i32, span: Span, dpi: u32) -> RECT {
    RECT {
        left,
        top: whole_px(span.top, dpi),
        right,
        bottom: whole_px(span.bottom, dpi),
    }
}

/// How many pixels of `place` are `within` of `color`.
fn count_near(page: &Picture, place: RECT, color: Color, within: u8) -> usize {
    (place.top..place.bottom)
        .flat_map(|y| (place.left..place.right).map(move |x| (x, y)))
        .filter(|&(x, y)| near(page.at(x, y), color, within))
        .count()
}

/// The path's box, which the controls don't give: from the card's text edge to the gap before the button.
fn path_box(page: &Picture, options: &Options, dpi: u32) -> RECT {
    let button = page.place_of(Control::ChangeFolder);
    rect(
        whole_px(layout::SIDE + 1 + layout::ROW_PADDING, dpi),
        button.left - whole_px(snip_card::FIELD_GAP, dpi),
        parts(options).field,
        dpi,
    )
}

/// The columns of the path's box where text is, from the first to the last: pixels far from the box's own color.
fn text_columns(page: &Picture, place: RECT, dpi: u32) -> Option<(i32, i32)> {
    let inset = whole_px(5, dpi);
    let background = page.at(place.left + 2, place.top + 2);
    let differs = |x: i32| {
        (place.top + inset..place.bottom - inset).any(|y| {
            page.at(x, y)
                .iter()
                .zip(background)
                .map(|(&have, want)| i32::from(have.abs_diff(want)))
                .sum::<i32>()
                > 150
        })
    };
    let columns: Vec<i32> = (place.left + 2..place.right - 2)
        .filter(|&x| differs(x))
        .collect();
    Some((*columns.first()?, *columns.last()?))
}

#[test]
fn the_snip_s_controls_are_right_of_their_rows_where_the_prototype_puts_them() {
    // Measured from the prototype's stylesheet at 100%: every control ends at 531, the card's right side less its border
    // and the row's padding; the switch is 42 × 24, "Change…" 30 tall and the folder's line starts at 29.
    for mode in [Mode::Dark, Mode::Light] {
        let options = Options::default();
        let page = drawn_with(mode, 96, &options, Pointer::default());
        let parts = parts(&options);
        let switch = page.place_of(Control::SaveToDefaultFolder);
        assert_eq!(
            (switch.right, width_of_rect(switch), height_of_rect(switch)),
            (531, 42, 24),
            "{mode:?} the switch"
        );
        let button = page.place_of(Control::ChangeFolder);
        assert_eq!(
            (button.right, height_of_rect(button)),
            (531, 30),
            "{mode:?} the button"
        );
        // "Change…" is its word and 11 of room either side.
        assert!(
            (60..80).contains(&width_of_rect(button)),
            "{mode:?} {button:?}"
        );
        for (destination, _) in snip_card::DESTINATIONS {
            let segment = page.place_of(Control::SnipDestination(destination));
            assert_eq!(height_of_rect(segment), 26, "{mode:?} {destination:?}");
        }
        let both = page.place_of(Control::SnipDestination(SnipDestination::Both));
        assert_eq!(both.right, 531 - 3, "{mode:?} the choice's last segment");
        // Each control is centered in its row.
        let middle = |place: RECT| (place.top + place.bottom) / 2;
        let row_middle = |span: Span| whole_px(span.top + span.bottom, 96) / 2;
        assert_eq!(middle(switch), row_middle(parts.save), "{mode:?}");
        assert_eq!(middle(button), row_middle(parts.field), "{mode:?}");
        assert_eq!(
            middle(page.place_of(Control::SnipDestination(SnipDestination::File))),
            row_middle(parts.destination),
            "{mode:?}"
        );
        // The path's box is 30 tall and ends 8 before the button.
        let path = path_box(&page, &options, 96);
        assert_eq!((path.left, height_of_rect(path)), (29, 30), "{mode:?}");
        assert_eq!(path.right, button.left - 8, "{mode:?}");
    }
}

#[test]
fn the_snip_s_controls_grow_with_the_scale_and_stay_inside_the_card() {
    let options = Options::default();
    for dpi in DPIS {
        let page = drawn_with(Mode::Dark, dpi, &options, Pointer::default());
        let card = layout::page(&[]).blocks[3].card;
        let (top, bottom) = (whole_px(card.top, dpi), whole_px(card.bottom, dpi));
        let right_edge = whole_px(layout::WIDTH - layout::SIDE - 1, dpi);
        let switch = page.place_of(Control::SaveToDefaultFolder);
        let button = page.place_of(Control::ChangeFolder);
        assert_eq!(height_of_rect(switch), whole_px(24, dpi), "{dpi}");
        assert_eq!(height_of_rect(button), whole_px(30, dpi), "{dpi}");
        for control in [
            Control::SnipDestination(SnipDestination::Clipboard),
            Control::SnipDestination(SnipDestination::Both),
            Control::SaveToDefaultFolder,
            Control::ChangeFolder,
        ] {
            let place = page.place_of(control);
            assert!(
                place.top > top && place.bottom < bottom,
                "{dpi} {control:?}"
            );
            assert!(place.right <= right_edge, "{dpi} {control:?}");
        }
    }
}

#[test]
fn only_the_destination_chosen_is_cyan_clipboard_by_default() {
    for mode in [Mode::Dark, Mode::Light] {
        for chosen in [
            SnipDestination::Clipboard,
            SnipDestination::File,
            SnipDestination::Both,
        ] {
            let options = with(chosen, DefaultFolder::default());
            let page = drawn_with(mode, 144, &options, Pointer::default());
            for (destination, _) in snip_card::DESTINATIONS {
                let place = page.place_of(Control::SnipDestination(destination));
                let blue = cyan(&page, place);
                if destination == chosen {
                    assert!(blue > 40.0, "{mode:?} {destination:?} chosen: {blue}");
                } else {
                    assert!(blue < 25.0, "{mode:?} {destination:?}: {blue}");
                }
            }
        }
    }
}

#[test]
fn the_folder_switch_is_cyan_when_on_and_sunken_grey_when_off() {
    for mode in [Mode::Dark, Mode::Light] {
        let off = drawn_with(mode, 144, &Options::default(), Pointer::default());
        let on_options = with(
            SnipDestination::Clipboard,
            DefaultFolder::On(folder(r"C:\Snips")),
        );
        let on = drawn_with(mode, 144, &on_options, Pointer::default());
        let track = off.place_of(Control::SaveToDefaultFolder);
        assert!(cyan(&on, track) > 40.0, "{mode:?} on: {}", cyan(&on, track));
        assert!(
            cyan(&off, track) < 25.0,
            "{mode:?} off: {}",
            cyan(&off, track)
        );
        // Off with a folder kept is as off as with none: only the path differs.
        let kept = with(
            SnipDestination::Clipboard,
            DefaultFolder::Off(Some(folder(r"C:\Snips"))),
        );
        let kept = drawn_with(mode, 144, &kept, Pointer::default());
        assert!(cyan(&kept, track) < 25.0, "{mode:?} off with a folder");
    }
}

#[test]
fn the_labels_are_in_the_ink_and_a_line_runs_between_the_two_rows() {
    let dpi = 144;
    let options = Options::default();
    for mode in [Mode::Dark, Mode::Light] {
        let page = drawn_with(mode, dpi, &options, Pointer::default());
        let parts = parts(&options);
        let left = whole_px(29, dpi);
        for (name, row) in [("Destination", parts.destination), ("Save", parts.save)] {
            let place = rect(left, left + whole_px(150, dpi), row, dpi);
            assert!(
                count_near(&page, place, palette(mode).ink, 30) > 50,
                "{mode:?} {name}"
            );
        }
        let line = whole_px(parts.line.top, dpi);
        let x = whole_px(200, dpi);
        assert_ne!(page.at(x, line), page.at(x, line - 2), "{mode:?}");
        assert_ne!(page.at(x, line), page.at(x, line + 3), "{mode:?}");
    }
}

#[test]
fn with_no_folder_the_box_says_so_in_the_dim_color() {
    let dpi = 144;
    let options = Options::default();
    for mode in [Mode::Dark, Mode::Light] {
        let page = drawn_with(mode, dpi, &options, Pointer::default());
        let path = path_box(&page, &options, dpi);
        let text = RECT {
            left: path.left + whole_px(10, dpi),
            right: path.left + whole_px(10 + 120, dpi),
            ..path
        };
        assert!(
            count_near(&page, text, palette(mode).dim, 40) > 40,
            "{mode:?}"
        );
        assert_eq!(
            count_near(&page, text, palette(mode).ink, 10),
            0,
            "{mode:?}"
        );
        // The text starts 10 in from the box, and the box is darker than the card (a well).
        let (first, _) = text_columns(&page, path, dpi).expect("some text");
        assert!(
            (path.left + whole_px(10, dpi) - 1..=path.left + whole_px(12, dpi)).contains(&first),
            "{mode:?}: {first} in a box from {}",
            path.left
        );
    }
}

#[test]
fn a_folder_kept_shows_its_path_in_the_ink() {
    let dpi = 144;
    for mode in [Mode::Dark, Mode::Light] {
        let options = with(
            SnipDestination::File,
            DefaultFolder::On(folder(r"C:\Snips")),
        );
        let page = drawn_with(mode, dpi, &options, Pointer::default());
        let path = path_box(&page, &options, dpi);
        let text = RECT {
            left: path.left + whole_px(10, dpi),
            right: path.left + whole_px(10 + 60, dpi),
            ..path
        };
        assert!(
            count_near(&page, text, palette(mode).ink, 30) > 30,
            "{mode:?}"
        );
        // A short path is shown whole: no ellipsis, so its text is as narrow as "C:\Snips" is.
        let (first, last) = text_columns(&page, path, dpi).expect("some text");
        assert!(
            last - first < whole_px(70, dpi),
            "{mode:?}: {first}..{last}"
        );
    }
}

#[test]
fn a_long_path_is_cut_to_fit_its_box_at_every_scale_using_the_room_it_has() {
    for dpi in [96, 120, 144, 168] {
        for mode in [Mode::Dark, Mode::Light] {
            let options = with(SnipDestination::File, DefaultFolder::On(folder(LONG_PATH)));
            let page = drawn_with(mode, dpi, &options, Pointer::default());
            let path = path_box(&page, &options, dpi);
            let padding = whole_px(10, dpi);
            let (first, last) = text_columns(&page, path, dpi).expect("some text");
            assert!(
                first >= path.left + padding - 1,
                "{dpi} {mode:?}: starts at {first} in a box from {}",
                path.left
            );
            assert!(
                last <= path.right - padding + 1,
                "{dpi} {mode:?}: ends at {last} in a box to {}",
                path.right
            );
            // It uses what room it has: the next character wouldn't have fitted, so no more than one or two are left.
            assert!(
                last >= path.right - padding - whole_px(24, dpi),
                "{dpi} {mode:?}: ends at {last} in a box to {}",
                path.right
            );
        }
    }
}

#[test]
fn the_note_shows_with_the_clipboard_only_and_the_card_is_as_tall_as_what_it_holds() {
    let dpi = 144;
    let with_note = Options::default();
    let without = with(SnipDestination::Both, DefaultFolder::default());
    for mode in [Mode::Dark, Mode::Light] {
        let shown = drawn_with(mode, dpi, &with_note, Pointer::default());
        let hidden = drawn_with(mode, dpi, &without, Pointer::default());
        // The window is as tall either way.
        assert_eq!(shown.height, hidden.height, "{mode:?}");
        let line = parts(&with_note).note.expect("the note's line");
        let place = rect(whole_px(29, dpi), whole_px(29 + 300, dpi), line, dpi);
        assert!(
            count_near(&shown, place, palette(mode).dim, 40) > 50,
            "{mode:?} the note"
        );
        assert_eq!(
            count_near(&hidden, place, palette(mode).dim, 40),
            0,
            "{mode:?} no note"
        );
        // Where the card ends without the note, it is the background beside it again, and with it still the card.
        let x_in = whole_px(layout::WIDTH - layout::SIDE - 20, dpi);
        let y = whole_px(parts(&without).bottom + 3, dpi);
        assert!(
            brightness(shown.at(x_in, y)) > brightness(shown.at(2, y)),
            "{mode:?} with the note"
        );
        assert_eq!(hidden.at(x_in, y), hidden.at(2, y), "{mode:?} without it");
    }
}

#[test]
fn change_brightens_with_the_cursor_on_it() {
    let dpi = 144;
    let options = Options::default();
    for mode in [Mode::Dark, Mode::Light] {
        let away = drawn_with(mode, dpi, &options, Pointer::default());
        let over = drawn_with(
            mode,
            dpi,
            &options,
            Pointer {
                over: Some(Control::ChangeFolder),
                held: None,
            },
        );
        let button = away.place_of(Control::ChangeFolder);
        let ink = |page: &Picture| {
            (button.top..button.bottom)
                .flat_map(|y| (button.left..button.right).map(move |x| (x, y)))
                .map(|(x, y)| brightness(page.at(x, y)))
                .max()
                .expect("pixels")
        };
        let (a, o) = (ink(&away), ink(&over));
        // The dark palette's sign goes to white, the light one's to near black.
        if mode == Mode::Dark {
            assert!(o > a, "{mode:?}: {o} over, {a} away");
        } else {
            let darkest = |page: &Picture| {
                (button.top..button.bottom)
                    .flat_map(|y| (button.left..button.right).map(move |x| (x, y)))
                    .map(|(x, y)| brightness(page.at(x, y)))
                    .min()
                    .expect("pixels")
            };
            assert!(darkest(&over) < darkest(&away), "{mode:?}");
        }
    }
}

/// Writes the Snip's card in a few states to target\settings-pages, for a look at it.
#[test]
#[ignore = "writes PNG files for a person to look at"]
fn write_the_snip_card_for_a_look() {
    let folder_path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/settings-pages");
    std::fs::create_dir_all(&folder_path).expect("the folder");
    let states = [
        (
            "snip-file-long",
            with(SnipDestination::File, DefaultFolder::On(folder(LONG_PATH))),
        ),
        (
            "snip-both-kept-off",
            with(
                SnipDestination::Both,
                DefaultFolder::Off(Some(folder(r"D:\Snips"))),
            ),
        ),
        (
            "snip-clipboard-on",
            with(
                SnipDestination::Clipboard,
                DefaultFolder::On(folder(r"\\server\share\Design Reviews")),
            ),
        ),
    ];
    for (name, options) in states {
        for mode in [Mode::Dark, Mode::Light] {
            for dpi in [96, 144] {
                let page = drawn_with(mode, dpi, &options, Pointer::default());
                let width = u32::try_from(page.width).expect("small");
                let image = screengauge_core::session::Image::new(width, page.height, page.bgra)
                    .expect("the bitmap's size");
                // On a thread of its own, which the encoder starts COM on in its own way.
                let png = std::thread::spawn(move || crate::png::encode(&image))
                    .join()
                    .expect("the encoder's thread")
                    .expect("the page as a PNG");
                std::fs::write(folder_path.join(format!("{name}-{mode:?}-{dpi}.png")), png)
                    .expect("the PNG");
            }
        }
    }
}
