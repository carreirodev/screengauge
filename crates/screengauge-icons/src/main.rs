//! Writes every icon file in `assets/` again: `cargo run -p screengauge-icons`.
//! With `--preview <folder>`, writes enlarged pictures of the small sizes on the dark and light taskbars instead, to
//! judge them pixel by pixel.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use screengauge_icons::{assets, contrast, preview};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("screengauge-icons: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [] => {
            let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets");
            assets::write_all(&root)?;
            println!("wrote {}", root.display());
            print_contrast()
        }
        [flag, folder] if flag == "--preview" => {
            preview::write(&PathBuf::from(folder))?;
            println!("wrote the previews to {folder}");
            Ok(())
        }
        _ => Err("usage: screengauge-icons [--preview <folder>]".into()),
    }
}

/// The share of each .ico picture with 3:1 contrast, per theme background.
fn print_contrast() -> Result<(), String> {
    let names: Vec<&str> = contrast::THEME_BACKGROUNDS
        .iter()
        .map(|(n, _)| *n)
        .collect();
    println!("3:1 share  {}", names.join(" | "));
    for size in assets::ICO_SIZES {
        let image = assets::app_icon(size)?;
        let shares: Vec<String> = contrast::THEME_BACKGROUNDS
            .iter()
            .map(|(_, background)| {
                format!(
                    "{:.1}%",
                    contrast::passing_share(&image, *background) * 100.0
                )
            })
            .collect();
        println!("{size:>4} px    {}", shares.join(" | "));
    }
    Ok(())
}
