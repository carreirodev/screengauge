//! HDR turned on and off by program, as Windows' own Settings does it (DisplayConfig's advanced color state, the way
//! WPC-327's live checks did), and put back as it was however the routine ends.

use std::time::Duration;

use windows::Win32::Devices::Display::{
    DISPLAYCONFIG_DEVICE_INFO_GET_ADVANCED_COLOR_INFO, DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME,
    DISPLAYCONFIG_DEVICE_INFO_HEADER, DISPLAYCONFIG_DEVICE_INFO_SET_ADVANCED_COLOR_STATE,
    DISPLAYCONFIG_DEVICE_INFO_TYPE, DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO, DISPLAYCONFIG_MODE_INFO,
    DISPLAYCONFIG_PATH_INFO, DISPLAYCONFIG_SET_ADVANCED_COLOR_STATE,
    DISPLAYCONFIG_SET_ADVANCED_COLOR_STATE_0, DISPLAYCONFIG_TARGET_DEVICE_NAME,
    DisplayConfigGetDeviceInfo, DisplayConfigSetDeviceInfo, GetDisplayConfigBufferSizes,
    QDC_ONLY_ACTIVE_PATHS, QueryDisplayConfig,
};
use windows::Win32::Foundation::{ERROR_INSUFFICIENT_BUFFER, LUID, WIN32_ERROR};

use crate::{screen, struct_size};

/// How long the screen takes to settle after HDR turns on or off: it repaints for a few seconds (WPC-327).
pub const SETTLE: Duration = Duration::from_secs(8);

/// A monitor as DisplayConfig sees it.
#[derive(Debug, Clone)]
pub struct Display {
    adapter: LUID,
    id: u32,
    pub name: String,
    pub supported: bool,
    pub on: bool,
}

/// The monitors shown now, with whether each can turn HDR on and whether it is on.
pub fn displays() -> Result<Vec<Display>, String> {
    paths()?
        .into_iter()
        .map(|path| {
            let (adapter, id) = (path.targetInfo.adapterId, path.targetInfo.id);
            let mut name = DISPLAYCONFIG_TARGET_DEVICE_NAME {
                header: header::<DISPLAYCONFIG_TARGET_DEVICE_NAME>(
                    DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME,
                    adapter,
                    id,
                ),
                ..Default::default()
            };
            // SAFETY: the header names its own struct's type and size.
            let code = unsafe { DisplayConfigGetDeviceInfo(&mut name.header) };
            check(code, "target name")?;
            let friendly = &name.monitorFriendlyDeviceName;
            let length = friendly
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(friendly.len());
            let (supported, on) = state(adapter, id)?;
            Ok(Display {
                adapter,
                id,
                name: String::from_utf16_lossy(&friendly[..length]),
                supported,
                on,
            })
        })
        .collect()
}

fn state(adapter: LUID, id: u32) -> Result<(bool, bool), String> {
    let mut info = DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO {
        header: header::<DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO>(
            DISPLAYCONFIG_DEVICE_INFO_GET_ADVANCED_COLOR_INFO,
            adapter,
            id,
        ),
        ..Default::default()
    };
    // SAFETY: the header names its own struct's type and size.
    let code = unsafe { DisplayConfigGetDeviceInfo(&mut info.header) };
    check(code, "advanced color info")?;
    // SAFETY: every bit pattern of the union is a valid u32. Bit 0 is advancedColorSupported, bit 1 advancedColorEnabled.
    let bits = unsafe { info.Anonymous.value };
    Ok((bits & 1 != 0, bits & 2 != 0))
}

/// Turns HDR on or off on `display`, and checks that Windows did.
pub fn set(display: &Display, on: bool) -> Result<(), String> {
    let request = DISPLAYCONFIG_SET_ADVANCED_COLOR_STATE {
        header: header::<DISPLAYCONFIG_SET_ADVANCED_COLOR_STATE>(
            DISPLAYCONFIG_DEVICE_INFO_SET_ADVANCED_COLOR_STATE,
            display.adapter,
            display.id,
        ),
        // Bit 0 is enableAdvancedColor.
        Anonymous: DISPLAYCONFIG_SET_ADVANCED_COLOR_STATE_0 {
            value: u32::from(on),
        },
    };
    // SAFETY: the header names its own struct's type and size.
    let code = unsafe { DisplayConfigSetDeviceInfo(&request.header) };
    check(code, "turning HDR on or off")?;
    let (_, now) = state(display.adapter, display.id)?;
    if now != on {
        return Err(format!(
            "Windows left HDR {} on {} after it was asked to turn it {}",
            if now { "on" } else { "off" },
            display.name,
            if on { "on" } else { "off" }
        ));
    }
    Ok(())
}

/// Turns HDR on or off on every monitor that can show it, and waits for the screen to settle when anything changed.
pub fn set_all(on: bool) -> Result<(), String> {
    let mut changed = false;
    for display in displays()?.iter().filter(|d| d.supported && d.on != on) {
        set(display, on)?;
        changed = true;
    }
    if changed {
        screen::wait(SETTLE);
    }
    Ok(())
}

/// Puts each monitor's HDR back as it was when made, when dropped.
pub struct Restore {
    before: Vec<Display>,
}

impl Restore {
    pub fn now() -> Result<Restore, String> {
        Ok(Restore {
            before: displays()?,
        })
    }
}

impl Drop for Restore {
    fn drop(&mut self) {
        let now = match displays() {
            Ok(now) => now,
            Err(e) => {
                eprintln!("warning: HDR can't be put back, the monitors can't be read: {e}");
                return;
            }
        };
        for before in self.before.iter().filter(|d| d.supported) {
            let same = now
                .iter()
                .find(|d| d.adapter == before.adapter && d.id == before.id);
            if same.is_some_and(|d| d.on != before.on)
                && let Err(e) = set(before, before.on)
            {
                eprintln!("warning: HDR on {} wasn't put back: {e}", before.name);
            }
        }
    }
}

fn paths() -> Result<Vec<DISPLAYCONFIG_PATH_INFO>, String> {
    // The monitors may change between asking the sizes and listing them; then it is asked again.
    for _ in 0..3 {
        let (mut path_count, mut mode_count) = (0, 0);
        // SAFETY: two u32 for the call to fill.
        unsafe {
            GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut path_count, &mut mode_count)
        }
        .ok()
        .map_err(|e| format!("GetDisplayConfigBufferSizes: {e}"))?;
        let length = |n: u32| usize::try_from(n).map_err(|e| e.to_string());
        let mut paths = vec![DISPLAYCONFIG_PATH_INFO::default(); length(path_count)?];
        let mut modes = vec![DISPLAYCONFIG_MODE_INFO::default(); length(mode_count)?];
        // SAFETY: each array holds as many elements as its count says.
        let listed = unsafe {
            QueryDisplayConfig(
                QDC_ONLY_ACTIVE_PATHS,
                &mut path_count,
                paths.as_mut_ptr(),
                &mut mode_count,
                modes.as_mut_ptr(),
                None,
            )
        };
        if listed == ERROR_INSUFFICIENT_BUFFER {
            continue;
        }
        listed
            .ok()
            .map_err(|e| format!("QueryDisplayConfig: {e}"))?;
        paths.truncate(length(path_count)?);
        return Ok(paths);
    }
    Err("the monitors changed 3 times while QueryDisplayConfig listed them".to_string())
}

fn header<T>(
    kind: DISPLAYCONFIG_DEVICE_INFO_TYPE,
    adapter: LUID,
    id: u32,
) -> DISPLAYCONFIG_DEVICE_INFO_HEADER {
    DISPLAYCONFIG_DEVICE_INFO_HEADER {
        r#type: kind,
        size: struct_size::<T>(),
        adapterId: adapter,
        id,
    }
}

fn check(code: i32, what: &str) -> Result<(), String> {
    WIN32_ERROR(code.cast_unsigned())
        .ok()
        .map_err(|e| format!("DisplayConfig, {what}: {e}"))
}
