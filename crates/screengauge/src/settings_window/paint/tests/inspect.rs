//! The Inspect block drawn on a bitmap: the color format's choice.

use super::*;

#[test]
fn only_the_chosen_format_is_cyan() {
    for mode in [Mode::Dark, Mode::Light] {
        for (format, _) in COLOR_FORMATS {
            let page = drawn_with(
                mode,
                144,
                &Options {
                    color_format: format,
                    ..Options::default()
                },
                Pointer::default(),
            );
            for (other, _) in COLOR_FORMATS {
                let lit = cyan(&page, page.place_of(Control::ColorFormat(other)));
                if other == format {
                    assert!(lit > 50.0, "{mode:?} {format:?}: {lit}");
                } else {
                    assert!(lit.abs() < 25.0, "{mode:?} {format:?} {other:?}: {lit}");
                }
            }
        }
    }
}
