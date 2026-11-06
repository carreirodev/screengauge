//! The monitor under a point of the virtual screen, which Settings, the tray icon's menu and Save As open on, and the
//! monitor a window is on.

use windows::Win32::Foundation::{E_FAIL, HWND, POINT, RECT};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, HMONITOR, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint,
    MonitorFromWindow,
};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::core::Error;

use crate::win32::struct_size;

/// A monitor, with its rectangle and its work area, in physical pixels of the virtual screen.
pub struct Monitor {
    handle: HMONITOR,
    /// The whole monitor.
    pub area: RECT,
    /// The monitor without the taskbar, and any bar docked to its sides.
    pub work: RECT,
}

/// The monitor at `point`, or the nearest one.
pub fn at(point: POINT) -> windows::core::Result<Monitor> {
    // SAFETY: no pointers; the nearest monitor is always one.
    described(unsafe { MonitorFromPoint(point, MONITOR_DEFAULTTONEAREST) })
}

/// The monitor most of `hwnd` is on, or the nearest one.
pub fn of_window(hwnd: HWND) -> windows::core::Result<Monitor> {
    // SAFETY: a window of ScreenGauge's; the nearest monitor is always one.
    described(unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) })
}

fn described(handle: HMONITOR) -> windows::core::Result<Monitor> {
    let mut info = MONITORINFO {
        cbSize: const { struct_size::<MONITORINFO>() },
        ..Default::default()
    };
    // SAFETY: fills a local whose size is set.
    if !unsafe { GetMonitorInfoW(handle, &mut info) }.as_bool() {
        return Err(Error::new(E_FAIL, "GetMonitorInfoW found no monitor"));
    }
    Ok(Monitor {
        handle,
        area: info.rcMonitor,
        work: info.rcWork,
    })
}

impl Monitor {
    /// The monitor's DPI: 96 at 100%.
    pub fn dpi(&self) -> windows::core::Result<u32> {
        let (mut dpi, mut unused) = (0, 0);
        // SAFETY: fills two locals.
        unsafe { GetDpiForMonitor(self.handle, MDT_EFFECTIVE_DPI, &mut dpi, &mut unused) }?;
        Ok(dpi)
    }
}
