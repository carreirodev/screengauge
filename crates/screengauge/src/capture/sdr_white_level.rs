//! The SDR white level of each monitor, as Windows holds it at the moment of the photo. It is read at every photo,
//! because Windows sends no message when the user moves "SDR content brightness" (WPC-289); reading it costs under a
//! millisecond, up to 18 ms the first time.

use screengauge_core::hdr::SdrWhiteLevel;
use windows::Win32::Devices::Display::{
    DISPLAYCONFIG_DEVICE_INFO_GET_SDR_WHITE_LEVEL, DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME,
    DISPLAYCONFIG_DEVICE_INFO_HEADER, DISPLAYCONFIG_MODE_INFO, DISPLAYCONFIG_PATH_INFO,
    DISPLAYCONFIG_SDR_WHITE_LEVEL, DISPLAYCONFIG_SOURCE_DEVICE_NAME, DisplayConfigGetDeviceInfo,
    GetDisplayConfigBufferSizes, QDC_ONLY_ACTIVE_PATHS, QueryDisplayConfig,
};
use windows::Win32::Foundation::{E_FAIL, ERROR_INSUFFICIENT_BUFFER, WIN32_ERROR};
use windows::core::Error;

use super::{count, device_name};
use crate::win32::struct_size;

/// How many times the list of monitors is asked for again when it changes between asking its size and reading it.
const TRIES: usize = 3;

/// The monitors showing the desktop, as DisplayConfig lists them: one path per monitor, from the desktop's source to the
/// monitor that shows it.
pub(super) struct SdrWhiteLevels {
    paths: Vec<DISPLAYCONFIG_PATH_INFO>,
}

impl SdrWhiteLevels {
    /// The monitors as they are now.
    pub(super) fn now() -> windows::core::Result<SdrWhiteLevels> {
        for _ in 0..TRIES {
            let (mut path_count, mut mode_count) = (0, 0);
            // SAFETY: both out pointers are locals.
            unsafe {
                GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut path_count, &mut mode_count)
            }
            .ok()?;
            let mut paths = vec![DISPLAYCONFIG_PATH_INFO::default(); length(path_count)?];
            let mut modes = vec![DISPLAYCONFIG_MODE_INFO::default(); length(mode_count)?];
            // SAFETY: each buffer holds as many elements as the count passed with it says.
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
            // A monitor came between the two calls, so the sizes are asked for again.
            if listed == ERROR_INSUFFICIENT_BUFFER {
                continue;
            }
            listed.ok()?;
            paths.truncate(length(path_count)?);
            return Ok(SdrWhiteLevels { paths });
        }
        Err(Error::new(
            E_FAIL,
            format!("the monitors changed {TRIES} times while QueryDisplayConfig listed them"),
        ))
    }

    /// The SDR white level of the monitor showing `device`, the GDI device name DXGI also gives its output, such as
    /// `\\.\DISPLAY1`. A desktop shown on two monitors at once takes the first one's. A path whose source name can't be
    /// read is passed over, since it may be another monitor's; its failure is the answer only if no path is `device`'s.
    pub(super) fn of(&self, device: &str) -> windows::core::Result<SdrWhiteLevel> {
        let mut unnamed = None;
        for path in &self.paths {
            let mut source = DISPLAYCONFIG_SOURCE_DEVICE_NAME {
                header: header::<DISPLAYCONFIG_SOURCE_DEVICE_NAME>(
                    DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME,
                    path.sourceInfo.adapterId,
                    path.sourceInfo.id,
                ),
                ..Default::default()
            };
            // SAFETY: the header starts the struct and its size is the whole struct's, which Windows fills.
            if let Err(e) = device_info(unsafe { DisplayConfigGetDeviceInfo(&mut source.header) }) {
                unnamed = Some(e);
                continue;
            }
            if device_name(&source.viewGdiDeviceName) != device {
                continue;
            }
            let mut white = DISPLAYCONFIG_SDR_WHITE_LEVEL {
                header: header::<DISPLAYCONFIG_SDR_WHITE_LEVEL>(
                    DISPLAYCONFIG_DEVICE_INFO_GET_SDR_WHITE_LEVEL,
                    path.targetInfo.adapterId,
                    path.targetInfo.id,
                ),
                SDRWhiteLevel: 0,
            };
            // SAFETY: as above.
            device_info(unsafe { DisplayConfigGetDeviceInfo(&mut white.header) })?;
            return SdrWhiteLevel::from_windows(white.SDRWhiteLevel)
                .ok_or_else(|| Error::new(E_FAIL, "Windows reported an SDR white level of 0"));
        }
        Err(unnamed
            .unwrap_or_else(|| Error::new(E_FAIL, "DisplayConfig lists no monitor showing it")))
    }
}

/// The header of a DisplayConfig request for `T`, which starts with it.
fn header<T>(
    kind: windows::Win32::Devices::Display::DISPLAYCONFIG_DEVICE_INFO_TYPE,
    adapter: windows::Win32::Foundation::LUID,
    id: u32,
) -> DISPLAYCONFIG_DEVICE_INFO_HEADER {
    DISPLAYCONFIG_DEVICE_INFO_HEADER {
        r#type: kind,
        size: struct_size::<T>(),
        adapterId: adapter,
        id,
    }
}

/// DisplayConfigGetDeviceInfo's result, a Win32 error code in an i32.
fn device_info(code: i32) -> windows::core::Result<()> {
    WIN32_ERROR(code.cast_unsigned()).ok()
}

/// A count Windows gave as a length.
fn length(size: u32) -> windows::core::Result<usize> {
    count(size).ok_or_else(|| {
        Error::new(
            E_FAIL,
            "QueryDisplayConfig listed more than can be addressed",
        )
    })
}
