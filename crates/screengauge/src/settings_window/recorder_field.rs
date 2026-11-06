//! The global shortcut's row in Settings (v1 spec WPC-296, Settings; WPC-323): the recorder's field, a sunken well with
//! the shortcut's keys in relief (`.rec`, `.kc`), and Reset beside it, a button with a word (`.pbtn`,
//! `controls::Laid::Button`), drawn in the toolbar's style as the other controls are (`controls`). The sizes are the prototype's stylesheet (`prototype/measurement-overlay`,
//! `#settings`), with `box-sizing: border-box`.
//!
//! While it records, the field has a cyan border and ring, and shows the modifiers held so far in relief and then a
//! blinking "…", or, with nothing held, a blinking "Press the keys…" (`.rec.recording`, `.wait`).

use std::time::Duration;

use screengauge_core::settings::{Modifiers, Refusal, Shortcut};
use windows::Win32::Foundation::RECT;

use super::controls::{self, Control, Label, LabelKind, RowEnd, rounded, whole_up};
use super::row_message::Text;
use super::{layout, snip_card};

use crate::graphics::Color;
use crate::look;
use crate::relief::palette::{self as relief_palette, Shadow, Surface, shadow};
use crate::relief::{self, Canvas};
use crate::theme::Mode;

/// What the field and the button say.
pub const RESET: &str = "Reset";
pub const PRESS_THE_KEYS: &str = "Press the keys\u{2026}";
const WAITING: &str = "\u{2026}";

/// The message under the row's label for `refusal`.
pub fn message(refusal: Refusal) -> &'static str {
    match refusal {
        Refusal::Incomplete => {
            "Use Win, Ctrl or Alt with a letter, a number or a key from F1 to F12."
        }
        Refusal::CtrlAltWithoutWin => {
            "Ctrl + Alt without Win isn't allowed: on Brazilian keyboards it's AltGr and would take characters like / and ?."
        }
        Refusal::AlreadyUsed => "Already used by another program",
        Refusal::Common => "Used by Windows and most programs.",
    }
}

/// What the row says under its label, and whether the amber band shows across it, with `refusal` the recorder's,
/// should it have one, and `busy` the global shortcut Windows refused at startup, while it has no other (WPC-324). While
/// the shortcut is busy, the field resting shows the band, as the prototype does (`settings.js`, `shortcutBusy && !rec`),
/// and says so, unless a combination tried was refused: the refusal says the try wasn't taken (the owner's call in
/// WPC-324; the prototype went back to the busy message). While it records, the band goes, and only the recorder's
/// refusals show.
pub fn said_under_label(
    refusal: Option<Refusal>,
    recording: bool,
    busy: Option<Shortcut>,
) -> Option<(Text, bool)> {
    let band = busy.is_some() && !recording;
    let text = match (refusal, busy) {
        (Some(refusal), _) => Text::Fixed(message(refusal)),
        (None, Some(shortcut)) if band => Text::ShortcutBusy(shortcut),
        (None, _) => return None,
    };
    Some((text, band))
}

// Every size below is in logical pixels.

/// The field (`.rec`): its least width, its height, the room inside its 1-pixel border left and right, its corners, and
/// the room between the keys and the plus signs.
const FIELD_MIN_WIDTH: u32 = 156;
const FIELD_HEIGHT: u32 = 32;
const FIELD_PADDING: u32 = 8;
const FIELD_RADIUS: u32 = 9;
const FIELD_BORDER: u32 = 1;
const FIELD_GAP: u32 = 4;

/// A key in relief (`.kc`): its word's size (`600 11.5px`), the room around it (`padding: 3px 7px`) and its corners. Its
/// height is its word's normal line and 3 above and below.
pub const KEYCAP_TEXT: f32 = 11.5;
const KEYCAP_PADDING: u32 = 7;
const KEYCAP_RADIUS: u32 = 6;
const KEYCAP_HEIGHT: f32 = KEYCAP_TEXT * controls::NORMAL_LINE + 6.0;

/// The plus between two keys (`.plus`: `font-size: 11px`), and the blinking text (`.wait`: the window's 13px, italic).
pub const PLUS_TEXT: f32 = 11.0;
pub const WAITING_TEXT: f32 = layout::LABEL_TEXT;

/// Reset is the Snip's "Change…" button (`.pbtn`, `600 12px` as a segment's word): as tall as the folder's line, with
/// the same room either side of its word. Between it and the field is the row's gap (`.srow`'s `gap`).
const BUTTON_HEIGHT: u32 = snip_card::FIELD_HEIGHT;
const BUTTON_PADDING: u32 = snip_card::BUTTON_PADDING;
const BUTTON_GAP: u32 = layout::ROW_GAP;

/// The recording field's ring: `look`'s `--accent` at 30%
/// (`box-shadow: 0 0 0 3px color-mix(in oklch, var(--accent) 30%, transparent)`).
const DARK_RING: [Shadow; 1] = [shadow(
    0,
    0,
    3,
    Color {
        alpha: 0.3,
        ..look::DARK.accent
    },
)];
const LIGHT_RING: [Shadow; 1] = [shadow(
    0,
    0,
    3,
    Color {
        alpha: 0.3,
        ..look::LIGHT.accent
    },
)];

/// The blink (`animation: blink 1.1s infinite`, `@keyframes blink { 50% { opacity: .35; } }`).
const BLINK: Duration = Duration::from_millis(1100);
const DIMMEST: f32 = 0.35;

/// What the field shows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct View {
    /// The global shortcut, shown while the field rests.
    pub shortcut: Shortcut,
    pub recording: bool,
    /// The modifiers held while it records.
    pub held: Modifiers,
    /// How opaque the blinking text is now, from 0.35 to 1.
    pub opacity: f32,
}

/// One thing in the field, left to right.
#[derive(Debug, Clone, PartialEq)]
enum Piece {
    Key(RECT, String),
    Plus(RECT),
    Waiting(RECT, &'static str),
}

/// The field laid out in the window's pixels.
#[derive(Debug, Clone, PartialEq)]
pub struct Laid {
    pub field: RECT,
    recording: bool,
    pieces: Vec<Piece>,
    opacity: f32,
}

/// What the field shows, left to right, before it is laid out: each key's word, the plus signs, and the blinking text.
fn words(view: &View) -> Vec<(LabelKind, String)> {
    let names = |held: Modifiers| {
        [
            (held.win, "Win"),
            (held.ctrl, "Ctrl"),
            (held.alt, "Alt"),
            (held.shift, "Shift"),
        ]
        .into_iter()
        .filter(|&(on, _)| on)
        .map(|(_, name)| name.to_owned())
        .collect::<Vec<String>>()
    };
    let mut keys = if view.recording {
        names(view.held)
    } else {
        names(view.shortcut.modifiers())
    };
    let last = if view.recording {
        None
    } else {
        Some(view.shortcut.key().to_string())
    };
    keys.extend(last);
    let mut words = Vec::new();
    for (index, key) in keys.into_iter().enumerate() {
        if index > 0 {
            words.push((LabelKind::Plus, "+".to_owned()));
        }
        words.push((LabelKind::Keycap, key));
    }
    if view.recording {
        if words.is_empty() {
            words.push((LabelKind::Waiting, PRESS_THE_KEYS.to_owned()));
        } else {
            words.push((LabelKind::Plus, "+".to_owned()));
            words.push((LabelKind::Waiting, WAITING.to_owned()));
        }
    }
    words
}

/// Lays out the field and Reset at the row's `end`, at `dpi`, with `measure` giving a word's width in pixels in the font
/// of its kind: the field, and where Reset goes.
pub fn lay(
    view: &View,
    dpi: u32,
    measure: &dyn Fn(&str, LabelKind) -> windows::core::Result<f32>,
    end: RowEnd,
) -> windows::core::Result<(Laid, RECT)> {
    let scaled = |logical: u32| relief::scaled(logical, dpi);
    let reset_width = whole_up(measure(RESET, LabelKind::Segment)?) + 2 * scaled(BUTTON_PADDING);
    let reset = end.place(reset_width, scaled(BUTTON_HEIGHT));
    let words = words(view);
    let mut widths = Vec::with_capacity(words.len());
    for (kind, word) in &words {
        let padding = if *kind == LabelKind::Keycap {
            2 * scaled(KEYCAP_PADDING)
        } else {
            0
        };
        widths.push(whole_up(measure(word, *kind)?) + padding);
    }
    let border = scaled(FIELD_BORDER).max(1);
    let gap = scaled(FIELD_GAP);
    let count = i32::try_from(widths.len()).unwrap_or(i32::MAX);
    let content = widths.iter().sum::<i32>() + gap * (count - 1).max(0);
    let width = (2 * (border + scaled(FIELD_PADDING)) + content).max(scaled(FIELD_MIN_WIDTH));
    let field = end
        .ending_at(reset.left - scaled(BUTTON_GAP))
        .place(width, scaled(FIELD_HEIGHT));
    let keycap_height = rounded(KEYCAP_HEIGHT, dpi);
    let mut left = field.left + border + scaled(FIELD_PADDING);
    let pieces = words
        .into_iter()
        .zip(widths)
        .map(|((kind, word), width)| {
            let line = RECT {
                left,
                top: field.top + border,
                right: left + width,
                bottom: field.bottom - border,
            };
            left += width + gap;
            match kind {
                LabelKind::Keycap => {
                    // Centered in the field's height, which the line inside its border shares.
                    let in_field = RowEnd {
                        right: line.right,
                        top: field.top,
                        bottom: field.bottom,
                    };
                    Piece::Key(in_field.place(width, keycap_height), word)
                }
                LabelKind::Plus => Piece::Plus(line),
                _ if word == WAITING => Piece::Waiting(line, WAITING),
                _ => Piece::Waiting(line, PRESS_THE_KEYS),
            }
        })
        .collect();
    let laid = Laid {
        field,
        recording: view.recording,
        pieces,
        opacity: view.opacity,
    };
    Ok((laid, reset))
}

/// The room a message under the row's label has, in logical pixels: from the label's left to the field, less the
/// row's gap. `measure` gives a word's width at 100%.
///
/// The field is as wide as `shortcut` resting, and as the four modifiers held while it records: the room is what the
/// wider of the two leaves, so the message keeps its lines, and the window its height, whatever the user holds, and
/// never runs under the field.
pub fn message_room(
    shortcut: Shortcut,
    measure: &dyn Fn(&str, LabelKind) -> windows::core::Result<f32>,
) -> windows::core::Result<f32> {
    let all = Modifiers {
        win: true,
        ctrl: true,
        alt: true,
        shift: true,
    };
    let resting = View {
        shortcut,
        recording: false,
        held: Modifiers::default(),
        opacity: 1.0,
    };
    let recording = View {
        recording: true,
        held: all,
        ..resting
    };
    let inner = layout::ROW_INSET;
    let right = i32::try_from(layout::WIDTH - inner).unwrap_or(i32::MAX);
    let mut left_most = right;
    for view in [resting, recording] {
        let (laid, _) = lay(
            &view,
            96,
            measure,
            RowEnd {
                right,
                top: 0,
                bottom: 42,
            },
        )?;
        left_most = left_most.min(laid.field.left);
    }
    let left = i32::try_from(inner).unwrap_or(0);
    let room = left_most - i32::try_from(BUTTON_GAP).unwrap_or(0) - left;
    // The room is a few hundred pixels, which a float holds exactly.
    Ok(f32::from(i16::try_from(room.max(0)).unwrap_or(i16::MAX)))
}

impl Laid {
    /// The part a click acts on, and where it is.
    pub fn hits(&self) -> Vec<(Control, RECT)> {
        vec![(Control::ShortcutField, self.field)]
    }
}

/// Paints the field's well and its keys in relief, with their shadows, in `mode`.
pub fn paint(canvas: &Canvas, laid: &Laid, mode: Mode) -> windows::core::Result<()> {
    let relief = relief_palette::palette(mode);
    let well = controls::well(mode);
    let accent = look::palette(mode).accent;
    let ring = match mode {
        Mode::Dark => &DARK_RING,
        Mode::Light => &LIGHT_RING,
    };
    if laid.recording {
        // The ring replaces the well's inset shadow, and is cut out under the see-through well.
        let recording = Surface {
            base: Some(well),
            gradient: &[],
            border: Some(accent),
            outer: ring,
            inset: &[],
        };
        canvas.surface(&recording, &laid.field, FIELD_RADIUS, FIELD_BORDER, true)?;
    } else {
        let resting = Surface {
            base: Some(well),
            gradient: &[],
            border: None,
            outer: &[],
            inset: &controls::WELL_INSET,
        };
        canvas.surface(&resting, &laid.field, FIELD_RADIUS, FIELD_BORDER, false)?;
    }
    for piece in &laid.pieces {
        if let Piece::Key(place, _) = piece {
            canvas.surface(&relief.button, place, KEYCAP_RADIUS, 0, false)?;
        }
    }
    Ok(())
}

/// The words the field shows, in `mode`, centered in their boxes.
pub fn labels(laid: &Laid, mode: Mode) -> Vec<Label> {
    let relief = relief_palette::palette(mode);
    let dim = look::palette(mode).dim;
    let label = |place: RECT, text: &str, ink: Color, kind: LabelKind| Label {
        place,
        text: text.to_owned(),
        ink,
        kind,
        shrinks: false,
    };
    laid.pieces
        .iter()
        .map(|piece| match piece {
            Piece::Key(place, word) => label(*place, word, relief.ink, LabelKind::Keycap),
            Piece::Plus(place) => label(*place, "+", dim, LabelKind::Plus),
            Piece::Waiting(place, text) => label(
                *place,
                text,
                Color {
                    alpha: laid.opacity,
                    ..dim
                },
                LabelKind::Waiting,
            ),
        })
        .collect()
}

/// How opaque the blinking text is `since` recording started: from 1 down to 0.35 and back every 1.1 s, each half on
/// CSS's `ease` curve.
pub fn opacity(since: Duration) -> f32 {
    let period = BLINK.as_secs_f32();
    let phase = (since.as_secs_f32() % period) / period;
    let (from, to, along) = if phase < 0.5 {
        (1.0, DIMMEST, phase * 2.0)
    } else {
        (DIMMEST, 1.0, (phase - 0.5) * 2.0)
    };
    from + (to - from) * ease(along)
}

/// CSS's `ease`, `cubic-bezier(0.25, 0.1, 0.25, 1)`: how far along the change is at `x`, from 0 to 1, of its time.
fn ease(x: f32) -> f32 {
    let bezier = |t: f32, p1: f32, p2: f32| {
        let u = 1.0 - t;
        3.0 * u * u * t * p1 + 3.0 * u * t * t * p2 + t * t * t
    };
    // The curve's x grows with t, so halving finds the t of `x`; 24 halvings reach a float's precision.
    let (mut low, mut high) = (0.0, 1.0);
    for _ in 0..24 {
        let middle = f32::midpoint(low, high);
        if bezier(middle, 0.25, 0.25) < x {
            low = middle;
        } else {
            high = middle;
        }
    }
    bezier(f32::midpoint(low, high), 0.1, 1.0)
}

#[cfg(test)]
mod tests {
    use screengauge_core::settings::Key;

    use super::*;

    fn win_alt() -> Modifiers {
        Modifiers {
            win: true,
            alt: true,
            ..Modifiers::default()
        }
    }

    fn view(recording: bool, held: Modifiers) -> View {
        View {
            shortcut: Shortcut::default(),
            recording,
            held,
            opacity: 1.0,
        }
    }

    fn texts(view: &View) -> Vec<String> {
        words(view).into_iter().map(|(_, word)| word).collect()
    }

    #[test]
    fn the_busy_shortcut_s_message_names_it() {
        assert_eq!(
            Text::ShortcutBusy(Shortcut::default()).to_string(),
            "Win + Shift + X is already used by another program. Choose another one."
        );
        assert_eq!(
            Text::Fixed("Already used by another program").to_string(),
            "Already used by another program"
        );
    }

    #[test]
    fn resting_while_the_shortcut_is_busy_the_row_says_so_on_the_band() {
        let busy = Some(Shortcut::default());
        assert_eq!(
            said_under_label(None, false, busy),
            Some((Text::ShortcutBusy(Shortcut::default()), true))
        );
    }

    #[test]
    fn a_combination_tried_while_the_shortcut_is_busy_and_refused_says_so_on_the_band() {
        // Win + Shift + X is busy; the user tries Ctrl + Shift + S, which another program holds too. Recording stops and
        // the field shows Win + Shift + X again: the row tells the try wasn't taken, and the band stays (the owner's call
        // in WPC-324).
        let busy = Some(Shortcut::default());
        assert_eq!(
            said_under_label(Some(Refusal::AlreadyUsed), false, busy),
            Some((Text::Fixed("Already used by another program"), true))
        );
    }

    #[test]
    fn recording_while_the_shortcut_is_busy_the_band_goes_and_the_recorder_speaks() {
        let busy = Some(Shortcut::default());
        assert_eq!(said_under_label(None, true, busy), None);
        assert_eq!(
            said_under_label(Some(Refusal::Incomplete), true, busy),
            Some((Text::Fixed(message(Refusal::Incomplete)), false))
        );
    }

    #[test]
    fn with_the_shortcut_working_only_the_recorder_s_refusals_show_with_no_band() {
        assert_eq!(said_under_label(None, false, None), None);
        assert_eq!(said_under_label(None, true, None), None);
        assert_eq!(
            said_under_label(Some(Refusal::AlreadyUsed), false, None),
            Some((Text::Fixed(message(Refusal::AlreadyUsed)), false))
        );
    }

    #[test]
    fn resting_the_field_shows_the_shortcut_s_keys_with_plus_signs_between() {
        assert_eq!(
            texts(&view(false, Modifiers::default())),
            ["Win", "+", "Shift", "+", "X"]
        );
        let f12 = View {
            shortcut: Shortcut::new(
                Modifiers {
                    win: true,
                    ctrl: true,
                    alt: true,
                    shift: true,
                },
                Key::Function(12),
            )
            .expect("allowed"),
            ..view(false, Modifiers::default())
        };
        assert_eq!(
            texts(&f12),
            ["Win", "+", "Ctrl", "+", "Alt", "+", "Shift", "+", "F12"]
        );
    }

    #[test]
    fn recording_with_nothing_held_it_asks_for_the_keys() {
        assert_eq!(
            texts(&view(true, Modifiers::default())),
            ["Press the keys…"]
        );
    }

    #[test]
    fn recording_it_shows_the_modifiers_held_and_waits_for_the_main_key() {
        let words = words(&view(true, win_alt()));
        assert_eq!(
            words,
            [
                (LabelKind::Keycap, "Win".to_owned()),
                (LabelKind::Plus, "+".to_owned()),
                (LabelKind::Keycap, "Alt".to_owned()),
                (LabelKind::Plus, "+".to_owned()),
                (LabelKind::Waiting, "…".to_owned()),
            ]
        );
    }

    /// The row the tests lay the field in: 42 pixels tall, ending at 531.
    const ROW: RowEnd = RowEnd {
        right: 531,
        top: 100,
        bottom: 142,
    };

    /// Every word 10 pixels wide, a key 30 with its room, at 100%.
    fn ten(_: &str, _: LabelKind) -> windows::core::Result<f32> {
        Ok(10.0)
    }

    #[test]
    fn the_field_is_at_least_156_wide_left_of_reset_and_both_are_centered_in_the_row() {
        let (laid, reset) = lay(&view(false, Modifiers::default()), 96, &ten, ROW).expect("laid");
        // Reset: its word and 11 either side, 30 tall, against the right; the field 10 left of it, 32 tall.
        assert_eq!(
            reset,
            RECT {
                left: 499,
                top: 106,
                right: 531,
                bottom: 136
            }
        );
        assert_eq!(
            laid.field,
            RECT {
                left: 489 - 156,
                top: 105,
                right: 489,
                bottom: 137
            }
        );
    }

    #[test]
    fn many_keys_widen_the_field_and_the_keys_start_inside_its_padding() {
        let all = View {
            shortcut: Shortcut::new(
                Modifiers {
                    win: true,
                    ctrl: true,
                    alt: true,
                    shift: true,
                },
                Key::Function(12),
            )
            .expect("allowed"),
            ..view(false, Modifiers::default())
        };
        let (laid, _) = lay(&all, 96, &ten, ROW).expect("laid");
        // Five keys of 24 (10 and 7 either side), four plus signs of 10, eight gaps of 4, 8 of room and a border either
        // side: 120 + 40 + 32 + 18.
        assert_eq!(laid.field.right - laid.field.left, 210);
        let Some(Piece::Key(first, word)) = laid.pieces.first() else {
            panic!("the first piece is a key: {:?}", laid.pieces);
        };
        assert_eq!(word, "Win");
        assert_eq!(first.left, laid.field.left + 9);
        // A key is 21 tall at 100% (its 11.5-pixel word's normal line and 3 above and below), centered in the field.
        assert_eq!(first.bottom - first.top, 21);
        assert_eq!(
            laid.field.bottom - first.bottom,
            first.top - laid.field.top + 1
        );
    }

    #[test]
    fn the_message_s_room_runs_from_the_label_to_the_widest_the_field_can_be() {
        let room = message_room(Shortcut::default(), &ten).expect("room");
        // Recording with the four modifiers held, the field is four keys of 24, four plus signs and "…" of 10,
        // eight gaps of 4 and 18 of padding and border: 196, wider than Win + Shift + X resting. 531 less Reset's 32,
        // the gap of 10, the field and the gap of 10, less the label's left at 29.
        assert!(
            (room - (531.0 - 32.0 - 10.0 - 196.0 - 10.0 - 29.0)).abs() < 0.01,
            "{room}"
        );
        // A shortcut whose resting field is wider leaves less.
        let widest = Shortcut::new(
            Modifiers {
                win: true,
                ctrl: true,
                alt: true,
                shift: true,
            },
            Key::Function(12),
        )
        .expect("allowed");
        assert!(message_room(widest, &ten).expect("room") < room);
    }

    #[test]
    fn the_messages_are_the_spec_s() {
        assert_eq!(
            message(Refusal::Incomplete),
            "Use Win, Ctrl or Alt with a letter, a number or a key from F1 to F12."
        );
        assert_eq!(
            message(Refusal::CtrlAltWithoutWin),
            "Ctrl + Alt without Win isn't allowed: on Brazilian keyboards it's AltGr and would take characters like / and ?."
        );
        assert_eq!(
            message(Refusal::AlreadyUsed),
            "Already used by another program"
        );
        assert_eq!(
            message(Refusal::Common),
            "Used by Windows and most programs."
        );
    }

    #[test]
    fn the_text_blinks_from_full_to_0_35_at_half_the_period_and_back() {
        let at = |ms: u64| opacity(Duration::from_millis(ms));
        assert!((at(0) - 1.0).abs() < 1e-3);
        assert!((at(550) - 0.35).abs() < 1e-3);
        assert!((at(1100) - 1.0).abs() < 1e-3);
        // `ease` is slow at both ends and fast in between: a quarter of the way in, more than half the fading is done.
        let quarter = at(137);
        assert!(quarter < 1.0 - 0.65 / 4.0, "{quarter}");
        // Every value stays between the two.
        for ms in (0..2200).step_by(10) {
            let value = at(ms);
            assert!((0.35 - 1e-3..=1.0 + 1e-3).contains(&value), "{ms}: {value}");
        }
    }

    #[test]
    fn ease_is_css_s_curve() {
        // Values of cubic-bezier(0.25, 0.1, 0.25, 1), the CSS `ease`.
        assert!(ease(0.0).abs() < 1e-6);
        assert!((ease(1.0) - 1.0).abs() < 1e-6);
        assert!((ease(0.5) - 0.8024).abs() < 1e-3, "{}", ease(0.5));
    }
}
