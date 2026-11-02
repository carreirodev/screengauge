//! What Settings' global shortcut recorder does (v1 spec WPC-296, Settings; WPC-323). A click on the field holds the
//! keyboard (`keyboard_hook`) and records; the recorder's rules (`screengauge_core::settings::Recorder`) decide each key;
//! a combination they accept is registered with Windows before the one there was is let go (`global_shortcut`), and then
//! saved. Esc, a click outside the field, Settings losing the front or closing, and ScreenGauge exiting stop recording,
//! and the keyboard goes back to Windows. While it records, the global shortcut doesn't call ScreenGauge
//! (`is_recording`).
//!
//! A refusal shows as a message under the row's label (`row_message`), and the window grows to hold it while it shows.
//!
//! Only the resident thread touches it, from Settings' window procedure.

use std::cell::{Cell, RefCell};
use std::time::Instant;

use screengauge_core::settings::{Change, Recorder, Shortcut, Step, Tried};
use windows::Win32::Foundation::{E_FAIL, ERROR_HOTKEY_ALREADY_REGISTERED, HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{KillTimer, SetTimer};
use windows::core::Error;

use super::layout::{Grown, Row};
use super::paint;
use super::recorder_field::{self, View};
use super::row_message::Message;
use crate::keyboard_hook::{self, KeyEvent, KeyboardHook};
use crate::{global_shortcut, graphics, log, settings_file};

/// The timer that draws the field again while it records, so its text blinks, and how often, in milliseconds: 25 times
/// a second, smooth for a fade of half a second.
pub const BLINK_TIMER: usize = 1;
const BLINK_EVERY: u32 = 40;

thread_local! {
    static RECORDER: Cell<Recorder> = Cell::new(Recorder::default());
    /// The keyboard hook, from the click on the field until the keys it kept are all up.
    static HOOK: RefCell<Option<KeyboardHook>> = const { RefCell::new(None) };
    /// Counts the hooks started, so the end of one stopped before isn't taken for the running one's.
    static GENERATION: Cell<usize> = const { Cell::new(0) };
    /// When the field started recording, for the blink.
    static SINCE: Cell<Option<Instant>> = const { Cell::new(None) };
    /// The message under the row's label, and how much it grows the row, while it shows.
    static MESSAGE: Cell<Option<Message>> = const { Cell::new(None) };
    static GROWN: Cell<Option<Grown>> = const { Cell::new(None) };
}

/// Whether the field records: the global shortcut then doesn't call ScreenGauge.
pub fn is_recording() -> bool {
    RECORDER.get().is_recording()
}

/// What the field shows now, with `shortcut` the global shortcut.
pub fn view(shortcut: Shortcut) -> View {
    let recorder = RECORDER.get();
    View {
        shortcut,
        recording: recorder.is_recording(),
        held: recorder.held(),
        opacity: SINCE
            .get()
            .map_or(1.0, |since| recorder_field::opacity(since.elapsed())),
    }
}

/// The message under the row's label, while it shows.
pub fn message() -> Option<Message> {
    MESSAGE.get()
}

/// How much the message makes its row grow, while it shows.
pub fn grown() -> Option<Grown> {
    GROWN.get()
}

/// A click on the field: it records, and holds the keyboard. Should the hook fail, the field doesn't record, since
/// a combination with Win would then open the Start menu.
pub fn click_field(hwnd: HWND) {
    let mut recorder = RECORDER.get();
    let was_recording = recorder.is_recording();
    recorder.start();
    if !was_recording {
        match hold_keyboard(hwnd) {
            Ok(()) => blink(hwnd, true),
            Err(e) => {
                log::failure("Settings: holding the keyboard to record a shortcut", &e);
                recorder.cancel();
            }
        }
    }
    RECORDER.set(recorder);
    refresh(hwnd);
}

/// Reset: stops recording, should the field record, and makes `Win + Shift + X` the global shortcut.
pub fn click_reset(hwnd: HWND) {
    let mut recorder = RECORDER.get();
    if recorder.cancel() == Step::Stop {
        let_keyboard_go(hwnd);
    }
    recorder.tried(register(Shortcut::default()));
    RECORDER.set(recorder);
    refresh(hwnd);
}

/// A click outside the field, or Settings losing the front: recording stops, and the shortcut there was stays.
pub fn cancel(hwnd: HWND) {
    let mut recorder = RECORDER.get();
    if recorder.cancel() == Step::Stop {
        RECORDER.set(recorder);
        let_keyboard_go(hwnd);
        refresh(hwnd);
    }
}

/// A key the hook saw go down or up ([`keyboard_hook::KEY`]). One that comes after recording stopped, already posted,
/// changes nothing.
pub fn on_key(hwnd: HWND, wparam: WPARAM, lparam: LPARAM) {
    let event = KeyEvent::of(wparam, lparam);
    let mut recorder = RECORDER.get();
    let before = recorder;
    let step = if event.down {
        recorder.key_down(event.held, keyboard_hook::pressed(event.vk))
    } else {
        recorder.key_up(event.held);
        Step::Show
    };
    let stopped = match step {
        Step::Show => false,
        Step::Stop => true,
        Step::Try(shortcut) => {
            recorder.tried(register(shortcut));
            true
        }
    };
    RECORDER.set(recorder);
    if stopped {
        let_keyboard_go(hwnd);
    }
    if recorder != before {
        refresh(hwnd);
    }
}

/// The hook's thread said its hook is removed ([`keyboard_hook::ENDED`]). Should the field record again meanwhile, from
/// a click before the keys were all up, a new hook holds the keyboard.
pub fn on_hook_ended(hwnd: HWND, wparam: WPARAM) {
    let this_one = HOOK.with_borrow(|hook| {
        hook.as_ref()
            .is_some_and(|hook| hook.generation() == wparam.0)
    });
    if !this_one {
        return;
    }
    drop(HOOK.take());
    if is_recording()
        && let Err(e) = hold_keyboard(hwnd)
    {
        log::failure(
            "Settings: holding the keyboard again to record a shortcut",
            &e,
        );
        cancel(hwnd);
    }
}

/// Settings is closing, as its X does or as ScreenGauge exits: the hook goes now, and the field rests.
pub fn close() {
    drop(HOOK.take());
    RECORDER.set(Recorder::default());
    SINCE.set(None);
    MESSAGE.set(None);
    GROWN.set(None);
}

/// Has the hook keep the keyboard: the one there is when the keys of the last recording aren't all up yet, or a new one.
fn hold_keyboard(hwnd: HWND) -> windows::core::Result<()> {
    let resumed = HOOK.with_borrow(|hook| hook.as_ref().map(KeyboardHook::record));
    match resumed {
        Some(Ok(())) => return Ok(()),
        Some(Err(e)) => {
            log::failure("Settings: recording again with the keyboard hook", &e);
            drop(HOOK.take());
        }
        None => {}
    }
    let generation = GENERATION.get().wrapping_add(1);
    GENERATION.set(generation);
    HOOK.set(Some(KeyboardHook::start(hwnd, generation)?));
    Ok(())
}

/// Recording stopped: the hook lets each key go once it is up, and the blink stops.
fn let_keyboard_go(hwnd: HWND) {
    blink(hwnd, false);
    let drained = HOOK.with_borrow(|hook| hook.as_ref().map(KeyboardHook::drain));
    if let Some(Err(e)) = drained {
        // The hook's thread can't be told: it is removed now instead.
        log::failure("Settings: letting the keyboard go", &e);
        drop(HOOK.take());
    }
}

/// Starts or stops the blink's timer.
fn blink(hwnd: HWND, on: bool) {
    if on {
        SINCE.set(Some(Instant::now()));
        // SAFETY: our own window, on the thread that created it; no callback, so WM_TIMER comes to it.
        if unsafe { SetTimer(Some(hwnd), BLINK_TIMER, BLINK_EVERY, None) } == 0 {
            log::failure("Settings: the blink's timer", &Error::from_thread());
        }
    } else if SINCE.take().is_some() {
        // SAFETY: as above.
        if let Err(e) = unsafe { KillTimer(Some(hwnd), BLINK_TIMER) } {
            log::failure("Settings: stopping the blink's timer", &e);
        }
    }
}

/// Makes `shortcut` the global shortcut and saves it, or says another program holds it. Any other failure to register
/// is logged, and the user sees the same message: the shortcut there was still works.
fn register(shortcut: Shortcut) -> Tried {
    match global_shortcut::change(shortcut) {
        Ok(()) => {
            let saved = settings_file::update(|stored| {
                stored.options.apply(Change::GlobalShortcut(shortcut))
            });
            // It holds in this run even when the file couldn't be saved.
            if let Err(e) = saved {
                log::failure("Settings: saving the global shortcut", &e);
            }
            Tried::Taken
        }
        Err(e) => {
            if e.code() != ERROR_HOTKEY_ALREADY_REGISTERED.to_hresult() {
                log::failure(&format!("Settings: registering {shortcut}"), &e);
            }
            Tried::AlreadyUsed
        }
    }
}

/// Draws the field again, with the message there is now, and resizes the window should the message's row change height.
fn refresh(hwnd: HWND) {
    let (message, grown) = match measured() {
        Ok(measured) => measured,
        Err(e) => {
            log::failure("Settings: laying out the shortcut's message", &e);
            (None, None)
        }
    };
    MESSAGE.set(message);
    if GROWN.replace(grown) == grown {
        super::invalidate(hwnd);
    } else {
        super::follow_page(hwnd);
    }
}

/// The message the recorder's refusal shows, should there be one, and how much it grows the row.
fn measured() -> windows::core::Result<(Option<Message>, Option<Grown>)> {
    let Some(refusal) = RECORDER.get().refusal() else {
        return Ok((None, None));
    };
    let shortcut = settings_file::current()
        .map_err(|e| Error::new(E_FAIL, e.to_string()))?
        .options
        .global_shortcut;
    let fonts = graphics::fonts()?;
    let measure = paint::measurer(&fonts, 1.0);
    let message = Message {
        row: Row::GlobalShortcut,
        text: recorder_field::message(refusal),
        band: false,
        width: recorder_field::message_room(&view(shortcut), &measure)?,
    };
    let grown = message.growth(&fonts)?;
    Ok((Some(message), Some(grown)))
}
