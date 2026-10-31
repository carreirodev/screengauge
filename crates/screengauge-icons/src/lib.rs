//! ScreenGauge's icon files: the app icon and the Store icon drawn from the set square of WPC-291, and every file made
//! from them in `assets/` (see `assets`). `cargo run -p screengauge-icons` writes them again; the tests check that
//! the files in the repository are what this crate draws.

pub mod art;
pub mod assets;
pub mod contrast;
pub mod ico;
pub mod image;
pub mod pixel_art;
pub mod png;
pub mod preview;
pub mod raster;
