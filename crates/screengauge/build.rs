// Embeds ScreenGauge.exe.manifest, which makes the process Per-Monitor V2 from the start.
fn main() {
    let manifest =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("ScreenGauge.exe.manifest");
    println!("cargo:rustc-link-arg-bins=/MANIFEST:EMBED");
    println!(
        "cargo:rustc-link-arg-bins=/MANIFESTINPUT:{}",
        manifest.display()
    );
    println!("cargo:rerun-if-changed=ScreenGauge.exe.manifest");
}
