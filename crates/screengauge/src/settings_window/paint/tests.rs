//! Settings drawn on a bitmap: what the pixel tests of each block (`general`, `measurements`, `inspect`, `snip`) and
//! of the footer (`footer`) share, and the page's own: where the controls are, the labels, the background and the
//! cards.

use screengauge_core::settings::{ColorFormat, Theme};

use super::*;
use crate::graphics::Color;
use crate::settings_window::controls::{COLOR_FORMATS, THEMES};
use crate::settings_window::row_message::Text;
use crate::settings_window::start_with_windows_row;
use crate::test_bitmap;

mod footer;
mod general;
mod inspect;
mod measurements;
mod snip;

/// The page as drawn, where the links went and where the controls' parts are.
struct Picture {
    image: test_bitmap::Picture,
    links: Vec<(Link, RECT)>,
    controls: Vec<(Control, RECT)>,
}

impl std::ops::Deref for Picture {
    type Target = test_bitmap::Picture;

    fn deref(&self) -> &test_bitmap::Picture {
        &self.image
    }
}

impl Picture {
    /// Where `link` is.
    fn link(&self, link: Link) -> RECT {
        self.links
            .iter()
            .find(|(drawn, _)| *drawn == link)
            .map(|&(_, place)| place)
            .unwrap_or_else(|| panic!("{link:?} isn't drawn"))
    }

    /// Where `control` is.
    fn place_of(&self, control: Control) -> RECT {
        self.controls
            .iter()
            .find(|(drawn, _)| *drawn == control)
            .map(|&(_, place)| place)
            .unwrap_or_else(|| panic!("{control:?} isn't drawn"))
    }
}

/// Draws the page in software on a bitmap of its size at `dpi` (`test_bitmap`).
fn drawn_showing(
    mode: Mode,
    dpi: u32,
    options: &Options,
    pointer: Pointer,
    shown: &Shown,
) -> Picture {
    let width = scaled(layout::WIDTH, dpi);
    let page = layout::with_status(layout::page(&shown.grown), shown.update.is_some());
    let height = scaled(page.height, dpi);
    let mut drawn = None;
    let image = test_bitmap::drawn(width, height, |target, factory| {
        let fonts = Fonts::new().expect("the fonts");
        let stroke = crate::sign::round_stroke(factory).expect("the signs' stroke");
        let surface = Surface {
            target,
            factory,
            fonts: &fonts,
            stroke: &stroke,
            mode,
            dpi,
            width,
        };
        drawn = Some(draw(&surface, options, pointer, shown).expect("the page is drawn"));
    });
    let drawn = drawn.expect("the page is drawn");
    Picture {
        image,
        links: drawn.links,
        controls: drawn.controls,
    }
}

/// The page with the default options and the mouse away.
fn drawn(mode: Mode, dpi: u32) -> Picture {
    drawn_with(mode, dpi, &Options::default(), Pointer::default())
}

/// What Settings shows with the field resting and no message.
fn resting(options: &Options) -> Shown {
    Shown {
        recorder: View {
            shortcut: options.global_shortcut,
            recording: false,
            held: screengauge_core::settings::Modifiers::default(),
            opacity: 1.0,
        },
        start_with_windows: false,
        messages: Vec::new(),
        grown: Vec::new(),
        update: None,
    }
}

fn drawn_with(mode: Mode, dpi: u32, options: &Options, pointer: Pointer) -> Picture {
    drawn_showing(mode, dpi, options, pointer, &resting(options))
}

fn near(pixel: [u8; 3], color: Color, within: u8) -> bool {
    pixel
        .iter()
        .zip(color.rgb)
        .all(|(&have, want)| have.abs_diff(want) <= within)
}

fn brightness(pixel: [u8; 3]) -> i32 {
    pixel.iter().map(|&channel| i32::from(channel)).sum()
}

fn whole_px(logical: u32, dpi: u32) -> i32 {
    i32::try_from(scaled(logical, dpi)).expect("a small page")
}

/// The owner's monitors' scales, and Full HD at 150%.
const DPIS: [u32; 3] = [120, 144, 168];

/// The average color of the pixels inside `place`, `inset` pixels in from each side.
fn average(page: &Picture, place: RECT, inset: i32) -> [f64; 3] {
    let mut sum = [0.0; 3];
    let mut count = 0.0;
    for y in place.top + inset..place.bottom - inset {
        for x in place.left + inset..place.right - inset {
            for (total, channel) in sum.iter_mut().zip(page.at(x, y)) {
                *total += f64::from(channel);
            }
            count += 1.0;
        }
    }
    sum.map(|total| total / count)
}

/// How much bluer than red a box is on average: the cyan of a lit control is far bluer, the greys far less.
fn cyan(page: &Picture, place: RECT) -> f64 {
    let [r, _, b] = average(page, place, 3);
    b - r
}

fn width_of_rect(place: RECT) -> i32 {
    place.right - place.left
}

fn height_of_rect(place: RECT) -> i32 {
    place.bottom - place.top
}

#[test]
fn the_controls_are_where_the_prototype_puts_them_right_of_their_rows() {
    // Measured in the prototype at 100% for WPC-320, from the window's left: every control ends at 531, the card's
    // right side less its border and the row's padding; the switch is 42 × 24, the buttons 30 × 30, the number's box
    // 40 wide, and the choice 32 tall with its segments 26.
    for mode in [Mode::Dark, Mode::Light] {
        let page = drawn(mode, 96);
        let switch = page.place_of(Control::LogicalPixels);
        assert_eq!(
            (switch.right, width_of_rect(switch), height_of_rect(switch)),
            (531, 42, 24)
        );
        let plus = page.place_of(Control::ToleranceUp);
        let minus = page.place_of(Control::ToleranceDown);
        assert_eq!(
            (plus.right, width_of_rect(plus), height_of_rect(plus)),
            (531, 30, 30)
        );
        // − / + is 112 wide: two buttons, the number's 40 and two gaps of 6.
        assert_eq!(plus.right - minus.left, 112);
        let both = page.place_of(Control::ColorFormat(ColorFormat::Both));
        assert_eq!((both.right, height_of_rect(both)), (528, 26));
        let hex = page.place_of(Control::ColorFormat(ColorFormat::Hex));
        // The prototype's choice is 143.29 wide: 3 of padding either side and between, and each word with 10
        // either side.
        let choice = both.right + 3 - (hex.left - 3);
        assert!((142..=146).contains(&choice), "{choice}");
        // Each control is centered in its row.
        for control in [
            Control::LogicalPixels,
            Control::ToleranceUp,
            Control::ColorFormat(ColorFormat::Hex),
        ] {
            let place = page.place_of(control);
            let row = match control {
                Control::LogicalPixels => layout::place_row(Row::AlsoShowLogicalPixels, &[]),
                Control::ColorFormat(_) => layout::place_row(Row::ColorFormat, &[]),
                _ => layout::place_row(Row::DefaultTolerance, &[]),
            };
            let above = place.top - i32::try_from(row.span.top).expect("small");
            let below = i32::try_from(row.span.bottom).expect("small") - place.bottom;
            assert!(
                (above - below).abs() <= 1,
                "{control:?}: {above} above, {below} below"
            );
        }
    }
}

#[test]
fn the_controls_grow_with_the_scale() {
    for dpi in DPIS {
        let page = drawn(Mode::Dark, dpi);
        let switch = page.place_of(Control::LogicalPixels);
        assert_eq!(width_of_rect(switch), whole_px(42, dpi), "{dpi}");
        assert_eq!(height_of_rect(switch), whole_px(24, dpi), "{dpi}");
        let plus = page.place_of(Control::ToleranceUp);
        assert_eq!(width_of_rect(plus), whole_px(30, dpi), "{dpi}");
        assert_eq!(
            plus.right,
            i32::try_from(page.width).expect("small") - whole_px(29, dpi),
            "{dpi}"
        );
    }
}

#[test]
fn each_row_s_label_is_in_the_ink_and_its_note_in_the_dim_color() {
    let dpi = 144;
    for mode in [Mode::Dark, Mode::Light] {
        let page = drawn(mode, dpi);
        let row = layout::place_row(Row::DefaultTolerance, &[]);
        let (top, bottom) = (whole_px(row.span.top, dpi), whole_px(row.span.bottom, dpi));
        let middle = (top + bottom) / 2;
        let left = whole_px(29, dpi);
        let count = |from: i32, to: i32, color: Color| {
            (from..to)
                .flat_map(|y| (left..left + whole_px(150, dpi)).map(move |x| (x, y)))
                .filter(|&(x, y)| near(page.at(x, y), color, 30))
                .count()
        };
        assert!(count(top, middle, palette(mode).ink) > 50, "{mode:?} label");
        assert!(
            count(middle, bottom, palette(mode).dim) > 50,
            "{mode:?} note"
        );
    }
}

/// What Settings shows with `text` under the global shortcut's label, the field recording or not, and the band
/// should `band` say so: the message's room and the row's growth worked out as the window works them out.
fn showing_message(options: &Options, text: Text, band: bool, recording: bool) -> Shown {
    let fonts = Fonts::new().expect("the fonts");
    let mut shown = resting(options);
    shown.recorder.recording = recording;
    let width = recorder_field::message_room(shown.recorder.shortcut, &measurer(&fonts, 1.0))
        .expect("the message's room");
    let message = Message {
        row: Row::GlobalShortcut,
        text,
        band,
        width,
    };
    shown.grown = vec![message.growth(&fonts).expect("the row's growth")];
    shown.messages = vec![message];
    shown
}

const CTRL_ALT: &str = "Ctrl + Alt without Win isn't allowed: on Brazilian keyboards it's AltGr and would take \
                        characters like / and ?.";

/// The busy shortcut's message, for the global shortcut `options` hold (WPC-324).
fn busy(options: &Options) -> Text {
    Text::ShortcutBusy(options.global_shortcut)
}

/// `shown` with "Start with Windows" turned off in Windows and its message showing, as after a click on its switch.
fn showing_turned_off(mut shown: Shown) -> Shown {
    let fonts = Fonts::new().expect("the fonts");
    let message =
        start_with_windows_row::message_saying(start_with_windows_row::Asked::TurnedOffInWindows);
    shown.start_with_windows = false;
    shown
        .grown
        .push(message.growth(&fonts).expect("the row's growth"));
    shown.messages.push(message);
    shown
}

/// Writes the page in both modes and at the owner's scales to target\settings-pages, for a look at it.
#[test]
#[ignore = "writes PNG files for a person to look at"]
fn write_the_pages_for_a_look() {
    let folder =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/settings-pages");
    std::fs::create_dir_all(&folder).expect("the folder");
    for mode in [Mode::Dark, Mode::Light] {
        for dpi in [96, 120, 144, 168] {
            let page = drawn(mode, dpi);
            page.image
                .write_png(&folder.join(format!("{mode:?}-{dpi}.png")));
        }
        let options = Options::default();
        for (name, shown) in [
            ("recording", {
                let mut shown = resting(&options);
                shown.recorder.recording = true;
                shown
            }),
            (
                "ctrl-alt",
                showing_message(&options, Text::Fixed(CTRL_ALT), false, true),
            ),
            (
                "busy-band",
                showing_message(&options, busy(&options), true, false),
            ),
            ("start-on", {
                let mut shown = resting(&options);
                shown.start_with_windows = true;
                shown
            }),
            ("start-turned-off", showing_turned_off(resting(&options))),
            (
                "busy-and-start-turned-off",
                showing_turned_off(showing_message(&options, busy(&options), true, false)),
            ),
        ] {
            let page = drawn_showing(mode, 144, &options, Pointer::default(), &shown);
            page.image
                .write_png(&folder.join(format!("{mode:?}-144-{name}.png")));
        }
    }
}

#[test]
fn the_background_runs_from_the_top_color_to_the_bottom_one() {
    for mode in [Mode::Dark, Mode::Light] {
        for dpi in DPIS {
            let page = drawn(mode, dpi);
            let bottom = whole_px(layout::page(&[]).height, dpi) - 1;
            assert!(near(page.at(2, 0), palette(mode).top, 1), "{mode:?} {dpi}");
            assert!(
                near(page.at(2, bottom), palette(mode).bottom, 1),
                "{mode:?} {dpi}"
            );
        }
    }
}

#[test]
fn the_cards_are_lighter_than_the_background_and_the_footer_has_its_line() {
    let dpi = 144;
    for mode in [Mode::Dark, Mode::Light] {
        let page = drawn(mode, dpi);
        for placed in layout::page(&[]).blocks {
            // Inside the card, past its border and corner and above its rows' labels, and beside it on the same row.
            let y = whole_px(placed.card.top + 4, dpi);
            let inside = page.at(whole_px(layout::SIDE + 30, dpi), y);
            let beside = page.at(2, y);
            assert!(
                brightness(inside) > brightness(beside),
                "{mode:?} {:?}: {inside:?} beside {beside:?}",
                placed.block
            );
        }
        // The footer's 1-pixel line differs from the background just above it.
        let line = whole_px(layout::page(&[]).footer.top, dpi);
        assert_ne!(page.at(300, line), page.at(300, line - 1), "{mode:?}");
    }
}
