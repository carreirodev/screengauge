//! Failures no caller can act on go to the debugger output (DebugView shows it). There is no log file:
//! nothing of the user's is written to disk without a click.

use windows::Win32::System::Diagnostics::Debug::OutputDebugStringW;
use windows::core::HSTRING;

pub fn line(text: &str) {
    let text = HSTRING::from(format!("ScreenGauge: {text}\n"));
    // SAFETY: a valid null-terminated string that outlives the call.
    unsafe { OutputDebugStringW(&text) };
}
