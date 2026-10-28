//! Builder A. PNG output through WIC and pixel comparison.

use std::path::Path;

use windows::Win32::Foundation::GENERIC_WRITE;
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_ContainerFormatPng, GUID_WICPixelFormat32bppBGRA, IWICBitmapFrameEncode,
    IWICImagingFactory, WICBitmapEncoderNoCache,
};
use windows::Win32::System::Com::StructuredStorage::IPropertyBag2;
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
use windows::core::PCWSTR;

use crate::common::{Ctx, DiffStats, Photo, PxRect};
use crate::monitors::to_wide;

/// Writes the photo as an opaque PNG.
pub fn save_png(photo: &Photo, path: &Path) -> Result<(), String> {
    let stride = photo.width * 4;
    if photo.bgra.len() != (stride * photo.height) as usize {
        return Err(format!(
            "save_png: buffer is {} bytes, expected {}x{}x4",
            photo.bgra.len(),
            photo.width,
            photo.height
        ));
    }
    let where_ = path.display().to_string();
    let name = to_wide(&where_);
    // SAFETY: COM is initialized on this thread (main.rs); every pointer below outlives its call.
    unsafe {
        let factory: IWICImagingFactory =
            CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ctx("WIC factory")?;
        let stream = factory.CreateStream().ctx("WIC CreateStream")?;
        stream.InitializeFromFilename(PCWSTR(name.as_ptr()), GENERIC_WRITE.0).ctx(&format!("WIC open {where_}"))?;
        let encoder = factory.CreateEncoder(&GUID_ContainerFormatPng, std::ptr::null()).ctx("WIC PNG encoder")?;
        encoder.Initialize(&stream, WICBitmapEncoderNoCache).ctx("WIC encoder Initialize")?;
        let mut frame: Option<IWICBitmapFrameEncode> = None;
        let mut props: Option<IPropertyBag2> = None;
        encoder.CreateNewFrame(&mut frame, &mut props).ctx("WIC CreateNewFrame")?;
        let frame = frame.ok_or("WIC CreateNewFrame returned no frame")?;
        frame.Initialize(props.as_ref()).ctx("WIC frame Initialize")?;
        frame.SetSize(photo.width, photo.height).ctx("WIC SetSize")?;
        let mut format = GUID_WICPixelFormat32bppBGRA;
        frame.SetPixelFormat(&mut format).ctx("WIC SetPixelFormat")?;
        if format != GUID_WICPixelFormat32bppBGRA {
            // The encoder would read our bytes with a different layout.
            return Err(format!("WIC PNG encoder refused 32bppBGRA and offered {format:?}"));
        }
        frame.WritePixels(photo.height, stride, &photo.bgra).ctx("WIC WritePixels")?;
        frame.Commit().ctx("WIC frame Commit")?;
        encoder.Commit().ctx("WIC encoder Commit")?;
    }
    Ok(())
}

/// Compares two photos of the same size pixel by pixel (B, G, R; alpha ignored).
/// `exclude` holds rectangles in photo coordinates to skip.
pub fn diff(a: &Photo, b: &Photo, exclude: &[PxRect]) -> Result<DiffStats, String> {
    if a.width != b.width || a.height != b.height {
        return Err(format!("diff: sizes differ, {}x{} vs {}x{}", a.width, a.height, b.width, b.height));
    }
    let expected = (a.width * a.height * 4) as usize;
    if a.bgra.len() != expected || b.bgra.len() != expected {
        return Err(format!("diff: buffer sizes {} and {}, expected {expected}", a.bgra.len(), b.bgra.len()));
    }
    let mut s = DiffStats::default();
    for y in 0..a.height {
        for x in 0..a.width {
            let (xi, yi) = (x as i32, y as i32);
            if exclude.iter().any(|r| xi >= r.left && xi < r.right && yi >= r.top && yi < r.bottom) {
                continue;
            }
            s.total += 1;
            let i = ((y * a.width + x) * 4) as usize;
            let d = (0..3).map(|c| a.bgra[i + c].abs_diff(b.bgra[i + c])).max().unwrap_or(0);
            if d > 0 {
                s.differing += 1;
                s.max_delta = s.max_delta.max(d);
                if s.first.is_none() {
                    s.first = Some((x, y));
                }
            }
        }
    }
    Ok(s)
}
