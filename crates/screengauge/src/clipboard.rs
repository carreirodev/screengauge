//! The clipboard: what the session core copies goes there for any program to paste, a measurement or a color as Unicode
//! text and the Snip's image as a PNG and a bitmap. What was copied stays after the tools close: it is handed to
//! Windows, not rendered later by a window of ScreenGauge's.

use std::thread;
use std::time::Duration;

use screengauge_core::session::Image;
use windows::Win32::Foundation::{E_FAIL, GlobalFree, HANDLE, HGLOBAL, HWND};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
};
use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
use windows::Win32::System::Ole::{CF_DIB, CF_UNICODETEXT};
use windows::core::{Error, w};

use crate::png;

/// How many more times to try opening the clipboard, and how long to wait before each: another program may hold it
/// open for a moment, as clipboard managers and Windows' own clipboard history do right after each copy.
const RETRIES: u32 = 5;
const RETRY_WAIT: Duration = Duration::from_millis(10);

/// Puts `text` on the clipboard in place of what was there, with `owner` as the clipboard's owner.
pub fn copy_text(owner: HWND, text: &str) -> windows::core::Result<()> {
    let bytes: Vec<u8> = wide_with_nul(text)
        .iter()
        .flat_map(|unit| unit.to_le_bytes())
        .collect();
    replace_with(owner, &[(u32::from(CF_UNICODETEXT.0), &bytes)])
}

/// Puts `image` on the clipboard in place of what was there, never a file's path, with `owner` as the clipboard's
/// owner (v1 spec WPC-296, Snip). It goes twice, so every program finds a form it pastes: as a PNG, under the "PNG"
/// format that browsers, chats and Office read first, and as a bitmap (CF_DIB), which every other program reads and
/// from which Windows makes the older bitmap formats. Both are opaque and hold the image's exact colors.
pub fn copy_image(owner: HWND, image: &Image) -> windows::core::Result<()> {
    let png = png::encode(image)?;
    let bitmap = dib(image)?;
    // SAFETY: a static name. The format is registered once per session of Windows, and its number is the same for
    // every program that registers "PNG".
    let png_format = unsafe { RegisterClipboardFormatW(w!("PNG")) };
    if png_format == 0 {
        return Err(Error::from_thread());
    }
    replace_with(owner, &[(png_format, &png), (u32::from(CF_DIB.0), &bitmap)])
}

/// Empties the clipboard and hands Windows each of `contents`, a format and its bytes, all at once, so a program that
/// pastes finds every form of the same copy.
fn replace_with(owner: HWND, contents: &[(u32, &[u8])]) -> windows::core::Result<()> {
    open(owner)?;
    let put = put_all(contents);
    // SAFETY: the clipboard was opened just above, by this thread.
    let closed = unsafe { CloseClipboard() };
    match (put, closed) {
        (Err(put), Err(closed)) => {
            // The copy's failure is the one returned; the clipboard left open matters too, as every program needs it.
            crate::log::failure("clipboard: closing it after a failed copy", &closed);
            Err(put)
        }
        (put, closed) => put.and(closed),
    }
}

/// Opens the clipboard for this thread, waiting a little while another program holds it.
fn open(owner: HWND) -> windows::core::Result<()> {
    let mut tries = 0;
    loop {
        // SAFETY: `owner` is a window of this thread, which outlives the moment the clipboard is open.
        match unsafe { OpenClipboard(Some(owner)) } {
            Ok(()) => return Ok(()),
            Err(_) if tries < RETRIES => {
                tries += 1;
                thread::sleep(RETRY_WAIT);
            }
            Err(e) => return Err(e),
        }
    }
}

/// Empties the open clipboard and hands Windows each of `contents`.
fn put_all(contents: &[(u32, &[u8])]) -> windows::core::Result<()> {
    // SAFETY: the clipboard is open, by this thread.
    unsafe { EmptyClipboard() }?;
    for &(format, bytes) in contents {
        put(format, bytes)?;
    }
    Ok(())
}

/// Hands Windows `bytes` as the open clipboard's `format`.
fn put(format: u32, bytes: &[u8]) -> windows::core::Result<()> {
    // SAFETY: a plain allocation; Windows takes it below, or it is freed here.
    let memory = unsafe { GlobalAlloc(GMEM_MOVEABLE, bytes.len()) }?;
    if let Err(e) = fill(memory, bytes) {
        free(memory);
        return Err(e);
    }
    // SAFETY: `memory` holds the bytes and is unlocked. Once SetClipboardData succeeds, Windows owns it.
    if let Err(e) = unsafe { SetClipboardData(format, Some(HANDLE(memory.0))) } {
        free(memory);
        return Err(e);
    }
    Ok(())
}

/// Copies `bytes` into `memory`, which is exactly their size, and unlocks it again.
fn fill(memory: HGLOBAL, bytes: &[u8]) -> windows::core::Result<()> {
    // SAFETY: `memory` is a movable allocation this thread just made.
    let pointer = unsafe { GlobalLock(memory) }.cast::<u8>();
    if pointer.is_null() {
        return Err(Error::from_thread());
    }
    // SAFETY: the allocation is `bytes.len()` bytes, locked, and apart from `bytes`.
    unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), pointer, bytes.len()) };
    // SAFETY: unlocks the lock taken above. GlobalUnlock returns FALSE once the memory is no longer locked, which is
    // what is meant here: only a failure with an error code is one.
    match unsafe { GlobalUnlock(memory) } {
        Err(e) if e.code().is_err() => Err(e),
        _ => Ok(()),
    }
}

/// Frees memory Windows didn't take.
fn free(memory: HGLOBAL) {
    // SAFETY: `memory` came from GlobalAlloc and nothing else holds it; GlobalFree frees it even should `fill` have
    // failed to unlock it. GlobalFree returns the handle on failure; there is nothing more to do with it then than log it.
    if let Err(e) = unsafe { GlobalFree(Some(memory)) } {
        crate::log::failure("clipboard: freeing what Windows didn't take", &e);
    }
}

/// `text` in UTF-16, the clipboard's Unicode text, ending with the NUL it requires.
fn wide_with_nul(text: &str) -> Vec<u16> {
    text.encode_utf16().chain([0]).collect()
}

/// The size of a BITMAPINFOHEADER, the bitmap's header.
const HEADER: u32 = 40;

/// `image` as a CF_DIB: a BITMAPINFOHEADER and then the pixels, 32 bits each, blue, green, red and an alpha of 255, so
/// no program takes a pixel for see-through whatever the fourth byte said, in rows from the bottom up, as a bitmap whose
/// height is above zero lays them out. 32-bit rows are always a whole number of 4-byte words, so they need no padding.
fn dib(image: &Image) -> windows::core::Result<Vec<u8>> {
    let too_large = || Error::new(E_FAIL, "the image is too large for a bitmap");
    let (width, height) = (image.width(), image.height());
    let size_image = width
        .checked_mul(height)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(too_large)?;
    let signed = |length: u32| i32::try_from(length).map_err(|_| too_large());
    let mut bytes =
        Vec::with_capacity(usize::try_from(HEADER + size_image).map_err(|_| too_large())?);
    bytes.extend(HEADER.to_le_bytes()); // biSize
    bytes.extend(signed(width)?.to_le_bytes()); // biWidth
    bytes.extend(signed(height)?.to_le_bytes()); // biHeight: above zero, so the bottom row comes first
    bytes.extend(1_u16.to_le_bytes()); // biPlanes
    bytes.extend(32_u16.to_le_bytes()); // biBitCount
    bytes.extend(0_u32.to_le_bytes()); // biCompression: BI_RGB
    bytes.extend(size_image.to_le_bytes()); // biSizeImage
    bytes.extend(0_i32.to_le_bytes()); // biXPelsPerMeter: none said
    bytes.extend(0_i32.to_le_bytes()); // biYPelsPerMeter
    bytes.extend(0_u32.to_le_bytes()); // biClrUsed: no color table
    bytes.extend(0_u32.to_le_bytes()); // biClrImportant
    let row = usize::try_from(width).map_err(|_| too_large())?;
    let pixels = image.bgra().as_chunks::<4>().0;
    // An image of no pixels has no rows, and `chunks` would refuse a row of none.
    if row > 0 {
        for line in pixels.chunks(row).rev() {
            bytes.extend(
                line.iter()
                    .flat_map(|&[blue, green, red, _]| [blue, green, red, 255]),
            );
        }
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    // The clipboard itself isn't tested here: it is the user's, shared by every program, and a test run would replace
    // what the owner had copied. The hardware check copies and reads it back, saving and restoring it.

    #[test]
    fn the_text_is_utf16_ending_with_one_nul() {
        assert_eq!(
            wide_with_nul("24 x 180"),
            [50, 52, 32, 120, 32, 49, 56, 48, 0]
        );
        assert_eq!(wide_with_nul(""), [0]);
    }

    fn word(bytes: &[u8], at: usize) -> u32 {
        u32::from_le_bytes(bytes[at..at + 4].try_into().expect("four bytes"))
    }

    fn half(bytes: &[u8], at: usize) -> u16 {
        u16::from_le_bytes(bytes[at..at + 2].try_into().expect("two bytes"))
    }

    #[test]
    fn the_bitmap_has_a_header_of_40_bytes_for_32_bit_bottom_up_pixels() {
        // 3 × 2, a fourth byte of 0 in every pixel, as a capture may leave it.
        let image = Image::new(3, 2, vec![0; 24]).expect("the pixels fill the image");
        let bytes = dib(&image).expect("a bitmap");
        assert_eq!(bytes.len(), 40 + 3 * 2 * 4);
        assert_eq!(word(&bytes, 0), 40, "biSize");
        assert_eq!(word(&bytes, 4), 3, "biWidth");
        assert_eq!(word(&bytes, 8), 2, "biHeight above zero: bottom-up");
        assert_eq!(half(&bytes, 12), 1, "biPlanes");
        assert_eq!(half(&bytes, 14), 32, "biBitCount");
        assert_eq!(word(&bytes, 16), 0, "biCompression BI_RGB");
        assert_eq!(word(&bytes, 20), 24, "biSizeImage");
        assert_eq!(&bytes[24..40], &[0; 16]);
    }

    #[test]
    fn the_bitmaps_rows_go_from_the_bottom_up_with_every_pixel_opaque() {
        // Top row: red, green. Bottom row: blue, white. Blue, green, red and a fourth byte of 0, as the photo has them.
        let image = Image::new(
            2,
            2,
            vec![
                0, 0, 255, 0, 0, 255, 0, 0, //
                255, 0, 0, 0, 255, 255, 255, 0,
            ],
        )
        .expect("the pixels fill the image");
        let bytes = dib(&image).expect("a bitmap");
        assert_eq!(
            &bytes[40..],
            &[
                255, 0, 0, 255, 255, 255, 255, 255, // the bottom row first: blue, white
                0, 0, 255, 255, 0, 255, 0, 255, // then the top row: red, green
            ]
        );
    }
}
