//! The names of the Snip's files (v1 spec WPC-296, Snip): the automatic one, from the local time the release came at,
//! and the `.png` every file ends with, whatever name Save As was given.

use std::path::{Path, PathBuf};

use super::LocalTime;

/// The automatic names of a Snip's file taken at one moment, in the order they are tried:
/// `screengauge-20261026-204112.png` first, then `-2`, `-3` and so on before `.png`, for when a file in the folder
/// already has the name. The date and time are local, to the second, and written as digits alone, so they never change
/// with the language of Windows. Which names are taken only the folder knows, so the Windows layer, which writes there,
/// goes down the list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileNames {
    time: LocalTime,
}

impl FileNames {
    pub fn at(time: LocalTime) -> FileNames {
        FileNames { time }
    }

    /// The name with no number: the one Save As suggests.
    pub fn first(self) -> String {
        self.numbered(1)
    }

    /// Every name in the order they are tried, the first one first, then `-2` up to `-4294967295`.
    pub fn iter(self) -> impl Iterator<Item = String> {
        (1..=u32::MAX).map(move |number| self.numbered(number))
    }

    fn numbered(self, number: u32) -> String {
        let LocalTime {
            year,
            month,
            day,
            hour,
            minute,
            second,
        } = self.time;
        let stem =
            format!("screengauge-{year:04}{month:02}{day:02}-{hour:02}{minute:02}{second:02}");
        match number {
            1 => format!("{stem}.{EXTENSION}"),
            _ => format!("{stem}-{number}.{EXTENSION}"),
        }
    }
}

/// The Snip's file is always a PNG (v1 spec WPC-296, Snip).
const EXTENSION: &str = "png";

/// `chosen`, the path Save As gave, ending with `.png`: a name with no extension gets it, and another extension is
/// replaced by it. One that already ends with `.png`, in any letter case, stays exactly as it was.
pub fn png_path(chosen: &Path) -> PathBuf {
    match chosen.extension() {
        Some(extension) if extension.eq_ignore_ascii_case(EXTENSION) => chosen.to_path_buf(),
        _ => chosen.with_extension(EXTENSION),
    }
}
