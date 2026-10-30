//! What the session core gives the Windows layer: the scene to draw and the effects to carry out.

use std::path::PathBuf;

use super::{Image, Photo};
use crate::settings::{Folder, Remembered};

/// What the Windows layer draws while the tools are open.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Scene<'a> {
    /// Every monitor's photo, drawn pixel for pixel over its monitor, in the frozen screen's order. Nothing marks the
    /// screen as frozen.
    pub frozen_screen: &'a [Photo],
    /// The cursor over the frozen screen.
    pub cursor: Cursor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cursor {
    /// Windows' thin cross.
    Cross,
}

/// Something the Windows layer does for the session, in the order the session returns them.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    /// Put this text on the clipboard.
    CopyText(String),
    /// Put this image on the clipboard.
    CopyImage(Image),
    /// Write this image as a PNG file at `path`.
    WritePng { path: PathBuf, image: Image },
    /// Open the Save As window in `folder` (`None` is Pictures), suggesting `file_name`. Its closing comes back as
    /// [`Input::SaveAsClosed`](super::Input::SaveAsClosed).
    OpenSaveAs {
        folder: Option<Folder>,
        file_name: String,
    },
    /// Open the Settings window.
    OpenSettings,
    /// Close the tools: the frozen screen goes, its photos are erased, and the mouse goes back to the programs. The
    /// session takes no more events.
    Close,
    /// Keep this remembered state in the settings file.
    SaveRemembered(Remembered),
}
