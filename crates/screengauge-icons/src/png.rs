//! PNG files in memory, through WIC.

use windows::Win32::Foundation::E_FAIL;
use windows::Win32::Graphics::Imaging::{
    GUID_ContainerFormatPng, GUID_WICPixelFormat32bppBGRA, WICBitmapDitherTypeNone,
    WICBitmapEncoderNoCache, WICBitmapPaletteTypeCustom, WICDecodeMetadataCacheOnDemand,
};
use windows::Win32::System::Com::{IStream, STATFLAG_NONAME, STATSTG, STREAM_SEEK_SET};
use windows::Win32::UI::Shell::SHCreateMemStream;
use windows::core::Error;

use crate::image::{Image, widen};
use crate::raster::wic;

/// `image` as the bytes of a PNG file, with its alpha straight as PNG keeps it.
pub fn encode(image: &Image) -> windows::core::Result<Vec<u8>> {
    let wic = wic()?;
    let stream = memory_stream(None)?;
    // SAFETY: a new encoder writing to a live stream.
    let encoder = unsafe { wic.CreateEncoder(&GUID_ContainerFormatPng, std::ptr::null()) }?;
    // SAFETY: as above.
    unsafe { encoder.Initialize(&stream, WICBitmapEncoderNoCache) }?;
    let mut frame = None;
    // SAFETY: both outputs are valid; no encoder options are wanted.
    unsafe { encoder.CreateNewFrame(&mut frame, std::ptr::null_mut()) }?;
    let frame = frame.ok_or_else(|| Error::new(E_FAIL, "the PNG encoder made no frame"))?;
    let mut format = GUID_WICPixelFormat32bppBGRA;
    let bgra: Vec<u8> = image
        .rgba()
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|&[r, g, b, a]| [b, g, r, a])
        .collect();
    // SAFETY: a new frame; the pixels hold `height` rows of `width` × 4 bytes and outlive the calls, which copy them.
    unsafe {
        frame.Initialize(None)?;
        frame.SetSize(image.width(), image.height())?;
        frame.SetPixelFormat(&mut format)?;
        if format != GUID_WICPixelFormat32bppBGRA {
            return Err(Error::new(E_FAIL, "the PNG encoder won't take 32-bit BGRA"));
        }
        frame.WritePixels(image.height(), image.width() * 4, &bgra)?;
        frame.Commit()?;
        encoder.Commit()?;
    }
    read_all(&stream)
}

/// The picture in a PNG file, as straight RGBA.
pub fn decode(bytes: &[u8]) -> windows::core::Result<Image> {
    let wic = wic()?;
    let stream = memory_stream(Some(bytes))?;
    // SAFETY: a live stream holding a copy of `bytes`.
    let decoder = unsafe {
        wic.CreateDecoderFromStream(&stream, std::ptr::null(), WICDecodeMetadataCacheOnDemand)
    }?;
    // SAFETY: frame 0 of a live decoder.
    let frame = unsafe { decoder.GetFrame(0) }?;
    // SAFETY: a new converter from a live factory.
    let converter = unsafe { wic.CreateFormatConverter() }?;
    // SAFETY: converts the live frame to straight BGRA, with no palette.
    unsafe {
        converter.Initialize(
            &frame,
            &GUID_WICPixelFormat32bppBGRA,
            WICBitmapDitherTypeNone,
            None,
            0.0,
            WICBitmapPaletteTypeCustom,
        )
    }?;
    let (mut width, mut height) = (0, 0);
    // SAFETY: both outputs are valid.
    unsafe { converter.GetSize(&mut width, &mut height) }?;
    let mut bgra = vec![0; widen(width) * widen(height) * 4];
    // SAFETY: the buffer holds exactly `height` rows of `width` × 4 bytes.
    unsafe { converter.CopyPixels(std::ptr::null(), width * 4, &mut bgra) }?;
    let rgba = bgra
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|&[b, g, r, a]| [r, g, b, a])
        .collect();
    Image::from_rgba(width, height, rgba)
        .ok_or_else(|| Error::new(E_FAIL, "the PNG decoded to the wrong number of bytes"))
}

fn memory_stream(contents: Option<&[u8]>) -> windows::core::Result<IStream> {
    // SAFETY: the stream copies `contents`, if any.
    unsafe { SHCreateMemStream(contents) }
        .ok_or_else(|| Error::new(E_FAIL, "SHCreateMemStream returned no stream"))
}

/// Everything written to `stream`.
fn read_all(stream: &IStream) -> windows::core::Result<Vec<u8>> {
    let mut status = STATSTG::default();
    // SAFETY: a live stream; with STATFLAG_NONAME no name is allocated.
    unsafe { stream.Stat(&mut status, STATFLAG_NONAME) }?;
    let length =
        u32::try_from(status.cbSize).map_err(|_| Error::new(E_FAIL, "the PNG is over 4 GB"))?;
    let mut bytes = vec![0; widen(length)];
    let mut read = 0;
    // SAFETY: rewinds a live stream, then reads into a buffer of exactly `length` bytes.
    unsafe {
        stream.Seek(0, STREAM_SEEK_SET, None)?;
        stream
            .Read(bytes.as_mut_ptr().cast(), length, Some(&mut read))
            .ok()?;
    }
    if read != length {
        return Err(Error::new(E_FAIL, "the PNG stream returned too few bytes"));
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::Rgba;

    #[test]
    fn a_picture_survives_a_round_trip() {
        let mut image = Image::new(3, 2);
        image.set_pixel(0, 0, Rgba::hex(0xF97316));
        image.set_pixel(2, 1, Rgba::hex(0x7C2D12).with_opacity(0.5));
        let bytes = encode(&image).expect("encode");
        assert_eq!(&bytes[1..4], b"PNG");
        assert_eq!(decode(&bytes).expect("decode"), image);
    }
}
