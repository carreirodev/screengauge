//! The capture behind the frozen screen: every monitor through DXGI Desktop Duplication, without the mouse pointer, one
//! right after the other, and through GDI on a monitor where DXGI fails (v1 spec WPC-296, Frozen screen), with the
//! place, scale and colors the session core receives with each photo. DXGI's image is in 8 bits on a monitor with HDR
//! off, and in 16 bits converted by the monitor's SDR white level on one with HDR on (Colors and HDR), so the
//! conversion happens once per call. Everything DXGI, Direct3D and GDI create for it is released before
//! [`every_monitor`] returns, so the capture exists only during the call.

mod gdi;
mod sdr_white_level;

use std::time::{Duration, Instant};

use screengauge_core::hdr::{self, SdrWhiteLevel};
use screengauge_core::session::{Colors, Image, Photo, Rect, Scale};
use windows::Win32::Foundation::{E_FAIL, HMODULE, LPARAM, RECT};
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_UNKNOWN;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_FLAG, D3D11_MAP_READ, D3D11_MAPPED_SUBRESOURCE,
    D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING, D3D11CreateDevice, ID3D11Device,
    ID3D11DeviceContext, ID3D11Texture2D,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_COLOR_SPACE_RGB_FULL_G2084_NONE_P2020, DXGI_COLOR_SPACE_TYPE, DXGI_FORMAT,
    DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_R16G16B16A16_FLOAT, DXGI_MODE_ROTATION_IDENTITY,
    DXGI_MODE_ROTATION_UNSPECIFIED, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, DXGI_ERROR_ACCESS_LOST, DXGI_ERROR_NOT_FOUND, DXGI_ERROR_WAIT_TIMEOUT,
    DXGI_OUTDUPL_FRAME_INFO, IDXGIAdapter, IDXGIAdapter1, IDXGIFactory1, IDXGIOutput, IDXGIOutput5,
    IDXGIOutput6, IDXGIOutputDuplication,
};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO, MONITORINFOEXW,
};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::core::{BOOL, Error, Interface};

use self::sdr_white_level::SdrWhiteLevels;
use crate::log;
use crate::win32::struct_size;

/// How long a new duplication may take to deliver its first desktop image.
const FIRST_IMAGE_BUDGET: Duration = Duration::from_millis(500);
/// How long each wait for that image lasts, in milliseconds.
const WAIT_MS: u32 = 100;

/// Takes the photo of every monitor, in the order Windows lists them: through DXGI, then through GDI for each monitor
/// DXGI couldn't photograph. A monitor neither path photographs comes back as `None`, so the session core keeps the
/// tools closed (all or nothing), and both failures go to the log. Only failing to list the monitors fails the call.
pub fn every_monitor() -> windows::core::Result<Vec<Option<Photo>>> {
    let monitors = monitors()?;
    let shots = one_per_monitor(&monitors, grab_every_output(), |monitor| {
        gdi::image(monitor.rect)
    });
    Ok(monitors
        .iter()
        .zip(shots)
        .map(|(monitor, shot)| shot.into_photo(&monitor.device))
        .collect())
}

/// One monitor's photo and the path that took it, or why neither path could.
#[derive(Debug)]
enum Shot {
    Dxgi(Photo),
    /// DXGI failed with `dxgi`, and GDI took the photo.
    Gdi {
        photo: Photo,
        dxgi: Error,
    },
    Failed {
        dxgi: Error,
        gdi: Error,
    },
}

impl Shot {
    /// The photo, when there is one. How the paths failed goes to the log, which names the monitor by its device.
    fn into_photo(self, device: &str) -> Option<Photo> {
        match self {
            Shot::Dxgi(photo) => Some(photo),
            Shot::Gdi { photo, dxgi } => {
                log::failure(
                    &format!("capture: {device} went through GDI, since DXGI failed"),
                    &dxgi,
                );
                Some(photo)
            }
            Shot::Failed { dxgi, gdi } => {
                log::failure(&format!("capture: {device} through DXGI"), &dxgi);
                log::failure(&format!("capture: {device} through GDI"), &gdi);
                None
            }
        }
    }
}

/// A monitor as Windows arranges it: the list every photo must match.
struct Monitor {
    /// The GDI device name, such as `\\.\DISPLAY1`; DXGI names its outputs the same way.
    device: String,
    /// In physical pixels of the virtual screen.
    rect: RECT,
    scale: Scale,
}

/// What one DXGI output gave.
struct Grab {
    device: String,
    /// HDR was on for the output's monitor: at the start of the photo, or for part of it when it was turned on or off.
    hdr: bool,
    image: windows::core::Result<Desktop>,
}

/// An output's desktop image as a photo's image, with how exact its colors are.
#[derive(Debug)]
struct Desktop {
    image: Image,
    colors: Colors,
}

/// How an output's desktop image becomes a photo: the rule is per monitor (v1 spec WPC-296, Colors and HDR).
#[derive(Debug, Clone, PartialEq)]
enum Path {
    /// HDR is off: the 8-bit image, with the colors the apps drew.
    Sdr,
    /// HDR is on and its SDR white level was read: the 16-bit image converted by it, the exact HDR path.
    Hdr(SdrWhiteLevel),
    /// HDR is on but its SDR white level couldn't be read, for this reason: the 8-bit image, with Windows' brightening
    /// in it, so its colors are approximate.
    Brightened(Error),
}

impl Path {
    /// The path of an output with HDR on or off. The 16-bit image is only for HDR on, because AMD's comes wrong with HDR
    /// off (WPC-289), so `white`, which reads the SDR white level, is asked only then.
    fn of(hdr: bool, white: impl FnOnce() -> windows::core::Result<SdrWhiteLevel>) -> Path {
        if !hdr {
            return Path::Sdr;
        }
        match white() {
            Ok(white) => Path::Hdr(white),
            Err(e) => Path::Brightened(e),
        }
    }

    fn hdr(&self) -> bool {
        !matches!(self, Path::Sdr)
    }

    /// The formats the duplication asks DXGI for, which converts the desktop to one of them when it's in none. B8G8R8A8
    /// is always among them, as Microsoft recommends, and its image is then approximate on a monitor with HDR on.
    fn formats(&self) -> &'static [DXGI_FORMAT] {
        match self {
            Path::Hdr(_) => &[DXGI_FORMAT_R16G16B16A16_FLOAT, DXGI_FORMAT_B8G8R8A8_UNORM],
            Path::Sdr | Path::Brightened(_) => &[DXGI_FORMAT_B8G8R8A8_UNORM],
        }
    }

    /// What becomes of a desktop image DXGI handed in `format`.
    fn reading(&self, format: DXGI_FORMAT) -> windows::core::Result<Reading> {
        match (self, format) {
            (Path::Sdr, DXGI_FORMAT_B8G8R8A8_UNORM) => Ok(Reading::Copy(Colors::Exact)),
            (Path::Hdr(_) | Path::Brightened(_), DXGI_FORMAT_B8G8R8A8_UNORM) => {
                Ok(Reading::Copy(Colors::Approximate))
            }
            (Path::Hdr(white), DXGI_FORMAT_R16G16B16A16_FLOAT) => Ok(Reading::Convert(*white)),
            _ => Err(Error::new(
                E_FAIL,
                format!("the desktop image came in format {}", format.0),
            )),
        }
    }
}

/// How a desktop image is read into a photo's image.
#[derive(Debug, PartialEq)]
enum Reading {
    /// 8 bits, copied as they are, with these colors.
    Copy(Colors),
    /// 16 bits, converted by this SDR white level: exact colors.
    Convert(SdrWhiteLevel),
}

fn monitors() -> windows::core::Result<Vec<Monitor>> {
    extern "system" fn push(monitor: HMONITOR, _: HDC, _: *mut RECT, list: LPARAM) -> BOOL {
        // SAFETY: `list` is the vector `monitors` passes below, alive and not otherwise borrowed during the call.
        unsafe { &mut *(list.0 as *mut Vec<HMONITOR>) }.push(monitor);
        true.into()
    }
    let mut handles: Vec<HMONITOR> = Vec::new();
    // SAFETY: the callback only pushes to `handles`, which outlives the call.
    let listed =
        unsafe { EnumDisplayMonitors(None, None, Some(push), LPARAM((&raw mut handles) as isize)) };
    if !listed.as_bool() {
        return Err(Error::new(E_FAIL, "EnumDisplayMonitors returned FALSE"));
    }
    handles
        .into_iter()
        .map(|handle| {
            let mut info = MONITORINFOEXW::default();
            info.monitorInfo.cbSize = const { struct_size::<MONITORINFOEXW>() };
            // SAFETY: MONITORINFOEXW starts with MONITORINFO, and its cbSize lets Windows fill the device name too.
            if !unsafe { GetMonitorInfoW(handle, (&raw mut info).cast::<MONITORINFO>()) }.as_bool()
            {
                return Err(Error::new(E_FAIL, "GetMonitorInfoW returned FALSE"));
            }
            // Windows gives the same DPI on both axes.
            let (mut dpi, mut dpi_y) = (0, 0);
            // SAFETY: a monitor EnumDisplayMonitors just listed; both out pointers are locals. The process is
            // Per-Monitor V2, so this is the monitor's own DPI.
            unsafe { GetDpiForMonitor(handle, MDT_EFFECTIVE_DPI, &mut dpi, &mut dpi_y) }?;
            let scale = Scale::from_dpi(dpi)
                .ok_or_else(|| Error::new(E_FAIL, "GetDpiForMonitor returned a DPI of 0"))?;
            Ok(Monitor {
                device: device_name(&info.szDevice),
                rect: info.monitorInfo.rcMonitor,
                scale,
            })
        })
        .collect()
}

/// A device name from its null-terminated UTF-16 buffer.
fn device_name(buffer: &[u16]) -> String {
    let end = buffer
        .iter()
        .position(|&unit| unit == 0)
        .unwrap_or(buffer.len());
    String::from_utf16_lossy(&buffer[..end])
}

/// The current image of every output attached to the desktop. Every device and duplication is made first, so the
/// images come one right after the other.
fn grab_every_output() -> windows::core::Result<Vec<Grab>> {
    Ok(duplicate_every_output()?
        .into_iter()
        .map(|output| {
            let (image, hdr) = match output.duplication {
                Ok(duplication) => duplication.image(),
                Err(e) => (Err(e), output.hdr),
            };
            Grab {
                device: output.device,
                hdr,
                image,
            }
        })
        .collect())
}

/// An output attached to the desktop, ready to give its image.
struct Output {
    device: String,
    hdr: bool,
    duplication: windows::core::Result<Duplication>,
}

/// The SDR white level of `device`, read through `levels`, which are listed on the first monitor with HDR on of each
/// call: none is read while HDR is off everywhere.
fn white_level(
    levels: &mut Option<windows::core::Result<SdrWhiteLevels>>,
    device: &str,
) -> windows::core::Result<SdrWhiteLevel> {
    match levels.get_or_insert_with(SdrWhiteLevels::now) {
        Ok(levels) => levels.of(device),
        Err(e) => Err(e.clone()),
    }
}

/// A duplication of every output attached to the desktop, on every adapter (the owner's two monitors are on two
/// graphics cards), or why it couldn't be made.
fn duplicate_every_output() -> windows::core::Result<Vec<Output>> {
    // SAFETY: no arguments; the factory is released when it drops.
    let factory: IDXGIFactory1 = unsafe { CreateDXGIFactory1() }?;
    let mut duplications = Vec::new();
    let mut white_levels = None;
    for index in 0.. {
        // SAFETY: enumerating until DXGI says there is no adapter at `index`.
        let adapter = match unsafe { factory.EnumAdapters1(index) } {
            Ok(adapter) => adapter,
            Err(e) if e.code() == DXGI_ERROR_NOT_FOUND => break,
            Err(e) => return Err(e),
        };
        // Created on the first output that needs it: an adapter without monitors needs no device.
        let mut gpu: Option<windows::core::Result<Gpu>> = None;
        for index in 0.. {
            // SAFETY: as above, for this adapter's outputs.
            let output = match unsafe { adapter.EnumOutputs(index) } {
                Ok(output) => output,
                Err(e) if e.code() == DXGI_ERROR_NOT_FOUND => break,
                Err(e) => return Err(e),
            };
            // IDXGIOutput6 is there on every Windows 11.
            // SAFETY: a plain query on a live output.
            let description = unsafe { output.cast::<IDXGIOutput6>()?.GetDesc1() }?;
            if !description.AttachedToDesktop.as_bool() {
                continue;
            }
            let device = device_name(&description.DeviceName);
            let hdr = is_hdr(description.ColorSpace);
            let path = Path::of(hdr, || white_level(&mut white_levels, &device));
            if let Path::Brightened(e) = &path {
                log::failure(
                    &format!(
                        "capture: the SDR white level of {device}, so its colors are approximate"
                    ),
                    e,
                );
            }
            let duplication = match gpu.get_or_insert_with(|| Gpu::on(&adapter)) {
                Ok(gpu) if is_upright(description.Rotation) => Duplication::of(gpu, output, path),
                Ok(_) => Err(Error::new(
                    E_FAIL,
                    "the monitor is rotated, and DXGI gives its image unrotated",
                )),
                Err(e) => Err(e.clone()),
            };
            duplications.push(Output {
                device,
                hdr,
                duplication,
            });
        }
    }
    Ok(duplications)
}

/// Whether HDR is on for an output in this color space: Microsoft's documented test (research/hdr-screen-colors).
fn is_hdr(color_space: DXGI_COLOR_SPACE_TYPE) -> bool {
    color_space == DXGI_COLOR_SPACE_RGB_FULL_G2084_NONE_P2020
}

/// A rotated monitor's desktop image comes in the monitor's native orientation, so that monitor goes to GDI, whose
/// photo is upright; turning DXGI's image upright is WPC-344.
fn is_upright(rotation: windows::Win32::Graphics::Dxgi::Common::DXGI_MODE_ROTATION) -> bool {
    rotation == DXGI_MODE_ROTATION_IDENTITY || rotation == DXGI_MODE_ROTATION_UNSPECIFIED
}

/// A Direct3D device on one adapter. DuplicateOutput needs the device of the adapter the output is connected to.
#[derive(Clone)]
struct Gpu {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
}

impl Gpu {
    fn on(adapter: &IDXGIAdapter1) -> windows::core::Result<Gpu> {
        let adapter: IDXGIAdapter = adapter.cast()?;
        let mut device = None;
        let mut context = None;
        // SAFETY: the out pointers are locals that outlive the call; with an adapter, the driver type must be UNKNOWN.
        unsafe {
            D3D11CreateDevice(
                &adapter,
                D3D_DRIVER_TYPE_UNKNOWN,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_FLAG::default(),
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )
        }?;
        match (device, context) {
            (Some(device), Some(context)) => Ok(Gpu { device, context }),
            _ => Err(Error::new(
                E_FAIL,
                "D3D11CreateDevice returned no device or context",
            )),
        }
    }
}

/// An output's duplication, with what it takes to read its image and to make it again.
struct Duplication {
    gpu: Gpu,
    output: IDXGIOutput,
    path: Path,
    duplication: IDXGIOutputDuplication,
}

impl Duplication {
    /// Asks DXGI for the formats `path` takes.
    fn of(gpu: &Gpu, output: IDXGIOutput, path: Path) -> windows::core::Result<Duplication> {
        // IDXGIOutput5 is there on every Windows 11.
        let output5: IDXGIOutput5 = output.cast()?;
        // SAFETY: the device was created on the adapter this output is connected to.
        let duplication = unsafe { output5.DuplicateOutput1(&gpu.device, 0, path.formats()) }?;
        Ok(Duplication {
            gpu: gpu.clone(),
            output,
            path,
            duplication,
        })
    }

    /// The output's current image, and whether HDR was on for its monitor during the photo, which marks a GDI photo
    /// taken in its place. The duplication is released before this returns.
    fn image(self) -> (windows::core::Result<Desktop>, bool) {
        let Duplication {
            gpu,
            output,
            path,
            duplication,
        } = self;
        let first = first_image(&gpu, &duplication, &path);
        // Windows allows one duplication per output and process at a time, so the first goes before another is made.
        drop(duplication);
        let desktop = match first {
            // The duplication was lost, as when the display mode or the desktop changes, and a new one gets one more
            // try, unless HDR was turned on or off, which `settled` then tells.
            Err(e)
                if e.code() == DXGI_ERROR_ACCESS_LOST
                    && hdr_now(&output).is_ok_and(|now| now == path.hdr()) =>
            {
                Duplication::of(&gpu, output.clone(), path.clone())
                    .and_then(|again| first_image(&again.gpu, &again.duplication, &again.path))
            }
            first => first,
        };
        settled(desktop, path.hdr(), hdr_now(&output))
    }
}

/// Whether HDR is on for `output`'s monitor now.
fn hdr_now(output: &IDXGIOutput) -> windows::core::Result<bool> {
    // SAFETY: a plain query on a live output.
    let description = unsafe { output.cast::<IDXGIOutput6>()?.GetDesc1() }?;
    Ok(is_hdr(description.ColorSpace))
}

/// A photo taken on a path chosen with HDR on or off (`chosen`), once HDR is read again after it (`now`), and whether
/// HDR was on for the monitor during it. HDR turned on or off between choosing the path and the end of the photo,
/// losing the duplication or not, leaves a path for the old mode, which would mark a brightened photo exact or take
/// AMD's wrong 16-bit one (WPC-289): the monitor goes to GDI, marked as HDR on, since it was for part of the photo.
fn settled(
    desktop: windows::core::Result<Desktop>,
    chosen: bool,
    now: windows::core::Result<bool>,
) -> (windows::core::Result<Desktop>, bool) {
    match now {
        Ok(now) if now == chosen => (desktop, chosen),
        Ok(_) => (
            Err(Error::new(
                E_FAIL,
                "HDR was turned on or off during the photo",
            )),
            true,
        ),
        // Unread, the photo's marks can't be trusted; a photo that had already failed keeps its own cause.
        Err(e) => (desktop.and(Err(e)), chosen),
    }
}

/// Waits for the duplication's first desktop image and reads it into memory as `path` says. An error between acquiring
/// a frame and releasing it leaves the frame to the duplication, which the caller drops right away.
fn first_image(
    gpu: &Gpu,
    duplication: &IDXGIOutputDuplication,
    path: &Path,
) -> windows::core::Result<Desktop> {
    let deadline = Instant::now() + FIRST_IMAGE_BUDGET;
    loop {
        let mut info = DXGI_OUTDUPL_FRAME_INFO::default();
        let mut resource = None;
        // SAFETY: both out pointers are locals that outlive the call.
        match unsafe { duplication.AcquireNextFrame(WAIT_MS, &mut info, &mut resource) } {
            Ok(()) => {
                // LastPresentTime 0: only the mouse pointer changed, and the frame may hold no desktop image yet.
                let image = match resource {
                    Some(resource) if info.LastPresentTime != 0 => {
                        Some(read(gpu, &resource.cast()?, path)?)
                    }
                    _ => None,
                };
                // SAFETY: releases the frame acquired above; the image was copied out of it.
                unsafe { duplication.ReleaseFrame() }?;
                if let Some(image) = image {
                    return Ok(image);
                }
            }
            // Nothing new on screen within the wait; waiting again is the only thing to do, until the deadline.
            Err(e) if e.code() == DXGI_ERROR_WAIT_TIMEOUT => {}
            Err(e) => return Err(e),
        }
        if Instant::now() >= deadline {
            return Err(Error::new(
                DXGI_ERROR_WAIT_TIMEOUT,
                format!(
                    "no desktop image within {} ms",
                    FIRST_IMAGE_BUDGET.as_millis()
                ),
            ));
        }
    }
}

/// Copies the desktop texture to memory the processor can read, through a staging texture, and reads it as `path`
/// says: an 8-bit image as it is, a 16-bit one converted.
fn read(gpu: &Gpu, texture: &ID3D11Texture2D, path: &Path) -> windows::core::Result<Desktop> {
    let mut description = D3D11_TEXTURE2D_DESC::default();
    // SAFETY: fills a local.
    unsafe { texture.GetDesc(&mut description) };
    let reading = path.reading(description.Format)?;
    let staging_description = D3D11_TEXTURE2D_DESC {
        Width: description.Width,
        Height: description.Height,
        MipLevels: 1,
        ArraySize: 1,
        Format: description.Format,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Usage: D3D11_USAGE_STAGING,
        BindFlags: 0,
        CPUAccessFlags: D3D11_CPU_ACCESS_READ.0.cast_unsigned(),
        MiscFlags: 0,
    };
    let mut staging = None;
    // SAFETY: a valid description of a staging texture; the out pointer is a local.
    unsafe {
        gpu.device
            .CreateTexture2D(&staging_description, None, Some(&mut staging))
    }?;
    let staging =
        staging.ok_or_else(|| Error::new(E_FAIL, "CreateTexture2D returned no texture"))?;
    // SAFETY: both textures are on `gpu.device`, with the same size and format.
    unsafe { gpu.context.CopyResource(&staging, texture) };
    let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
    // SAFETY: the staging texture allows reading; Map waits for the copy to finish.
    unsafe {
        gpu.context
            .Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
    }?;
    let (width, height) = (description.Width, description.Height);
    let desktop = match reading {
        Reading::Copy(colors) => mapped_image(&mapped, width, height, 4, |rows, pitch| {
            tight_rows(rows, pitch, count(width)?, count(height)?)
                .and_then(|bgra| Image::new(width, height, bgra))
        })
        .map(|image| Desktop { image, colors }),
        Reading::Convert(white) => mapped_image(&mapped, width, height, 8, |rows, pitch| {
            hdr::convert(rows, pitch, width, height, white)
        })
        .map(|image| Desktop {
            image,
            colors: Colors::Exact,
        }),
    };
    // SAFETY: unmaps what was mapped above, after `mapped_image` read it.
    unsafe { gpu.context.Unmap(&staging, 0) };
    desktop
}

/// A size or count Windows gives in a u32, such as a width or height, as a usize. `None` never happens on x64, the only
/// target ScreenGauge builds for.
fn count(size: u32) -> Option<usize> {
    usize::try_from(size).ok()
}

/// The mapped rows of a `width` by `height` texture of `pixel_size`-byte pixels, as `image` makes them an [`Image`],
/// from the rows and the bytes between their starts. `image` gives `None` when the rows don't fill the image.
fn mapped_image(
    mapped: &D3D11_MAPPED_SUBRESOURCE,
    width: u32,
    height: u32,
    pixel_size: usize,
    image: impl FnOnce(&[u8], usize) -> Option<Image>,
) -> windows::core::Result<Image> {
    let too_large = || Error::new(E_FAIL, "the desktop image is too large to address");
    let (columns, rows) = (
        count(width).ok_or_else(too_large)?,
        count(height).ok_or_else(too_large)?,
    );
    let pitch = usize::try_from(mapped.RowPitch).map_err(|_| too_large())?;
    let row_length = columns.checked_mul(pixel_size).ok_or_else(too_large)?;
    // Every row but the last is `pitch` bytes long; the last may end right after its pixels.
    let length = match rows.checked_sub(1) {
        Some(before_last) => before_last
            .checked_mul(pitch)
            .and_then(|start| start.checked_add(row_length))
            .ok_or_else(too_large)?,
        None => 0,
    };
    if mapped.pData.is_null() || pitch < row_length {
        return Err(Error::new(E_FAIL, "Map returned no rows of that size"));
    }
    // SAFETY: Map succeeded for a `width` by `height` texture of `pixel_size`-byte pixels, so `pData` holds `rows` rows
    // `pitch` bytes apart, each with `columns` pixels; the slice ends at the last pixel and is dropped before Unmap.
    let mapped_rows = unsafe { std::slice::from_raw_parts(mapped.pData.cast::<u8>(), length) };
    image(mapped_rows, pitch)
        .ok_or_else(|| Error::new(E_FAIL, "Map returned fewer rows than the image has"))
}

/// Copies `rows` rows of `columns` BGRA pixels that start `pitch` bytes apart into rows with no gap, and makes every
/// pixel opaque: the desktop image's alpha bytes say nothing. `None` when `mapped` is too short.
fn tight_rows(mapped: &[u8], pitch: usize, columns: usize, rows: usize) -> Option<Vec<u8>> {
    let row_length = columns.checked_mul(4)?;
    let mut bgra = Vec::with_capacity(row_length.checked_mul(rows)?);
    for row in 0..rows {
        let start = row.checked_mul(pitch)?;
        bgra.extend_from_slice(mapped.get(start..start.checked_add(row_length)?)?);
    }
    for pixel in bgra.as_chunks_mut::<4>().0 {
        pixel[3] = u8::MAX;
    }
    Some(bgra)
}

/// Pairs every monitor with its photo, in the monitors' order: DXGI's image when it has the monitor's exact size, and
/// otherwise `gdi`'s, which is asked only for those monitors. `dxgi` is an error when DXGI failed for every monitor at
/// once. A monitor neither path photographs gets [`Shot::Failed`], and the others still get their photos.
fn one_per_monitor(
    monitors: &[Monitor],
    dxgi: windows::core::Result<Vec<Grab>>,
    mut gdi: impl FnMut(&Monitor) -> windows::core::Result<Image>,
) -> Vec<Shot> {
    let (mut grabs, dxgi_failure) = match dxgi {
        Ok(grabs) => (grabs, None),
        Err(e) => (Vec::new(), Some(e)),
    };
    monitors
        .iter()
        .map(|monitor| {
            let grab = grabs
                .iter()
                .position(|grab| grab.device == monitor.device)
                .map(|index| grabs.swap_remove(index));
            // Whether HDR is on comes from DXGI's description of the monitor. Without one, HDR counts as off, since the
            // Approximate color notice says "HDR on". With HDR on, GDI's photo still measured exact (WPC-289); the spec
            // marks it approximate only because nothing promises that, so a missed mark is unlikely to hide a wrong color.
            let hdr = grab.as_ref().is_some_and(|grab| grab.hdr);
            let dxgi_desktop = match grab {
                Some(grab) => grab.image.and_then(|desktop| {
                    Ok(Desktop {
                        image: fits(monitor, desktop.image)?,
                        colors: desktop.colors,
                    })
                }),
                None => Err(dxgi_failure
                    .clone()
                    .unwrap_or_else(|| Error::new(E_FAIL, "DXGI has no image of this monitor"))),
            };
            match dxgi_desktop {
                // DXGI's colors are as the monitor's path read them (Path::reading).
                Ok(desktop) => Shot::Dxgi(photo(monitor, desktop.image, desktop.colors)),
                Err(dxgi) => match gdi(monitor).and_then(|image| fits(monitor, image)) {
                    Ok(image) => Shot::Gdi {
                        photo: photo(monitor, image, gdi_colors(hdr)),
                        dxgi,
                    },
                    Err(gdi) => Shot::Failed { dxgi, gdi },
                },
            }
        })
        .collect()
}

/// `image`, when it has the monitor's exact size.
fn fits(monitor: &Monitor, image: Image) -> windows::core::Result<Image> {
    let size = (
        u32::try_from(monitor.rect.right - monitor.rect.left),
        u32::try_from(monitor.rect.bottom - monitor.rect.top),
    );
    if size == (Ok(image.width()), Ok(image.height())) {
        Ok(image)
    } else {
        Err(Error::new(
            E_FAIL,
            format!(
                "the image is {}x{}, and the monitor is {:?}",
                image.width(),
                image.height(),
                monitor.rect
            ),
        ))
    }
}

fn photo(monitor: &Monitor, image: Image, colors: Colors) -> Photo {
    Photo {
        rect: Rect {
            left: monitor.rect.left,
            top: monitor.rect.top,
            width: image.width(),
            height: image.height(),
        },
        scale: monitor.scale,
        colors,
        image,
    }
}

/// GDI's photo is never the exact HDR path (v1 spec WPC-296, Colors and HDR): with HDR on, its colors are approximate,
/// even where DXGI's are exact.
fn gdi_colors(hdr: bool) -> Colors {
    if hdr {
        Colors::Approximate
    } else {
        Colors::Exact
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_lose_their_padding_and_become_opaque() {
        // Two rows of two pixels, 12 bytes apart: 8 bytes of pixels and 4 of padding each.
        let mapped = [
            1, 2, 3, 0, 4, 5, 6, 0, 99, 99, 99, 99, //
            7, 8, 9, 0, 10, 11, 12, 128,
        ];
        assert_eq!(
            tight_rows(&mapped, 12, 2, 2),
            Some(vec![
                1, 2, 3, 255, 4, 5, 6, 255, //
                7, 8, 9, 255, 10, 11, 12, 255,
            ])
        );
    }

    #[test]
    fn rows_without_padding_stay_whole() {
        let mapped = [1, 2, 3, 4, 5, 6, 7, 8];
        assert_eq!(
            tight_rows(&mapped, 4, 1, 2),
            Some(vec![1, 2, 3, 255, 5, 6, 7, 255])
        );
    }

    #[test]
    fn rows_missing_from_the_mapping_are_refused() {
        // Two rows promised, but the bytes stop in the middle of the second one.
        let mapped = [1, 2, 3, 4, 0, 0, 0, 0, 5, 6];
        assert_eq!(tight_rows(&mapped, 8, 1, 2), None);
    }

    const DISPLAY1: &str = r"\\.\DISPLAY1";
    const DISPLAY2: &str = r"\\.\DISPLAY2";
    const DISPLAY3: &str = r"\\.\DISPLAY3";
    const LEFT: RECT = RECT {
        left: -1920,
        top: 0,
        right: 0,
        bottom: 1080,
    };
    const RIGHT: RECT = RECT {
        left: 0,
        top: 0,
        right: 3840,
        bottom: 2160,
    };
    const BELOW: RECT = RECT {
        left: 0,
        top: 2160,
        right: 1920,
        bottom: 3240,
    };

    fn monitor(device: &str, rect: RECT, dpi: u32) -> Monitor {
        Monitor {
            device: device.to_owned(),
            rect,
            scale: Scale::from_dpi(dpi).expect("a real scale"),
        }
    }

    /// The owner's 4K at 175% as DISPLAY1 and Full HD at 125% as DISPLAY2.
    fn owners_monitors() -> [Monitor; 2] {
        [monitor(DISPLAY1, RIGHT, 168), monitor(DISPLAY2, LEFT, 120)]
    }

    /// An image whose first byte tells it apart.
    fn marked(width: u32, height: u32, mark: u8) -> Image {
        let mut bgra = vec![0; usize::try_from(width * height * 4).expect("a small image")];
        bgra[0] = mark;
        Image::new(width, height, bgra).expect("a whole image")
    }

    /// What DXGI gave for `device`: an image marked `mark`, with HDR off.
    fn grab(device: &str, width: u32, height: u32, mark: u8) -> Grab {
        Grab {
            device: device.to_owned(),
            hdr: false,
            image: Ok(Desktop {
                image: marked(width, height, mark),
                colors: Colors::Exact,
            }),
        }
    }

    /// DXGI described `device` but couldn't give its image.
    fn failed_grab(device: &str, hdr: bool) -> Grab {
        Grab {
            device: device.to_owned(),
            hdr,
            image: Err(Error::new(DXGI_ERROR_ACCESS_LOST, "lost")),
        }
    }

    /// GDI's photo of a monitor, marked 9.
    fn gdi_image(monitor: &Monitor) -> windows::core::Result<Image> {
        let width = u32::try_from(monitor.rect.right - monitor.rect.left).expect("a width");
        let height = u32::try_from(monitor.rect.bottom - monitor.rect.top).expect("a height");
        Ok(marked(width, height, 9))
    }

    /// GDI for a test where DXGI photographs every monitor, so GDI must never be asked.
    fn no_gdi(monitor: &Monitor) -> windows::core::Result<Image> {
        panic!(
            "GDI was asked for {}, which DXGI photographed",
            monitor.device
        )
    }

    fn rect(rect: RECT) -> Rect {
        Rect {
            left: rect.left,
            top: rect.top,
            width: u32::try_from(rect.right - rect.left).expect("a monitor's width"),
            height: u32::try_from(rect.bottom - rect.top).expect("a monitor's height"),
        }
    }

    /// Each shot as the path that photographed it, the photo's place, mark and colors, and DXGI's failure, if any.
    fn seen(shots: &[Shot]) -> Vec<(&'static str, Rect, u8, Colors, Option<String>)> {
        shots
            .iter()
            .map(|shot| {
                let (path, photo, dxgi) = match shot {
                    Shot::Dxgi(photo) => ("DXGI", photo, None),
                    Shot::Gdi { photo, dxgi } => ("GDI", photo, Some(dxgi.message())),
                    Shot::Failed { .. } => panic!("no photo: {shot:?}"),
                };
                (path, photo.rect, photo.image.bgra()[0], photo.colors, dxgi)
            })
            .collect()
    }

    #[test]
    fn every_monitor_gets_its_own_dxgi_image_in_the_monitors_order() {
        let grabs = vec![grab(DISPLAY2, 1920, 1080, 2), grab(DISPLAY1, 3840, 2160, 1)];
        let shots = one_per_monitor(&owners_monitors(), Ok(grabs), no_gdi);
        assert_eq!(
            seen(&shots),
            [
                ("DXGI", rect(RIGHT), 1, Colors::Exact, None),
                ("DXGI", rect(LEFT), 2, Colors::Exact, None),
            ]
        );
        let Shot::Dxgi(photo) = &shots[1] else {
            panic!("{:?}", shots[1]);
        };
        assert_eq!(photo.scale.dpi(), 120);
    }

    #[test]
    fn a_dxgi_photo_keeps_the_colors_its_monitor_s_path_read() {
        // Both monitors have HDR on: the first one's image was converted, the second one's came in 8 bits.
        let mut converted = grab(DISPLAY1, 3840, 2160, 1);
        converted.hdr = true;
        let mut brightened = grab(DISPLAY2, 1920, 1080, 2);
        brightened.hdr = true;
        if let Ok(desktop) = &mut brightened.image {
            desktop.colors = Colors::Approximate;
        }
        let shots = one_per_monitor(&owners_monitors(), Ok(vec![converted, brightened]), no_gdi);
        let colors: Vec<_> = seen(&shots).into_iter().map(|shot| shot.3).collect();
        assert_eq!(colors, [Colors::Exact, Colors::Approximate]);
    }

    /// An SDR white level of 240 nits.
    fn white_240() -> SdrWhiteLevel {
        SdrWhiteLevel::from_windows(3000).expect("a white level above zero")
    }

    #[test]
    fn a_monitor_with_hdr_off_takes_8_bits_without_reading_its_sdr_white_level() {
        // AMD's 16-bit image comes wrong with HDR off (WPC-289).
        let path = Path::of(false, || panic!("the SDR white level was read"));
        assert_eq!(path, Path::Sdr);
        assert_eq!(path.formats(), [DXGI_FORMAT_B8G8R8A8_UNORM]);
    }

    #[test]
    fn a_monitor_with_hdr_on_takes_16_bits_converted_by_its_sdr_white_level() {
        let path = Path::of(true, || Ok(white_240()));
        assert_eq!(path, Path::Hdr(white_240()));
        // B8G8R8A8 stays in the list, as Microsoft recommends, should DXGI have no 16-bit image to give.
        assert_eq!(
            path.formats(),
            [DXGI_FORMAT_R16G16B16A16_FLOAT, DXGI_FORMAT_B8G8R8A8_UNORM]
        );
    }

    #[test]
    fn a_monitor_with_hdr_on_whose_sdr_white_level_is_unknown_takes_8_bits() {
        let unread = Error::new(E_FAIL, "no white level");
        let path = Path::of(true, || Err(unread.clone()));
        assert_eq!(path, Path::Brightened(unread));
        assert_eq!(path.formats(), [DXGI_FORMAT_B8G8R8A8_UNORM]);
    }

    #[test]
    fn colors_are_exact_with_hdr_off_or_when_the_16_bit_image_was_converted() {
        // The exact path mark: a 16-bit image with the SDR white level read, or HDR off.
        let brightened = Path::Brightened(Error::new(E_FAIL, "no white level"));
        let readings = [
            (Path::Sdr, DXGI_FORMAT_B8G8R8A8_UNORM),
            (Path::Hdr(white_240()), DXGI_FORMAT_R16G16B16A16_FLOAT),
            (Path::Hdr(white_240()), DXGI_FORMAT_B8G8R8A8_UNORM),
            (brightened, DXGI_FORMAT_B8G8R8A8_UNORM),
        ]
        .map(|(path, format)| path.reading(format).ok());
        assert_eq!(
            readings,
            [
                Some(Reading::Copy(Colors::Exact)),
                Some(Reading::Convert(white_240())),
                Some(Reading::Copy(Colors::Approximate)),
                Some(Reading::Copy(Colors::Approximate)),
            ]
        );
    }

    #[test]
    fn a_16_bit_image_without_an_sdr_white_level_to_convert_by_is_refused() {
        // DXGI hands only formats it was asked for, so this is DXGI going wrong, and the monitor goes to GDI.
        let reading = Path::Sdr.reading(DXGI_FORMAT_R16G16B16A16_FLOAT);
        assert!(reading.is_err(), "{reading:?}");
    }

    #[test]
    fn an_output_without_a_monitor_is_left_out() {
        let monitors = [monitor(DISPLAY1, RIGHT, 168)];
        let grabs = vec![
            grab(DISPLAY1, 3840, 2160, 1),
            grab(r"\\.\DISPLAY7", 1920, 1080, 7),
        ];
        let shots = one_per_monitor(&monitors, Ok(grabs), no_gdi);
        assert_eq!(
            seen(&shots),
            [("DXGI", rect(RIGHT), 1, Colors::Exact, None)]
        );
    }

    #[test]
    fn gdi_photographs_only_the_monitor_dxgi_failed() {
        let grabs = vec![grab(DISPLAY1, 3840, 2160, 1), failed_grab(DISPLAY2, false)];
        let mut asked = Vec::new();
        let shots = one_per_monitor(&owners_monitors(), Ok(grabs), |monitor| {
            asked.push(monitor.device.clone());
            gdi_image(monitor)
        });
        assert_eq!(asked, [DISPLAY2]);
        assert_eq!(
            seen(&shots),
            [
                ("DXGI", rect(RIGHT), 1, Colors::Exact, None),
                (
                    "GDI",
                    rect(LEFT),
                    9,
                    Colors::Exact,
                    Some(String::from("lost"))
                ),
            ]
        );
    }

    #[test]
    fn a_monitor_dxgi_has_no_image_of_goes_to_gdi() {
        let grabs = vec![grab(DISPLAY1, 3840, 2160, 1)];
        let shots = one_per_monitor(&owners_monitors(), Ok(grabs), gdi_image);
        let paths: Vec<_> = seen(&shots).into_iter().map(|shot| shot.0).collect();
        assert_eq!(paths, ["DXGI", "GDI"]);
    }

    #[test]
    fn a_dxgi_image_of_another_size_goes_to_gdi() {
        let grabs = vec![grab(DISPLAY1, 1920, 1080, 1), grab(DISPLAY2, 1920, 1080, 2)];
        let shots = one_per_monitor(&owners_monitors(), Ok(grabs), gdi_image);
        let first = seen(&shots).remove(0);
        assert_eq!((first.0, first.2), ("GDI", 9));
        let dxgi = first.4.expect("DXGI's failure is kept");
        assert!(dxgi.contains("1920x1080"), "{dxgi}");
    }

    #[test]
    fn when_dxgi_fails_as_a_whole_gdi_photographs_every_monitor() {
        let mut asked = Vec::new();
        let shots = one_per_monitor(
            &owners_monitors(),
            Err(Error::new(E_FAIL, "no DXGI factory")),
            |monitor| {
                asked.push(monitor.device.clone());
                gdi_image(monitor)
            },
        );
        assert_eq!(asked, [DISPLAY1, DISPLAY2]);
        let failures: Vec<_> = seen(&shots).into_iter().map(|shot| shot.4).collect();
        let failure = Some(String::from("no DXGI factory"));
        assert_eq!(failures, [failure.clone(), failure]);
    }

    #[test]
    fn a_gdi_photo_is_approximate_only_where_dxgi_saw_hdr_on() {
        // GDI's photo is never the exact HDR path. Whether HDR is on comes from DXGI's description of the monitor, so
        // a monitor DXGI never described counts as HDR off.
        let monitors = [
            monitor(DISPLAY1, RIGHT, 168),
            monitor(DISPLAY2, LEFT, 120),
            monitor(DISPLAY3, BELOW, 96),
        ];
        let grabs = vec![failed_grab(DISPLAY1, false), failed_grab(DISPLAY2, true)];
        let shots = one_per_monitor(&monitors, Ok(grabs), gdi_image);
        let marks: Vec<_> = seen(&shots)
            .into_iter()
            .map(|shot| (shot.0, shot.3))
            .collect();
        assert_eq!(
            marks,
            [
                ("GDI", Colors::Exact),
                ("GDI", Colors::Approximate),
                ("GDI", Colors::Exact),
            ]
        );
    }

    #[test]
    fn a_monitor_neither_path_photographs_keeps_both_causes_and_the_others_keep_their_photos() {
        // All or nothing is the session core's rule: the capture still photographs every other monitor.
        let grabs = vec![failed_grab(DISPLAY1, false), grab(DISPLAY2, 1920, 1080, 2)];
        let shots = one_per_monitor(&owners_monitors(), Ok(grabs), |_| {
            Err(Error::new(E_FAIL, "BitBlt failed"))
        });
        let Shot::Failed { dxgi, gdi } = &shots[0] else {
            panic!("{:?}", shots[0]);
        };
        assert_eq!(
            (dxgi.code(), gdi.message()),
            (DXGI_ERROR_ACCESS_LOST, String::from("BitBlt failed"))
        );
        assert_eq!(
            seen(&shots[1..]),
            [("DXGI", rect(LEFT), 2, Colors::Exact, None)]
        );
    }

    #[test]
    #[ignore = "photographs the real screen, so it runs on the owner's computer, never in CI"]
    fn gdi_photographs_every_real_monitor_as_dxgi_does() {
        // With HDR on, GDI's photo measured exact (WPC-289), so it checks DXGI's converted one, which must be exact.
        let monitors = monitors().expect("the monitors");
        let grabs = grab_every_output().expect("DXGI");
        for monitor in &monitors {
            let gdi = gdi::image(monitor.rect).expect("GDI's photo");
            let grab = grabs
                .iter()
                .find(|grab| grab.device == monitor.device)
                .expect("DXGI's output");
            let Desktop {
                image: dxgi,
                colors,
            } = grab.image.as_ref().expect("DXGI's photo");
            println!("{}: HDR on {}, {colors:?} colors", monitor.device, grab.hdr);
            assert_eq!(*colors, Colors::Exact, "{}", monitor.device);
            assert_eq!(
                (gdi.width(), gdi.height()),
                (dxgi.width(), dxgi.height()),
                "{}",
                monitor.device
            );
            // The two photos are taken a moment apart, so whatever animates on screen (a clock, a spinner) may differ.
            let pixels = gdi.bgra().len() / 4;
            let differing = gdi
                .bgra()
                .as_chunks::<4>()
                .0
                .iter()
                .zip(dxgi.bgra().as_chunks::<4>().0)
                .filter(|(gdi, dxgi)| gdi != dxgi)
                .count();
            println!("{}: {differing} of {pixels} pixels differ", monitor.device);
            assert!(differing * 100 < pixels, "{}", monitor.device);
        }
    }

    #[test]
    fn a_gdi_image_of_another_size_counts_as_gdi_failing() {
        let grabs = vec![failed_grab(DISPLAY1, false)];
        let monitors = [monitor(DISPLAY1, RIGHT, 168)];
        let shots = one_per_monitor(&monitors, Ok(grabs), |_| Ok(marked(1920, 1080, 9)));
        let Shot::Failed { gdi, .. } = &shots[0] else {
            panic!("{:?}", shots[0]);
        };
        assert!(gdi.message().contains("1920x1080"), "{gdi}");
    }

    fn desktop(colors: Colors) -> windows::core::Result<Desktop> {
        Ok(Desktop {
            image: marked(4, 4, 1),
            colors,
        })
    }

    #[test]
    fn hdr_turned_on_during_the_photo_sends_the_monitor_to_gdi_as_hdr_on() {
        // The path was chosen with HDR off, so its 8-bit photo of a now brightened desktop says exact.
        let (image, hdr) = settled(desktop(Colors::Exact), false, Ok(true));
        assert!(image.is_err());
        assert!(hdr);
    }

    #[test]
    fn hdr_turned_off_during_the_photo_sends_the_monitor_to_gdi_as_hdr_on() {
        // HDR was on for part of the photo, and GDI's photo can't say which part.
        let (image, hdr) = settled(desktop(Colors::Exact), true, Ok(false));
        assert!(image.is_err());
        assert!(hdr);
    }

    #[test]
    fn hdr_as_the_path_was_chosen_keeps_the_photo() {
        for chosen in [false, true] {
            let (image, hdr) = settled(desktop(Colors::Exact), chosen, Ok(chosen));
            assert_eq!(
                image.map(|desktop| desktop.colors).ok(),
                Some(Colors::Exact)
            );
            assert_eq!(hdr, chosen);
        }
    }

    #[test]
    fn hdr_unread_after_the_photo_sends_the_monitor_to_gdi_as_the_path_was_chosen() {
        let unread = || Err(Error::new(E_FAIL, "GetDesc1 failed"));
        let (image, hdr) = settled(desktop(Colors::Exact), true, unread());
        assert_eq!(
            image.map(|_| ()).err().map(|e| e.message()),
            Some(String::from("GetDesc1 failed"))
        );
        assert!(hdr);
        // A photo that had already failed keeps its own cause.
        let lost = Err(Error::new(DXGI_ERROR_ACCESS_LOST, "lost"));
        let (image, hdr) = settled(lost, false, unread());
        assert_eq!(
            image.map(|_| ()).err().map(|e| e.code()),
            Some(DXGI_ERROR_ACCESS_LOST)
        );
        assert!(!hdr);
    }

    #[test]
    fn a_failed_photo_while_hdr_was_turned_on_marks_gdis_photo_approximate() {
        let (image, hdr) = settled(
            Err(Error::new(DXGI_ERROR_ACCESS_LOST, "lost")),
            false,
            Ok(true),
        );
        let grabs = vec![Grab {
            device: DISPLAY1.to_owned(),
            hdr,
            image,
        }];
        let monitors = [monitor(DISPLAY1, RIGHT, 168)];
        let shots = one_per_monitor(&monitors, Ok(grabs), gdi_image);
        let Shot::Gdi { photo, .. } = &shots[0] else {
            panic!("{:?}", shots[0]);
        };
        assert_eq!(photo.colors, Colors::Approximate);
    }
}
