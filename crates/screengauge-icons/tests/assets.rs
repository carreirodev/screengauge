//! The icon files in `assets/` are what this crate draws, and they hold what WPC-304 asks for: the .ico's sizes, the
//! tray sizes drawn by hand, Microsoft's 3:1 contrast rule on both themes, every package image at its size, the
//! notification header icons (WPC-347), and the Store icon at 300 × 300. After a change to the drawing,
//! `cargo run -p screengauge-icons` writes the files again.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use screengauge_icons::assets::{self, Picture};
use screengauge_icons::image::Image;
use screengauge_icons::{contrast, ico, pixel_art, png};

/// The largest difference allowed in any channel between a committed picture and a fresh drawing: Direct2D's
/// anti-aliasing may round differently on another Windows build, as on the CI runner.
const TOLERANCE: u8 = 2;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets")
}

fn read(path: &str) -> Vec<u8> {
    let file = root().join(path);
    std::fs::read(&file).unwrap_or_else(|e| panic!("reading {}: {e}", file.display()))
}

fn committed_ico() -> Vec<ico::Entry> {
    ico::read(&read(assets::ICO_FILE)).expect("ScreenGauge.ico reads")
}

/// Panics, naming the first differing pixel, unless the pictures match within `TOLERANCE`.
fn assert_close(name: &str, committed: &Image, fresh: &Image) {
    assert_eq!(
        (committed.width(), committed.height()),
        (fresh.width(), fresh.height()),
        "{name}: size"
    );
    let differs = committed
        .rgba()
        .iter()
        .zip(fresh.rgba())
        .position(|(a, b)| a.abs_diff(*b) > TOLERANCE);
    if let Some(byte) = differs {
        let pixel = byte / 4;
        let width = usize::try_from(fresh.width()).expect("small");
        panic!(
            "{name} differs from the drawing at ({}, {}): run `cargo run -p screengauge-icons`",
            pixel % width,
            pixel / width
        );
    }
}

#[test]
fn the_ico_holds_every_size_in_order_with_a_png_only_at_256() {
    let entries = committed_ico();
    let sizes: Vec<u32> = entries.iter().map(|e| e.size).collect();
    assert_eq!(sizes, assets::ICO_SIZES);
    // Microsoft's minimum set, and the tray icon's sizes from 100% to 200%.
    for size in [16, 20, 24, 28, 32, 48, 256] {
        assert!(sizes.contains(&size), "{size}");
    }
    for entry in &entries {
        assert_eq!(entry.bit_count, 32, "{} px", entry.size);
        assert_eq!(entry.is_png(), entry.size == 256, "{} px", entry.size);
    }
}

#[test]
fn the_tray_sizes_are_exactly_the_hand_drawn_pictures() {
    let entries = committed_ico();
    for size in pixel_art::SIZES {
        let drawn = pixel_art::hand_tuned(size)
            .expect("drawn by hand")
            .expect("valid");
        let entry = entries
            .iter()
            .find(|e| e.size == size)
            .unwrap_or_else(|| panic!("no {size} px picture"));
        assert_eq!(entry.image().expect("decodes"), drawn, "{size} px");
    }
}

#[test]
fn the_ico_is_what_the_drawing_makes() {
    let mut drawn = BTreeMap::new();
    for entry in committed_ico() {
        let fresh = assets::make(Picture::AppIcon(entry.size), &mut drawn).expect("draws");
        assert_close(
            &format!("ScreenGauge.ico at {} px", entry.size),
            &entry.image().expect("decodes"),
            &fresh,
        );
    }
}

#[test]
fn half_of_every_size_has_3_to_1_contrast_on_both_themes() {
    let mut failures = Vec::new();
    for entry in committed_ico() {
        let image = entry.image().expect("decodes");
        for (theme, background) in contrast::THEME_BACKGROUNDS {
            let share = contrast::passing_share(&image, background);
            if share < 0.5 {
                failures.push(format!(
                    "{} px on the {theme}: {:.1}%",
                    entry.size,
                    share * 100.0
                ));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "under half at 3:1: {}",
        failures.join("; ")
    );
}

#[test]
fn the_package_holds_exactly_its_images_each_at_its_size_and_as_drawn() {
    assert_folder_holds(assets::PACKAGE_DIR, assets::package_files());
}

#[test]
fn the_notification_folder_holds_exactly_its_icons_each_at_its_size_and_as_drawn() {
    assert_folder_holds(assets::NOTIFICATION_DIR, assets::notification_files());
}

#[test]
fn the_notification_icons_at_the_tray_sizes_are_exactly_the_hand_drawn_pictures() {
    // Windows shows them pixel for pixel at those sizes, so no rounding is allowed.
    for size in pixel_art::SIZES {
        let path = format!("{}/NotificationIcon-{size}.png", assets::NOTIFICATION_DIR);
        let committed = png::decode(&read(&path)).unwrap_or_else(|e| panic!("{path}: {e}"));
        let drawn = pixel_art::hand_tuned(size)
            .expect("drawn by hand")
            .expect("valid");
        assert_eq!(committed, drawn, "{path}");
    }
}

/// Panics unless `folder` holds exactly the `expected` files, each at its size and as drawn.
fn assert_folder_holds(folder: &str, expected: BTreeMap<String, Picture>) {
    let dir = root().join(folder);
    let present: BTreeSet<String> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .map(|entry| {
            let name = entry.expect("a folder entry").file_name();
            format!("{folder}/{}", name.to_string_lossy())
        })
        .collect();
    assert_eq!(
        present,
        expected.keys().cloned().collect::<BTreeSet<_>>(),
        "the {folder} folder"
    );
    let mut drawn = BTreeMap::new();
    for (path, picture) in expected {
        let image = png::decode(&read(&path)).unwrap_or_else(|e| panic!("{path}: {e}"));
        assert_eq!(
            (image.width(), image.height()),
            (picture.side(), picture.side()),
            "{path}"
        );
        assert_close(
            &path,
            &image,
            &assets::make(picture, &mut drawn).expect("draws"),
        );
    }
}

#[test]
fn the_theme_variants_of_each_target_size_are_the_same_picture() {
    for size in assets::TARGET_SIZES {
        let plain = read(&format!("package/AppList.targetsize-{size}.png"));
        for variant in &assets::THEME_VARIANTS[1..] {
            let other = read(&format!("package/AppList.targetsize-{size}{variant}.png"));
            assert_eq!(plain, other, "{size}{variant}");
        }
    }
}

#[test]
fn the_store_icon_is_300_by_300_and_as_drawn() {
    let image = png::decode(&read(assets::STORE_ICON_FILE)).expect("decodes");
    assert_eq!((image.width(), image.height()), (300, 300));
    assert_close(
        assets::STORE_ICON_FILE,
        &image,
        &assets::store_icon().expect("draws"),
    );
}
