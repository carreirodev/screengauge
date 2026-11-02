//! The settings file on disk (v1 spec WPC-296, Settings file and remembered state): opened when ScreenGauge starts,
//! created with the defaults the first time so there is a file to edit, and replaced in one step whenever it is saved.
//! What the file holds and how its text reads belongs to the core (`screengauge_core::settings`).
//!
//! What the file holds is kept here once, with the file to save it to: the tools read it at every call, Settings reads
//! and changes it, and the remembered state is saved into it, so no part ever saves a copy that undoes another's change.

use std::cell::RefCell;
use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{self, Write};
use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;

use screengauge_core::settings::{self, Damaged, Reset, Stored};
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::UI::Shell::{FOLDERID_LocalAppData, KF_FLAG_DEFAULT, SHGetKnownFolderPath};

use crate::log;
use crate::notification::{self, Notification};

thread_local! {
    /// What the settings file holds, and the file to save it to. Set once, as the process starts, on the resident
    /// thread, the only one that reads or changes it.
    static KEPT: RefCell<Option<Kept>> = const { RefCell::new(None) };
}

/// The settings file as ScreenGauge read it at startup, kept up to date as it saves.
struct Kept {
    /// `None` when the file couldn't be opened: ScreenGauge then runs with the defaults and saves nothing.
    file: Option<SettingsFile>,
    stored: Stored,
}

/// Keeps what the settings file held at startup, and the file to save it to, for the rest of the run.
pub fn keep(file: Option<SettingsFile>, stored: Stored) {
    KEPT.set(Some(Kept { file, stored }));
}

/// What the settings file holds now, Settings' changes and the remembered state included.
pub fn current() -> io::Result<Stored> {
    KEPT.with_borrow(|kept| {
        kept.as_ref()
            .map(|kept| kept.stored.clone())
            .ok_or_else(not_kept)
    })
}

/// Changes what the settings file holds with `change`, which says whether it changed anything, and then saves the file
/// when there is one. The change is kept even when the save fails, so it holds until ScreenGauge exits. Returns whether
/// anything changed.
pub fn update(change: impl FnOnce(&mut Stored) -> bool) -> io::Result<bool> {
    KEPT.with_borrow_mut(|kept| {
        let kept = kept.as_mut().ok_or_else(not_kept)?;
        if !change(&mut kept.stored) {
            return Ok(false);
        }
        if let Some(file) = &kept.file {
            file.save(&kept.stored)?;
        }
        Ok(true)
    })
}

fn not_kept() -> io::Error {
    io::Error::other("the settings file wasn't read at startup")
}

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

/// Opens the settings file as ScreenGauge starts. A damaged file means a Windows notification, which waits for the
/// startup call's photo, so ScreenGauge never goes back to the defaults in silence; a value that went back to its
/// default reaches only the debugger output. That output never shows a folder's path: it holds the Windows user name.
/// When the file can't be opened, ScreenGauge runs with the defaults and has no file to save to, so it never writes
/// over a file it couldn't read or move aside.
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
                Opened::Damaged { damage, copy } => {
                    log::failure(
                        &format!(
                            "settings: back to the defaults, the damaged file kept as {}",
                            copy.file_name().unwrap_or_default().to_string_lossy()
                        ),
                        &damage,
                    );
                    notification::wait(Notification::SettingsReset);
                }
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
            // The damaged file stays where it was, untouched, so a copy is kept all the same.
            notification::wait(Notification::SettingsReset);
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

    use screengauge_core::settings::{self as core, Change, ColorFormat, Theme, Tool};

    use super::*;
    use crate::test_folder::{TestFolder, make_read_only};

    /// The folder already there, holding a settings file with `bytes`.
    fn with_settings(test: &str, bytes: &[u8]) -> TestFolder {
        let folder = TestFolder::made(test);
        fs::write(folder.file("settings.ini"), bytes).expect("the settings file is written");
        folder
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
        let folder = with_settings("damaged", damaged);

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
        let folder = with_settings("damaged-read-only", &[0xFF, 0x01]);
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
        let folder = with_settings("damaged-twice", &[0xFF, 0x01]);
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
        let folder = with_settings("damaged-locked", &[0xFF, 0x01]);
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

    /// What the file in `folder` holds, read back from the disk.
    fn on_disk(folder: &TestFolder) -> Stored {
        let bytes = fs::read(folder.file("settings.ini")).expect("the file is there");
        core::read(&bytes).expect("the file reads").stored
    }

    #[test]
    fn a_change_from_settings_is_kept_and_saved_at_once() {
        let folder = TestFolder::new("kept-change");
        let (file, stored, _) = open(&folder);
        keep(Some(file), stored);

        let changed = update(|stored| stored.options.apply(Change::ColorFormat(ColorFormat::Hex)));
        assert!(changed.expect("the change is saved"));
        // The next call of the tools reads what is kept, and the file holds it too.
        assert_eq!(
            current().expect("kept").options.color_format,
            ColorFormat::Hex
        );
        assert_eq!(on_disk(&folder).options.color_format, ColorFormat::Hex);
    }

    #[test]
    fn saving_the_remembered_state_keeps_an_option_changed_since_the_tools_opened() {
        let folder = TestFolder::new("remembered-after-change");
        let (file, stored, _) = open(&folder);
        keep(Some(file), stored);
        // The tools open with what is kept now, and Settings changes an option afterwards.
        let mut remembered = current().expect("kept").remembered;
        update(|stored| stored.options.apply(Change::DefaultToleranceUp)).expect("saved");
        // The tools close and save what they remember.
        remembered.last_tool = Tool::Snip;
        update(|stored| {
            stored.remembered = remembered.clone();
            true
        })
        .expect("saved");
        let read = on_disk(&folder);
        assert_eq!(read.remembered.last_tool, Tool::Snip);
        assert_eq!(
            read.options.default_tolerance,
            core::Tolerance::default().up()
        );
    }

    #[test]
    fn a_change_that_changes_nothing_writes_nothing() {
        let folder = TestFolder::new("no-change");
        let (file, stored, _) = open(&folder);
        keep(Some(file), stored);
        // Read-only, the file can't be saved: a save would fail.
        make_read_only(&folder.file("settings.ini"));
        let changed = update(|stored| stored.options.apply(Change::ColorFormat(ColorFormat::Both)));
        assert!(!changed.expect("nothing to save"));
    }

    #[test]
    fn a_change_holds_for_the_run_even_when_the_file_can_t_be_saved() {
        let folder = TestFolder::new("read-only-change");
        let (file, stored, _) = open(&folder);
        keep(Some(file), stored);
        make_read_only(&folder.file("settings.ini"));
        let saved = update(|stored| stored.options.apply(Change::AlsoShowLogicalPixels(false)));
        assert!(saved.is_err(), "{saved:?}");
        assert!(!current().expect("kept").options.also_show_logical_pixels);
        assert!(on_disk(&folder).options.also_show_logical_pixels);
    }

    #[test]
    fn without_a_file_a_change_holds_for_the_run() {
        keep(None, Stored::default());
        let changed = update(|stored| stored.options.apply(Change::DefaultToleranceDown));
        assert!(changed.expect("nothing to save to"));
        assert_eq!(
            current().expect("kept").options.default_tolerance,
            core::Tolerance::default().down()
        );
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
