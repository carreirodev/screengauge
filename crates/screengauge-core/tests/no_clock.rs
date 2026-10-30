//! The core never reads the clock (v1 spec WPC-296, Modules): the local time comes with each event, so every rule that
//! depends on it, like the Snip's file name, is tested with a time the test chooses. This reads the core's code for
//! the standard library's clocks; a clock from another crate can't come in, since the core has no dependency.

use std::fs;
use std::path::Path;

const CLOCKS: [&str; 3] = ["SystemTime", "Instant", "UNIX_EPOCH"];

/// Every `.rs` file under `folder`.
fn rust_files(folder: &Path, files: &mut Vec<std::path::PathBuf>) {
    for entry in fs::read_dir(folder).expect("the source folder can be listed") {
        let path = entry.expect("a folder entry").path();
        if path.is_dir() {
            rust_files(&path, files);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            files.push(path);
        }
    }
}

#[test]
fn core_code_never_reads_the_clock() {
    let mut files = Vec::new();
    rust_files(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut files,
    );
    // Guards against a vacuous pass: the session core must be among the files read.
    assert!(
        files.iter().any(|file| file.ends_with("session.rs")),
        "the session core wasn't found in {files:?}"
    );
    let mut readings = Vec::new();
    for file in &files {
        let code = fs::read_to_string(file).expect("a readable source file");
        for (number, line) in code.lines().enumerate() {
            // Comments may say what the core doesn't do.
            if line.trim_start().starts_with("//") {
                continue;
            }
            if CLOCKS.iter().any(|clock| line.contains(clock)) {
                readings.push(format!(
                    "{}:{}: {}",
                    file.display(),
                    number + 1,
                    line.trim()
                ));
            }
        }
    }
    assert!(
        readings.is_empty(),
        "the core reads the clock:\n{}",
        readings.join("\n")
    );
}
