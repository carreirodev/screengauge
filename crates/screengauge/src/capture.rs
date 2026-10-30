//! The capture behind the frozen screen: every monitor through DXGI Desktop Duplication, in 8 bits and without the
//! mouse pointer, one right after the other (v1 spec WPC-296, Frozen screen), with the place, scale and HDR state the
//! session core receives with each photo. Everything DXGI and Direct3D create for it is released before
//! [`every_monitor`] returns, so the capture exists only during the call.

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

use crate::win32::struct_size;

/// How long a new duplication may take to deliver its first desktop image.
const FIRST_IMAGE_BUDGET: Duration = Duration::from_millis(500);
/// How long each wait for that image lasts, in milliseconds.
const WAIT_MS: u32 = 100;

/// Takes the photo of every monitor, or none: a monitor DXGI can't photograph fails the whole call. Taking it
/// through GDI instead, and telling the user when both fail, comes with WPC-303.
pub fn every_monitor() -> windows::core::Result<Vec<Photo>> {
    let monitors = monitors()?;
    let grabs = grab_every_output()?;
    one_per_monitor(&monitors, grabs)
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
            let (mut dpi, mut unused) = (0, 0);
            // SAFETY: a monitor EnumDisplayMonitors just listed; both out pointers are locals. The process is
            // Per-Monitor V2, so this is the monitor's own DPI.
            unsafe { GetDpiForMonitor(handle, MDT_EFFECTIVE_DPI, &mut dpi, &mut unused) }?;
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

/// A rotated monitor's desktop image comes in the monitor's native orientation; turning it upright isn't built.
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

/// Pairs every monitor with its output's image, in the monitors' order. All or nothing: a monitor without an image of
/// its exact size fails the whole frozen screen.
fn one_per_monitor(
    monitors: &[Monitor],
    mut grabs: Vec<Grab>,
) -> windows::core::Result<Vec<Photo>> {
    let mut photos = Vec::with_capacity(monitors.len());
    for monitor in monitors {
        let failed = |message: String| Error::new(E_FAIL, format!("{}: {message}", monitor.device));
        let grab = grabs
            .iter()
            .position(|grab| grab.device == monitor.device)
            .map(|index| grabs.swap_remove(index))
            .ok_or_else(|| failed(String::from("DXGI has no image of this monitor")))?;
        let image = grab
            .image
            .map_err(|e| Error::new(e.code(), format!("{}: {}", monitor.device, e.message())))?;
        let size = (
            u32::try_from(monitor.rect.right - monitor.rect.left),
            u32::try_from(monitor.rect.bottom - monitor.rect.top),
        );
        if size != (Ok(image.width()), Ok(image.height())) {
            return Err(failed(format!(
                "the image is {}x{}, and the monitor is {:?}",
                image.width(),
                image.height(),
                monitor.rect
            )));
        }
        photos.push(Photo {
            rect: Rect {
                left: monitor.rect.left,
                top: monitor.rect.top,
                width: image.width(),
                height: image.height(),
            },
            scale: monitor.scale,
            // With HDR on, the 8-bit photo has Windows' brightening in it (WPC-289); undoing it comes with WPC-327.
            colors: if grab.hdr {
                Colors::Approximate
            } else {
                Colors::Exact
            },
            image,
        });
    }
    Ok(photos)
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

    fn monitor(device: &str, rect: RECT, dpi: u32) -> Monitor {
        Monitor {
            device: device.to_owned(),
            rect,
            scale: Scale::from_dpi(dpi).expect("a real scale"),
        }
    }

    /// An image whose first byte tells it apart.
    fn grab(device: &str, width: u32, height: u32, mark: u8) -> Grab {
        let mut bgra = vec![0; usize::try_from(width * height * 4).expect("a small image")];
        bgra[0] = mark;
        Grab {
            device: device.to_owned(),
            hdr: false,
            image: Ok(Image::new(width, height, bgra).expect("a whole image")),
        }
    }

    fn rect(rect: RECT) -> Rect {
        Rect {
            left: rect.left,
            top: rect.top,
            width: u32::try_from(rect.right - rect.left).expect("a monitor's width"),
            height: u32::try_from(rect.bottom - rect.top).expect("a monitor's height"),
        }
    }

    #[test]
    fn every_monitor_gets_its_own_image_in_the_monitors_order() {
        let monitors = [
            monitor(r"\\.\DISPLAY1", RIGHT, 168),
            monitor(r"\\.\DISPLAY2", LEFT, 120),
        ];
        let grabs = vec![
            grab(r"\\.\DISPLAY2", 1920, 1080, 2),
            grab(r"\\.\DISPLAY1", 3840, 2160, 1),
        ];
        let photos = one_per_monitor(&monitors, grabs).expect("a photo per monitor");
        let seen: Vec<_> = photos
            .iter()
            .map(|photo| (photo.rect, photo.scale.dpi(), photo.image.bgra()[0]))
            .collect();
        assert_eq!(seen, [(rect(RIGHT), 168, 1), (rect(LEFT), 120, 2)]);
    }

    #[test]
    fn a_monitor_with_hdr_on_gets_approximate_colors() {
        let monitors = [
            monitor(r"\\.\DISPLAY1", RIGHT, 168),
            monitor(r"\\.\DISPLAY2", LEFT, 120),
        ];
        let mut hdr_on = grab(r"\\.\DISPLAY2", 1920, 1080, 2);
        hdr_on.hdr = true;
        let grabs = vec![grab(r"\\.\DISPLAY1", 3840, 2160, 1), hdr_on];
        let photos = one_per_monitor(&monitors, grabs).expect("a photo per monitor");
        let colors: Vec<_> = photos.iter().map(|photo| photo.colors).collect();
        assert_eq!(colors, [Colors::Exact, Colors::Approximate]);
    }

    #[test]
    fn an_output_without_a_monitor_is_left_out() {
        let monitors = [monitor(r"\\.\DISPLAY1", RIGHT, 168)];
        let grabs = vec![
            grab(r"\\.\DISPLAY1", 3840, 2160, 1),
            grab(r"\\.\DISPLAY7", 1920, 1080, 7),
        ];
        let photos = one_per_monitor(&monitors, grabs).expect("a photo per monitor");
        assert_eq!(photos.len(), 1);
        assert_eq!(photos[0].image.bgra()[0], 1);
    }

    #[test]
    fn a_monitor_without_an_image_fails_them_all() {
        let monitors = [
            monitor(r"\\.\DISPLAY1", RIGHT, 168),
            monitor(r"\\.\DISPLAY2", LEFT, 120),
        ];
        let grabs = vec![grab(r"\\.\DISPLAY1", 3840, 2160, 1)];
        let error = one_per_monitor(&monitors, grabs).expect_err("no frozen screen");
        assert!(error.message().contains(r"\\.\DISPLAY2"), "{error}");
    }

    #[test]
    fn an_image_of_another_size_fails_them_all() {
        let monitors = [monitor(r"\\.\DISPLAY1", RIGHT, 168)];
        let grabs = vec![grab(r"\\.\DISPLAY1", 1920, 1080, 1)];
        let error = one_per_monitor(&monitors, grabs).expect_err("no frozen screen");
        assert!(error.message().contains("1920x1080"), "{error}");
    }

    #[test]
    fn a_failed_image_fails_them_all_and_keeps_its_cause() {
        let monitors = [monitor(r"\\.\DISPLAY1", RIGHT, 168)];
        let grabs = vec![Grab {
            device: r"\\.\DISPLAY1".to_owned(),
            hdr: false,
            image: Err(Error::new(DXGI_ERROR_ACCESS_LOST, "lost")),
        }];
        let error = one_per_monitor(&monitors, grabs).expect_err("no frozen screen");
        assert_eq!(error.code(), DXGI_ERROR_ACCESS_LOST);
        assert!(error.message().contains(r"\\.\DISPLAY1"), "{error}");
    }
}
