//! Windows' Save As window for the Snip's file (v1 spec WPC-296, Snip): the image is already taken, and the window only
//! chooses where that one file goes. It opens over the frozen screen, on the cursor's monitor, in the folder the last
//! file was saved to or in Pictures, suggests the automatic name, takes PNG files alone, and Windows itself asks before
//! a file is replaced.
//!
//! Its owner is a window of its own, the host: invisible, over the frozen screen, with a button on the taskbar and in
//! Alt+Tab. When another program comes to the front, the frozen screen and Save As step aside behind it and wait, and
//! that button brings them back (the owner's choice in WPC-318); the frozen screen's watch of the window in front
//! (`foreground`) tells of the way back. The frozen screen's windows have no such button, as tool windows, and Save As
//! itself, an owned window, gets none either.
//!
//! [`choose`] runs Windows' own message loop until Save As closes, so the windows' messages go on meanwhile: it must be
//! called with nothing borrowed that their procedures borrow.

use std::cell::RefCell;
use std::ffi::{OsStr, OsString};
use std::os::windows::ffi::OsStringExt;
use std::path::{Path, PathBuf};

use screengauge_core::session::{Point, png_path};
use screengauge_core::settings::Folder;
use windows::Win32::Foundation::{
    COLORREF, ERROR_CANCELLED, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM,
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
    FOS_PATHMUSTEXIST, FileSaveDialog, IFileSaveDialog, IShellItem, KF_FLAG_DEFAULT,
    SHCreateItemFromParsingName, SHGetKnownFolderItem, SIGDN_FILESYSPATH,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetWindowRect, HWND_BOTTOM, HWND_TOPMOST,
    IsWindowVisible, KillTimer, LWA_ALPHA, LoadIconW, RegisterClassExW, SW_SHOWNA, SWP_NOACTIVATE,
    SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SetForegroundWindow, SetLayeredWindowAttributes,
    SetTimer, SetWindowPos, ShowWindow, USER_TIMER_MINIMUM, WM_TIMER, WNDCLASSEXW, WS_EX_APPWINDOW,
    WS_EX_LAYERED, WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::{Error, HRESULT, HSTRING, Interface, PCWSTR, w};

use crate::win32::struct_size;
use crate::{com, log};

const HOST_CLASS: PCWSTR = w!("ScreenGauge.SaveAs");

/// The host's timer that moves Save As to the middle of the monitor once it shows.
const CENTER_TIMER: usize = 1;

/// The app icon's group in the exe's resources (build.rs), which the host's button on the taskbar shows.
const APP_ICON_ID: usize = 1;

thread_local! {
    /// Save As while it is open. Never borrowed while Save As runs its message loop: only for a moment, here.
    static OPEN: RefCell<Option<Open>> = const { RefCell::new(None) };
}

struct Open {
    host: HWND,
    dialog: IFileSaveDialog,
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
/// they cancelled. `front_after`, the frozen screen's window under the cursor, takes the front back as Save As closes.
///
/// A name typed with another extension, `button.jpg`, is written as `button.png` (v1 spec WPC-296, Snip), so Windows'
/// question about replacing a file was about the wrong one. When `button.png` is there already, Save As opens again
/// with that name, in that folder, and Windows asks about it then: no file is ever replaced without its question.
pub fn choose(
    cursor: Point,
    front_after: Option<HWND>,
    folder: Option<&Folder>,
    file_name: &str,
) -> windows::core::Result<Option<PathBuf>> {
    let mut folder = folder.map(|folder| folder.as_path().to_path_buf());
    let mut file_name = OsString::from(file_name);
    loop {
        let Some(chosen) = choose_once(cursor, front_after, folder.as_deref(), &file_name)? else {
            return Ok(None);
        };
        let png = png_path(&chosen);
        if png == chosen {
            return Ok(Some(chosen));
        }
        match png.try_exists() {
            Ok(false) => return Ok(Some(chosen)),
            Ok(true) => {}
            // Whether it is there can't be told: Save As asks, as if it were, rather than replace it unasked.
            Err(e) => log::failure("Save As: looking for the .png file", &e),
        }
        let (Some(parent), Some(name)) = (png.parent(), png.file_name()) else {
            // Can't happen: Save As gives a file's full path.
            return Ok(Some(chosen));
        };
        folder = Some(parent.to_path_buf());
        file_name = name.to_os_string();
    }
}

/// Save As, once, in `folder`, suggesting `file_name`.
fn choose_once(
    cursor: Point,
    front_after: Option<HWND>,
    folder: Option<&Path>,
    file_name: &OsStr,
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
                // Not FOS_STRICTFILETYPES, with which Windows makes `button.jpg` into `button.jpg.png`.
                | FOS_OVERWRITEPROMPT
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
    let host = Host::new(cursor, front_after, &dialog)?;
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
fn starting_folder(folder: Option<&Path>) -> windows::core::Result<IShellItem> {
    if let Some(folder) = folder {
        // SAFETY: a path string that outlives the call; no bind context.
        match unsafe { SHCreateItemFromParsingName(&HSTRING::from(folder.as_os_str()), None) } {
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
    /// The window that takes the front back before this one goes. Otherwise Windows hands the front to the next
    /// program in line, which counts as another program in front and closes the tools.
    front_after: Option<HWND>,
}

impl Host {
    fn new(
        cursor: Point,
        front_after: Option<HWND>,
        dialog: &IFileSaveDialog,
    ) -> windows::core::Result<Host> {
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
        let host = Host {
            window,
            front_after,
        };
        // Fully transparent: the frozen screen shows through it.
        // SAFETY: our own layered window.
        unsafe { SetLayeredWindowAttributes(window, COLORREF(0), 0, LWA_ALPHA) }?;
        // SAFETY: our own window. ShowWindow returns whether it was visible before, not an error.
        let _ = unsafe { ShowWindow(window, SW_SHOWNA) };
        OPEN.set(Some(Open {
            host: window,
            dialog: dialog.clone(),
        }));
        // Windows puts Save As at its owner's top left corner, which here is the monitor's: it is moved to the middle as
        // soon as it shows. Should the timer be refused, it stays in the corner.
        // SAFETY: our own window, on its thread; the timer goes with it.
        if unsafe { SetTimer(Some(window), CENTER_TIMER, USER_TIMER_MINIMUM, None) } == 0 {
            log::failure("Save As: centering it", &Error::from_thread());
        }
        Ok(host)
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        OPEN.set(None);
        // Save As has closed and given the front back to this window, so ScreenGauge may pass it on.
        if let Some(front) = self.front_after
            // SAFETY: a window of this thread.
            && !unsafe { SetForegroundWindow(front) }.as_bool()
        {
            log::failure(
                "Save As: giving the front back to the frozen screen",
                &"Windows refused",
            );
        }
        // SAFETY: our own window, on its thread.
        if let Err(e) = unsafe { DestroyWindow(self.window) } {
            log::failure("Save As: destroying its window", &e);
        }
    }
}

/// The rectangle of the monitor at `cursor`, or the nearest one, in physical pixels of the virtual screen.
fn monitor_at(cursor: Point) -> RECT {
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
        RECT {
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
    with_open("Save As: stepping aside", |open| {
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
    with_open("Save As: coming back", |open| {
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
        let dialog = window_of(&open.dialog)?;
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

/// Closes Save As as Cancel would, as ScreenGauge exits while it is open. Nothing while Save As is closed.
pub fn cancel() {
    with_open("Save As: cancelling it at exit", |open| {
        // SAFETY: a live dialog, shown; it closes once this message is handled.
        unsafe { open.dialog.Close(HRESULT::from_win32(ERROR_CANCELLED.0)) }
    });
}

/// Runs `act` on Save As while it is open, logging its failure under `context`: the window then stays as it is.
fn with_open(context: &str, act: impl FnOnce(&Open) -> windows::core::Result<()>) {
    OPEN.with_borrow(|open| {
        if let Some(open) = open
            && let Err(e) = act(open)
        {
            log::failure(context, &e);
        }
    });
}

/// Save As's own window, which exists only while it shows.
fn window_of(dialog: &IFileSaveDialog) -> windows::core::Result<HWND> {
    // SAFETY: a live dialog; before it shows, the call fails and says so.
    unsafe { dialog.cast::<IOleWindow>()?.GetWindow() }
}

/// Moves Save As to the middle of `host`, the monitor it opened on, once it shows, and stops `host`'s timer then. Until
/// it shows, the timer ticks again.
fn center(host: HWND) {
    // Before Save As shows, it has no window yet: the timer ticks again.
    let dialog = OPEN.with_borrow(|open| window_of(&open.as_ref()?.dialog).ok());
    // SAFETY: a window handle, which IsWindowVisible checks.
    let Some(dialog) = dialog.filter(|&dialog| unsafe { IsWindowVisible(dialog) }.as_bool()) else {
        return;
    };
    // SAFETY: our own window, on its thread.
    if let Err(e) = unsafe { KillTimer(Some(host), CENTER_TIMER) } {
        log::failure("Save As: centering it", &e);
    }
    let (mut monitor, mut window) = (RECT::default(), RECT::default());
    // SAFETY: fill locals, for windows of this thread.
    let moved = unsafe { GetWindowRect(host, &mut monitor) }
        .and_then(|()| unsafe { GetWindowRect(dialog, &mut window) })
        .and_then(|()| {
            let left =
                monitor.left + (monitor.right - monitor.left - (window.right - window.left)) / 2;
            let top =
                monitor.top + (monitor.bottom - monitor.top - (window.bottom - window.top)) / 2;
            // SAFETY: a window of this thread, moved and nothing else.
            unsafe {
                SetWindowPos(
                    dialog,
                    None,
                    left.max(monitor.left),
                    top.max(monitor.top),
                    0,
                    0,
                    SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                )
            }
        });
    if let Err(e) = moved {
        log::failure("Save As: centering it", &e);
    }
}

/// The host's procedure: its timer centers Save As, and Windows does the rest.
extern "system" fn host_proc(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if message == WM_TIMER && wparam.0 == CENTER_TIMER {
        center(hwnd);
        return LRESULT(0);
    }
    // SAFETY: the arguments are the ones Windows passed to this window procedure.
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}
