//! The parts of ScreenGauge that make no Windows call, so every rule they own is tested without a screen.
//!
//! Per the spec's Modules section, this crate is where the session core (everything between calling
//! ScreenGauge and closing the tools), the HDR conversion and the settings file live. The Windows layer
//! (`screengauge`) feeds them and carries out what they decide.

#![forbid(unsafe_code)]
