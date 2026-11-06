//! What the frozen screen's windows draw over their photos, copied from the scene, and which windows draw again when
//! it changes.

use std::collections::BTreeSet;

use screengauge_core::session::{
    Cursor, DrawnRectangle, MagnifiedView, Measurement, Notice, SnipArea, ToleranceIndicator,
    Toolbar,
};

/// What the windows draw over their photos: the scene without the photos, copied.
#[derive(Debug, Clone, PartialEq, Default)]
pub(super) struct Shown {
    pub(super) live: Option<Measurement>,
    pub(super) drawn_rectangle: Option<DrawnRectangle>,
    pub(super) snip_area: Option<SnipArea>,
    pub(super) pinned: Vec<Measurement>,
    pub(super) magnified_view: Option<MagnifiedView>,
    pub(super) tolerance_indicator: Option<ToleranceIndicator>,
    pub(super) notice: Option<Notice>,
    /// `None` only when the scene couldn't be read.
    pub(super) toolbar: Option<Toolbar>,
    /// The cursor the scene names, which no window draws: Windows shows it. `None` only when the scene couldn't be
    /// read.
    pub(super) cursor: Option<Cursor>,
}

/// One window's part of what is shown: what is on its monitor.
#[derive(Debug, PartialEq)]
pub(super) struct OnMonitor<'a> {
    /// Oldest first.
    pub(super) pinned: Vec<&'a Measurement>,
    pub(super) live: Option<&'a Measurement>,
    pub(super) drawn_rectangle: Option<&'a DrawnRectangle>,
    pub(super) snip_area: Option<&'a SnipArea>,
    pub(super) magnified_view: Option<&'a MagnifiedView>,
    pub(super) tolerance_indicator: Option<&'a ToleranceIndicator>,
    pub(super) notice: Option<&'a Notice>,
    pub(super) toolbar: Option<&'a Toolbar>,
}

impl Shown {
    pub(super) fn on(&self, monitor: usize) -> OnMonitor<'_> {
        OnMonitor {
            pinned: self
                .pinned
                .iter()
                .filter(|measurement| measurement.monitor == monitor)
                .collect(),
            live: self
                .live
                .as_ref()
                .filter(|measurement| measurement.monitor == monitor),
            drawn_rectangle: self
                .drawn_rectangle
                .as_ref()
                .filter(|drawn| drawn.monitor == monitor),
            snip_area: self
                .snip_area
                .as_ref()
                .filter(|area| area.monitor == monitor),
            magnified_view: self
                .magnified_view
                .as_ref()
                .filter(|view| view.monitor == monitor),
            tolerance_indicator: self
                .tolerance_indicator
                .as_ref()
                .filter(|indicator| indicator.monitor == monitor),
            notice: self
                .notice
                .as_ref()
                .filter(|notice| notice.monitor == monitor),
            toolbar: self
                .toolbar
                .as_ref()
                .filter(|toolbar| toolbar.monitor == monitor),
        }
    }

    /// Every monitor something is drawn on.
    fn monitors(&self) -> impl Iterator<Item = usize> + '_ {
        let measured = self
            .pinned
            .iter()
            .chain(&self.live)
            .map(|measurement| measurement.monitor)
            .chain(self.drawn_rectangle.iter().map(|drawn| drawn.monitor))
            .chain(self.snip_area.iter().map(|area| area.monitor));
        let magnified = self.magnified_view.iter().map(|view| view.monitor);
        let indicated = self
            .tolerance_indicator
            .iter()
            .map(|indicator| indicator.monitor);
        measured
            .chain(magnified)
            .chain(indicated)
            .chain(self.notice.iter().map(|notice| notice.monitor))
            .chain(self.toolbar.iter().map(|toolbar| toolbar.monitor))
    }
}

/// The monitors whose windows show something different from `before` to `after`, each once and in order: the one a
/// measurement left, which must lose its line, the one it is on now, the one a measurement was pinned on or taken off,
/// the one Bounds or Snip draws a rectangle on while it appears, changes or goes, the one Snip's area appeared on,
/// changed on or went from, the ones Inspect's magnified view appeared on,
/// changed on, followed the cursor between or went from, the ones the tolerance indicator appeared on,
/// followed the cursor between or went from, the one a notice appeared on or went from, and the toolbar's when another
/// of its buttons lights up or it moves.
pub(super) fn monitors_to_draw_again(before: &Shown, after: &Shown) -> Vec<usize> {
    let monitors: BTreeSet<usize> = before.monitors().chain(after.monitors()).collect();
    monitors
        .into_iter()
        .filter(|&monitor| before.on(monitor) != after.on(monitor))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    use screengauge_core::session::{
        Label, Measured, Message, NoticeSide, Point, Rect, ToolbarButton,
    };
    use screengauge_core::settings::{DimensionsMode, Tolerance, Tool};

    /// A measurement on `monitor` whose label reads `text`, so two of them on one monitor differ.
    fn on(monitor: usize, text: &str) -> Measurement {
        Measurement {
            monitor,
            measured: Measured::Spans {
                horizontal: None,
                vertical: None,
            },
            label: Label {
                physical: text.to_owned(),
                logical: None,
            },
        }
    }

    fn live(measurement: Measurement) -> Shown {
        Shown {
            live: Some(measurement),
            ..Shown::default()
        }
    }

    fn copied_on(monitor: usize) -> Notice {
        Notice {
            message: Message::Copied,
            at: Point { x: 10, y: 10 },
            monitor,
            side: NoticeSide::Above,
        }
    }

    #[test]
    fn a_measurement_that_appears_or_goes_redraws_its_monitor() {
        let nothing = Shown::default();
        assert_eq!(monitors_to_draw_again(&nothing, &live(on(1, "24"))), [1]);
        assert_eq!(monitors_to_draw_again(&live(on(1, "24")), &nothing), [1]);
    }

    #[test]
    fn a_measurement_that_changes_on_one_monitor_redraws_it_once() {
        assert_eq!(
            monitors_to_draw_again(&live(on(0, "24")), &live(on(0, "25"))),
            [0]
        );
    }

    #[test]
    fn a_measurement_that_moves_to_another_monitor_redraws_both() {
        assert_eq!(
            monitors_to_draw_again(&live(on(0, "24")), &live(on(2, "24"))),
            [0, 2]
        );
    }

    fn drawing(monitor: usize, right: i32) -> Shown {
        Shown {
            drawn_rectangle: Some(DrawnRectangle {
                monitor,
                rect: Rect {
                    left: 100,
                    top: 100,
                    width: right.abs_diff(100) + 1,
                    height: 50,
                },
            }),
            ..Shown::default()
        }
    }

    #[test]
    fn a_drawn_rectangle_that_appears_grows_or_goes_redraws_its_monitor_alone() {
        // Over nothing but the reference color, Bounds measures nothing, and the drawn rectangle alone changes.
        let nothing = Shown::default();
        assert_eq!(monitors_to_draw_again(&nothing, &drawing(1, 300)), [1]);
        assert_eq!(
            monitors_to_draw_again(&drawing(1, 300), &drawing(1, 301)),
            [1]
        );
        assert_eq!(monitors_to_draw_again(&drawing(1, 301), &nothing), [1]);
        assert_eq!(
            monitors_to_draw_again(&drawing(1, 301), &drawing(1, 301)),
            Vec::<usize>::new()
        );
    }

    fn snipping(monitor: usize, right: i32) -> Shown {
        let width = right.abs_diff(100) + 1;
        Shown {
            snip_area: Some(SnipArea {
                monitor,
                rect: Rect {
                    left: 100,
                    top: 100,
                    width,
                    height: 50,
                },
                label: Label {
                    physical: format!("{width} × 50"),
                    logical: None,
                },
            }),
            ..Shown::default()
        }
    }

    #[test]
    fn a_snip_area_that_appears_grows_or_goes_redraws_its_monitor_alone() {
        // With Alt, Snip shows its area with no drawn rectangle under it, so the area alone changes.
        let nothing = Shown::default();
        assert_eq!(monitors_to_draw_again(&nothing, &snipping(1, 300)), [1]);
        assert_eq!(
            monitors_to_draw_again(&snipping(1, 300), &snipping(1, 301)),
            [1]
        );
        assert_eq!(monitors_to_draw_again(&snipping(1, 301), &nothing), [1]);
        assert_eq!(
            monitors_to_draw_again(&snipping(1, 301), &snipping(1, 301)),
            Vec::<usize>::new()
        );
    }

    #[test]
    fn the_same_measurement_or_none_redraws_nothing() {
        let same = live(on(0, "24"));
        assert_eq!(monitors_to_draw_again(&same, &same), Vec::<usize>::new());
        let nothing = Shown::default();
        assert_eq!(
            monitors_to_draw_again(&nothing, &nothing),
            Vec::<usize>::new()
        );
    }

    #[test]
    fn pinning_the_live_measurement_redraws_its_monitor_though_the_live_one_stays() {
        let before = live(on(0, "24"));
        let after = Shown {
            pinned: vec![on(0, "24")],
            ..before.clone()
        };
        assert_eq!(monitors_to_draw_again(&before, &after), [0]);
    }

    #[test]
    fn taking_off_a_pin_on_another_monitor_redraws_that_monitor_alone() {
        let before = Shown {
            pinned: vec![on(1, "180")],
            ..live(on(0, "24"))
        };
        let after = live(on(0, "24"));
        assert_eq!(monitors_to_draw_again(&before, &after), [1]);
    }

    /// A magnified view on `monitor` around the pixel `(x, 100)`, with its magnification indicator showing when
    /// `magnification_indicator`.
    fn magnified_on(monitor: usize, x: i32, magnification_indicator: bool) -> Shown {
        let square = |left: i32, top: i32, side: u32| Rect {
            left,
            top,
            width: side,
            height: side,
        };
        let coordinate = |text: &str, line: Rect| screengauge_core::session::Coordinate {
            label: Label {
                physical: text.to_owned(),
                logical: None,
            },
            line,
        };
        Shown {
            magnified_view: Some(MagnifiedView {
                monitor,
                panel: square(x + 22, 122, 200),
                enlargement: square(x + 29, 129, 168),
                source: square(x - 10, 90, 21),
                shown: square(x - 10, 90, 21),
                center: Point { x, y: 100 },
                magnification: screengauge_core::settings::Magnification::X8,
                grid: true,
                magnification_indicator,
                x: coordinate(&x.to_string(), square(x + 33, 304, 16)),
                y: coordinate("100", square(x + 120, 304, 16)),
                color: screengauge_core::session::PixelColor {
                    rgb: [255, 255, 255],
                    swatch: square(x + 33, 325, 22),
                    first: screengauge_core::session::ColorText {
                        text: "#FFFFFF".to_owned(),
                        line: square(x + 63, 328, 16),
                    },
                    second: None,
                },
                approximate_color: None,
            }),
            ..Shown::default()
        }
    }

    #[test]
    fn a_magnified_view_that_appears_moves_or_goes_redraws_its_monitor() {
        let nothing = Shown::default();
        assert_eq!(
            monitors_to_draw_again(&nothing, &magnified_on(1, 500, false)),
            [1]
        );
        assert_eq!(
            monitors_to_draw_again(&magnified_on(1, 500, false), &magnified_on(1, 501, false)),
            [1]
        );
        assert_eq!(
            monitors_to_draw_again(&magnified_on(1, 501, false), &nothing),
            [1]
        );
        assert_eq!(
            monitors_to_draw_again(&magnified_on(1, 501, false), &magnified_on(1, 501, false)),
            Vec::<usize>::new()
        );
    }

    #[test]
    fn the_magnification_indicator_that_goes_redraws_the_views_monitor() {
        assert_eq!(
            monitors_to_draw_again(&magnified_on(0, 500, true), &magnified_on(0, 500, false)),
            [0]
        );
    }

    #[test]
    fn a_magnified_view_that_follows_the_cursor_to_another_monitor_redraws_both() {
        assert_eq!(
            monitors_to_draw_again(&magnified_on(0, 500, false), &magnified_on(2, 500, false)),
            [0, 2]
        );
    }

    fn tolerance_on(monitor: usize, value: u8) -> ToleranceIndicator {
        ToleranceIndicator {
            tolerance: Tolerance::new(value).expect("within the limits"),
            at: Point { x: 10, y: 10 },
            monitor,
        }
    }

    #[test]
    fn the_tolerance_indicator_that_appears_changes_or_goes_redraws_its_monitor() {
        let quiet = live(on(0, "24"));
        let indicated = Shown {
            tolerance_indicator: Some(tolerance_on(0, 3)),
            ..quiet.clone()
        };
        let raised = Shown {
            tolerance_indicator: Some(tolerance_on(0, 4)),
            ..quiet.clone()
        };
        assert_eq!(monitors_to_draw_again(&quiet, &indicated), [0]);
        assert_eq!(monitors_to_draw_again(&indicated, &raised), [0]);
        assert_eq!(monitors_to_draw_again(&indicated, &quiet), [0]);
    }

    #[test]
    fn the_tolerance_indicator_that_follows_the_cursor_to_another_monitor_redraws_both() {
        let here = Shown {
            tolerance_indicator: Some(tolerance_on(0, 3)),
            ..Shown::default()
        };
        let there = Shown {
            tolerance_indicator: Some(tolerance_on(1, 3)),
            ..Shown::default()
        };
        assert_eq!(monitors_to_draw_again(&here, &there), [0, 1]);
    }

    fn toolbar_on(monitor: usize, active: Tool, mode: Option<DimensionsMode>) -> Shown {
        Shown {
            toolbar: Some(Toolbar {
                monitor,
                corner: Point {
                    x: 1920 + 700,
                    y: 14,
                },
                active,
                mode,
                minimized: false,
                hovered: None,
            }),
            ..Shown::default()
        }
    }

    #[test]
    fn another_tool_or_mode_lit_redraws_the_toolbars_monitor_alone() {
        let dimensions = toolbar_on(1, Tool::Dimensions, Some(DimensionsMode::Cross));
        let bounds = toolbar_on(1, Tool::Bounds, None);
        let horizontal = toolbar_on(1, Tool::Dimensions, Some(DimensionsMode::Horizontal));
        assert_eq!(monitors_to_draw_again(&dimensions, &bounds), [1]);
        assert_eq!(monitors_to_draw_again(&dimensions, &horizontal), [1]);
        assert_eq!(
            monitors_to_draw_again(&dimensions, &dimensions),
            Vec::<usize>::new()
        );
    }

    /// `shown` with its toolbar changed by `change`.
    fn with_toolbar(shown: &Shown, change: impl FnOnce(&mut Toolbar)) -> Shown {
        let mut changed = shown.clone();
        if let Some(toolbar) = changed.toolbar.as_mut() {
            change(toolbar);
        }
        changed
    }

    #[test]
    fn the_cursor_onto_another_button_or_minimizing_redraws_the_toolbars_monitor_and_the_cursor_alone_nothing()
     {
        let rest = toolbar_on(1, Tool::Dimensions, None);
        let on_close = with_toolbar(&rest, |toolbar| {
            toolbar.hovered = Some(ToolbarButton::Close);
        });
        let on_gear = with_toolbar(&rest, |toolbar| {
            toolbar.hovered = Some(ToolbarButton::Settings);
        });
        let small = with_toolbar(&rest, |toolbar| toolbar.minimized = true);
        assert_eq!(monitors_to_draw_again(&rest, &on_close), [1]);
        assert_eq!(monitors_to_draw_again(&on_close, &on_gear), [1]);
        assert_eq!(monitors_to_draw_again(&rest, &small), [1]);
        // The cursor's shape is Windows' to show: it changing alone draws nothing again.
        let arrow = Shown {
            cursor: Some(Cursor::Arrow),
            ..rest.clone()
        };
        assert_eq!(monitors_to_draw_again(&rest, &arrow), Vec::<usize>::new());
    }

    #[test]
    fn a_dragged_toolbar_redraws_its_monitor_alone() {
        let here = toolbar_on(1, Tool::Dimensions, None);
        let lower = with_toolbar(&here, |toolbar| toolbar.corner.y += 300);
        assert_eq!(monitors_to_draw_again(&here, &lower), [1]);
    }

    #[test]
    fn a_measurement_moving_on_another_monitor_leaves_the_toolbars_alone() {
        let toolbar = toolbar_on(0, Tool::Dimensions, Some(DimensionsMode::Cross));
        let before = Shown {
            live: Some(on(1, "24")),
            ..toolbar.clone()
        };
        let after = Shown {
            live: Some(on(1, "25")),
            ..toolbar
        };
        assert_eq!(monitors_to_draw_again(&before, &after), [1]);
    }

    #[test]
    fn a_notice_that_appears_or_goes_redraws_its_monitor() {
        let quiet = live(on(0, "24"));
        let noticed = Shown {
            notice: Some(copied_on(1)),
            ..quiet.clone()
        };
        assert_eq!(monitors_to_draw_again(&quiet, &noticed), [1]);
        assert_eq!(monitors_to_draw_again(&noticed, &quiet), [1]);
    }
}
