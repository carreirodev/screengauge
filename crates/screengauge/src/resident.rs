//! The resident process: one window that is never shown, so ScreenGauge has no taskbar button while idle.
//! It receives what Windows tells ScreenGauge: the global shortcut, the tray icon's clicks, the calls of a copy opened
//! again and what becomes of its Windows notifications. At startup only COM, the notifications' activator and their
//! registration with Windows are prepared, and there is no timer at rest: the thread sleeps in GetMessageW until
//! Windows sends something. While the tools are open, it carries out the effects the session core returns.

use std::cell::{Cell, RefCell};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use screengauge_core::session::{Effect, Image, Input, Opening, SaveTo, Session};
use screengauge_core::settings::{Folder, Remembered, StartWithWindows, Stored};
use windows::Win32::Foundation::{
    E_FAIL, ERROR_HOTKEY_ALREADY_REGISTERED, ERROR_SUCCESS, HINSTANCE, HWND, LPARAM, LRESULT,
    POINT, SetLastError, WPARAM,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::GetDoubleClickTime;
use windows::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
    FindWindowW, GetMessageW, GetWindowThreadProcessId, IsWindow, MSG, PostMessageW,
    PostQuitMessage, RegisterClassExW, RegisterWindowMessageW, SetCursorPos, TranslateMessage,
    WINDOW_EX_STYLE, WM_CONTEXTMENU, WM_DESTROY, WM_DISPLAYCHANGE, WM_DPICHANGED, WM_HOTKEY,
    WM_SETTINGCHANGE, WM_TIMER, WNDCLASSEXW, WS_OVERLAPPED,
};
use windows::core::{Error, PCWSTR, w};

use crate::StartedBy;
use crate::frozen_screen::{self, FrozenScreen};
use crate::notification::{self, Notification};
use crate::settings_file::{self, SettingsFile};
use crate::start_with_windows;
use crate::win32::{low_word, struct_size};
use crate::{
    capture, clipboard, event, folder_picker, global_shortcut, graphics, hidden_icons_panel, log,
    png, save_as, settings_window, snip_file, theme, tray, tray_menu,
};

/// The resident window's class, which a copy opened again looks for to call the running copy.
const CLASS_NAME: PCWSTR = w!("ScreenGauge.Resident");

/// Explorer broadcasts this message when the taskbar is created: at sign-in, and when Explorer restarts
/// and has dropped every tray icon.
static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);

/// The call that a copy opened again sends the running copy; the running copy also sends it to itself, from the tray
/// icon and at startup, so every call but the global shortcut's goes the same way. Registered by name, so each copy
/// gets the same number, whichever installed it.
const CALL_NAME: PCWSTR = w!("ScreenGauge.Call");
static CALL: AtomicU32 = AtomicU32::new(0);

thread_local! {
    /// The resident window, once it is made: what tells that ScreenGauge is exiting (`is_exiting`).
    static RESIDENT: Cell<Option<HWND>> = const { Cell::new(None) };
    /// Save As and the frozen screen were asked to come back while Settings' folder window was open: they wait for it
    /// to close, as the front goes to Settings when it does (`folder_window_closed`).
    static COMING_BACK_AFTER_FOLDER_WINDOW: Cell<bool> = const { Cell::new(false) };
    /// The frozen screen, with its session, while the tools are open. Only the resident window's procedure touches it.
    /// Opening and closing set it and take it out, never while it is borrowed, because they show and destroy windows;
    /// what runs while it is borrowed dispatches no message.
    static FROZEN_SCREEN: RefCell<Option<FrozenScreen>> = const { RefCell::new(None) };
}

/// Calls ScreenGauge right away when the user opened it, since opening it is a call, but not when Windows started it for
/// a notification's click or at sign-in (`started_by`), then runs until the user exits from the tray icon, with what the
/// settings file holds and the file to save it to.
pub fn run(
    file: Option<SettingsFile>,
    stored: Stored,
    started_by: StartedBy,
) -> windows::core::Result<()> {
    let shortcut = stored.options.global_shortcut;
    settings_file::keep(file, stored);
    TASKBAR_CREATED.store(register_message(w!("TaskbarCreated"))?, Ordering::Relaxed);
    CALL.store(register_message(CALL_NAME)?, Ordering::Relaxed);

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
    save_as::register_class(instance)?;
    settings_window::register_class(instance)?;
    tray_menu::register_class(instance)?;
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
    RESIDENT.set(Some(hwnd));
    // Before anything else that may wait: when Windows started ScreenGauge for a notification's click, the click comes
    // once the activator is registered, and it is posted to this window.
    notification::start(hwnd);
    // At sign-in the taskbar may not exist yet; the icon then comes with TaskbarCreated, so a failure
    // here leaves ScreenGauge running.
    if let Err(e) = tray::add(hwnd) {
        log::failure("tray icon: adding it at startup", &e);
    }
    // Another program may hold the combination, or Windows may have taken it: ScreenGauge keeps running, called from
    // the tray icon, and a notification leads the user to Settings to choose another one (WPC-324). Whatever the
    // failure, the user hears of it the same way, as in Settings' recorder; only another program's hold isn't logged.
    match global_shortcut::register(hwnd, shortcut) {
        Ok(()) => notification::shortcut_free(),
        Err(e) => {
            if e.code() != ERROR_HOTKEY_ALREADY_REGISTERED.to_hresult() {
                log::failure(&format!("global shortcut: registering {shortcut}"), &e);
            }
            notification::wait(Notification::ShortcutBusy);
        }
    }
    // At sign-in ScreenGauge starts hidden, its tools closed until a call (WPC-326).
    if started_by == StartedBy::User {
        post_call(hwnd, None);
    }
    // After the call, so a notification from startup, such as the damaged settings file, never lands in its photo;
    // when the call opened the tools, it waits for them to close.
    notification::post_show_waiting(hwnd);

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
            // Explorer sends this message and waits for the answer, so the call is posted and answered at once: the
            // photo is taken once the click is over, with only the icon's highlight left of it.
            if tray::is_call(lparam) {
                post_call(hwnd, Some(wparam));
            }
            // A right click, or the menu key while the keyboard is on the icon, opens its menu (WPC-325). Explorer
            // waits for the answer, but the menu is modeless: it opens and the answer goes at once.
            if low_word(lparam) == WM_CONTEXTMENU {
                open_tray_menu(hwnd, tray::anchor_of(wparam));
            }
            LRESULT(0)
        }
        tray_menu::CHOSEN => {
            match tray_menu::chosen(wparam) {
                Some(tray_menu::Item::Settings) => open_settings(),
                Some(tray_menu::Item::StartWithWindows) => click_start_with_windows(),
                Some(tray_menu::Item::Exit) => {
                    // SAFETY: our own window, on the thread that created it.
                    if let Err(e) = unsafe { DestroyWindow(hwnd) } {
                        log::failure("exit: DestroyWindow", &e);
                    }
                }
                None => log::failure("tray menu: an item it doesn't have", &wparam.0),
            }
            LRESULT(0)
        }
        WM_HOTKEY if global_shortcut::is_global_shortcut(wparam) => {
            // While Settings' field records a new combination, the global shortcut doesn't call ScreenGauge (WPC-323).
            if !settings_window::is_recording() {
                on_call(hwnd, Input::GlobalShortcut);
            }
            LRESULT(0)
        }
        frozen_screen::EFFECTS_READY => {
            let effects = FROZEN_SCREEN.with_borrow(|open| {
                open.as_ref()
                    .map(FrozenScreen::take_effects)
                    .unwrap_or_default()
            });
            carry_out(hwnd, effects);
            LRESULT(0)
        }
        notification::SHOW_WAITING => {
            notification::show_waiting();
            LRESULT(0)
        }
        notification::EVENTS_READY => {
            // Once the notifications' state is no longer borrowed: opening Settings dispatches messages.
            for clicked in notification::on_events_ready() {
                if clicked.opens_settings() {
                    open_settings();
                }
            }
            LRESULT(0)
        }
        // The scale of the display the taskbar is on may have changed, and the tray icon then needs another size.
        // This window, never shown, gets WM_SETTINGCHANGE and WM_DISPLAYCHANGE for it (WPC-304); WM_DPICHANGED is
        // handled the same in case a Windows build sends it.
        WM_DPICHANGED | WM_DISPLAYCHANGE | WM_SETTINGCHANGE => {
            if let Err(e) = tray::follow_scale(hwnd) {
                log::failure("tray icon: following the taskbar's scale", &e);
            }
            // Windows says so when its app mode changes ("ImmersiveColorSet"), among other settings. The theme is read
            // again on any of them, which are rare, and what ScreenGauge shows changes only when its palette does.
            if message == WM_SETTINGCHANGE {
                follow_theme();
            }
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == tray::SCALE_TIMER => {
            if let Err(e) = tray::on_scale_timer(hwnd) {
                log::failure("tray icon: following the taskbar's scale", &e);
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            // Save As runs its own message loop, which this exit may be inside of: it closes first.
            save_as::cancel();
            // Settings' folder window runs its own message loop too.
            folder_picker::cancel();
            close_for_exit(hwnd);
            // Settings and the menu draw with the factories released next, so they close before them.
            settings_window::close();
            tray_menu::close();
            graphics::release();
            if let Err(e) = global_shortcut::unregister() {
                log::failure("exit: unregistering the global shortcut", &e);
            }
            if let Err(e) = tray::remove(hwnd) {
                log::failure("exit: removing the tray icon", &e);
            }
            notification::stop();
            // SAFETY: ends this thread's message loop.
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        _ if message == TASKBAR_CREATED.load(Ordering::Relaxed) => {
            if let Err(e) = tray::add(hwnd) {
                log::failure("tray icon: adding it when the taskbar was created", &e);
            }
            // A notification Windows refused before the taskbar existed, as at sign-in, gets another chance.
            notification::show_waiting();
            LRESULT(0)
        }
        _ if message == CALL.load(Ordering::Relaxed) => {
            // A click on the tray icon inside the hidden icons panel leaves the panel open: it goes before the photo
            // (WPC-329). Hidden here, not while Explorer waits for the icon's message to be answered. Only the tray
            // icon's call hides it: through the global shortcut, a panel the user opened is in the photo like anything
            // else open.
            if let Some(anchor) = tray_anchor(wparam, lparam) {
                hidden_icons_panel::hide_holding(anchor);
            }
            on_call(hwnd, Input::Called);
            LRESULT(0)
        }
        // SAFETY: the arguments are the ones Windows passed to this window procedure.
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}

/// A call opens the tools, or, when they are open, goes to the session core as `input`: the global shortcut closes
/// them, and the tray icon or opening ScreenGauge again (`Input::Called`) leaves them as they are.
fn on_call(resident: HWND, input: Input) {
    // Whatever called, the tray icon's menu goes, and before the photo, so it is never in it.
    tray_menu::close_before_photo();
    // Read before the photo, which takes about 120 ms: the double-click began with the call.
    let second_click_until = second_click_until(&input);
    let told = FROZEN_SCREEN.with_borrow(|open| {
        open.as_ref()
            .map(|frozen| frozen.handle(event::of_message(input)))
    });
    match told {
        Some(effects) => carry_out(resident, effects),
        None => match open(resident, second_click_until) {
            Ok(frozen) => {
                FROZEN_SCREEN.set(Some(frozen));
                notification::tools_opened();
                // After the frozen screen shows, so taking it out never delays the photo.
                notification::capture_succeeded();
            }
            // Whatever kept the tools closed, the user hears of it, never a failure in silence (v1 spec WPC-296,
            // Frozen screen). Most often it is a monitor neither capture path could photograph.
            Err(e) => {
                log::failure("frozen screen: opening it", &e);
                notification::show(Notification::CaptureFailed);
            }
        },
    }
}

/// Opens the tools, taking a press before `second_click_until` for the second click of the double-click that called
/// them: the photo comes first, before any window of ScreenGauge appears or takes the keyboard, so whatever was on
/// screen at the call (a hover, an open menu, a tooltip) is in it.
fn open(
    resident: HWND,
    second_click_until: Option<Duration>,
) -> windows::core::Result<FrozenScreen> {
    let frozen_screen = capture::every_monitor()?;
    let Stored {
        options,
        remembered,
    } = settings_file::current().map_err(|e| Error::new(E_FAIL, e.to_string()))?;
    let mode = theme::mode(options.theme);
    let session = Session::open(Opening {
        frozen_screen,
        cursor: event::cursor_now()?,
        options,
        remembered,
        second_click_until,
    })
    .map_err(|refused| Error::new(E_FAIL, refused.to_string()))?;
    FrozenScreen::show(session, resident, mode)
}

/// Until when a press over the frozen screen is the second click of a double-click that called ScreenGauge: Windows'
/// double-click time from the call, for any call but the global shortcut's (the owner's choice in WPC-308). The tray
/// icon is the call a double-click habit reaches, since the photo shows about 140 ms after its first click (WPC-301).
fn second_click_until(call: &Input) -> Option<Duration> {
    if *call == Input::GlobalShortcut {
        return None;
    }
    // SAFETY: no arguments.
    let double_click = Duration::from_millis(u64::from(unsafe { GetDoubleClickTime() }));
    Some(event::uptime() + double_click)
}

/// Does what the session core asked, in its order, for the resident window `resident`.
fn carry_out(resident: HWND, effects: Vec<Effect>) {
    for effect in effects {
        match effect {
            Effect::Close => {
                close();
            }
            Effect::SaveRemembered(remembered) => {
                save_remembered(remembered);
            }
            Effect::MoveCursor(point) => {
                // SAFETY: no pointers. The process is Per-Monitor V2, so the point is in physical pixels.
                if let Err(e) = unsafe { SetCursorPos(point.x, point.y) } {
                    log::failure("session: moving the cursor", &e);
                }
            }
            // The notice "Copied" already shows: should Windows refuse the clipboard after the retries, it is logged.
            // TODO(WPC-352): tell the user when the copy fails instead of showing "Copied" anyway.
            Effect::CopyText(text) => {
                if let Err(e) = clipboard::copy_text(resident, &text) {
                    log::failure("session: copying text", &e);
                }
            }
            // As with the text: "Copied" already shows.
            // TODO(WPC-352): tell the user when the copy fails instead of showing "Copied" anyway.
            Effect::CopyImage(image) => {
                if let Err(e) = clipboard::copy_image(resident, &image) {
                    log::failure("session: copying an image", &e);
                }
            }
            Effect::WakeAt(uptime) => {
                FROZEN_SCREEN.with_borrow(|open| {
                    // With the tools closed there is nothing left to wake: what was due to go went with them.
                    if let Some(frozen) = open {
                        frozen.wake_at(uptime);
                    }
                });
            }
            Effect::WritePng { to, image } => {
                let written = write_png(&to, &image);
                tell(resident, written);
            }
            Effect::OpenSaveAs { folder, file_name } => {
                let closed = open_save_as(folder.as_ref(), &file_name);
                // ScreenGauge exited while Save As was open: the end of the message loop it asked for went to Save
                // As's own loop, which kept it, so it is asked for again (seen in WPC-318: the process stayed, with no
                // window).
                // SAFETY: a handle only compared, never used.
                if !unsafe { IsWindow(Some(resident)) }.as_bool() {
                    // SAFETY: ends this thread's message loop.
                    unsafe { PostQuitMessage(0) };
                    return;
                }
                tell(resident, closed);
            }
            Effect::StepAside => {
                save_as::step_aside();
                FROZEN_SCREEN.with_borrow(|open| open.as_ref().map(FrozenScreen::step_aside));
            }
            Effect::ComeBack => {
                // Settings' folder window may be open, from Settings used while the frozen screen and Save As waited
                // aside: they come back over everything, and it would be left under them, which Save As's own loop
                // waits on. It closes as if the user cancelled it, nothing changes, and they come back once it is gone.
                if folder_picker::is_open() {
                    folder_picker::cancel();
                    COMING_BACK_AFTER_FOLDER_WINDOW.set(true);
                } else {
                    come_back();
                }
            }
            // The tools closed just before, by the gear: Settings opens over the programs.
            Effect::OpenSettings => {
                open_settings();
            }
        }
    }
}

/// Draws Settings and the tools, whichever are open, in the palette of the theme as it is now: in Automatic, Windows'
/// app mode may have just changed, or the user chose another theme in Settings (v1 spec WPC-296, Theme; WPC-321).
pub fn follow_theme() {
    let theme = match settings_file::current() {
        Ok(stored) => stored.options.theme,
        Err(e) => {
            log::failure("theme: following Windows", &e);
            return;
        }
    };
    let mode = theme::mode(theme);
    settings_window::follow(mode);
    FROZEN_SCREEN.with_borrow(|open| {
        if let Some(frozen) = open {
            frozen.follow(mode);
        }
    });
}

/// Opens the tray icon's menu for a right click at `anchor`, in the palette of the theme the settings file holds, with
/// "Start with Windows" checked as Windows holds it.
fn open_tray_menu(resident: HWND, anchor: POINT) {
    // Unreadable, it shows unchecked, and a click reads it again, and says so should it fail too.
    let checked = start_with_windows::state().map_or_else(
        |e| {
            log::failure("tray menu: reading Start with Windows", &e);
            false
        },
        StartWithWindows::is_on,
    );
    let opened = settings_file::current()
        .map_err(|e| Error::new(E_FAIL, e.to_string()))
        .and_then(|stored| {
            tray_menu::open(resident, anchor, theme::mode(stored.options.theme), checked)
        });
    if let Err(e) = opened {
        log::failure("tray menu: opening it", &e);
    }
}

/// The tray menu's "Start with Windows". The menu has no room for the amber message: when there is one to show, as with
/// it turned off in Windows, Settings opens with it (the owner's choice in WPC-326).
fn click_start_with_windows() {
    if settings_window::click_start_with_windows() {
        open_settings();
    }
}

/// Opens Settings, or brings it to the front, in the palette of the theme the settings file holds.
fn open_settings() {
    let theme = settings_file::current()
        .map(|stored| stored.options.theme)
        .map_err(|e| Error::new(E_FAIL, e.to_string()));
    let opened = theme.and_then(|theme| settings_window::open(theme::mode(theme)));
    if let Err(e) = opened {
        log::failure("Settings: opening it", &e);
    }
}

/// Brings the frozen screen back over every program, and Save As over it.
fn come_back() {
    FROZEN_SCREEN.with_borrow(|open| open.as_ref().map(FrozenScreen::come_back));
    save_as::come_back();
}

/// Settings' folder window closed: should the frozen screen and Save As have been asked to come back meanwhile, they do
/// now. Not before: closing it gives the front to Settings, which would send them aside again.
pub fn folder_window_closed() {
    if COMING_BACK_AFTER_FOLDER_WINDOW.take() {
        come_back();
    }
}

/// Whether ScreenGauge is exiting: the resident window is destroyed or being destroyed, as when a message loop that was
/// asked to end kept the request (Save As's, Settings' folder window's). False while it runs.
pub fn is_exiting() -> bool {
    RESIDENT.get().is_some_and(|hwnd| {
        // SAFETY: a handle only compared, never used.
        !unsafe { IsWindow(Some(hwnd)) }.as_bool()
    })
}

/// Hands the session `input`, which the Windows layer made itself, and carries out what it returns. With the tools
/// closed meanwhile, there is no session to tell, and nothing to do.
fn tell(resident: HWND, input: Input) {
    let told = FROZEN_SCREEN
        .with_borrow(|open| open.as_ref().map(|frozen| frozen.handle(event::now(input))));
    if let Some(effects) = told {
        carry_out(resident, effects);
    }
}

/// Writes the Snip's `image` as a PNG file where `to` says. The log never shows the path, which holds the Windows user
/// name.
fn write_png(to: &SaveTo, image: &Image) -> Input {
    let bytes = match png::encode(image) {
        Ok(bytes) => bytes,
        Err(e) => {
            log::failure("Snip: making the PNG", &e);
            return Input::NotSaved;
        }
    };
    match snip_file::write(to, &bytes) {
        Ok(_) => Input::Saved,
        Err(e) => {
            log::failure("Snip: writing the file", &e);
            Input::NotSaved
        }
    }
}

/// Opens Save As in `folder`, suggesting `file_name`, and waits until it closes. Its own message loop runs meanwhile,
/// which is why it is called here, outside every borrow of the frozen screen.
fn open_save_as(folder: Option<&Folder>, file_name: &str) -> Input {
    let cursor = event::cursor_now().unwrap_or_else(|e| {
        log::failure("Save As: finding the cursor", &e);
        event::message_cursor()
    });
    let front_after = FROZEN_SCREEN.with_borrow(|open| open.as_ref()?.window_at(cursor));
    match save_as::choose(cursor, front_after, folder, file_name) {
        Ok(chosen) => Input::SaveAsClosed(chosen),
        Err(e) => {
            log::failure("Save As: opening it", &e);
            Input::SaveAsFailed
        }
    }
}

/// ScreenGauge exits: open tools close through the session, so what this call changed is kept as closing them any
/// other way keeps it, also with Save As open (WPC-325). With no tools open there is nothing to close.
fn close_for_exit(resident: HWND) {
    tell(resident, Input::Exit);
    // The session closes them, unless it had closed already and left its windows to this.
    if FROZEN_SCREEN.with_borrow(Option::is_some) {
        close();
    }
}

/// Dropping the frozen screen closes its windows, drops the session and erases the photos. The notifications that
/// waited for the tools to close then show. Every way of closing the tools comes here, so none leaves them waiting.
fn close() {
    drop(FROZEN_SCREEN.with_borrow_mut(Option::take));
    notification::tools_closed();
}

/// Keeps `remembered` for the next call, and in the settings file when there is one. Only the remembered state is
/// replaced: an option changed in Settings since the tools opened stays as it was changed.
fn save_remembered(remembered: Remembered) {
    let saved = settings_file::update(|stored| {
        let changed = stored.remembered != remembered;
        stored.remembered = remembered;
        changed
    });
    if let Err(e) = saved {
        log::failure("settings: saving the remembered state", &e);
    }
}

/// Posts the call to this copy's own resident window, which handles it once the message being handled is answered. A
/// call from the tray icon carries the icon's wParam, which says where it was clicked; startup's carries nothing.
fn post_call(resident: HWND, from_tray: Option<WPARAM>) {
    let (wparam, lparam) = from_tray.map_or((WPARAM(0), LPARAM(0)), tray_call);
    let message = CALL.load(Ordering::Relaxed);
    // SAFETY: our own window; the message carries two numbers.
    if let Err(e) = unsafe { PostMessageW(Some(resident), message, wparam, lparam) } {
        log::failure("call: posting it", &e);
    }
}

/// The wParam of a `CALL` from the tray icon. Startup and a copy opened again post 0.
const FROM_TRAY: usize = 1;

/// The wParam and lParam of a `CALL` from the tray icon, whose own message had `anchor` for wParam.
fn tray_call(anchor: WPARAM) -> (WPARAM, LPARAM) {
    (WPARAM(FROM_TRAY), LPARAM(anchor.0.cast_signed()))
}

/// Where the tray icon was clicked, for a `CALL` from it (`tray_call`); `None` for any other call.
fn tray_anchor(wparam: WPARAM, lparam: LPARAM) -> Option<POINT> {
    (wparam.0 == FROM_TRAY).then(|| tray::anchor_of(WPARAM(lparam.0.cast_unsigned())))
}

/// For a copy opened while another runs: calls the running copy. Only the program the user just opened may bring a
/// window to the front, so it passes that right on to the running copy, whose frozen screen can then take the keyboard.
pub fn call_running_copy() -> windows::core::Result<()> {
    call_window_of_class(CLASS_NAME)
}

/// `call_running_copy` for a resident window of class `class`, a null-terminated name that outlives the call; the
/// tests give a class of their own.
fn call_window_of_class(class: PCWSTR) -> windows::core::Result<()> {
    let call_message = register_message(CALL_NAME)?;
    // FindWindowW sets no error code when no window has the class, and claiming the single instance has just left
    // one, so it is cleared first.
    // SAFETY: sets this thread's last error.
    unsafe { SetLastError(ERROR_SUCCESS) };
    // SAFETY: a valid class name, as this function requires, and no window name.
    let running = unsafe { FindWindowW(class, PCWSTR::null()) }.map_err(|e| {
        // The running copy may have no window yet: opened twice in quick succession, the first copy still calls itself
        // once it has one.
        if e.code().is_ok() {
            Error::new(E_FAIL, "the running copy has no resident window")
        } else {
            e
        }
    })?;
    let mut process = 0;
    // SAFETY: fills a local.
    if unsafe { GetWindowThreadProcessId(running, Some(&mut process)) } == 0 {
        // The window closed since FindWindowW: the running copy is exiting.
        return Err(Error::from_thread());
    }
    // Without the right, the frozen screen still opens, and the keyboard reaches it once it is clicked.
    // SAFETY: a process ID; Windows checks that this process may give the right.
    if let Err(e) = unsafe { AllowSetForegroundWindow(process) } {
        log::failure("call: letting the running copy come to the front", &e);
    }
    // SAFETY: the message carries nothing.
    unsafe { PostMessageW(Some(running), call_message, WPARAM(0), LPARAM(0)) }
}

/// A message number every program gets for the same `name`, a null-terminated name that outlives the call.
fn register_message(name: PCWSTR) -> windows::core::Result<u32> {
    // SAFETY: a valid, null-terminated message name.
    match unsafe { RegisterWindowMessageW(name) } {
        0 => Err(Error::from_thread()),
        message => Ok(message),
    }
}

#[cfg(test)]
mod tests {
    use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, SetLastError};
    use windows::Win32::UI::WindowsAndMessaging::{PM_REMOVE, PeekMessageW};
    use windows::core::HSTRING;

    use super::*;

    /// A window class of its own per test, so a ScreenGauge running on this computer never receives a test's call.
    fn test_class(test: &str) -> HSTRING {
        HSTRING::from(format!("ScreenGauge.Test.{}.{test}", std::process::id()))
    }

    extern "system" fn default_proc(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        // SAFETY: the arguments are the ones Windows passed to this window procedure.
        unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
    }

    /// A hidden top-level window of `class`, standing in for the running copy's resident window.
    fn running_copy(class: &HSTRING) -> HWND {
        // SAFETY: no module name, so this is the handle of the test executable.
        let instance: HINSTANCE = unsafe { GetModuleHandleW(None) }
            .expect("GetModuleHandleW")
            .into();
        let window_class = WNDCLASSEXW {
            cbSize: const { struct_size::<WNDCLASSEXW>() },
            lpfnWndProc: Some(default_proc),
            hInstance: instance,
            lpszClassName: PCWSTR(class.as_ptr()),
            ..Default::default()
        };
        // SAFETY: `window_class` is fully initialized, and the class name outlives the test.
        assert_ne!(unsafe { RegisterClassExW(&window_class) }, 0);
        // SAFETY: the class was just registered. The window is never shown.
        unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                PCWSTR(class.as_ptr()),
                None,
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
        }
        .expect("CreateWindowExW")
    }

    #[test]
    fn a_copy_opened_again_calls_the_running_copy() {
        let class = test_class("running");
        let running = running_copy(&class);

        call_window_of_class(PCWSTR(class.as_ptr())).expect("calling the running copy");

        let call_message = register_message(CALL_NAME).expect("the call message");
        let mut message = MSG::default();
        // SAFETY: `message` is a valid MSG for the call to fill; the window is this thread's.
        let received = unsafe {
            PeekMessageW(
                &mut message,
                Some(running),
                call_message,
                call_message,
                PM_REMOVE,
            )
        }
        .as_bool();
        assert!(received, "the running copy received no call");
        // SAFETY: the test's own window, on the thread that created it.
        unsafe { DestroyWindow(running) }.expect("DestroyWindow");
    }

    #[test]
    fn a_call_from_the_tray_icon_carries_where_it_was_clicked() {
        // The icon's wParam for a click at (-1200, 2100), on a monitor left of the main one: x in the low word, y in
        // the high word.
        let (wparam, lparam) = tray_call(WPARAM(0x0834_FB50));
        assert_eq!(
            tray_anchor(wparam, lparam),
            Some(POINT { x: -1200, y: 2100 })
        );
    }

    #[test]
    fn a_call_from_startup_or_a_copy_opened_again_carries_no_place() {
        // What both post: nothing.
        assert_eq!(tray_anchor(WPARAM(0), LPARAM(0)), None);
    }

    #[test]
    fn with_no_copy_running_the_call_fails_and_says_so() {
        let class = test_class("none");
        // What claiming the single instance leaves behind in a copy opened again, so the failure can't be read from it.
        // SAFETY: sets this thread's last error.
        unsafe { SetLastError(ERROR_ALREADY_EXISTS) };
        let error = call_window_of_class(PCWSTR(class.as_ptr()))
            .expect_err("there is no running copy to call");
        assert_eq!(error.code(), E_FAIL, "{error}");
    }
}
