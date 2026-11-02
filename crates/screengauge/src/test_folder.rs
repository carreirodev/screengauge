//! A real folder for the tests that write files: one per test, in %TEMP%, removed when the test ends.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// A folder of its own per test, which doesn't exist yet and is removed when the test ends. `test` must be unique among
/// every test of the crate, as the tests run at the same time.
pub struct TestFolder(pub PathBuf);

impl TestFolder {
    pub fn new(test: &str) -> TestFolder {
        let path =
            std::env::temp_dir().join(format!("ScreenGauge.Test.{}.{test}", std::process::id()));
        remove(&path);
        TestFolder(path)
    }

    /// The folder, made at once.
    pub fn made(test: &str) -> TestFolder {
        let folder = TestFolder::new(test);
        fs::create_dir_all(&folder.0).expect("the folder is created");
        folder
    }

    pub fn file(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }

    /// The names in the folder, sorted.
    pub fn names(&self) -> Vec<String> {
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

/// Makes the file at `path` read-only. `remove_dir_all` still removes it.
pub fn make_read_only(path: &Path) {
    let mut permissions = fs::metadata(path).expect("the file exists").permissions();
    permissions.set_readonly(true);
    fs::set_permissions(path, permissions).expect("the file is made read-only");
}
