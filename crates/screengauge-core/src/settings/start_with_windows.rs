//! "Start with Windows" (v1 spec WPC-296, Settings; WPC-326): whether ScreenGauge starts when the user signs in to
//! Windows, as Settings' switch and the tray menu's check mark show it, and what a click on either does.
//!
//! Windows holds the answer, not the settings file: the Windows layer reads whether ScreenGauge is listed to start, and
//! what Windows keeps of the user's own choice in Task Manager or in Settings > Apps > Startup (ADR 0004). That choice
//! is never overridden: turned off there, the switch shows off, and a click on it only says where to turn it on.

/// What the switch and the check mark show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartWithWindows {
    Off,
    On,
    /// Listed to start, but the user turned it off in Windows: it shows off.
    TurnedOffInWindows,
}

/// What a click on the switch, or on the tray menu's item, does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Click {
    TurnOn,
    TurnOff,
    /// Nothing changes: the amber message says Windows turned it off, and where to turn it on.
    SayTurnedOffInWindows,
}

impl StartWithWindows {
    /// The state from whether ScreenGauge is `listed` to start, and what Windows keeps of the user's choice for it
    /// (`approved`), should it keep anything. Windows writes 02 for an entry left on and 03 with the time for one turned
    /// off; an odd first byte is read as off. Not listed, it is off whatever Windows kept: Windows shows the user's
    /// choice only for an entry that is listed, so turning it on lists it, where the user can then turn it on.
    pub fn of(listed: bool, approved: Option<&[u8]>) -> StartWithWindows {
        let turned_off = approved
            .and_then(|bytes| bytes.first())
            .is_some_and(|first| first & 1 == 1);
        match (listed, turned_off) {
            (false, _) => StartWithWindows::Off,
            (true, false) => StartWithWindows::On,
            (true, true) => StartWithWindows::TurnedOffInWindows,
        }
    }

    /// Whether the switch shows on, and the menu's item its check mark.
    pub fn is_on(self) -> bool {
        self == StartWithWindows::On
    }

    pub fn click(self) -> Click {
        match self {
            StartWithWindows::Off => Click::TurnOn,
            StartWithWindows::On => Click::TurnOff,
            StartWithWindows::TurnedOffInWindows => Click::SayTurnedOffInWindows,
        }
    }
}
