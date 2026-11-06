//! The General block drawn on a bitmap: the global shortcut's field, "Start with Windows", the theme's choice, and
//! the messages and bands under their labels.

use super::*;

#[test]
fn the_theme_s_choice_lights_the_theme_chosen_automatic_by_default() {
    for mode in [Mode::Dark, Mode::Light] {
        for (theme, _) in THEMES {
            let page = drawn_with(
                mode,
                144,
                &Options {
                    theme,
                    ..Options::default()
                },
                Pointer::default(),
            );
            for (other, _) in THEMES {
                let lit = cyan(&page, page.place_of(Control::Theme(other)));
                if other == theme {
                    assert!(lit > 50.0, "{mode:?} {theme:?}: {lit}");
                } else {
                    assert!(lit.abs() < 25.0, "{mode:?} {theme:?} {other:?}: {lit}");
                }
            }
        }
    }
    let page = drawn(Mode::Light, 144);
    assert!(cyan(&page, page.place_of(Control::Theme(Theme::Automatic))) > 50.0);
}

#[test]
fn the_theme_s_choice_is_in_general_s_last_row_at_the_right_like_the_others() {
    // Measured in the prototype at 100%: "Automatic · Light · Dark" is 181.67 wide and ends at 531.
    let page = drawn(Mode::Dark, 96);
    let automatic = page.place_of(Control::Theme(Theme::Automatic));
    let dark = page.place_of(Control::Theme(Theme::Dark));
    assert_eq!(dark.right + 3, 531);
    let width = dark.right + 3 - (automatic.left - 3);
    assert!((180..=184).contains(&width), "{width}");
    let row = layout::place_row(Row::Theme, &[]);
    assert!(automatic.top > i32::try_from(row.span.top).expect("small"));
    assert!(automatic.bottom < i32::try_from(row.span.bottom).expect("small"));
}

/// Whether a pixel is amber: far redder than blue, which no grey, ink or cyan of the page is.
fn amber(pixel: [u8; 3]) -> bool {
    i32::from(pixel[0]) - i32::from(pixel[2]) > 50
}

#[test]
fn the_shortcut_s_field_is_left_of_reset_at_the_right_of_general_s_first_row() {
    // The prototype's `.rec` is 156 wide at least and 32 tall, `.pbtn` 30 tall, 10 apart, ending at 531.
    for mode in [Mode::Dark, Mode::Light] {
        let page = drawn(mode, 96);
        let field = page.place_of(Control::ShortcutField);
        let reset = page.place_of(Control::ShortcutReset);
        assert_eq!(reset.right, 531, "{mode:?}");
        assert_eq!(height_of_rect(reset), 30, "{mode:?}");
        assert_eq!(field.right, reset.left - 10, "{mode:?}");
        assert_eq!(height_of_rect(field), 32, "{mode:?}");
        assert!(width_of_rect(field) >= 156, "{mode:?}: {field:?}");
        // Reset's word is a few dozen pixels: "Reset" at 600 12px and 11 either side.
        assert!((50..=62).contains(&width_of_rect(reset)), "{reset:?}");
        let row = layout::place_row(Row::GlobalShortcut, &[]);
        let above = field.top - i32::try_from(row.span.top).expect("small");
        let below = i32::try_from(row.span.bottom).expect("small") - field.bottom;
        assert!((above - below).abs() <= 1, "{above} above, {below} below");
    }
}

#[test]
fn the_field_shows_the_shortcut_s_keys_in_relief_inside_its_well() {
    let dpi = 144;
    for mode in [Mode::Dark, Mode::Light] {
        let page = drawn(mode, dpi);
        let field = page.place_of(Control::ShortcutField);
        let middle = (field.top + field.bottom) / 2;
        // Win + Shift + X: three keys lighter or darker than the well, and the ink of their words on them.
        let well = page.at(field.right - whole_px(6, dpi), middle);
        let changes = (field.left..field.right)
            .filter(|&x| page.at(x, middle) != page.at(x + 1, middle))
            .count();
        assert!(changes > 12, "{mode:?}: {changes} changes across the field");
        let ink = relief::palette::palette(mode).ink;
        let inked = (field.top..field.bottom)
            .flat_map(|y| (field.left..field.right).map(move |x| (x, y)))
            .filter(|&(x, y)| near(page.at(x, y), ink, 30))
            .count();
        assert!(inked > 30, "{mode:?}: {inked} pixels of the keys' ink");
        assert_ne!(
            well,
            page.at(field.left + whole_px(20, dpi), middle),
            "{mode:?}"
        );
    }
}

#[test]
fn recording_the_field_has_a_cyan_border_and_ring_and_resting_none() {
    let dpi = 144;
    for mode in [Mode::Dark, Mode::Light] {
        let options = Options::default();
        let resting_page = drawn(mode, dpi);
        let mut recording = resting(&options);
        recording.recorder.recording = true;
        let recording_page = drawn_showing(mode, dpi, &options, Pointer::default(), &recording);
        let field = recording_page.place_of(Control::ShortcutField);
        let middle = (field.top + field.bottom) / 2;
        let bluer = |page: &Picture, x: i32| {
            let [r, _, b] = page.at(x, middle);
            i32::from(b) - i32::from(r)
        };
        assert!(bluer(&recording_page, field.left) > 60, "{mode:?} border");
        assert!(
            bluer(&resting_page, field.left).abs() < 25,
            "{mode:?} resting"
        );
        // The ring, a few pixels out.
        assert!(
            bluer(&recording_page, field.left - 2) > bluer(&resting_page, field.left - 2) + 10,
            "{mode:?} ring"
        );
    }
}

#[test]
fn recording_with_nothing_held_the_field_asks_for_the_keys_in_the_dim_color() {
    let dpi = 144;
    for mode in [Mode::Dark, Mode::Light] {
        let options = Options::default();
        let mut shown = resting(&options);
        shown.recorder.recording = true;
        let page = drawn_showing(mode, dpi, &options, Pointer::default(), &shown);
        let field = page.place_of(Control::ShortcutField);
        let dim = (field.top..field.bottom)
            .flat_map(|y| (field.left..field.right).map(move |x| (x, y)))
            .filter(|&(x, y)| near(page.at(x, y), palette(mode).dim, 30))
            .count();
        assert!(dim > 60, "{mode:?}: {dim} pixels of \"Press the keys…\"");
        // No key in relief: the keys' ink is nowhere in the field.
        let ink = relief::palette::palette(mode).ink;
        let inked = (field.top..field.bottom)
            .flat_map(|y| (field.left..field.right).map(move |x| (x, y)))
            .filter(|&(x, y)| near(page.at(x, y), ink, 8))
            .count();
        assert!(inked < 10, "{mode:?}: {inked}");
    }
}

#[test]
fn a_message_shows_in_amber_under_the_label_inside_its_grown_row_at_every_scale() {
    let options = Options::default();
    let shown = showing_message(&options, Text::Fixed(CTRL_ALT), false, false);
    let grown = shown.grown[0];
    assert!(grown.by > 20, "three lines or so: {grown:?}");
    for mode in [Mode::Dark, Mode::Light] {
        for dpi in [96, 120, 144, 168] {
            let page = drawn_showing(mode, dpi, &options, Pointer::default(), &shown);
            let row = layout::place_row(Row::GlobalShortcut, &shown.grown);
            let (top, bottom) = (whole_px(row.span.top, dpi), whole_px(row.span.bottom, dpi));
            let field = page.place_of(Control::ShortcutField);
            let left = whole_px(layout::SIDE + layout::CARD_BORDER, dpi);
            let amber_rows: Vec<i32> = (top - whole_px(4, dpi)..bottom + whole_px(8, dpi))
                .filter(|&y| (left..field.left).any(|x| amber(page.at(x, y))))
                .collect();
            let (Some(&first), Some(&last)) = (amber_rows.first(), amber_rows.last()) else {
                panic!("{mode:?} {dpi}: no amber in the row");
            };
            // Below the label, and inside the row with room to spare below the last line.
            assert!(
                first > top + whole_px(15, dpi),
                "{mode:?} {dpi}: {first} from {top}"
            );
            assert!(
                last < bottom - whole_px(3, dpi),
                "{mode:?} {dpi}: {last} to {bottom}"
            );
            // Its lines never reach the field.
            assert!(
                (top..bottom)
                    .all(|y| (field.left - whole_px(6, dpi)..field.left)
                        .all(|x| !amber(page.at(x, y)))),
                "{mode:?} {dpi}: amber against the field"
            );
            // The rows below moved down with it: the theme's choice is as far below the row as ever.
            let theme = page.place_of(Control::Theme(Theme::Automatic));
            let theme_row = layout::place_row(Row::Theme, &shown.grown);
            assert!(
                theme.top > whole_px(theme_row.span.top, dpi),
                "{mode:?} {dpi}"
            );
            assert!(
                theme.bottom < whole_px(theme_row.span.bottom, dpi),
                "{mode:?} {dpi}"
            );
        }
    }
}

#[test]
fn the_snip_s_card_and_the_footer_move_down_with_a_message_in_general() {
    let options = Options::default();
    let shown = showing_message(&options, Text::Fixed(CTRL_ALT), false, false);
    let by = shown.grown[0].by;
    for dpi in [96, 144] {
        let still = drawn(Mode::Dark, dpi);
        let grown = drawn_showing(Mode::Dark, dpi, &options, Pointer::default(), &shown);
        let moved = whole_px(by, dpi);
        for control in [
            Control::SnipDestination(screengauge_core::settings::SnipDestination::File),
            Control::SaveToDefaultFolder,
            Control::ChangeFolder,
        ] {
            let (was, is) = (still.place_of(control), grown.place_of(control));
            assert_eq!(is.left, was.left, "{dpi} {control:?}");
            // Within a pixel of the growth: each part's top is rounded on its own at the scale.
            assert!(
                (is.top - was.top - moved).abs() <= 1,
                "{dpi} {control:?}: {was:?} -> {is:?}, by {moved}"
            );
        }
        assert!(
            (grown.link(Link::ProjectPage).top - still.link(Link::ProjectPage).top - moved).abs()
                <= 1,
            "{dpi}: the footer's link {:?} -> {:?}",
            still.link(Link::ProjectPage),
            grown.link(Link::ProjectPage)
        );
        // The Snip's card is drawn where it moved: its top border is a card's color, not the background's.
        let card = snip_card::fitted(layout::page(&shown.grown), snip_card::note_shown(&options))
            .blocks[3]
            .card;
        let y = whole_px(card.top + 4, dpi);
        let x = whole_px(layout::SIDE + 30, dpi);
        assert!(
            brightness(grown.at(x, y)) > brightness(grown.at(2, y)),
            "{dpi}"
        );
    }
}

#[test]
fn a_short_message_takes_one_line_and_grows_the_row_by_a_few_pixels() {
    let options = Options::default();
    let shown = showing_message(
        &options,
        Text::Fixed("Already used by another program"),
        false,
        false,
    );
    let fonts = Fonts::new().expect("the fonts");
    let message = shown.messages[0];
    assert_eq!(message.lines(&fonts).expect("lines"), 1);
    // 5 + 16.9 + 2 + 14.95 + 5 = 43.85: 44, against the row's 42.
    assert_eq!(shown.grown[0].by, 2);
}

#[test]
fn with_no_message_the_page_has_no_amber() {
    for mode in [Mode::Dark, Mode::Light] {
        let page = drawn(mode, 144);
        let amber_pixels = page.pixels.iter().filter(|&&pixel| amber(pixel)).count();
        assert_eq!(amber_pixels, 0, "{mode:?}");
    }
}

#[test]
fn the_band_tints_the_whole_row_with_a_bar_of_amber_on_its_left() {
    let dpi = 144;
    let options = Options::default();
    for mode in [Mode::Dark, Mode::Light] {
        let plain = drawn_showing(
            mode,
            dpi,
            &options,
            Pointer::default(),
            &showing_message(
                &options,
                Text::Fixed("Already used by another program"),
                false,
                false,
            ),
        );
        let banded = drawn_showing(
            mode,
            dpi,
            &options,
            Pointer::default(),
            &showing_message(&options, busy(&options), true, false),
        );
        let row = layout::place_row(Row::GlobalShortcut, &banded_grown(&options));
        let middle = whole_px((row.span.top + row.span.bottom) / 2, dpi);
        let inner_left = whole_px(layout::SIDE, dpi) + whole_px(1, dpi).max(1);
        // The bar: 3 logical pixels of --warn against the card's border.
        assert!(
            near(banded.at(inner_left + 1, middle), palette(mode).warn, 12),
            "{mode:?}: {:?}",
            banded.at(inner_left + 1, middle)
        );
        assert!(!amber(plain.at(inner_left + 1, middle)), "{mode:?}");
        // The tint, where nothing else is drawn: just inside the row's top, halfway across.
        let y = whole_px(row.span.top, dpi) + 2;
        let x = whole_px(250, dpi);
        let ([r, _, b], [pr, _, pb]) = (banded.at(x, y), plain.at(x, y));
        assert!(
            i32::from(r) - i32::from(b) > i32::from(pr) - i32::from(pb) + 8,
            "{mode:?}: {:?} against {:?}",
            banded.at(x, y),
            plain.at(x, y)
        );
    }
}

#[test]
fn start_with_windows_is_a_switch_at_the_right_of_general_s_middle_row_off_by_default() {
    for mode in [Mode::Dark, Mode::Light] {
        let options = Options::default();
        let off = drawn(mode, 96);
        let on = drawn_showing(
            mode,
            96,
            &options,
            Pointer::default(),
            &Shown {
                start_with_windows: true,
                ..resting(&options)
            },
        );
        let track = off.place_of(Control::StartWithWindows);
        // The prototype's `.tgl`: 42 by 24, ending at 531 as every control, centered in its row.
        assert_eq!(
            (track.right, width_of_rect(track), height_of_rect(track)),
            (531, 42, 24)
        );
        let row = layout::place_row(Row::StartWithWindows, &[]);
        let above = track.top - i32::try_from(row.span.top).expect("small");
        let below = i32::try_from(row.span.bottom).expect("small") - track.bottom;
        assert!((above - below).abs() <= 1, "{above} above, {below} below");
        assert!(
            cyan(&off, track).abs() < 25.0,
            "{mode:?}: {}",
            cyan(&off, track)
        );
        assert!(cyan(&on, track) > 60.0, "{mode:?}: {}", cyan(&on, track));
    }
}

#[test]
fn turned_off_in_windows_the_amber_message_shows_under_its_label_with_the_switch_off() {
    let options = Options::default();
    let shown = showing_turned_off(resting(&options));
    for mode in [Mode::Dark, Mode::Light] {
        for dpi in [96, 120, 144, 168] {
            let page = drawn_showing(mode, dpi, &options, Pointer::default(), &shown);
            let row = layout::place_row(Row::StartWithWindows, &shown.grown);
            let (top, bottom) = (whole_px(row.span.top, dpi), whole_px(row.span.bottom, dpi));
            let track = page.place_of(Control::StartWithWindows);
            let left = whole_px(layout::SIDE + layout::CARD_BORDER, dpi);
            let amber_rows: Vec<i32> = (top..bottom)
                .filter(|&y| (left..track.left).any(|x| amber(page.at(x, y))))
                .collect();
            let (Some(&first), Some(&last)) = (amber_rows.first(), amber_rows.last()) else {
                panic!("{mode:?} {dpi}: no amber in the row");
            };
            assert!(
                first > top + whole_px(15, dpi),
                "{mode:?} {dpi}: {first} from {top}"
            );
            assert!(
                last < bottom - whole_px(3, dpi),
                "{mode:?} {dpi}: {last} to {bottom}"
            );
            // No band: the row's left edge stays the card's, and the switch stays off.
            let middle = (top + bottom) / 2;
            assert!(!amber(page.at(left + 1, middle)), "{mode:?} {dpi}: a band");
            assert!(cyan(&page, track).abs() < 25.0, "{mode:?} {dpi}");
            // Never against the switch.
            assert!(
                (top..bottom)
                    .all(|y| (track.left - whole_px(6, dpi)..track.left)
                        .all(|x| !amber(page.at(x, y)))),
                "{mode:?} {dpi}: amber against the switch"
            );
        }
    }
}

#[test]
fn the_busy_shortcut_s_band_and_start_with_windows_message_show_together_each_in_its_row() {
    let options = Options::default();
    let shown = showing_turned_off(showing_message(&options, busy(&options), true, false));
    assert_eq!(shown.grown.len(), 2);
    let dpi = 144;
    for mode in [Mode::Dark, Mode::Light] {
        let page = drawn_showing(mode, dpi, &options, Pointer::default(), &shown);
        let left = whole_px(layout::SIDE + layout::CARD_BORDER, dpi);
        for row in [Row::GlobalShortcut, Row::StartWithWindows] {
            let placed = layout::place_row(row, &shown.grown);
            let (top, bottom) = (
                whole_px(placed.span.top, dpi),
                whole_px(placed.span.bottom, dpi),
            );
            let inked = (top + whole_px(15, dpi)..bottom).any(|y| {
                (left + whole_px(12, dpi)..whole_px(300, dpi)).any(|x| amber(page.at(x, y)))
            });
            assert!(inked, "{mode:?} {row:?}: no amber message");
        }
        // The rows below moved down by both: Theme's choice is still inside its row.
        let theme = page.place_of(Control::Theme(Theme::Automatic));
        let theme_row = layout::place_row(Row::Theme, &shown.grown);
        assert!(theme.top > whole_px(theme_row.span.top, dpi), "{mode:?}");
        assert!(
            theme.bottom < whole_px(theme_row.span.bottom, dpi),
            "{mode:?}"
        );
    }
}

fn banded_grown(options: &Options) -> Vec<layout::Grown> {
    showing_message(options, busy(options), true, false).grown
}
