//! Every comment line in the workspace's Rust files fits in 120 columns, counted by character, as the code around it
//! does (WPC-395). rustfmt keeps the code inside its width but leaves comments as written, so this holds them.

use std::fs;
use std::path::{Path, PathBuf};

/// The widest a comment line may be, in characters.
const WIDTH: usize = 120;

#[test]
fn no_comment_line_is_wider_than_120_columns() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the core sits in the workspace's crates folder")
        .to_path_buf();
    let mut files = Vec::new();
    rust_files(&crates, &mut files);
    // Guards against a vacuous pass: the walk must find the core's own source.
    assert!(
        files
            .iter()
            .any(|file| file.ends_with("screengauge-core/src/lib.rs")),
        "the walk found no core source under {}",
        crates.display()
    );

    let mut wide = Vec::new();
    for file in &files {
        let text = fs::read_to_string(file).expect("a Rust file is UTF-8");
        for (index, line) in text.lines().enumerate() {
            let width = line.chars().count();
            if line.trim_start().starts_with("//") && width > WIDTH {
                wide.push(format!("{}:{} ({width})", file.display(), index + 1));
            }
        }
    }
    assert!(
        wide.is_empty(),
        "{} comment lines are wider than {WIDTH} columns:\n{}",
        wide.len(),
        wide.join("\n")
    );
}

/// Every `.rs` file under `folder`, into `files`, leaving out build output.
fn rust_files(folder: &Path, files: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(folder).expect("a folder of the workspace") {
        let path = entry.expect("an entry of the folder").path();
        if path.is_dir() {
            if path.file_name().is_some_and(|name| name != "target") {
                rust_files(&path, files);
            }
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            files.push(path);
        }
    }
}
