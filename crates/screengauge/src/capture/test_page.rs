//! The test page's captures (v1 spec WPC-296, test layer 1; ticket WPC-331): `test-page/test-page.html`, drawn by
//! Chrome on the owner's monitors and photographed through ScreenGauge's own capture, cropped to the page alone, and
//! kept in `test-page/captures/` for the tests without a screen. `test-page/README.md` records how they were taken.

//!
//! The capture program is the ignored test below, so neither CI nor the pre-commit hook runs it: neither has a real
//! monitor. It opens a Chrome of its own, puts the page on every monitor in turn, and writes each monitor's capture.
//! Run it on the owner's computer, with the monitors at the scales to capture:
//!
//! `cargo test -p screengauge --release --bin ScreenGauge capture_the_test_page -- --ignored --nocapture`
//!
//! The other tests read the captures in the repository and run everywhere.

use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use screengauge_core::session::{Colors, Image, Scale};
use windows::Win32::Foundation::{HWND, LPARAM, RECT, WPARAM};
use windows::Win32::Storage::FileSystem::{
    GetFileVersionInfoSizeW, GetFileVersionInfoW, VS_FIXEDFILEINFO, VerQueryValueW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowTextW, GetWindowThreadProcessId, HWND_TOPMOST, IsWindowVisible,
    PostMessageW, SW_MAXIMIZE, SW_RESTORE, SWP_SHOWWINDOW, SetWindowPos, ShowWindow, WM_CLOSE,
};
use windows::core::{BOOL, PCWSTR, w};

use super::{Monitor, Shot, every_shot, monitors};
use crate::png;

/// The page's stage, in CSS px, as test-page.html sizes it.
const PAGE_WIDTH: u32 = 1200;
const PAGE_HEIGHT: u32 = 720;
/// Around the stage, pure magenta (#FF00FF), which nothing on the stage is: blue, green, red, as a photo's pixels are.
const OUTSIDE: [u8; 3] = [0xFF, 0x00, 0xFF];

/// The page alone, cropped from `image`, the photo of a monitor at `scale` where Chrome shows it at 100% zoom: the one
/// rectangle of the page's size at that scale with magenta all around it and none inside. Anything else is refused,
/// with why, so a capture never holds anything but the page.
fn page_in(image: &Image, scale: Scale) -> Result<Image, String> {
    let (width, height) = page_size(scale).ok_or_else(|| {
        format!(
            "at {} DPI the page isn't a whole number of pixels",
            scale.dpi()
        )
    })?;
    let (left, top) = find(image, width, height)?;
    if let Some((x, y)) = first_outside(image, left, top, width, height) {
        return Err(format!(
            "the page of {width} × {height} at ({left}, {top}) has magenta inside, at ({x}, {y}) of the photo, which \
             test-page.html never draws there"
        ));
    }
    Ok(crop(image, left, top, width, height))
}

/// The page's size in physical pixels at `scale`, as Chrome draws a CSS px at the monitor's scale: `None` when it isn't
/// whole, which no Windows scale makes, since each is a multiple of 25% and the page's sides are multiples of 4.
fn page_size(scale: Scale) -> Option<(u32, u32)> {
    let physical = |css: u32| {
        let scaled = css.checked_mul(scale.dpi())?;
        (scaled % 96 == 0).then_some(scaled / 96)
    };
    Some((physical(PAGE_WIDTH)?, physical(PAGE_HEIGHT)?))
}

/// The top-left corner of the one `width` × `height` rectangle of `image` whose ring of the pixels just around it is
/// all magenta, and whose own corner isn't.
fn find(image: &Image, width: u32, height: u32) -> Result<(u32, u32), String> {
    let (photo_width, photo_height) = (image.width(), image.height());
    let wanted = format!("page of {width} × {height} with magenta all around it");
    if photo_width < width + 2 || photo_height < height + 2 {
        return Err(format!(
            "the photo, {photo_width} × {photo_height}, is too small for a {wanted}"
        ));
    }
    let outside = |x, y| is_outside(image, x, y);
    let mut found = Vec::new();
    for top in 1..photo_height - height {
        for left in 1..photo_width - width {
            if outside(left - 1, top - 1)
                && outside(left - 1, top)
                && outside(left, top - 1)
                && !outside(left, top)
                && ringed(image, left, top, width, height)
            {
                found.push((left, top));
            }
        }
    }
    match found.as_slice() {
        [] => Err(format!(
            "no {wanted}: Chrome isn't at 100% zoom, the window is covered or not on this monitor, or the monitor's \
             scale isn't the one Windows reports"
        )),
        [one] => Ok(*one),
        many => Err(format!("{} places look like the {wanted}", many.len())),
    }
}

/// Whether every pixel just around the `width` × `height` rectangle at (`left`, `top`) is magenta.
fn ringed(image: &Image, left: u32, top: u32, width: u32, height: u32) -> bool {
    let (right, bottom) = (left + width, top + height);
    (left - 1..=right).all(|x| is_outside(image, x, top - 1) && is_outside(image, x, bottom))
        && (top..bottom).all(|y| is_outside(image, left - 1, y) && is_outside(image, right, y))
}

/// The first magenta pixel inside the `width` × `height` rectangle at (`left`, `top`), row by row, if any.
fn first_outside(
    image: &Image,
    left: u32,
    top: u32,
    width: u32,
    height: u32,
) -> Option<(u32, u32)> {
    (top..top + height)
        .flat_map(|y| (left..left + width).map(move |x| (x, y)))
        .find(|&(x, y)| is_outside(image, x, y))
}

fn is_outside(image: &Image, x: u32, y: u32) -> bool {
    let [blue, green, red, _] = image.pixels()[pixel_index(image, x, y)];
    [blue, green, red] == OUTSIDE
}

fn pixel_index(image: &Image, x: u32, y: u32) -> usize {
    // Can't fail: ScreenGauge builds for x64 alone, where a usize holds any u32, and the image's pixels fit in memory.
    usize::try_from(u64::from(y) * u64::from(image.width()) + u64::from(x))
        .expect("a pixel of an image in memory")
}

/// The `width` × `height` rectangle of `image` at (`left`, `top`), which must be inside it.
fn crop(image: &Image, left: u32, top: u32, width: u32, height: u32) -> Image {
    let row_bytes = usize::try_from(width).expect("a row in memory") * 4;
    let bgra = (top..top + height)
        .flat_map(|y| {
            let start = pixel_index(image, left, y) * 4;
            image.bgra()[start..start + row_bytes].iter().copied()
        })
        .collect();
    Image::new(width, height, bgra).expect("whole rows of the crop's width")
}

/// A capture's file name: the monitor's resolution and its scale, such as `3840x2160-175.png`. `None` at a scale that
/// isn't a whole percent.
fn file_name(width: u32, height: u32, scale: Scale) -> Option<String> {
    let percent = scale.dpi().checked_mul(100)?;
    (percent % 96 == 0).then(|| format!("{width}x{height}-{}.png", percent / 96))
}

/// The monitor's resolution and scale a capture's file name gives, as [`file_name`] writes it.
fn parse_file_name(name: &str) -> Option<(u32, u32, Scale)> {
    let (size, percent) = name.strip_suffix(".png")?.split_once('-')?;
    let (width, height) = size.split_once('x')?;
    let dpi = percent.parse::<u32>().ok()?.checked_mul(96)?;
    if dpi % 100 != 0 {
        return None;
    }
    Some((
        width.parse().ok()?,
        height.parse().ok()?,
        Scale::from_dpi(dpi / 100)?,
    ))
}

/// The repository's `test-page` folder.
fn folder() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the crate is two folders below the repository's")
        .join("test-page")
}

/// The captures the ticket asks for: the AOC at 175% and the ARZOPA at 100% and 125%, as [`file_name`] names them.
const CAPTURES: [&str; 3] = [
    "3840x2160-175.png",
    "1920x1080-100.png",
    "1920x1080-125.png",
];

/// Chrome, where the owner has it.
const CHROME: &str = r"C:\Program Files\Google\Chrome\Application\chrome.exe";
/// Besides its profile and the page: no first-run pages or default-browser question, and colors as the page gives
/// them, whatever color profile the monitor has.
const CHROME_FLAGS: [&str; 3] = [
    "--no-first-run",
    "--no-default-browser-check",
    "--force-color-profile=srgb",
];
/// The page's title, which Chrome gives its window.
const TITLE: &str = "ScreenGauge test page";

/// A Chrome of its own, apart from the owner's: a new, empty profile, so no tab, extension, account or setting of
/// theirs can show, and the page alone in an app window, with no address bar, at 100% zoom. Dropping it closes it.
struct Chrome {
    process: Child,
    window: Option<HWND>,
}

impl Chrome {
    fn start() -> Chrome {
        let profile = std::env::temp_dir().join("screengauge-test-page-chrome");
        if profile.exists() {
            std::fs::remove_dir_all(&profile)
                .expect("the last run's Chrome profile is removed, so the profile is new");
        }
        let page = folder().join("test-page.html");
        assert!(page.is_file(), "{} is missing", page.display());
        let url = format!(
            "file:///{}",
            page.display()
                .to_string()
                .replace('\\', "/")
                .replace(' ', "%20")
        );
        let process = Command::new(CHROME)
            .arg(format!("--user-data-dir={}", profile.display()))
            .args(CHROME_FLAGS)
            .arg(format!("--app={url}"))
            .spawn()
            .expect("Chrome starts");
        Chrome {
            process,
            window: None,
        }
    }

    /// The page's window, once Chrome shows it.
    fn window(&mut self) -> HWND {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            if let Some(window) = top_windows()
                .into_iter()
                .find(|&window| is_page_window(window, self.process.id()))
            {
                self.window = Some(window);
                return window;
            }
            assert!(
                Instant::now() < deadline,
                "Chrome showed no window titled {TITLE:?} in 15 s"
            );
            std::thread::sleep(Duration::from_millis(200));
        }
    }
}

impl Drop for Chrome {
    fn drop(&mut self) {
        if let Some(window) = self.window {
            // SAFETY: a message with no pointers, to a window that may be gone already, which PostMessageW reports.
            if let Err(e) = unsafe { PostMessageW(Some(window), WM_CLOSE, WPARAM(0), LPARAM(0)) } {
                eprintln!("couldn't ask Chrome's window to close: {e}");
            }
        }
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            match self.process.try_wait() {
                Ok(Some(_)) => return,
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(200));
                }
                Ok(None) => break,
                Err(e) => {
                    eprintln!("couldn't tell whether Chrome closed: {e}");
                    break;
                }
            }
        }
        eprintln!("Chrome didn't close in 10 s, so it is ended");
        if let Err(e) = self.process.kill() {
            eprintln!("couldn't end Chrome: {e}");
        }
    }
}

/// Every top-level window.
fn top_windows() -> Vec<HWND> {
    extern "system" fn push(window: HWND, list: LPARAM) -> BOOL {
        // SAFETY: `list` is the vector `top_windows` passes below, alive and not otherwise borrowed during the call.
        unsafe { &mut *(list.0 as *mut Vec<HWND>) }.push(window);
        true.into()
    }
    let mut windows: Vec<HWND> = Vec::new();
    // SAFETY: the callback only pushes to `windows`, which outlives the call.
    unsafe { EnumWindows(Some(push), LPARAM((&raw mut windows) as isize)) }
        .expect("the top-level windows");
    windows
}

/// Whether `window` is the page's, shown by the Chrome of process `process`.
fn is_page_window(window: HWND, process: u32) -> bool {
    let mut owner = 0;
    // SAFETY: a window EnumWindows listed, which may be gone already: then the calls return 0 and FALSE.
    unsafe {
        GetWindowThreadProcessId(window, Some(&mut owner));
        if owner != process || !IsWindowVisible(window).as_bool() {
            return false;
        }
    }
    let mut title = [0u16; 64];
    // SAFETY: as above, into a local buffer of the length passed.
    let length = unsafe { GetWindowTextW(window, &mut title) };
    let length = usize::try_from(length).expect("a length of at most 63");
    String::from_utf16_lossy(&title[..length]) == TITLE
}

/// Puts the page's window maximized on `monitor`, on top of every other window.
fn place(window: HWND, monitor: &Monitor) {
    let RECT { left, top, .. } = monitor.rect;
    // SAFETY: plain calls on a live window of another process, which Windows allows across processes of one user.
    // ShowWindow returns whether the window was visible before, not an error.
    unsafe {
        let _ = ShowWindow(window, SW_RESTORE);
        SetWindowPos(
            window,
            Some(HWND_TOPMOST),
            left + 64,
            top + 64,
            1024,
            768,
            SWP_SHOWWINDOW,
        )
        .expect("Chrome's window moves to the monitor");
        let _ = ShowWindow(window, SW_MAXIMIZE);
    }
}

/// The page alone, from ScreenGauge's photo of the monitor at `index` of `monitor`: from two photos in a row that agree,
/// so Chrome is done drawing at the monitor's scale. Refused when the photo's colors aren't the ones drawn, or no
/// photo shows the page in 10 s, with the last reason.
fn photograph_page(index: usize, monitor: &Monitor) -> Result<(Image, String), String> {
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut last: Option<Image> = None;
    loop {
        let (listed, shots) = every_shot().map_err(|e| format!("no photo: {e}"))?;
        if listed.get(index).map(|listed| &listed.device) != Some(&monitor.device) {
            return Err(String::from("the monitors changed during the capture"));
        }
        let (photo, how) = match shots.into_iter().nth(index) {
            Some(Shot::Dxgi(photo)) => (photo, String::from("through DXGI")),
            Some(Shot::Gdi { photo, dxgi }) => {
                (photo, format!("through GDI, since DXGI failed ({dxgi})"))
            }
            Some(Shot::Failed { dxgi, gdi }) => {
                return Err(format!(
                    "no photo: DXGI failed ({dxgi}), and GDI failed ({gdi})"
                ));
            }
            None => return Err(String::from("no photo of the monitor")),
        };
        if photo.colors != Colors::Exact {
            return Err(String::from(
                "the photo's colors are approximate, since HDR is on and its brightening couldn't be undone",
            ));
        }
        match page_in(&photo.image, monitor.scale) {
            Ok(page) if last.as_ref() == Some(&page) => return Ok((page, how)),
            Ok(page) => last = Some(page),
            Err(e) if Instant::now() >= deadline => {
                return Err(with_photo(e, &photo.image, &monitor.device));
            }
            // Chrome may still be moving the window or drawing at the new scale: the reason is kept only at the deadline.
            Err(_) => last = None,
        }
        if Instant::now() >= deadline {
            return Err(String::from("Chrome kept changing the page for 10 s"));
        }
        std::thread::sleep(Duration::from_millis(300));
    }
}

/// `reason`, with where the monitor's photo is saved to see it: in the temporary folder, never in the repository, since
/// it shows the whole monitor.
fn with_photo(reason: String, image: &Image, device: &str) -> String {
    let name = device.trim_start_matches(r"\\.\").to_lowercase();
    let path = std::env::temp_dir().join(format!("screengauge-test-page-{name}.png"));
    match png::encode(image)
        .map_err(|e| e.to_string())
        .and_then(|bytes| std::fs::write(&path, bytes).map_err(|e| e.to_string()))
    {
        Ok(()) => format!("{reason}; the photo is at {}", path.display()),
        Err(e) => format!("{reason}; the photo couldn't be saved: {e}"),
    }
}

/// Writes `page` as the PNG at `path`, and says how it compares with the file already there.
fn write_capture(path: &Path, page: &Image) -> &'static str {
    crate::com::ensure_started().expect("COM, for WIC");
    let before = std::fs::read(path)
        .ok()
        .map(|bytes| pixels_of(&png::read_back::decode(&bytes)));
    let bytes = png::encode(page).expect("the page as a PNG");
    std::fs::write(path, bytes).expect("the capture is written");
    let now: Vec<[u8; 3]> = page
        .pixels()
        .iter()
        .map(|&[b, g, r, _]| [b, g, r])
        .collect();
    match before {
        None => "new",
        Some(before) if before == now => "the same pixels as the file that was there",
        Some(_) => "REPLACED: its pixels differ from the file that was there",
    }
}

/// A read-back PNG's pixels, as blue, green, red.
fn pixels_of(decoded: &png::read_back::Decoded) -> Vec<[u8; 3]> {
    decoded.bgra.iter().map(|&[b, g, r, _]| [b, g, r]).collect()
}

/// Chrome's version, from its file's version resource.
fn chrome_version() -> String {
    let path: Vec<u16> = CHROME.encode_utf16().chain([0]).collect();
    let path = PCWSTR(path.as_ptr());
    // SAFETY: a null-terminated path that outlives the calls; the buffer has the size Windows asked for, and the value
    // VerQueryValueW points to lies inside it, read while it lives.
    unsafe {
        let size = GetFileVersionInfoSizeW(path, None);
        assert!(size > 0, "Chrome's file has no version resource");
        let mut data = vec![0u8; usize::try_from(size).expect("a small resource")];
        GetFileVersionInfoW(path, None, size, data.as_mut_ptr().cast())
            .expect("Chrome's version resource");
        let mut value = std::ptr::null_mut();
        let mut length = 0;
        assert!(
            VerQueryValueW(data.as_ptr().cast(), w!("\\"), &mut value, &mut length).as_bool(),
            "Chrome's fixed version"
        );
        let fixed = &*value.cast::<VS_FIXEDFILEINFO>();
        format!(
            "{}.{}.{}.{}",
            fixed.dwProductVersionMS >> 16,
            fixed.dwProductVersionMS & 0xFFFF,
            fixed.dwProductVersionLS >> 16,
            fixed.dwProductVersionLS & 0xFFFF
        )
    }
}

#[test]
#[ignore = "opens Chrome and photographs the real screen, so it runs on the owner's computer, never in CI"]
fn capture_the_test_page() {
    assert!(
        super::self_check::per_monitor_v2(),
        "the test program isn't Per-Monitor V2, so its pixels wouldn't be physical: the manifest didn't reach it"
    );
    let captures = folder().join("captures");
    std::fs::create_dir_all(&captures).expect("the captures folder");
    let mut chrome = Chrome::start();
    let window = chrome.window();
    let monitors = monitors().expect("the monitors");
    let mut report = format!("Chrome {}\n", chrome_version());
    let mut failed = 0;
    for (index, monitor) in monitors.iter().enumerate() {
        let RECT {
            left,
            top,
            right,
            bottom,
        } = monitor.rect;
        let (width, height) = (
            u32::try_from(right - left).expect("a monitor's width"),
            u32::try_from(bottom - top).expect("a monitor's height"),
        );
        report.push_str(&format!(
            "{}: {width} × {height} at ({left}, {top}), {} DPI\n",
            monitor.device,
            monitor.scale.dpi()
        ));
        place(window, monitor);
        let name = file_name(width, height, monitor.scale);
        match (name, photograph_page(index, monitor)) {
            (Some(name), Ok((page, how))) => {
                let path = captures.join(&name);
                let compared = write_capture(&path, &page);
                report.push_str(&format!(
                    "  {name}: {} × {}, {how}, {compared}\n",
                    page.width(),
                    page.height()
                ));
            }
            (None, _) => {
                failed += 1;
                report.push_str("  FAIL: the scale isn't a whole percent\n");
            }
            (_, Err(e)) => {
                failed += 1;
                report.push_str(&format!("  FAIL: {e}\n"));
            }
        }
    }
    drop(chrome);
    println!("{report}");
    assert_eq!(failed, 0, "a monitor wasn't captured; the report is above");
}

#[cfg(test)]
mod tests {
    use super::*;

    const WHITE: [u8; 3] = [0xFF, 0xFF, 0xFF];

    /// A photo of `width` × `height`, all magenta, with a white page of `page` = (width, height) at `at`, its pixels
    /// varied so a crop off by one shows, and `then` applied to it as (x, y, blue-green-red) changes.
    fn photo(
        (width, height): (u32, u32),
        at: (u32, u32),
        page: (u32, u32),
        then: &[(u32, u32, [u8; 3])],
    ) -> Image {
        let mut pixels = vec![OUTSIDE; usize::try_from(width * height).expect("a small photo")];
        for y in 0..page.1 {
            for x in 0..page.0 {
                let shade = u8::try_from((x * 7 + y * 3) % 200).expect("under 200");
                pixels[index(width, at.0 + x, at.1 + y)] = [shade, shade, 0xFF];
            }
        }
        for &(x, y, color) in then {
            pixels[index(width, x, y)] = color;
        }
        let bgra = pixels
            .into_iter()
            .flat_map(|[blue, green, red]| [blue, green, red, 0])
            .collect();
        Image::new(width, height, bgra).expect("the pixels fill the photo")
    }

    fn index(width: u32, x: u32, y: u32) -> usize {
        usize::try_from(y * width + x).expect("a small photo")
    }

    /// The pixel at (x, y) of `image`, as blue, green, red.
    fn at(image: &Image, x: u32, y: u32) -> [u8; 3] {
        let [blue, green, red, _] = image.pixels()[index(image.width(), x, y)];
        [blue, green, red]
    }

    fn at_125() -> Scale {
        Scale::from_dpi(120).expect("120 DPI")
    }

    #[test]
    fn the_page_at_125_percent_is_cropped_to_1500_by_900_pixel_for_pixel() {
        let image = photo((1600, 1000), (20, 30), (1500, 900), &[]);
        let page = page_in(&image, at_125()).expect("the page");
        assert_eq!((page.width(), page.height()), (1500, 900));
        for (x, y) in [(0, 0), (1499, 0), (0, 899), (1499, 899), (733, 401)] {
            assert_eq!(at(&page, x, y), at(&image, 20 + x, 30 + y), "({x}, {y})");
        }
    }

    #[test]
    fn at_100_percent_the_page_is_1200_by_720() {
        let image = photo((1300, 800), (16, 16), (1200, 720), &[]);
        let page = page_in(&image, Scale::HUNDRED_PERCENT).expect("the page");
        assert_eq!((page.width(), page.height()), (1200, 720));
        assert_eq!(at(&page, 5, 9), at(&image, 21, 25));
    }

    #[test]
    fn a_page_one_pixel_narrower_than_the_scale_makes_it_is_refused() {
        // Chrome at another zoom, or the monitor at another scale, draws the page at another size.
        let image = photo((1600, 1000), (20, 30), (1499, 900), &[]);
        let refused = page_in(&image, at_125()).expect_err("no page of 1500 × 900");
        assert!(refused.contains("1500 × 900"), "{refused}");
    }

    #[test]
    fn a_window_over_the_magenta_around_the_page_makes_it_refused() {
        // One pixel of the ring just below the page, as a window covering the page's edge would leave it.
        let image = photo((1600, 1000), (20, 30), (1500, 900), &[(700, 930, WHITE)]);
        assert!(page_in(&image, at_125()).is_err());
    }

    #[test]
    fn magenta_inside_the_page_makes_it_refused() {
        // Nothing on the page is magenta, so a page with some isn't the page as test-page.html draws it.
        let image = photo((1600, 1000), (20, 30), (1500, 900), &[(800, 500, OUTSIDE)]);
        let refused = page_in(&image, at_125()).expect_err("magenta inside");
        assert!(refused.contains("(800, 500)"), "{refused}");
    }

    #[test]
    fn a_photo_with_no_magenta_or_too_small_for_the_page_is_refused() {
        let all_page = photo((1502, 902), (0, 0), (1502, 902), &[]);
        assert!(page_in(&all_page, at_125()).is_err());
        let small = photo((800, 600), (10, 10), (700, 500), &[]);
        assert!(page_in(&small, at_125()).is_err());
    }

    #[test]
    fn a_capture_is_named_by_its_monitors_resolution_and_scale() {
        let at = |dpi| Scale::from_dpi(dpi).expect("a DPI");
        assert_eq!(
            file_name(3840, 2160, at(168)).as_deref(),
            Some("3840x2160-175.png")
        );
        assert_eq!(
            file_name(1920, 1080, at(96)).as_deref(),
            Some("1920x1080-100.png")
        );
        assert_eq!(file_name(1920, 1080, at(101)), None);
        assert_eq!(
            parse_file_name("1920x1080-125.png"),
            Some((1920, 1080, at(120)))
        );
        for odd in [
            "notes.png",
            "1920x1080-125.jpg",
            "1920x1080.png",
            "axb-100.png",
            "1x1-101.png",
        ] {
            assert_eq!(parse_file_name(odd), None, "{odd}");
        }
    }

    #[test]
    fn the_repository_holds_the_three_captures_and_nothing_else() {
        // Only the page's captures may enter the public repository: a stray file could be a capture of anything.
        let mut names: Vec<String> = std::fs::read_dir(folder().join("captures"))
            .expect("test-page/captures")
            .map(|entry| {
                entry
                    .expect("an entry of test-page/captures")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        names.sort();
        let mut wanted = CAPTURES.map(String::from).to_vec();
        wanted.sort();
        assert_eq!(names, wanted);
    }

    #[test]
    fn each_capture_is_the_page_alone_at_its_monitors_scale() {
        crate::com::ensure_started().expect("COM starts on a test thread");
        for name in CAPTURES {
            let (_, _, scale) = parse_file_name(name).expect("a capture's name");
            let bytes = std::fs::read(folder().join("captures").join(name)).expect("the capture");
            let decoded = png::read_back::decode(&bytes);
            assert_eq!(
                Some((decoded.width, decoded.height)),
                page_size(scale),
                "{name}: the page's size at its scale"
            );
            assert!(
                decoded.stored_as_24_bit_color,
                "{name}: opaque 24-bit color"
            );
            // Not one pixel of the magenta around the page: the crop kept the page alone.
            assert!(
                !pixels_of(&decoded).contains(&OUTSIDE),
                "{name} holds magenta, from outside the page"
            );
        }
    }

    #[test]
    fn test_page_html_draws_the_stage_at_the_size_the_crop_looks_for() {
        let html = std::fs::read_to_string(folder().join("test-page.html")).expect("the page");
        let stage = html
            .split_once("#stage {")
            .and_then(|(_, rest)| rest.split_once('}'))
            .map(|(stage, _)| stage)
            .expect("the stage's style");
        for wanted in [
            format!("width: {PAGE_WIDTH}px;"),
            format!("height: {PAGE_HEIGHT}px;"),
            String::from("background: #FFFFFF;"),
        ] {
            assert!(stage.contains(&wanted), "#stage has {wanted}");
        }
        assert!(html.contains(&format!("<title>{TITLE}</title>")));
        // Magenta around the stage, and none on it.
        assert!(html.contains("background: #FF00FF;"));
        let body = html
            .split_once("<body>")
            .expect("the page's body")
            .1
            .to_lowercase();
        for magenta in ["ff00ff", "#f0f", "magenta", "fuchsia", "255, 0, 255"] {
            assert!(!body.contains(magenta), "the stage has {magenta}");
        }
    }

    #[test]
    fn a_scale_where_the_page_is_not_whole_pixels_is_refused() {
        // 1200 CSS px at 101 DPI is 1262.5 physical pixels: no Windows scale does that.
        let image = photo((1400, 900), (16, 16), (1262, 757), &[]);
        let odd = Scale::from_dpi(101).expect("101 DPI");
        assert!(page_in(&image, odd).is_err());
    }
}
