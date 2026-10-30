//! The core makes no Windows call (spec, Modules): no Windows bindings crate may enter its dependency
//! tree, directly or through another crate, on any target. Dev-dependencies don't count: they never ship.

use std::process::Command;

const WINDOWS_BINDINGS: [&str; 2] = ["windows", "winapi"];

#[test]
fn core_dependency_tree_has_no_windows_bindings() {
    let output = Command::new(env!("CARGO"))
        .args([
            "tree",
            "--package",
            env!("CARGO_PKG_NAME"),
            "--target",
            "all",
            "--edges",
            "normal,build",
        ])
        .args(["--prefix", "none", "--format", "{p}"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("cargo tree should start");
    assert!(
        output.status.success(),
        "cargo tree failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let tree = String::from_utf8(output.stdout).expect("cargo tree prints UTF-8");

    // Guards against a vacuous pass: the tree must start at the core itself.
    assert!(
        tree.starts_with(env!("CARGO_PKG_NAME")),
        "unexpected cargo tree output:\n{tree}"
    );
    let bindings: Vec<&str> = tree
        .lines()
        .filter(|crate_line| {
            WINDOWS_BINDINGS
                .iter()
                .any(|name| crate_line.starts_with(name))
        })
        .collect();
    assert!(
        bindings.is_empty(),
        "the core depends on Windows bindings: {bindings:?}"
    );
}
