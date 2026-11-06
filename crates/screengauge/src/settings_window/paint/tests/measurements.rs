//! The Measurements block drawn on a bitmap: the default tolerance's − and + around its number, and the switch of
//! "Also show logical pixels".

use super::*;

#[test]
fn the_switch_s_track_is_cyan_when_on_and_sunken_grey_when_off() {
    for mode in [Mode::Dark, Mode::Light] {
        let on = drawn(mode, 144);
        let off = drawn_with(
            mode,
            144,
            &Options {
                also_show_logical_pixels: false,
                ..Options::default()
            },
            Pointer::default(),
        );
        let track = on.place_of(Control::LogicalPixels);
        assert!(cyan(&on, track) > 60.0, "{mode:?}: {}", cyan(&on, track));
        assert!(
            cyan(&off, track).abs() < 25.0,
            "{mode:?}: {}",
            cyan(&off, track)
        );
    }
}

#[test]
fn the_switch_s_knob_sits_right_when_on_and_left_when_off() {
    let dpi = 144;
    for mode in [Mode::Dark, Mode::Light] {
        let on = drawn(mode, dpi);
        let off = drawn_with(
            mode,
            dpi,
            &Options {
                also_show_logical_pixels: false,
                ..Options::default()
            },
            Pointer::default(),
        );
        let track = on.place_of(Control::LogicalPixels);
        let middle = (track.top + track.bottom) / 2;
        // The knob's middle, 3 + 9 logical pixels in from the side it sits on.
        let near_left = track.left + whole_px(12, dpi);
        let near_right = track.right - whole_px(12, dpi);
        // On, the knob is white over the cyan; off, it is the raised button over the sunken well.
        assert!(
            brightness(on.at(near_right, middle)) > brightness(on.at(near_left, middle)),
            "{mode:?} on"
        );
        assert_ne!(
            off.at(near_left, middle),
            off.at(near_right, middle),
            "{mode:?} off"
        );
        assert!(
            cyan(
                &off,
                RECT {
                    left: near_left - 4,
                    right: near_left + 4,
                    top: middle - 4,
                    bottom: middle + 4
                }
            )
            .abs()
                < 25.0
        );
    }
}

#[test]
fn the_number_between_minus_and_plus_is_the_default_tolerance() {
    let dpi = 144;
    let mut previous: Option<Vec<[u8; 3]>> = None;
    for value in [2, 13] {
        let options = Options {
            default_tolerance: screengauge_core::settings::Tolerance::new(value)
                .expect("a tolerance"),
            ..Options::default()
        };
        let page = drawn_with(Mode::Dark, dpi, &options, Pointer::default());
        let minus = page.place_of(Control::ToleranceDown);
        let plus = page.place_of(Control::ToleranceUp);
        // The number's box, between the two buttons: text in the light ink shows there.
        let number = RECT {
            left: minus.right,
            right: plus.left,
            ..minus
        };
        let inked = (number.top..number.bottom)
            .flat_map(|y| (number.left..number.right).map(move |x| (x, y)))
            .filter(|&(x, y)| near(page.at(x, y), controls::DARK.ink, 40))
            .count();
        assert!(inked > 20, "{value}: {inked} pixels of ink");
        let pixels: Vec<[u8; 3]> = (number.top..number.bottom)
            .flat_map(|y| (number.left..number.right).map(move |x| (x, y)))
            .map(|(x, y)| page.at(x, y))
            .collect();
        if let Some(previous) = previous {
            assert_ne!(previous, pixels, "2 and 13 look the same");
        }
        previous = Some(pixels);
    }
}

#[test]
fn the_three_digits_of_the_highest_tolerance_fit_inside_the_numbers_box() {
    // 100, the highest tolerance since WPC-332, is the widest number the box shows: its ink stays clear of the
    // gaps beside the box, at each of the owner's scales.
    for dpi in [96, 120, 168] {
        let options = Options {
            default_tolerance: screengauge_core::settings::Tolerance::MAX,
            ..Options::default()
        };
        let page = drawn_with(Mode::Dark, dpi, &options, Pointer::default());
        let minus = page.place_of(Control::ToleranceDown);
        let plus = page.place_of(Control::ToleranceUp);
        let inked: Vec<i32> = (minus.top..minus.bottom)
            .flat_map(|y| (minus.right..plus.left).map(move |x| (x, y)))
            .filter(|&(x, y)| near(page.at(x, y), controls::DARK.ink, 40))
            .map(|(x, _)| x)
            .collect();
        let (first, last) = (
            inked.iter().min().expect("ink"),
            inked.iter().max().expect("ink"),
        );
        let gap = i32::try_from(scaled(controls::STEP_GAP, dpi)).expect("a small gap");
        assert!(
            *first > minus.right + gap && *last < plus.left - gap - 1,
            "at {dpi} DPI the ink runs from {first} to {last}, the box from {} to {}",
            minus.right + gap,
            plus.left - gap - 1
        );
    }
}

#[test]
fn plus_brightens_with_the_cursor_on_it_and_shrinks_while_held() {
    let dpi = 144;
    let options = Options::default();
    let rest = drawn(Mode::Dark, dpi);
    let over = drawn_with(
        Mode::Dark,
        dpi,
        &options,
        Pointer {
            over: Some(Control::ToleranceUp),
            held: None,
        },
    );
    let held = drawn_with(
        Mode::Dark,
        dpi,
        &options,
        Pointer {
            over: Some(Control::ToleranceUp),
            held: Some(Control::ToleranceUp),
        },
    );
    let plus = rest.place_of(Control::ToleranceUp);
    let minus = rest.place_of(Control::ToleranceDown);
    let brightest = |page: &Picture, place: RECT| {
        (place.top..place.bottom)
            .flat_map(|y| (place.left..place.right).map(move |x| (x, y)))
            .map(|(x, y)| brightness(page.at(x, y)))
            .max()
            .unwrap_or(0)
    };
    // The sign goes white (#fff) from its grey (#aeb3bd); − stays as it was.
    assert!(brightest(&over, plus) > brightest(&rest, plus), "hover");
    assert_eq!(brightest(&over, minus), brightest(&rest, minus));
    // Held, the button is 0.92 of its size: its corner pixel shows the card behind it.
    let corner = (plus.left + 1, (plus.top + plus.bottom) / 2);
    assert_ne!(
        held.at(corner.0, corner.1),
        over.at(corner.0, corner.1),
        "held"
    );
}

#[test]
fn measurements_rows_have_a_line_between_them_and_inspect_s_none() {
    let dpi = 144;
    for mode in [Mode::Dark, Mode::Light] {
        let page = drawn(mode, dpi);
        let row = layout::place_row(Row::AlsoShowLogicalPixels, &[]);
        let line = whole_px(row.span.top, dpi) - 1;
        let x = whole_px(200, dpi);
        assert_ne!(page.at(x, line), page.at(x, line - 2), "{mode:?}");
        let color = layout::place_row(Row::ColorFormat, &[]);
        let middle = whole_px((color.span.top + color.span.bottom) / 2, dpi);
        assert!(
            // Only the background's gradient changes from one pixel row to the next, by a level at most.
            (whole_px(color.span.top, dpi) + 2..middle - 8).all(|y| near(
                page.at(x, y),
                Color {
                    rgb: page.at(x, y + 1),
                    alpha: 1.0
                },
                1
            )),
            "{mode:?}: a line in Inspect's card"
        );
    }
}
