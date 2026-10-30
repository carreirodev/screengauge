//! The parts of ScreenGauge that make no Windows call, so every rule they own is tested without a screen.
//!
//! Per the Modules section of the v1 spec (Linear WPC-296), this crate is where the session core
//! (everything between calling ScreenGauge and closing the tools), the HDR conversion and the settings
//! file live. The Windows layer (`screengauge`) feeds them and carries out what they decide.

#![forbid(unsafe_code)]

pub mod session;
pub mod settings;
