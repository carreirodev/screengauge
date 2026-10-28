//! Builder A. Screen capture (DXGI Desktop Duplication, GDI, Windows.Graphics.Capture) and the `capture` mode.

mod wgc;

use std::collections::HashMap;
use std::time::Duration;

use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE, D3D_DRIVER_TYPE_UNKNOWN, D3D_FEATURE_LEVEL};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAP_READ, D3D11_MAPPED_SUBRESOURCE,
    D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING, D3D11CreateDevice, ID3D11Device,
    ID3D11DeviceContext, ID3D11Texture2D,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, DXGI_ERROR_ACCESS_LOST, DXGI_ERROR_NOT_FOUND, DXGI_ERROR_WAIT_TIMEOUT, DXGI_OUTDUPL_FRAME_INFO,
    IDXGIAdapter, IDXGIAdapter1, IDXGIFactory1, IDXGIOutput1, IDXGIOutput5, IDXGIOutputDuplication, IDXGIResource,
};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CAPTUREBLT, CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS,
    DeleteDC, DeleteObject, GdiFlush, GetDC, HGDIOBJ, ROP_CODE, ReleaseDC, SRCCOPY, SelectObject,
};
use windows::Win32::UI::WindowsAndMessaging::{DispatchMessageW, MSG, PM_REMOVE, PeekMessageW, TranslateMessage};
use windows::core::Interface;

use crate::common::{Args, CaptureMethod, Ctx, MonitorInfo, Photo, PxRect};
use crate::monitors::{self, px, rotation_name, short_name, wide};
use crate::{image, log};

/// Total time the first grab of an output may spend waiting for a real desktop image.
const FIRST_FRAME_BUDGET_MS: f64 = 500.0;
/// AcquireNextFrame timeout per attempt while waiting for the first image.
const FIRST_ATTEMPT_TIMEOUT_MS: u32 = 100;
/// AcquireNextFrame timeout once an output has a cached image. Learn: with a zero timeout the call
/// "verifies whether there is a new desktop image available, returns immediately". A static screen then
/// costs nothing to re-grab, and any change since the previous acquire is returned at once.
const LATER_TIMEOUT_MS: u32 = 0;

/// One Desktop Duplication per attached output, each created on the adapter that owns that output.
/// Kept alive so the same session can grab several times (the frozen screen's delay series).
/// A process may duplicate an output only once (Learn, DuplicateOutput: E_INVALIDARG when "the calling
/// application is already duplicating this desktop output"), so drop a session before creating another
/// one, and before calling `frozen_photo`, which creates its own.
pub struct DxgiSession {
    outputs: Vec<DupOutput>,
}

impl DxgiSession {
    pub fn new() -> Result<Self, String> {
        let t0 = log::now_ms();
        // SAFETY: plain factory creation.
        let factory: IDXGIFactory1 = unsafe { CreateDXGIFactory1() }.ctx("CreateDXGIFactory1")?;
        let mut outputs = Vec::new();
        for ai in 0u32.. {
            // SAFETY: plain enumeration; NOT_FOUND ends it.
            let adapter = match unsafe { factory.EnumAdapters1(ai) } {
                Ok(a) => a,
                Err(e) if e.code() == DXGI_ERROR_NOT_FOUND => break,
                Err(e) => return Err(format!("EnumAdapters1({ai}): {e}")),
            };
            // SAFETY: plain query.
            let adapter_name = wide(&unsafe { adapter.GetDesc1() }.ctx("IDXGIAdapter1::GetDesc1")?.Description);
            // One D3D11 device per adapter, created when its first attached output shows up.
            let mut gpu: Option<Result<Gpu, String>> = None;
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
                if !d.AttachedToDesktop.as_bool() {
                    log::line(format!("dxgi: {name} on '{adapter_name}' is not attached to the desktop; skipped"));
                    continue;
                }
                let rect = px(&d.DesktopCoordinates);
                let gpu = gpu.get_or_insert_with(|| Gpu::on(&adapter, &adapter_name)).clone();
                let output1: IDXGIOutput1 = output.cast().ctx("IDXGIOutput1")?;
                let mut o = DupOutput {
                    device_name: name.clone(),
                    rect,
                    output: output1,
                    gpu,
                    dup: Err("not created yet".to_string()),
                    staging: None,
                    cached: None,
                };
                // Learn (Desktop Duplication API, "Rotating the desktop image"): in a rotated mode the surface
                // is un-rotated and the image is rotated within it, so the rows would not match DesktopCoordinates.
                let rot = d.Rotation.0;
                o.dup = if rot == 0 || rot == 1 {
                    o.duplicate()
                } else {
                    Err(format!("output is rotated ({}); refused, this test does not un-rotate", rotation_name(d.Rotation)))
                };
                match &o.dup {
                    Ok(_) => log::line(format!(
                        "dxgi: {name} on '{adapter_name}' DesktopCoordinates {rect} rotation {} ({}): ready",
                        rot,
                        rotation_name(d.Rotation)
                    )),
                    Err(e) => log::line(format!("dxgi: {name} on '{adapter_name}': ERROR {e}")),
                }
                outputs.push(o);
            }
        }
        if outputs.is_empty() {
            return Err("DXGI reports no output attached to the desktop".to_string());
        }
        log::line(format!("dxgi: session with {} outputs created in {:.3} ms", outputs.len(), log::now_ms() - t0));
        Ok(Self { outputs })
    }

    /// The current image of every output: one entry per output, keyed by GDI device name.
    pub fn grab_all(&mut self) -> Vec<(String, Result<Photo, String>)> {
        self.outputs.iter_mut().map(|o| (o.device_name.clone(), o.grab())).collect()
    }

    /// The current image of one output, by GDI device name.
    pub fn grab(&mut self, device_name: &str) -> Result<Photo, String> {
        match self.outputs.iter_mut().find(|o| o.device_name == device_name) {
            Some(o) => o.grab(),
            None => Err(format!("dxgi: no attached output named {device_name}")),
        }
    }
}

#[derive(Clone)]
struct Gpu {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
}

impl Gpu {
    /// A D3D11 device on `adapter`; Learn (DuplicateOutput): the device "must be created from the adapter
    /// to which the output is connected".
    fn on(adapter: &IDXGIAdapter1, adapter_name: &str) -> Result<Gpu, String> {
        let base: IDXGIAdapter = adapter.cast().ctx("IDXGIAdapter")?;
        create_device(Some(&base), D3D_DRIVER_TYPE_UNKNOWN).map_err(|e| format!("D3D11 device on '{adapter_name}': {e}"))
    }
}

fn create_device(adapter: Option<&IDXGIAdapter>, driver: D3D_DRIVER_TYPE) -> Result<Gpu, String> {
    let t0 = log::now_ms();
    let mut device = None;
    let mut context = None;
    let mut level = D3D_FEATURE_LEVEL::default();
    // SAFETY: the out pointers refer to locals that outlive the call.
    unsafe {
        D3D11CreateDevice(
            adapter,
            driver,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            None,
            D3D11_SDK_VERSION,
            Some(&mut device),
            Some(&mut level),
            Some(&mut context),
        )
    }
    .ctx("D3D11CreateDevice")?;
    log::line(format!("d3d11: device created, feature level 0x{:x}, {:.3} ms", level.0, log::now_ms() - t0));
    Ok(Gpu {
        device: device.ok_or("D3D11CreateDevice returned no device")?,
        context: context.ok_or("D3D11CreateDevice returned no context")?,
    })
}

struct Staging {
    tex: ID3D11Texture2D,
    width: u32,
    height: u32,
}

struct Cached {
    width: u32,
    height: u32,
    bgra: Vec<u8>,
    grabbed_ms: f64,
}

struct DupOutput {
    device_name: String,
    rect: PxRect,
    output: IDXGIOutput1,
    gpu: Result<Gpu, String>,
    dup: Result<IDXGIOutputDuplication, String>,
    staging: Option<Staging>,
    /// The last image read from this output, returned when DXGI reports nothing new.
    cached: Option<Cached>,
}

enum Fail {
    AccessLost,
    Other(String),
}

impl DupOutput {
    /// DuplicateOutput1 first (Learn recommends it "for improved performance"; it takes the format list),
    /// then DuplicateOutput. Learn states no DPI-awareness requirement for either; the process is
    /// Per-Monitor V2 from its manifest, so whichever one succeeds here is the observed fact.
    fn duplicate(&self) -> Result<IDXGIOutputDuplication, String> {
        let gpu = self.gpu.as_ref().map_err(String::clone)?;
        let t0 = log::now_ms();
        let dup = match self.output.cast::<IDXGIOutput5>() {
            // SAFETY: the device was created on the adapter that owns this output.
            Ok(o5) => match unsafe { o5.DuplicateOutput1(&gpu.device, 0, &[DXGI_FORMAT_B8G8R8A8_UNORM]) } {
                Ok(d) => {
                    log::line(format!("dxgi: {} DuplicateOutput1(B8G8R8A8_UNORM) OK", self.device_name));
                    Some(d)
                }
                Err(e) => {
                    log::line(format!(
                        "dxgi: {} DuplicateOutput1 failed: {e} (0x{:08x}); trying DuplicateOutput",
                        self.device_name,
                        e.code().0 as u32
                    ));
                    None
                }
            },
            Err(e) => {
                log::line(format!("dxgi: {} has no IDXGIOutput5 ({e}); trying DuplicateOutput", self.device_name));
                None
            }
        };
        let dup = match dup {
            Some(d) => d,
            None => {
                // SAFETY: as above.
                let d = unsafe { self.output.DuplicateOutput(&gpu.device) }.ctx("DuplicateOutput")?;
                log::line(format!("dxgi: {} DuplicateOutput OK", self.device_name));
                d
            }
        };
        // SAFETY: plain query.
        let dd = unsafe { dup.GetDesc() };
        log::line(format!(
            "dxgi: {} duplication desc: {}x{} format {} rotation {} image in system memory {}; created in {:.3} ms",
            self.device_name,
            dd.ModeDesc.Width,
            dd.ModeDesc.Height,
            dd.ModeDesc.Format.0,
            rotation_name(dd.Rotation),
            dd.DesktopImageInSystemMemory.as_bool(),
            log::now_ms() - t0
        ));
        Ok(dup)
    }

    fn grab(&mut self) -> Result<Photo, String> {
        let r = match self.try_grab() {
            Ok(p) => Ok(p),
            Err(Fail::Other(e)) => Err(e),
            Err(Fail::AccessLost) => {
                log::line(format!(
                    "dxgi: {} DXGI_ERROR_ACCESS_LOST; recreating the duplication once",
                    self.device_name
                ));
                // Release the old duplication first: one duplication per output per process.
                self.dup = Err("being recreated".to_string());
                self.cached = None;
                self.dup = self.duplicate();
                match self.try_grab() {
                    Ok(p) => Ok(p),
                    Err(Fail::AccessLost) => Err("DXGI_ERROR_ACCESS_LOST again after recreating".to_string()),
                    Err(Fail::Other(e)) => Err(format!("after recreating: {e}")),
                }
            }
        };
        if let Err(e) = &r {
            log::line(format!("dxgi: {} grab failed: {e}", self.device_name));
        }
        r
    }

    fn try_grab(&mut self) -> Result<Photo, Fail> {
        let dup = self.dup.as_ref().map_err(|e| Fail::Other(e.clone()))?.clone();
        let start = log::now_ms();
        let first = self.cached.is_none();
        let mut attempts = 0u32;
        loop {
            attempts += 1;
            let timeout = if first { FIRST_ATTEMPT_TIMEOUT_MS } else { LATER_TIMEOUT_MS };
            let mut info = DXGI_OUTDUPL_FRAME_INFO::default();
            let mut resource: Option<IDXGIResource> = None;
            let called = log::now_ms();
            // SAFETY: both out pointers refer to locals; a successful call is paired with ReleaseFrame below.
            let r = unsafe { dup.AcquireNextFrame(timeout, &mut info, &mut resource) };
            let returned = log::now_ms();
            match r {
                Ok(()) => {}
                Err(e) if e.code() == DXGI_ERROR_WAIT_TIMEOUT => {
                    if let Some(c) = &self.cached {
                        log::line(format!(
                            "dxgi: {} no new frame (timeout {timeout} ms, waited {:.3} ms); cached image from {:.3} ms is current, taken_ms = {returned:.3} (confirmed-current time)",
                            self.device_name,
                            returned - called,
                            c.grabbed_ms
                        ));
                        return Ok(self.cached_photo(returned));
                    }
                    log::line(format!(
                        "dxgi: {} first frame attempt {attempts}: timeout after {:.3} ms",
                        self.device_name,
                        returned - called
                    ));
                    if returned - start >= FIRST_FRAME_BUDGET_MS {
                        return Err(Fail::Other(format!(
                            "no desktop image within {FIRST_FRAME_BUDGET_MS} ms ({attempts} attempts)"
                        )));
                    }
                    continue;
                }
                Err(e) if e.code() == DXGI_ERROR_ACCESS_LOST => return Err(Fail::AccessLost),
                Err(e) => return Err(Fail::Other(format!("AcquireNextFrame: {e}"))),
            }
            let held = HeldFrame { dup: &dup, released: false };
            if first || info.LastPresentTime == 0 {
                log::line(format!(
                    "dxgi: {} acquire {attempts}: waited {:.3} ms, LastPresentTime {}, AccumulatedFrames {}, LastMouseUpdateTime {}, RectsCoalesced {}, ProtectedContentMaskedOut {}",
                    self.device_name,
                    returned - called,
                    info.LastPresentTime,
                    info.AccumulatedFrames,
                    info.LastMouseUpdateTime,
                    info.RectsCoalesced.as_bool(),
                    info.ProtectedContentMaskedOut.as_bool()
                ));
            }
            if info.LastPresentTime == 0 {
                // Learn (DXGI_OUTDUPL_FRAME_INFO): zero means the desktop image was not updated since the last
                // AcquireNextFrame; only the pointer changed.
                held.release().map_err(Fail::Other)?;
                if self.cached.is_some() {
                    return Ok(self.cached_photo(returned));
                }
                // No image of our own yet: the surface may hold no desktop image. Try again.
                if returned - start >= FIRST_FRAME_BUDGET_MS {
                    return Err(Fail::Other(format!(
                        "only pointer updates (LastPresentTime 0) within {FIRST_FRAME_BUDGET_MS} ms ({attempts} attempts)"
                    )));
                }
                continue;
            }
            let resource = resource.ok_or(Fail::Other("AcquireNextFrame returned no resource".to_string()))?;
            let tex: ID3D11Texture2D = resource.cast().map_err(|e| Fail::Other(format!("desktop resource as texture: {e}")))?;
            let gpu = self.gpu.as_ref().map_err(|e| Fail::Other(e.clone()))?;
            let copy_start = log::now_ms();
            let (width, height, bgra) =
                read_bgra(gpu, &mut self.staging, &tex, None).map_err(Fail::Other)?;
            // The pixels are in system memory now; give the frame back to DXGI.
            held.release().map_err(Fail::Other)?;
            let copy_ms = log::now_ms() - copy_start;
            if width as i32 != self.rect.width() || height as i32 != self.rect.height() {
                log::line(format!(
                    "dxgi: {} WARNING surface {width}x{height} differs from DesktopCoordinates {}",
                    self.device_name, self.rect
                ));
            }
            let cache_start = log::now_ms();
            self.cached = Some(Cached { width, height, bgra: bgra.clone(), grabbed_ms: returned });
            let cache_ms = log::now_ms() - cache_start;
            log::line(format!(
                "dxgi: {} frame {width}x{height}: acquire waited {:.3} ms, copy+map {copy_ms:.3} ms, cache copy {cache_ms:.3} ms, {attempts} attempt(s), total {:.3} ms",
                self.device_name,
                returned - called,
                log::now_ms() - start
            ));
            return Ok(Photo {
                device_name: self.device_name.clone(),
                rect: self.rect,
                width,
                height,
                bgra,
                method: CaptureMethod::Dxgi,
                taken_ms: returned,
            });
        }
    }

    fn cached_photo(&self, confirmed_ms: f64) -> Photo {
        // Only called when `cached` is Some; an empty photo would fail every size check loudly.
        let (width, height, bgra) = match &self.cached {
            Some(c) => (c.width, c.height, c.bgra.clone()),
            None => (0, 0, Vec::new()),
        };
        Photo {
            device_name: self.device_name.clone(),
            rect: self.rect,
            width,
            height,
            bgra,
            method: CaptureMethod::Dxgi,
            taken_ms: confirmed_ms,
        }
    }
}

/// Releases an acquired duplication frame on every path; `release` reports the error, `Drop` logs it.
struct HeldFrame<'a> {
    dup: &'a IDXGIOutputDuplication,
    released: bool,
}

impl HeldFrame<'_> {
    fn release(mut self) -> Result<(), String> {
        self.released = true;
        // SAFETY: a frame was acquired and not yet released.
        unsafe { self.dup.ReleaseFrame() }.ctx("ReleaseFrame")
    }
}

impl Drop for HeldFrame<'_> {
    fn drop(&mut self) {
        if !self.released {
            // SAFETY: as above; this is the error path.
            if let Err(e) = unsafe { self.dup.ReleaseFrame() } {
                log::line(format!("ERROR: ReleaseFrame on an error path: {e}"));
            }
        }
    }
}

/// Copies a BGRA texture into tightly packed top-down rows with alpha 255, through a staging texture.
/// `crop` limits the copy to the top-left `(w, h)`.
fn read_bgra(
    gpu: &Gpu,
    staging: &mut Option<Staging>,
    tex: &ID3D11Texture2D,
    crop: Option<(u32, u32)>,
) -> Result<(u32, u32, Vec<u8>), String> {
    let mut desc = D3D11_TEXTURE2D_DESC::default();
    // SAFETY: fills one struct on this stack frame.
    unsafe { tex.GetDesc(&mut desc) };
    if desc.Format != DXGI_FORMAT_B8G8R8A8_UNORM {
        return Err(format!("texture format {} is not B8G8R8A8_UNORM (87)", desc.Format.0));
    }
    let reuse = matches!(staging, Some(s) if s.width == desc.Width && s.height == desc.Height);
    if !reuse {
        let sd = D3D11_TEXTURE2D_DESC {
            Width: desc.Width,
            Height: desc.Height,
            MipLevels: 1,
            ArraySize: 1,
            Format: desc.Format,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Usage: D3D11_USAGE_STAGING,
            BindFlags: 0,
            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
            MiscFlags: 0,
        };
        let mut t = None;
        // SAFETY: `sd` describes a valid staging texture; the out pointer is a local.
        unsafe { gpu.device.CreateTexture2D(&sd, None, Some(&mut t)) }.ctx("CreateTexture2D(staging)")?;
        let t = t.ok_or("CreateTexture2D returned no texture")?;
        *staging = Some(Staging { tex: t, width: desc.Width, height: desc.Height });
    }
    let st = &staging.as_ref().ok_or("staging texture missing")?.tex;
    let (w, h) = match crop {
        Some((cw, ch)) => (cw.min(desc.Width), ch.min(desc.Height)),
        None => (desc.Width, desc.Height),
    };
    // SAFETY: both textures live on `gpu.device` and have the same size and format.
    unsafe { gpu.context.CopyResource(st, tex) };
    let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
    // SAFETY: the staging texture has CPU read access; Map waits for the copy to finish.
    unsafe { gpu.context.Map(st, 0, D3D11_MAP_READ, 0, Some(&mut mapped)) }.ctx("Map(staging)")?;
    let row = w as usize * 4;
    let pitch = mapped.RowPitch as usize;
    let result = if mapped.pData.is_null() || pitch < row {
        Err(format!("Map returned pData {:?} RowPitch {pitch} for rows of {row} bytes", mapped.pData))
    } else {
        let mut out = vec![0u8; row * h as usize];
        let src = mapped.pData as *const u8;
        for y in 0..h as usize {
            // SAFETY: the mapped texture has `desc.Height >= h` rows of `pitch` bytes, and `row <= pitch`.
            let line = unsafe { std::slice::from_raw_parts(src.add(y * pitch), row) };
            out[y * row..(y + 1) * row].copy_from_slice(line);
        }
        for p in out.as_chunks_mut::<4>().0 {
            p[3] = 255;
        }
        Ok((w, h, out))
    };
    // SAFETY: unmaps the subresource mapped above.
    unsafe { gpu.context.Unmap(st, 0) };
    result
}

/// One monitor through GDI (BitBlt from the screen DC).
pub fn gdi_monitor(m: &MonitorInfo) -> Result<Photo, String> {
    gdi_rect(&m.device_name, m.rect, SRCCOPY)
}

/// Runs the closure when dropped: cleanup that must happen on every path (GDI objects, the cursor).
pub struct Defer<F: FnMut()>(pub F);

impl<F: FnMut()> Drop for Defer<F> {
    fn drop(&mut self) {
        (self.0)();
    }
}

fn gdi_rect(device_name: &str, rect: PxRect, rop: ROP_CODE) -> Result<Photo, String> {
    let (w, h) = (rect.width(), rect.height());
    if w <= 0 || h <= 0 {
        return Err(format!("gdi: empty rectangle {rect}"));
    }
    // SAFETY: plain GDI calls; every handle is released by a Defer declared right after it, and the
    // DIB bits are read only while the bitmap is alive.
    unsafe {
        let screen = GetDC(None);
        if screen.is_invalid() {
            return Err("gdi: GetDC(NULL) failed".to_string());
        }
        let _release_screen = Defer(|| {
            if ReleaseDC(None, screen) != 1 {
                log::line("ERROR: gdi: ReleaseDC failed");
            }
        });
        let mem = CreateCompatibleDC(Some(screen));
        if mem.is_invalid() {
            return Err("gdi: CreateCompatibleDC failed".to_string());
        }
        let _delete_mem = Defer(|| {
            if !DeleteDC(mem).as_bool() {
                log::line("ERROR: gdi: DeleteDC failed");
            }
        });
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                // Negative height: top-down rows.
                biHeight: -h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
        let bmp = CreateDIBSection(Some(screen), &bmi, DIB_RGB_COLORS, &mut bits, None, 0).ctx("gdi: CreateDIBSection")?;
        if bits.is_null() {
            return Err("gdi: CreateDIBSection returned no bits".to_string());
        }
        let _delete_bmp = Defer(|| {
            if !DeleteObject(HGDIOBJ(bmp.0)).as_bool() {
                log::line("ERROR: gdi: DeleteObject(bitmap) failed");
            }
        });
        let old = SelectObject(mem, HGDIOBJ(bmp.0));
        if old.is_invalid() {
            return Err("gdi: SelectObject failed".to_string());
        }
        let _unselect = Defer(|| {
            if SelectObject(mem, old).is_invalid() {
                log::line("ERROR: gdi: restoring the DC's bitmap failed");
            }
        });
        let t0 = log::now_ms();
        BitBlt(mem, 0, 0, w, h, Some(screen), rect.left, rect.top, rop).ctx("gdi: BitBlt")?;
        let taken = log::now_ms();
        if !GdiFlush().as_bool() {
            return Err("gdi: GdiFlush failed".to_string());
        }
        let n = (w as usize) * (h as usize) * 4;
        let mut bgra = std::slice::from_raw_parts(bits as *const u8, n).to_vec();
        for p in bgra.as_chunks_mut::<4>().0 {
            p[3] = 255;
        }
        log::line(format!(
            "gdi: {device_name} {w}x{h} rop 0x{:08x}: BitBlt {:.3} ms, total with copy {:.3} ms",
            rop.0,
            taken - t0,
            log::now_ms() - t0
        ));
        Ok(Photo {
            device_name: device_name.to_string(),
            rect,
            width: w as u32,
            height: h as u32,
            bgra,
            method: CaptureMethod::Gdi,
            taken_ms: taken,
        })
    }
}

/// Dispatches every pending message of this thread (windows created here, COM/WinRT callbacks).
pub fn pump_messages() {
    let mut msg = MSG::default();
    // SAFETY: standard message loop on this thread's queue.
    unsafe {
        while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
            // TranslateMessage's return only says whether a character message was posted.
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

/// The frozen-screen photo: DXGI on every monitor, GDI for any monitor where DXGI failed.
/// All or nothing: an error if any monitor fails both ways.
/// Creates (and drops) its own `DxgiSession`: the caller must not hold one (one duplication per output).
pub fn frozen_photo() -> Result<Vec<Photo>, String> {
    let t0 = log::now_ms();
    let monitors = monitors::enumerate()?;
    let mut by_name: HashMap<String, Result<Photo, String>> = match DxgiSession::new() {
        Ok(mut s) => s.grab_all().into_iter().collect(),
        Err(e) => {
            log::line(format!("frozen: DXGI session failed, GDI for every monitor: {e}"));
            HashMap::new()
        }
    };
    let mut photos = Vec::new();
    for m in &monitors {
        let photo = match by_name.remove(&m.device_name) {
            Some(Ok(p)) => p,
            Some(Err(e)) => {
                log::line(format!("frozen: {} DXGI failed ({e}); falling back to GDI", m.device_name));
                gdi_monitor(m).map_err(|g| format!("{}: DXGI failed ({e}) and GDI failed ({g})", m.device_name))?
            }
            None => {
                log::line(format!("frozen: {} has no DXGI output; falling back to GDI", m.device_name));
                gdi_monitor(m).map_err(|g| format!("{}: no DXGI output and GDI failed ({g})", m.device_name))?
            }
        };
        if photo.rect != m.rect {
            log::line(format!(
                "frozen: WARNING {} photo rect {} differs from rcMonitor {}",
                m.device_name, photo.rect, m.rect
            ));
        }
        log::line(format!(
            "frozen: {} via {:?}, {}x{}, taken at {:.3} ms",
            m.device_name, photo.method, photo.width, photo.height, photo.taken_ms
        ));
        photos.push(photo);
    }
    for name in by_name.keys() {
        log::line(format!("frozen: DXGI output {name} matches no monitor; ignored"));
    }
    let earliest = photos.iter().map(|p| p.taken_ms).fold(f64::INFINITY, f64::min);
    let latest = photos.iter().map(|p| p.taken_ms).fold(f64::NEG_INFINITY, f64::max);
    log::line(format!(
        "frozen: {} monitors in {:.3} ms; taken_ms spread (latest - earliest) {:.3} ms",
        photos.len(),
        log::now_ms() - t0,
        latest - earliest
    ));
    Ok(photos)
}

pub fn capture_mode(args: &Args) -> Result<(), String> {
    let hold_secs = args.num("hold-secs", 3)?;
    let save = !args.flag("no-save");
    let out_dir = log::out_dir();
    let monitors = monitors::enumerate()?;
    for m in &monitors {
        log::line(format!(
            "monitor {} '{}' rcMonitor {} dpi {} ({:.0}%) primary {}",
            m.device_name,
            m.friendly_name,
            m.rect,
            m.dpi,
            m.scale() * 100.0,
            m.primary
        ));
    }
    log::line(format!(
        "dxgi: later grabs use AcquireNextFrame timeout {LATER_TIMEOUT_MS} ms; a cached image's taken_ms is the time DXGI confirmed nothing changed"
    ));

    log::line("--- DXGI cold and warm");
    // GDI grabs of every monitor before any duplication exists, to see later whether an open
    // duplication makes Windows draw a capture border (the same check as for WGC).
    let mut before_dxgi = Vec::new();
    for m in &monitors {
        before_dxgi.push(gdi_monitor(m));
    }
    let t0 = log::now_ms();
    let mut session = DxgiSession::new()?;
    let t1 = log::now_ms();
    let first = session.grab_all();
    let t2 = log::now_ms();
    for (name, r) in &first {
        match r {
            Ok(p) => log::line(format!("dxgi cold: {name} {}x{} at {:.3} ms", p.width, p.height, p.taken_ms)),
            Err(e) => log::line(format!("dxgi cold: {name} ERROR {e}")),
        }
    }
    log::line(format!(
        "RESULT dxgi-cold-ms: {:.3} (DxgiSession::new {:.3} + first grab_all {:.3}), {}/{} outputs OK",
        t2 - t0,
        t1 - t0,
        t2 - t1,
        first.iter().filter(|(_, r)| r.is_ok()).count(),
        first.len()
    ));
    let second = session.grab_all();
    let t3 = log::now_ms();
    log::line(format!(
        "RESULT dxgi-warm-ms: {:.3} (second grab_all), {}/{} outputs OK",
        t3 - t2,
        second.iter().filter(|(_, r)| r.is_ok()).count(),
        second.len()
    ));
    let mut warm = Vec::new();
    for _ in 0..5 {
        let a = log::now_ms();
        let r = session.grab_all();
        warm.push(format!("{:.3}{}", log::now_ms() - a, if r.iter().all(|(_, x)| x.is_ok()) { "" } else { "(err)" }));
    }
    log::line(format!("dxgi warm series (ms): {}", warm.join(", ")));

    // The session is still open: give a border time to appear, then grab the edges through GDI.
    let until = log::now_ms() + 700.0;
    while log::now_ms() < until {
        pump_messages();
        std::thread::sleep(Duration::from_millis(10));
    }
    for (m, before) in monitors.iter().zip(&before_dxgi) {
        let label = short_name(&m.device_name);
        let verdict = match (before, gdi_monitor(m)) {
            (Ok(b), Ok(d)) => wgc::edge_yellow(b, &d).unwrap_or_else(|e| format!("ERROR {e}")),
            (Err(e), _) => format!("skipped, the GDI grab before the session failed: {e}"),
            (_, Err(e)) => format!("skipped, the GDI grab during the session failed: {e}"),
        };
        log::line(format!("RESULT dxgi-border-pixels {label}: {verdict}"));
    }

    for m in &monitors {
        let label = short_name(&m.device_name);
        log::line(format!("--- {} ({label}), back to back", m.device_name));
        let mut shots: Vec<(&str, Photo)> = Vec::new();
        let mut timed = |name: &'static str, f: &mut dyn FnMut() -> Result<Photo, String>| {
            let a = log::now_ms();
            let r = f();
            let ms = log::now_ms() - a;
            match r {
                Ok(p) => {
                    let same = p.width as i32 == m.rect.width() && p.height as i32 == m.rect.height();
                    log::line(format!(
                        "RESULT capture-size {name} {label}: {}x{} vs rcMonitor {}x{} -> {} ({ms:.3} ms)",
                        p.width,
                        p.height,
                        m.rect.width(),
                        m.rect.height(),
                        if same { "MATCH" } else { "MISMATCH" }
                    ));
                    shots.push((name, p));
                }
                Err(e) => log::line(format!("RESULT capture-size {name} {label}: ERROR {e} ({ms:.3} ms)")),
            }
        };
        timed("dxgi", &mut || session.grab(&m.device_name));
        timed("gdi", &mut || gdi_monitor(m));
        timed("gdi-captureblt", &mut || gdi_rect(&m.device_name, m.rect, ROP_CODE(SRCCOPY.0 | CAPTUREBLT.0)));
        // The WGC calls include the hold and the GDI border checks; RESULT wgc-time has the capture alone.
        let mut during: Vec<(String, Photo)> = Vec::new();
        for (name, borderless) in [("wgc-border", false), ("wgc-borderless", true)] {
            timed(name, &mut || {
                wgc::capture_monitor(m, borderless, hold_secs).map(|shot| {
                    if let Some(p) = shot.during {
                        during.push((format!("gdi-during-{name}"), p));
                    }
                    shot.photo
                })
            });
        }

        let find = |n: &str| shots.iter().find(|(k, _)| *k == n).map(|(_, p)| p);
        for (a, b) in [("dxgi", "gdi"), ("gdi", "gdi-captureblt"), ("dxgi", "wgc-border"), ("dxgi", "wgc-borderless")] {
            let (Some(pa), Some(pb)) = (find(a), find(b)) else {
                log::line(format!("RESULT diff {a}-vs-{b} {label}: skipped, a capture failed"));
                continue;
            };
            match image::diff(pa, pb, &[]) {
                Ok(s) => log::line(format!(
                    "RESULT diff {a}-vs-{b} {label}: {} of {} pixels differ, max delta {}, first at {:?}; captures {:.1} ms apart, so screen changes count too",
                    s.differing,
                    s.total,
                    s.max_delta,
                    s.first,
                    (pb.taken_ms - pa.taken_ms).abs()
                )),
                Err(e) => log::line(format!("RESULT diff {a}-vs-{b} {label}: ERROR {e}")),
            }
        }
        if save {
            let all = shots.iter().map(|(n, p)| (n.to_string(), p)).chain(during.iter().map(|(n, p)| (n.clone(), p)));
            for (name, p) in all {
                let path = out_dir.join(format!("capture-{name}-{label}.png"));
                let a = log::now_ms();
                match image::save_png(p, &path) {
                    Ok(()) => log::line(format!("saved {} ({:.1} ms)", path.display(), log::now_ms() - a)),
                    Err(e) => log::line(format!("ERROR saving {}: {e}", path.display())),
                }
            }
        }
    }
    // frozen_photo creates its own duplications, so this session must be gone first.
    drop(session);

    log::line("--- frozen_photo()");
    match frozen_photo() {
        Ok(photos) => {
            let methods: Vec<String> =
                photos.iter().map(|p| format!("{}={:?}", short_name(&p.device_name), p.method)).collect();
            log::line(format!("RESULT frozen-photo: OK, {}", methods.join(" ")));
        }
        Err(e) => log::line(format!("RESULT frozen-photo: ERROR {e}")),
    }
    Ok(())
}
