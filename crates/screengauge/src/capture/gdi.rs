//! The capture's second path, for a monitor DXGI couldn't photograph: GDI copies the monitor's part of the screen with
//! BitBlt (v1 spec WPC-296, Frozen screen). Like DXGI's, its photo leaves the mouse pointer out, and a rotated monitor
//! comes upright. Every GDI object made here is released before [`image`] returns.

use screengauge_core::session::Image;
use windows::Win32::Foundation::{E_FAIL, RECT};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CreateCompatibleDC, CreateDIBSection,
    DIB_RGB_COLORS, DeleteDC, DeleteObject, GdiFlush, GetDC, HBITMAP, HDC, HGDIOBJ, ReleaseDC,
    SRCCOPY, SelectObject,
};
use windows::core::Error;

use super::tight_rows;
use crate::log;
use crate::win32::struct_size;

/// The photo of `rect`, a monitor's place in physical pixels of the virtual screen.
pub fn image(rect: RECT) -> windows::core::Result<Image> {
    let (width, height) = (rect.right - rect.left, rect.bottom - rect.top);
    let empty = || Error::new(E_FAIL, format!("the monitor's rectangle {rect:?} is empty"));
    let (Ok(columns), Ok(rows)) = (usize::try_from(width), usize::try_from(height)) else {
        return Err(empty());
    };
    if columns == 0 || rows == 0 {
        return Err(empty());
    }
    let screen = ScreenDc::get()?;
    let memory = MemoryDc::compatible_with(&screen)?;
    let bitmap = Bitmap::top_down(&screen, width, height)?;
    let selected = memory.select(&bitmap)?;
    // Plain SRCCOPY: with the desktop composed, the screen already holds layered windows such as menus and tooltips.
    // WPC-281 compared it with CAPTUREBLT, and only what changed on screen between the two photos differed.
    // SAFETY: both device contexts are alive, and the bitmap selected into `memory` is `width` by `height`.
    unsafe {
        BitBlt(
            memory.0,
            0,
            0,
            width,
            height,
            Some(screen.0),
            rect.left,
            rect.top,
            SRCCOPY,
        )
    }?;
    // SAFETY: no arguments; makes GDI finish drawing into the bitmap before its bits are read.
    if !unsafe { GdiFlush() }.as_bool() {
        return Err(Error::new(E_FAIL, "GdiFlush returned FALSE"));
    }
    let row_length = columns * 4;
    // SAFETY: the bitmap is alive and still selected, and its bits are `rows` rows of `columns` 4-byte pixels with no
    // gap, since a 32-bit row is always a whole number of 4-byte units; the slice is dropped before the bitmap.
    let pixels = unsafe { std::slice::from_raw_parts(bitmap.bits.cast::<u8>(), row_length * rows) };
    let bgra = tight_rows(pixels, row_length, columns, rows)
        .ok_or_else(|| Error::new(E_FAIL, "the bitmap holds fewer rows than the image has"))?;
    drop(selected);
    let (width, height) = (width.cast_unsigned(), height.cast_unsigned());
    Image::new(width, height, bgra)
        .ok_or_else(|| Error::new(E_FAIL, "the rows read don't fill the image"))
}

/// The screen's device context, released when dropped.
struct ScreenDc(HDC);

impl ScreenDc {
    fn get() -> windows::core::Result<ScreenDc> {
        // SAFETY: no window, so this is the device context of the whole virtual screen.
        let dc = unsafe { GetDC(None) };
        if dc.is_invalid() {
            return Err(Error::new(E_FAIL, "GetDC returned no device context"));
        }
        Ok(ScreenDc(dc))
    }
}

impl Drop for ScreenDc {
    fn drop(&mut self) {
        // SAFETY: the device context came from GetDC(None) and is released once, here.
        if unsafe { ReleaseDC(None, self.0) } != 1 {
            log::failure("capture: GDI", &"ReleaseDC didn't release the screen");
        }
    }
}

/// A memory device context like the screen's, deleted when dropped.
struct MemoryDc(HDC);

impl MemoryDc {
    fn compatible_with(screen: &ScreenDc) -> windows::core::Result<MemoryDc> {
        // SAFETY: a live device context.
        let dc = unsafe { CreateCompatibleDC(Some(screen.0)) };
        if dc.is_invalid() {
            return Err(Error::new(
                E_FAIL,
                "CreateCompatibleDC returned no device context",
            ));
        }
        Ok(MemoryDc(dc))
    }

    /// Selects `bitmap` into this device context until the returned value drops.
    fn select<'a>(&'a self, bitmap: &'a Bitmap) -> windows::core::Result<Selected<'a>> {
        // SAFETY: both are alive; the bitmap is selected into no other device context.
        let previous = unsafe { SelectObject(self.0, bitmap.handle.into()) };
        if previous.is_invalid() {
            return Err(Error::new(E_FAIL, "SelectObject returned no object"));
        }
        Ok(Selected {
            memory: self,
            previous,
        })
    }
}

impl Drop for MemoryDc {
    fn drop(&mut self) {
        // SAFETY: the device context came from CreateCompatibleDC and is deleted once, here.
        if !unsafe { DeleteDC(self.0) }.as_bool() {
            log::failure("capture: GDI", &"DeleteDC returned FALSE");
        }
    }
}

/// A bitmap selected into a memory device context, put back to what it held before when dropped, so the bitmap can be
/// deleted.
struct Selected<'a> {
    memory: &'a MemoryDc,
    previous: HGDIOBJ,
}

impl Drop for Selected<'_> {
    fn drop(&mut self) {
        // SAFETY: puts back the object SelectObject returned for this live device context.
        if unsafe { SelectObject(self.memory.0, self.previous) }.is_invalid() {
            log::failure("capture: GDI", &"SelectObject didn't put the bitmap back");
        }
    }
}

/// A 32-bit bitmap whose rows run from the top, with its bits, deleted when dropped.
struct Bitmap {
    handle: HBITMAP,
    bits: *mut core::ffi::c_void,
}

impl Bitmap {
    fn top_down(screen: &ScreenDc, width: i32, height: i32) -> windows::core::Result<Bitmap> {
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: const { struct_size::<BITMAPINFOHEADER>() },
                biWidth: width,
                // A negative height puts the first row at the top, as in an Image.
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        // SAFETY: a valid description of a 32-bit bitmap; `bits` is a local that receives the bitmap's memory.
        let handle =
            unsafe { CreateDIBSection(Some(screen.0), &info, DIB_RGB_COLORS, &mut bits, None, 0) }?;
        let bitmap = Bitmap { handle, bits };
        if bitmap.bits.is_null() {
            return Err(Error::new(E_FAIL, "CreateDIBSection returned no bits"));
        }
        Ok(bitmap)
    }
}

impl Drop for Bitmap {
    fn drop(&mut self) {
        // SAFETY: the bitmap came from CreateDIBSection, is no longer selected, and is deleted once, here.
        if !unsafe { DeleteObject(self.handle.into()) }.as_bool() {
            log::failure(
                "capture: GDI",
                &"DeleteObject returned FALSE for the bitmap",
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_rectangle_is_refused() {
        let rect = RECT {
            left: 10,
            top: 10,
            right: 10,
            bottom: 20,
        };
        let error = image(rect).expect_err("nothing to photograph");
        assert!(error.message().contains("empty"), "{error}");
    }
}
