//! Builder A. The `measure` mode: an element of known size on each monitor, captured and measured.

use std::time::Duration;

use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::DwmFlush;
use windows::Win32::Graphics::Gdi::{
    BeginPaint, ClientToScreen, CreateSolidBrush, DeleteObject, EndPaint, FillRect, HDC, PAINTSTRUCT, UpdateWindow,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::WindowsProgramming::MulDiv;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect, GetWindowRect, IDC_ARROW, LoadCursorW,
    RegisterClassExW, SW_SHOWNOACTIVATE, SetCursorPos, ShowWindow, UnregisterClassW, WM_ERASEBKGND, WM_PAINT,
    WNDCLASSEXW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::{PCWSTR, w};

use crate::capture::{self, Defer, pump_messages};
use crate::common::{Args, Ctx, MonitorInfo, Photo, PxRect};
use crate::log;
use crate::monitors::{self, cursor_pair, px, short_name};

const CLASS: PCWSTR = w!("hwtest-measure");
/// Window size, physical pixels.
const WIN_W: i32 = 600;
const WIN_H: i32 = 400;
/// Rectangle (1): fixed physical size, client coordinates.
const RECT1: RECT = RECT { left: 20, top: 20, right: 320, bottom: 220 };
/// Rectangle (2): DIP size, placed below (1); fits the window up to 300% scale.
const RECT2_DIP: (i32, i32) = (100, 50);
const RECT2_ORIGIN: (i32, i32) = (20, 240);
/// COLORREF is 0x00BBGGRR.
const WHITE: COLORREF = COLORREF(0x00FF_FFFF);
const BLUE: COLORREF = COLORREF(0x00FF_901E); // #1E90FF
const ORANGE: COLORREF = COLORREF(0x0000_8CFF); // #FF8C00

pub fn measure_mode(_args: &Args) -> Result<(), String> {
    let t_start = log::now_ms();
    let monitors = monitors::enumerate()?;
    log::line(format!(
        "rect (1): {}x{} physical px #1E90FF; rect (2): {}x{} DIPs #FF8C00 scaled as MulDiv(v, GetDpiForWindow, 96), which rounds to nearest with .5 away from zero",
        RECT1.right - RECT1.left,
        RECT1.bottom - RECT1.top,
        RECT2_DIP.0,
        RECT2_DIP.1
    ));

    // SAFETY: the module handle of this exe; no reference count to manage.
    let hinst: HINSTANCE = unsafe { GetModuleHandleW(None) }.ctx("GetModuleHandleW")?.into();
    let wc = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: Some(wndproc),
        hInstance: hinst,
        // SAFETY: loads a system cursor.
        hCursor: unsafe { LoadCursorW(None, IDC_ARROW) }.ctx("LoadCursorW")?,
        lpszClassName: CLASS,
        ..Default::default()
    };
    // SAFETY: `wc` is fully initialized and the class name is a static string.
    if unsafe { RegisterClassExW(&wc) } == 0 {
        return Err(format!("RegisterClassExW: {}", windows::core::Error::from_thread()));
    }
    let _unregister = Defer(|| {
        // SAFETY: the class registered above; every window of it is destroyed before this runs.
        if let Err(e) = unsafe { UnregisterClassW(CLASS, Some(hinst)) } {
            log::line(format!("ERROR: UnregisterClassW: {e}"));
        }
    });

    let mut windows: Vec<(HWND, &MonitorInfo)> = Vec::new();
    let destroy = |list: &mut Vec<(HWND, &MonitorInfo)>| {
        for (h, _) in list.drain(..) {
            // SAFETY: the window was created on this thread.
            if let Err(e) = unsafe { DestroyWindow(h) } {
                log::line(format!("ERROR: DestroyWindow: {e}"));
            }
        }
    };
    let result = (|| -> Result<(), String> {
        for m in &monitors {
            let x = m.rect.left + (m.rect.width() - WIN_W) / 2;
            let y = m.rect.top + (m.rect.height() - WIN_H) / 2;
            // SAFETY: the class is registered; created directly at its final position on the target
            // monitor, so it starts at that monitor's DPI and gets no WM_DPICHANGED.
            let h = unsafe {
                CreateWindowExW(
                    WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                    CLASS,
                    w!("hwtest measure"),
                    WS_POPUP,
                    x,
                    y,
                    WIN_W,
                    WIN_H,
                    None,
                    None,
                    Some(hinst),
                    None,
                )
            }
            .ctx("CreateWindowExW")?;
            windows.push((h, m));
            // SAFETY: plain calls on the window just created.
            unsafe {
                // ShowWindow returns the previous visibility, not an error.
                let _ = ShowWindow(h, SW_SHOWNOACTIVATE);
                if !UpdateWindow(h).as_bool() {
                    log::line(format!("{}: UpdateWindow failed", m.device_name));
                }
            }
        }
        pump_messages();
        for i in 0..2 {
            // SAFETY: plain call; waits for the next composition.
            if let Err(e) = unsafe { DwmFlush() } {
                log::line(format!("DwmFlush #{}: ERROR {e}", i + 1));
            }
            pump_messages();
        }
        let until = log::now_ms() + 100.0;
        while log::now_ms() < until {
            pump_messages();
            std::thread::sleep(Duration::from_millis(5));
        }
        log::line(format!("windows on screen {:.1} ms after start", log::now_ms() - t_start));

        let mut placed = Vec::new();
        for (h, m) in &windows {
            let p = placement(*h)?;
            let label = short_name(&m.device_name);
            let ok = p.client.width() == WIN_W && p.client.height() == WIN_H && p.dpi == m.dpi;
            log::line(format!(
                "RESULT measure-window {label}: window {} client {}x{} origin ({},{}), GetDpiForWindow {} vs monitor {} -> {}",
                p.window,
                p.client.width(),
                p.client.height(),
                p.origin.x,
                p.origin.y,
                p.dpi,
                m.dpi,
                if ok { "OK" } else { "MISMATCH" }
            ));
            placed.push((*m, p));
        }

        let frozen = capture::frozen_photo()?;
        let mut gdi = Vec::new();
        for m in &monitors {
            gdi.push(capture::gdi_monitor(m));
        }

        for (i, (m, p)) in placed.iter().enumerate() {
            let label = short_name(&m.device_name);
            let (w2, h2) = rect2_size(p.dpi);
            let s = f64::from(p.dpi) / 96.0;
            let trunc = (RECT2_DIP.0 * p.dpi as i32 / 96, RECT2_DIP.1 * p.dpi as i32 / 96);
            log::line(format!(
                "{label}: rect (2) at {:.0}%: exact {:.2}x{:.2}, MulDiv {w2}x{h2} (drawn), truncation would give {}x{}",
                s * 100.0,
                f64::from(RECT2_DIP.0) * s,
                f64::from(RECT2_DIP.1) * s,
                trunc.0,
                trunc.1
            ));
            let r2 = rect2(p.dpi);
            let targets = [("rect1-300x200px", RECT1, [0xFF, 0x90, 0x1E]), ("rect2-100x50dip", r2, [0x00, 0x8C, 0xFF])];
            let mut shots: Vec<(&str, &Photo)> = Vec::new();
            match frozen.iter().find(|ph| ph.device_name == m.device_name) {
                Some(ph) => shots.push(("frozen", ph)),
                None => log::line(format!("{label}: no frozen photo for this monitor")),
            }
            match &gdi[i] {
                Ok(ph) => shots.push(("gdi", ph)),
                Err(e) => log::line(format!("{label}: gdi capture ERROR {e}")),
            }
            for (method, photo) in &shots {
                for (name, client, bgr) in &targets {
                    let expected = PxRect {
                        left: p.origin.x + client.left,
                        top: p.origin.y + client.top,
                        right: p.origin.x + client.right,
                        bottom: p.origin.y + client.bottom,
                    };
                    let cx = (expected.left + expected.right) / 2;
                    let cy = (expected.top + expected.bottom) / 2;
                    match scan(photo, cx, cy) {
                        Ok(f) => {
                            let size_ok = f.rect.width() == expected.width() && f.rect.height() == expected.height();
                            let pos_ok = f.rect == expected;
                            let color_ok = f.color == *bgr;
                            log::line(format!(
                                "RESULT measure {label} {name} {method} ({:?}): expected {}x{} at ({},{}) {}, measured {}x{} at ({},{}) {}, scaled {:.2}x{:.2} ({}x{}) -> {}",
                                photo.method,
                                expected.width(),
                                expected.height(),
                                expected.left,
                                expected.top,
                                hex(*bgr),
                                f.rect.width(),
                                f.rect.height(),
                                f.rect.left,
                                f.rect.top,
                                hex(f.color),
                                f64::from(f.rect.width()) / s,
                                f64::from(f.rect.height()) / s,
                                (f64::from(f.rect.width()) / s).round(),
                                (f64::from(f.rect.height()) / s).round(),
                                verdict(size_ok, pos_ok, color_ok)
                            ));
                        }
                        Err(e) => log::line(format!("RESULT measure {label} {name} {method}: ERROR {e}")),
                    }
                }
            }
            match frozen.iter().find(|ph| ph.device_name == m.device_name) {
                Some(ph) => cursor_check(label, p, ph)?,
                None => log::line(format!("RESULT cursor {label}: skipped, no frozen photo")),
            }
        }
        Ok(())
    })();
    destroy(&mut windows);
    pump_messages();
    let total = log::now_ms() - t_start;
    log::line(format!(
        "RESULT measure-duration: {total:.1} ms -> {}",
        if total < 5000.0 { "under 5 s" } else { "OVER 5 s" }
    ));
    result
}

struct Placement {
    window: PxRect,
    client: PxRect,
    /// Client (0,0) on the virtual screen, physical pixels.
    origin: POINT,
    dpi: u32,
}

fn placement(h: HWND) -> Result<Placement, String> {
    let mut wr = RECT::default();
    let mut cr = RECT::default();
    let mut origin = POINT::default();
    // SAFETY: plain queries on a live window; the out values live on this stack frame.
    unsafe {
        GetWindowRect(h, &mut wr).ctx("GetWindowRect")?;
        GetClientRect(h, &mut cr).ctx("GetClientRect")?;
        if !ClientToScreen(h, &mut origin).as_bool() {
            return Err("ClientToScreen failed".to_string());
        }
    }
    // SAFETY: plain query on a live window.
    let dpi = unsafe { GetDpiForWindow(h) };
    Ok(Placement { window: px(&wr), client: px(&cr), origin, dpi })
}

/// Moves the cursor to the center of rect (1), reads it back, checks the frozen photo there, restores it.
fn cursor_check(label: &str, p: &Placement, photo: &Photo) -> Result<(), String> {
    let (saved, _) = cursor_pair()?;
    let _restore = Defer(|| {
        // SAFETY: plain call; puts the cursor back where the owner left it.
        match unsafe { SetCursorPos(saved.x, saved.y) } {
            Ok(()) => log::line(format!("cursor restored to ({},{})", saved.x, saved.y)),
            Err(e) => log::line(format!("ERROR: restoring the cursor to ({},{}): {e}", saved.x, saved.y)),
        }
    });
    let tx = p.origin.x + (RECT1.left + RECT1.right) / 2;
    let ty = p.origin.y + (RECT1.top + RECT1.bottom) / 2;
    // SAFETY: plain call; Per-Monitor V2, so these are physical coordinates.
    unsafe { SetCursorPos(tx, ty) }.ctx("SetCursorPos")?;
    let (logical, physical) = cursor_pair()?;
    let at = |pt: POINT| match pixel(photo, pt.x, pt.y) {
        Some(c) => hex(c),
        None => "outside the photo".to_string(),
    };
    let under = at(logical);
    let ok = logical.x == tx && logical.y == ty && physical == logical && under == "#1E90FF";
    log::line(format!(
        "RESULT cursor {label}: SetCursorPos({tx},{ty}) -> GetCursorPos ({},{}) GetPhysicalCursorPos ({},{}); frozen-photo pixel at GetCursorPos {under}, at GetPhysicalCursorPos {} (expected #1E90FF) -> {}",
        logical.x,
        logical.y,
        physical.x,
        physical.y,
        at(physical),
        if ok { "OK" } else { "MISMATCH" }
    ));
    Ok(())
}

fn verdict(size_ok: bool, pos_ok: bool, color_ok: bool) -> String {
    if size_ok && pos_ok && color_ok {
        return "OK".to_string();
    }
    let mut what = Vec::new();
    if !size_ok {
        what.push("size");
    }
    if !pos_ok {
        what.push("position");
    }
    if !color_ok {
        what.push("color");
    }
    format!("MISMATCH ({})", what.join(", "))
}

fn rect2_size(dpi: u32) -> (i32, i32) {
    let d = i32::try_from(dpi).unwrap_or(96);
    // SAFETY: pure arithmetic.
    unsafe { (MulDiv(RECT2_DIP.0, d, 96), MulDiv(RECT2_DIP.1, d, 96)) }
}

fn rect2(dpi: u32) -> RECT {
    let (w, h) = rect2_size(dpi);
    RECT { left: RECT2_ORIGIN.0, top: RECT2_ORIGIN.1, right: RECT2_ORIGIN.0 + w, bottom: RECT2_ORIGIN.1 + h }
}

struct Found {
    /// Virtual-screen physical pixels.
    rect: PxRect,
    /// B, G, R of the starting pixel.
    color: [u8; 3],
}

/// From the center outward along its row and column until the color differs (tolerance 0), the way
/// Pixmeter's Dimensions tool measures. `(cx, cy)` are virtual-screen physical pixels.
fn scan(photo: &Photo, cx: i32, cy: i32) -> Result<Found, String> {
    let (x0, y0) = (cx - photo.rect.left, cy - photo.rect.top);
    let (w, h) = (photo.width as i32, photo.height as i32);
    if x0 < 0 || y0 < 0 || x0 >= w || y0 >= h {
        return Err(format!("center ({cx},{cy}) is outside the photo {}", photo.rect));
    }
    let at = |x: i32, y: i32| {
        let i = ((y * w + x) * 4) as usize;
        [photo.bgra[i], photo.bgra[i + 1], photo.bgra[i + 2]]
    };
    let c = at(x0, y0);
    let (mut l, mut r, mut t, mut b) = (x0, x0, y0, y0);
    while l > 0 && at(l - 1, y0) == c {
        l -= 1;
    }
    while r + 1 < w && at(r + 1, y0) == c {
        r += 1;
    }
    while t > 0 && at(x0, t - 1) == c {
        t -= 1;
    }
    while b + 1 < h && at(x0, b + 1) == c {
        b += 1;
    }
    Ok(Found {
        rect: PxRect {
            left: l + photo.rect.left,
            top: t + photo.rect.top,
            right: r + 1 + photo.rect.left,
            bottom: b + 1 + photo.rect.top,
        },
        color: c,
    })
}

/// B, G, R at a virtual-screen point, or None outside the photo.
fn pixel(photo: &Photo, x: i32, y: i32) -> Option<[u8; 3]> {
    let (px_, py) = (x - photo.rect.left, y - photo.rect.top);
    if px_ < 0 || py < 0 || px_ >= photo.width as i32 || py >= photo.height as i32 {
        return None;
    }
    let i = ((py * photo.width as i32 + px_) * 4) as usize;
    Some([photo.bgra[i], photo.bgra[i + 1], photo.bgra[i + 2]])
}

/// B, G, R as `#RRGGBB`.
fn hex(c: [u8; 3]) -> String {
    format!("#{:02X}{:02X}{:02X}", c[2], c[1], c[0])
}

unsafe extern "system" fn wndproc(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        // WM_PAINT fills every pixel, so skip the erase to avoid a flash of another color.
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => {
            paint(h);
            LRESULT(0)
        }
        // SAFETY: default handling for every other message.
        _ => unsafe { DefWindowProcW(h, msg, wp, lp) },
    }
}

fn paint(h: HWND) {
    let mut ps = PAINTSTRUCT::default();
    // SAFETY: BeginPaint/EndPaint pair on the window being painted; brushes are deleted after use.
    unsafe {
        let hdc = BeginPaint(h, &mut ps);
        if hdc.is_invalid() {
            log::line("ERROR: BeginPaint failed");
            return;
        }
        let dpi = GetDpiForWindow(h);
        fill(hdc, &RECT { left: 0, top: 0, right: WIN_W, bottom: WIN_H }, WHITE);
        fill(hdc, &RECT1, BLUE);
        fill(hdc, &rect2(dpi), ORANGE);
        if !EndPaint(h, &ps).as_bool() {
            log::line("ERROR: EndPaint failed");
        }
    }
}

fn fill(hdc: HDC, rc: &RECT, color: COLORREF) {
    // SAFETY: plain GDI calls on a valid paint DC; the brush is deleted right after use.
    unsafe {
        let brush = CreateSolidBrush(color);
        if brush.is_invalid() {
            log::line("ERROR: CreateSolidBrush failed");
            return;
        }
        if FillRect(hdc, rc, brush) == 0 {
            log::line("ERROR: FillRect failed");
        }
        if !DeleteObject(brush.into()).as_bool() {
            log::line("ERROR: DeleteObject(brush) failed");
        }
    }
}
