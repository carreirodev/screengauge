//! The capture behind the frozen screen: every monitor through DXGI Desktop Duplication, in 8 bits and without the
//! mouse pointer, one right after the other, and through GDI on a monitor where DXGI fails (v1 spec WPC-296, Frozen
//! screen), with the place, scale and HDR state the session core receives with each photo. Everything DXGI, Direct3D
//! and GDI create for it is released before [`every_monitor`] returns, so the capture exists only during the call.

mod gdi;

use std::time::{Duration, Instant};

use screengauge_core::session::{Colors, Image, Photo, Rect, Scale};
use windows::Win32::Foundation::{E_FAIL, HMODULE, LPARAM, RECT};
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_UNKNOWN;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_FLAG, D3D11_MAP_READ, D3D11_MAPPED_SUBRESOURCE,
    D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING, D3D11CreateDevice, ID3D11Device,
    ID3D11DeviceContext, ID3D11Texture2D,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_COLOR_SPACE_RGB_FULL_G2084_NONE_P2020, DXGI_FORMAT_B8G8R8A8_UNORM,
    DXGI_MODE_ROTATION_IDENTITY, DXGI_MODE_ROTATION_UNSPECIFIED, DXGI_SAMPLE_DESC,
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
    /// HDR is on for the output's monitor.
    hdr: bool,
    image: windows::core::Result<Image>,
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
        .map(|output| Grab {
            device: output.device,
            hdr: output.hdr,
            image: output.duplication.and_then(Duplication::image),
        })
        .collect())
}

/// An output attached to the desktop, ready to give its image.
struct Output {
    device: String,
    hdr: bool,
    duplication: windows::core::Result<Duplication>,
}

/// A duplication of every output attached to the desktop, on every adapter (the owner's two monitors are on two
/// graphics cards), or why it couldn't be made.
fn duplicate_every_output() -> windows::core::Result<Vec<Output>> {
    // SAFETY: no arguments; the factory is released when it drops.
    let factory: IDXGIFactory1 = unsafe { CreateDXGIFactory1() }?;
    let mut duplications = Vec::new();
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
            let duplication = match gpu.get_or_insert_with(|| Gpu::on(&adapter)) {
                Ok(gpu) if is_upright(description.Rotation) => Duplication::of(gpu, output),
                Ok(_) => Err(Error::new(
                    E_FAIL,
                    "the monitor is rotated, and DXGI gives its image unrotated",
                )),
                Err(e) => Err(e.clone()),
            };
            duplications.push(Output {
                device: device_name(&description.DeviceName),
                // Microsoft's documented test for HDR (research/hdr-screen-colors).
                hdr: description.ColorSpace == DXGI_COLOR_SPACE_RGB_FULL_G2084_NONE_P2020,
                duplication,
            });
        }
    }
    Ok(duplications)
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
    duplication: IDXGIOutputDuplication,
}

impl Duplication {
    /// Asks for B8G8R8A8, the 8-bit format; with HDR on, the 16-bit path comes with WPC-327.
    fn of(gpu: &Gpu, output: IDXGIOutput) -> windows::core::Result<Duplication> {
        // IDXGIOutput5 is there on every Windows 11.
        let output5: IDXGIOutput5 = output.cast()?;
        // SAFETY: the device was created on the adapter this output is connected to.
        let duplication =
            unsafe { output5.DuplicateOutput1(&gpu.device, 0, &[DXGI_FORMAT_B8G8R8A8_UNORM]) }?;
        Ok(Duplication {
            gpu: gpu.clone(),
            output,
            duplication,
        })
    }

    /// The output's current image. The duplication is released before this returns.
    fn image(self) -> windows::core::Result<Image> {
        match first_image(&self.gpu, &self.duplication) {
            // The duplication was lost, as when the display mode or the desktop changes, and a new one gets one more
            // try. Windows allows one duplication per output and process at a time, so the lost one goes first.
            Err(e) if e.code() == DXGI_ERROR_ACCESS_LOST => {
                let Duplication {
                    gpu,
                    output,
                    duplication,
                } = self;
                drop(duplication);
                let again = Duplication::of(&gpu, output)?;
                first_image(&again.gpu, &again.duplication)
            }
            first => first,
        }
    }
}

/// Waits for the duplication's first desktop image and reads it into memory. An error between acquiring a frame and
/// releasing it leaves the frame to the duplication, which the caller drops right away.
fn first_image(gpu: &Gpu, duplication: &IDXGIOutputDuplication) -> windows::core::Result<Image> {
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
                        Some(read(gpu, &resource.cast()?)?)
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

/// Copies the desktop texture to memory the processor can read, through a staging texture.
fn read(gpu: &Gpu, texture: &ID3D11Texture2D) -> windows::core::Result<Image> {
    let mut description = D3D11_TEXTURE2D_DESC::default();
    // SAFETY: fills a local.
    unsafe { texture.GetDesc(&mut description) };
    if description.Format != DXGI_FORMAT_B8G8R8A8_UNORM {
        return Err(Error::new(
            E_FAIL,
            format!("the desktop image came in format {}", description.Format.0),
        ));
    }
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
    let image = mapped_image(&mapped, description.Width, description.Height);
    // SAFETY: unmaps what was mapped above, after `mapped_image` copied it.
    unsafe { gpu.context.Unmap(&staging, 0) };
    image
}

/// The mapped rows as an [`Image`].
fn mapped_image(
    mapped: &D3D11_MAPPED_SUBRESOURCE,
    width: u32,
    height: u32,
) -> windows::core::Result<Image> {
    let too_large = || Error::new(E_FAIL, "the desktop image is too large to address");
    let (columns, rows) = (
        usize::try_from(width).map_err(|_| too_large())?,
        usize::try_from(height).map_err(|_| too_large())?,
    );
    let pitch = usize::try_from(mapped.RowPitch).map_err(|_| too_large())?;
    // Every row but the last is `pitch` bytes long; the last may end right after its pixels.
    let length = match rows.checked_sub(1) {
        Some(before_last) => before_last
            .checked_mul(pitch)
            .and_then(|start| start.checked_add(columns.checked_mul(4)?))
            .ok_or_else(too_large)?,
        None => 0,
    };
    if mapped.pData.is_null() || pitch < columns * 4 {
        return Err(Error::new(E_FAIL, "Map returned no rows of that size"));
    }
    // SAFETY: Map succeeded for a `width` by `height` B8G8R8A8 texture, so `pData` holds `rows` rows `pitch` bytes
    // apart, each with `columns` pixels; the slice ends at the last pixel and is dropped before Unmap.
    let mapped_rows = unsafe { std::slice::from_raw_parts(mapped.pData.cast::<u8>(), length) };
    let bgra = tight_rows(mapped_rows, pitch, columns, rows)
        .ok_or_else(|| Error::new(E_FAIL, "Map returned fewer rows than the image has"))?;
    Image::new(width, height, bgra)
        .ok_or_else(|| Error::new(E_FAIL, "the rows read don't fill the image"))
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
            let dxgi_image = match grab {
                Some(grab) => grab.image.and_then(|image| fits(monitor, image)),
                None => Err(dxgi_failure
                    .clone()
                    .unwrap_or_else(|| Error::new(E_FAIL, "DXGI has no image of this monitor"))),
            };
            match dxgi_image {
                Ok(image) => Shot::Dxgi(photo(monitor, image, dxgi_colors(hdr))),
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

/// With HDR on, DXGI's 8-bit photo has Windows' brightening in it (WPC-289); undoing it comes with WPC-327.
fn dxgi_colors(hdr: bool) -> Colors {
    if hdr {
        Colors::Approximate
    } else {
        Colors::Exact
    }
}

/// GDI's photo is never the exact HDR path (v1 spec WPC-296, Colors and HDR): with HDR on, its colors are approximate,
/// even once DXGI's are exact.
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
            image: Ok(marked(width, height, mark)),
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
    fn a_monitor_with_hdr_on_gets_approximate_colors() {
        let mut hdr_on = grab(DISPLAY2, 1920, 1080, 2);
        hdr_on.hdr = true;
        let grabs = vec![grab(DISPLAY1, 3840, 2160, 1), hdr_on];
        let shots = one_per_monitor(&owners_monitors(), Ok(grabs), no_gdi);
        let colors: Vec<_> = seen(&shots).into_iter().map(|shot| shot.3).collect();
        assert_eq!(colors, [Colors::Exact, Colors::Approximate]);
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
        let monitors = monitors().expect("the monitors");
        let grabs = grab_every_output().expect("DXGI");
        for monitor in &monitors {
            let gdi = gdi::image(monitor.rect).expect("GDI's photo");
            let dxgi = grabs
                .iter()
                .find(|grab| grab.device == monitor.device)
                .and_then(|grab| grab.image.as_ref().ok())
                .expect("DXGI's photo");
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
}
