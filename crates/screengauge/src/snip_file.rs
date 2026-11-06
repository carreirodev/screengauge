//! The Snip's PNG file on disk (v1 spec WPC-296, Snip): a new file in the default folder, under the first automatic
//! name no file there has, or the path Save As chose. A file is written whole or not at all: whatever fails, a deleted
//! folder, a full disk, no permission or a network folder gone, no file is left behind, and no folder is ever made.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use screengauge_core::session::SaveTo;

/// Writes `png`, the bytes of the Snip's PNG file, where `to` says, and returns the file's path.
pub fn write(to: &SaveTo, png: &[u8]) -> io::Result<PathBuf> {
    match to {
        SaveTo::DefaultFolder { folder, names } => {
            for name in names.iter() {
                let path = folder.as_path().join(name);
                // Opening it as a new file takes the name in the same step as finding it free, so an earlier file is
                // never replaced, even one another program writes at that very moment.
                match OpenOptions::new().write(true).create_new(true).open(&path) {
                    Ok(file) => return fill(file, &path, png).map(|()| path),
                    Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
                    Err(e) => return Err(e),
                }
            }
            Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "every automatic name for the file is taken",
            ))
        }
        SaveTo::Chosen(path) => replace(path, png).map(|()| path.clone()),
    }
}

/// Writes `png` to `file`, just made at `path`, and makes sure it is on disk; should that fail, the file goes.
fn fill(mut file: File, path: &Path, png: &[u8]) -> io::Result<()> {
    let filled = file.write_all(png).and_then(|()| file.sync_all());
    drop(file);
    filled.map_err(|e| removing(path, e))
}

/// Writes `png` at `path` in one step, replacing the file there: the bytes go to a temporary file beside it first, and
/// renaming it over `path` happens whole or not at all, so a full disk never leaves the file Windows asked to replace
/// half written, or gone.
fn replace(path: &Path, png: &[u8]) -> io::Result<()> {
    let temporary = temporary_beside(path)?;
    // One left by a crash between the write and the rename is written over.
    let file = File::create(&temporary)?;
    fill(file, &temporary, png)?;
    fs::rename(&temporary, path).map_err(|e| removing(&temporary, e))
}

/// Where a file chosen in Save As is written before it takes its place: `name.png.screengauge-tmp`, beside it.
fn temporary_beside(path: &Path) -> io::Result<PathBuf> {
    let mut name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "the path names no file"))?
        .to_os_string();
    name.push(".screengauge-tmp");
    Ok(path.with_file_name(name))
}

/// `failure`, after removing the file it left at `path`. Should the file stay, the error says so too.
fn removing(path: &Path, failure: io::Error) -> io::Error {
    match fs::remove_file(path) {
        Ok(()) => failure,
        Err(cleanup) if cleanup.kind() == io::ErrorKind::NotFound => failure,
        Err(cleanup) => io::Error::new(
            failure.kind(),
            format!("{failure}; removing the unfinished file failed too: {cleanup}"),
        ),
    }
}

#[cfg(test)]
mod tests {
    use screengauge_core::session::{FileNames, LocalTime};
    use screengauge_core::settings::Folder;

    use super::*;
    use crate::test_folder::{TestFolder, make_read_only};

    const EVENING: LocalTime = LocalTime {
        year: 2026,
        month: 10,
        day: 26,
        hour: 20,
        minute: 41,
        second: 12,
    };

    fn in_folder(folder: &TestFolder) -> SaveTo {
        SaveTo::DefaultFolder {
            folder: Folder::new(&folder.0).expect("an absolute path"),
            names: FileNames::at(EVENING),
        }
    }

    #[test]
    fn the_default_folder_gets_the_file_under_the_automatic_name() {
        let folder = TestFolder::made("snip-file-default");
        let path = write(&in_folder(&folder), b"first").expect("the file is written");
        assert_eq!(path, folder.file("screengauge-20261026-204112.png"));
        assert_eq!(fs::read(&path).expect("the file reads back"), b"first");
    }

    #[test]
    fn a_taken_name_gets_2_then_3_and_the_earlier_files_stay_as_they_were() {
        let folder = TestFolder::made("snip-file-numbered");
        for bytes in [b"first", b"secnd", b"third"] {
            write(&in_folder(&folder), bytes).expect("the file is written");
        }
        assert_eq!(
            folder.names(),
            [
                "screengauge-20261026-204112-2.png",
                "screengauge-20261026-204112-3.png",
                "screengauge-20261026-204112.png",
            ]
        );
        let read = |name| fs::read(folder.file(name)).expect("the file reads back");
        assert_eq!(read("screengauge-20261026-204112.png"), b"first");
        assert_eq!(read("screengauge-20261026-204112-2.png"), b"secnd");
        assert_eq!(read("screengauge-20261026-204112-3.png"), b"third");
    }

    #[test]
    fn a_default_folder_that_no_longer_exists_fails_and_is_never_made_again() {
        let folder = TestFolder::new("snip-file-deleted");
        let error = write(&in_folder(&folder), b"first").expect_err("there is no folder");
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
        assert!(!folder.0.exists());
    }

    #[test]
    fn save_as_writes_exactly_the_path_chosen_and_leaves_no_temporary_file() {
        let folder = TestFolder::made("snip-file-chosen");
        let chosen = folder.file("login button.png");
        let path = write(&SaveTo::Chosen(chosen.clone()), b"first").expect("the file is written");
        assert_eq!(path, chosen);
        assert_eq!(folder.names(), ["login button.png"]);
        assert_eq!(fs::read(&chosen).expect("the file reads back"), b"first");
    }

    #[test]
    fn save_as_replaces_the_file_windows_asked_about() {
        let folder = TestFolder::made("snip-file-replaced");
        let chosen = folder.file("button.png");
        fs::write(&chosen, b"older").expect("an earlier file");
        write(&SaveTo::Chosen(chosen.clone()), b"newer").expect("the file is written");
        assert_eq!(folder.names(), ["button.png"]);
        assert_eq!(fs::read(&chosen).expect("the file reads back"), b"newer");
    }

    #[test]
    fn save_as_in_a_folder_that_no_longer_exists_fails_with_no_file() {
        let folder = TestFolder::new("snip-file-chosen-deleted");
        let error = write(&SaveTo::Chosen(folder.file("button.png")), b"first")
            .expect_err("there is no folder");
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
        assert!(!folder.0.exists());
    }

    #[test]
    fn a_file_that_cant_be_replaced_stays_as_it_was_with_no_temporary_file() {
        let folder = TestFolder::made("snip-file-read-only");
        let chosen = folder.file("button.png");
        fs::write(&chosen, b"older").expect("an earlier file");
        make_read_only(&chosen);
        let error = write(&SaveTo::Chosen(chosen.clone()), b"newer")
            .expect_err("a read-only file isn't replaced");
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
        assert_eq!(folder.names(), ["button.png"]);
        assert_eq!(fs::read(&chosen).expect("the file reads back"), b"older");
    }
}
