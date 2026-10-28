//! Builder A. Screen capture (DXGI Desktop Duplication, GDI, Windows.Graphics.Capture) and the `capture` mode.

use crate::common::{Args, MonitorInfo, Photo};

/// One Desktop Duplication per attached output, each created on the adapter that owns that output.
/// Kept alive so the same session can grab several times (the frozen screen's delay series).
pub struct DxgiSession {}

impl DxgiSession {
    pub fn new() -> Result<Self, String> {
        todo!("builder A")
    }

    /// The current image of every output: one entry per output, keyed by GDI device name.
    pub fn grab_all(&mut self) -> Vec<(String, Result<Photo, String>)> {
        todo!("builder A")
    }
}

/// One monitor through GDI (BitBlt from the screen DC).
pub fn gdi_monitor(_m: &MonitorInfo) -> Result<Photo, String> {
    todo!("builder A")
}

/// The frozen-screen photo: DXGI on every monitor, GDI for any monitor where DXGI failed.
/// All or nothing: an error if any monitor fails both ways.
pub fn frozen_photo() -> Result<Vec<Photo>, String> {
    todo!("builder A")
}

pub fn capture_mode(_args: &Args) -> Result<(), String> {
    todo!("builder A")
}
