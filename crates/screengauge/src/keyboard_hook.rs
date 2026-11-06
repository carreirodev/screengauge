//! The keyboard hook of Settings' global shortcut recorder (WPC-323): while the field records, a low-level keyboard
//! hook keeps every key from Windows and the other programs, so a combination with Win records without the Start menu
//! opening, and tells the Settings window which key went down or up. It is how PowerToys records shortcuts, and how
//! WPC-281's hardware test recorded `Win + Alt + J` (`prototype/hardware-test-keys`, `keys/llhook.rs`).
//!
//! Every key the hook sees go down is either kept (Windows never sees it) or passed (Windows has it down), and its
//! key-up goes the same way, whatever happens to the recording in between: a key Windows saw go down always comes up
//! in Windows, and one it never saw go down never comes up there. A key down before the recording passes; while the
//! field records, every new key is kept; once recording stops, a new key passes, and the hook goes when the last key it
//! kept is up. A Win that passed comes up behind a key that is no key (`0xFF`, as PowerToys' remapping does), so
//! Windows never takes it for a lone Win and opens the Start menu.
//!
//! Windows' own key state doesn't show a kept key, so a key-up Windows never delivers, as when the screen locks or
//! Ctrl + Alt + Del switches to the secure desktop, can't be read back: the hook forgets every key when the desktop
//! switches, so none stays kept after.
//!
//! The hook runs on a thread of its own, at a raised priority, whose message loop nothing else can hold up: Windows
//! removes a hook that takes longer than its timeout to answer, silently. Each recording installs the hook afresh, so a
//! hook removed that way never lasts past one recording; the Settings window sees keys reach it while it records and
//! stops recording should that happen. The hook procedure only decides, and posts what it saw to the Settings window;
//! it never logs, paints or waits.

use std::cell::Cell;
use std::sync::mpsc;
use std::thread::{self, JoinHandle};

use screengauge_core::settings::{Key, Modifiers, Pressed};
use windows::Win32::Foundation::{E_FAIL, ERROR_INVALID_THREAD_ID, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::{
    GetCurrentThread, GetCurrentThreadId, SetThreadPriority, THREAD_PRIORITY_TIME_CRITICAL,
};
use windows::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT,
    KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, SendInput, VIRTUAL_KEY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, EVENT_SYSTEM_DESKTOPSWITCH, GetMessageW, HC_ACTION, HHOOK,
    KBDLLHOOKSTRUCT, MSG, PM_NOREMOVE, PeekMessageW, PostMessageW, PostThreadMessageW,
    SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx, WH_KEYBOARD_LL,
    WINEVENT_OUTOFCONTEXT, WM_APP, WM_KEYDOWN, WM_KEYUP, WM_QUIT, WM_SYSKEYDOWN, WM_SYSKEYUP,
};
use windows::core::Error;

use crate::app_message::{HOOK_DONE, HOOK_DRAIN, HOOK_ENDED, HOOK_KEY, HOOK_RECORD};
use crate::log;
use crate::win32::struct_size;

/// The virtual-key code no key has, which a Win that passed comes up behind.
pub const NO_KEY: u32 = 0xFF;

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
        let window = window.0.expose_provenance();
        let (ready, installed) = mpsc::channel();
        let thread = thread::Builder::new()
            .name("keyboard hook".to_owned())
            .spawn(move || run(window, generation, &ready))
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

    /// Records again, after `drain`: a click on the field before the keys of the last recording were all up. The
    /// hook is installed afresh.
    pub fn record(&self) -> windows::core::Result<()> {
        self.post(HOOK_RECORD)
    }

    /// Recording stopped: the hook goes once the keys it kept are up.
    pub fn drain(&self) -> windows::core::Result<()> {
        self.post(HOOK_DRAIN)
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
        // An ended thread has nothing to end: it is only joined. (Its ID stays its own while the handle is held, so
        // posting would only fail.)
        if thread.is_finished() {
            join(thread);
            return;
        }
        // SAFETY: as in `post`.
        match unsafe { PostThreadMessageW(self.thread_id, WM_QUIT, WPARAM(0), LPARAM(0)) } {
            // The thread ended since, after the keys were all up.
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
    if is_modifier(vk) {
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

/// Whether `vk` is Win, Ctrl, Alt or Shift: either side, or the generic codes a window's own key messages use.
pub fn is_modifier(vk: u32) -> bool {
    matches!(
        vk,
        0x10 | 0x11 | 0x12 | 0x5B | 0x5C | 0xA0 | 0xA1 | 0xA2 | 0xA3 | 0xA4 | 0xA5
    )
}

const LWIN: u32 = 0x5B;
const RWIN: u32 = 0x5C;

/// The generic Shift, Ctrl and Alt, which Windows' key state reports along with the side that is down: they are never
/// a key of their own, and a low-level hook never sees them.
const GENERIC: [u32; 3] = [0x10, 0x11, 0x12];

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

    fn without(self, other: Keys) -> Keys {
        let mut words = self.0;
        for (word, &out) in words.iter_mut().zip(&other.0) {
            *word &= !out;
        }
        Keys(words)
    }

    fn any(self, vks: &[u32]) -> bool {
        vks.iter().any(|&vk| self.has(vk))
    }
}

/// Where the hook is: keeping every new key while the field records, or passing them once it stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Recording,
    Draining,
}

/// What the hook knows, on its thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct State {
    phase: Phase,
    /// The keys whose key-down it kept and whose key-up hasn't come: their key-up is kept too.
    kept: Keys,
    /// The keys Windows has down: down before the recording, or pressed after it stopped. Their key-up passes.
    passed: Keys,
}

impl State {
    /// Recording, with `down` the keys Windows has down.
    fn recording(down: Keys) -> State {
        let mut state = State {
            phase: Phase::Draining,
            kept: Keys::default(),
            passed: Keys::default(),
        };
        state.record(down);
        state
    }

    /// Records (again), with `down` the keys Windows has down now. A key kept from before stays kept; any other key
    /// Windows has down passes, and only those: a key whose key-up passed, or was lost, is gone from the set.
    fn record(&mut self, down: Keys) {
        self.phase = Phase::Recording;
        self.passed = down.without(self.kept);
    }

    /// Every key forgotten, as the desktop switched: their key-ups go elsewhere, or come as key-ups of keys Windows
    /// has up, which pass.
    fn forget(&mut self) {
        self.kept = Keys::default();
        self.passed = Keys::default();
    }

    /// The modifiers held, kept or passed, left and right apart.
    fn modifiers(&self) -> Modifiers {
        let held = |vks: &[u32]| self.kept.any(vks) || self.passed.any(vks);
        Modifiers {
            win: held(&[LWIN, RWIN]),
            ctrl: held(&[0xA2, 0xA3]),
            alt: held(&[0xA4, 0xA5]),
            shift: held(&[0xA0, 0xA1]),
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
    /// A Win that passed coming up: keep it, and send no key and then the same key-up in its place.
    mask: bool,
}

const PASS: Verdict = Verdict {
    keep: false,
    report: false,
    mask: false,
};

/// Decides what happens to the key `vk` going down or up, and what the hook knows after.
fn decide(mut state: State, vk: u32, down: bool) -> (State, Verdict) {
    if vk > 255 || vk == NO_KEY {
        return (state, PASS);
    }
    let held_before = state.modifiers();
    let mut verdict = PASS;
    if down {
        if state.kept.has(vk) {
            // A repeat of a key kept.
            verdict.keep = true;
        } else if state.passed.has(vk) {
            // A repeat of a key Windows has down.
        } else if state.phase == Phase::Recording {
            state.kept.set(vk, true);
            verdict.keep = true;
            verdict.report = true;
        } else {
            state.passed.set(vk, true);
        }
    } else if state.kept.has(vk) {
        state.kept.set(vk, false);
        verdict.keep = true;
    } else if state.passed.has(vk) {
        state.passed.set(vk, false);
        verdict.mask = vk == LWIN || vk == RWIN;
    }
    // Recording, a modifier coming up changes what the field shows.
    if !down && state.phase == Phase::Recording && state.modifiers() != held_before {
        verdict.report = true;
    }
    (state, verdict)
}

/// The keys Windows has down now, from 8 up, so no mouse button, and not the generic Shift, Ctrl and Alt.
fn keys_down() -> Keys {
    let mut keys = Keys::default();
    for vk in (8..=254).filter(|vk| !GENERIC.contains(vk)) {
        // SAFETY: no pointers. The code is below 255, which an i32 holds.
        let state = unsafe { GetAsyncKeyState(i32::try_from(vk).unwrap_or(0)) };
        // The top bit says the key is down.
        if state < 0 {
            keys.set(vk, true);
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
    /// How many messages and keys Windows refused, logged once the hook is removed.
    static LOST: Cell<u32> = const { Cell::new(0) };
}

/// The hook's thread: installs the hook, says so on `ready`, and runs until the keys are all up after recording, or
/// until asked to end.
fn run(window: usize, generation: usize, ready: &mpsc::Sender<windows::core::Result<u32>>) {
    WINDOW.set(window);
    // SAFETY: no arguments.
    THREAD.set(unsafe { GetCurrentThreadId() });
    // Windows calls the hook on this thread for every key, in every program: it must answer before anything else.
    // SAFETY: this thread's own pseudo-handle.
    if let Err(e) = unsafe { SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_TIME_CRITICAL) }
    {
        log::failure("keyboard hook: raising its thread's priority", &e);
    }
    let mut message = MSG::default();
    // Makes the thread's queue now, so the Settings window's first message to it can't fail for want of one.
    // SAFETY: `message` is a valid MSG to fill. WM_APP itself is no message of ours (`app_message` starts at WM_APP +
    // 1), so nothing is taken out.
    let _ = unsafe { PeekMessageW(&mut message, None, WM_APP, WM_APP, PM_NOREMOVE) };
    let mut hook = match Hook::install() {
        Ok(hook) => hook,
        Err(e) => {
            // Should the starter be gone, the failure is logged instead.
            if let Err(mpsc::SendError(Err(e))) = ready.send(Err(e)) {
                log::failure("keyboard hook: installing it", &e);
            }
            return;
        }
    };
    // Read once the hook is in place, before the first key reaches it: a key Windows has down now passed before the
    // hook, and any other comes to the hook first.
    STATE.set(Some(State::recording(keys_down())));
    let switches = DesktopSwitches::watch();
    if ready.send(Ok(THREAD.get())).is_err() {
        // The starter is gone and nothing will end this thread: the hook goes now.
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
        if message.hwnd.is_invalid() && on_thread_message(message.message, &mut hook) {
            break;
        }
        // SAFETY: a message GetMessageW just returned. TranslateMessage's result only says whether a character
        // message was posted.
        unsafe {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    drop(switches);
    drop(hook);
    let lost = LOST.take();
    if lost > 0 {
        log::failure(
            "keyboard hook",
            &format!("Windows refused {lost} of its messages or keys"),
        );
    }
    post_to_window(HOOK_ENDED, WPARAM(generation), LPARAM(0));
}

/// Handles a message to the thread itself, and says whether the loop ends.
fn on_thread_message(message: u32, hook: &mut Hook) -> bool {
    let Some(mut state) = STATE.get() else {
        return true;
    };
    match message {
        HOOK_RECORD => {
            // A hook Windows removed for being slow is never told so: a fresh one replaces it for each recording.
            drop(std::mem::replace(
                hook,
                match Hook::install() {
                    Ok(fresh) => fresh,
                    Err(e) => {
                        log::failure("keyboard hook: installing it again", &e);
                        return true;
                    }
                },
            ));
            state.record(keys_down());
        }
        HOOK_DRAIN => {
            state.phase = Phase::Draining;
            if state.drained() {
                post_done();
            }
        }
        // Checked again: a click on the field may have come between.
        HOOK_DONE => return state.drained(),
        _ => {}
    }
    STATE.set(Some(state));
    false
}

/// The hook, removed when dropped, on every way out of the thread.
struct Hook(HHOOK);

impl Hook {
    fn install() -> windows::core::Result<Hook> {
        // SAFETY: no module name, so this is the handle of ScreenGauge.exe itself.
        let module = unsafe { GetModuleHandleW(None) }?;
        // SAFETY: the procedure never unwinds, and this thread runs a message loop for it until the hook is removed.
        let hook =
            unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(procedure), Some(module.into()), 0) }?;
        Ok(Hook(hook))
    }
}

impl Drop for Hook {
    fn drop(&mut self) {
        // SAFETY: installed by this thread, and removed once.
        if let Err(e) = unsafe { UnhookWindowsHookEx(self.0) } {
            log::failure("keyboard hook: removing it", &e);
        }
    }
}

/// Windows telling this thread the desktop switched, as when the screen locks or Ctrl + Alt + Del shows: the keys
/// held then come up on the other desktop, out of the hook's sight.
struct DesktopSwitches(Option<HWINEVENTHOOK>);

impl DesktopSwitches {
    fn watch() -> DesktopSwitches {
        // SAFETY: the callback never unwinds; WINEVENT_OUTOFCONTEXT calls it on this thread, from its message loop.
        let hook = unsafe {
            SetWinEventHook(
                EVENT_SYSTEM_DESKTOPSWITCH,
                EVENT_SYSTEM_DESKTOPSWITCH,
                None,
                Some(on_desktop_switch),
                0,
                0,
                WINEVENT_OUTOFCONTEXT,
            )
        };
        // SetWinEventHook sets no error code to read. Without it a lost key-up costs one later press of that key.
        if hook.is_invalid() {
            log::failure(
                "keyboard hook",
                &"SetWinEventHook refused to watch desktop switches",
            );
            return DesktopSwitches(None);
        }
        DesktopSwitches(Some(hook))
    }
}

impl Drop for DesktopSwitches {
    fn drop(&mut self) {
        if let Some(hook) = self.0
            // SAFETY: set by this thread, and removed once.
            && !unsafe { UnhookWinEvent(hook) }.as_bool()
        {
            log::failure("keyboard hook", &"UnhookWinEvent failed");
        }
    }
}

extern "system" fn on_desktop_switch(
    _hook: HWINEVENTHOOK,
    _event: u32,
    _hwnd: HWND,
    _object: i32,
    _child: i32,
    _thread: u32,
    _time: u32,
) {
    if let Some(mut state) = STATE.get() {
        state.forget();
        STATE.set(Some(state));
        if state.drained() {
            post_done();
        }
    }
}

/// The hook procedure. Fast, and never panics: no logging, no indexing, no unwrap.
extern "system" fn procedure(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == i32::try_from(HC_ACTION).unwrap_or(0)
        && let Some(state) = STATE.get()
    {
        let message = u32::try_from(wparam.0).unwrap_or(0);
        let down = matches!(message, WM_KEYDOWN | WM_SYSKEYDOWN);
        let up = matches!(message, WM_KEYUP | WM_SYSKEYUP);
        let info = std::ptr::with_exposed_provenance::<KBDLLHOOKSTRUCT>(lparam.0.cast_unsigned());
        if (down || up)
            // SAFETY: for HC_ACTION, a low-level keyboard hook's lParam points to a KBDLLHOOKSTRUCT valid during the
            // call.
            && let Some(info) = unsafe { info.as_ref() }
        {
            let (after, verdict) = decide(state, info.vkCode, down);
            STATE.set(Some(after));
            if verdict.report {
                post_to_window(
                    HOOK_KEY,
                    WPARAM(usize::try_from(info.vkCode).unwrap_or(usize::MAX)),
                    KeyEvent::lparam(down, after.modifiers()),
                );
            }
            if after.drained() {
                post_done();
            }
            if verdict.keep || (verdict.mask && mask(info)) {
                return LRESULT(1);
            }
        }
    }
    // SAFETY: passes the arguments on unchanged.
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

/// Sends no key, down and up, and then `up` itself again: the Win coming up then follows another key, so Windows
/// doesn't open the Start menu. False when Windows took none of it, so the real key-up passes instead.
fn mask(up: &KBDLLHOOKSTRUCT) -> bool {
    let key = |vk: u32, flags: KEYBD_EVENT_FLAGS, scan: u32| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(u16::try_from(vk).unwrap_or(0)),
                wScan: u16::try_from(scan).unwrap_or(0),
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    let inputs = [
        key(NO_KEY, KEYBD_EVENT_FLAGS(0), 0),
        key(NO_KEY, KEYEVENTF_KEYUP, 0),
        key(
            up.vkCode,
            KEYEVENTF_KEYUP | KEYEVENTF_EXTENDEDKEY,
            up.scanCode,
        ),
    ];
    let size = i32::try_from(struct_size::<INPUT>()).unwrap_or(i32::MAX);
    // SAFETY: three keyboard inputs, fully set, of the size given.
    let sent = unsafe { SendInput(&inputs, size) };
    match sent {
        3 => true,
        // The no key went but not the key-up: the real one must pass, or Win stays down in Windows.
        _ => {
            LOST.set(LOST.get().saturating_add(1));
            false
        }
    }
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
    if unsafe { PostThreadMessageW(THREAD.get(), HOOK_DONE, WPARAM(0), LPARAM(0)) }.is_err() {
        LOST.set(LOST.get().saturating_add(1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LCTRL: u32 = 0xA2;
    const LALT: u32 = 0xA4;
    const LSHIFT: u32 = 0xA0;
    const RSHIFT: u32 = 0xA1;
    const ESC: u32 = 0x1B;
    const ENTER: u32 = 0x0D;
    const J: u32 = 0x4A;
    const E: u32 = 0x45;
    const L: u32 = 0x4C;

    /// What happened to one key event: kept from Windows, told to the field, and, for a Win coming up, masked.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Seen {
        /// Windows got it.
        Passed,
        Kept,
        /// Kept, and told to the field.
        KeptTold,
        /// Windows got it, and the field was told (a modifier coming up).
        PassedTold,
        /// A Win that passed coming up behind no key.
        Masked,
    }

    fn seen(verdict: Verdict) -> Seen {
        match (verdict.keep, verdict.report, verdict.mask) {
            (_, _, true) => Seen::Masked,
            (true, true, _) => Seen::KeptTold,
            (true, false, _) => Seen::Kept,
            (false, true, _) => Seen::PassedTold,
            (false, false, _) => Seen::Passed,
        }
    }

    fn feed(mut state: State, events: &[(u32, bool)]) -> (State, Vec<Seen>) {
        let seen = events
            .iter()
            .map(|&(vk, down)| {
                let (after, verdict) = decide(state, vk, down);
                state = after;
                seen(verdict)
            })
            .collect();
        (state, seen)
    }

    fn keys(vks: &[u32]) -> Keys {
        let mut keys = Keys::default();
        for &vk in vks {
            keys.set(vk, true);
        }
        keys
    }

    fn recording() -> State {
        State::recording(Keys::default())
    }

    fn draining(mut state: State) -> State {
        state.phase = Phase::Draining;
        state
    }

    /// Windows' view of the keys after `events`, from `windows_had` down: a key is down there when its key-down got
    /// through and its key-up didn't, which the invariant forbids once every key is up.
    fn windows_has_down(windows_had: &[u32], events: &[(u32, bool)], seen: &[Seen]) -> Vec<u32> {
        let mut down = keys(windows_had);
        for (&(vk, is_down), &what) in events.iter().zip(seen) {
            if matches!(what, Seen::Passed | Seen::PassedTold | Seen::Masked) {
                down.set(vk, is_down);
            }
        }
        (0..256).filter(|&vk| down.has(vk)).collect()
    }

    #[test]
    fn recording_keeps_every_new_key_and_tells_the_field() {
        let (state, seen) = feed(recording(), &[(LWIN, true), (LALT, true), (J, true)]);
        assert_eq!(seen, [Seen::KeptTold; 3]);
        assert_eq!(
            state.modifiers(),
            Modifiers {
                win: true,
                alt: true,
                ..Modifiers::default()
            }
        );
        // A held key repeats: kept, and not told again.
        let (_, seen) = feed(state, &[(LWIN, true), (J, true)]);
        assert_eq!(seen, [Seen::Kept; 2]);
    }

    #[test]
    fn a_lone_win_is_kept_both_ways_so_the_start_menu_never_sees_it() {
        let (state, seen) = feed(recording(), &[(LWIN, true), (LWIN, false)]);
        assert_eq!(seen, [Seen::KeptTold, Seen::KeptTold]);
        assert!(state.kept.is_empty());
    }

    #[test]
    fn draining_passes_new_keys_and_ends_with_the_last_key_kept() {
        let (state, _) = feed(
            recording(),
            &[(LWIN, true), (LALT, true), (J, true), (J, false)],
        );
        let state = draining(state);
        let (state, seen) = feed(state, &[(E, true), (E, false), (LALT, false)]);
        assert_eq!(seen, [Seen::Passed, Seen::Passed, Seen::Kept]);
        assert!(!state.drained());
        let (state, seen) = feed(state, &[(LWIN, false)]);
        assert_eq!(seen, [Seen::Kept]);
        assert!(state.drained());
    }

    #[test]
    fn a_key_pressed_while_draining_comes_up_in_windows_even_if_the_field_records_again() {
        // The field recorded Win + Alt + J with Alt still held, so the hook drains. Win goes down: Windows gets it.
        // The field is clicked again, Win repeats, J comes and goes, and Win comes up: Windows must see it come up.
        let (state, _) = feed(recording(), &[(LALT, true), (J, true), (J, false)]);
        let mut state = draining(state);
        let events_before = [(LWIN, true)];
        let (after, seen_before) = feed(state, &events_before);
        state = after;
        state.record(keys(&[LWIN]));
        let events = [
            (LWIN, true),
            (J, true),
            (J, false),
            (LWIN, false),
            (LALT, false),
        ];
        let (state, seen) = feed(state, &events);
        assert_eq!(
            seen,
            [
                Seen::Passed,
                Seen::KeptTold,
                Seen::Kept,
                Seen::Masked,
                Seen::KeptTold
            ]
        );
        assert!(state.kept.is_empty() && state.passed.is_empty());
        let all_events: Vec<(u32, bool)> = events_before.iter().chain(&events).copied().collect();
        let all_seen: Vec<Seen> = seen_before.iter().chain(&seen).copied().collect();
        assert_eq!(
            windows_has_down(&[], &all_events, &all_seen),
            Vec::<u32>::new()
        );
    }

    #[test]
    fn shift_held_across_the_click_comes_up_in_windows() {
        // Shift is down in Windows as the field starts recording: its repeats and its key-up pass, the rest is kept.
        let state = State::recording(keys(&[LSHIFT]));
        assert!(state.modifiers().shift);
        let events = [
            (LSHIFT, true),
            (LWIN, true),
            (J, true),
            (LSHIFT, false),
            (J, false),
            (LWIN, false),
        ];
        let (state, seen) = feed(state, &events);
        assert_eq!(
            seen,
            [
                Seen::Passed,
                Seen::KeptTold,
                Seen::KeptTold,
                Seen::PassedTold,
                Seen::Kept,
                Seen::KeptTold
            ]
        );
        assert!(!state.modifiers().shift);
        assert_eq!(
            windows_has_down(&[LSHIFT], &events, &seen),
            Vec::<u32>::new()
        );
    }

    #[test]
    fn a_key_that_went_down_before_the_hook_was_read_passes_both_ways() {
        // Windows' key state is read once the hook is in place: a key it shows down passed before the hook.
        let (state, seen) = feed(State::recording(keys(&[E])), &[(E, true), (E, false)]);
        assert_eq!(seen, [Seen::Passed, Seen::Passed]);
        assert!(state.passed.is_empty());
        // A key-up the hook never saw the key-down of passes too.
        let (_, seen) = feed(recording(), &[(ENTER, false)]);
        assert_eq!(seen, [Seen::Passed]);
    }

    #[test]
    fn a_win_held_before_the_click_comes_up_behind_no_key_so_the_start_menu_stays_shut() {
        let state = State::recording(keys(&[LWIN]));
        let events = [(LWIN, true), (J, true), (J, false), (LWIN, false)];
        let (_, seen) = feed(state, &events);
        assert_eq!(
            seen,
            [Seen::Passed, Seen::KeptTold, Seen::Kept, Seen::Masked]
        );
    }

    #[test]
    fn recording_again_drops_keys_windows_no_longer_has_down() {
        // Shift passed while draining, and its key-up was lost: recording again reads Windows' key state, which has it
        // up, so Shift isn't held any more, and a new Shift is kept.
        let (state, _) = feed(draining(recording()), &[(LSHIFT, true)]);
        assert!(state.modifiers().shift);
        let mut state = state;
        state.record(Keys::default());
        assert!(!state.modifiers().shift);
        let (_, seen) = feed(state, &[(LSHIFT, true)]);
        assert_eq!(seen, [Seen::KeptTold]);
    }

    #[test]
    fn the_desktop_switching_forgets_every_key_so_none_stays_kept() {
        // Win + L locks the screen: the key-ups go to the lock screen, and the hook would keep the next Win and L.
        let (state, _) = feed(recording(), &[(LWIN, true), (L, true)]);
        let mut state = draining(state);
        assert!(!state.drained());
        state.forget();
        assert!(state.drained());
        // Should the key-ups come after all, they pass: Windows has those keys up.
        let (_, seen) = feed(state, &[(L, false), (LWIN, false)]);
        assert_eq!(seen, [Seen::Passed, Seen::Passed]);
    }

    #[test]
    fn the_generic_shift_ctrl_and_alt_are_never_held() {
        // Windows' key state shows the generic Shift with the right one; the right one coming up leaves nothing held.
        let state = State::recording(keys(&[RSHIFT]));
        assert!(state.modifiers().shift);
        let (state, _) = feed(state, &[(RSHIFT, false)]);
        assert!(!state.modifiers().shift);
        // Were the generic Shift read with it, nothing would ever take it out of the keys Windows has down.
        let state = State::recording(keys(&[RSHIFT, 0x10]));
        let (state, _) = feed(state, &[(RSHIFT, false)]);
        assert!(
            !state.passed.is_empty(),
            "the generic code is why `keys_down` skips it"
        );
    }

    #[test]
    fn esc_is_kept_like_any_key_and_drains_on_its_key_up() {
        let (state, seen) = feed(recording(), &[(LSHIFT, true), (ESC, true)]);
        assert_eq!(seen, [Seen::KeptTold; 2]);
        let (state, _) = feed(draining(state), &[(ESC, false), (LSHIFT, false)]);
        assert!(state.drained());
    }

    #[test]
    fn left_and_right_of_a_modifier_are_apart() {
        let (state, _) = feed(recording(), &[(LWIN, true), (RWIN, true), (LWIN, false)]);
        assert!(state.modifiers().win);
        let (state, _) = feed(state, &[(RWIN, false)]);
        assert!(!state.modifiers().win);
        assert!(
            !feed(recording(), &[(LCTRL, true), (LCTRL, false)])
                .0
                .modifiers()
                .ctrl
        );
    }

    #[test]
    fn no_key_and_codes_past_255_pass_untouched() {
        let (state, seen) = feed(
            recording(),
            &[(NO_KEY, true), (NO_KEY, false), (300, true), (300, false)],
        );
        assert_eq!(seen, [Seen::Passed; 4]);
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
            LWIN, RWIN, LCTRL, 0xA3, LALT, 0xA5, LSHIFT, RSHIFT, 0x10, 0x11, 0x12,
        ] {
            assert_eq!(pressed(vk), Pressed::Modifier, "{vk:#x}");
        }
        // Space, Tab, F13, a number on the numeric keypad, and no key at all.
        for vk in [0x20, 0x09, 0x7C, 0x60, 300, u32::MAX] {
            assert_eq!(pressed(vk), Pressed::Other, "{vk:#x}");
        }
    }
}
