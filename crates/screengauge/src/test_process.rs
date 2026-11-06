//! For tests: one test run alone, in a process of its own, for what the tests sharing the test exe's process would
//! hide. COM is one: a test that holds it open covers for another that needs it held (WPC-387).

use std::process::Command;

/// Runs the test at `path` (`module_path!()` and the test's name), ignored so the shared run skips it, alone in a new
/// process of this test exe, and fails if it fails, crashes the process or isn't found.
pub fn run_alone(path: &str) {
    let (_crate, name) = path
        .split_once("::")
        .expect("a test's path starts with its crate");
    let exe = std::env::current_exe().expect("this test exe's path");
    let output = Command::new(exe)
        .args(["--ignored", "--exact", name, "--test-threads=1"])
        .output()
        .expect("this test exe starts again");
    let stdout = String::from_utf8_lossy(&output.stdout);
    // A name no test has runs nothing and succeeds: only the test's own line passes.
    assert!(
        output.status.success() && stdout.contains(&format!("test {name} ... ok")),
        "{name}, alone in a process of its own: {}\n{stdout}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr),
    );
}
