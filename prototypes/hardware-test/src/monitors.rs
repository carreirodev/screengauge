//! Builder A. Monitor enumeration and the `info` mode.

use std::collections::HashMap;

use windows::Win32::Devices::Display::{
    DISPLAYCONFIG_DEVICE_INFO_GET_ADVANCED_COLOR_INFO, DISPLAYCONFIG_DEVICE_INFO_GET_SDR_WHITE_LEVEL,
    DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME, DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME,
    DISPLAYCONFIG_DEVICE_INFO_HEADER, DISPLAYCONFIG_DEVICE_INFO_TYPE, DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO,
    DISPLAYCONFIG_MODE_INFO, DISPLAYCONFIG_PATH_INFO, DISPLAYCONFIG_SDR_WHITE_LEVEL, DISPLAYCONFIG_SOURCE_DEVICE_NAME,
    DISPLAYCONFIG_TARGET_DEVICE_NAME, DisplayConfigGetDeviceInfo, GetDisplayConfigBufferSizes, QDC_ONLY_ACTIVE_PATHS,
    QueryDisplayConfig,
};
use windows::Win32::Foundation::{ERROR_INSUFFICIENT_BUFFER, ERROR_SUCCESS, LPARAM, POINT, RECT};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_COLOR_SPACE_RGB_FULL_G22_NONE_P709, DXGI_COLOR_SPACE_RGB_FULL_G2084_NONE_P2020, DXGI_MODE_ROTATION,
};
use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, DXGI_ERROR_NOT_FOUND, IDXGIFactory1, IDXGIOutput6};
use windows::Win32::Graphics::Gdi::{
    DEVMODEW, ENUM_CURRENT_SETTINGS, ENUM_DISPLAY_SETTINGS_FLAGS, EnumDisplayMonitors, EnumDisplaySettingsExW,
    GetMonitorInfoW, HDC, HMONITOR, MONITORINFO, MONITORINFOEXW,
};
use windows::Win32::UI::HiDpi::{
    AreDpiAwarenessContextsEqual, DPI_AWARENESS_CONTEXT, DPI_AWARENESS_CONTEXT_UNAWARE, GetDpiForMonitor,
    GetThreadDpiAwarenessContext, MDT_ANGULAR_DPI, MDT_EFFECTIVE_DPI, MDT_RAW_DPI, MONITOR_DPI_TYPE,
    SetThreadDpiAwarenessContext,
};
use windows::Win32::UI::Shell::GetScaleFactorForMonitor;
use windows::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, GetPhysicalCursorPos, GetSystemMetrics, MONITORINFOF_PRIMARY, SM_CMONITORS, SM_CXSCREEN,
    SM_CXVIRTUALSCREEN, SM_CYSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
};
use windows::core::{BOOL, Interface, PCWSTR};

use crate::common::{Args, Ctx, MonitorInfo, PxRect};
use crate::log;

/// Every attached monitor as this Per-Monitor V2 process sees it, primary first.
pub fn enumerate() -> Result<Vec<MonitorInfo>, String> {
    let names = match friendly_names() {
        Ok(n) => n,
        Err(e) => {
            log::line(format!("monitors: friendly names unavailable, leaving them empty: {e}"));
            HashMap::new()
        }
    };
    let mut out = Vec::new();
    for h in handles()? {
        let b = basics(h)?;
        let (dpi, _) = dpi(h, MDT_EFFECTIVE_DPI)?;
        out.push(MonitorInfo {
            hmonitor: h,
            friendly_name: names.get(&b.device).cloned().unwrap_or_default(),
            device_name: b.device,
            rect: b.rect,
            work: b.work,
            dpi,
            primary: b.primary,
        });
    }
    // Stable sort: primary first, the rest in enumeration order.
    out.sort_by_key(|m| !m.primary);
    Ok(out)
}

pub fn info_mode(_args: &Args) -> Result<(), String> {
    let monitors = enumerate()?;
    log::line(format!("--- {} monitors, Per-Monitor V2 view", monitors.len()));
    let mut devmodes: HashMap<String, DevMode> = HashMap::new();
    for m in &monitors {
        log::line(format!(
            "{} '{}' primary={} hmonitor={:?}",
            m.device_name, m.friendly_name, m.primary, m.hmonitor.0
        ));
        log::line(format!("  rcMonitor {}", m.rect));
        log::line(format!("  rcWork    {}", m.work));
        for (label, kind) in [("effective", MDT_EFFECTIVE_DPI), ("angular", MDT_ANGULAR_DPI), ("raw", MDT_RAW_DPI)] {
            match dpi(m.hmonitor, kind) {
                Ok((x, y)) => log::line(format!("  dpi {label}: {x} x {y}")),
                Err(e) => log::line(format!("  dpi {label}: ERROR {e}")),
            }
        }
        log::line(format!("  scale from effective dpi: {:.4} ({}%)", m.scale(), m.dpi * 100 / 96));
        // SAFETY: plain query on a monitor handle that EnumDisplayMonitors just returned.
        match unsafe { GetScaleFactorForMonitor(m.hmonitor) } {
            Ok(f) => log::line(format!("  GetScaleFactorForMonitor: {}", f.0)),
            Err(e) => log::line(format!("  GetScaleFactorForMonitor: ERROR {e}")),
        }
        match devmode(&m.device_name) {
            Ok(d) => {
                log::line(format!(
                    "  DEVMODE current: {}x{} at ({},{}), orientation {}, {} Hz, {} bpp",
                    d.width, d.height, d.x, d.y, d.orientation, d.frequency, d.bits
                ));
                devmodes.insert(m.device_name.clone(), d);
            }
            Err(e) => log::line(format!("  DEVMODE: ERROR {e}")),
        }
    }
    let pmv2_virtual = virtual_screen();
    log::line(format!("virtual screen (Per-Monitor V2): {pmv2_virtual}"));

    log::line("--- DXGI");
    let outputs = dxgi_outputs()?;

    log::line("--- physical pixels: rcMonitor vs DXGI DesktopCoordinates vs DEVMODE");
    for m in &monitors {
        let dx = outputs.iter().find(|o| o.device_name == m.device_name);
        let dm = devmodes.get(&m.device_name);
        let dx_rect = dx.map(|o| o.rect);
        let dm_rect = dm.map(|d| PxRect { left: d.x, top: d.y, right: d.x + d.width, bottom: d.y + d.height });
        let verdict = match (dx_rect, dm_rect) {
            (Some(a), Some(b)) if a == m.rect && b == m.rect => "SAME (all three are physical pixels)".to_string(),
            (Some(_), Some(_)) => "DIFFERENT".to_string(),
            _ => "INCOMPLETE (a source is missing; see the errors above)".to_string(),
        };
        log::line(format!(
            "RESULT physical-px {}: rcMonitor={} dxgi={} devmode={} -> {verdict}",
            m.device_name,
            m.rect,
            dx_rect.map_or("none".to_string(), |r| r.to_string()),
            dm_rect.map_or("none".to_string(), |r| r.to_string()),
        ));
        let hdr = match dx.and_then(|o| o.color_space) {
            Some(cs) if cs == DXGI_COLOR_SPACE_RGB_FULL_G2084_NONE_P2020.0 => "ON (ColorSpace G2084_P2020)".to_string(),
            Some(cs) if cs == DXGI_COLOR_SPACE_RGB_FULL_G22_NONE_P709.0 => "off (ColorSpace G22_P709, SDR)".to_string(),
            Some(cs) => format!("unknown (ColorSpace {cs})"),
            None => "unknown (no IDXGIOutput6 data)".to_string(),
        };
        log::line(format!("RESULT hdr {}: {hdr}", m.device_name));
    }
    for o in outputs.iter().filter(|o| !monitors.iter().any(|m| m.device_name == o.device_name)) {
        log::line(format!("DXGI output {} on '{}' matches no GDI monitor", o.device_name, o.adapter));
    }

    let (logical, physical) = cursor_pair()?;
    log::line(format!(
        "RESULT cursor-pmv2: GetCursorPos=({},{}) GetPhysicalCursorPos=({},{}) -> {}",
        logical.x,
        logical.y,
        physical.x,
        physical.y,
        if logical == physical { "SAME" } else { "DIFFERENT" }
    ));

    log::line("--- DPI-unaware thread view (SetThreadDpiAwarenessContext UNAWARE)");
    let (unaware, unaware_virtual, unaware_cursor) = {
        let _guard = ThreadDpi::set(DPI_AWARENESS_CONTEXT_UNAWARE)?;
        // SAFETY: plain queries on the calling thread's DPI context.
        let is_unaware = unsafe {
            AreDpiAwarenessContextsEqual(GetThreadDpiAwarenessContext(), DPI_AWARENESS_CONTEXT_UNAWARE)
        }
        .as_bool();
        log::line(format!("thread context is now unaware: {is_unaware}"));
        let mut rects = Vec::new();
        for h in handles()? {
            let b = basics(h)?;
            let d = match dpi(h, MDT_EFFECTIVE_DPI) {
                Ok((x, _)) => x.to_string(),
                Err(e) => format!("ERROR {e}"),
            };
            log::line(format!(
                "{} unaware: rcMonitor {} rcWork {} effective dpi {d}",
                b.device, b.rect, b.work
            ));
            rects.push((b.device, b.rect));
        }
        let virt = virtual_screen();
        log::line(format!("virtual screen (unaware): {virt}"));
        let cursor = cursor_pair()?;
        log::line(format!(
            "cursor (unaware): GetCursorPos=({},{}) GetPhysicalCursorPos=({},{})",
            cursor.0.x, cursor.0.y, cursor.1.x, cursor.1.y
        ));
        (rects, virt, cursor)
    };
    // SAFETY: plain queries on the calling thread's DPI context.
    let restored = unsafe {
        AreDpiAwarenessContextsEqual(
            GetThreadDpiAwarenessContext(),
            windows::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        )
    }
    .as_bool();
    log::line(format!("thread context restored to per-monitor v2: {restored}"));
    if !restored {
        return Err("could not restore the thread DPI context to Per-Monitor V2".to_string());
    }

    log::line("--- unaware vs physical (the rounding columns are deductions from the numbers above)");
    let primary_scale = monitors.iter().find(|m| m.primary).map_or(1.0, MonitorInfo::scale);
    for m in &monitors {
        let Some((_, u)) = unaware.iter().find(|(d, _)| *d == m.device_name) else {
            log::line(format!("RESULT unaware-rect {}: missing from the unaware enumeration", m.device_name));
            continue;
        };
        let s = m.scale();
        let ew = f64::from(m.rect.width()) / s;
        let eh = f64::from(m.rect.height()) / s;
        log::line(format!(
            "RESULT unaware-rect {}: physical {} at {:.0}% -> unaware {}; size physical/scale = {:.2}x{:.2}, rounding seen: w {} h {}",
            m.device_name,
            m.rect,
            s * 100.0,
            u,
            ew,
            eh,
            rounding(ew, u.width()),
            rounding(eh, u.height())
        ));
        log::line(format!(
            "  origin: unaware ({},{}); physical/own scale = ({:.2},{:.2}); physical/primary scale = ({:.2},{:.2})",
            u.left,
            u.top,
            f64::from(m.rect.left) / s,
            f64::from(m.rect.top) / s,
            f64::from(m.rect.left) / primary_scale,
            f64::from(m.rect.top) / primary_scale
        ));
    }
    for (i, a) in monitors.iter().enumerate() {
        for b in monitors.iter().skip(i + 1) {
            let ua = unaware.iter().find(|(d, _)| *d == a.device_name).map(|(_, r)| *r);
            let ub = unaware.iter().find(|(d, _)| *d == b.device_name).map(|(_, r)| *r);
            let (Some(ua), Some(ub)) = (ua, ub) else { continue };
            log::line(format!(
                "RESULT unaware-layout {}|{}: physical {} -> unaware {}",
                a.device_name,
                b.device_name,
                relation(&a.rect, &b.rect),
                relation(&ua, &ub)
            ));
        }
    }
    log::line(format!(
        "RESULT unaware-virtual-screen: Per-Monitor V2 {pmv2_virtual} -> unaware {unaware_virtual}"
    ));
    let (ul, up) = unaware_cursor;
    log::line(format!(
        "RESULT cursor-unaware: GetCursorPos=({},{}) GetPhysicalCursorPos=({},{}) -> {}",
        ul.x,
        ul.y,
        up.x,
        up.y,
        if ul == up { "SAME" } else { "DIFFERENT" }
    ));
    Ok(())
}

/// GetCursorPos and GetPhysicalCursorPos, read back to back in the calling thread's DPI context.
pub fn cursor_pair() -> Result<(POINT, POINT), String> {
    let mut logical = POINT::default();
    let mut physical = POINT::default();
    // SAFETY: each call writes one POINT that lives on this stack frame.
    unsafe { GetCursorPos(&mut logical) }.ctx("GetCursorPos")?;
    // SAFETY: as above.
    unsafe { GetPhysicalCursorPos(&mut physical) }.ctx("GetPhysicalCursorPos")?;
    Ok((logical, physical))
}

/// A null-terminated UTF-16 buffer as a Rust string.
pub fn wide(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

/// A Rust string as a null-terminated UTF-16 buffer, for `PCWSTR(v.as_ptr())`.
pub fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn px(r: &RECT) -> PxRect {
    PxRect { left: r.left, top: r.top, right: r.right, bottom: r.bottom }
}

/// `\\.\DISPLAY1` -> `DISPLAY1`, for file names and short labels.
pub fn short_name(device: &str) -> &str {
    device.trim_start_matches(r"\\.\")
}

pub fn rotation_name(r: DXGI_MODE_ROTATION) -> &'static str {
    match r.0 {
        0 => "unspecified",
        1 => "identity",
        2 => "rotate90",
        3 => "rotate180",
        4 => "rotate270",
        _ => "unknown",
    }
}

/// Restores the thread's previous DPI awareness context when dropped.
struct ThreadDpi(DPI_AWARENESS_CONTEXT);

impl ThreadDpi {
    fn set(ctx: DPI_AWARENESS_CONTEXT) -> Result<Self, String> {
        // SAFETY: changes only the calling thread's DPI context; the guard restores it.
        let old = unsafe { SetThreadDpiAwarenessContext(ctx) };
        if old.0.is_null() {
            return Err("SetThreadDpiAwarenessContext returned NULL (invalid context)".to_string());
        }
        Ok(Self(old))
    }
}

impl Drop for ThreadDpi {
    fn drop(&mut self) {
        // SAFETY: restores the context returned by the earlier SetThreadDpiAwarenessContext.
        let back = unsafe { SetThreadDpiAwarenessContext(self.0) };
        if back.0.is_null() {
            log::line("ERROR: SetThreadDpiAwarenessContext failed to restore the previous context");
        }
    }
}

struct Basics {
    device: String,
    rect: PxRect,
    work: PxRect,
    primary: bool,
}

fn handles() -> Result<Vec<HMONITOR>, String> {
    unsafe extern "system" fn push(h: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> BOOL {
        // SAFETY: `data` is the `&mut Vec` passed below, alive for the whole EnumDisplayMonitors call.
        let list = unsafe { &mut *(data.0 as *mut Vec<HMONITOR>) };
        list.push(h);
        true.into()
    }
    let mut list: Vec<HMONITOR> = Vec::new();
    // SAFETY: the callback only touches `list`, which outlives the call.
    let ok = unsafe { EnumDisplayMonitors(None, None, Some(push), LPARAM(&mut list as *mut Vec<HMONITOR> as isize)) };
    if !ok.as_bool() {
        return Err("EnumDisplayMonitors failed".to_string());
    }
    Ok(list)
}

fn basics(h: HMONITOR) -> Result<Basics, String> {
    let mut mi = MONITORINFOEXW::default();
    mi.monitorInfo.cbSize = size_of::<MONITORINFOEXW>() as u32;
    // SAFETY: MONITORINFOEXW starts with MONITORINFO, and cbSize tells the API it may fill the extended struct.
    let ok = unsafe { GetMonitorInfoW(h, &mut mi as *mut MONITORINFOEXW as *mut MONITORINFO) };
    if !ok.as_bool() {
        return Err(format!("GetMonitorInfoW({:?}) failed", h.0));
    }
    Ok(Basics {
        device: wide(&mi.szDevice),
        rect: px(&mi.monitorInfo.rcMonitor),
        work: px(&mi.monitorInfo.rcWork),
        primary: mi.monitorInfo.dwFlags & MONITORINFOF_PRIMARY != 0,
    })
}

fn dpi(h: HMONITOR, kind: MONITOR_DPI_TYPE) -> Result<(u32, u32), String> {
    let (mut x, mut y) = (0u32, 0u32);
    // SAFETY: writes two u32 on this stack frame.
    unsafe { GetDpiForMonitor(h, kind, &mut x, &mut y) }.ctx("GetDpiForMonitor")?;
    Ok((x, y))
}

struct DevMode {
    width: i32,
    height: i32,
    x: i32,
    y: i32,
    orientation: u32,
    frequency: u32,
    bits: u32,
}

fn devmode(device: &str) -> Result<DevMode, String> {
    let mut dm = DEVMODEW { dmSize: size_of::<DEVMODEW>() as u16, ..Default::default() };
    let name = to_wide(device);
    // SAFETY: `dm` has dmSize set and `name` is a null-terminated UTF-16 string that outlives the call.
    let ok = unsafe {
        EnumDisplaySettingsExW(PCWSTR(name.as_ptr()), ENUM_CURRENT_SETTINGS, &mut dm, ENUM_DISPLAY_SETTINGS_FLAGS(0))
    };
    if !ok.as_bool() {
        return Err(format!("EnumDisplaySettingsExW({device}, ENUM_CURRENT_SETTINGS) failed"));
    }
    // SAFETY: for display devices the union holds the position/orientation variant (dmPosition, dmDisplayOrientation).
    let display = unsafe { dm.Anonymous1.Anonymous2 };
    Ok(DevMode {
        width: dm.dmPelsWidth as i32,
        height: dm.dmPelsHeight as i32,
        x: display.dmPosition.x,
        y: display.dmPosition.y,
        orientation: display.dmDisplayOrientation.0,
        frequency: dm.dmDisplayFrequency,
        bits: dm.dmBitsPerPel,
    })
}

fn virtual_screen() -> String {
    // SAFETY: plain metric queries.
    let (x, y, w, h, n, sw, sh) = unsafe {
        (
            GetSystemMetrics(SM_XVIRTUALSCREEN),
            GetSystemMetrics(SM_YVIRTUALSCREEN),
            GetSystemMetrics(SM_CXVIRTUALSCREEN),
            GetSystemMetrics(SM_CYVIRTUALSCREEN),
            GetSystemMetrics(SM_CMONITORS),
            GetSystemMetrics(SM_CXSCREEN),
            GetSystemMetrics(SM_CYSCREEN),
        )
    };
    format!("origin ({x},{y}) size {w}x{h}, {n} monitors, primary SM_CXSCREEN x SM_CYSCREEN = {sw}x{sh}")
}

/// The active DisplayConfig paths (one per lit monitor).
fn active_paths() -> Result<Vec<DISPLAYCONFIG_PATH_INFO>, String> {
    let mut paths: Vec<DISPLAYCONFIG_PATH_INFO> = Vec::new();
    let mut done = false;
    for _attempt in 0..3 {
        let (mut np, mut nm) = (0u32, 0u32);
        // SAFETY: writes two u32 on this stack frame.
        let e = unsafe { GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut np, &mut nm) };
        if e != ERROR_SUCCESS {
            return Err(format!("GetDisplayConfigBufferSizes: {e:?}"));
        }
        paths = vec![DISPLAYCONFIG_PATH_INFO::default(); np as usize];
        let mut modes = vec![DISPLAYCONFIG_MODE_INFO::default(); nm as usize];
        // SAFETY: the buffers hold exactly np and nm elements, as the counts passed in say.
        let e = unsafe {
            QueryDisplayConfig(QDC_ONLY_ACTIVE_PATHS, &mut np, paths.as_mut_ptr(), &mut nm, modes.as_mut_ptr(), None)
        };
        if e == ERROR_INSUFFICIENT_BUFFER {
            // The topology changed between the two calls; ask for the sizes again.
            continue;
        }
        if e != ERROR_SUCCESS {
            return Err(format!("QueryDisplayConfig: {e:?}"));
        }
        paths.truncate(np as usize);
        done = true;
        break;
    }
    if !done {
        return Err("QueryDisplayConfig kept returning ERROR_INSUFFICIENT_BUFFER".to_string());
    }
    Ok(paths)
}

/// The GDI device name (`\\.\DISPLAYn`) of a path's source.
fn source_gdi_name(p: &DISPLAYCONFIG_PATH_INFO) -> Result<String, String> {
    let mut src = DISPLAYCONFIG_SOURCE_DEVICE_NAME::default();
    src.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME;
    src.header.size = size_of::<DISPLAYCONFIG_SOURCE_DEVICE_NAME>() as u32;
    src.header.adapterId = p.sourceInfo.adapterId;
    src.header.id = p.sourceInfo.id;
    // SAFETY: header.size matches the struct that the header pointer belongs to.
    let e = unsafe { DisplayConfigGetDeviceInfo(&mut src.header) };
    if e != 0 {
        return Err(format!("DisplayConfigGetDeviceInfo(source name) error {e}"));
    }
    Ok(wide(&src.viewGdiDeviceName))
}

/// GDI device name (`\\.\DISPLAYn`) -> monitor model name, through DisplayConfig.
fn friendly_names() -> Result<HashMap<String, String>, String> {
    let paths = active_paths()?;
    let mut names = HashMap::new();
    for p in &paths {
        let gdi = match source_gdi_name(p) {
            Ok(n) => n,
            Err(e) => {
                log::line(format!("monitors: {e}; path skipped"));
                continue;
            }
        };
        let mut tgt = DISPLAYCONFIG_TARGET_DEVICE_NAME::default();
        tgt.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME;
        tgt.header.size = size_of::<DISPLAYCONFIG_TARGET_DEVICE_NAME>() as u32;
        tgt.header.adapterId = p.targetInfo.adapterId;
        tgt.header.id = p.targetInfo.id;
        // SAFETY: as above.
        let e = unsafe { DisplayConfigGetDeviceInfo(&mut tgt.header) };
        if e != 0 {
            log::line(format!("monitors: DisplayConfigGetDeviceInfo(target name) error {e}; path skipped"));
            continue;
        }
        names.insert(gdi, wide(&tgt.monitorFriendlyDeviceName));
    }
    Ok(names)
}

/// `DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO_2` from wingdi.h (SDK 10.0.26100.0, Windows 11 and later),
/// missing from windows 0.62.2. The bitfield union is kept as its `value`.
#[repr(C)]
#[derive(Default)]
struct AdvancedColorInfo2 {
    header: DISPLAYCONFIG_DEVICE_INFO_HEADER,
    value: u32,
    color_encoding: i32,
    bits_per_color_channel: u32,
    active_color_mode: i32,
}

/// `DISPLAYCONFIG_DEVICE_INFO_GET_ADVANCED_COLOR_INFO_2` in wingdi.h.
const GET_ADVANCED_COLOR_INFO_2: DISPLAYCONFIG_DEVICE_INFO_TYPE = DISPLAYCONFIG_DEVICE_INFO_TYPE(15);

/// HDR state and SDR white level of one monitor, as DisplayConfig and DXGI report them right now.
/// Every field is `None` when its query failed (the failure is logged).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ColorState {
    /// GDI device name, e.g. `\\.\DISPLAY1`.
    pub device_name: String,
    /// `IDXGIOutput6::GetDesc1().ColorSpace`.
    pub dxgi_color_space: Option<i32>,
    pub dxgi_bits_per_color: Option<u32>,
    /// `DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO` bitfield: supported 1, enabled 2, wide color enforced 4, force disabled 8.
    pub ac_bits: Option<u32>,
    pub bits_per_channel: Option<u32>,
    /// `DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO_2` bitfield: supported 1, active 2, limited by policy 8,
    /// HDR supported 0x10, HDR user enabled 0x20, wide color supported 0x40, wide color user enabled 0x80.
    pub ac2_bits: Option<u32>,
    /// `activeColorMode`: 0 SDR, 1 WCG, 2 HDR.
    pub ac2_mode: Option<i32>,
    /// `DISPLAYCONFIG_SDR_WHITE_LEVEL.SDRWhiteLevel`: 1000 = 80 nits.
    pub sdr_white_raw: Option<u32>,
}

impl ColorState {
    /// HDR is on: the active color mode says so, or (without it) DXGI reports the ST.2084 color space.
    pub fn hdr_on(&self) -> bool {
        match self.ac2_mode {
            Some(mode) => mode == 2,
            None => self.dxgi_color_space == Some(DXGI_COLOR_SPACE_RGB_FULL_G2084_NONE_P2020.0),
        }
    }

    /// Learn (DISPLAYCONFIG_SDR_WHITE_LEVEL): nits = SDRWhiteLevel / 1000 * 80.
    pub fn sdr_white_nits(&self) -> Option<f64> {
        self.sdr_white_raw.map(|raw| f64::from(raw) / 1000.0 * 80.0)
    }

    pub fn describe(&self) -> String {
        let opt = |v: Option<String>| v.unwrap_or_else(|| "?".to_string());
        let mode = match self.ac2_mode {
            Some(0) => "SDR".to_string(),
            Some(1) => "WCG".to_string(),
            Some(2) => "HDR".to_string(),
            Some(m) => format!("unknown {m}"),
            None => "?".to_string(),
        };
        let cs = match self.dxgi_color_space {
            Some(cs) if cs == DXGI_COLOR_SPACE_RGB_FULL_G2084_NONE_P2020.0 => "G2084_P2020 (HDR)".to_string(),
            Some(cs) if cs == DXGI_COLOR_SPACE_RGB_FULL_G22_NONE_P709.0 => "G22_P709 (SDR)".to_string(),
            Some(cs) => format!("{cs}"),
            None => "?".to_string(),
        };
        format!(
            "HDR {} (activeColorMode {mode}, info2 bits {}, info bits {}, DXGI ColorSpace {cs}, DXGI bits {}, link bits {}), SDR white level {} = {} nits",
            if self.hdr_on() { "ON" } else { "off" },
            opt(self.ac2_bits.map(|b| format!("0x{b:02x}"))),
            opt(self.ac_bits.map(|b| format!("0x{b:x}"))),
            opt(self.dxgi_bits_per_color.map(|b| b.to_string())),
            opt(self.bits_per_channel.map(|b| b.to_string())),
            opt(self.sdr_white_raw.map(|r| r.to_string())),
            opt(self.sdr_white_nits().map(|n| format!("{n:.1}")))
        )
    }
}

/// The color state of every active monitor, keyed by GDI device name. Queried fresh on every call.
pub fn color_states() -> Result<Vec<ColorState>, String> {
    let mut out = Vec::new();
    for p in &active_paths()? {
        let device_name = source_gdi_name(p)?;
        let mut s = ColorState { device_name, ..Default::default() };

        let mut ac = DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO::default();
        ac.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_ADVANCED_COLOR_INFO;
        ac.header.size = size_of::<DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO>() as u32;
        ac.header.adapterId = p.targetInfo.adapterId;
        ac.header.id = p.targetInfo.id;
        // SAFETY: the pointer covers the whole struct that header.size describes.
        let e = unsafe { DisplayConfigGetDeviceInfo((&raw mut ac).cast()) };
        if e == 0 {
            // SAFETY: both union members are a u32 view of the same bits.
            s.ac_bits = Some(unsafe { ac.Anonymous.value });
            s.bits_per_channel = Some(ac.bitsPerColorChannel);
        } else {
            log::line(format!("monitors: {} advanced color info error {e}", s.device_name));
        }

        let mut ac2 = AdvancedColorInfo2::default();
        ac2.header.r#type = GET_ADVANCED_COLOR_INFO_2;
        ac2.header.size = size_of::<AdvancedColorInfo2>() as u32;
        ac2.header.adapterId = p.targetInfo.adapterId;
        ac2.header.id = p.targetInfo.id;
        // SAFETY: AdvancedColorInfo2 mirrors the SDK struct; the pointer covers all of it.
        let e = unsafe { DisplayConfigGetDeviceInfo((&raw mut ac2).cast()) };
        if e == 0 {
            s.ac2_bits = Some(ac2.value);
            s.ac2_mode = Some(ac2.active_color_mode);
        } else {
            log::line(format!("monitors: {} advanced color info 2 error {e}", s.device_name));
        }

        let mut white = DISPLAYCONFIG_SDR_WHITE_LEVEL::default();
        white.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_SDR_WHITE_LEVEL;
        white.header.size = size_of::<DISPLAYCONFIG_SDR_WHITE_LEVEL>() as u32;
        white.header.adapterId = p.targetInfo.adapterId;
        white.header.id = p.targetInfo.id;
        // SAFETY: as above.
        let e = unsafe { DisplayConfigGetDeviceInfo((&raw mut white).cast()) };
        if e == 0 {
            s.sdr_white_raw = Some(white.SDRWhiteLevel);
        } else {
            log::line(format!("monitors: {} SDR white level error {e}", s.device_name));
        }
        out.push(s);
    }

    // SAFETY: plain factory creation; a new factory sees the current outputs.
    let factory: IDXGIFactory1 = unsafe { CreateDXGIFactory1() }.ctx("CreateDXGIFactory1")?;
    for ai in 0u32.. {
        // SAFETY: plain enumeration; NOT_FOUND ends it.
        let adapter = match unsafe { factory.EnumAdapters1(ai) } {
            Ok(a) => a,
            Err(e) if e.code() == DXGI_ERROR_NOT_FOUND => break,
            Err(e) => return Err(format!("EnumAdapters1({ai}): {e}")),
        };
        for oi in 0u32.. {
            // SAFETY: plain enumeration; NOT_FOUND ends it.
            let output = match unsafe { adapter.EnumOutputs(oi) } {
                Ok(o) => o,
                Err(e) if e.code() == DXGI_ERROR_NOT_FOUND => break,
                Err(e) => return Err(format!("EnumOutputs({oi}): {e}")),
            };
            let o6: IDXGIOutput6 = match output.cast() {
                Ok(o) => o,
                Err(e) => {
                    log::line(format!("monitors: output {oi} has no IDXGIOutput6: {e}"));
                    continue;
                }
            };
            // SAFETY: plain query.
            let d1 = unsafe { o6.GetDesc1() }.ctx("IDXGIOutput6::GetDesc1")?;
            let name = wide(&d1.DeviceName);
            if let Some(s) = out.iter_mut().find(|s| s.device_name == name) {
                s.dxgi_color_space = Some(d1.ColorSpace.0);
                s.dxgi_bits_per_color = Some(d1.BitsPerColor);
            }
        }
    }
    Ok(out)
}

struct DxgiOutput {
    adapter: String,
    device_name: String,
    rect: PxRect,
    color_space: Option<i32>,
}

/// Logs every DXGI adapter and output and returns the outputs.
fn dxgi_outputs() -> Result<Vec<DxgiOutput>, String> {
    // SAFETY: plain factory creation.
    let factory: IDXGIFactory1 = unsafe { CreateDXGIFactory1() }.ctx("CreateDXGIFactory1")?;
    let mut out = Vec::new();
    for ai in 0u32.. {
        // SAFETY: plain enumeration; NOT_FOUND ends it.
        let adapter = match unsafe { factory.EnumAdapters1(ai) } {
            Ok(a) => a,
            Err(e) if e.code() == DXGI_ERROR_NOT_FOUND => break,
            Err(e) => return Err(format!("EnumAdapters1({ai}): {e}")),
        };
        // SAFETY: plain query.
        let ad = unsafe { adapter.GetDesc1() }.ctx("IDXGIAdapter1::GetDesc1")?;
        let adapter_name = wide(&ad.Description);
        log::line(format!(
            "adapter {ai}: '{adapter_name}' vendor 0x{:04x} device 0x{:04x} luid {:08x}:{:08x} flags 0x{:x} dedicated video {} MB",
            ad.VendorId,
            ad.DeviceId,
            ad.AdapterLuid.HighPart,
            ad.AdapterLuid.LowPart,
            ad.Flags,
            ad.DedicatedVideoMemory / (1024 * 1024)
        ));
        for oi in 0u32.. {
            // SAFETY: plain enumeration; NOT_FOUND ends it.
            let output = match unsafe { adapter.EnumOutputs(oi) } {
                Ok(o) => o,
                Err(e) if e.code() == DXGI_ERROR_NOT_FOUND => break,
                Err(e) => return Err(format!("EnumOutputs({oi}) on '{adapter_name}': {e}")),
            };
            // SAFETY: plain query.
            let d = unsafe { output.GetDesc() }.ctx("IDXGIOutput::GetDesc")?;
            let name = wide(&d.DeviceName);
            let rect = px(&d.DesktopCoordinates);
            log::line(format!(
                "  output {oi}: {name} DesktopCoordinates {rect} AttachedToDesktop={} Rotation={} ({})",
                d.AttachedToDesktop.as_bool(),
                d.Rotation.0,
                rotation_name(d.Rotation)
            ));
            let mut color_space = None;
            match output.cast::<IDXGIOutput6>() {
                // SAFETY: plain query.
                Ok(o6) => match unsafe { o6.GetDesc1() } {
                    Ok(d1) => {
                        log::line(format!(
                            "    GetDesc1: ColorSpace={} BitsPerColor={} MinLuminance={} MaxLuminance={} MaxFullFrameLuminance={}",
                            d1.ColorSpace.0, d1.BitsPerColor, d1.MinLuminance, d1.MaxLuminance, d1.MaxFullFrameLuminance
                        ));
                        color_space = Some(d1.ColorSpace.0);
                    }
                    Err(e) => log::line(format!("    IDXGIOutput6::GetDesc1: ERROR {e}")),
                },
                Err(e) => log::line(format!("    no IDXGIOutput6: {e}")),
            }
            out.push(DxgiOutput { adapter: adapter_name.clone(), device_name: name, rect, color_space });
        }
    }
    Ok(out)
}

/// Which rounding of `exact` produced `seen` (a deduction from two numbers, not an API fact).
fn rounding(exact: f64, seen: i32) -> String {
    let mut hits = Vec::new();
    if exact.floor() as i32 == seen {
        hits.push("floor");
    }
    if exact.round() as i32 == seen {
        hits.push("round");
    }
    if exact.ceil() as i32 == seen {
        hits.push("ceil");
    }
    if hits.is_empty() { format!("none (seen {seen}, exact {exact:.2})") } else { format!("{}={seen}", hits.join("/")) }
}

/// How two rectangles sit relative to each other: overlap, shared edge, gap, or apart.
fn relation(a: &PxRect, b: &PxRect) -> String {
    let x_ov = a.right.min(b.right) - a.left.max(b.left);
    let y_ov = a.bottom.min(b.bottom) - a.top.max(b.top);
    match (x_ov.signum(), y_ov.signum()) {
        (1, 1) => format!("OVERLAP {x_ov}x{y_ov} px"),
        (0, 1) => format!("touch on a vertical edge (shared span {y_ov} px)"),
        (1, 0) => format!("touch on a horizontal edge (shared span {x_ov} px)"),
        (-1, 1) => format!("HOLE: horizontal gap {} px (vertical span shared {y_ov} px)", -x_ov),
        (1, -1) => format!("HOLE: vertical gap {} px (horizontal span shared {x_ov} px)", -y_ov),
        _ => format!("no shared edge (x overlap {x_ov}, y overlap {y_ov})"),
    }
}
