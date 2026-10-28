//! Builder B. The `hotkey`, `hotkey-child` and `recorder` modes.
//!
//! - `hotkey`: registers Win+Ctrl+M, asks a second process whether it can tell the combination is taken, tries
//!   combinations Windows owns, and (with `--wait-secs`) logs presses and foreground changes.
//! - `hotkey-child`: the second process of that conflict test.
//! - `recorder`: the prototype of Pixmeter's Settings shortcut recorder (keys/recorder.rs).

mod combo;
mod foreground;
mod hotkey;
mod llhook;
mod recorder;
mod window;

use crate::common::Args;

pub fn hotkey_mode(args: &Args) -> Result<(), String> {
    hotkey::run(args)
}

pub fn hotkey_child_mode(args: &Args) -> Result<(), String> {
    hotkey::run_child(args)
}

pub fn recorder_mode(args: &Args) -> Result<(), String> {
    recorder::run(args)
}
