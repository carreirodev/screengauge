//! The Snip's image as the bytes of a PNG file, written by WIC, which comes with Windows (v1 spec WPC-296, Snip: "PNG
//! opaco, sem opção de outro formato"). The PNG holds 24-bit color and no alpha channel, so it is opaque whatever the
//! fourth byte of the frozen screen's pixels says, and each pixel keeps the exact red, green and blue the frozen screen
//! has.
//!
//! COM must be started on the calling thread ([`crate::com::Apartment`]).

use screengauge_core::session::Image;
use windows::Win32::Foundation::E_FAIL;
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_ContainerFormatPng, GUID_WICPixelFormat24bppBGR,
    IWICImagingFactory, WICBitmapEncoderNoCache,
};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, CoCreateInstance, IStream, STATFLAG_NONAME, STATSTG, STREAM_SEEK_SET,
};
use windows::Win32::UI::Shell::SHCreateMemStream;
use windows::core::Error;

/// `image` as the bytes of a PNG file: 24-bit color, opaque, exactly its pixels.
pub fn encode(image: &Image) -> windows::core::Result<Vec<u8>> {
    let (width, height) = (image.width(), image.height());
    let stride = width
        .checked_mul(3)
        .ok_or_else(|| Error::new(E_FAIL, "the image is too wide for a PNG"))?;
    let bgr = without_fourth_byte(image);
    let wic = factory()?;
    let stream = memory_stream(None)?;
    // SAFETY: a new encoder writing to a live stream.
    let encoder = unsafe { wic.CreateEncoder(&GUID_ContainerFormatPng, std::ptr::null()) }?;
    // SAFETY: as above.
    unsafe { encoder.Initialize(&stream, WICBitmapEncoderNoCache) }?;
    let mut frame = None;
    // SAFETY: both outputs are valid; no encoder options are wanted.
    unsafe { encoder.CreateNewFrame(&mut frame, std::ptr::null_mut()) }?;
    let frame = frame.ok_or_else(|| Error::new(E_FAIL, "the PNG encoder made no frame"))?;
    let mut format = GUID_WICPixelFormat24bppBGR;
    // SAFETY: a new frame; the pixels hold `height` rows of `stride` bytes and outlive the calls, which copy them.
    unsafe {
        frame.Initialize(None)?;
        frame.SetSize(width, height)?;
        // The encoder writes back the format it will take: anything but 24-bit BGR would mean converting, or an alpha
        // channel, so it is refused rather than written.
        frame.SetPixelFormat(&mut format)?;
        if format != GUID_WICPixelFormat24bppBGR {
            return Err(Error::new(E_FAIL, "the PNG encoder won't take 24-bit BGR"));
        }
        frame.WritePixels(height, stride, &bgr)?;
        frame.Commit()?;
        encoder.Commit()?;
    }
    read_all(&stream)
}

/// The image's pixels as WIC's 24-bit BGR, three bytes each with no gap between rows: blue, green and red as the frozen
/// screen has them, and the fourth byte, which says nothing, left out.
fn without_fourth_byte(image: &Image) -> Vec<u8> {
    image
        .bgra()
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|&[blue, green, red, _]| [blue, green, red])
        .collect()
}

/// WIC's factory. COM is started on this thread.
fn factory() -> windows::core::Result<IWICImagingFactory> {
    // SAFETY: a registered in-process COM class, created with no outer object, on a thread where COM is started.
    unsafe { CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER) }
}

/// A stream in memory, holding a copy of `contents`, if any.
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
    let size = usize::try_from(length).map_err(|_| Error::new(E_FAIL, "the PNG is over 4 GB"))?;
    let mut bytes = vec![0; size];
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
    use windows::Win32::Graphics::Imaging::{
        GUID_WICPixelFormat32bppBGRA, WICBitmapDitherTypeNone, WICBitmapPaletteTypeCustom,
        WICDecodeMetadataCacheOnDemand,
    };

    use super::*;
    use crate::com::Apartment;

    /// What a PNG decodes to, read back by WIC as a program pasting it would: its size, the pixel format the file holds,
    /// and every pixel as blue, green, red and alpha.
    struct Decoded {
        width: u32,
        height: u32,
        stored_as_24_bit_color: bool,
        bgra: Vec<[u8; 4]>,
    }

    fn decode(bytes: &[u8]) -> Decoded {
        let wic = factory().expect("the WIC factory");
        let stream = memory_stream(Some(bytes)).expect("a stream");
        // SAFETY: a live stream holding a copy of `bytes`.
        let decoder = unsafe {
            wic.CreateDecoderFromStream(&stream, std::ptr::null(), WICDecodeMetadataCacheOnDemand)
        }
        .expect("a PNG WIC reads");
        // SAFETY: frame 0 of a live decoder.
        let frame = unsafe { decoder.GetFrame(0) }.expect("a frame");
        // SAFETY: a live frame.
        let stored = unsafe { frame.GetPixelFormat() }.expect("its format");
        // SAFETY: a new converter from a live factory.
        let converter = unsafe { wic.CreateFormatConverter() }.expect("a converter");
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
        }
        .expect("a conversion to BGRA");
        let (mut width, mut height) = (0, 0);
        // SAFETY: both outputs are valid.
        unsafe { converter.GetSize(&mut width, &mut height) }.expect("its size");
        let count = usize::try_from(width * height * 4).expect("a small image");
        let mut bgra = vec![0; count];
        // SAFETY: the buffer holds exactly `height` rows of `width` × 4 bytes.
        unsafe { converter.CopyPixels(std::ptr::null(), width * 4, &mut bgra) }
            .expect("the pixels");
        Decoded {
            width,
            height,
            stored_as_24_bit_color: stored == GUID_WICPixelFormat24bppBGR,
            bgra: bgra.as_chunks::<4>().0.to_vec(),
        }
    }

    /// An image of `width` × `height` whose pixels are each a different color, given as blue, green, red and a fourth
    /// byte of 0, as a capture may leave it.
    fn varied(width: u32, height: u32) -> Image {
        let bgra = (0..width * height)
            .flat_map(|i| {
                let [a, b, c, _] = i.wrapping_mul(2_654_435_761).to_le_bytes();
                [a, b, c, 0]
            })
            .collect();
        Image::new(width, height, bgra).expect("the pixels fill the image")
    }

    #[test]
    fn the_png_reads_back_pixel_for_pixel_and_opaque_though_the_fourth_byte_is_zero() {
        let _com = Apartment::enter().expect("COM starts on a test thread");
        // 37 is odd, so a row of 24-bit pixels isn't a whole number of 4-byte words.
        let image = varied(37, 11);
        let bytes = encode(&image).expect("a PNG");
        assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
        let decoded = decode(&bytes);
        assert_eq!((decoded.width, decoded.height), (37, 11));
        assert!(
            decoded.stored_as_24_bit_color,
            "no alpha channel in the file"
        );
        let wanted: Vec<[u8; 4]> = image
            .bgra()
            .as_chunks::<4>()
            .0
            .iter()
            .map(|&[blue, green, red, _]| [blue, green, red, 255])
            .collect();
        assert_eq!(decoded.bgra, wanted);
    }

    #[test]
    fn a_one_pixel_image_and_a_wide_one_survive_too() {
        let _com = Apartment::enter().expect("COM starts on a test thread");
        for (width, height) in [(1, 1), (3840, 2)] {
            let image = varied(width, height);
            let decoded = decode(&encode(&image).expect("a PNG"));
            assert_eq!((decoded.width, decoded.height), (width, height));
            let rgb: Vec<[u8; 3]> = decoded.bgra.iter().map(|&[b, g, r, _]| [b, g, r]).collect();
            let wanted: Vec<[u8; 3]> = image
                .bgra()
                .as_chunks::<4>()
                .0
                .iter()
                .map(|&[b, g, r, _]| [b, g, r])
                .collect();
            assert_eq!(rgb, wanted, "{width} × {height}");
        }
    }

    #[test]
    fn the_png_says_its_colors_are_srgb_and_carries_no_profile_that_would_change_them() {
        let _com = Apartment::enter().expect("COM starts on a test thread");
        let bytes = encode(&varied(4, 4)).expect("a PNG");
        // Each chunk after the signature: a 4-byte length, a 4-byte type, the data and a 4-byte CRC.
        let mut chunks = Vec::new();
        let mut at = 8;
        while at + 8 <= bytes.len() {
            let length = usize::try_from(u32::from_be_bytes(
                bytes[at..at + 4].try_into().expect("four bytes"),
            ))
            .expect("a small chunk");
            let kind = String::from_utf8_lossy(&bytes[at + 4..at + 8]).into_owned();
            chunks.push((kind, bytes[at + 8..at + 8 + length].to_vec()));
            at += 12 + length;
        }
        let kinds: Vec<&str> = chunks.iter().map(|(kind, _)| kind.as_str()).collect();
        assert_eq!(kinds.first(), Some(&"IHDR"));
        assert_eq!(kinds.last(), Some(&"IEND"));
        // WIC marks the pixels as sRGB, which the screen's are: a program pasting them shows them as they are. The
        // gAMA beside it is the one the PNG standard pairs with sRGB, 1/2.2, which a reader of sRGB ignores. A color
        // profile or other primaries would have a program convert the colors.
        assert!(kinds.contains(&"sRGB"), "{kinds:?}");
        for (kind, data) in &chunks {
            match kind.as_str() {
                "gAMA" => assert_eq!(data.as_slice(), 45_455_u32.to_be_bytes()),
                "iCCP" | "cHRM" => panic!("{kinds:?}"),
                _ => {}
            }
        }
    }
}
