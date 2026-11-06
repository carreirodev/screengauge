//! Where the tray icon's menu goes and what is where inside it, in physical pixels at the scale of the monitor it opens
//! on. The items are a list, one under the other, so another item adds a row without moving the rest.

use windows::Win32::Foundation::{POINT, RECT, SIZE};

use crate::relief::scaled;

/// The menu's width, in logical pixels: room for the longest label, "Start with Windows", after the check marks'
/// column.
pub const WIDTH: u32 = 180;
/// The margin between the menu's edge and its rows, on every side.
pub const MARGIN: u32 = 4;
/// Each row's height.
pub const ROW: u32 = 30;
/// How far in from its row's left side a label starts: after the column where "Start with Windows" has its check mark,
/// as Windows' own menus leave one for every item when any has a mark (the owner's choice in WPC-326).
pub const LABEL_INSET: u32 = 30;
/// The middle of the check marks' column, from the row's left side, and the mark's width and height and its stroke.
pub const CHECK_CENTER: u32 = 15;
pub const CHECK_WIDTH: f32 = 10.0;
pub const CHECK_HEIGHT: f32 = 7.0;
pub const CHECK_STROKE: f32 = 1.5;
/// The labels' size, as Settings' rows'.
pub const LABEL_TEXT: f32 = 13.0;
/// The radius of a lit row's corners, half the window's, which Windows rounds as its own menus' (`DWMWCP_ROUND`).
pub const ROW_CORNER: u32 = 4;

/// What the menu offers, in its order from the top.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Item {
    /// Opens Settings.
    Settings,
    /// Turns "Start with Windows" on or off, with a check mark while it is on (WPC-326).
    StartWithWindows,
    /// Exits ScreenGauge.
    Exit,
}

pub const ITEMS: [Item; 3] = [Item::Settings, Item::StartWithWindows, Item::Exit];

impl Item {
    pub fn label(self) -> &'static str {
        match self {
            Item::Settings => "Settings",
            Item::StartWithWindows => "Start with Windows",
            Item::Exit => "Exit",
        }
    }

    /// Its place in `ITEMS`, from 0 at the top.
    pub fn index(self) -> usize {
        match self {
            Item::Settings => 0,
            Item::StartWithWindows => 1,
            Item::Exit => 2,
        }
    }
}

/// Which way an arrow key moves through the items.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Up,
    Down,
}

/// The menu's size at `dpi`.
pub fn size_at(dpi: u32) -> SIZE {
    SIZE {
        cx: scaled(WIDTH, dpi),
        cy: scaled(bottom_of(ITEMS.len()) + MARGIN, dpi),
    }
}

/// The bottom of the first `rows` rows, in logical pixels from the menu's top.
fn bottom_of(rows: usize) -> u32 {
    // A handful of rows: saturating only keeps the arithmetic total.
    ROW.saturating_mul(u32::try_from(rows).unwrap_or(u32::MAX))
        .saturating_add(MARGIN)
}

/// Where `item`'s row is, in the menu's pixels at `dpi`. Each side is scaled on its own, so two rows meet on the same
/// pixel whatever the scale.
pub fn item_box(item: Item, dpi: u32) -> RECT {
    let index = item.index();
    RECT {
        left: scaled(MARGIN, dpi),
        top: scaled(bottom_of(index), dpi),
        right: scaled(WIDTH - MARGIN, dpi),
        bottom: scaled(bottom_of(index + 1), dpi),
    }
}

/// The item whose row holds `point`, in the menu's pixels at `dpi`; `None` in the margin and outside the menu.
pub fn item_at(point: POINT, dpi: u32) -> Option<Item> {
    ITEMS.into_iter().find(|&item| {
        let place = item_box(item, dpi);
        (place.left..place.right).contains(&point.x) && (place.top..place.bottom).contains(&point.y)
    })
}

/// The item an arrow key lights after `lit`: the next one down or up, round from one end to the other, and with none
/// lit, the first one down or the last one up.
pub fn next(lit: Option<Item>, direction: Direction) -> Item {
    let count = ITEMS.len();
    let index = match (lit, direction) {
        (None, Direction::Down) => 0,
        (None, Direction::Up) => count - 1,
        (Some(item), Direction::Down) => (item.index() + 1) % count,
        (Some(item), Direction::Up) => (item.index() + count - 1) % count,
    };
    ITEMS[index]
}

/// Where the top left corner of a menu of `size` goes for a click at `anchor`, inside `work`, the work area of the
/// click's monitor, which leaves the taskbar out. As Windows places a tray icon's menu: its bottom left corner at the
/// click, left of the click instead where the right side has no room, and never over the taskbar, so a click on the
/// taskbar puts it just above. With no room above, as under a taskbar at the top, it goes below the click.
pub fn placed(anchor: POINT, size: SIZE, work: RECT) -> POINT {
    let mut x = anchor.x;
    if x + size.cx > work.right {
        x = anchor.x - size.cx;
    }
    let mut y = anchor.y.min(work.bottom) - size.cy;
    if y < work.top {
        y = anchor.y;
    }
    POINT {
        x: x.min(work.right - size.cx).max(work.left),
        y: y.min(work.bottom - size.cy).max(work.top),
    }
}

#[cfg(test)]
mod tests {
    use windows::Win32::Foundation::{POINT, RECT, SIZE};

    use super::*;

    /// The owner's 4K monitor at 175%, its taskbar 72 pixels tall at the bottom.
    const UHD: RECT = RECT {
        left: 0,
        top: 0,
        right: 3840,
        bottom: 2160 - 84,
    };

    /// The menu at 175%.
    const MENU: SIZE = SIZE { cx: 315, cy: 172 };

    fn at(x: i32, y: i32) -> POINT {
        POINT { x, y }
    }

    #[test]
    fn opened_from_the_taskbar_by_the_clock_it_sits_on_the_taskbar_left_of_the_click() {
        // Too near the right side for the menu to go right of the click: it goes left of it, above the taskbar.
        assert_eq!(
            placed(at(3700, 2120), MENU, UHD),
            at(3700 - 315, 2076 - 172)
        );
    }

    #[test]
    fn opened_from_the_hidden_icons_panel_its_bottom_left_corner_is_at_the_click() {
        assert_eq!(placed(at(3300, 1900), MENU, UHD), at(3300, 1900 - 172));
    }

    #[test]
    fn on_a_monitor_left_of_the_main_one_it_stays_on_that_monitor() {
        // The owner's Full HD at 125%, left of the 4K, with its own taskbar 60 pixels tall.
        let left = RECT {
            left: -1920,
            top: 0,
            right: 0,
            bottom: 1080 - 60,
        };
        assert_eq!(
            placed(at(-150, 1050), MENU, left),
            at(-150 - 315, 1020 - 172)
        );
        assert_eq!(placed(at(-1900, 1050), MENU, left), at(-1900, 1020 - 172));
    }

    #[test]
    fn under_a_taskbar_at_the_top_it_opens_below_the_click() {
        let below_taskbar = RECT {
            left: 0,
            top: 84,
            right: 3840,
            bottom: 2160,
        };
        assert_eq!(
            placed(at(3700, 40), MENU, below_taskbar),
            at(3700 - 315, 84)
        );
    }

    #[test]
    fn a_menu_larger_than_the_room_either_side_of_the_click_stays_inside_the_monitor() {
        let narrow = RECT {
            left: 0,
            top: 0,
            right: 400,
            bottom: 1000,
        };
        // No room right of the click: it goes left of it, as far as the monitor's left side.
        assert_eq!(placed(at(200, 900), MENU, narrow), at(0, 900 - 172));
    }

    #[test]
    fn the_menu_is_180_by_98_logical_pixels_at_every_scale() {
        for (dpi, size) in [(96, (180, 98)), (144, (270, 147)), (168, (315, 172))] {
            let SIZE { cx, cy } = size_at(dpi);
            assert_eq!((cx, cy), size, "{dpi} DPI");
        }
    }

    #[test]
    fn settings_comes_first_then_start_with_windows_then_exit() {
        assert_eq!(ITEMS, [Item::Settings, Item::StartWithWindows, Item::Exit]);
        for (index, item) in ITEMS.into_iter().enumerate() {
            assert_eq!(item.index(), index, "{item:?}");
        }
        assert_eq!(Item::Settings.label(), "Settings");
        assert_eq!(Item::StartWithWindows.label(), "Start with Windows");
        assert_eq!(Item::Exit.label(), "Exit");
    }

    #[test]
    fn each_item_is_a_row_30_logical_pixels_tall_inside_a_4_pixel_margin() {
        // At 175%, the 4 logical pixels are 7, and 30 are 52.5, so the rows meet at 60 and 112 with no gap or overlap.
        assert_eq!(
            item_box(Item::Settings, 168),
            RECT {
                left: 7,
                top: 7,
                right: 315 - 7,
                bottom: 60
            }
        );
        assert_eq!(
            item_box(Item::StartWithWindows, 168),
            RECT {
                left: 7,
                top: 60,
                right: 315 - 7,
                bottom: 112
            }
        );
        assert_eq!(
            item_box(Item::Exit, 168),
            RECT {
                left: 7,
                top: 112,
                right: 315 - 7,
                bottom: 172 - 7
            }
        );
    }

    #[test]
    fn the_item_under_the_cursor_is_the_row_it_is_on_and_the_margin_is_none() {
        assert_eq!(item_at(at(150, 30), 168), Some(Item::Settings));
        assert_eq!(item_at(at(150, 59), 168), Some(Item::Settings));
        assert_eq!(item_at(at(150, 60), 168), Some(Item::StartWithWindows));
        assert_eq!(item_at(at(150, 111), 168), Some(Item::StartWithWindows));
        assert_eq!(item_at(at(150, 112), 168), Some(Item::Exit));
        for margin in [at(150, 3), at(3, 30), at(312, 30), at(150, 168)] {
            assert_eq!(item_at(margin, 168), None, "{margin:?}");
        }
    }

    #[test]
    fn the_arrow_keys_go_through_the_items_and_round_from_one_end_to_the_other() {
        assert_eq!(next(None, Direction::Down), Item::Settings);
        assert_eq!(
            next(Some(Item::Settings), Direction::Down),
            Item::StartWithWindows
        );
        assert_eq!(
            next(Some(Item::StartWithWindows), Direction::Down),
            Item::Exit
        );
        assert_eq!(next(Some(Item::Exit), Direction::Down), Item::Settings);
        assert_eq!(next(None, Direction::Up), Item::Exit);
        assert_eq!(
            next(Some(Item::Exit), Direction::Up),
            Item::StartWithWindows
        );
        assert_eq!(next(Some(Item::Settings), Direction::Up), Item::Exit);
    }
}
