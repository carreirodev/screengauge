//! Windows' own window for choosing a folder, which Settings opens from "Change…" and from turning "Save to default
//! folder" on with no folder kept (v1 spec WPC-296, Settings; WPC-322). It belongs to the Settings window, so it opens
//! over it and Settings waits. It counts as Settings for the frozen screen (`foreground`): with Save As waiting aside, it
//! brings nothing back over itself, and the tools called over it close as they do for Settings.
//!
//! [`choose`] runs Windows' own message loop until the window closes, so the windows' messages go on meanwhile: it
//! must be called with nothing borrowed that their procedures borrow.

use std::cell::RefCell;
use std::path::PathBuf;

use screengauge_core::settings::Folder;
use windows::Win32::Foundation::{ERROR_CANCELLED, HWND, LPARAM, WPARAM};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
use windows::Win32::System::Ole::IOleWindow;
use windows::Win32::UI::Shell::{
    FOS_FORCEFILESYSTEM, FOS_PATHMUSTEXIST, FOS_PICKFOLDERS, FileOpenDialog, IFileOpenDialog,
    IShellItem, SHCreateItemFromParsingName,
};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_CLOSE};
use windows::core::{HRESULT, HSTRING, Interface};

use crate::{com, log, save_as};

thread_local! {
    /// The window while it is open. Never borrowed while it runs its message loop: only for a moment, here.
    static OPEN: RefCell<Option<IFileOpenDialog>> = const { RefCell::new(None) };
}

/// Opens the folder window over `owner`, starting in `folder` or, with none or one that no longer exists, where Windows
/// last had a folder window, and waits until it closes: `Some` path the user chose, or `None` when they cancelled.
pub fn choose(owner: HWND, folder: Option<&Folder>) -> windows::core::Result<Option<PathBuf>> {
    com::ensure_started()?;
    // SAFETY: a new in-process COM object, on this single-threaded apartment's thread.
    let dialog: IFileOpenDialog =
        unsafe { CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER) }?;
    // SAFETY: a live dialog.
    unsafe {
        let options = dialog.GetOptions()?;
        dialog.SetOptions(options | FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST)?;
    }
    if let Some(start) = folder.and_then(starting_folder) {
        // SAFETY: a live dialog and a live item, which the call keeps its own reference to.
        unsafe { dialog.SetFolder(&start) }?;
    }
    OPEN.set(Some(dialog.clone()));
    // SAFETY: a live dialog, owned by a live window of this thread. It runs its own message loop until it closes.
    let shown = unsafe { dialog.Show(Some(owner)) };
    OPEN.set(None);
    match shown {
        Ok(()) => {
            // SAFETY: the window closed with a choice, so it has a result.
            let item = unsafe { dialog.GetResult() }?;
            save_as::path_of(&item).map(Some)
        }
        Err(e) if e.code() == HRESULT::from_win32(ERROR_CANCELLED.0) => Ok(None),
        Err(e) => Err(e),
    }
}

/// `folder` as the item the window opens in. `None` when it was deleted, or can't be reached, since the user chose it:
/// the window then opens where Windows opens it.
fn starting_folder(folder: &Folder) -> Option<IShellItem> {
    // SAFETY: a path string that outlives the call; no bind context.
    match unsafe { SHCreateItemFromParsingName(&HSTRING::from(folder.as_path().as_os_str()), None) }
    {
        Ok(item) => Some(item),
        Err(e) => {
            log::failure("Folder window: opening in the default folder", &e);
            None
        }
    }
}

/// Whether the folder window is open.
pub fn is_open() -> bool {
    OPEN.with_borrow(Option::is_some)
}

/// Closes the folder window as Cancel would, as ScreenGauge exits while it is open or Save As comes back over it; the
/// window is gone, and `choose` returns, once its message is handled. Nothing while it is closed.
///
/// It closes its window, as the user's X would: IFileDialog::Close, which Save As is closed with at exit, was seen to
/// leave this one open (WPC-322).
pub fn cancel() {
    OPEN.with_borrow(|open| {
        let Some(dialog) = open else {
            return;
        };
        // SAFETY: a live dialog, shown, so it has a window.
        let window = unsafe { dialog.cast::<IOleWindow>().and_then(|ole| ole.GetWindow()) };
        let closed = window.and_then(|window| {
            // SAFETY: a window of this thread; the message carries nothing.
            unsafe { PostMessageW(Some(window), WM_CLOSE, WPARAM(0), LPARAM(0)) }
        });
        if let Err(e) = closed {
            log::failure("Folder window: cancelling it", &e);
        }
    });
}
