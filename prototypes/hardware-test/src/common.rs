//! Types shared by every mode. Owned by the integrator; builders read it and don't edit it.

use windows::Win32::Graphics::Gdi::HMONITOR;

/// Command line: `hwtest <mode> [--name value | --flag]...`.
pub struct Args {
    pub mode: String,
    pub rest: Vec<String>,
}

impl Args {
    pub fn from_env() -> Self {
        let mut all = std::env::args().skip(1);
        let mode = all.next().unwrap_or_else(|| "help".to_string());
        Self { mode, rest: all.collect() }
    }

    /// `--name value` parsed as a number, or `default` when absent.
    pub fn num(&self, name: &str, default: u64) -> Result<u64, String> {
        match self.value(name) {
            None => Ok(default),
            Some(v) => v.parse().map_err(|e| format!("--{name} {v}: {e}")),
        }
    }

    pub fn value(&self, name: &str) -> Option<&str> {
        let key = format!("--{name}");
        let at = self.rest.iter().position(|a| *a == key)?;
        self.rest.get(at + 1).map(String::as_str)
    }

    pub fn flag(&self, name: &str) -> bool {
        let key = format!("--{name}");
        self.rest.iter().any(|a| *a == key)
    }
}

/// A rectangle in physical pixels on the virtual screen (right and bottom exclusive).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PxRect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl PxRect {
    pub fn width(&self) -> i32 {
        self.right - self.left
    }

    pub fn height(&self) -> i32 {
        self.bottom - self.top
    }
}

impl std::fmt::Display for PxRect {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "({},{})-({},{}) {}x{}", self.left, self.top, self.right, self.bottom, self.width(), self.height())
    }
}

/// One monitor as a Per-Monitor V2 process sees it.
#[derive(Clone, Debug)]
pub struct MonitorInfo {
    pub hmonitor: HMONITOR,
    /// GDI device name, e.g. `\\.\DISPLAY1`.
    pub device_name: String,
    /// Model name from DisplayConfig, e.g. `U27B3A`; empty when unknown.
    pub friendly_name: String,
    /// `rcMonitor`, physical pixels.
    pub rect: PxRect,
    /// `rcWork`, physical pixels.
    pub work: PxRect,
    /// Effective DPI (`MDT_EFFECTIVE_DPI`); 96 = 100%.
    pub dpi: u32,
    pub primary: bool,
}

impl MonitorInfo {
    /// 1.0 at 100%, 1.75 at 175%.
    pub fn scale(&self) -> f64 {
        f64::from(self.dpi) / 96.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureMethod {
    Dxgi,
    Gdi,
    Wgc,
}

/// The pixels of one monitor at one instant.
pub struct Photo {
    /// GDI device name of the monitor, matching `MonitorInfo::device_name`.
    pub device_name: String,
    /// Where the photo sits on the virtual screen, physical pixels.
    pub rect: PxRect,
    pub width: u32,
    pub height: u32,
    /// BGRA, top-down rows, stride `width * 4`, alpha forced to 255.
    pub bgra: Vec<u8>,
    pub method: CaptureMethod,
    /// `log::now_ms()` when the pixels were grabbed.
    pub taken_ms: f64,
}

/// Result of comparing two photos pixel by pixel.
#[derive(Clone, Copy, Debug, Default)]
pub struct DiffStats {
    /// Pixels compared (outside the excluded rectangles).
    pub total: u64,
    /// Pixels where any of B, G, R differs.
    pub differing: u64,
    /// Largest difference in any channel.
    pub max_delta: u8,
    /// First differing pixel, in photo coordinates.
    pub first: Option<(u32, u32)>,
}

/// Adds a short context to an error and turns it into a `String`, for `?` across the program.
pub trait Ctx<T> {
    fn ctx(self, what: &str) -> Result<T, String>;
}

impl<T, E: std::fmt::Display> Ctx<T> for Result<T, E> {
    fn ctx(self, what: &str) -> Result<T, String> {
        self.map_err(|e| format!("{what}: {e}"))
    }
}
