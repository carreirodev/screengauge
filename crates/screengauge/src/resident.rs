//! The resident process: one window that is never shown, so ScreenGauge has no taskbar button while idle.
//! It receives what Windows tells ScreenGauge: the global shortcut and the tray icon's clicks. Nothing is
//! prepared at startup and there is no timer: the thread sleeps in GetMessageW until Windows sends something.
//! While the tools are open, it carries out the effects the session core returns.

use std::cell::RefCell;
use std::sync::atomic::{AtomicU32, Ordering};

use screengauge_core::session::{Effect, Input, Opening, Session};
use screengauge_core::settings::{Remembered, Stored};
use windows::Win32::Foundation::{E_FAIL, HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW, MSG,
    PostQuitMessage, RegisterClassExW, RegisterWindowMessageW, TranslateMessage, WINDOW_EX_STYLE,
    WM_CONTEXTMENU, WM_DESTROY, WM_HOTKEY, WNDCLASSEXW, WS_OVERLAPPED,
};
use windows::core::{Error, PCWSTR, w};

use crate::frozen_screen::{self, FrozenScreen};
use crate::settings_file::SettingsFile;
use crate::win32::{low_word, struct_size};
use crate::{capture, event, global_shortcut, log, tray};

const CLASS_NAME: PCWSTR = w!("ScreenGauge.Resident");

/// Explorer broadcasts this message when the taskbar is created: at sign-in, and when Explorer restarts
/// and has dropped every tray icon.
static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);

thread_local! {
    /// The frozen screen, with its session, while the tools are open. Only the resident window's procedure touches it,
    /// and never while it holds the borrow: opening and closing take it out first.
    static FROZEN_SCREEN: RefCell<Option<FrozenScreen>> = const { RefCell::new(None) };
    /// What the settings file holds, and the file when it could be opened. Set once, as the process starts.
    static SETTINGS: RefCell<Option<Settings>> = const { RefCell::new(None) };
}

struct Settings {
    /// `None` when the file couldn't be opened: ScreenGauge then runs with the defaults and saves nothing.
    file: Option<SettingsFile>,
    stored: Stored,
}

/// Runs until the user exits from the tray icon, with what the settings file holds and the file to save it to.
pub fn run(file: Option<SettingsFile>, stored: Stored) -> windows::core::Result<()> {
    let shortcut = stored.options.global_shortcut;
    SETTINGS.set(Some(Settings { file, stored }));
    // SAFETY: registers a constant message name.
    let taskbar_created = unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) };
    if taskbar_created == 0 {
        return Err(Error::from_thread());
    }
    TASKBAR_CREATED.store(taskbar_created, Ordering::Relaxed);

    // SAFETY: no module name, so this is the handle of ScreenGauge.exe itself.
    let instance: HINSTANCE = unsafe { GetModuleHandleW(None) }?.into();
    let class = WNDCLASSEXW {
        cbSize: const { struct_size::<WNDCLASSEXW>() },
        lpfnWndProc: Some(window_proc),
        hInstance: instance,
        lpszClassName: CLASS_NAME,
        ..Default::default()
    };
    // SAFETY: `class` is fully initialized, and its name is a static string.
    if unsafe { RegisterClassExW(&class) } == 0 {
        return Err(Error::from_thread());
    }
    frozen_screen::register_class(instance)?;
    // SAFETY: the class was just registered in this module. The window is never shown.
    let hwnd = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            CLASS_NAME,
            w!("ScreenGauge"),
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(instance),
            None,
        )
    }?;
    // At sign-in the taskbar may not exist yet; the icon then comes with TaskbarCreated, so a failure
    // here leaves ScreenGauge running.
    if let Err(e) = tray::add(hwnd) {
        log::failure("tray icon: adding it at startup", &e);
    }
    // Another program may hold the combination; ScreenGauge keeps running, and telling the user comes with WPC-324.
    if let Err(e) = global_shortcut::register(hwnd, shortcut) {
        log::failure(&format!("global shortcut: registering {shortcut}"), &e);
    }

    let mut message = MSG::default();
    loop {
        // SAFETY: `message` is a valid MSG for the call to fill.
        match unsafe { GetMessageW(&mut message, None, 0, 0) }.0 {
            -1 => return Err(Error::from_thread()),
            0 => return Ok(()),
            _ => {
                // SAFETY: a message GetMessageW just returned. TranslateMessage's result only says whether
                // a character message was posted, so there is nothing to check.
                unsafe {
                    let _ = TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
            }
        }
    }
}

extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        tray::CALLBACK_MESSAGE => {
            // Provisional exit, until the tray icon's menu exists (WPC-325).
            if low_word(lparam) == WM_CONTEXTMENU {
                // SAFETY: our own window, on the thread that created it.
                if let Err(e) = unsafe { DestroyWindow(hwnd) } {
                    log::failure("exit: DestroyWindow", &e);
                }
            }
            LRESULT(0)
        }
        WM_HOTKEY if global_shortcut::is_global_shortcut(wparam) => {
            open_or_tell(hwnd);
            LRESULT(0)
        }
        frozen_screen::EFFECTS_READY => {
            let effects = FROZEN_SCREEN.with_borrow(|open| {
                open.as_ref()
                    .map(FrozenScreen::take_effects)
                    .unwrap_or_default()
            });
            carry_out(effects);
            LRESULT(0)
        }
        WM_DESTROY => {
            close();
            frozen_screen::release_factory();
            if let Err(e) = global_shortcut::unregister(hwnd) {
                log::failure("exit: unregistering the global shortcut", &e);
            }
            if let Err(e) = tray::remove(hwnd) {
                log::failure("exit: removing the tray icon", &e);
            }
            // SAFETY: ends this thread's message loop.
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        _ if message == TASKBAR_CREATED.load(Ordering::Relaxed) => {
            if let Err(e) = tray::add(hwnd) {
                log::failure("tray icon: adding it when the taskbar was created", &e);
            }
            LRESULT(0)
        }
        // SAFETY: the arguments are the ones Windows passed to this window procedure.
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}

/// The global shortcut opens the tools, or, when they are open, goes to the session core, which closes them.
fn open_or_tell(resident: HWND) {
    let told = FROZEN_SCREEN.with_borrow(|open| {
        open.as_ref()
            .map(|frozen| frozen.handle(event::of_message(Input::GlobalShortcut)))
    });
    match told {
        Some(effects) => carry_out(effects),
        None => match open(resident) {
            Ok(frozen) => FROZEN_SCREEN.set(Some(frozen)),
            // Until Windows notifications exist (WPC-303), a failed call reaches only the debugger output.
            Err(e) => log::failure("frozen screen: opening it", &e),
        },
    }
}

/// Opens the tools: the photo comes first, before any window of ScreenGauge appears or takes the keyboard, so whatever
/// was on screen at the press (a hover, an open menu, a tooltip) is in it.
fn open(resident: HWND) -> windows::core::Result<FrozenScreen> {
    let frozen_screen = capture::every_monitor()?;
    let Stored {
        options,
        remembered,
    } = SETTINGS.with_borrow(|settings| {
        settings
            .as_ref()
            .map(|settings| settings.stored.clone())
            .unwrap_or_default()
    });
    let session = Session::open(Opening {
        frozen_screen,
        options,
        remembered,
    })
    .map_err(|refused| Error::new(E_FAIL, refused.to_string()))?;
    FrozenScreen::show(session, resident)
}

/// Does what the session core asked, in its order.
fn carry_out(effects: Vec<Effect>) {
    for effect in effects {
        let not_built = match effect {
            Effect::Close => {
                close();
                continue;
            }
            Effect::SaveRemembered(remembered) => {
                save_remembered(remembered);
                continue;
            }
            // No rule asks for these yet: the tools that do bring them (Dimensions' copy with WPC-308, Snip with
            // WPC-317 and WPC-318, the gear with WPC-319). The log never shows their content, which may hold a path.
            Effect::CopyText(_) => "copying text",
            Effect::CopyImage(_) => "copying an image",
            Effect::WritePng { .. } => "writing a PNG file",
            Effect::OpenSaveAs { .. } => "opening Save As",
            Effect::OpenSettings => "opening Settings",
        };
        log::failure("session", &format!("{not_built} isn't built yet"));
    }
}

/// Dropping the frozen screen closes its windows, drops the session and erases the photos.
fn close() {
    drop(FROZEN_SCREEN.with_borrow_mut(Option::take));
}

/// Keeps `remembered` for the next call, and in the settings file when there is one.
fn save_remembered(remembered: Remembered) {
    SETTINGS.with_borrow_mut(|settings| {
        let Some(settings) = settings else {
            return;
        };
        settings.stored.remembered = remembered;
        if let Some(file) = &settings.file
            && let Err(e) = file.save(&settings.stored)
        {
            log::failure("settings: saving the remembered state", &e);
        }
    });
}
