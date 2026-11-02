//! The tray icon's menu drawn onto any Direct2D render target at 96 DPI, so a test can render it on a bitmap and look at
//! its pixels. Its looks come from Settings' (WPC-279's `#settings` tokens): the window's background, its rows' labels,
//! and its cards' edge for the border Windows draws and for the row under the mouse or the arrow keys.

use windows::Win32::Foundation::E_FAIL;
use windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F;
use windows::Win32::Graphics::Direct2D::{
    D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_ROUNDED_RECT, ID2D1RenderTarget,
};
use windows::Win32::Graphics::DirectWrite::{DWRITE_FONT_WEIGHT_NORMAL, DWRITE_LINE_METRICS};
use windows::core::Error;

use super::layout::{self, Item};
use crate::graphics::{Fonts, over, solid_brush};
use crate::relief::{px, scaled};
use crate::settings_window::{background, dpi_scale, palette, write};
use crate::theme::Mode;

/// Draws the menu at `dpi` in `mode`'s palette, with `lit`'s row tinted.
pub fn draw(
    target: &ID2D1RenderTarget,
    fonts: &Fonts,
    mode: Mode,
    dpi: u32,
    lit: Option<Item>,
) -> windows::core::Result<()> {
    let colors = palette(mode);
    let size = layout::size_at(dpi);
    background(target, colors, px(size.cx), px(size.cy))?;
    // SAFETY: a setting on a live render target.
    unsafe { target.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE) };
    if let Some(item) = lit {
        let row = layout::item_box(item, dpi);
        let radius = px(scaled(layout::ROW_CORNER, dpi));
        let shape = D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F {
                left: px(row.left),
                top: px(row.top),
                right: px(row.right),
                bottom: px(row.bottom),
            },
            radiusX: radius,
            radiusY: radius,
        };
        let tint = solid_brush(target, colors.edge)?;
        // SAFETY: a drawing call on a live render target, with arguments that outlive it.
        unsafe { target.FillRoundedRectangle(&shape, &tint) };
    }
    let format = fonts.text_format(
        DWRITE_FONT_WEIGHT_NORMAL,
        layout::LABEL_TEXT * dpi_scale(dpi),
    )?;
    let ink = solid_brush(target, colors.ink)?;
    for item in layout::ITEMS {
        let row = layout::item_box(item, dpi);
        let text: Vec<u16> = item.label().encode_utf16().collect();
        // SAFETY: the layout copies the text and keeps its own reference to the format. A width and height past any
        // label keep it on one line.
        let label = unsafe {
            fonts
                .factory
                .CreateTextLayout(&text, &format, 4096.0, 4096.0)
        }?;
        let mut metrics = [DWRITE_LINE_METRICS::default()];
        let mut lines = 0;
        // SAFETY: fills a one-line buffer, and fails should the label need more.
        unsafe { label.GetLineMetrics(Some(&mut metrics), &mut lines) }?;
        let [metrics] = metrics;
        if lines != 1 {
            return Err(Error::new(E_FAIL, "a menu label took more than one line"));
        }
        // Centered in its row, with the baseline on a whole pixel so the letters stay crisp, as Settings' labels.
        let baseline =
            (px(row.top) + (px(row.bottom - row.top) - metrics.height) / 2.0 + metrics.baseline)
                .round();
        let left = px(row.left + scaled(layout::LABEL_INSET, dpi));
        write(target, &label, left, baseline - metrics.baseline, &ink);
    }
    Ok(())
}

/// The color Windows draws the window's 1-pixel border in: Settings' cards' edge over the menu's top.
pub fn border(mode: Mode) -> [u8; 3] {
    let colors = palette(mode);
    over(colors.edge, colors.top.rgb)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graphics::Fonts;
    use crate::graphics::over;
    use crate::relief::scaled;
    use crate::settings_window::palette;
    use crate::test_bitmap::{Picture, drawn, near};

    fn menu(mode: Mode, dpi: u32, lit: Option<Item>) -> Picture {
        let size = layout::size_at(dpi);
        let width = u32::try_from(size.cx).expect("a small menu");
        let height = u32::try_from(size.cy).expect("a small menu");
        drawn(width, height, |target, _| {
            let fonts = Fonts::new().expect("the fonts");
            draw(target, &fonts, mode, dpi, lit).expect("the menu is drawn");
        })
    }

    /// The owner's monitors' scales, and 100%.
    const DPIS: [u32; 3] = [96, 120, 168];

    #[test]
    fn the_background_is_settings_window_from_its_top_color_to_its_bottom_one() {
        for mode in [Mode::Dark, Mode::Light] {
            for dpi in DPIS {
                let picture = menu(mode, dpi, None);
                let bottom = i32::try_from(picture.height).expect("small") - 1;
                let colors = palette(mode);
                assert!(near(picture.at(1, 0), colors.top.rgb, 2), "{mode:?} {dpi}");
                assert!(
                    near(picture.at(1, bottom), colors.bottom.rgb, 2),
                    "{mode:?} {dpi}"
                );
            }
        }
    }

    #[test]
    fn the_lit_row_is_tinted_with_settings_edge_color_and_the_others_are_not() {
        for mode in [Mode::Dark, Mode::Light] {
            for dpi in DPIS {
                let picture = menu(mode, dpi, Some(Item::Exit));
                let colors = palette(mode);
                for item in layout::ITEMS {
                    let row = layout::item_box(item, dpi);
                    // Inside the row's rounded corners, left of its label.
                    let (x, y) = (row.left + 3, (row.top + row.bottom) / 2);
                    let background = picture.at(1, y);
                    let want = if item == Item::Exit {
                        over(colors.edge, background)
                    } else {
                        background
                    };
                    assert!(
                        near(picture.at(x, y), want, 2),
                        "{mode:?} {dpi} {item:?}: {:?} for {want:?}",
                        picture.at(x, y)
                    );
                }
            }
        }
    }

    /// The columns of `item`'s row where its label's ink is: pixels far from the row's own color.
    fn label_columns(picture: &Picture, item: Item, dpi: u32) -> Option<(i32, i32)> {
        let row = layout::item_box(item, dpi);
        let background = picture.at(row.left + 1, (row.top + row.bottom) / 2);
        let inked = |x: i32| {
            (row.top..row.bottom).any(|y| {
                let pixel = picture.at(x, y);
                pixel
                    .iter()
                    .zip(background)
                    .map(|(&have, want)| u32::from(have.abs_diff(want)))
                    .sum::<u32>()
                    > 150
            })
        };
        let columns: Vec<i32> = (row.left..row.right).filter(|&x| inked(x)).collect();
        Some((*columns.first()?, *columns.last()?))
    }

    #[test]
    fn each_label_starts_12_logical_pixels_into_its_row() {
        for mode in [Mode::Dark, Mode::Light] {
            for dpi in DPIS {
                let picture = menu(mode, dpi, Some(Item::Settings));
                for item in layout::ITEMS {
                    let row = layout::item_box(item, dpi);
                    let (first, last) = label_columns(&picture, item, dpi)
                        .unwrap_or_else(|| panic!("{mode:?} {dpi} {item:?} has no label"));
                    let start = row.left + scaled(layout::LABEL_INSET, dpi);
                    // A letter's side bearing leaves a pixel or two before its ink.
                    assert!(
                        (start..=start + scaled(2, dpi)).contains(&first),
                        "{mode:?} {dpi} {item:?}: ink from {first}, label at {start}"
                    );
                    assert!(last < row.right - scaled(layout::LABEL_INSET, dpi));
                }
            }
        }
    }

    #[test]
    fn the_labels_are_settings_ink() {
        for mode in [Mode::Dark, Mode::Light] {
            let picture = menu(mode, 168, None);
            let ink = palette(mode).ink.rgb;
            let row = layout::item_box(Item::Settings, 168);
            let darkest_or_brightest = (row.left..row.right)
                .flat_map(|x| (row.top..row.bottom).map(move |y| (x, y)))
                .any(|(x, y)| near(picture.at(x, y), ink, 12));
            assert!(
                darkest_or_brightest,
                "{mode:?}: no pixel of the ink's color"
            );
        }
    }

    #[test]
    fn windows_draws_the_border_in_settings_edge_color_over_the_menus_top() {
        // --card-edge over --win-bg's top: white at 7% over #25272c, and rgb(70 60 45) at 13% over #f8f7f4.
        assert_eq!(border(Mode::Dark), [52, 54, 59]);
        assert_eq!(border(Mode::Light), [225, 223, 218]);
    }

    /// Writes the menu in both modes and at the owner's scales to target\tray-menu, for a look at it.
    #[test]
    #[ignore = "writes PNG files for a person to look at"]
    fn write_the_menus_for_a_look() {
        let folder =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/tray-menu");
        std::fs::create_dir_all(&folder).expect("the folder");
        for mode in [Mode::Dark, Mode::Light] {
            for dpi in [96, 120, 144, 168] {
                for (name, lit) in [("rest", None), ("settings-lit", Some(Item::Settings))] {
                    menu(mode, dpi, lit)
                        .write_png(&folder.join(format!("{mode:?}-{dpi}-{name}.png")));
                }
            }
        }
    }
}
