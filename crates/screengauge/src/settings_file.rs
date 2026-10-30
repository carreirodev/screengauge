//! The settings file on disk (v1 spec WPC-296, "Arquivo de configurações e estado lembrado"): opened when ScreenGauge
//! starts, created with the defaults the first time so there is a file to edit, and replaced in one step whenever it
//! is saved. What the file holds and how its text reads belongs to the core (`screengauge_core::settings`).

use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;

use screengauge_core::settings::{self, Damaged, Reset, Stored};
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::UI::Shell::{FOLDERID_LocalAppData, KF_FLAG_DEFAULT, SHGetKnownFolderPath};

use crate::log;

const FILE_NAME: &str = "settings.ini";
/// What a save writes before it replaces the file.
const TEMPORARY_NAME: &str = "settings.ini.tmp";

/// `%LOCALAPPDATA%\ScreenGauge`, the installer copy's folder. Local rather than roaming, because what the file
/// remembers (folders, the toolbar's place) belongs to this computer. The Store copy's folder is its package's own
/// (WPC-334), so the two copies never share a file.
pub fn installer_copy_folder() -> windows::core::Result<PathBuf> {
    // SAFETY: a known folder ID, default flags and no token, meaning the signed-in user.
    let path = unsafe { SHGetKnownFolderPath(&FOLDERID_LocalAppData, KF_FLAG_DEFAULT, None) }?;
    // SAFETY: on success the string is valid and null-terminated until it is freed below.
    let local_app_data = PathBuf::from(OsString::from_wide(unsafe { path.as_wide() }));
    // SAFETY: SHGetKnownFolderPath allocated the string with the COM allocator; it is freed once, here.
    unsafe { CoTaskMemFree(Some(path.0.cast_const().cast())) };
    Ok(local_app_data.join("ScreenGauge"))
}

/// Opens the settings file as ScreenGauge starts. Until Windows notifications exist (WPC-303), a damaged file or a
/// value that went back to its default reaches only the debugger output. When the file can't be opened, ScreenGauge
/// runs with the defaults and has no file to save to, so it never writes over a file it couldn't read.
pub fn open_at_startup() -> (Option<SettingsFile>, Stored) {
    let folder = match installer_copy_folder() {
        Ok(folder) => folder,
        Err(e) => {
            log::failure("settings: finding the user's data folder", &e);
            return (None, Stored::default());
        }
    };
    match SettingsFile::open(folder) {
        Ok((file, stored, opened)) => {
            match opened {
                Opened::New => {}
                Opened::Read(resets) => {
                    for reset in &resets {
                        log::failure("settings", reset);
                    }
                }
                Opened::Damaged { damage, copy } => log::failure(
                    &format!(
                        "settings: back to the defaults, damaged file kept at {}",
                        copy.display()
                    ),
                    &damage,
                ),
            }
            (Some(file), stored)
        }
        Err(e) => {
            log::failure("settings: opening the file", &e);
            (None, Stored::default())
        }
    }
}

/// A settings file that was opened, so saving may replace it.
pub struct SettingsFile {
    folder: PathBuf,
}

/// What opening the settings file found.
#[derive(Debug)]
pub enum Opened {
    /// No file yet: one with the defaults was written.
    New,
    /// The file was read. Each reset is a value that was wrong and went back to its default.
    Read(Vec<Reset>),
    /// The file was damaged. It was kept at `copy`, and a file with the defaults took its place: the remembered state
    /// went back to its defaults too.
    Damaged { damage: Damaged, copy: PathBuf },
}

impl SettingsFile {
    /// Opens `settings.ini` in `folder`. An error leaves the folder as it was.
    pub fn open(folder: PathBuf) -> io::Result<(SettingsFile, Stored, Opened)> {
        let file = SettingsFile { folder };
        let bytes = match fs::read(file.path()) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                let stored = Stored::default();
                file.save(&stored)?;
                return Ok((file, stored, Opened::New));
            }
            Err(e) => return Err(e),
        };
        match settings::read(&bytes) {
            Ok(read) => Ok((file, read.stored, Opened::Read(read.resets))),
            Err(damage) => {
                // Kept before anything is written, so a failure leaves the damaged file where it was.
                let copy = file.keep_damaged(&bytes)?;
                let stored = Stored::default();
                file.save(&stored)?;
                Ok((file, stored, Opened::Damaged { damage, copy }))
            }
        }
    }

    /// Replaces the file in one step: the new text goes to a temporary file first, and renaming it over the old one
    /// either happens whole or not at all, so a crash or a full disk never leaves half a file.
    pub fn save(&self, stored: &Stored) -> io::Result<()> {
        fs::create_dir_all(&self.folder)?;
        let temporary = self.folder.join(TEMPORARY_NAME);
        let mut file = File::create(&temporary)?;
        file.write_all(settings::write(stored).as_bytes())?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, self.path())
    }

    fn path(&self) -> PathBuf {
        self.folder.join(FILE_NAME)
    }

    /// Writes a damaged file's bytes next to it, as `settings.damaged.ini`, or `settings.damaged-2.ini` and so on
    /// when an earlier copy is there, so no copy is ever replaced.
    fn keep_damaged(&self, bytes: &[u8]) -> io::Result<PathBuf> {
        for number in 1..=u32::MAX {
            let name = match number {
                1 => String::from("settings.damaged.ini"),
                _ => format!("settings.damaged-{number}.ini"),
            };
            let copy = self.folder.join(name);
            match OpenOptions::new().write(true).create_new(true).open(&copy) {
                Ok(mut file) => {
                    file.write_all(bytes)?;
                    file.sync_all()?;
                    return Ok(copy);
                }
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(e),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "every name for the damaged file's copy is taken",
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use screengauge_core::settings::{self as core, Theme, Tool};

    use super::*;

    /// A folder of its own per test, which doesn't exist yet and is removed when the test ends.
    struct TestFolder(PathBuf);

    impl TestFolder {
        fn new(test: &str) -> TestFolder {
            let path = std::env::temp_dir()
                .join(format!("ScreenGauge.Test.{}.{test}", std::process::id()));
            remove(&path);
            TestFolder(path)
        }

        fn file(&self, name: &str) -> PathBuf {
            self.0.join(name)
        }

        fn names(&self) -> Vec<String> {
            let mut names: Vec<String> = fs::read_dir(&self.0)
                .expect("the folder exists")
                .map(|entry| {
                    entry
                        .expect("a folder entry")
                        .file_name()
                        .into_string()
                        .expect("a Unicode name")
                })
                .collect();
            names.sort();
            names
        }
    }

    impl Drop for TestFolder {
        fn drop(&mut self) {
            remove(&self.0);
        }
    }

    /// A folder left in %TEMP% harms nothing, so a failure is only reported.
    fn remove(path: &Path) {
        match fs::remove_dir_all(path) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => {
                eprintln!("couldn't remove {}: {e}", path.display());
            }
            _ => {}
        }
    }

    fn open(folder: &TestFolder) -> (SettingsFile, Stored, Opened) {
        SettingsFile::open(folder.0.clone()).expect("the settings file opens")
    }

    #[test]
    fn a_missing_file_is_created_with_the_defaults() {
        let folder = TestFolder::new("missing");
        let (_, stored, opened) = open(&folder);
        assert!(matches!(opened, Opened::New), "{opened:?}");
        assert_eq!(stored, Stored::default());

        let written = fs::read(folder.file("settings.ini")).expect("the file was written");
        assert_eq!(
            core::read(&written).map(|read| read.stored),
            Ok(Stored::default())
        );
    }

    #[test]
    fn saved_settings_and_remembered_state_are_read_back() {
        let folder = TestFolder::new("saved");
        let (file, _, _) = open(&folder);
        let mut changed = Stored::default();
        changed.options.theme = Theme::Dark;
        changed.remembered.last_tool = Tool::Snip;
        changed.remembered.warned_about_other_copy = true;
        file.save(&changed).expect("the file is saved");

        let (_, stored, opened) = open(&folder);
        assert!(
            matches!(&opened, Opened::Read(resets) if resets.is_empty()),
            "{opened:?}"
        );
        assert_eq!(stored, changed);
        assert_eq!(
            folder.names(),
            ["settings.ini"],
            "no temporary file is left"
        );
    }

    #[test]
    fn a_damaged_file_is_kept_and_the_defaults_take_its_place() {
        let folder = TestFolder::new("damaged");
        let damaged = b"last_tool = Snip\nwarned_about_other_copy = true\nnot a setting\n";
        fs::create_dir_all(&folder.0).expect("the folder is created");
        fs::write(folder.file("settings.ini"), damaged).expect("the damaged file is written");

        let (_, stored, opened) = open(&folder);
        let Opened::Damaged { damage, copy } = opened else {
            panic!("not damaged: {opened:?}");
        };
        assert_eq!(damage, Damaged::NotNameValue { line: 3 });
        assert_eq!(copy, folder.file("settings.damaged.ini"));
        assert_eq!(fs::read(&copy).expect("the copy exists"), damaged);
        // The remembered state went back to its defaults with the rest.
        assert_eq!(stored, Stored::default());

        let (_, stored, opened) = open(&folder);
        assert!(
            matches!(&opened, Opened::Read(resets) if resets.is_empty()),
            "{opened:?}"
        );
        assert_eq!(stored, Stored::default());
    }

    #[test]
    fn a_second_damaged_file_keeps_the_first_copy() {
        let folder = TestFolder::new("damaged-twice");
        fs::create_dir_all(&folder.0).expect("the folder is created");
        let settings = folder.file("settings.ini");
        fs::write(&settings, [0xFF, 0x01]).expect("the first damaged file is written");
        open(&folder);
        fs::write(&settings, [0xFF, 0x02]).expect("the second damaged file is written");

        let (_, _, opened) = open(&folder);
        assert!(
            matches!(&opened, Opened::Damaged { copy, .. } if *copy == folder.file("settings.damaged-2.ini")),
            "{opened:?}"
        );
        assert_eq!(
            fs::read(folder.file("settings.damaged.ini")).expect("the first copy"),
            [0xFF, 0x01]
        );
        assert_eq!(
            fs::read(folder.file("settings.damaged-2.ini")).expect("the second copy"),
            [0xFF, 0x02]
        );
    }

    #[test]
    fn a_file_that_cant_be_read_is_never_written_over() {
        let folder = TestFolder::new("unreadable");
        // A folder where the file should be: reading it fails, but not because it is missing.
        fs::create_dir_all(folder.file("settings.ini")).expect("the folder is created");

        assert!(SettingsFile::open(folder.0.clone()).is_err());
        assert!(folder.file("settings.ini").is_dir());
        assert_eq!(folder.names(), ["settings.ini"]);
    }

    #[test]
    fn the_installer_copy_keeps_its_file_in_local_app_data() {
        let local_app_data =
            std::env::var_os("LOCALAPPDATA").expect("LOCALAPPDATA is set on Windows");
        assert_eq!(
            installer_copy_folder().expect("the folder is found"),
            PathBuf::from(local_app_data).join("ScreenGauge")
        );
    }
}
