//! Key combinations: parsing `win+ctrl+m`, labels, and RegisterHotKey with Win32 error codes.

use std::fmt;

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyNameTextW, HOT_KEY_MODIFIERS, MAPVK_VK_TO_VSC_EX, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN,
    MapVirtualKeyW, RegisterHotKey, UnregisterHotKey,
};

use crate::log;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Combo {
    pub mods: HOT_KEY_MODIFIERS,
    pub vk: u32,
}

impl Combo {
    pub fn new(win: bool, ctrl: bool, alt: bool, shift: bool, vk: u32) -> Self {
        let mut bits = 0;
        for (on, flag) in [(win, MOD_WIN), (ctrl, MOD_CONTROL), (alt, MOD_ALT), (shift, MOD_SHIFT)] {
            if on {
                bits |= flag.0;
            }
        }
        Self { mods: HOT_KEY_MODIFIERS(bits), vk }
    }

    /// Parses `win+ctrl+shift+alt+<letter or digit>`; the modifiers are optional and unordered.
    pub fn parse(text: &str) -> Result<Self, String> {
        let (mut win, mut ctrl, mut alt, mut shift) = (false, false, false, false);
        let mut key: Option<u32> = None;
        for part in text.split('+') {
            let part = part.trim().to_ascii_lowercase();
            let flag = match part.as_str() {
                "win" => &mut win,
                "ctrl" | "control" => &mut ctrl,
                "alt" => &mut alt,
                "shift" => &mut shift,
                single => {
                    let mut chars = single.chars();
                    let (Some(c), None) = (chars.next(), chars.next()) else {
                        return Err(format!("'{text}': '{single}' is not win, ctrl, alt, shift or one letter/digit"));
                    };
                    if !c.is_ascii_alphanumeric() {
                        return Err(format!("'{text}': '{c}' is not a letter or digit"));
                    }
                    if key.replace(u32::from(c.to_ascii_uppercase())).is_some() {
                        return Err(format!("'{text}': more than one non-modifier key"));
                    }
                    continue;
                }
            };
            if *flag {
                return Err(format!("'{text}': '{part}' appears twice"));
            }
            *flag = true;
        }
        let vk = key.ok_or_else(|| format!("'{text}': no letter or digit key"))?;
        Ok(Self::new(win, ctrl, alt, shift, vk))
    }

    pub fn has_win(&self) -> bool {
        self.mods.0 & MOD_WIN.0 != 0
    }

    /// `Win+Ctrl+Alt+Shift+M`, in that order.
    pub fn label(&self) -> String {
        format!("{}{}", mods_label(self.mods), key_label(self.vk))
    }
}

impl fmt::Display for Combo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.label())
    }
}

/// `Win+Ctrl+` (with the trailing plus) for the set modifiers, empty for none.
pub fn mods_label(mods: HOT_KEY_MODIFIERS) -> String {
    let mut out = String::new();
    for (flag, name) in [(MOD_WIN, "Win+"), (MOD_CONTROL, "Ctrl+"), (MOD_ALT, "Alt+"), (MOD_SHIFT, "Shift+")] {
        if mods.0 & flag.0 != 0 {
            out.push_str(name);
        }
    }
    out
}

/// A readable name for a virtual-key code: the character for letters and digits, else the keyboard-layout name.
pub fn key_label(vk: u32) -> String {
    if matches!(vk, 0x30..=0x39 | 0x41..=0x5A)
        && let Ok(byte) = u8::try_from(vk)
    {
        return char::from(byte).to_string();
    }
    // SAFETY: plain lookup of the scan code for a virtual key.
    let scan_ex = unsafe { MapVirtualKeyW(vk, MAPVK_VK_TO_VSC_EX) };
    if scan_ex != 0 {
        let extended = if scan_ex >> 8 == 0xE0 { 1u32 << 24 } else { 0 };
        if let Ok(lparam) = i32::try_from(((scan_ex & 0xFF) << 16) | extended) {
            let mut buf = [0u16; 64];
            // SAFETY: writes at most `buf.len()` UTF-16 units into the buffer.
            let n = unsafe { GetKeyNameTextW(lparam, &mut buf) };
            if let Ok(n) = usize::try_from(n)
                && n > 0
                && n <= buf.len()
            {
                return String::from_utf16_lossy(&buf[..n]);
            }
        }
    }
    format!("VK 0x{vk:02X}")
}

/// A failed RegisterHotKey, with the Win32 code extracted from the HRESULT.
#[derive(Clone, Debug)]
pub struct RegError {
    /// Low 16 bits of an HRESULT of the form 0x8007xxxx; the raw HRESULT when it is some other kind.
    pub win32: u32,
    pub hresult: u32,
    pub message: String,
}

impl RegError {
    fn from_windows(e: &windows::core::Error) -> Self {
        let hresult = e.code().0.cast_unsigned();
        let win32 = if hresult >> 16 == 0x8007 { hresult & 0xFFFF } else { hresult };
        Self { win32, hresult, message: e.message() }
    }
}

impl fmt::Display for RegError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "error {} (HRESULT 0x{:08X}): {}", self.win32, self.hresult, self.message.trim())
    }
}

/// A registered hotkey. Unregisters on drop, so every exit path releases it.
pub struct Registration {
    hwnd: Option<HWND>,
    id: i32,
    label: String,
    live: bool,
}

/// Registers `combo` (always with MOD_NOREPEAT, like the real app would) for `hwnd` under `id`.
pub fn register(hwnd: Option<HWND>, id: i32, combo: Combo) -> Result<Registration, RegError> {
    let mods = HOT_KEY_MODIFIERS(combo.mods.0 | MOD_NOREPEAT.0);
    // SAFETY: plain call; `hwnd` is a window of this thread or None (then the hotkey belongs to the thread).
    unsafe { RegisterHotKey(hwnd, id, mods, combo.vk) }.map_err(|e| RegError::from_windows(&e))?;
    Ok(Registration { hwnd, id, label: combo.label(), live: true })
}

impl Registration {
    /// Unregisters now and reports a failure; the drop does nothing afterwards.
    pub fn unregister(mut self) -> Result<(), String> {
        self.live = false;
        // SAFETY: same window and id as the registration.
        unsafe { UnregisterHotKey(self.hwnd, self.id) }.map_err(|e| format!("UnregisterHotKey {}: {e}", self.label))
    }
}

impl Drop for Registration {
    fn drop(&mut self) {
        if !self.live {
            return;
        }
        // SAFETY: same window and id as the registration.
        if let Err(e) = unsafe { UnregisterHotKey(self.hwnd, self.id) } {
            log::line(format!("ERROR: UnregisterHotKey {} on cleanup: {e}", self.label));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_labels() -> Result<(), String> {
        let c = Combo::parse("win+ctrl+m")?;
        assert_eq!(c.label(), "Win+Ctrl+M");
        assert_eq!(c.vk, 0x4D);
        assert!(c.has_win());
        // Any order, any case, all four modifiers, digits.
        let c = Combo::parse("Shift+ALT+9+ctrl+Win")?;
        assert_eq!(c.label(), "Win+Ctrl+Alt+Shift+9");
        assert_eq!(c.vk, 0x39);
        assert!(!Combo::parse("ctrl+alt+k")?.has_win());
        Ok(())
    }

    #[test]
    fn rejects_bad_input() {
        for bad in ["", "win+ctrl", "win+m+n", "win+win+m", "win+f1", "win++m", "win+ctrl+!"] {
            assert!(Combo::parse(bad).is_err(), "'{bad}' should be rejected");
        }
    }

    #[test]
    fn win32_code_comes_from_the_hresult() {
        // ERROR_HOTKEY_ALREADY_REGISTERED (1409) as the HRESULT that windows-rs hands back: 0x80070581.
        let e = windows::core::Error::from_hresult(windows::core::HRESULT(0x8007_0581_u32.cast_signed()));
        let reg = RegError::from_windows(&e);
        assert_eq!(reg.win32, 1409);
        assert_eq!(reg.hresult, 0x8007_0581);
        // An HRESULT that is not a Win32 wrapper keeps its full value.
        let e = windows::core::Error::from_hresult(windows::core::HRESULT(0x8000_4005_u32.cast_signed()));
        assert_eq!(RegError::from_windows(&e).win32, 0x8000_4005);
    }
}
