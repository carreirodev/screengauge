//! When the toolbar is on screen, told from outside: the routine lays a stage of one color under the toolbar's zone,
//! reads a few of the screen's pixels in the middle of the toolbar's tools again and again from the global shortcut on,
//! and the toolbar is there from the first read that shows what the pixels end on, once they stay that way. The frozen
//! screen alone shows the stage's photo, so only the toolbar changes those pixels; a fade or a first frame of another
//! color would not count as the toolbar until it settles. Nothing here touches Windows.

use std::time::Duration;

use screengauge_core::session::{Point, Rect};

/// How far apart, on any channel, two reads of the same pixels may be and still count as the same: room for the
/// rounding of a photo taken with HDR on.
const SAME: u8 = 8;
/// How far from the stage's color, on some channel, a pixel must be to count as something drawn over the stage.
const CHANGED: u8 = 48;
/// How long the pixels are read after they first stop being the stage, to see what they end on.
pub const SETTLE: Duration = Duration::from_millis(400);

/// One read of the pixels, red, green and blue, with the time since the global shortcut.
#[derive(Debug, Clone, PartialEq)]
pub struct Read {
    pub at: Duration,
    pub pixels: Vec<[u8; 3]>,
}

fn near(a: [u8; 3], b: [u8; 3], within: u8) -> bool {
    a.iter().zip(b).all(|(&a, b)| a.abs_diff(b) <= within)
}

/// Whether every one of `pixels` is still the stage's `color`.
pub fn is_stage(pixels: &[[u8; 3]], color: [u8; 3]) -> bool {
    pixels.iter().all(|&pixel| near(pixel, color, CHANGED))
}

fn same(a: &[[u8; 3]], b: &[[u8; 3]]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(&a, &b)| near(a, b, SAME))
}

/// When the toolbar appeared: the time of the first of `reads` from which every read shows what the last one does,
/// the last one not being the stage's `color`. `None` when the last read is still the stage.
pub fn appeared_at(reads: &[Read], color: [u8; 3]) -> Option<Duration> {
    let last = reads.last()?;
    if is_stage(&last.pixels, color) {
        return None;
    }
    let settled = reads
        .iter()
        .rposition(|read| !same(&read.pixels, &last.pixels))
        .map_or(0, |unlike| unlike + 1);
    Some(reads[settled].at)
}

/// Where to put the cursor on `monitor` so the tool it opens with draws away from `toolbar`: on a row and a column the
/// toolbar doesn't cross, an eighth of the monitor in from two sides, so Dimensions' lines through the cursor never
/// reach the pixels read. `None` when the toolbar spans the monitor's every such row or column.
pub fn cursor_spot(monitor: Rect, toolbar: Rect) -> Option<Point> {
    let span = |start: i32, length: u32| (i64::from(start), i64::from(start) + i64::from(length));
    let (left, right) = span(monitor.left, monitor.width);
    let (top, bottom) = span(monitor.top, monitor.height);
    let (toolbar_left, toolbar_right) = span(toolbar.left, toolbar.width);
    let (toolbar_top, toolbar_bottom) = span(toolbar.top, toolbar.height);
    let inset = |from: i64, to: i64| [from + (to - from) / 8, to - 1 - (to - from) / 8];
    let outside = |value: i64, from: i64, to: i64| value < from || value >= to;
    let x = inset(left, right)
        .into_iter()
        .find(|&x| outside(x, toolbar_left, toolbar_right))?;
    let y = inset(top, bottom)
        .into_iter()
        .rev()
        .find(|&y| outside(y, toolbar_top, toolbar_bottom))?;
    Some(Point {
        x: i32::try_from(x).ok()?,
        y: i32::try_from(y).ok()?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const STAGE: [u8; 3] = [0xFF, 0x00, 0xFF];
    const PANEL: [u8; 3] = [0x2B, 0x2B, 0x2E];

    fn read(at_ms: u64, pixels: &[[u8; 3]]) -> Read {
        Read {
            at: Duration::from_millis(at_ms),
            pixels: pixels.to_vec(),
        }
    }

    #[test]
    fn the_toolbar_appears_at_the_first_read_that_is_no_longer_the_stage() {
        let reads = [
            read(0, &[STAGE, STAGE]),
            read(90, &[STAGE, [0xFE, 0x01, 0xFC]]),
            read(160, &[PANEL, PANEL]),
            read(170, &[PANEL, PANEL]),
            read(560, &[PANEL, [0x2D, 0x2A, 0x2E]]),
        ];
        assert_eq!(appeared_at(&reads, STAGE), Some(Duration::from_millis(160)));
    }

    #[test]
    fn a_fade_or_a_first_frame_of_another_color_counts_only_once_the_pixels_settle() {
        let reads = [
            read(0, &[STAGE]),
            read(120, &[[0, 0, 0]]),
            read(130, &[[0x90, 0x10, 0x90]]),
            read(140, &[[0x50, 0x20, 0x55]]),
            read(150, &[PANEL]),
            read(550, &[PANEL]),
        ];
        assert_eq!(appeared_at(&reads, STAGE), Some(Duration::from_millis(150)));
    }

    #[test]
    fn pixels_still_the_stage_at_the_end_mean_no_toolbar() {
        assert_eq!(
            appeared_at(&[read(0, &[STAGE]), read(3000, &[STAGE])], STAGE),
            None
        );
        assert_eq!(appeared_at(&[], STAGE), None);
        // One pixel the toolbar covers is enough.
        assert!(!is_stage(&[STAGE, PANEL], STAGE));
        assert!(is_stage(&[STAGE, [0xF0, 0x20, 0xF8]], STAGE));
    }

    fn rect(left: i32, top: i32, width: u32, height: u32) -> Rect {
        Rect {
            left,
            top,
            width,
            height,
        }
    }

    #[test]
    fn the_cursor_goes_on_a_row_and_a_column_the_toolbar_does_not_cross() {
        // A 4K monitor with the toolbar at the top center.
        let monitor = rect(0, 0, 3840, 2160);
        let top_center = rect(1500, 25, 840, 130);
        assert_eq!(
            cursor_spot(monitor, top_center),
            Some(Point { x: 480, y: 1889 })
        );
        // At the bottom left, the cursor goes to the other side, above the toolbar.
        let bottom_left = rect(25, 2005, 840, 130);
        assert_eq!(
            cursor_spot(monitor, bottom_left),
            Some(Point { x: 3359, y: 1889 })
        );
        // A monitor to the right of the first, at another height.
        let second = rect(3840, -200, 1920, 1080);
        assert_eq!(
            cursor_spot(second, rect(4380, -180, 840, 100)),
            Some(Point { x: 4080, y: 744 })
        );
    }

    #[test]
    fn a_toolbar_across_every_candidate_leaves_no_spot() {
        let monitor = rect(0, 0, 800, 600);
        assert_eq!(cursor_spot(monitor, rect(0, 250, 800, 100)), None);
        assert_eq!(cursor_spot(monitor, rect(10, 10, 780, 100)), None);
        assert_eq!(cursor_spot(monitor, rect(10, 10, 100, 580)), None);
    }
}
