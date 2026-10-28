//! The WH_KEYBOARD_LL hook of the recorder, on its own thread with its own message loop.
//!
//! The callback only updates a small state and sends messages over a channel. It never logs, never paints and
//! never unhooks: a console print can block (QuickEdit selection), and a hook callback that takes longer than the
//! system's LowLevelHooksTimeout gets the hook silently removed, which would let keys (and a lone Win press) leak
//! through. Unhooking happens on this thread after its message loop ends.

use std::cell::Cell;
use std::sync::OnceLock;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use windows::Win32::Foundation::{HINSTANCE, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_ESCAPE};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, HHOOK, KBDLLHOOKSTRUCT, LLKHF_EXTENDED, LLKHF_INJECTED, MSG,
    PM_NOREMOVE, PeekMessageW, PostQuitMessage, PostThreadMessageW, SetWindowsHookExW, TranslateMessage,
    UnhookWindowsHookEx, WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP, WM_QUIT, WM_SYSKEYDOWN, WM_SYSKEYUP, WM_USER,
};

use crate::common::Ctx;
use crate::log;

/// One bit per virtual-key code 0..=255.
pub type Bits = [u64; 4];

fn bit_get(bits: &Bits, vk: u32) -> bool {
    let Ok(word) = usize::try_from(vk / 64) else { return false };
    bits.get(word).is_some_and(|w| w & (1u64 << (vk % 64)) != 0)
}

fn bit_set(bits: &mut Bits, vk: u32) {
    if let Ok(word) = usize::try_from(vk / 64)
        && let Some(w) = bits.get_mut(word)
    {
        *w |= 1u64 << (vk % 64);
    }
}

fn bit_clear(bits: &mut Bits, vk: u32) {
    if let Ok(word) = usize::try_from(vk / 64)
        && let Some(w) = bits.get_mut(word)
    {
        *w &= !(1u64 << (vk % 64));
    }
}

fn bit_empty(bits: &Bits) -> bool {
    bits.iter().all(|w| *w == 0)
}

/// Modifier bits tracked by the hook (held modifier keys, left and right apart).
pub const WIN: u8 = 0b0000_0011;
pub const SHIFT: u8 = 0b0000_1100;
pub const CTRL: u8 = 0b0011_0000;
pub const ALT: u8 = 0b1100_0000;

/// Bit of a modifier key in the held mask. LL hooks report the left/right codes; the generic ones are mapped too.
fn mod_bit(vk: u32) -> Option<u8> {
    let index = match vk {
        0x5B => 0,        // VK_LWIN
        0x5C => 1,        // VK_RWIN
        0xA0 | 0x10 => 2, // VK_LSHIFT, VK_SHIFT
        0xA1 => 3,        // VK_RSHIFT
        0xA2 | 0x11 => 4, // VK_LCONTROL, VK_CONTROL
        0xA3 => 5,        // VK_RCONTROL
        0xA4 | 0x12 => 6, // VK_LMENU, VK_MENU
        0xA5 => 7,        // VK_RMENU
        _ => return None,
    };
    Some(1 << index)
}

/// Virtual-key codes (8..=254, so no mouse buttons) that are physically down right now.
pub fn down_keys() -> Vec<u32> {
    (8u32..=254)
        .filter(|vk| {
            i32::try_from(*vk)
                // SAFETY: plain query of the asynchronous key state.
                .is_ok_and(|code| unsafe { GetAsyncKeyState(code) }.cast_unsigned() & 0x8000 != 0)
        })
        .collect()
}

pub fn to_bits(keys: &[u32]) -> Bits {
    let mut bits = [0u64; 4];
    for vk in keys {
        bit_set(&mut bits, *vk);
    }
    bits
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// Swallowing everything, waiting for the first non-modifier key-down (or Esc).
    Recording,
    /// Combination decided; still swallowing until every swallowed key has been released.
    Draining,
    /// Passing everything through; the thread is about to remove the hook.
    Done,
}

#[derive(Clone, Copy)]
struct HookState {
    phase: Phase,
    /// Keys whose key-down was swallowed and whose key-up has not come yet.
    swallowed: Bits,
    /// Keys already down when the hook was installed: their events pass through.
    entry: Bits,
    /// Held modifiers (WIN/SHIFT/CTRL/ALT bits), tracked here because swallowed keys don't update GetAsyncKeyState.
    held: u8,
}

const INITIAL: HookState = HookState { phase: Phase::Recording, swallowed: [0; 4], entry: [0; 4], held: 0 };

thread_local! {
    static STATE: Cell<HookState> = const { Cell::new(INITIAL) };
}

static TX: OnceLock<Sender<HookMsg>> = OnceLock::new();

/// One key event as the hook saw it.
pub struct KeyNote {
    pub t_ms: f64,
    pub vk: u32,
    pub scan: u32,
    pub down: bool,
    pub injected: bool,
    pub extended: bool,
    pub swallowed: bool,
    pub note: &'static str,
    /// Held modifiers after this event.
    pub held: u8,
}

/// The combination, decided by the first non-modifier key-down.
#[derive(Clone, Copy)]
pub struct Recorded {
    pub t_ms: f64,
    pub held: u8,
    pub vk: u32,
}

pub enum HookMsg {
    Installed { thread_id: u32 },
    Key(KeyNote),
    Recorded(Recorded),
    Cancelled { t_ms: f64 },
    /// Every swallowed key has been released; the hook now passes everything through and is being removed.
    Drained { t_ms: f64 },
    /// Last message of a thread that ended normally: the hook is removed.
    Finished,
    Failed(String),
}

fn emit(msg: HookMsg) {
    if let Some(tx) = TX.get()
        && tx.send(msg).is_err()
    {
        // The receiver is gone only when the main thread is already shutting down; nobody is left to tell.
    }
}

/// The running hook thread, seen from the main thread. Stops and joins it on drop.
pub struct HookHandle {
    thread_id: u32,
    join: Option<JoinHandle<()>>,
    pub rx: Receiver<HookMsg>,
    /// Set by the owner when it has seen `HookMsg::Finished`.
    pub finished: bool,
}

impl HookHandle {
    /// Starts the thread and waits until the hook is installed. `entry` holds the keys already down.
    pub fn start(entry: Bits) -> Result<Self, String> {
        let (tx, rx) = mpsc::channel();
        let join = thread::Builder::new().name("keyboard-hook".into()).spawn(move || hook_thread(tx, entry)).ctx("spawn hook thread")?;
        match rx.recv_timeout(Duration::from_secs(3)) {
            Ok(HookMsg::Installed { thread_id }) => Ok(Self { thread_id, join: Some(join), rx, finished: false }),
            Ok(HookMsg::Failed(e)) => {
                join_logged(join);
                Err(e)
            }
            Ok(_) => Err("keyboard hook thread sent something other than 'installed' first".to_string()),
            Err(e) => Err(format!("keyboard hook thread did not report within 3 s: {e}")),
        }
    }

    /// Asks the thread to leave its message loop; it removes the hook on its way out.
    pub fn request_stop(&self) -> Result<(), String> {
        // SAFETY: posts WM_QUIT to a thread of this process that created its message queue before reporting.
        unsafe { PostThreadMessageW(self.thread_id, WM_QUIT, WPARAM(0), LPARAM(0)) }.ctx("PostThreadMessageW(WM_QUIT) to the hook thread")
    }

    /// True when the thread has ended (normally or not).
    pub fn thread_ended(&self) -> bool {
        self.join.as_ref().is_none_or(JoinHandle::is_finished)
    }

    /// Waits for the thread to end.
    pub fn join(&mut self) {
        if let Some(join) = self.join.take() {
            join_logged(join);
        }
    }
}

impl Drop for HookHandle {
    fn drop(&mut self) {
        if self.join.is_none() {
            return;
        }
        if !self.finished
            && let Err(e) = self.request_stop()
        {
            log::line(format!("ERROR: {e}"));
        }
        self.join();
    }
}

fn join_logged(join: JoinHandle<()>) {
    if join.join().is_err() {
        log::line("ERROR: the keyboard hook thread panicked");
    }
}

/// Removes the hook when dropped, on every exit path of the thread.
struct HookGuard(HHOOK);

impl Drop for HookGuard {
    fn drop(&mut self) {
        // SAFETY: the hook was installed by this thread and is removed once.
        if let Err(e) = unsafe { UnhookWindowsHookEx(self.0) } {
            emit(HookMsg::Failed(format!("UnhookWindowsHookEx: {e}")));
        }
    }
}

fn hook_thread(tx: Sender<HookMsg>, entry: Bits) {
    if let Err(tx) = TX.set(tx) {
        if tx.send(HookMsg::Failed("a keyboard hook was already started in this process".into())).is_err() {
            // The starter stopped waiting; there is no one to tell.
        }
        return;
    }
    STATE.set(HookState { entry, ..INITIAL });

    // Creates this thread's message queue now, so PostThreadMessageW from the main thread cannot fail for lack of it.
    let mut msg = MSG::default();
    // SAFETY: peeks an unused message number; `msg` is a valid out-parameter. The result is irrelevant here.
    _ = unsafe { PeekMessageW(&mut msg, None, WM_USER, WM_USER, PM_NOREMOVE) };

    // SAFETY: asks for the handle of the running executable.
    let module = match unsafe { GetModuleHandleW(None) } {
        Ok(m) => m,
        Err(e) => return emit(HookMsg::Failed(format!("GetModuleHandleW: {e}"))),
    };
    // SAFETY: `ll_keyboard` is an `extern "system"` fn that does not unwind; the thread pumps messages below.
    let hook = match unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(ll_keyboard), Some(HINSTANCE(module.0)), 0) } {
        Ok(h) => h,
        Err(e) => return emit(HookMsg::Failed(format!("SetWindowsHookExW(WH_KEYBOARD_LL): {e}"))),
    };
    let guard = HookGuard(hook);
    // SAFETY: plain query.
    emit(HookMsg::Installed { thread_id: unsafe { GetCurrentThreadId() } });

    loop {
        // SAFETY: `msg` is a valid out-parameter; messages belong to this thread.
        match unsafe { GetMessageW(&mut msg, None, 0, 0) }.0 {
            0 => break, // WM_QUIT
            -1 => {
                emit(HookMsg::Failed(format!("GetMessageW: {}", windows::core::Error::from_thread())));
                break;
            }
            _ => {
                // SAFETY: `msg` came from GetMessageW on this thread.
                unsafe {
                    // Only says whether a character message was generated; not an error code.
                    _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
        }
    }
    drop(guard);
    emit(HookMsg::Finished);
}

/// The hook callback. Must stay fast and must not panic: no logging, no indexing, no unwrap.
unsafe extern "system" fn ll_keyboard(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // A negative code means "pass it on without looking" (HC_ACTION is 0).
    if code == 0 {
        // SAFETY: for HC_ACTION in a WH_KEYBOARD_LL hook, lparam points to a KBDLLHOOKSTRUCT valid during the call.
        let info = unsafe { std::ptr::with_exposed_provenance::<KBDLLHOOKSTRUCT>(lparam.0.cast_unsigned()).as_ref() };
        if let Some(info) = info {
            // A message number that does not fit u32 is not a key message; 0 makes both flags below false.
            let message = u32::try_from(wparam.0).unwrap_or(0);
            let down = matches!(message, WM_KEYDOWN | WM_SYSKEYDOWN);
            let up = matches!(message, WM_KEYUP | WM_SYSKEYUP);
            if (down || up) && decide(info, down) {
                return LRESULT(1);
            }
        }
    }
    // SAFETY: passes the unchanged arguments down the hook chain.
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

/// Returns true when the event must be swallowed.
fn decide(info: &KBDLLHOOKSTRUCT, down: bool) -> bool {
    let vk = info.vkCode;
    let t_ms = log::now_ms();
    let mut st = STATE.get();
    let mut swallow = false;
    let mut report = true;
    let mut note = "";
    let mut follow = None;

    if vk > 255 {
        note = "pass: virtual-key code out of range";
    } else if st.phase == Phase::Done {
        note = "pass: recording is over";
    } else if down {
        if bit_get(&st.entry, vk) {
            note = "pass: key was already down when recording started";
        } else if bit_get(&st.swallowed, vk) {
            // Auto-repeat of a key already swallowed: swallow it too, without a log line each time.
            swallow = true;
            report = false;
        } else {
            swallow = true;
            bit_set(&mut st.swallowed, vk);
            if let Some(bit) = mod_bit(vk) {
                st.held |= bit;
                note = "modifier swallowed";
            } else if st.phase == Phase::Recording {
                st.phase = Phase::Draining;
                if vk == u32::from(VK_ESCAPE.0) {
                    note = "Esc swallowed: cancel";
                    follow = Some(HookMsg::Cancelled { t_ms });
                } else {
                    note = "key swallowed: completes the combination";
                    follow = Some(HookMsg::Recorded(Recorded { t_ms, held: st.held, vk }));
                }
            } else {
                note = "swallowed while draining (not recorded)";
            }
        }
    } else if bit_get(&st.swallowed, vk) {
        bit_clear(&mut st.swallowed, vk);
        swallow = true;
        if let Some(bit) = mod_bit(vk) {
            st.held &= !bit;
        }
        note = "swallowed (its key-down was swallowed)";
    } else if bit_get(&st.entry, vk) {
        bit_clear(&mut st.entry, vk);
        note = "pass: key was already down when recording started";
    } else {
        note = "pass: key-up whose key-down this hook never saw";
    }

    let drained = st.phase == Phase::Draining && bit_empty(&st.swallowed);
    if drained {
        st.phase = Phase::Done;
    }
    STATE.set(st);

    if report {
        emit(HookMsg::Key(KeyNote {
            t_ms,
            vk,
            scan: info.scanCode,
            down,
            injected: info.flags.0 & LLKHF_INJECTED.0 != 0,
            extended: info.flags.0 & LLKHF_EXTENDED.0 != 0,
            swallowed: swallow,
            note,
            held: st.held,
        }));
    }
    if let Some(msg) = follow {
        emit(msg);
    }
    if drained {
        emit(HookMsg::Drained { t_ms });
        // Ends this thread's message loop; the hook is removed there, not inside this callback.
        // SAFETY: plain call on the thread that runs the callback.
        unsafe { PostQuitMessage(0) };
    }
    swallow
}

#[cfg(test)]
mod tests {
    use super::*;

    const LWIN: u32 = 0x5B;
    const LCTRL: u32 = 0xA2;
    const LSHIFT: u32 = 0xA0;
    const ENTER: u32 = 0x0D;
    const ESC: u32 = 0x1B;
    const M: u32 = 0x4D;
    const E: u32 = 0x45;
    const LWIN_BIT: u8 = 1 << 0;
    const LCTRL_BIT: u8 = 1 << 4;

    fn event(vk: u32) -> KBDLLHOOKSTRUCT {
        KBDLLHOOKSTRUCT { vkCode: vk, ..Default::default() }
    }

    /// Feeds (vk, is_down) events through the decision function; returns whether each was swallowed.
    fn feed(events: &[(u32, bool)]) -> Vec<bool> {
        events.iter().map(|(vk, down)| decide(&event(*vk), *down)).collect()
    }

    fn reset(entry: &[u32]) {
        STATE.set(HookState { entry: to_bits(entry), ..INITIAL });
    }

    #[derive(Debug, PartialEq)]
    enum Seen {
        Recorded { held: u8, vk: u32 },
        Cancelled,
        Drained,
    }

    fn seen(rx: &Receiver<HookMsg>) -> Vec<Seen> {
        rx.try_iter()
            .filter_map(|m| match m {
                HookMsg::Recorded(r) => Some(Seen::Recorded { held: r.held, vk: r.vk }),
                HookMsg::Cancelled { .. } => Some(Seen::Cancelled),
                HookMsg::Drained { .. } => Some(Seen::Drained),
                _ => None,
            })
            .collect()
    }

    // One test function: the hook sends over a process-wide channel, so the scenarios run one after another.
    #[test]
    fn hook_state_machine() {
        let (tx, rx) = mpsc::channel();
        assert!(TX.set(tx).is_ok());

        // Win+Ctrl+M: everything swallowed, recorded on M, hook drains only after the last key-up.
        reset(&[]);
        let swallowed = feed(&[(LWIN, true), (LCTRL, true), (M, true), (M, false), (LCTRL, false)]);
        assert_eq!(swallowed, [true; 5]);
        assert_eq!(seen(&rx), [Seen::Recorded { held: LWIN_BIT | LCTRL_BIT, vk: M }]);
        assert_eq!(feed(&[(LWIN, false)]), [true]);
        assert_eq!(seen(&rx), [Seen::Drained]);
        // Once drained, everything passes through.
        assert_eq!(feed(&[(LWIN, true), (LWIN, false), (E, true), (E, false)]), [false; 4]);
        assert!(seen(&rx).is_empty());

        // Win released before the key: its key-up is swallowed too; still one Drained, at the very end.
        reset(&[]);
        feed(&[(LWIN, true), (LCTRL, true), (M, true)]);
        assert_eq!(seen(&rx), [Seen::Recorded { held: LWIN_BIT | LCTRL_BIT, vk: M }]);
        assert_eq!(feed(&[(LWIN, false), (M, false)]), [true, true]);
        assert!(seen(&rx).is_empty());
        assert_eq!(feed(&[(LCTRL, false)]), [true]);
        assert_eq!(seen(&rx), [Seen::Drained]);

        // A lone Win press is swallowed and records nothing (so Start cannot open).
        reset(&[]);
        assert_eq!(feed(&[(LWIN, true), (LWIN, false)]), [true, true]);
        assert!(seen(&rx).is_empty());

        // Esc cancels, then drains on its key-up; held modifiers are swallowed along.
        reset(&[]);
        assert_eq!(feed(&[(LSHIFT, true), (ESC, true)]), [true, true]);
        assert_eq!(seen(&rx), [Seen::Cancelled]);
        assert_eq!(feed(&[(ESC, false), (LSHIFT, false)]), [true, true]);
        assert_eq!(seen(&rx), [Seen::Drained]);

        // Auto-repeat of a swallowed key is swallowed and does not record twice.
        reset(&[]);
        assert_eq!(feed(&[(LWIN, true), (LWIN, true), (LWIN, true), (M, true), (M, true)]), [true; 5]);
        assert_eq!(seen(&rx), [Seen::Recorded { held: LWIN_BIT, vk: M }]);
        assert_eq!(feed(&[(M, false), (LWIN, false)]), [true, true]);
        assert_eq!(seen(&rx), [Seen::Drained]);

        // A second non-modifier while draining is swallowed, not recorded, and must be released before Done.
        reset(&[]);
        feed(&[(LWIN, true), (M, true), (E, true)]);
        assert_eq!(seen(&rx), [Seen::Recorded { held: LWIN_BIT, vk: M }]);
        feed(&[(M, false), (LWIN, false)]);
        assert!(seen(&rx).is_empty());
        feed(&[(E, false)]);
        assert_eq!(seen(&rx), [Seen::Drained]);

        // Enter held when the hook started (it launched the program): its repeats and key-up pass through.
        reset(&[ENTER]);
        assert_eq!(feed(&[(ENTER, true), (ENTER, true), (ENTER, false)]), [false; 3]);
        assert!(seen(&rx).is_empty());
        // A key-up whose key-down the hook never saw passes through as well.
        assert_eq!(feed(&[(E, false)]), [false]);
        assert!(seen(&rx).is_empty());
        // And recording still works afterwards.
        feed(&[(LWIN, true), (M, true)]);
        assert_eq!(seen(&rx), [Seen::Recorded { held: LWIN_BIT, vk: M }]);
        feed(&[(M, false), (LWIN, false)]);
        assert_eq!(seen(&rx), [Seen::Drained]);

        // Out-of-range codes pass through untouched.
        reset(&[]);
        assert_eq!(feed(&[(300, true), (300, false)]), [false, false]);
    }
}
