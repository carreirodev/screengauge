//! The clipboard: what the session core copies goes there as Unicode text, for any program to paste. What was copied
//! stays after the tools close: the text is handed to Windows, not rendered later by a window of ScreenGauge's.

use std::thread;
use std::time::Duration;

use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL, HWND};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
};
use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
use windows::Win32::System::Ole::CF_UNICODETEXT;
use windows::core::Error;

/// How many more times to try opening the clipboard, and how long to wait before each: another program may hold it
/// open for a moment, as clipboard managers and Windows' own clipboard history do right after each copy.
const RETRIES: u32 = 5;
const RETRY_WAIT: Duration = Duration::from_millis(10);

/// Puts `text` on the clipboard in place of what was there, with `owner` as the clipboard's owner.
pub fn copy_text(owner: HWND, text: &str) -> windows::core::Result<()> {
    open(owner)?;
    let put = put_text(&wide_with_nul(text));
    // SAFETY: the clipboard was opened just above, by this thread.
    let closed = unsafe { CloseClipboard() };
    put.and(closed)
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

/// Empties the open clipboard and hands Windows `wide`, which ends with its NUL, as its Unicode text.
fn put_text(wide: &[u16]) -> windows::core::Result<()> {
    // SAFETY: the clipboard is open, by this thread.
    unsafe { EmptyClipboard() }?;
    // SAFETY: a plain allocation; Windows takes it below, or it is freed here.
    let memory = unsafe { GlobalAlloc(GMEM_MOVEABLE, size_of_val(wide)) }?;
    if let Err(e) = fill(memory, wide) {
        free(memory);
        return Err(e);
    }
    // SAFETY: `memory` holds the text and is unlocked. Once SetClipboardData succeeds, Windows owns it.
    if let Err(e) = unsafe { SetClipboardData(u32::from(CF_UNICODETEXT.0), Some(HANDLE(memory.0))) }
    {
        free(memory);
        return Err(e);
    }
    Ok(())
}

/// Copies `wide` into `memory`, which is exactly its size, and unlocks it again.
fn fill(memory: HGLOBAL, wide: &[u16]) -> windows::core::Result<()> {
    // SAFETY: `memory` is a movable allocation this thread just made.
    let pointer = unsafe { GlobalLock(memory) }.cast::<u16>();
    if pointer.is_null() {
        return Err(Error::from_thread());
    }
    // SAFETY: the allocation is `size_of_val(wide)` bytes, locked, suitably aligned for u16, and apart from `wide`.
    unsafe { std::ptr::copy_nonoverlapping(wide.as_ptr(), pointer, wide.len()) };
    // SAFETY: unlocks the lock taken above. GlobalUnlock returns FALSE once the memory is no longer locked, which is
    // what is meant here: only a failure with an error code is one.
    match unsafe { GlobalUnlock(memory) } {
        Err(e) if e.code().is_err() => Err(e),
        _ => Ok(()),
    }
}

/// Frees memory Windows didn't take.
fn free(memory: HGLOBAL) {
    // SAFETY: `memory` came from GlobalAlloc, is unlocked, and nothing else holds it. GlobalFree returns the handle on
    // failure; there is nothing more to do with it then than log it.
    if let Err(e) = unsafe { GlobalFree(Some(memory)) } {
        crate::log::failure("clipboard: freeing the text Windows didn't take", &e);
    }
}

/// `text` in UTF-16, the clipboard's Unicode text, ending with the NUL it requires.
fn wide_with_nul(text: &str) -> Vec<u16> {
    text.encode_utf16().chain([0]).collect()
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
}
