//! The footer drawn on a bitmap (WPC-319, WPC-335): its texts on the left, "Project page" and "Check for updates" on
//! the right, and the line under them that says what "Check for updates" found.

use super::*;
use crate::update_check::{Answer, Version};

/// Every line "Check for updates" shows, in the order a click goes through them.
fn statuses() -> [Status; 4] {
    [
        Status::Checking,
        Status::Answered(Answer::Latest),
        Status::Answered(Answer::Available(
            Version::parse("1.1.0").expect("a version"),
        )),
        Status::Answered(Answer::CouldntCheck),
    ]
}

/// The page with the default options, the mouse away, and `status`' line under the footer.
fn showing(mode: Mode, dpi: u32, status: Status) -> Picture {
    let options = Options::default();
    let mut shown = resting(&options);
    shown.update = Some(status);
    drawn_showing(mode, dpi, &options, Pointer::default(), &shown)
}

/// The rows of the line under the footer, in pixels at `dpi`: from where it starts to the bottom of the page.
fn line_rows(dpi: u32) -> (i32, i32) {
    let footer = layout::with_status(layout::page(&[]), true).footer;
    (
        whole_px(layout::status_line(footer), dpi),
        whole_px(footer.bottom, dpi),
    )
}

/// The leftmost and the rightmost column, in `rows`, of a pixel within `within` of `color`.
fn columns_of(page: &Picture, color: Color, within: u8, rows: (i32, i32)) -> Option<(i32, i32)> {
    let width = i32::try_from(page.width).expect("a small page");
    let found: Vec<i32> = (rows.0..rows.1)
        .flat_map(|y| (0..width).map(move |x| (x, y)))
        .filter(|&(x, y)| near(page.at(x, y), color, within))
        .map(|(x, _)| x)
        .collect();
    Some((*found.iter().min()?, *found.iter().max()?))
}

/// The leftmost and the rightmost column, in `rows`, of a pixel that stands out from the background at the page's left
/// side on its row: anything drawn there, the faintest edge of a letter included.
fn drawn_columns(page: &Picture, rows: (i32, i32)) -> Option<(i32, i32)> {
    let width = i32::try_from(page.width).expect("a small page");
    let found: Vec<i32> = (rows.0..rows.1)
        .flat_map(|y| (0..width).map(move |x| (x, y)))
        .filter(|&(x, y)| (brightness(page.at(x, y)) - brightness(page.at(2, y))).abs() > 30)
        .map(|(x, _)| x)
        .collect();
    Some((*found.iter().min()?, *found.iter().max()?))
}

/// Whether a row between `top` and `bottom` is `color` all along `place`'s width but a pixel at each end: an underline.
fn underlined(page: &Picture, place: RECT, top: i32, bottom: i32, color: Color) -> bool {
    (top..bottom).any(|y| (place.left + 1..place.right - 1).all(|x| near(page.at(x, y), color, 2)))
}

#[test]
fn before_any_click_the_footer_has_its_two_links_and_no_line_under_them() {
    for dpi in DPIS {
        let page = drawn(Mode::Dark, dpi);
        let links: Vec<Link> = page.links.iter().map(|&(link, _)| link).collect();
        assert_eq!(links, [Link::ProjectPage, Link::CheckForUpdates], "{dpi}");
        assert_eq!(page.height, scaled(layout::page(&[]).height, dpi), "{dpi}");
    }
}

#[test]
fn check_for_updates_ends_the_footer_on_the_right_after_project_page_both_underlined_links() {
    for mode in [Mode::Dark, Mode::Light] {
        for dpi in DPIS {
            let page = drawn(mode, dpi);
            let (check, project) = (
                page.link(Link::CheckForUpdates),
                page.link(Link::ProjectPage),
            );
            let footer = layout::page(&[]).footer;
            // It ends 16 logical pixels in from the right, inside the footer.
            let right = whole_px(layout::WIDTH - layout::SIDE, dpi);
            assert!(
                (right - 1..=right).contains(&check.right),
                "{mode:?} {dpi}: {check:?}"
            );
            // "Project page", a dot and the footer's gap either side of it, then "Check for updates", on one line.
            assert!(
                project.right < check.left,
                "{mode:?} {dpi}: {project:?} {check:?}"
            );
            assert!(project.left > right / 2, "{mode:?} {dpi}: {project:?}");
            assert_eq!((project.top, project.bottom), (check.top, check.bottom));
            let (top, bottom) = (whole_px(footer.top, dpi), whole_px(footer.bottom, dpi));
            for link in [project, check] {
                assert!(
                    top < link.top && link.bottom < bottom,
                    "{mode:?} {dpi}: {link:?}"
                );
                assert!(
                    underlined(&page, link, link.top, bottom, palette(mode).link),
                    "{mode:?} {dpi}: no underline in {link:?}"
                );
            }
            // The dot between them is in the footer's dim color.
            let between = (project.right + 1..check.left - 1).any(|x| {
                (check.top..check.bottom).any(|y| near(page.at(x, y), palette(mode).dim, 24))
            });
            assert!(between, "{mode:?} {dpi}: no dot between the links");
        }
    }
}

#[test]
fn the_footer_s_texts_on_the_left_are_drawn_in_the_dim_color() {
    for mode in [Mode::Dark, Mode::Light] {
        let page = drawn(mode, 144);
        let link = page.link(Link::ProjectPage);
        let texts = (link.top..link.bottom)
            .any(|y| (24..420).any(|x| near(page.at(x, y), palette(mode).dim, 24)));
        assert!(texts, "{mode:?}");
    }
}

#[test]
fn every_line_shows_under_the_links_at_the_right_and_grows_the_page_by_24() {
    for mode in [Mode::Dark, Mode::Light] {
        for dpi in DPIS {
            for status in statuses() {
                let page = showing(mode, dpi, status);
                assert_eq!(
                    page.height,
                    scaled(layout::page(&[]).height + 24, dpi),
                    "{mode:?} {dpi} {status:?}"
                );
                let rows = line_rows(dpi);
                // Under the footer's links, which stay where they were.
                assert_eq!(
                    page.link(Link::CheckForUpdates),
                    drawn(mode, dpi).link(Link::CheckForUpdates),
                    "{mode:?} {dpi} {status:?}"
                );
                assert!(
                    page.link(Link::CheckForUpdates).bottom <= rows.0,
                    "{mode:?} {dpi} {status:?}"
                );
                // Its words are in the rows' ink. What it draws ends at the footer's right side, and starts right of
                // the middle, as `justify-content: flex-end` puts it.
                assert!(
                    columns_of(&page, palette(mode).ink, 24, rows).is_some(),
                    "{mode:?} {dpi} {status:?}: no words"
                );
                let (left, right) = drawn_columns(&page, rows)
                    .unwrap_or_else(|| panic!("{mode:?} {dpi} {status:?}: nothing drawn"));
                let side = whole_px(layout::WIDTH - layout::SIDE, dpi);
                assert!(
                    (side - 3..side).contains(&right),
                    "{mode:?} {dpi} {status:?}: ends at {right}"
                );
                assert!(
                    left > side / 3,
                    "{mode:?} {dpi} {status:?}: starts at {left}"
                );
            }
        }
    }
}

#[test]
fn the_latest_version_has_the_accent_s_check_mark_and_couldn_t_check_the_warning_s_sign() {
    for mode in [Mode::Dark, Mode::Light] {
        for dpi in DPIS {
            let rows = line_rows(dpi);
            let accent = recorder_field::accent(mode);
            let warn = row_message::warn(mode);
            for (status, with_accent, with_warn) in [
                (Status::Checking, false, false),
                (Status::Answered(Answer::Latest), true, false),
                (Status::Answered(Answer::CouldntCheck), false, true),
            ] {
                let page = showing(mode, dpi, status);
                let sign = columns_of(&page, accent, 40, rows);
                let alert = columns_of(&page, warn, 40, rows);
                assert_eq!(
                    sign.is_some(),
                    with_accent,
                    "{mode:?} {dpi} {status:?}: {sign:?}"
                );
                assert_eq!(
                    alert.is_some(),
                    with_warn,
                    "{mode:?} {dpi} {status:?}: {alert:?}"
                );
                // The sign comes before the words, the gap between them.
                if let Some((_, sign_right)) = sign.or(alert) {
                    let (words_left, _) =
                        columns_of(&page, palette(mode).ink, 24, rows).expect("the words");
                    assert!(sign_right < words_left, "{mode:?} {dpi} {status:?}");
                    let side = whole_px(13, dpi);
                    let (sign_left, _) = sign.or(alert).expect("the sign");
                    assert!(sign_right - sign_left <= side, "{mode:?} {dpi} {status:?}");
                }
            }
        }
    }
}

#[test]
fn a_newer_version_has_download_at_the_right_of_its_line_an_underlined_link() {
    for mode in [Mode::Dark, Mode::Light] {
        for dpi in DPIS {
            let page = showing(
                mode,
                dpi,
                Status::Answered(Answer::Available(
                    Version::parse("1.1.0").expect("a version"),
                )),
            );
            let download = page.link(Link::Download);
            let rows = line_rows(dpi);
            let side = whole_px(layout::WIDTH - layout::SIDE, dpi);
            assert!(
                (side - 1..=side).contains(&download.right),
                "{mode:?} {dpi}: {download:?}"
            );
            assert!(
                rows.0 <= download.top && download.bottom <= rows.1,
                "{mode:?} {dpi}: {download:?}"
            );
            assert!(
                underlined(&page, download, download.top, rows.1, palette(mode).link),
                "{mode:?} {dpi}: no underline in {download:?}"
            );
            // "Version 1.1.0 is available." comes before it, in the ink.
            let (_, words_right) =
                columns_of(&page, palette(mode).ink, 24, rows).expect("the words");
            assert!(words_right < download.left, "{mode:?} {dpi}");
        }
    }
}

#[test]
fn only_a_newer_version_s_line_has_a_link() {
    for status in statuses() {
        let page = showing(Mode::Dark, 144, status);
        let download = page.links.iter().any(|&(link, _)| link == Link::Download);
        assert_eq!(
            download,
            matches!(status, Status::Answered(Answer::Available(_))),
            "{status:?}"
        );
    }
}

/// Writes the page with each line, in both modes at 150%, to target\settings-pages, for a look at it.
#[test]
#[ignore = "writes PNG files for a person to look at"]
fn write_the_lines_for_a_look() {
    let folder =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/settings-pages");
    std::fs::create_dir_all(&folder).expect("the folder");
    for mode in [Mode::Dark, Mode::Light] {
        for (name, status) in ["checking", "latest", "available", "couldnt-check"]
            .into_iter()
            .zip(statuses())
        {
            let page = showing(mode, 144, status);
            let width = u32::try_from(page.width).expect("small");
            let image = screengauge_core::session::Image::new(width, page.height, page.bgra)
                .expect("the bitmap's size");
            // On a thread of its own, which the encoder starts COM on in its own way.
            let png = std::thread::spawn(move || crate::png::encode(&image))
                .join()
                .expect("the encoder's thread")
                .expect("the page as a PNG");
            std::fs::write(folder.join(format!("{mode:?}-144-update-{name}.png")), png)
                .expect("the PNG");
        }
    }
}
