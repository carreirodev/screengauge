//! Where Inspect's magnified view goes and what it holds (v1 spec WPC-296, Inspect; the prototype's variant A, WPC-279:
//! `drawMag`, `.panel`, `.magframe` and `.info`). Every size is the prototype's, in logical pixels, which
//! [`scaled`](super::toolbar::scaled) turns into physical pixels for the current monitor's scale. The strip's lines have
//! a fixed height, as CSS gives them, so the panel's size needs no font and the session core places it.

use super::frozen_screen::{Colors, Photo, Point, Rect};
use super::output::{
    ApproximateColor, ColorText, Coordinate, MagnifiedView, NoticeLine, PixelColor,
};
use super::toolbar::scaled;
use super::{color, pixels, units};
use crate::settings::{ColorFormat, Magnification};

// Every size below is in logical pixels.

/// About how wide and tall the enlarged pixels are: as many whole pixels as fit, an odd number of them (v1 spec
/// WPC-296, derived rule 10).
const SIDE: u32 = 176;
/// The fewest pixels a side the view ever holds, as in the prototype.
const FEWEST: u32 = 3;
/// How far right of and below the cursor the panel starts.
const REACH: u32 = 22;
/// How near the monitor's side the panel may come before it goes to the cursor's other side, on that axis alone.
const FLIP: u32 = 4;
/// How near the monitor's side the panel may come at all, on whichever side of the cursor it is.
const MARGIN: u32 = 2;
/// The panel's border (`.panel`'s `border: 1px`) and the room inside it around what it holds (`padding: 6px`).
const RING: u32 = 1;
const PAD: u32 = 6;
/// The strip under the frame (`.info`): its room above, at the sides and below its lines (`padding: 7px 4px 2px`).
const STRIP_TOP: u32 = 7;
const STRIP_SIDE: u32 = 4;
const STRIP_BOTTOM: u32 = 2;
/// The height of one line of the strip, in hundredths of a logical pixel: CSS's 12px text at a line height of 1.35.
const LINE_HUNDREDTHS: u32 = 1620;
/// The room between X and Y when they share a line (the grid's `gap: 0 8px`).
const COLUMN_GAP: u32 = 8;
/// The room between the cursor's position and the color's row under it (the row's `margin-top: 5px`).
const COLOR_GAP: u32 = 5;
/// The swatch's side, its ring inside it (`.sw`, 22px by 22px with `box-sizing: border-box`).
const SWATCH: u32 = 22;
/// The room between the swatch and the color's text (the row's `gap: 8px`).
const SWATCH_GAP: u32 = 8;
/// The Approximate color notice under the color's row (the prototype's `.hdr` in a `div` with `margin-top: 4px`): the
/// room above it, the height of each of its two lines in hundredths of a logical pixel (11px text at the strip's line
/// height of 1.35), its sign's side, and the room between the sign and the text (`gap: 5px`).
const NOTICE_GAP: u32 = 4;
const NOTICE_LINE_HUNDREDTHS: u32 = 1485;
const NOTICE_SIGN: u32 = 13;
const NOTICE_SIGN_GAP: u32 = 5;
/// What the notice says, a line each (the owner's choice in WPC-328).
const NOTICE_FIRST: &str = "Approximate color";
const NOTICE_SECOND: &str = "(HDR on)";

/// The lowest magnification with a grid between the enlarged pixels.
const GRID_FROM: Magnification = Magnification::X8;

/// The magnified view around `center`, the pixel under the cursor, on `photo`'s monitor, the frozen screen's monitor
/// `monitor`: at `magnification`, with its tag while `tag`, the cursor's position with its logical pixels when
/// `also_show_logical_pixels` and the monitor's scale isn't 100%, and the pixel's color in `color_format`.
pub(super) fn magnified_view(
    photo: &Photo,
    monitor: usize,
    center: Point,
    magnification: Magnification,
    tag: bool,
    also_show_logical_pixels: bool,
    color_format: ColorFormat,
) -> MagnifiedView {
    let dpi = photo.scale.dpi();
    let factor = magnification.factor();
    let across = pixels_across(dpi, magnification);
    let half = across / 2;
    let source = Rect {
        left: center.x.saturating_sub_unsigned(half),
        top: center.y.saturating_sub_unsigned(half),
        width: across,
        height: across,
    };
    let x = units::label(
        &[center.x.abs_diff(photo.rect.left)],
        photo.scale,
        also_show_logical_pixels,
    );
    let y = units::label(
        &[center.y.abs_diff(photo.rect.top)],
        photo.scale,
        also_show_logical_pixels,
    );
    // X and Y with their logical pixels don't fit the strip side by side (the owner's choice in WPC-315).
    let stacked = x.logical.is_some();
    let rgb = pixels::color(photo, center);
    let color::Texts {
        first,
        other,
        other_shows,
    } = color::texts(rgb, color_format);
    let second = other_shows.then_some(other);

    let side = across.saturating_mul(factor);
    let inset = scaled(RING, dpi) + scaled(PAD, dpi);
    let line = line_height(LINE_HUNDREDTHS, dpi);
    let lines = if stacked { 2 } else { 1 };
    let swatch_side = scaled(SWATCH, dpi);
    let color_lines = if second.is_some() { 2 } else { 1 };
    // The color's row is as tall as the swatch or its text, whichever is taller (`align-items: center`).
    let row = swatch_side.max(color_lines * line);
    // The notice shows only on a monitor whose photo didn't come through the exact path (v1 spec WPC-296, Colors and
    // HDR): the photo's mark already says HDR is on there.
    let approximate = photo.colors == Colors::Approximate;
    let notice_line = line_height(NOTICE_LINE_HUNDREDTHS, dpi);
    let notice_row = notice_line * 2;
    let notice_height = if approximate {
        scaled(NOTICE_GAP, dpi) + notice_row
    } else {
        0
    };
    let strip = scaled(STRIP_TOP, dpi)
        + lines * line
        + scaled(COLOR_GAP, dpi)
        + row
        + notice_height
        + scaled(STRIP_BOTTOM, dpi);
    let width = inset + side + inset;
    let height = inset + side + strip + inset;
    let panel = Rect {
        left: place(center.x, width, photo.rect.left, photo.rect.width, dpi),
        top: place(center.y, height, photo.rect.top, photo.rect.height, dpi),
        width,
        height,
    };
    let frame = Rect {
        left: panel.left.saturating_add_unsigned(inset),
        top: panel.top.saturating_add_unsigned(inset),
        width: side,
        height: side,
    };

    let text_left = frame.left.saturating_add_unsigned(scaled(STRIP_SIDE, dpi));
    let text_width = side.saturating_sub(2 * scaled(STRIP_SIDE, dpi));
    let first_line = frame
        .top
        .saturating_add_unsigned(side)
        .saturating_add_unsigned(scaled(STRIP_TOP, dpi));
    let (x_line, y_line) = if stacked {
        let x_line = Rect {
            left: text_left,
            top: first_line,
            width: text_width,
            height: line,
        };
        let y_line = Rect {
            top: first_line.saturating_add_unsigned(line),
            ..x_line
        };
        (x_line, y_line)
    } else {
        let column = text_width.saturating_sub(scaled(COLUMN_GAP, dpi)) / 2;
        let x_line = Rect {
            left: text_left,
            top: first_line,
            width: column,
            height: line,
        };
        let y_line = Rect {
            left: text_left.saturating_add_unsigned(text_width - column),
            ..x_line
        };
        (x_line, y_line)
    };

    let row_top = first_line
        .saturating_add_unsigned(lines * line)
        .saturating_add_unsigned(scaled(COLOR_GAP, dpi));
    let swatch = Rect {
        left: text_left,
        top: row_top.saturating_add_unsigned(offset_to_center(row, swatch_side)),
        width: swatch_side,
        height: swatch_side,
    };
    let beside = swatch_side + scaled(SWATCH_GAP, dpi);
    let first_color_line = Rect {
        left: text_left.saturating_add_unsigned(beside),
        top: row_top.saturating_add_unsigned(offset_to_center(row, color_lines * line)),
        width: text_width.saturating_sub(beside),
        height: line,
    };
    let color = PixelColor {
        rgb,
        swatch,
        first: ColorText {
            text: first,
            line: first_color_line,
        },
        second: second.map(|text| ColorText {
            text,
            line: Rect {
                top: first_color_line.top.saturating_add_unsigned(line),
                ..first_color_line
            },
        }),
    };

    let approximate_color = approximate.then(|| {
        let top = row_top
            .saturating_add_unsigned(row)
            .saturating_add_unsigned(scaled(NOTICE_GAP, dpi));
        let sign_side = scaled(NOTICE_SIGN, dpi);
        let after_sign = sign_side + scaled(NOTICE_SIGN_GAP, dpi);
        let first_notice_line = Rect {
            left: text_left.saturating_add_unsigned(after_sign),
            top,
            width: text_width.saturating_sub(after_sign),
            height: notice_line,
        };
        ApproximateColor {
            sign: Rect {
                left: text_left,
                top: top.saturating_add_unsigned(offset_to_center(notice_row, sign_side)),
                width: sign_side,
                height: sign_side,
            },
            lines: [
                NoticeLine {
                    text: NOTICE_FIRST,
                    line: first_notice_line,
                },
                NoticeLine {
                    text: NOTICE_SECOND,
                    line: Rect {
                        top: top.saturating_add_unsigned(notice_line),
                        ..first_notice_line
                    },
                },
            ],
        }
    });

    MagnifiedView {
        monitor,
        panel,
        frame,
        source,
        shown: source.overlap(photo.rect),
        center,
        magnification,
        grid: magnification >= GRID_FROM,
        tag,
        x: Coordinate {
            label: x,
            line: x_line,
        },
        y: Coordinate {
            label: y,
            line: y_line,
        },
        color,
        approximate_color,
    }
}

/// How far below the top of a row `outer` pixels tall something `inner` pixels tall starts when centered in it: half
/// the spare room, rounded up as Chromium puts a box on the pixel grid.
fn offset_to_center(outer: u32, inner: u32) -> u32 {
    outer.saturating_sub(inner).div_ceil(2)
}

/// How many of the frozen screen's pixels the view holds a side on a monitor of `dpi` at `magnification`: the most whole
/// enlarged pixels that fit in `SIDE`, an odd number of them so the cursor's is the middle one, and never fewer than
/// `FEWEST` (the prototype's `drawMag`).
fn pixels_across(dpi: u32, magnification: Magnification) -> u32 {
    let fit = scaled(SIDE, dpi) / magnification.factor();
    let odd = if fit.is_multiple_of(2) {
        fit.saturating_sub(1)
    } else {
        fit
    };
    odd.max(FEWEST)
}

/// The height of a line `hundredths` hundredths of a logical pixel tall on a monitor of `dpi`, in whole physical pixels,
/// halves up.
fn line_height(hundredths: u32, dpi: u32) -> u32 {
    hundredths.saturating_mul(dpi).saturating_add(4800) / 9600
}

/// Where the panel, `size` physical pixels along one axis, starts on that axis, with the cursor at `cursor` on a monitor
/// that starts at `start` and is `length` long there (the prototype's `drawMag`): `REACH` after the cursor, or, should
/// that bring it within `FLIP` of the monitor's end, `REACH` before it; then kept `MARGIN` inside the monitor, which may
/// put it over the cursor on a monitor too small for either side. Should the monitor be smaller than the panel, it
/// starts `MARGIN` inside the monitor's start.
fn place(cursor: i32, size: u32, start: i32, length: u32, dpi: u32) -> i32 {
    let end = start.saturating_add_unsigned(length);
    let reach = scaled(REACH, dpi);
    let after = cursor.saturating_add_unsigned(reach);
    let wanted =
        if after.saturating_add_unsigned(size) > end.saturating_sub_unsigned(scaled(FLIP, dpi)) {
            cursor
                .saturating_sub_unsigned(reach)
                .saturating_sub_unsigned(size)
        } else {
            after
        };
    let margin = scaled(MARGIN, dpi);
    // Not `clamp`, which panics when the monitor is smaller than the panel.
    wanted
        .min(
            end.saturating_sub_unsigned(margin)
                .saturating_sub_unsigned(size),
        )
        .max(start.saturating_add_unsigned(margin))
}
