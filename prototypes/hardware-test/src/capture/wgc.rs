//! Windows.Graphics.Capture of one monitor, with the default border and with the border-off request,
//! plus the objective check for a border window (top-level windows that appear during the capture).

use std::collections::HashSet;
use std::time::Duration;

use windows::Graphics::Capture::{
    Direct3D11CaptureFramePool, GraphicsCaptureAccess, GraphicsCaptureAccessKind, GraphicsCaptureItem,
    GraphicsCaptureSession,
};
use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Security::Authorization::AppCapabilityAccess::AppCapabilityAccessStatus;
use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, RECT};
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use windows::Win32::Graphics::Direct3D11::{D3D11_TEXTURE2D_DESC, ID3D11Texture2D};
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::Win32::System::WinRT::Direct3D11::{CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess};
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GWL_EXSTYLE, GetClassNameW, GetWindowLongW, GetWindowRect, GetWindowTextW, GetWindowThreadProcessId,
    IsWindow, IsWindowVisible, WS_EX_TOPMOST,
};
use windows::core::{BOOL, Interface, PWSTR};

use windows::Win32::Graphics::Gdi::SRCCOPY;

use super::{create_device, gdi_rect, pump_messages, read_bgra};
use crate::common::{CaptureMethod, Ctx, MonitorInfo, Photo, PxRect};
use crate::log;
use crate::monitors::{px, short_name, wide};

/// How long to poll TryGetNextFrame after StartCapture.
const FIRST_FRAME_LIMIT_MS: f64 = 2000.0;
/// How long to wait for RequestAccessAsync before giving up (it may be waiting on a consent prompt).
const ACCESS_LIMIT_MS: f64 = 10_000.0;
/// How often to look for new top-level windows during the hold.
const SCAN_EVERY_MS: f64 = 250.0;

/// The WGC photo, plus a GDI grab of the same monitor taken while the session was open (the border check).
pub struct WgcShot {
    pub photo: Photo,
    pub during: Option<Photo>,
}

/// One monitor through Windows.Graphics.Capture. `borderless` asks for the border-off access first.
/// The session stays open for `hold_secs` so a person can look for the yellow border.
pub fn capture_monitor(m: &MonitorInfo, borderless: bool, hold_secs: u64) -> Result<WgcShot, String> {
    let variant = if borderless { "wgc-borderless" } else { "wgc-border" };
    let label = short_name(&m.device_name);
    // Taken before anything WGC exists, so every window the capture creates shows up as new.
    let before: HashSet<isize> = top_hwnds()?.into_iter().collect();
    // The screen before the capture, to tell a border apart from yellow content that was already there.
    let baseline = match gdi_rect(&m.device_name, m.rect, SRCCOPY) {
        Ok(p) => Some(p),
        Err(e) => {
            log::line(format!("{variant} {label}: baseline GDI grab ERROR {e}; border-pixel check skipped"));
            None
        }
    };
    let t0 = log::now_ms();
    match GraphicsCaptureSession::IsSupported() {
        Ok(v) => log::line(format!("{variant} {label}: GraphicsCaptureSession::IsSupported = {v}")),
        Err(e) => log::line(format!("{variant} {label}: GraphicsCaptureSession::IsSupported ERROR {e}")),
    }
    let interop = windows::core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()
        .ctx("IGraphicsCaptureItemInterop factory")?;
    // SAFETY: the monitor handle came from EnumDisplayMonitors in this process.
    let item: GraphicsCaptureItem = unsafe { interop.CreateForMonitor(m.hmonitor) }.ctx("CreateForMonitor")?;
    let size = item.Size().ctx("GraphicsCaptureItem::Size")?;
    let s = m.scale();
    let unit = if size.Width == m.rect.width() && size.Height == m.rect.height() {
        "equals rcMonitor: physical pixels"
    } else if (f64::from(size.Width) - f64::from(m.rect.width()) / s).abs() < 1.0
        && (f64::from(size.Height) - f64::from(m.rect.height()) / s).abs() < 1.0
    {
        "equals rcMonitor / scale: DIPs"
    } else {
        "matches neither physical pixels nor DIPs"
    };
    let size_line = format!(
        "wgc-item-size {label}: Size() {}x{} vs rcMonitor {}x{} at {:.0}% -> {unit}",
        size.Width,
        size.Height,
        m.rect.width(),
        m.rect.height(),
        s * 100.0
    );
    // Both variants measure the same thing; one RESULT line per monitor is enough.
    log::line(if borderless { size_line } else { format!("RESULT {size_line}") });

    let gpu = create_device(None, D3D_DRIVER_TYPE_HARDWARE)?;
    let dxgi: IDXGIDevice = gpu.device.cast().ctx("ID3D11Device as IDXGIDevice")?;
    // SAFETY: plain interop call on a live DXGI device.
    let d3d: IDirect3DDevice = unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi) }
        .ctx("CreateDirect3D11DeviceFromDXGIDevice")?
        .cast()
        .ctx("IDirect3DDevice")?;
    let pool =
        Direct3D11CaptureFramePool::CreateFreeThreaded(&d3d, DirectXPixelFormat::B8G8R8A8UIntNormalized, 1, size)
            .ctx("Direct3D11CaptureFramePool::CreateFreeThreaded")?;
    let session = pool.CreateCaptureSession(&item).ctx("CreateCaptureSession")?;
    let closer = CloseOnDrop { session: session.clone(), pool: pool.clone() };
    match session.SetIsCursorCaptureEnabled(false) {
        Ok(()) => log::line(format!("{variant} {label}: SetIsCursorCaptureEnabled(false) OK")),
        Err(e) => log::line(format!("{variant} {label}: SetIsCursorCaptureEnabled(false) ERROR {e}")),
    }
    if borderless {
        let status = request_borderless();
        log::line(format!("RESULT wgc-borderless-access {label}: RequestAccessAsync(Borderless) -> {status}"));
        match session.SetIsBorderRequired(false) {
            Ok(()) => log::line(format!(
                "RESULT wgc-borderless-setter {label}: SetIsBorderRequired(false) returned OK (Learn: without consent it succeeds but is ignored, so this alone proves nothing)"
            )),
            Err(e) => log::line(format!(
                "RESULT wgc-borderless-setter {label}: SetIsBorderRequired(false) ERROR {e} (0x{:08x})",
                e.code().0 as u32
            )),
        }
    }
    match session.IsBorderRequired() {
        Ok(v) => log::line(format!("{variant} {label}: IsBorderRequired reads back {v}")),
        Err(e) => log::line(format!("{variant} {label}: IsBorderRequired ERROR {e}")),
    }
    let setup_ms = log::now_ms() - t0;

    session.StartCapture().ctx("StartCapture")?;
    let started = log::now_ms();
    let frame = loop {
        match pool.TryGetNextFrame() {
            Ok(f) => break f,
            // A null frame comes back as an error with a success code: nothing yet.
            Err(e) if e.code().is_ok() => {}
            Err(e) => return Err(format!("TryGetNextFrame: {e}")),
        }
        if log::now_ms() - started > FIRST_FRAME_LIMIT_MS {
            return Err(format!("no frame within {FIRST_FRAME_LIMIT_MS} ms of StartCapture"));
        }
        pump_messages();
        std::thread::sleep(Duration::from_millis(1));
    };
    let taken = log::now_ms();
    let content = frame.ContentSize().ctx("ContentSize")?;
    let surface = frame.Surface().ctx("Surface")?;
    let access: IDirect3DDxgiInterfaceAccess = surface.cast().ctx("IDirect3DDxgiInterfaceAccess")?;
    // SAFETY: plain interface query on a live surface.
    let tex: ID3D11Texture2D = unsafe { access.GetInterface() }.ctx("surface as ID3D11Texture2D")?;
    let mut desc = D3D11_TEXTURE2D_DESC::default();
    // SAFETY: fills one struct on this stack frame.
    unsafe { tex.GetDesc(&mut desc) };
    let copy_start = log::now_ms();
    let mut staging = None;
    let crop = (u32::try_from(content.Width).unwrap_or(0), u32::try_from(content.Height).unwrap_or(0));
    let (width, height, bgra) = read_bgra(&gpu, &mut staging, &tex, Some(crop))?;
    let copy_ms = log::now_ms() - copy_start;
    if let Err(e) = frame.Close() {
        log::line(format!("{variant} {label}: frame Close ERROR {e}"));
    }
    log::line(format!(
        "RESULT wgc-sizes {variant} {label}: item {}x{}, ContentSize {}x{}, texture {}x{}, photo {width}x{height}, rcMonitor {}x{}",
        size.Width,
        size.Height,
        content.Width,
        content.Height,
        desc.Width,
        desc.Height,
        m.rect.width(),
        m.rect.height()
    ));
    log::line(format!(
        "RESULT wgc-time {variant} {label}: setup {setup_ms:.3} ms, first frame {:.3} ms after StartCapture, copy {copy_ms:.3} ms, total {:.3} ms (hold excluded)",
        taken - started,
        log::now_ms() - t0
    ));

    log::line(format!(
        "HOLD {variant} {label}: session open for {hold_secs} s; look for a yellow border around {label}"
    ));
    let (new_windows, during) = hold(m, hold_secs, &before, variant)?;
    drop(closer);
    match (&baseline, &during) {
        (Some(b), Some(d)) => match edge_yellow(b, d) {
            Ok(s) => log::line(format!("RESULT wgc-border-pixels {variant} {label}: {s}")),
            Err(e) => log::line(format!("RESULT wgc-border-pixels {variant} {label}: ERROR {e}")),
        },
        _ => log::line(format!("RESULT wgc-border-pixels {variant} {label}: skipped, a GDI grab is missing")),
    }
    let border_like = new_windows.iter().filter(|w| w.visible && encloses(&w.rect, m)).count();
    if new_windows.is_empty() {
        log::line(format!(
            "RESULT wgc-border-window {variant} {label}: no new top-level window during the capture; a border, if drawn, is not a top-level window and needs eyes"
        ));
    } else {
        // Give the system a moment to tear down whatever the session created, then see what is left.
        let until = log::now_ms() + 300.0;
        while log::now_ms() < until {
            pump_messages();
            std::thread::sleep(Duration::from_millis(10));
        }
        let fates: Vec<String> = new_windows
            .iter()
            .map(|w| {
                // SAFETY: IsWindow accepts any handle value, including stale ones.
                let alive = unsafe { IsWindow(Some(HWND(w.hwnd as *mut core::ffi::c_void))) }.as_bool();
                format!(
                    "class '{}' exe {} {} visible {} -> {}",
                    w.class,
                    w.exe,
                    rect_text(&w.rect),
                    w.visible,
                    if alive { "still exists after Close" } else { "gone after Close" }
                )
            })
            .collect();
        log::line(format!(
            "RESULT wgc-border-window {variant} {label}: {} new top-level window(s), {border_like} border-like (visible and enclosing the monitor); other apps can create windows too: {}",
            new_windows.len(),
            fates.join("; ")
        ));
    }

    Ok(WgcShot {
        photo: Photo {
            device_name: m.device_name.clone(),
            rect: m.rect,
            width,
            height,
            bgra,
            method: CaptureMethod::Wgc,
            taken_ms: taken,
        },
        during,
    })
}

/// Width of the edge band, in physical pixels, where a capture border would be drawn.
const BAND_PX: u32 = 8;

/// Strong yellow in B, G, R order: the capture border is yellow, desktop content at the edges rarely is.
fn yellowish(p: &[u8]) -> bool {
    p[2] >= 200 && p[1] >= 150 && p[0] <= 90
}

/// Counts edge-band pixels that are yellow-ish in `during` but were not in `before`.
fn edge_yellow(before: &Photo, during: &Photo) -> Result<String, String> {
    if before.width != during.width || before.height != during.height {
        return Err(format!(
            "sizes differ, {}x{} vs {}x{}",
            before.width, before.height, during.width, during.height
        ));
    }
    let (w, h) = (during.width, during.height);
    let band = BAND_PX.min(w / 2).min(h / 2);
    let (mut total, mut newly, mut already) = (0u64, 0u64, 0u64);
    let mut first: Option<(u32, u32, String)> = None;
    for y in 0..h {
        for x in 0..w {
            if x >= band && y >= band && x < w - band && y < h - band {
                continue;
            }
            total += 1;
            let i = ((y * w + x) * 4) as usize;
            let (d, b) = (&during.bgra[i..i + 3], &before.bgra[i..i + 3]);
            if yellowish(d) {
                if yellowish(b) {
                    already += 1;
                } else {
                    newly += 1;
                    if first.is_none() {
                        first = Some((x, y, format!("#{:02X}{:02X}{:02X}", d[2], d[1], d[0])));
                    }
                }
            }
        }
    }
    Ok(format!(
        "GDI grab during the capture, {band}-px edge band of {total} pixels: {newly} turned yellow-ish (R>=200 G>=150 B<=90), {already} were yellow-ish before; first new {} (grabs {:.0} ms apart)",
        first.map_or("none".to_string(), |(x, y, c)| format!("at ({x},{y}) {c}")),
        during.taken_ms - before.taken_ms
    ))
}

/// True when `r` covers the whole monitor, give or take 20 px (a border drawn around it would).
fn encloses(r: &Result<PxRect, String>, m: &MonitorInfo) -> bool {
    match r {
        Ok(r) => {
            r.left <= m.rect.left + 20
                && r.top <= m.rect.top + 20
                && r.right >= m.rect.right - 20
                && r.bottom >= m.rect.bottom - 20
        }
        Err(_) => false,
    }
}

fn rect_text(r: &Result<PxRect, String>) -> String {
    match r {
        Ok(r) => r.to_string(),
        Err(e) => format!("(GetWindowRect: {e})"),
    }
}

/// Closes the capture session and frame pool on every path.
struct CloseOnDrop {
    session: GraphicsCaptureSession,
    pool: Direct3D11CaptureFramePool,
}

impl Drop for CloseOnDrop {
    fn drop(&mut self) {
        if let Err(e) = self.session.Close() {
            log::line(format!("ERROR: GraphicsCaptureSession::Close: {e}"));
        }
        if let Err(e) = self.pool.Close() {
            log::line(format!("ERROR: Direct3D11CaptureFramePool::Close: {e}"));
        }
    }
}

/// Keeps the session open for `hold_secs`, logging every top-level window that was not in `before`,
/// and grabs the monitor through GDI once midway (or right away when there is no hold).
fn hold(
    m: &MonitorInfo,
    hold_secs: u64,
    before: &HashSet<isize>,
    variant: &str,
) -> Result<(Vec<TopWindow>, Option<Photo>), String> {
    let label = short_name(&m.device_name);
    let start = log::now_ms();
    let hold_ms = hold_secs as f64 * 1000.0;
    let end = start + hold_ms;
    let grab_at = start + (hold_ms / 2.0).min(500.0);
    let mut during = None;
    let mut grabbed = false;
    let mut seen: HashSet<isize> = HashSet::new();
    let mut found = Vec::new();
    let mut next_scan = log::now_ms();
    loop {
        let now = log::now_ms();
        if !grabbed && now >= grab_at {
            grabbed = true;
            match gdi_rect(&m.device_name, m.rect, SRCCOPY) {
                Ok(p) => {
                    log::line(format!(
                        "{variant} {label}: GDI grab during the capture, {:.0} ms into the hold",
                        p.taken_ms - start
                    ));
                    during = Some(p);
                }
                Err(e) => log::line(format!("{variant} {label}: GDI grab during the capture ERROR {e}")),
            }
        }
        if now >= next_scan {
            for hwnd in top_hwnds()? {
                if !before.contains(&hwnd) && seen.insert(hwnd) {
                    let w = describe(HWND(hwnd as *mut core::ffi::c_void));
                    log::line(format!(
                        "{variant} {label}: NEW top-level window hwnd 0x{:x} class '{}' title '{}' exe {} rect {} visible {} topmost {}",
                        w.hwnd,
                        w.class,
                        w.title,
                        w.exe,
                        rect_text(&w.rect),
                        w.visible,
                        w.topmost
                    ));
                    found.push(w);
                }
            }
            next_scan = now + SCAN_EVERY_MS;
        }
        if now >= end {
            break;
        }
        pump_messages();
        std::thread::sleep(Duration::from_millis(10));
    }
    Ok((found, during))
}

/// Asks for the border-off capture access, polling instead of blocking so a pending consent prompt
/// cannot hang the test. Returns a description of the outcome.
fn request_borderless() -> String {
    let t0 = log::now_ms();
    let op = match GraphicsCaptureAccess::RequestAccessAsync(GraphicsCaptureAccessKind::Borderless) {
        Ok(op) => op,
        Err(e) => return format!("call failed: {e} (0x{:08x})", e.code().0 as u32),
    };
    loop {
        let status = match op.Status() {
            Ok(s) => s,
            Err(e) => return format!("Status() failed: {e}"),
        };
        // AsyncStatus: 0 Started, 1 Completed, 2 Canceled, 3 Error. The type lives in windows-future,
        // which the windows crate does not re-export, so the raw value is compared.
        match status.0 {
            0 => {
                if log::now_ms() - t0 > ACCESS_LIMIT_MS {
                    let cancel = match op.Cancel() {
                        Ok(()) => "cancelled".to_string(),
                        Err(e) => format!("Cancel failed: {e}"),
                    };
                    return format!(
                        "still pending after {ACCESS_LIMIT_MS} ms (a consent prompt may be waiting); {cancel}"
                    );
                }
                pump_messages();
                std::thread::sleep(Duration::from_millis(5));
            }
            1 => {
                return match op.GetResults() {
                    Ok(s) => format!("{} {} after {:.1} ms", s.0, access_name(s), log::now_ms() - t0),
                    Err(e) => format!("completed but GetResults failed: {e}"),
                };
            }
            other => {
                let code = match op.ErrorCode() {
                    Ok(c) => format!("0x{:08x}", c.0 as u32),
                    Err(e) => format!("ErrorCode() failed: {e}"),
                };
                return format!("ended with AsyncStatus {other} (2 Canceled, 3 Error), error {code}");
            }
        }
    }
}

fn access_name(s: AppCapabilityAccessStatus) -> &'static str {
    match s.0 {
        0 => "DeniedBySystem",
        1 => "NotDeclaredByApp",
        2 => "DeniedByUser",
        3 => "UserPromptRequired",
        4 => "Allowed",
        _ => "unknown",
    }
}

struct TopWindow {
    hwnd: isize,
    class: String,
    title: String,
    exe: String,
    rect: Result<PxRect, String>,
    visible: bool,
    topmost: bool,
}

/// Every top-level window handle, as integers (comparable across snapshots).
fn top_hwnds() -> Result<Vec<isize>, String> {
    unsafe extern "system" fn push(h: HWND, data: LPARAM) -> BOOL {
        // SAFETY: `data` is the `&mut Vec` passed below, alive for the whole EnumWindows call.
        let list = unsafe { &mut *(data.0 as *mut Vec<isize>) };
        list.push(h.0 as isize);
        true.into()
    }
    let mut hwnds: Vec<isize> = Vec::new();
    // SAFETY: the callback only touches `hwnds`, which outlives the call.
    unsafe { EnumWindows(Some(push), LPARAM(&mut hwnds as *mut Vec<isize> as isize)) }.ctx("EnumWindows")?;
    Ok(hwnds)
}

fn describe(h: HWND) -> TopWindow {
    let mut class = [0u16; 256];
    let mut title = [0u16; 256];
    let mut rect = RECT::default();
    let mut pid = 0u32;
    // SAFETY: plain queries on a window handle; the buffers and out values live on this stack frame.
    // GetWindowThreadProcessId's return (the thread id) is not needed; when it fails `pid` stays 0 and
    // exe_name reports the OpenProcess error instead.
    let (nc, nt, rect_ok, visible, ex, _thread) = unsafe {
        (
            GetClassNameW(h, &mut class),
            GetWindowTextW(h, &mut title),
            GetWindowRect(h, &mut rect),
            IsWindowVisible(h).as_bool(),
            GetWindowLongW(h, GWL_EXSTYLE),
            GetWindowThreadProcessId(h, Some(&mut pid)),
        )
    };
    TopWindow {
        hwnd: h.0 as isize,
        class: if nc > 0 { wide(&class) } else { "(GetClassNameW failed)".to_string() },
        title: if nt > 0 { wide(&title) } else { String::new() },
        exe: exe_name(pid),
        rect: rect_ok.map(|()| px(&rect)).map_err(|e| e.to_string()),
        visible,
        topmost: (ex as u32) & WS_EX_TOPMOST.0 != 0,
    }
}

fn exe_name(pid: u32) -> String {
    // SAFETY: opens a query-only handle; it is closed below.
    let h = match unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) } {
        Ok(h) => h,
        Err(e) => return format!("pid {pid} (OpenProcess: {e})"),
    };
    let mut buf = [0u16; 1024];
    let mut len = buf.len() as u32;
    // SAFETY: `buf` holds `len` UTF-16 units and outlives the call.
    let r = unsafe { QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len) };
    // SAFETY: closes the handle opened above.
    if let Err(e) = unsafe { CloseHandle(h) } {
        log::line(format!("ERROR: CloseHandle(process {pid}): {e}"));
    }
    match r {
        Ok(()) => {
            let path = String::from_utf16_lossy(&buf[..len as usize]);
            let name = path.rsplit('\\').next().unwrap_or(&path).to_string();
            format!("{name} (pid {pid})")
        }
        Err(e) => format!("pid {pid} (QueryFullProcessImageNameW: {e})"),
    }
}
