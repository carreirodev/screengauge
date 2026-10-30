//! The settings file on disk (v1 spec WPC-296, Settings file and remembered state): opened when ScreenGauge starts,
//! created with the defaults the first time so there is a file to edit, and replaced in one step whenever it is saved.
//! What the file holds and how its text reads belongs to the core (`screengauge_core::settings`).

use std::ffi::OsString;
use std::fs::{self, File};
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
/// value that went back to its default reaches only the debugger output, which never shows a folder's path: it
/// holds the Windows user name. When the file can't be opened, ScreenGauge runs with the defaults and has no file to
/// save to, so it never writes over a file it couldn't read or move aside.
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
                        "settings: back to the defaults, the damaged file kept as {}",
                        copy.file_name().unwrap_or_default().to_string_lossy()
                    ),
                    &damage,
                ),
            }
            (Some(file), stored)
        }
        Err(OpenError {
            damage: Some(damage),
            error,
        }) => {
            log::failure(
                &format!("settings: back to the defaults ({damage}), but the file wasn't replaced"),
                &error,
            );
            (None, Stored::default())
        }
        Err(OpenError {
            damage: None,
            error,
        }) => {
            log::failure("settings: opening the file", &error);
            (None, Stored::default())
        }
    }
}

/// A settings file that was opened, so saving may replace it.
#[derive(Debug)]
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
    /// The file was damaged. It was moved to `copy`, and a file with the defaults took its place: the remembered state
    /// went back to its defaults too.
    Damaged { damage: Damaged, copy: PathBuf },
}

/// Opening failed, so ScreenGauge runs with the defaults and without a file to save to.
#[derive(Debug)]
pub struct OpenError {
    /// The file was damaged: ScreenGauge went back to the defaults all the same, and the user must hear of it.
    pub damage: Option<Damaged>,
    pub error: io::Error,
}

impl From<io::Error> for OpenError {
    fn from(error: io::Error) -> OpenError {
        OpenError {
            damage: None,
            error,
        }
    }
}

impl SettingsFile {
    /// Opens `settings.ini` in `folder`. An error loses nothing: the file is where it was, or moved aside whole.
    pub fn open(folder: PathBuf) -> Result<(SettingsFile, Stored, Opened), OpenError> {
        let file = SettingsFile { folder };
        let bytes = match fs::read(file.path()) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                let stored = Stored::default();
                file.save(&stored)?;
                return Ok((file, stored, Opened::New));
            }
            Err(e) => return Err(e.into()),
        };
        match settings::read(&bytes) {
            Ok(loaded) => Ok((file, loaded.stored, Opened::Read(loaded.resets))),
            Err(damage) => {
                let stored = Stored::default();
                let copy = file
                    .move_damaged_aside()
                    .and_then(|copy| file.save(&stored).map(|()| copy))
                    .map_err(|error| OpenError {
                        damage: Some(damage),
                        error,
                    })?;
                Ok((file, stored, Opened::Damaged { damage, copy }))
            }
        }
    }

    /// Replaces the file in one step: the new text goes to a temporary file first, and renaming it over the old one
    /// either happens whole or not at all, so a crash or a full disk never leaves half a file.
    pub fn save(&self, stored: &Stored) -> io::Result<()> {
        fs::create_dir_all(&self.folder)?;
        let temporary = self.folder.join(TEMPORARY_NAME);
        let replaced = File::create(&temporary)
            .and_then(|mut file| {
                file.write_all(settings::write(stored).as_bytes())?;
                file.sync_all()
            })
            .and_then(|()| fs::rename(&temporary, self.path()));
        let Err(e) = replaced else {
            return Ok(());
        };
        // A failed save leaves no temporary file behind.
        match fs::remove_file(&temporary) {
            Ok(()) => Err(e),
            Err(cleanup) if cleanup.kind() == io::ErrorKind::NotFound => Err(e),
            Err(cleanup) => Err(io::Error::new(
                e.kind(),
                format!("{e}; removing {TEMPORARY_NAME} failed too: {cleanup}"),
            )),
        }
    }

    fn path(&self) -> PathBuf {
        self.folder.join(FILE_NAME)
    }

    /// Moves a damaged file to `settings.damaged.ini`, or to `settings.damaged-2.ini` and so on when an earlier copy
    /// is there, so no copy is ever replaced. A move keeps the bytes as they were and works on a read-only file too.
    fn move_damaged_aside(&self) -> io::Result<PathBuf> {
        for number in 1..=u32::MAX {
            let name = match number {
                1 => String::from("settings.damaged.ini"),
                _ => format!("settings.damaged-{number}.ini"),
            };
            let copy = self.folder.join(name);
            // Only the running copy of ScreenGauge writes in this folder, so nothing takes the name between this check
            // and the move.
            if !copy.try_exists()? {
                fs::rename(self.path(), &copy)?;
                return Ok(copy);
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
    use std::fs::OpenOptions;
    use std::os::windows::fs::OpenOptionsExt;
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

        /// The folder already there, holding a settings file with `bytes`.
        fn with_settings(test: &str, bytes: &[u8]) -> TestFolder {
            let folder = TestFolder::new(test);
            fs::create_dir_all(&folder.0).expect("the folder is created");
            fs::write(folder.file("settings.ini"), bytes).expect("the settings file is written");
            folder
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

    fn make_read_only(path: &Path) {
        let mut permissions = fs::metadata(path).expect("the file exists").permissions();
        permissions.set_readonly(true);
        fs::set_permissions(path, permissions).expect("the file is made read-only");
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
            core::read(&written).map(|loaded| loaded.stored),
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
    fn a_failed_save_leaves_the_file_whole_and_no_temporary_file() {
        let folder = TestFolder::new("failed-save");
        let (file, _, _) = open(&folder);
        let before = fs::read(folder.file("settings.ini")).expect("the file was written");
        make_read_only(&folder.file("settings.ini"));

        let mut changed = Stored::default();
        changed.options.theme = Theme::Dark;
        assert!(file.save(&changed).is_err());
        assert_eq!(
            fs::read(folder.file("settings.ini")).expect("the file is there"),
            before
        );
        assert_eq!(folder.names(), ["settings.ini"]);
    }

    #[test]
    fn a_damaged_file_is_kept_and_the_defaults_take_its_place() {
        let damaged = b"last_tool = Snip\nwarned_about_other_copy = true\nnot a setting\n";
        let folder = TestFolder::with_settings("damaged", damaged);

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
    fn a_read_only_damaged_file_is_kept_and_replaced_too() {
        let folder = TestFolder::with_settings("damaged-read-only", &[0xFF, 0x01]);
        make_read_only(&folder.file("settings.ini"));

        let (_, _, opened) = open(&folder);
        assert!(matches!(opened, Opened::Damaged { .. }), "{opened:?}");
        assert_eq!(
            fs::read(folder.file("settings.damaged.ini")).expect("the copy"),
            [0xFF, 0x01]
        );
        assert_eq!(folder.names(), ["settings.damaged.ini", "settings.ini"]);
    }

    #[test]
    fn a_second_damaged_file_keeps_the_first_copy() {
        let folder = TestFolder::with_settings("damaged-twice", &[0xFF, 0x01]);
        open(&folder);
        fs::write(folder.file("settings.ini"), [0xFF, 0x02])
            .expect("the second damaged file is written");

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
    fn a_damaged_file_that_cant_be_moved_is_left_as_it_was_and_still_reported() {
        let folder = TestFolder::with_settings("damaged-locked", &[0xFF, 0x01]);
        // Another program holds the file open for reading only (FILE_SHARE_READ), so it can't be moved.
        let _held = OpenOptions::new()
            .read(true)
            .share_mode(0x1)
            .open(folder.file("settings.ini"))
            .expect("the file is held open");

        let result = SettingsFile::open(folder.0.clone());
        assert!(
            matches!(
                &result,
                Err(OpenError {
                    damage: Some(Damaged::NotUtf8),
                    ..
                })
            ),
            "{result:?}"
        );
        assert_eq!(
            fs::read(folder.file("settings.ini")).expect("the file is there"),
            [0xFF, 0x01]
        );
        assert_eq!(folder.names(), ["settings.ini"]);
    }

    #[test]
    fn a_file_that_cant_be_read_is_never_written_over() {
        let folder = TestFolder::new("unreadable");
        // A folder where the file should be: reading it fails, but not because it is missing.
        fs::create_dir_all(folder.file("settings.ini")).expect("the folder is created");

        let result = SettingsFile::open(folder.0.clone());
        assert!(
            matches!(&result, Err(OpenError { damage: None, .. })),
            "{result:?}"
        );
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
