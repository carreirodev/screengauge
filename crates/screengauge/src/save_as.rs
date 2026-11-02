//! Windows' Save As window for the Snip's file (v1 spec WPC-296, Snip): the image is already taken, and the window only
//! chooses where that one file goes. It opens over the frozen screen, on the cursor's monitor, in the folder the last
//! file was saved to or in Pictures, suggests the automatic name, takes PNG files alone, and Windows itself asks before
//! a file is replaced.
//!
//! Its owner is a window of its own, the host: invisible, over the frozen screen, with a button on the taskbar and in
//! Alt+Tab. When another program comes to the front, the frozen screen and Save As step aside behind it and wait, and
//! that button brings them back (the owner's choice in WPC-318). The frozen screen's windows have no such button, as
//! tool windows, and Save As itself, an owned window, gets none either.
//!
//! [`choose`] runs Windows' own message loop until Save As closes, so the windows' messages go on meanwhile: it must be
//! called with nothing borrowed that their procedures borrow.

use std::cell::RefCell;
use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;

use screengauge_core::session::Point;
use screengauge_core::settings::Folder;
use windows::Win32::Foundation::{
    COLORREF, ERROR_CANCELLED, HINSTANCE, HWND, LPARAM, LRESULT, POINT, WPARAM,
};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint,
};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, CoTaskMemFree};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Ole::IOleWindow;
use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
use windows::Win32::UI::Shell::{
    FOLDERID_Pictures, FOS_FORCEFILESYSTEM, FOS_NOREADONLYRETURN, FOS_OVERWRITEPROMPT,
    FOS_PATHMUSTEXIST, FOS_STRICTFILETYPES, FileSaveDialog, IFileSaveDialog, IShellItem,
    KF_FLAG_DEFAULT, SHCreateItemFromParsingName, SHGetKnownFolderItem, SIGDN_FILESYSPATH,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, HWND_BOTTOM, HWND_TOPMOST, LWA_ALPHA,
    LoadIconW, PostMessageW, RegisterClassExW, SW_SHOWNA, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    SetForegroundWindow, SetLayeredWindowAttributes, SetWindowPos, ShowWindow, WM_ACTIVATEAPP,
    WM_APP, WNDCLASSEXW, WS_EX_APPWINDOW, WS_EX_LAYERED, WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::{Error, HRESULT, HSTRING, PCWSTR, w};

use crate::win32::struct_size;
use crate::{com, log};

/// What the host posts to the resident window when the user comes back to Save As after it stepped aside: by its
/// button on the taskbar, in Alt+Tab, or a click on it. Posted, so the session hears of it outside the window procedure
/// that saw it.
pub const BACK_IN_FRONT: u32 = WM_APP + 6;

const HOST_CLASS: PCWSTR = w!("ScreenGauge.SaveAs");

/// The app icon's group in the exe's resources (build.rs), which the host's button on the taskbar shows.
const APP_ICON_ID: usize = 1;

thread_local! {
    /// Save As while it is open. Never borrowed while Save As runs its message loop: only for a moment, here.
    static OPEN: RefCell<Option<Open>> = const { RefCell::new(None) };
}

struct Open {
    host: HWND,
    dialog: IFileSaveDialog,
    /// Receives [`BACK_IN_FRONT`].
    resident: HWND,
}

/// Registered once at startup.
pub fn register_class(instance: HINSTANCE) -> windows::core::Result<()> {
    // SAFETY: MAKEINTRESOURCE of the icon group, in this exe's own module. A shared icon: never destroyed.
    let icon = unsafe {
        LoadIconW(
            Some(instance),
            PCWSTR(std::ptr::without_provenance(APP_ICON_ID)),
        )
    }?;
    let class = WNDCLASSEXW {
        cbSize: const { struct_size::<WNDCLASSEXW>() },
        lpfnWndProc: Some(host_proc),
        hInstance: instance,
        hIcon: icon,
        lpszClassName: HOST_CLASS,
        ..Default::default()
    };
    // SAFETY: `class` is fully initialized, and its name is a static string.
    if unsafe { RegisterClassExW(&class) } == 0 {
        return Err(Error::from_thread());
    }
    Ok(())
}

/// Opens Save As over the frozen screen, on the monitor at `cursor`, in `folder` or, with none or one that no longer
/// exists, in Pictures, suggesting `file_name`, and waits until it closes: `Some` path the user chose, or `None` when
/// they cancelled. `resident` receives [`BACK_IN_FRONT`].
pub fn choose(
    resident: HWND,
    cursor: Point,
    folder: Option<&Folder>,
    file_name: &str,
) -> windows::core::Result<Option<PathBuf>> {
    com::ensure_started()?;
    // SAFETY: a new in-process COM object, on this single-threaded apartment's thread.
    let dialog: IFileSaveDialog =
        unsafe { CoCreateInstance(&FileSaveDialog, None, CLSCTX_INPROC_SERVER) }?;
    let png = [COMDLG_FILTERSPEC {
        pszName: w!("PNG image"),
        pszSpec: w!("*.png"),
    }];
    // SAFETY: a live dialog; the strings outlive the calls, which copy them.
    unsafe {
        let options = dialog.GetOptions()?;
        dialog.SetOptions(
            options
                | FOS_OVERWRITEPROMPT
                | FOS_STRICTFILETYPES
                | FOS_FORCEFILESYSTEM
                | FOS_PATHMUSTEXIST
                | FOS_NOREADONLYRETURN,
        )?;
        dialog.SetFileTypes(&png)?;
        dialog.SetFileTypeIndex(1)?;
        dialog.SetDefaultExtension(w!("png"))?;
        dialog.SetFileName(&HSTRING::from(file_name))?;
        dialog.SetFolder(&starting_folder(folder)?)?;
    }
    let host = Host::new(resident, cursor, &dialog)?;
    // SAFETY: a live dialog, owned by a live window of this thread. It runs its own message loop until it closes.
    let shown = unsafe { dialog.Show(Some(host.window)) };
    drop(host);
    match shown {
        Ok(()) => {
            // SAFETY: Save As closed with a choice, so it has a result.
            let item = unsafe { dialog.GetResult() }?;
            path_of(&item).map(Some)
        }
        Err(e) if e.code() == HRESULT::from_win32(ERROR_CANCELLED.0) => Ok(None),
        Err(e) => Err(e),
    }
}

/// The folder Save As opens in: `folder`, or Pictures when there is none or it no longer exists.
fn starting_folder(folder: Option<&Folder>) -> windows::core::Result<IShellItem> {
    if let Some(folder) = folder {
        // SAFETY: a path string that outlives the call; no bind context.
        match unsafe {
            SHCreateItemFromParsingName(&HSTRING::from(folder.as_path().as_os_str()), None)
        } {
            Ok(item) => return Ok(item),
            // Deleted or out of reach since: Pictures, as the first time.
            Err(e) => log::failure("Save As: opening in the last folder", &e),
        }
    }
    // SAFETY: a known folder's ID, with no token: the current user's Pictures.
    unsafe { SHGetKnownFolderItem(&FOLDERID_Pictures, KF_FLAG_DEFAULT, None) }
}

/// The file system path of `item`.
fn path_of(item: &IShellItem) -> windows::core::Result<PathBuf> {
    // SAFETY: a live item; FOS_FORCEFILESYSTEM made it one with a path.
    let path = unsafe { item.GetDisplayName(SIGDN_FILESYSPATH) }?;
    // SAFETY: on success the string is valid and null-terminated until it is freed below.
    let chosen = PathBuf::from(OsString::from_wide(unsafe { path.as_wide() }));
    // SAFETY: GetDisplayName allocated the string with the COM allocator; it is freed once, here.
    unsafe { CoTaskMemFree(Some(path.0.cast_const().cast())) };
    Ok(chosen)
}

/// The window Save As belongs to while it is open: invisible, over the whole monitor at the cursor so Save As opens in
/// its middle, topmost so Save As shows over the frozen screen, and with the app icon's button on the taskbar and in
/// Alt+Tab. Destroyed when dropped, and Save As is forgotten with it.
struct Host {
    window: HWND,
}

impl Host {
    fn new(resident: HWND, cursor: Point, dialog: &IFileSaveDialog) -> windows::core::Result<Host> {
        let monitor = monitor_at(cursor);
        // SAFETY: no module name, so this is the handle of ScreenGauge.exe itself.
        let instance: HINSTANCE = unsafe { GetModuleHandleW(None) }?.into();
        // SAFETY: the class was registered at startup in this module.
        let window = unsafe {
            CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_APPWINDOW | WS_EX_LAYERED,
                HOST_CLASS,
                w!("ScreenGauge"),
                WS_POPUP,
                monitor.left,
                monitor.top,
                monitor.right - monitor.left,
                monitor.bottom - monitor.top,
                None,
                None,
                Some(instance),
                None,
            )
        }?;
        let host = Host { window };
        // Fully transparent: the frozen screen shows through it.
        // SAFETY: our own layered window.
        unsafe { SetLayeredWindowAttributes(window, COLORREF(0), 0, LWA_ALPHA) }?;
        // SAFETY: our own window. ShowWindow returns whether it was visible before, not an error.
        let _ = unsafe { ShowWindow(window, SW_SHOWNA) };
        OPEN.set(Some(Open {
            host: window,
            dialog: dialog.clone(),
            resident,
        }));
        Ok(host)
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        OPEN.set(None);
        // SAFETY: our own window, on its thread.
        if let Err(e) = unsafe { DestroyWindow(self.window) } {
            log::failure("Save As: destroying its window", &e);
        }
    }
}

/// The rectangle of the monitor at `cursor`, or the nearest one, in physical pixels of the virtual screen.
fn monitor_at(cursor: Point) -> windows::Win32::Foundation::RECT {
    let point = POINT {
        x: cursor.x,
        y: cursor.y,
    };
    // SAFETY: no pointers; the nearest monitor is always one.
    let monitor = unsafe { MonitorFromPoint(point, MONITOR_DEFAULTTONEAREST) };
    let mut info = MONITORINFO {
        cbSize: const { struct_size::<MONITORINFO>() },
        ..Default::default()
    };
    // SAFETY: fills a local whose size is set.
    if unsafe { GetMonitorInfoW(monitor, &mut info) }.as_bool() {
        info.rcMonitor
    } else {
        log::failure("Save As: finding the monitor", &Error::from_thread());
        // Save As then opens around the cursor.
        windows::Win32::Foundation::RECT {
            left: cursor.x,
            top: cursor.y,
            right: cursor.x + 1,
            bottom: cursor.y + 1,
        }
    }
}

/// Sends Save As behind the program in front, with its host, which keeps its button on the taskbar. Nothing while
/// Save As is closed.
pub fn step_aside() {
    with_open(|open| {
        // At the bottom, and so no longer topmost, nor is Save As, which it owns.
        // SAFETY: our own window, on its thread.
        unsafe {
            SetWindowPos(
                open.host,
                Some(HWND_BOTTOM),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            )
        }
    });
}

/// Brings Save As back over every program and over the frozen screen, which comes back first, and gives it the keyboard.
/// Nothing while Save As is closed.
pub fn come_back() {
    with_open(|open| {
        // SAFETY: our own window, on its thread. Save As, which it owns, becomes topmost with it.
        unsafe {
            SetWindowPos(
                open.host,
                Some(HWND_TOPMOST),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            )
        }?;
        // SAFETY: a live dialog, shown, so it has a window.
        let dialog =
            unsafe { windows::core::Interface::cast::<IOleWindow>(&open.dialog)?.GetWindow() }?;
        // Coming back by Save As's own button, it is in front already; by the global shortcut, Windows lets
        // ScreenGauge come to the front.
        // SAFETY: a window of this thread.
        if !unsafe { SetForegroundWindow(dialog) }.as_bool() {
            log::failure(
                "Save As: coming back",
                &"Windows didn't bring it to the front",
            );
        }
        Ok(())
    });
}

/// Runs `act` on Save As while it is open, logging its failure: the window then stays where it is.
fn with_open(act: impl FnOnce(&Open) -> windows::core::Result<()>) {
    OPEN.with_borrow(|open| {
        if let Some(open) = open
            && let Err(e) = act(open)
        {
            log::failure("Save As: stepping aside or coming back", &e);
        }
    });
}

/// The host's procedure. Coming back to ScreenGauge while Save As is open reaches it as the app's activation, whichever
/// way the user came: Save As takes the front for its disabled owner.
extern "system" fn host_proc(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if message == WM_ACTIVATEAPP && wparam.0 != 0 {
        let resident = OPEN.with_borrow(|open| open.as_ref().map(|open| open.resident));
        if let Some(resident) = resident {
            // SAFETY: our own window; the message carries nothing.
            let posted =
                unsafe { PostMessageW(Some(resident), BACK_IN_FRONT, WPARAM(0), LPARAM(0)) };
            if let Err(e) = posted {
                log::failure("Save As: telling of the way back", &e);
            }
        }
    }
    // SAFETY: the arguments are the ones Windows passed to this window procedure.
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}
