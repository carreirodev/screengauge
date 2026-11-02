//! Settings' controls, drawn by ScreenGauge in the toolbar's style (v1 spec WPC-296, Settings; WPC-320): the switch
//! (`.tgl`), whose track is cyan when it is on; the choice of three (`.segc`), segments in a well with the chosen one
//! cyan; and − / + (`.step`), the number in a small box between two small buttons. The sizes are the prototype's
//! stylesheet, measured in a browser for WPC-320; the surfaces are the toolbar's (`relief::palette`).
//!
//! The − and + react to the mouse as the toolbar's small buttons do (`.sbtn:hover` and `:active`): the sign brightens
//! with the cursor on it, and the button shrinks while held down on it. The switch and the segments don't react. The
//! switch's knob moves at once, with no 0.15 s slide, as the owner chose for the toolbar in WPC-311.

use screengauge_core::settings::{Change, ColorFormat, Options, SnipDestination, Theme};
use windows::Win32::Foundation::RECT;

use super::snip_card;
use crate::graphics::{Color, color};
use crate::relief::palette::{self as relief_palette, Shadow, Stop, Surface, rgb, shadow, stop};
use crate::relief::{Canvas, Motion};
use crate::theme::Mode;

// Every size below is in logical pixels, which the canvas's `scaled` turns into physical pixels for the window's scale.

/// The switch's track (`.tgl`) and its knob (`::after`): the knob's side, and how far it sits in from the track.
pub const SWITCH_WIDTH: u32 = 42;
pub const SWITCH_HEIGHT: u32 = 24;
const KNOB: u32 = 18;
const KNOB_INSET: u32 = 3;

/// The choice's well (`.segc`): the room around and between its segments, and its corners; a segment's room left and
/// right of its word (`padding: 5px 10px`), its corners, and its height, its word's line (12 × Segoe UI's normal
/// line-height of 1.33) and 5 above and below.
const WELL_PADDING: u32 = 3;
const WELL_RADIUS: u32 = 11;
pub const SEGMENT_PADDING: u32 = 10;
const SEGMENT_RADIUS: u32 = 8;
pub const SEGMENT_TEXT: f32 = 12.0;
const SEGMENT_HEIGHT: f32 = 12.0 * NORMAL_LINE + 10.0;

/// − / + (`.step`): the buttons' side and corners (`.step .sbtn`), the number's box (`.num`: 40 wide, its 14-pixel
/// number's normal line and 5 above and below, corners of 8), and the room between the three.
const STEP_BUTTON: u32 = 30;
const STEP_RADIUS: u32 = 10;
const NUMBER_WIDTH: u32 = 40;
const NUMBER_HEIGHT: f32 = 14.0 * NORMAL_LINE + 10.0;
const NUMBER_RADIUS: u32 = 8;
const STEP_GAP: u32 = 6;
/// How much a held − or + shrinks (`.sbtn:active`).
const PRESSED: f32 = 0.92;
/// The number's size (`font: 700 14px`) and the sign's (`font: 700 16px/1`).
pub const NUMBER_TEXT: f32 = 14.0;
pub const SIGN_TEXT: f32 = 16.0;

/// Segoe UI's `line-height: normal`: its ascent and descent over its em, (2210 + 514) / 2048.
const NORMAL_LINE: f32 = 1.33;

/// `--well`'s inset shadow under the choice and the number (`inset 0 1px 3px rgba(0,0,0,.3)`), and the switch's, a
/// little darker (`.35`). Black in both palettes.
const WELL_INSET: [Shadow; 1] = [shadow(1, 3, 0, color(0, 0, 0, 0.3))];
const TRACK_INSET: [Shadow; 1] = [shadow(1, 3, 0, color(0, 0, 0, 0.35))];

/// The knob of a switch that is on (`.tgl.on::after`): white, a little grey at the bottom, in both palettes.
const KNOB_ON: [Stop; 2] = [
    stop(0.0, rgb(0xff, 0xff, 0xff)),
    stop(1.0, rgb(0xe6, 0xe6, 0xe6)),
];

/// The colors of the controls that aren't the toolbar's: `--well`, the sunken box under the choice, the number and an
/// off switch; and `--panel-ink`, the number's.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Colors {
    pub well: Color,
    pub ink: Color,
}

pub const DARK: Colors = Colors {
    well: color(0, 0, 0, 0.38),
    ink: rgb(0xe9, 0xeb, 0xef),
};

pub const LIGHT: Colors = Colors {
    well: color(70, 60, 45, 0.11),
    ink: rgb(0x1d, 0x1c, 0x1a),
};

pub fn colors(mode: Mode) -> &'static Colors {
    match mode {
        Mode::Light => &LIGHT,
        Mode::Dark => &DARK,
    }
}

/// A part of a control that a click acts on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Control {
    Theme(Theme),
    ToleranceDown,
    ToleranceUp,
    LogicalPixels,
    ColorFormat(ColorFormat),
    SnipDestination(SnipDestination),
    SaveToDefaultFolder,
    ChangeFolder,
}

/// What a click on `control` changes, with `options` as Settings shows them: the switch turns to the other side. `None`
/// for a click that needs Windows' folder window first, and changes nothing until it answers: "Change…", and the
/// switch turned on with no folder kept (`Options::choose_default_folder` then keeps the folder).
pub fn change(control: Control, options: &Options) -> Option<Change> {
    Some(match control {
        Control::Theme(theme) => Change::Theme(theme),
        Control::ToleranceDown => Change::DefaultToleranceDown,
        Control::ToleranceUp => Change::DefaultToleranceUp,
        Control::LogicalPixels => Change::AlsoShowLogicalPixels(!options.also_show_logical_pixels),
        Control::ColorFormat(format) => Change::ColorFormat(format),
        Control::SnipDestination(destination) => Change::SnipDestination(destination),
        Control::SaveToDefaultFolder if options.default_folder.is_on() => {
            Change::SaveToDefaultFolder(false)
        }
        Control::SaveToDefaultFolder => match options.default_folder.folder() {
            Some(_) => Change::SaveToDefaultFolder(true),
            None => return None,
        },
        Control::ChangeFolder => return None,
    })
}

/// The choices of "Theme", in the spec's order, with their words.
pub const THEMES: [(Theme, &str); 3] = [
    (Theme::Automatic, "Automatic"),
    (Theme::Light, "Light"),
    (Theme::Dark, "Dark"),
];

/// The choices of "Color format", in the spec's order, with their words.
pub const COLOR_FORMATS: [(ColorFormat, &str); 3] = [
    (ColorFormat::Hex, "HEX"),
    (ColorFormat::Rgb, "RGB"),
    (ColorFormat::Both, "Both"),
];

/// Where the mouse is and what it holds, for the controls that react to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Pointer {
    /// What the cursor is on.
    pub over: Option<Control>,
    /// What the left button went down on and still holds.
    pub held: Option<Control>,
}

impl Pointer {
    fn hovered(self, control: Control) -> bool {
        self.over == Some(control)
    }

    /// Held down and with the cursor still on it, as the toolbar's buttons look pressed (WPC-311).
    fn pressed(self, control: Control) -> bool {
        self.held == Some(control) && self.over == Some(control)
    }
}

/// One control laid out in the window's pixels.
#[derive(Debug, Clone, PartialEq)]
pub enum Laid {
    Switch {
        control: Control,
        track: RECT,
        on: bool,
    },
    Choice {
        well: RECT,
        /// Each segment, its word, and whether it is the one chosen.
        segments: Vec<(Control, RECT, &'static str, bool)>,
    },
    Stepper {
        minus: RECT,
        number: RECT,
        plus: RECT,
        value: u8,
    },
    /// A button with a word (`.pbtn`): a small button of the toolbar's style.
    Button {
        control: Control,
        place: RECT,
        word: &'static str,
    },
    /// The box that shows a folder's path (`.fpath`): a well, whose text `paint::snip` writes.
    Path { place: RECT },
}

impl Laid {
    /// The parts a click acts on, and where they are.
    pub fn hits(&self) -> Vec<(Control, RECT)> {
        match self {
            Laid::Switch { control, track, .. } => vec![(*control, *track)],
            Laid::Choice { segments, .. } => segments
                .iter()
                .map(|&(control, place, _, _)| (control, place))
                .collect(),
            Laid::Stepper { minus, plus, .. } => {
                vec![
                    (Control::ToleranceDown, *minus),
                    (Control::ToleranceUp, *plus),
                ]
            }
            Laid::Button { control, place, .. } => vec![(*control, *place)],
            Laid::Path { .. } => Vec::new(),
        }
    }
}

/// A box `width` by `height` pixels whose right side is at `right`, centered in the row from `top` to `bottom`.
fn at_right(right: i32, top: i32, bottom: i32, width: i32, height: i32) -> RECT {
    let top = top + (bottom - top - height) / 2;
    RECT {
        left: right - width,
        top,
        right,
        bottom: top + height,
    }
}

/// `logical` pixels of a size that isn't whole, at `dpi`, rounded to whole pixels.
fn rounded(logical: f32, dpi: u32) -> i32 {
    let pixels = (logical * dpi_scale(dpi)).round();
    debug_assert!((0.0..1e6).contains(&pixels), "{pixels}");
    // A control's size is a few hundred pixels at most, which the cast keeps exactly.
    pixels as i32
}

/// How many pixels a logical pixel is at `dpi`.
pub fn dpi_scale(dpi: u32) -> f32 {
    // A DPI is a few hundred at most, which a float holds exactly, so the cast loses nothing.
    dpi as f32 / 96.0
}

/// The switch at the right of the row from `top` to `bottom`, in the window's pixels.
pub fn switch(
    canvas: &Canvas,
    control: Control,
    on: bool,
    right: i32,
    top: i32,
    bottom: i32,
) -> Laid {
    Laid::Switch {
        control,
        track: at_right(
            right,
            top,
            bottom,
            canvas.scaled(SWITCH_WIDTH),
            canvas.scaled(SWITCH_HEIGHT),
        ),
        on,
    }
}

/// The choice at the right of the row, with `choices` and their words' widths in pixels, as DirectWrite lays them out.
pub fn choice(
    canvas: &Canvas,
    choices: &[(Control, &'static str, f32, bool)],
    right: i32,
    top: i32,
    bottom: i32,
) -> Laid {
    let padding = canvas.scaled(WELL_PADDING);
    let height = rounded(SEGMENT_HEIGHT, canvas.dpi);
    // Each segment is its word, up to a whole pixel, and the room either side of it.
    let widths: Vec<i32> = choices
        .iter()
        .map(|&(_, _, word, _)| whole_up(word) + 2 * canvas.scaled(SEGMENT_PADDING))
        .collect();
    // The well's padding on the left, then each segment with the room after it.
    let width = padding + widths.iter().map(|width| width + padding).sum::<i32>();
    let well = at_right(right, top, bottom, width, height + 2 * padding);
    let mut left = well.left + padding;
    let segments = choices
        .iter()
        .zip(&widths)
        .map(|(&(control, word, _, chosen), &width)| {
            let place = RECT {
                left,
                top: well.top + padding,
                right: left + width,
                bottom: well.top + padding + height,
            };
            left += width + padding;
            (control, place, word, chosen)
        })
        .collect();
    Laid::Choice { well, segments }
}

/// − / + at the right of the row, around `value`.
pub fn stepper(canvas: &Canvas, value: u8, right: i32, top: i32, bottom: i32) -> Laid {
    let side = canvas.scaled(STEP_BUTTON);
    let gap = canvas.scaled(STEP_GAP);
    let plus = at_right(right, top, bottom, side, side);
    let number = at_right(
        plus.left - gap,
        top,
        bottom,
        canvas.scaled(NUMBER_WIDTH),
        rounded(NUMBER_HEIGHT, canvas.dpi),
    );
    let minus = at_right(number.left - gap, top, bottom, side, side);
    Laid::Stepper {
        minus,
        number,
        plus,
        value,
    }
}

/// Where a button whose word is `word_width` pixels wide goes: at the right of the row from `top` to `bottom`, as tall
/// as the folder's line.
pub fn button_place(canvas: &Canvas, word_width: f32, right: i32, top: i32, bottom: i32) -> RECT {
    let width = whole_up(word_width) + 2 * canvas.scaled(snip_card::BUTTON_PADDING);
    let height = canvas.scaled(snip_card::FIELD_HEIGHT);
    at_right(right, top, bottom, width, height)
}

/// Where the path's box goes: from `left` to `right`, as tall as the button beside it, in the row from `top` to `bottom`.
pub fn path_place(canvas: &Canvas, left: i32, right: i32, top: i32, bottom: i32) -> RECT {
    let height = canvas.scaled(snip_card::FIELD_HEIGHT);
    at_right(right, top, bottom, right - left, height)
}

/// A measured width rounded up to whole pixels, so a segment never cuts into its word. Widths are a few hundred pixels
/// at most, far inside `i32`.
fn whole_up(width: f32) -> i32 {
    debug_assert!((0.0..1e6).contains(&width), "a word {width} pixels wide");
    width.ceil() as i32
}

/// Paints `laid`'s surfaces in `mode`: the wells, the track and its knob, the chosen segment and the small buttons,
/// with their shadows. Its words and signs are `text`'s, painted after over the window.
pub fn paint(
    canvas: &Canvas,
    laid: &Laid,
    mode: Mode,
    pointer: Pointer,
) -> windows::core::Result<()> {
    let relief = relief_palette::palette(mode);
    let colors = colors(mode);
    let well = |inset: &'static [Shadow]| Surface {
        base: Some(colors.well),
        gradient: &[],
        border: None,
        outer: &[],
        inset,
    };
    match laid {
        Laid::Switch { track, on, .. } => {
            let radius = SWITCH_HEIGHT / 2;
            if *on {
                canvas.surface(&relief.lit, track, radius, 0, false)?;
            } else {
                canvas.surface(&well(&TRACK_INSET), track, radius, 0, false)?;
            }
            let inset = canvas.scaled(KNOB_INSET);
            let side = canvas.scaled(KNOB);
            let left = if *on {
                track.right - inset - side
            } else {
                track.left + inset
            };
            let knob = RECT {
                left,
                top: track.top + inset,
                right: left + side,
                bottom: track.top + inset + side,
            };
            let knob_on = Surface {
                base: None,
                gradient: &KNOB_ON,
                border: None,
                outer: relief.button.outer,
                inset: relief.button.inset,
            };
            let surface = if *on { &knob_on } else { &relief.button };
            canvas.surface(surface, &knob, KNOB / 2, 0, false)
        }
        Laid::Choice {
            well: place,
            segments,
        } => {
            canvas.surface(&well(&WELL_INSET), place, WELL_RADIUS, 0, false)?;
            for (_, segment, _, chosen) in segments {
                if *chosen {
                    canvas.surface(&relief.lit, segment, SEGMENT_RADIUS, 0, false)?;
                }
            }
            Ok(())
        }
        Laid::Button { place, .. } => {
            canvas.surface(&relief.small, place, snip_card::BUTTON_RADIUS, 0, false)
        }
        Laid::Path { place } => canvas.surface(&well(&[]), place, snip_card::PATH_RADIUS, 0, false),
        Laid::Stepper {
            minus,
            number,
            plus,
            ..
        } => {
            canvas.surface(&well(&WELL_INSET), number, NUMBER_RADIUS, 0, false)?;
            for (control, place) in [
                (Control::ToleranceDown, minus),
                (Control::ToleranceUp, plus),
            ] {
                let button = || canvas.surface(&relief.small, place, STEP_RADIUS, 0, false);
                if pointer.pressed(control) {
                    canvas.in_motion(place, pressed(), button)?;
                } else {
                    button()?;
                }
            }
            Ok(())
        }
    }
}

/// How a held − or + moves: smaller around its middle, where it is.
pub fn pressed() -> Motion {
    Motion {
        scale: PRESSED,
        down: 0,
    }
}

/// A word or a sign to write over a control: where its box is, the text, its color, and whether it shrinks with a
/// held button.
#[derive(Debug, Clone, PartialEq)]
pub struct Label {
    pub place: RECT,
    pub text: String,
    pub ink: Color,
    pub kind: LabelKind,
    pub shrinks: bool,
}

/// Which font a label is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LabelKind {
    /// A segment's word: `600 12px`.
    Segment,
    /// The tolerance: `700 14px`.
    Number,
    /// − or +: `700 16px/1`.
    Sign,
}

/// The words and signs `laid` shows, in `mode`, centered in their boxes.
pub fn labels(laid: &Laid, mode: Mode, pointer: Pointer) -> Vec<Label> {
    let relief = relief_palette::palette(mode);
    let colors = colors(mode);
    match laid {
        Laid::Switch { .. } | Laid::Path { .. } => Vec::new(),
        Laid::Button {
            control,
            place,
            word,
        } => vec![Label {
            place: *place,
            text: (*word).to_owned(),
            ink: if pointer.hovered(*control) {
                relief.small_ink_hover
            } else {
                relief.small_ink
            },
            kind: LabelKind::Segment,
            shrinks: false,
        }],
        Laid::Choice { segments, .. } => segments
            .iter()
            .map(|&(_, place, word, chosen)| Label {
                place,
                text: word.to_owned(),
                ink: if chosen { relief.ink_on } else { relief.ink },
                kind: LabelKind::Segment,
                shrinks: false,
            })
            .collect(),
        Laid::Stepper {
            minus,
            number,
            plus,
            value,
        } => {
            let sign = |control: Control, place: RECT, text: &str| Label {
                place,
                text: text.to_owned(),
                ink: if pointer.hovered(control) {
                    relief.small_ink_hover
                } else {
                    relief.small_ink
                },
                kind: LabelKind::Sign,
                shrinks: pointer.pressed(control),
            };
            vec![
                sign(Control::ToleranceDown, *minus, "\u{2212}"),
                Label {
                    place: *number,
                    text: value.to_string(),
                    ink: colors.ink,
                    kind: LabelKind::Number,
                    shrinks: false,
                },
                sign(Control::ToleranceUp, *plus, "+"),
            ]
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use screengauge_core::settings::Folder;

    use super::*;

    #[test]
    fn a_click_on_the_switch_turns_it_to_the_other_side() {
        let mut options = Options::default();
        assert_eq!(
            change(Control::LogicalPixels, &options),
            Some(Change::AlsoShowLogicalPixels(false))
        );
        options.also_show_logical_pixels = false;
        assert_eq!(
            change(Control::LogicalPixels, &options),
            Some(Change::AlsoShowLogicalPixels(true))
        );
    }

    #[test]
    fn minus_plus_and_a_segment_change_what_they_show() {
        let options = Options::default();
        assert_eq!(
            change(Control::ToleranceDown, &options),
            Some(Change::DefaultToleranceDown)
        );
        assert_eq!(
            change(Control::ToleranceUp, &options),
            Some(Change::DefaultToleranceUp)
        );
        assert_eq!(
            change(Control::ColorFormat(ColorFormat::Rgb), &options),
            Some(Change::ColorFormat(ColorFormat::Rgb))
        );
        assert_eq!(
            change(Control::Theme(Theme::Dark), &options),
            Some(Change::Theme(Theme::Dark))
        );
    }

    fn kept_folder() -> Folder {
        Folder::new(Path::new(r"C:\Snips")).expect("an absolute path")
    }

    #[test]
    fn a_destination_changes_it_and_never_the_folder() {
        let options = Options::default();
        assert_eq!(
            change(Control::SnipDestination(SnipDestination::Both), &options),
            Some(Change::SnipDestination(SnipDestination::Both))
        );
    }

    #[test]
    fn the_folder_switch_with_a_folder_kept_turns_to_the_other_side() {
        let mut options = Options::default();
        options.choose_default_folder(kept_folder(), false);
        assert_eq!(
            change(Control::SaveToDefaultFolder, &options),
            Some(Change::SaveToDefaultFolder(true))
        );
        options.choose_default_folder(kept_folder(), true);
        assert_eq!(
            change(Control::SaveToDefaultFolder, &options),
            Some(Change::SaveToDefaultFolder(false))
        );
    }

    #[test]
    fn the_folder_switch_with_no_folder_kept_and_change_wait_for_the_folder_window() {
        let options = Options::default();
        assert_eq!(change(Control::SaveToDefaultFolder, &options), None);
        assert_eq!(change(Control::ChangeFolder, &options), None);
        let mut kept = Options::default();
        kept.choose_default_folder(kept_folder(), true);
        assert_eq!(change(Control::ChangeFolder, &kept), None);
    }

    #[test]
    fn only_a_button_held_with_the_cursor_still_on_it_looks_pressed() {
        let held_on = Pointer {
            over: Some(Control::ToleranceUp),
            held: Some(Control::ToleranceUp),
        };
        let held_off = Pointer {
            over: None,
            held: Some(Control::ToleranceUp),
        };
        assert!(held_on.pressed(Control::ToleranceUp));
        assert!(!held_off.pressed(Control::ToleranceUp));
        assert!(!held_on.pressed(Control::ToleranceDown));
    }

    #[test]
    fn a_box_at_the_right_is_centered_in_its_row() {
        assert_eq!(
            at_right(500, 100, 142, 42, 24),
            RECT {
                left: 458,
                top: 109,
                right: 500,
                bottom: 133
            }
        );
    }

    #[test]
    fn the_boxes_heights_are_the_prototype_s_measured_ones() {
        // Measured at 100% for WPC-320: a segment 26, the number's box 28.67.
        assert_eq!(rounded(SEGMENT_HEIGHT, 96), 26);
        assert_eq!(rounded(NUMBER_HEIGHT, 96), 29);
        assert_eq!(rounded(NUMBER_HEIGHT, 144), 43);
    }
}
