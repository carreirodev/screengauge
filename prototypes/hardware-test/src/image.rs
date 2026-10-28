//! Builder A. PNG output through WIC and pixel comparison.

use std::path::Path;

use crate::common::{DiffStats, Photo, PxRect};

/// Writes the photo as an opaque PNG.
pub fn save_png(_photo: &Photo, _path: &Path) -> Result<(), String> {
    todo!("builder A")
}

/// Compares two photos of the same size pixel by pixel (B, G, R; alpha ignored).
/// `exclude` holds rectangles in photo coordinates to skip.
pub fn diff(_a: &Photo, _b: &Photo, _exclude: &[PxRect]) -> Result<DiffStats, String> {
    todo!("builder A")
}
