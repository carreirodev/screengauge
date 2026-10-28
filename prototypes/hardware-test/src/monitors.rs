//! Builder A. Monitor enumeration and the `info` mode.

use crate::common::{Args, MonitorInfo};

/// Every attached monitor as this Per-Monitor V2 process sees it, primary first.
pub fn enumerate() -> Result<Vec<MonitorInfo>, String> {
    todo!("builder A")
}

pub fn info_mode(_args: &Args) -> Result<(), String> {
    todo!("builder A")
}
