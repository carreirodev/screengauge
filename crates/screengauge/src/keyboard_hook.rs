//! The keyboard hook of Settings' global shortcut recorder (WPC-323): while the field records, a low-level keyboard hook
//! keeps every key from Windows and the other programs, so a combination with Win records without the Start menu
//! opening, and tells the Settings window which key went down or up. It is how PowerToys records shortcuts, and how
//! WPC-281's hardware test recorded `Win + Alt + J` (`prototype/hardware-test-keys`, `keys/llhook.rs`).
//!
//! Once recording stops, the hook keeps only the key-ups (and repeats) of the keys it kept the key-down of, so Windows
//! never sees a Win come up it never saw go down, and passes every other key. When the last of them is up, the hook is
//! removed and the keyboard is Windows' again. A key-up Windows never delivers, as across Ctrl + Alt + Del, costs one
//! later press of that key, never the keyboard.
//!
//! The hook runs on a thread of its own, whose message loop nothing else can hold up: Windows removes a hook that takes
//! longer than its timeout to answer, silently, and the keys would then reach the Start menu. The hook procedure only
//! decides, and posts what it saw to the Settings window; it never logs, paints or waits.

use std::cell::Cell;
use std::sync::mpsc;
use std::thread::{self, JoinHandle};

use screengauge_core::settings::{Key, Modifiers, Pressed};
use windows::Win32::Foundation::{E_FAIL, ERROR_INVALID_THREAD_ID, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, HC_ACTION, KBDLLHOOKSTRUCT, MSG, PM_NOREMOVE,
    PeekMessageW, PostMessageW, PostThreadMessageW, SetWindowsHookExW, TranslateMessage,
    UnhookWindowsHookEx, WH_KEYBOARD_LL, WM_APP, WM_KEYDOWN, WM_KEYUP, WM_QUIT, WM_SYSKEYDOWN,
    WM_SYSKEYUP,
};
use windows::core::Error;

use crate::log;

/// What the hook posts to the Settings window for a key that went down or up while the field records: the wParam is
/// its virtual-key code, and the lParam whether it went down and the modifiers held after it ([`KeyEvent::of`]).
/// WM_APP + 1 to + 6 are taken by the tray icon, the frozen screen, the notifications and the foreground watch.
pub const KEY: u32 = WM_APP + 7;

/// What the hook's thread posts to the Settings window once the hook is removed; the wParam is the number `start` was
/// given, so a hook stopped before can't be taken for the one running.
pub const ENDED: u32 = WM_APP + 8;

/// What the Settings window asks of the hook's thread: record again, or let the keys go once they are up. And what the
/// thread posts itself when the last key it kept is up.
const RECORD: u32 = WM_APP + 9;
const DRAIN: u32 = WM_APP + 10;
const DONE: u32 = WM_APP + 11;

/// The hook's thread, seen from the Settings window. Dropping it removes the hook at once, whatever keys are held.
pub struct KeyboardHook {
    thread_id: u32,
    generation: usize,
    thread: Option<JoinHandle<()>>,
}

impl KeyboardHook {
    /// Installs the hook on a thread of its own, recording, and waits until it is in place. What it sees goes to
    /// `window` as [`KEY`], and its end as [`ENDED`] with `generation`.
    pub fn start(window: HWND, generation: usize) -> windows::core::Result<KeyboardHook> {
        let entry = keys_down();
        let window = window.0.expose_provenance();
        let (ready, installed) = mpsc::channel();
        let thread = thread::Builder::new()
            .name("keyboard hook".to_owned())
            .spawn(move || run(window, generation, entry, &ready))
            .map_err(|e| Error::new(E_FAIL, e.to_string()))?;
        // The thread always answers once, right after installing the hook or failing to; it drops `ready` should it
        // panic before, which ends the wait too.
        match installed.recv() {
            Ok(Ok(thread_id)) => Ok(KeyboardHook {
                thread_id,
                generation,
                thread: Some(thread),
            }),
            Ok(Err(e)) => {
                join(thread);
                Err(e)
            }
            Err(_) => {
                join(thread);
                Err(Error::new(
                    E_FAIL,
                    "the keyboard hook's thread ended before installing it",
                ))
            }
        }
    }

    pub fn generation(&self) -> usize {
        self.generation
    }

    /// Records again, after `drain`: a click on the field before the keys of the last recording were all up.
    pub fn record(&self) -> windows::core::Result<()> {
        self.post(RECORD)
    }

    /// Recording stopped: the hook goes once the keys it kept are up.
    pub fn drain(&self) -> windows::core::Result<()> {
        self.post(DRAIN)
    }

    fn post(&self, message: u32) -> windows::core::Result<()> {
        // SAFETY: a thread of this process, whose queue exists since before it answered `start`. The message carries
        // nothing.
        unsafe { PostThreadMessageW(self.thread_id, message, WPARAM(0), LPARAM(0)) }
    }
}

impl Drop for KeyboardHook {
    /// Removes the hook now, as Settings closes or ScreenGauge exits: a key held then comes up in Windows' hands.
    fn drop(&mut self) {
        let Some(thread) = self.thread.take() else {
            return;
        };
        // Thread IDs are reused across Windows once a thread ends: an ended one is only joined.
        if thread.is_finished() {
            join(thread);
            return;
        }
        // SAFETY: as in `post`.
        match unsafe { PostThreadMessageW(self.thread_id, WM_QUIT, WPARAM(0), LPARAM(0)) } {
            // The thread already ended, after the keys were all up.
            Err(e) if e.code() == ERROR_INVALID_THREAD_ID.to_hresult() => {}
            Err(e) => log::failure("keyboard hook: asking its thread to end", &e),
            Ok(()) => {}
        }
        join(thread);
    }
}

fn join(thread: JoinHandle<()>) {
    if thread.join().is_err() {
        log::failure("keyboard hook", &"its thread panicked");
    }
}

/// A key that went down or up, as the Settings window receives it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyEvent {
    pub vk: u32,
    pub down: bool,
    /// The modifiers held after it.
    pub held: Modifiers,
}

impl KeyEvent {
    /// The event a [`KEY`] message carries.
    pub fn of(wparam: WPARAM, lparam: LPARAM) -> KeyEvent {
        let bits = lparam.0;
        let bit = |n: u32| bits & (1 << n) != 0;
        KeyEvent {
            // The hook posts a virtual-key code, which is below 256; anything wider is no key, which `pressed` calls
            // another key.
            vk: u32::try_from(wparam.0).unwrap_or(u32::MAX),
            down: bit(0),
            held: Modifiers {
                win: bit(1),
                ctrl: bit(2),
                alt: bit(3),
                shift: bit(4),
            },
        }
    }

    fn lparam(down: bool, held: Modifiers) -> LPARAM {
        let bits = [down, held.win, held.ctrl, held.alt, held.shift]
            .iter()
            .enumerate()
            .fold(0, |bits, (n, &on)| bits | (isize::from(on) << n));
        LPARAM(bits)
    }
}

/// How the recorder tells the key with virtual-key code `vk` apart.
pub fn pressed(vk: u32) -> Pressed {
    const ESCAPE: u32 = 0x1B;
    const F1: u32 = 0x70;
    const F12: u32 = 0x7B;
    if modifier(vk).is_some() {
        return Pressed::Modifier;
    }
    let Ok(code) = u8::try_from(vk) else {
        return Pressed::Other;
    };
    match vk {
        ESCAPE => Pressed::Escape,
        // The virtual-key codes of A to Z and of 0 to 9 on the main keys are their ASCII codes.
        0x41..=0x5A => Pressed::Main(Key::Letter(char::from(code))),
        0x30..=0x39 => Pressed::Main(Key::Digit(code - b'0')),
        F1..=F12 => Pressed::Main(Key::Function(code - 0x6F)),
        _ => Pressed::Other,
    }
}

/// The modifier `vk` is, either side: the low-level hook reports the left and right codes, and the generic ones are
/// taken too.
fn modifier(vk: u32) -> Option<Modifier> {
    match vk {
        0x5B | 0x5C => Some(Modifier::Win),
        0x10 | 0xA0 | 0xA1 => Some(Modifier::Shift),
        0x11 | 0xA2 | 0xA3 => Some(Modifier::Ctrl),
        0x12 | 0xA4 | 0xA5 => Some(Modifier::Alt),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Modifier {
    Win,
    Ctrl,
    Alt,
    Shift,
}

/// A set of virtual-key codes, 0 to 255.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct Keys([u64; 4]);

impl Keys {
    fn spot(vk: u32) -> Option<(usize, u64)> {
        let word = usize::try_from(vk / 64).ok().filter(|&word| word < 4)?;
        Some((word, 1 << (vk % 64)))
    }

    fn has(self, vk: u32) -> bool {
        Keys::spot(vk).is_some_and(|(word, bit)| self.0.get(word).is_some_and(|w| w & bit != 0))
    }

    fn set(&mut self, vk: u32, on: bool) {
        if let Some((word, bit)) = Keys::spot(vk)
            && let Some(w) = self.0.get_mut(word)
        {
            if on {
                *w |= bit;
            } else {
                *w &= !bit;
            }
        }
    }

    fn is_empty(self) -> bool {
        self.0 == [0; 4]
    }

    fn iter(self) -> impl Iterator<Item = u32> {
        (0..256).filter(move |&vk| self.has(vk))
    }
}

/// Where the hook is: keeping every key while the field records, or only the key-ups of the keys it kept after.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Recording,
    Draining,
}

/// What the hook knows, on its thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct State {
    phase: Phase,
    /// The keys whose key-down it kept and whose key-up hasn't come.
    kept: Keys,
    /// The keys already down when it was installed, as the click on the field with Shift held: they pass.
    entry: Keys,
    /// The modifiers held, the kept ones included, which Windows' own key state doesn't show.
    held: [bool; 8],
}

impl State {
    fn new(entry: Keys) -> State {
        let mut state = State {
            phase: Phase::Recording,
            kept: Keys::default(),
            entry,
            held: [false; 8],
        };
        for vk in entry.iter() {
            state.hold(vk, true);
        }
        state
    }

    /// Notes a modifier going down or up; the left and right keys of each are apart, so one coming up leaves the other.
    fn hold(&mut self, vk: u32, down: bool) {
        let side = match vk {
            0x5C | 0xA1 | 0xA3 | 0xA5 => 1,
            _ => 0,
        };
        let index = match modifier(vk) {
            Some(Modifier::Win) => 0,
            Some(Modifier::Ctrl) => 2,
            Some(Modifier::Alt) => 4,
            Some(Modifier::Shift) => 6,
            None => return,
        };
        if let Some(held) = self.held.get_mut(index + side) {
            *held = down;
        }
    }

    fn modifiers(&self) -> Modifiers {
        let [win_l, win_r, ctrl_l, ctrl_r, alt_l, alt_r, shift_l, shift_r] = self.held;
        Modifiers {
            win: win_l || win_r,
            ctrl: ctrl_l || ctrl_r,
            alt: alt_l || alt_r,
            shift: shift_l || shift_r,
        }
    }

    /// Whether the hook has nothing left to keep, and goes.
    fn drained(&self) -> bool {
        self.phase == Phase::Draining && self.kept.is_empty()
    }
}

/// What the hook does with one key event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Verdict {
    /// Keep it from Windows and every program.
    keep: bool,
    /// Tell the Settings window.
    report: bool,
}

/// Decides what happens to the key `vk` going down or up, and what the hook knows after.
fn decide(mut state: State, vk: u32, down: bool) -> (State, Verdict) {
    let mut verdict = Verdict {
        keep: false,
        report: false,
    };
    if vk > 255 {
        return (state, verdict);
    }
    let held_before = state.modifiers();
    state.hold(vk, down);
    if down {
        if state.entry.has(vk) {
            // A repeat of a key down since before the hook: it passes.
        } else if state.kept.has(vk) {
            // A repeat of a key kept, or, while draining, a key pressed again whose key-up Windows never delivered.
            verdict.keep = true;
        } else if state.phase == Phase::Recording {
            state.kept.set(vk, true);
            verdict = Verdict {
                keep: true,
                report: true,
            };
        }
    } else if state.kept.has(vk) {
        state.kept.set(vk, false);
        verdict.keep = true;
    } else {
        state.entry.set(vk, false);
    }
    // Recording, a modifier coming up changes what the field shows.
    if !down && state.phase == Phase::Recording && state.modifiers() != held_before {
        verdict.report = true;
    }
    (state, verdict)
}

/// The keys down now, from 8 up, so no mouse button.
fn keys_down() -> Keys {
    let mut keys = Keys::default();
    for vk in 8..=254 {
        // SAFETY: no pointers.
        let state = unsafe { GetAsyncKeyState(vk) };
        // The top bit says the key is down.
        if state < 0 {
            keys.set(vk.unsigned_abs(), true);
        }
    }
    keys
}

thread_local! {
    /// The hook's state, on its thread: the hook procedure is called on the thread that installed it.
    static STATE: Cell<Option<State>> = const { Cell::new(None) };
    /// The Settings window, and this thread's ID, where the hook procedure posts.
    static WINDOW: Cell<usize> = const { Cell::new(0) };
    static THREAD: Cell<u32> = const { Cell::new(0) };
    /// How many messages to the Settings window Windows refused, logged once the hook is removed.
    static LOST: Cell<u32> = const { Cell::new(0) };
}

/// The hook's thread: installs the hook, says so on `ready`, and runs until the keys are all up after recording, or
/// until asked to end.
fn run(
    window: usize,
    generation: usize,
    entry: Keys,
    ready: &mpsc::Sender<windows::core::Result<u32>>,
) {
    STATE.set(Some(State::new(entry)));
    WINDOW.set(window);
    // SAFETY: no arguments.
    THREAD.set(unsafe { GetCurrentThreadId() });
    let mut message = MSG::default();
    // Makes the thread's queue now, so the Settings window's first message to it can't fail for want of one.
    // SAFETY: `message` is a valid MSG to fill; no message has this number, so nothing is taken out.
    let _ = unsafe { PeekMessageW(&mut message, None, WM_APP, WM_APP, PM_NOREMOVE) };
    let installed = install();
    let hook = match installed {
        Ok(hook) => hook,
        Err(e) => {
            // Should the starter be gone, the failure is logged instead.
            if let Err(mpsc::SendError(Err(e))) = ready.send(Err(e)) {
                log::failure("keyboard hook: installing it", &e);
            }
            return;
        }
    };
    if ready.send(Ok(THREAD.get())).is_err() {
        // The starter is gone and nothing will end this thread: the hook goes now.
        drop(hook);
        return;
    }
    loop {
        // SAFETY: `message` is a valid MSG for the call to fill.
        match unsafe { GetMessageW(&mut message, None, 0, 0) }.0 {
            0 => break,
            -1 => {
                log::failure("keyboard hook: its message loop", &Error::from_thread());
                break;
            }
            _ => {}
        }
        if message.hwnd.is_invalid() && on_thread_message(message.message) {
            break;
        }
        // SAFETY: a message GetMessageW just returned. TranslateMessage's result only says whether a character
        // message was posted.
        unsafe {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    drop(hook);
    let lost = LOST.take();
    if lost > 0 {
        log::failure(
            "keyboard hook",
            &format!("{lost} key messages couldn't reach Settings"),
        );
    }
    post_to_window(ENDED, WPARAM(generation), LPARAM(0));
}

/// Handles a message to the thread itself, and says whether the loop ends.
fn on_thread_message(message: u32) -> bool {
    let Some(mut state) = STATE.get() else {
        return true;
    };
    match message {
        RECORD => state.phase = Phase::Recording,
        DRAIN => {
            state.phase = Phase::Draining;
            if state.drained() {
                post_done();
            }
        }
        // Checked again: a click on the field may have come between.
        DONE => return state.drained(),
        _ => {}
    }
    STATE.set(Some(state));
    false
}

/// The hook, removed when dropped, on every way out of the thread.
struct Hook(windows::Win32::UI::WindowsAndMessaging::HHOOK);

impl Drop for Hook {
    fn drop(&mut self) {
        // SAFETY: installed by this thread, and removed once.
        if let Err(e) = unsafe { UnhookWindowsHookEx(self.0) } {
            log::failure("keyboard hook: removing it", &e);
        }
    }
}

fn install() -> windows::core::Result<Hook> {
    // SAFETY: no module name, so this is the handle of ScreenGauge.exe itself.
    let module = unsafe { GetModuleHandleW(None) }?;
    // SAFETY: the procedure never unwinds, and this thread runs a message loop for it until the hook is removed.
    let hook =
        unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(procedure), Some(module.into()), 0) }?;
    Ok(Hook(hook))
}

/// The hook procedure. Fast, and never panics: no logging, no indexing, no unwrap.
extern "system" fn procedure(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == i32::try_from(HC_ACTION).unwrap_or(0)
        && let Some(state) = STATE.get()
    {
        let message = u32::try_from(wparam.0).unwrap_or(0);
        let down = matches!(message, WM_KEYDOWN | WM_SYSKEYDOWN);
        let up = matches!(message, WM_KEYUP | WM_SYSKEYUP);
        if down || up {
            // SAFETY: for HC_ACTION, a low-level keyboard hook's lParam points to a KBDLLHOOKSTRUCT valid during the call.
            let vk = unsafe { (*(lparam.0 as *const KBDLLHOOKSTRUCT)).vkCode };
            let (after, verdict) = decide(state, vk, down);
            STATE.set(Some(after));
            if verdict.report {
                post_to_window(
                    KEY,
                    WPARAM(usize::try_from(vk).unwrap_or(usize::MAX)),
                    KeyEvent::lparam(down, after.modifiers()),
                );
            }
            if after.drained() {
                post_done();
            }
            if verdict.keep {
                return LRESULT(1);
            }
        }
    }
    // SAFETY: passes the arguments on unchanged.
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

fn post_to_window(message: u32, wparam: WPARAM, lparam: LPARAM) {
    let window = HWND(std::ptr::with_exposed_provenance_mut(WINDOW.get()));
    // SAFETY: a window of this process, given by `start`; posting never waits for it.
    if unsafe { PostMessageW(Some(window), message, wparam, lparam) }.is_err() {
        LOST.set(LOST.get().saturating_add(1));
    }
}

fn post_done() {
    // SAFETY: this thread, whose queue exists.
    if unsafe { PostThreadMessageW(THREAD.get(), DONE, WPARAM(0), LPARAM(0)) }.is_err() {
        LOST.set(LOST.get().saturating_add(1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LWIN: u32 = 0x5B;
    const RWIN: u32 = 0x5C;
    const LCTRL: u32 = 0xA2;
    const LALT: u32 = 0xA4;
    const LSHIFT: u32 = 0xA0;
    const ESC: u32 = 0x1B;
    const ENTER: u32 = 0x0D;
    const J: u32 = 0x4A;
    const E: u32 = 0x45;

    /// Feeds key events through `decide` from `state`, and returns the state after and each event's verdict as
    /// (kept, reported).
    fn feed(mut state: State, events: &[(u32, bool)]) -> (State, Vec<(bool, bool)>) {
        let verdicts = events
            .iter()
            .map(|&(vk, down)| {
                let (after, verdict) = decide(state, vk, down);
                state = after;
                (verdict.keep, verdict.report)
            })
            .collect();
        (state, verdicts)
    }

    fn recording() -> State {
        State::new(Keys::default())
    }

    fn draining(mut state: State) -> State {
        state.phase = Phase::Draining;
        state
    }

    #[test]
    fn recording_keeps_every_key_and_reports_each_new_one() {
        let (state, verdicts) = feed(recording(), &[(LWIN, true), (LALT, true), (J, true)]);
        assert_eq!(verdicts, [(true, true); 3]);
        assert_eq!(
            state.modifiers(),
            Modifiers {
                win: true,
                alt: true,
                ..Modifiers::default()
            }
        );
        // A held key repeats: kept, and not reported again.
        let (_, verdicts) = feed(state, &[(LWIN, true), (J, true)]);
        assert_eq!(verdicts, [(true, false); 2]);
    }

    #[test]
    fn a_modifier_coming_up_is_kept_and_reported_and_a_main_key_coming_up_only_kept() {
        let (state, _) = feed(recording(), &[(LWIN, true), (J, true)]);
        let (state, verdicts) = feed(state, &[(J, false), (LWIN, false)]);
        assert_eq!(verdicts, [(true, false), (true, true)]);
        assert_eq!(state.modifiers(), Modifiers::default());
        assert!(state.kept.is_empty());
    }

    #[test]
    fn a_lone_win_is_kept_both_ways_so_the_start_menu_never_sees_it() {
        let (state, verdicts) = feed(recording(), &[(LWIN, true), (LWIN, false)]);
        assert_eq!(verdicts, [(true, true), (true, true)]);
        assert!(state.kept.is_empty());
    }

    #[test]
    fn draining_keeps_only_the_key_ups_of_the_keys_kept_and_ends_with_the_last() {
        // Win + Alt + J recorded; Win and Alt are still down when recording stops.
        let (state, _) = feed(
            recording(),
            &[(LWIN, true), (LALT, true), (J, true), (J, false)],
        );
        let state = draining(state);
        assert!(!state.drained());
        // A new key passes, and the field hears nothing more.
        let (state, verdicts) = feed(state, &[(E, true), (E, false)]);
        assert_eq!(verdicts, [(false, false); 2]);
        let (state, verdicts) = feed(state, &[(LALT, false)]);
        assert_eq!(verdicts, [(true, false)]);
        assert!(!state.drained());
        let (state, verdicts) = feed(state, &[(LWIN, false)]);
        assert_eq!(verdicts, [(true, false)]);
        assert!(state.drained());
        // Once drained, everything passes.
        let (_, verdicts) = feed(state, &[(LWIN, true), (LWIN, false)]);
        assert_eq!(verdicts, [(false, false); 2]);
    }

    #[test]
    fn a_key_up_windows_never_delivered_costs_one_press_of_that_key_then_the_hook_goes() {
        // Ctrl's key-up went to the secure desktop: the next Ctrl press is kept, then the hook can go.
        let (state, _) = feed(recording(), &[(LCTRL, true)]);
        let state = draining(state);
        let (state, verdicts) = feed(state, &[(LCTRL, true), (E, true), (E, false)]);
        assert_eq!(verdicts, [(true, false), (false, false), (false, false)]);
        let (state, _) = feed(state, &[(LCTRL, false)]);
        assert!(state.drained());
    }

    #[test]
    fn esc_is_kept_like_any_key_and_drains_on_its_key_up() {
        let (state, verdicts) = feed(recording(), &[(LSHIFT, true), (ESC, true)]);
        assert_eq!(verdicts, [(true, true); 2]);
        let state = draining(state);
        let (state, _) = feed(state, &[(ESC, false), (LSHIFT, false)]);
        assert!(state.drained());
    }

    #[test]
    fn keys_down_before_the_hook_pass_and_their_modifiers_count_as_held() {
        let mut entry = Keys::default();
        entry.set(ENTER, true);
        entry.set(LSHIFT, true);
        let state = State::new(entry);
        assert!(state.modifiers().shift);
        let (state, verdicts) = feed(state, &[(ENTER, true), (ENTER, false)]);
        assert_eq!(verdicts, [(false, false); 2]);
        let (state, verdicts) = feed(state, &[(LWIN, true), (J, true)]);
        assert_eq!(verdicts, [(true, true); 2]);
        assert_eq!(
            state.modifiers(),
            Modifiers {
                win: true,
                shift: true,
                ..Modifiers::default()
            }
        );
        // Shift comes up: it passes, since Windows saw it go down, and the field hears of it.
        let (state, verdicts) = feed(state, &[(LSHIFT, false)]);
        assert_eq!(verdicts, [(false, true)]);
        assert!(!state.modifiers().shift);
    }

    #[test]
    fn left_and_right_of_a_modifier_are_apart() {
        let (state, _) = feed(recording(), &[(LWIN, true), (RWIN, true), (LWIN, false)]);
        assert!(state.modifiers().win);
        let (state, _) = feed(state, &[(RWIN, false)]);
        assert!(!state.modifiers().win);
    }

    #[test]
    fn a_code_past_255_passes_untouched() {
        let (state, verdicts) = feed(recording(), &[(300, true), (300, false)]);
        assert_eq!(verdicts, [(false, false); 2]);
        assert_eq!(state, recording());
    }

    #[test]
    fn a_key_event_goes_to_settings_and_back_as_it_was() {
        let held = Modifiers {
            win: true,
            ctrl: false,
            alt: true,
            shift: true,
        };
        for down in [true, false] {
            let event = KeyEvent::of(WPARAM(0x4A), KeyEvent::lparam(down, held));
            assert_eq!(
                event,
                KeyEvent {
                    vk: 0x4A,
                    down,
                    held
                }
            );
        }
    }

    #[test]
    fn keys_are_told_apart_as_the_recorder_needs() {
        assert_eq!(pressed(0x41), Pressed::Main(Key::Letter('A')));
        assert_eq!(pressed(0x5A), Pressed::Main(Key::Letter('Z')));
        assert_eq!(pressed(0x30), Pressed::Main(Key::Digit(0)));
        assert_eq!(pressed(0x39), Pressed::Main(Key::Digit(9)));
        assert_eq!(pressed(0x70), Pressed::Main(Key::Function(1)));
        assert_eq!(pressed(0x7B), Pressed::Main(Key::Function(12)));
        assert_eq!(pressed(ESC), Pressed::Escape);
        for vk in [
            LWIN, RWIN, LCTRL, 0xA3, LALT, 0xA5, LSHIFT, 0xA1, 0x10, 0x11, 0x12,
        ] {
            assert_eq!(pressed(vk), Pressed::Modifier, "{vk:#x}");
        }
        // Space, Tab, F13, a number on the numeric keypad, and no key at all.
        for vk in [0x20, 0x09, 0x7C, 0x60, 300, u32::MAX] {
            assert_eq!(pressed(vk), Pressed::Other, "{vk:#x}");
        }
    }
}
