//! Every icon file in `assets/`, by name, and the picture each holds.
//!
//! - `ScreenGauge.ico`: ScreenGauge.exe's icon, which the tray icon loads at the taskbar's size, and the installer's.
//! - `package/`: the images the Store copy's MSIX package names in its manifest (Microsoft Learn, "Construct your
//!   Windows app's icon", read 2026-10-31): `AppList` for Square44x44Logo, with a picture at each target size in three
//!   variants (plain, `altform-unplated` for the dark theme, `altform-lightunplated` for the light theme), all three
//!   required even when, as here, they are the same; `MedTile` for Square150x150Logo; `StoreLogo` for the package logo.
//! - `notification/`: the app icon in the header of ScreenGauge's Windows notifications, which ScreenGauge.exe embeds
//!   and registers with Windows at each start. Windows draws it at the small icon size and scales a PNG of any other
//!   size, so there is one per size, the hand-drawn tray sizes included (WPC-347).
//! - `store/StoreIcon.png`: the Store icon, 300 × 300, uploaded in Partner Center for the Store page.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use crate::image::Image;
use crate::{art, ico, pixel_art, png, raster};

/// The pictures in `ScreenGauge.ico`. Beyond Microsoft's minimum (16, 24, 32, 48, 256): 20 and 28 for the tray icon
/// at 125% and 175%, and the target sizes Windows asks for elsewhere, so it rarely has to shrink a picture.
pub const ICO_SIZES: [u32; 15] = [16, 20, 24, 28, 30, 32, 36, 40, 48, 60, 64, 72, 80, 96, 256];

/// The target sizes of the app list images.
pub const TARGET_SIZES: [u32; 14] = [16, 20, 24, 30, 32, 36, 40, 48, 60, 64, 72, 80, 96, 256];

/// The theme variants of each app list image: plain, dark theme, light theme.
pub const THEME_VARIANTS: [&str; 3] = ["", "_altform-unplated", "_altform-lightunplated"];

/// The display scales, in percent, of the scaled package images.
pub const SCALES: [u32; 5] = [100, 125, 150, 200, 400];

/// The sizes of the notification header icon: the small icon size at 100%, 125%, 150%, 175%, 200%, 225%, 250% and
/// 300% scale.
pub const NOTIFICATION_SIZES: [u32; 8] = [16, 20, 24, 28, 32, 36, 40, 48];

pub const ICO_FILE: &str = "ScreenGauge.ico";
pub const STORE_ICON_FILE: &str = "store/StoreIcon.png";
pub const STORE_ICON_SIZE: u32 = 300;
pub const PACKAGE_DIR: &str = "package";
pub const NOTIFICATION_DIR: &str = "notification";

/// What a package image shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Picture {
    /// The app icon at that size.
    AppIcon(u32),
    /// The app icon at `icon` pixels, centered on a transparent `canvas` × `canvas` tile.
    Tile { canvas: u32, icon: u32 },
}

impl Picture {
    pub fn side(self) -> u32 {
        match self {
            Picture::AppIcon(size) => size,
            Picture::Tile { canvas, .. } => canvas,
        }
    }
}

/// Every package image, by its path under `assets/`.
pub fn package_files() -> BTreeMap<String, Picture> {
    let mut files = BTreeMap::new();
    for size in TARGET_SIZES {
        for variant in THEME_VARIANTS {
            files.insert(
                format!("{PACKAGE_DIR}/AppList.targetsize-{size}{variant}.png"),
                Picture::AppIcon(size),
            );
        }
    }
    for scale in SCALES {
        files.insert(
            format!("{PACKAGE_DIR}/AppList.scale-{scale}.png"),
            Picture::AppIcon(scaled(44, scale)),
        );
        let canvas = scaled(150, scale);
        files.insert(
            format!("{PACKAGE_DIR}/MedTile.scale-{scale}.png"),
            Picture::Tile {
                canvas,
                icon: canvas.div_ceil(2),
            },
        );
        files.insert(
            format!("{PACKAGE_DIR}/StoreLogo.scale-{scale}.png"),
            Picture::AppIcon(scaled(50, scale)),
        );
    }
    files
}

/// Every notification header icon, by its path under `assets/`.
pub fn notification_files() -> BTreeMap<String, Picture> {
    NOTIFICATION_SIZES
        .iter()
        .map(|&size| {
            (
                format!("{NOTIFICATION_DIR}/NotificationIcon-{size}.png"),
                Picture::AppIcon(size),
            )
        })
        .collect()
}

/// A base size at a display scale, rounded half up as Microsoft's table rounds it (150 at 125% is 188).
fn scaled(base: u32, percent: u32) -> u32 {
    (base * percent + 50) / 100
}

/// The app icon at `size`: drawn by hand at the tray icon's sizes (`pixel_art::SIZES`), from the shapes otherwise.
pub fn app_icon(size: u32) -> Result<Image, String> {
    match pixel_art::hand_tuned(size) {
        Some(drawn) => drawn,
        None => raster::render(&art::app_icon(size), size)
            .map_err(|e| format!("drawing the {size} px app icon: {e}")),
    }
}

pub fn store_icon() -> Result<Image, String> {
    raster::render(&art::store_icon(STORE_ICON_SIZE), STORE_ICON_SIZE)
        .map_err(|e| format!("drawing the Store icon: {e}"))
}

/// The picture `picture` stands for, drawing each app icon size once.
pub fn make(picture: Picture, drawn: &mut BTreeMap<u32, Image>) -> Result<Image, String> {
    let mut icon = |size: u32| -> Result<Image, String> {
        if let Some(image) = drawn.get(&size) {
            return Ok(image.clone());
        }
        let image = app_icon(size)?;
        drawn.insert(size, image.clone());
        Ok(image)
    };
    match picture {
        Picture::AppIcon(size) => icon(size),
        Picture::Tile { canvas, icon: size } => Ok(icon(size)?.centered_on(canvas, canvas)),
    }
}

/// The bytes of `ScreenGauge.ico`.
pub fn ico_file(drawn: &mut BTreeMap<u32, Image>) -> Result<Vec<u8>, String> {
    let images = ICO_SIZES
        .iter()
        .map(|&size| make(Picture::AppIcon(size), drawn))
        .collect::<Result<Vec<_>, _>>()?;
    ico::write(&images).map_err(|e| format!("writing {ICO_FILE}: {e}"))
}

/// Writes every icon file under `root` (the repository's `assets/`), and removes package and notification images no
/// longer made.
pub fn write_all(root: &Path) -> Result<(), String> {
    let mut drawn = BTreeMap::new();
    write(root, ICO_FILE, &ico_file(&mut drawn)?)?;
    write_folder(root, PACKAGE_DIR, &package_files(), &mut drawn)?;
    write_folder(root, NOTIFICATION_DIR, &notification_files(), &mut drawn)?;
    let store = store_icon()?;
    write(root, STORE_ICON_FILE, &encode(STORE_ICON_FILE, &store)?)
}

/// Writes `files`, all in `folder` under `root`, and removes any other file there.
fn write_folder(
    root: &Path,
    folder: &str,
    files: &BTreeMap<String, Picture>,
    drawn: &mut BTreeMap<u32, Image>,
) -> Result<(), String> {
    for (path, picture) in files {
        let image = make(*picture, drawn)?;
        write(root, path, &encode(path, &image)?)?;
    }
    let dir = root.join(folder);
    for entry in fs::read_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))? {
        let entry = entry.map_err(|e| format!("{}: {e}", dir.display()))?;
        let name = format!("{folder}/{}", entry.file_name().to_string_lossy());
        if !files.contains_key(&name) {
            fs::remove_file(entry.path()).map_err(|e| format!("removing {name}: {e}"))?;
        }
    }
    Ok(())
}

fn encode(path: &str, image: &Image) -> Result<Vec<u8>, String> {
    png::encode(image).map_err(|e| format!("encoding {path}: {e}"))
}

fn write(root: &Path, path: &str, bytes: &[u8]) -> Result<(), String> {
    let file = root.join(path);
    if let Some(folder) = file.parent() {
        fs::create_dir_all(folder).map_err(|e| format!("{}: {e}", folder.display()))?;
    }
    fs::write(&file, bytes).map_err(|e| format!("{}: {e}", file.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scales_round_as_microsofts_table() {
        assert_eq!(SCALES.map(|s| scaled(44, s)), [44, 55, 66, 88, 176]);
        assert_eq!(SCALES.map(|s| scaled(150, s)), [150, 188, 225, 300, 600]);
        assert_eq!(SCALES.map(|s| scaled(50, s)), [50, 63, 75, 100, 200]);
    }

    #[test]
    fn every_target_size_has_its_three_theme_variants() {
        let files = package_files();
        for size in TARGET_SIZES {
            for variant in THEME_VARIANTS {
                let name = format!("package/AppList.targetsize-{size}{variant}.png");
                assert_eq!(files.get(&name), Some(&Picture::AppIcon(size)), "{name}");
            }
        }
        assert_eq!(files.len(), TARGET_SIZES.len() * 3 + SCALES.len() * 3);
    }

    #[test]
    fn every_tray_size_has_a_notification_icon() {
        // At those scales the notification header is the same size as the tray icon.
        for size in pixel_art::SIZES {
            assert!(NOTIFICATION_SIZES.contains(&size), "{size}");
        }
    }

    #[test]
    fn every_target_size_is_in_the_ico() {
        for size in TARGET_SIZES {
            assert!(ICO_SIZES.contains(&size), "{size}");
        }
    }
}
