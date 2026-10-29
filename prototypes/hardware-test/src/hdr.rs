//! The `hdr` mode (WPC-289): known colors painted on every monitor and read back through every capture
//! path, with HDR off or on; the frozen photo shown back and compared with the live screen in FP16; the
//! frozen photo's time; and, with `--watch-secs N`, every HDR or SDR white level change and the window
//! messages around it.
//!
//! Flags: `--patch-px N` (12), `--no-wgc`, `--no-overlay`, `--no-timing`, `--hold-secs N` (0),
//! `--watch-secs N`.

use std::cell::RefCell;
use std::time::Duration;

use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::DwmFlush;
use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory1};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BeginPaint, ClientToScreen, DIB_RGB_COLORS, EndPaint, PAINTSTRUCT,
    SetDIBitsToDevice, UpdateWindow,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect, IDC_ARROW, LoadCursorW, RegisterClassExW,
    SW_SHOWNOACTIVATE, ShowWindow, UnregisterClassW, WM_DEVICECHANGE, WM_DISPLAYCHANGE, WM_DPICHANGED,
    WM_ERASEBKGND, WM_PAINT, WM_SETTINGCHANGE, WM_THEMECHANGED, WNDCLASSEXW, WNDPROC, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::{PCWSTR, w};

use crate::capture::{
    self, Curve, Defer, DupFormat, DxgiSession, Fp16Image, fp16_lut, fp16_to_bgra, half_to_f32, pump_messages,
};
use crate::common::{Args, CaptureMethod, Ctx, MonitorInfo, Photo, PxRect};
use crate::monitors::{self, ColorState, short_name};
use crate::{frozen, image, log};

const CLASS_IMAGE: PCWSTR = w!("hwtest-hdr-image");
const CLASS_WATCH: PCWSTR = w!("hwtest-hdr-watch");
/// Patches per row of the test window.
const COLS: usize = 64;
/// Color of the empty slots after the last patch; not analyzed.
const FILLER: [u8; 3] = [0x40, 0x40, 0x40];
/// How often the watch polls DisplayConfig and DXGI.
const WATCH_POLL_MS: f64 = 250.0;

/// One painted square of a known color, R, G, B.
struct Patch {
    name: String,
    rgb: [u8; 3],
}

/// The named colors, every gray, every level of each channel alone, and 64 fixed pseudo-random colors.
fn patches() -> Vec<Patch> {
    let named: [(&str, [u8; 3]); 14] = [
        ("#1E90FF", [0x1E, 0x90, 0xFF]),
        ("#FF8C00", [0xFF, 0x8C, 0x00]),
        ("white", [255, 255, 255]),
        ("black", [0, 0, 0]),
        ("#3C3C3C", [0x3C, 0x3C, 0x3C]),
        ("#7F7F7F", [0x7F, 0x7F, 0x7F]),
        ("#808080", [0x80, 0x80, 0x80]),
        ("#C0C0C0", [0xC0, 0xC0, 0xC0]),
        ("red", [255, 0, 0]),
        ("green", [0, 255, 0]),
        ("blue", [0, 0, 255]),
        ("yellow", [255, 255, 0]),
        ("cyan", [0, 255, 255]),
        ("magenta", [255, 0, 255]),
    ];
    let mut out: Vec<Patch> = named.iter().map(|(n, c)| Patch { name: (*n).to_string(), rgb: *c }).collect();
    for v in 0..=255u8 {
        out.push(Patch { name: format!("gray-{v}"), rgb: [v, v, v] });
    }
    for (prefix, channel) in [("r", 0), ("g", 1), ("b", 2)] {
        for v in 0..=255u8 {
            let mut rgb = [0u8; 3];
            rgb[channel] = v;
            out.push(Patch { name: format!("{prefix}-{v}"), rgb });
        }
    }
    // A 32-bit LCG (Numerical Recipes constants) with a fixed seed: the same colors on every run.
    let mut s: u32 = 0x2890_0289;
    for i in 0..64 {
        s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let [r, g, b, _] = s.to_be_bytes();
        out.push(Patch { name: format!("rnd-{i}"), rgb: [r, g, b] });
    }
    out
}

/// R, G, B as `#RRGGBB`.
fn hex(c: [u8; 3]) -> String {
    format!("#{:02X}{:02X}{:02X}", c[0], c[1], c[2])
}

/// What an image window paints 1:1: BGRA, top-down, stride `width * 4`.
struct Image {
    width: i32,
    height: i32,
    bgra: Vec<u8>,
}

thread_local! {
    /// hwnd -> the image its WM_PAINT draws. Filled before the window is shown, emptied when it goes.
    static IMAGES: RefCell<Vec<(isize, Image)>> = const { RefCell::new(Vec::new()) };
}

fn patch_image(patches: &[Patch], p: usize) -> Result<Image, String> {
    let rows = patches.len().div_ceil(COLS);
    let (w, h) = (COLS * p, rows * p);
    let mut bgra = vec![0u8; w * h * 4];
    for slot in 0..rows * COLS {
        let c = patches.get(slot).map_or(FILLER, |pt| pt.rgb);
        let (x0, y0) = ((slot % COLS) * p, (slot / COLS) * p);
        for y in y0..y0 + p {
            for x in x0..x0 + p {
                let i = (y * w + x) * 4;
                bgra[i..i + 4].copy_from_slice(&[c[2], c[1], c[0], 255]);
            }
        }
    }
    Ok(Image { width: i32::try_from(w).ctx("image width")?, height: i32::try_from(h).ctx("image height")?, bgra })
}

fn register_class(hinst: HINSTANCE, class: PCWSTR, proc_: WNDPROC) -> Result<Defer<impl FnMut()>, String> {
    let wc = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: proc_,
        hInstance: hinst,
        // SAFETY: loads a system cursor.
        hCursor: unsafe { LoadCursorW(None, IDC_ARROW) }.ctx("LoadCursorW")?,
        lpszClassName: class,
        ..Default::default()
    };
    // SAFETY: `wc` is fully initialized and the class name is a static string.
    if unsafe { RegisterClassExW(&wc) } == 0 {
        return Err(format!("RegisterClassExW: {}", windows::core::Error::from_thread()));
    }
    Ok(Defer(move || {
        // SAFETY: the class registered above; every window of it is destroyed before this runs.
        if let Err(e) = unsafe { UnregisterClassW(class, Some(hinst)) } {
            log::line(format!("ERROR: UnregisterClassW: {e}"));
        }
    }))
}

/// A topmost, non-activating popup covering `rect` (physical pixels) that paints `image` 1:1.
fn image_window(hinst: HINSTANCE, rect: PxRect, image: Image, title: PCWSTR) -> Result<HWND, String> {
    // SAFETY: the class is registered; created directly at its final position on the target monitor, so
    // it starts at that monitor's DPI and gets no WM_DPICHANGED.
    let h = unsafe {
        CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            CLASS_IMAGE,
            title,
            WS_POPUP,
            rect.left,
            rect.top,
            rect.width(),
            rect.height(),
            None,
            None,
            Some(hinst),
            None,
        )
    }
    .ctx("CreateWindowExW")?;
    IMAGES.with(|m| m.borrow_mut().push((h.0 as isize, image)));
    // SAFETY: plain calls on the window just created.
    unsafe {
        // ShowWindow returns the previous visibility, not an error.
        let _ = ShowWindow(h, SW_SHOWNOACTIVATE);
        if !UpdateWindow(h).as_bool() {
            log::line("ERROR: UpdateWindow failed");
        }
    }
    Ok(h)
}

fn destroy(list: &mut Vec<HWND>) {
    for h in list.drain(..) {
        // SAFETY: the window was created on this thread.
        if let Err(e) = unsafe { DestroyWindow(h) } {
            log::line(format!("ERROR: DestroyWindow: {e}"));
        }
        IMAGES.with(|m| m.borrow_mut().retain(|(k, _)| *k != h.0 as isize));
    }
    pump_messages();
}

/// Lets DWM compose what was just drawn: two DwmFlush and 150 ms of message pumping.
fn settle() {
    pump_messages();
    for i in 0..2 {
        // SAFETY: plain call; waits for the next composition.
        if let Err(e) = unsafe { DwmFlush() } {
            log::line(format!("DwmFlush #{}: ERROR {e}", i + 1));
        }
        pump_messages();
    }
    let until = log::now_ms() + 150.0;
    while log::now_ms() < until {
        pump_messages();
        std::thread::sleep(Duration::from_millis(5));
    }
}

unsafe extern "system" fn image_proc(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
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
    // SAFETY: BeginPaint/EndPaint pair on the window being painted; the bits outlive SetDIBitsToDevice.
    unsafe {
        let hdc = BeginPaint(h, &mut ps);
        if hdc.is_invalid() {
            log::line("ERROR: BeginPaint failed");
            return;
        }
        IMAGES.with(|m| match m.borrow().iter().find(|(k, _)| *k == h.0 as isize) {
            Some((_, img)) => {
                let bmi = BITMAPINFO {
                    bmiHeader: BITMAPINFOHEADER {
                        biSize: size_of::<BITMAPINFOHEADER>() as u32,
                        biWidth: img.width,
                        // Negative height: top-down rows.
                        biHeight: -img.height,
                        biPlanes: 1,
                        biBitCount: 32,
                        biCompression: BI_RGB.0,
                        ..Default::default()
                    },
                    ..Default::default()
                };
                let (w, rows) = (img.width.unsigned_abs(), img.height.unsigned_abs());
                let n = SetDIBitsToDevice(hdc, 0, 0, w, rows, 0, 0, 0, rows, img.bgra.as_ptr().cast(), &bmi, DIB_RGB_COLORS);
                if n == 0 {
                    log::line("ERROR: SetDIBitsToDevice copied no rows");
                }
            }
            None => log::line(format!("ERROR: image window 0x{:x} has no image", h.0 as isize)),
        });
        if !EndPaint(h, &ps).as_bool() {
            log::line("ERROR: EndPaint failed");
        }
    }
}

/// Where one monitor's test window landed.
struct Placed {
    device_name: String,
    label: String,
    /// Client (0,0) on the virtual screen, physical pixels.
    origin: POINT,
    /// The color state when the run started.
    state: Option<ColorState>,
}

impl Placed {
    /// SDR white for the conversion: the monitor's level with HDR on, 80 nits (scRGB 1.0) with it off.
    fn white_nits(&self) -> Option<f64> {
        match &self.state {
            Some(s) if s.hdr_on() => s.sdr_white_nits(),
            _ => Some(80.0),
        }
    }
}

pub fn hdr_mode(args: &Args) -> Result<(), String> {
    if args.value("watch-secs").is_some() {
        return watch(args.num("watch-secs", 60)?);
    }
    let p = usize::try_from(args.num("patch-px", 12)?).ctx("--patch-px")?;
    let hold_secs = args.num("hold-secs", 0)?;
    let t_start = log::now_ms();
    let monitors = monitors::enumerate()?;
    let states = monitors::color_states()?;
    for m in &monitors {
        let label = short_name(&m.device_name);
        match states.iter().find(|s| s.device_name == m.device_name) {
            Some(s) => log::line(format!("RESULT hdr-state {label} '{}': {}", m.friendly_name, s.describe())),
            None => log::line(format!("RESULT hdr-state {label} '{}': no DisplayConfig path", m.friendly_name)),
        }
    }
    let patches = patches();
    log::line(format!(
        "patches: {} ({} named, 256 grays, 256 levels each of R, G and B alone, 64 fixed pseudo-random), {p}x{p} px each, {COLS} per row; painted with SetDIBitsToDevice (GDI, no color management)",
        patches.len(),
        patches.len() - 256 * 4 - 64
    ));

    // SAFETY: the module handle of this exe; no reference count to manage.
    let hinst: HINSTANCE = unsafe { GetModuleHandleW(None) }.ctx("GetModuleHandleW")?.into();
    let _unregister = register_class(hinst, CLASS_IMAGE, Some(image_proc))?;
    let mut windows = Vec::new();
    let result = (|| -> Result<(), String> {
        let mut placed = Vec::new();
        for m in &monitors {
            let img = patch_image(&patches, p)?;
            let (w, h) = (img.width, img.height);
            let left = m.rect.left + (m.rect.width() - w) / 2;
            let top = m.rect.top + (m.rect.height() - h) / 2;
            let rect = PxRect { left, top, right: left + w, bottom: top + h };
            let hwnd = image_window(hinst, rect, img, w!("hwtest hdr patches"))?;
            windows.push(hwnd);
            placed.push((m, hwnd, rect));
        }
        settle();
        let mut where_ = Vec::new();
        for (m, hwnd, rect) in &placed {
            let mut origin = POINT::default();
            let mut client = RECT::default();
            // SAFETY: plain queries on a live window; the out values live on this stack frame.
            unsafe {
                if !ClientToScreen(*hwnd, &mut origin).as_bool() {
                    return Err("ClientToScreen failed".to_string());
                }
                GetClientRect(*hwnd, &mut client).ctx("GetClientRect")?;
            }
            let label = short_name(&m.device_name).to_string();
            let ok = origin.x == rect.left && origin.y == rect.top && client.right == rect.width() && client.bottom == rect.height();
            log::line(format!(
                "hdr {label}: test window wanted {rect}, client origin ({},{}) size {}x{} -> {}",
                origin.x,
                origin.y,
                client.right,
                client.bottom,
                if ok { "OK" } else { "MISMATCH" }
            ));
            where_.push(Placed {
                device_name: m.device_name.clone(),
                label,
                origin,
                state: states.iter().find(|s| s.device_name == m.device_name).cloned(),
            });
        }
        color_run(&monitors, &where_, &patches, p, !args.flag("no-wgc"))?;
        if !args.flag("no-overlay") {
            overlay_check(hinst, &monitors, &where_)?;
        }
        if !args.flag("no-timing") {
            timing();
        }
        if hold_secs > 0 {
            log::line(format!("HOLD hdr: test windows stay up for {hold_secs} s"));
            let until = log::now_ms() + hold_secs as f64 * 1000.0;
            while log::now_ms() < until {
                pump_messages();
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        Ok(())
    })();
    destroy(&mut windows);
    log::line(format!("hdr: run took {:.0} ms", log::now_ms() - t_start));
    result
}

/// Every capture path on every monitor, each compared patch by patch with what was painted.
fn color_run(monitors: &[MonitorInfo], placed: &[Placed], patches: &[Patch], p: usize, wgc: bool) -> Result<(), String> {
    // One session at a time: a process may duplicate an output only once.
    for (method, format) in
        [("dxgi-b8", DupFormat::Bgra8), ("dxgi-legacy", DupFormat::Legacy), ("dxgi-fp16", DupFormat::Fp16)]
    {
        log::line(format!("--- {method}"));
        let a = log::now_ms();
        let mut session = match DxgiSession::with(format, format == DupFormat::Fp16) {
            Ok(s) => s,
            Err(e) => {
                log::line(format!("RESULT hdr-color all {method}: ERROR session: {e}"));
                continue;
            }
        };
        let grabs = session.grab_all();
        log::line(format!("hdr {method}: session and grab of every monitor {:.1} ms", log::now_ms() - a));
        let formats = session.formats();
        for pl in placed {
            match grabs.iter().find(|(n, _)| *n == pl.device_name) {
                Some((_, Ok(photo))) => {
                    let got = formats.iter().find(|(n, _)| *n == pl.device_name).map(|(_, f)| *f);
                    report(pl, &format!("{method} (as {got:?})"), photo, patches, p);
                    if format == DupFormat::Fp16 {
                        match session.raw(&pl.device_name) {
                            Some(raw) => raw_report(pl, method, raw, photo.rect, patches, p),
                            None => log::line(format!(
                                "RESULT hdr-raw {} {method}: no FP16 image (the duplication fell back to B8G8R8A8)",
                                pl.label
                            )),
                        }
                    }
                }
                Some((_, Err(e))) => log::line(format!("RESULT hdr-color {} {method}: ERROR {e}", pl.label)),
                None => log::line(format!("RESULT hdr-color {} {method}: no DXGI output", pl.label)),
            }
        }
    }

    log::line("--- gdi");
    for (m, pl) in monitors.iter().zip(placed) {
        match capture::gdi_monitor(m) {
            Ok(photo) => report(pl, "gdi", &photo, patches, p),
            Err(e) => log::line(format!("RESULT hdr-color {} gdi: ERROR {e}", pl.label)),
        }
    }

    if wgc {
        for (m, pl) in monitors.iter().zip(placed) {
            log::line(format!("--- wgc {}", pl.label));
            match capture::wgc::capture_monitor_as(m, true, 0, None) {
                Ok(shot) => report(pl, "wgc-b8", &shot.photo, patches, p),
                Err(e) => log::line(format!("RESULT hdr-color {} wgc-b8: ERROR {e}", pl.label)),
            }
            let Some(white) = pl.white_nits() else {
                log::line(format!("RESULT hdr-color {} wgc-fp16: skipped, no SDR white level", pl.label));
                continue;
            };
            let lut = fp16_lut(white, Curve::Srgb);
            match capture::wgc::capture_monitor_as(m, true, 0, Some(&lut)) {
                Ok(shot) => {
                    report(pl, "wgc-fp16", &shot.photo, patches, p);
                    match &shot.raw {
                        Some(raw) => raw_report(pl, "wgc-fp16", raw, shot.photo.rect, patches, p),
                        None => log::line(format!("RESULT hdr-raw {} wgc-fp16: no FP16 image", pl.label)),
                    }
                }
                Err(e) => log::line(format!("RESULT hdr-color {} wgc-fp16: ERROR {e}", pl.label)),
            }
        }
    }
    Ok(())
}

fn report(pl: &Placed, method: &str, photo: &Photo, patches: &[Patch], p: usize) {
    match analyze(pl, method, photo, patches, p) {
        Ok(s) => log::line(s),
        Err(e) => log::line(format!("RESULT hdr-color {} {method}: ERROR {e}", pl.label)),
    }
}

/// Patch `i`'s top-left corner in photo coordinates.
fn patch_corner(pl: &Placed, rect: PxRect, i: usize, p: usize) -> Result<(i32, i32), String> {
    let dx = i32::try_from((i % COLS) * p).ctx("patch x")?;
    let dy = i32::try_from((i / COLS) * p).ctx("patch y")?;
    Ok((pl.origin.x - rect.left + dx, pl.origin.y - rect.top + dy))
}

/// Every pixel of every patch against the painted color: the worst channel difference per patch.
fn analyze(pl: &Placed, method: &str, photo: &Photo, patches: &[Patch], p: usize) -> Result<String, String> {
    let (pw, ph) = (i32::try_from(photo.width).ctx("width")?, i32::try_from(photo.height).ctx("height")?);
    let pi = i32::try_from(p).ctx("patch size")?;
    let at = |x: i32, y: i32| {
        let k = ((y * pw + x) * 4) as usize;
        [photo.bgra[k + 2], photo.bgra[k + 1], photo.bgra[k]]
    };
    let (mut exact, mut off1, mut uniform, mut gray_exact) = (0usize, 0usize, 0usize, 0usize);
    let mut worst: Option<(u8, usize, [u8; 3])> = None;
    let mut gray_bad = Vec::new();
    let mut blue = String::new();
    for (i, pt) in patches.iter().enumerate() {
        let (x0, y0) = patch_corner(pl, photo.rect, i, p)?;
        if x0 < 0 || y0 < 0 || x0 + pi > pw || y0 + pi > ph {
            return Err(format!("patch {} at ({x0},{y0}) is outside the {}x{} photo", pt.name, pw, ph));
        }
        let first = at(x0, y0);
        let center = at(x0 + pi / 2, y0 + pi / 2);
        let (mut max_d, mut same) = (0u8, true);
        for y in y0..y0 + pi {
            for x in x0..x0 + pi {
                let c = at(x, y);
                same &= c == first;
                for (got, want) in c.iter().zip(pt.rgb) {
                    max_d = max_d.max(got.abs_diff(want));
                }
            }
        }
        uniform += usize::from(same);
        match max_d {
            0 => exact += 1,
            1 => off1 += 1,
            _ => {}
        }
        if max_d > 0 && worst.is_none_or(|(d, _, _)| max_d > d) {
            worst = Some((max_d, i, center));
        }
        if pt.name.starts_with("gray-") {
            if max_d == 0 {
                gray_exact += 1;
            } else if gray_bad.len() < 6 {
                gray_bad.push(format!("{}->{}", hex(pt.rgb), hex(center)));
            }
        }
        if pt.name == "#1E90FF" {
            blue = hex(center);
        }
    }
    let n = patches.len();
    let worst = match worst {
        Some((d, i, c)) => format!("{} painted {} reads {} (delta {d})", patches[i].name, hex(patches[i].rgb), hex(c)),
        None => "none".to_string(),
    };
    Ok(format!(
        "RESULT hdr-color {} {method}: {} — {exact}/{n} patches exact, {off1} off by 1, {} off by more; worst {worst}; #1E90FF reads {blue}; grays exact {gray_exact}/256{}; uniform patches {uniform}/{n}",
        pl.label,
        if exact == n { "EXACT" } else { "NOT EXACT" },
        n - exact - off1,
        if gray_bad.is_empty() { String::new() } else { format!(" (first misses {})", gray_bad.join(" ")) }
    ))
}

/// The scRGB values behind a few patches, the SDR white they imply, and the patch check redone with
/// three conversions: sRGB curve and gamma 2.2 at the reported white, and sRGB at the measured white.
fn raw_report(pl: &Placed, method: &str, raw: &Fp16Image, rect: PxRect, patches: &[Patch], p: usize) {
    let center = |name: &str| -> Option<[f32; 3]> {
        let i = patches.iter().position(|pt| pt.name == name)?;
        let (x0, y0) = patch_corner(pl, rect, i, p).ok()?;
        let half = i32::try_from(p / 2).ok()?;
        let (x, y) = (usize::try_from(x0 + half).ok()?, usize::try_from(y0 + half).ok()?);
        let k = (y * raw.width as usize + x) * 4;
        let px = raw.rgba.get(k..k + 3)?;
        Some([half_to_f32(px[0]), half_to_f32(px[1]), half_to_f32(px[2])])
    };
    let show = |name: &str| match center(name) {
        Some(c) => format!("{name} ({:.5}, {:.5}, {:.5})", c[0], c[1], c[2]),
        None => format!("{name} (outside)"),
    };
    let names = ["white", "#C0C0C0", "gray-128", "gray-16", "gray-1", "black", "#1E90FF", "#FF8C00"];
    log::line(format!(
        "hdr-raw {} {method}: scRGB at patch centers (1.0 = 80 nits): {}",
        pl.label,
        names.iter().map(|n| show(n)).collect::<Vec<_>>().join(", ")
    ));
    let reported = pl.white_nits();
    let measured = center("white").map(|c| f64::from(c[1]) * 80.0);
    log::line(format!(
        "RESULT hdr-white {} {method}: white patch = {} nits; SDR white used for the conversion {} nits -> {}",
        pl.label,
        measured.map_or("?".to_string(), |m| format!("{m:.2}")),
        reported.map_or("?".to_string(), |r| format!("{r:.2}")),
        match (measured, reported) {
            (Some(m), Some(r)) if (m - r).abs() < 0.5 => "MATCH",
            (Some(_), Some(_)) => "DIFFERENT",
            _ => "unknown",
        }
    ));
    let mut variants: Vec<(&str, f64, Curve)> = Vec::new();
    if let Some(r) = reported {
        variants.push(("srgb", r, Curve::Srgb));
        variants.push(("g22", r, Curve::Gamma22));
    }
    if let Some(m) = measured.filter(|m| m.is_finite() && *m > 0.0) {
        variants.push(("srgb-measured-white", m, Curve::Srgb));
    }
    for (conv, white, curve) in variants {
        let photo = Photo {
            device_name: pl.device_name.clone(),
            rect,
            width: raw.width,
            height: raw.height,
            bgra: fp16_to_bgra(raw, &fp16_lut(white, curve)),
            method: CaptureMethod::Dxgi,
            taken_ms: log::now_ms(),
        };
        report(pl, &format!("{method}/{conv}@{white:.1}"), &photo, patches, p);
    }
}

/// The frozen screen with HDR: the photo shown back 1:1 in SDR windows must compose to the same scRGB
/// values as the live screen did, or the colors change when the screen freezes.
fn overlay_check(hinst: HINSTANCE, monitors: &[MonitorInfo], placed: &[Placed]) -> Result<(), String> {
    log::line("--- frozen photo shown back");
    let (live_raw, live_fp16) = {
        let mut s = DxgiSession::with(DupFormat::Fp16, true)?;
        let grabs = s.grab_all();
        let mut raws = Vec::new();
        let mut photos = Vec::new();
        for (name, r) in grabs {
            match (r, s.raw(&name)) {
                (Ok(photo), Some(raw)) => {
                    raws.push((name.clone(), Fp16Image { width: raw.width, height: raw.height, rgba: raw.rgba.clone() }));
                    photos.push(photo);
                }
                (Ok(photo), None) => {
                    log::line(format!("hdr-frozen {}: no FP16 live image; only the 8-bit comparison", short_name(&name)));
                    photos.push(photo);
                }
                (Err(e), _) => log::line(format!("RESULT hdr-frozen {}: ERROR live FP16 grab: {e}", short_name(&name))),
            }
        }
        (raws, photos)
    };
    let mut b8 = Vec::new();
    {
        let mut s = DxgiSession::with(DupFormat::Bgra8, false)?;
        for (name, r) in s.grab_all() {
            match r {
                Ok(photo) => b8.push(photo),
                Err(e) => log::line(format!("RESULT hdr-frozen {}: ERROR live B8G8R8A8 grab: {e}", short_name(&name))),
            }
        }
    }
    // What the frozen photo uses (HWTEST_DXGI_FORMAT=auto): FP16 converted where HDR is on, B8G8R8A8 elsewhere.
    let hdr_on = |name: &str| placed.iter().any(|pl| pl.device_name == name && pl.state.as_ref().is_some_and(ColorState::hdr_on));
    let auto: Vec<&Photo> = monitors
        .iter()
        .filter_map(|m| {
            let list = if hdr_on(&m.device_name) { &live_fp16 } else { &b8 };
            list.iter().find(|ph| ph.device_name == m.device_name)
        })
        .collect();
    show_back(hinst, "auto", &auto, &live_raw, &live_fp16)?;
    if placed.iter().any(|pl| pl.state.as_ref().is_some_and(ColorState::hdr_on)) {
        let b8_refs: Vec<&Photo> = b8.iter().collect();
        show_back(hinst, "b8", &b8_refs, &live_raw, &live_fp16)?;
    }
    Ok(())
}

/// Shows `photos` full screen, grabs the screen again in FP16 and compares with the live grab.
fn show_back(
    hinst: HINSTANCE,
    variant: &str,
    photos: &[&Photo],
    live_raw: &[(String, Fp16Image)],
    live_fp16: &[Photo],
) -> Result<(), String> {
    let mut wins = Vec::new();
    let result = (|| -> Result<(), String> {
        for ph in photos {
            let img = Image {
                width: i32::try_from(ph.width).ctx("photo width")?,
                height: i32::try_from(ph.height).ctx("photo height")?,
                bgra: ph.bgra.clone(),
            };
            wins.push(image_window(hinst, ph.rect, img, w!("hwtest hdr frozen"))?);
        }
        settle();
        let mut s = DxgiSession::with(DupFormat::Fp16, true)?;
        let grabs = s.grab_all();
        for (name, r) in &grabs {
            let label = short_name(name);
            let shown = match r {
                Ok(p) => p,
                Err(e) => {
                    log::line(format!("RESULT hdr-frozen {label} {variant}: ERROR grab of the shown photo: {e}"));
                    continue;
                }
            };
            match (live_raw.iter().find(|(n, _)| n == name), s.raw(name)) {
                (Some((_, a)), Some(b)) => log::line(compare_raw(label, variant, a, b)),
                _ => log::line(format!("hdr-frozen {label} {variant}: no FP16 pair; 8-bit comparison only")),
            }
            if let Some(live) = live_fp16.iter().find(|p| p.device_name == *name) {
                match image::diff(live, shown, &[]) {
                    Ok(d) => log::line(format!(
                        "RESULT hdr-frozen-8bit {label} {variant}: {} of {} pixels differ after the conversion, max delta {}, first at {:?}",
                        d.differing, d.total, d.max_delta, d.first
                    )),
                    Err(e) => log::line(format!("RESULT hdr-frozen-8bit {label} {variant}: ERROR {e}")),
                }
            }
        }
        Ok(())
    })();
    destroy(&mut wins);
    result
}

fn compare_raw(label: &str, variant: &str, a: &Fp16Image, b: &Fp16Image) -> String {
    if a.width != b.width || a.height != b.height || a.rgba.len() != b.rgba.len() {
        return format!(
            "RESULT hdr-frozen {label} {variant}: ERROR sizes differ, {}x{} vs {}x{}",
            a.width, a.height, b.width, b.height
        );
    }
    let w = a.width as usize;
    let (mut differing, mut max_d) = (0u64, 0f32);
    let mut bbox: Option<(usize, usize, usize, usize)> = None;
    for (i, (pa, pb)) in a.rgba.as_chunks::<4>().0.iter().zip(b.rgba.as_chunks::<4>().0).enumerate() {
        if pa[..3] == pb[..3] {
            continue;
        }
        differing += 1;
        for c in 0..3 {
            max_d = max_d.max((half_to_f32(pa[c]) - half_to_f32(pb[c])).abs());
        }
        let (x, y) = (i % w, i / w);
        bbox = Some(match bbox {
            None => (x, y, x + 1, y + 1),
            Some((l, t, r, btm)) => (l.min(x), t.min(y), r.max(x + 1), btm.max(y + 1)),
        });
    }
    let total = a.rgba.len() / 4;
    format!(
        "RESULT hdr-frozen {label} {variant}: {} — {differing} of {total} pixels differ in FP16, max |delta| {max_d:.5} scRGB ({:.2} nits){}",
        if differing == 0 { "IDENTICAL" } else { "DIFFERENT" },
        f64::from(max_d) * 80.0,
        match bbox {
            Some((l, t, r, btm)) => format!("; differences inside ({l},{t})-({r},{btm}) in photo coordinates"),
            None => String::new(),
        }
    )
}

/// The frozen photo of every monitor, cold (session created each time), three times per format.
fn timing() {
    log::line("--- frozen photo time");
    for (name, format) in [("auto", DupFormat::Auto), ("bgra8", DupFormat::Bgra8)] {
        let mut times = Vec::new();
        for _ in 0..3 {
            let a = log::now_ms();
            match capture::frozen_photo_with(format) {
                Ok(photos) => {
                    let methods: Vec<String> =
                        photos.iter().map(|p| format!("{}={:?}", short_name(&p.device_name), p.method)).collect();
                    times.push(format!("{:.1} ({})", log::now_ms() - a, methods.join(" ")));
                }
                Err(e) => times.push(format!("ERROR {e}")),
            }
        }
        log::line(format!("RESULT hdr-photo-time {name}: cold frozen photo of every monitor, ms: {}", times.join("; ")));
    }
}

/// Logs every change of HDR state or SDR white level for `secs` seconds, polling DisplayConfig and DXGI
/// every 250 ms, with the window messages a hidden top-level window receives meanwhile.
fn watch(secs: u64) -> Result<(), String> {
    // SAFETY: the module handle of this exe; no reference count to manage.
    let hinst: HINSTANCE = unsafe { GetModuleHandleW(None) }.ctx("GetModuleHandleW")?.into();
    let _unregister = register_class(hinst, CLASS_WATCH, Some(watch_proc))?;
    // A hidden top-level window: broadcasts such as WM_DISPLAYCHANGE reach top-level windows, and a
    // message-only window would miss them.
    // SAFETY: the class is registered; the window is never shown.
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_TOOLWINDOW,
            CLASS_WATCH,
            w!("hwtest hdr watch"),
            WS_POPUP,
            0,
            0,
            1,
            1,
            None,
            None,
            Some(hinst),
            None,
        )
    }
    .ctx("CreateWindowExW(watch)")?;
    let _destroy = Defer(|| {
        // SAFETY: the window was created on this thread.
        if let Err(e) = unsafe { DestroyWindow(hwnd) } {
            log::line(format!("ERROR: DestroyWindow(watch): {e}"));
        }
    });
    // SAFETY: plain factory creation.
    let mut factory: IDXGIFactory1 = unsafe { CreateDXGIFactory1() }.ctx("CreateDXGIFactory1")?;
    let mut last = monitors::color_states()?;
    for s in &last {
        log::line(format!("hdr-watch start {}: {}", short_name(&s.device_name), s.describe()));
    }
    log::line(format!(
        "HOLD hdr-watch: watching for {secs} s; turn HDR on or off, move the SDR content brightness slider"
    ));
    let end = log::now_ms() + secs as f64 * 1000.0;
    let mut next = log::now_ms();
    let mut changes = 0u32;
    while log::now_ms() < end {
        pump_messages();
        std::thread::sleep(Duration::from_millis(10));
        if log::now_ms() < next {
            continue;
        }
        next = log::now_ms() + WATCH_POLL_MS;
        // SAFETY: plain query on a live factory.
        if !unsafe { factory.IsCurrent() }.as_bool() {
            log::line("RESULT hdr-watch dxgi-factory: IsCurrent() turned false (DXGI saw the outputs change); new factory");
            // SAFETY: as above.
            factory = unsafe { CreateDXGIFactory1() }.ctx("CreateDXGIFactory1")?;
        }
        let now = match monitors::color_states() {
            Ok(s) => s,
            Err(e) => {
                log::line(format!("hdr-watch: color states ERROR {e}"));
                continue;
            }
        };
        for s in &now {
            let before = last.iter().find(|b| b.device_name == s.device_name);
            if before == Some(s) {
                continue;
            }
            changes += 1;
            log::line(format!(
                "RESULT hdr-watch change #{changes} {}: {} -> {}",
                short_name(&s.device_name),
                before.map_or("(new monitor)".to_string(), ColorState::describe),
                s.describe()
            ));
        }
        for b in last.iter().filter(|b| !now.iter().any(|s| s.device_name == b.device_name)) {
            changes += 1;
            log::line(format!("RESULT hdr-watch change #{changes} {}: monitor gone", short_name(&b.device_name)));
        }
        last = now;
    }
    log::line(format!("RESULT hdr-watch summary: {changes} change(s) in {secs} s"));
    Ok(())
}

unsafe extern "system" fn watch_proc(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let name = match msg {
        WM_DISPLAYCHANGE => Some("WM_DISPLAYCHANGE"),
        WM_SETTINGCHANGE => Some("WM_SETTINGCHANGE"),
        WM_DEVICECHANGE => Some("WM_DEVICECHANGE"),
        WM_DPICHANGED => Some("WM_DPICHANGED"),
        WM_THEMECHANGED => Some("WM_THEMECHANGED"),
        _ => None,
    };
    if let Some(name) = name {
        let detail = if msg == WM_SETTINGCHANGE { frozen::setting_string(lp) } else { format!("lParam {:#x}", lp.0) };
        log::line(format!("hdr-watch signal {name}: wParam {:#x}, {detail}", wp.0));
    }
    // SAFETY: default handling for every message.
    unsafe { DefWindowProcW(h, msg, wp, lp) }
}
