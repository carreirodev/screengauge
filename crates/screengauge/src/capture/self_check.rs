//! The self-check on the real screen (v1 spec WPC-296, test layer 3; ticket WPC-330): it paints a card with shapes of
//! known size on every monitor, photographs the screen through ScreenGauge's own capture, measures each shape through
//! the session core, and checks that every pixel of each card in the photo is as painted. Two photos are checked per
//! monitor: the app's (DXGI, or GDI where DXGI fails) and GDI's, which the app takes only where DXGI fails, so it would
//! otherwise go unchecked on a computer where DXGI never does.
//!
//! It is test code, so it never reaches ScreenGauge.exe, and it is ignored, so neither CI nor the pre-commit hook runs
//! it: neither has a real monitor. Run it on the owner's computer, with the monitors as they are:
//!
//! `cargo test -p screengauge --release --bin ScreenGauge self_check -- --ignored --nocapture`

use std::fmt;
use std::time::{Duration, Instant};

use screengauge_core::session::toolbar;
use screengauge_core::session::{
    Colors, Event, Held, Input, LocalTime, Measurement, Opening, Photo, Point, Rect, Session, Span,
};
use screengauge_core::settings::{DimensionsMode, Options, Remembered, Tool};
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{
    DWMWA_TRANSITIONS_FORCEDISABLED, DwmFlush, DwmSetWindowAttribute,
};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, ClientToScreen, CreateSolidBrush, DeleteObject, EndPaint, FillRect, HDC,
    PAINTSTRUCT, UpdateWindow,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{
    AreDpiAwarenessContextsEqual, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForWindow,
    GetThreadDpiAwarenessContext,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetClientRect, MSG,
    PM_REMOVE, PeekMessageW, RegisterClassExW, SW_SHOWNOACTIVATE, ShowWindow, TranslateMessage,
    UnregisterClassW, WM_ERASEBKGND, WM_PAINT, WNDCLASSEXW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::{BOOL, Error, PCWSTR, w};

use super::{Monitor, Shot, every_shot, fits, gdi, monitors, photo};
use crate::win32::{self, struct_size};

/// The card's client area, in physical pixels: the same on every monitor, and big enough for its shapes up to 500%,
/// Windows' highest scale.
const CARD_WIDTH: u32 = 600;
const CARD_HEIGHT: u32 = 520;
/// The card's background, red, green, blue.
const WHITE: [u8; 3] = [0xFF, 0xFF, 0xFF];

/// A shape on the card.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Shape {
    name: &'static str,
    /// On the card, its top-left corner at (0, 0), in physical pixels.
    rect: Rect,
    /// Red, green, blue.
    color: [u8; 3],
    /// Its width and height in logical pixels, for a shape drawn to a size in logical pixels: what its label's logical
    /// pixels must round to.
    logical: Option<(u32, u32)>,
}

/// The shapes on a card painted at `dpi`: one of a fixed size in physical pixels, and one of a fixed size in logical
/// pixels, scaled by the app's own rule, so the photo's place and the monitor's scale both show in what is measured.
fn shapes(dpi: u32) -> [Shape; 2] {
    let (width, height) = (toolbar::scaled(100, dpi), toolbar::scaled(50, dpi));
    [
        Shape {
            name: "blue, 300 × 200 physical px",
            rect: Rect {
                left: 20,
                top: 20,
                width: 300,
                height: 200,
            },
            color: [0x1E, 0x90, 0xFF],
            logical: None,
        },
        // Below the blue one, and inside the card up to 500%: 500 × 250 from row 240.
        Shape {
            name: "orange, 100 × 50 logical px",
            rect: Rect {
                left: 20,
                top: 240,
                width,
                height,
            },
            color: [0xFF, 0x8C, 0x00],
            logical: Some((100, 50)),
        },
    ]
}

/// The card as it is on one monitor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Card {
    /// The monitor, as an index in the order Windows lists them: the frozen screen's order.
    monitor: usize,
    /// Its client area's top-left corner, in physical pixels of the virtual screen.
    left: i32,
    top: i32,
    /// The DPI its window was painted at.
    dpi: u32,
}

impl Card {
    /// Its client area, in physical pixels of the virtual screen.
    fn rect(self) -> Rect {
        Rect {
            left: self.left,
            top: self.top,
            width: CARD_WIDTH,
            height: CARD_HEIGHT,
        }
    }

    /// Its shapes, in physical pixels of the virtual screen.
    fn shapes(self) -> [Shape; 2] {
        shapes(self.dpi).map(|shape| Shape {
            rect: Rect {
                left: self.left + shape.rect.left,
                top: self.top + shape.rect.top,
                ..shape.rect
            },
            ..shape
        })
    }

    /// The color painted at `at`, a pixel of the card in physical pixels of the virtual screen.
    fn color_at(self, at: Point) -> [u8; 3] {
        self.shapes()
            .into_iter()
            .find(|shape| shape.rect.contains(at))
            .map_or(WHITE, |shape| shape.color)
    }
}

/// One thing the self-check checked, and what it found.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Check {
    what: String,
    passed: bool,
    found: String,
}

impl Check {
    fn new(what: impl Into<String>, passed: bool, found: impl Into<String>) -> Check {
        Check {
            what: what.into(),
            passed,
            found: found.into(),
        }
    }
}

impl fmt::Display for Check {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mark = if self.passed { "PASS" } else { "FAIL" };
        write!(f, "{mark}  {}: {}", self.what, self.found)
    }
}

/// Checks one path's photos of every monitor against the cards painted on them: for each card, every pixel, and each
/// shape as Dimensions measures and labels it in Cross mode with the cursor at its center. `photos` are in the order
/// Windows lists the monitors, `None` for a monitor the path couldn't photograph. The checks come back per card.
fn check_photos(photos: Vec<Option<Photo>>, cards: &[Card]) -> Vec<Vec<Check>> {
    let mut checks: Vec<Vec<Check>> = cards
        .iter()
        .map(|card| photo_checks(photos.get(card.monitor).and_then(Option::as_ref), *card))
        .collect();
    let mut opened = match cards.first() {
        None => return checks,
        Some(first) => Session::open(Opening {
            frozen_screen: photos,
            cursor: center(first.shapes()[0].rect),
            options: Options::default(),
            remembered: Remembered {
                last_tool: Tool::Dimensions,
                dimensions_mode: DimensionsMode::Cross,
                ..Remembered::default()
            },
            second_click_until: None,
        }),
    };
    for (card, checks) in cards.iter().zip(&mut checks) {
        for shape in card.shapes() {
            let what = format!("measures {}", shape.name);
            checks.push(match &mut opened {
                Ok(session) => {
                    measure_check(session_at(session, center(shape.rect)), *card, shape, what)
                }
                Err(refused) => Check::new(
                    what,
                    false,
                    format!("the session refused the photos: {refused}"),
                ),
            });
        }
    }
    checks
}

/// The pixel check of one card's photo. The photo's scale needs no check of its own: the labels' logical pixels show it.
fn photo_checks(photo: Option<&Photo>, card: Card) -> Vec<Check> {
    match photo {
        Some(photo) => vec![identical_check(photo, card)],
        None => vec![Check::new(
            "photo",
            false,
            "this path took no photo of the monitor",
        )],
    }
}

/// Whether every pixel of the card in the photo is the color painted there.
fn identical_check(photo: &Photo, card: Card) -> Check {
    let what = "identical to the screen";
    let rect = card.rect();
    let inside = photo.rect.contains(Point {
        x: rect.left,
        y: rect.top,
    }) && photo.rect.contains(Point {
        x: rect.right(),
        y: rect.bottom(),
    });
    if !inside {
        return Check::new(
            what,
            false,
            format!(
                "the card {rect:?} isn't all inside the photo {:?}",
                photo.rect
            ),
        );
    }
    let width = usize::try_from(photo.rect.width).expect("a photo narrower than memory");
    let pixels = photo.image.pixels();
    let mut differing = 0_u64;
    let mut first = None;
    for y in rect.top..=rect.bottom() {
        for x in rect.left..=rect.right() {
            let at = Point { x, y };
            let column = usize::try_from(x - photo.rect.left).expect("inside the photo");
            let row = usize::try_from(y - photo.rect.top).expect("inside the photo");
            let [blue, green, red, _] = pixels[row * width + column];
            let painted = card.color_at(at);
            if [red, green, blue] != painted {
                differing += 1;
                first.get_or_insert((at, [red, green, blue], painted));
            }
        }
    }
    let total = u64::from(CARD_WIDTH) * u64::from(CARD_HEIGHT);
    match first {
        None => Check::new(what, true, format!("{total} of {total} pixels as painted")),
        Some((at, photographed, painted)) => Check::new(
            what,
            false,
            format!(
                "{differing} of {total} pixels differ; the first at ({}, {}) is {} where {} was painted",
                at.x,
                at.y,
                hex(photographed),
                hex(painted)
            ),
        ),
    }
}

/// The live measurement once the mouse moves to `at`, or why there is none.
fn session_at(session: &mut Session, at: Point) -> Result<Measurement, String> {
    if session.toolbar_hit(at).is_some() {
        return Err(format!("the toolbar covers ({}, {})", at.x, at.y));
    }
    session.handle(Event {
        cursor: at,
        held: Held::default(),
        // No measurement depends on the time.
        time: LocalTime {
            year: 2026,
            month: 11,
            day: 3,
            hour: 12,
            minute: 0,
            second: 0,
        },
        uptime: Duration::from_secs(60),
        input: Input::Move,
    });
    session
        .scene()
        .live
        .cloned()
        .ok_or_else(|| format!("Dimensions measured nothing at ({}, {})", at.x, at.y))
}

/// Whether `measured` is the whole of `shape`, on the card's monitor, and its label says so: the physical pixels exactly,
/// and the logical ones at the card's scale, to the tenth the label shows them.
fn measure_check(
    measured: Result<Measurement, String>,
    card: Card,
    shape: Shape,
    what: String,
) -> Check {
    let measured = match measured {
        Ok(measured) => measured,
        Err(why) => return Check::new(what, false, why),
    };
    let rect = shape.rect;
    let (x, y) = (center(rect).x, center(rect).y);
    let expected = (
        Some(Span {
            first: Point { x: rect.left, y },
            last: Point { x: rect.right(), y },
        }),
        Some(Span {
            first: Point { x, y: rect.top },
            last: Point {
                x,
                y: rect.bottom(),
            },
        }),
    );
    let label = &measured.label;
    let passed = measured.monitor == card.monitor
        && (measured.horizontal, measured.vertical) == expected
        && label.physical == format!("{} × {}", rect.width, rect.height)
        && logical_label_fits(label.logical.as_deref(), shape, card.dpi);
    let shown = match &label.logical {
        Some(logical) => format!("{} | {logical}", label.physical),
        None => label.physical.clone(),
    };
    let found = match (measured.horizontal, measured.vertical) {
        (Some(horizontal), Some(vertical)) => format!(
            "{} × {} from ({}, {}), label \"{shown}\"; expected {} × {} from ({}, {}) at {} DPI",
            horizontal.length(),
            vertical.length(),
            horizontal.first.x,
            vertical.first.y,
            rect.width,
            rect.height,
            rect.left,
            rect.top,
            card.dpi
        ),
        _ => format!("not a Cross measurement: {measured:?}"),
    };
    Check::new(what, passed, found)
}

/// Whether a label's logical pixels are the shape's physical width and height at `dpi`, each to the tenth, and, for a
/// shape drawn to a size in logical pixels, round to that size. At 100% the label has none.
fn logical_label_fits(logical: Option<&str>, shape: Shape, dpi: u32) -> bool {
    let Some(logical) = logical else {
        return dpi == 96;
    };
    let lengths: Vec<Option<f64>> = logical
        .split(" × ")
        .map(|length| length.parse().ok())
        .collect();
    let [Some(width), Some(height)] = lengths[..] else {
        return false;
    };
    let at_scale = |physical: u32| f64::from(physical) * 96.0 / f64::from(dpi);
    // The label rounds to the tenth: half a tenth off at most, and a hair more for the float.
    let to_the_tenth = |shown: f64, physical: u32| (shown - at_scale(physical)).abs() <= 0.051;
    let drawn = shape.logical.is_none_or(|(w, h)| {
        (width - f64::from(w)).abs() < 0.5 && (height - f64::from(h)).abs() < 0.5
    });
    dpi != 96
        && to_the_tenth(width, shape.rect.width)
        && to_the_tenth(height, shape.rect.height)
        && drawn
}

/// The pixel at the center of `rect`, rounded up and left.
fn center(rect: Rect) -> Point {
    Point {
        x: (rect.left + rect.right()) / 2,
        y: (rect.top + rect.bottom()) / 2,
    }
}

/// Red, green, blue as `#RRGGBB`.
fn hex([red, green, blue]: [u8; 3]) -> String {
    format!("#{red:02X}{green:02X}{blue:02X}")
}

/// What a photo's colors are, for the report.
fn colors(photo: &Photo) -> &'static str {
    match photo.colors {
        Colors::Exact => "exact colors",
        Colors::Approximate => "approximate colors",
    }
}

/// One path's photo of each monitor, with how it was taken, for the report.
struct Taken {
    how: Vec<String>,
    photos: Vec<Option<Photo>>,
}

/// The app's photos, as [`super::every_monitor`] takes them, with the path each one came through.
fn app_photos(shots: Vec<Shot>) -> Taken {
    let (how, photos) = shots
        .into_iter()
        .map(|shot| match shot {
            Shot::Dxgi(photo) => (format!("through DXGI, {}", colors(&photo)), Some(photo)),
            Shot::Gdi { photo, dxgi } => (
                format!(
                    "through GDI, since DXGI failed ({dxgi}), {}",
                    colors(&photo)
                ),
                Some(photo),
            ),
            Shot::Failed { dxgi, gdi } => (
                format!("none: DXGI failed ({dxgi}), and GDI failed ({gdi})"),
                None,
            ),
        })
        .unzip();
    Taken { how, photos }
}

/// GDI's photo of every monitor, as the app takes it where DXGI fails.
fn gdi_photos(monitors: &[Monitor]) -> Taken {
    let (how, photos) = monitors
        .iter()
        .map(|monitor| {
            match gdi::image(monitor.rect).and_then(|image| fits(monitor, image)) {
                // Its colors don't matter here: the check is whether they are as painted.
                Ok(image) => (
                    String::from("through GDI"),
                    Some(photo(monitor, image, Colors::Exact)),
                ),
                Err(e) => (format!("none: GDI failed ({e})"), None),
            }
        })
        .unzip();
    Taken { how, photos }
}

/// The cards on screen, one per monitor, in the monitors' order. Dropping it takes them off the screen, so a failing
/// check never leaves a window on top of the owner's.
struct Windows {
    instance: HINSTANCE,
    windows: Vec<HWND>,
}

const CLASS_NAME: PCWSTR = w!("ScreenGauge.SelfCheck");

impl Windows {
    /// A card centered on each monitor, on top of everything, never taking the keyboard.
    fn show(monitors: &[Monitor]) -> windows::core::Result<Windows> {
        // SAFETY: this program's own module, whose handle needs no release.
        let instance: HINSTANCE = unsafe { GetModuleHandleW(None) }?.into();
        let class = WNDCLASSEXW {
            cbSize: const { struct_size::<WNDCLASSEXW>() },
            lpfnWndProc: Some(window_proc),
            hInstance: instance,
            lpszClassName: CLASS_NAME,
            ..Default::default()
        };
        // SAFETY: `class` is fully initialized, and its name is a static string.
        if unsafe { RegisterClassExW(&class) } == 0 {
            return Err(Error::from_thread());
        }
        let mut shown = Windows {
            instance,
            windows: Vec::new(),
        };
        let (width, height) = (CARD_WIDTH.cast_signed(), CARD_HEIGHT.cast_signed());
        for monitor in monitors {
            let rect = monitor.rect;
            // SAFETY: the class is registered above. Made at its place on the monitor, the window starts at that
            // monitor's DPI and gets no WM_DPICHANGED.
            let window = unsafe {
                CreateWindowExW(
                    WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                    CLASS_NAME,
                    w!("ScreenGauge self-check"),
                    WS_POPUP,
                    rect.left + (rect.right - rect.left - width) / 2,
                    rect.top + (rect.bottom - rect.top - height) / 2,
                    width,
                    height,
                    None,
                    None,
                    Some(instance),
                    None,
                )
            }?;
            shown.windows.push(window);
            // Shown at once, as the tray menu is: a fade the photo caught would fail a correct screen.
            let no_fade = BOOL::from(true);
            // SAFETY: the window just made; the value outlives the call, which reads exactly its size.
            unsafe {
                DwmSetWindowAttribute(
                    window,
                    DWMWA_TRANSITIONS_FORCEDISABLED,
                    (&raw const no_fade).cast(),
                    const { struct_size::<BOOL>() },
                )
            }?;
            // SAFETY: the window was just made on this thread. ShowWindow returns whether it was visible, not an error.
            unsafe {
                let _ = ShowWindow(window, SW_SHOWNOACTIVATE);
                UpdateWindow(window).ok()?;
            }
        }
        // Painted, then composed twice, so the screen holds the cards before the photos.
        pump_messages();
        for _ in 0..2 {
            // SAFETY: a plain call that waits for the next composition.
            unsafe { DwmFlush() }?;
            pump_messages();
        }
        Ok(shown)
    }

    /// Where each card is on the virtual screen and at which DPI it was painted, with the check that its window is on
    /// its monitor at the card's size and at the monitor's scale.
    fn cards(&self, monitors: &[Monitor]) -> windows::core::Result<Vec<(Card, Check)>> {
        self.windows
            .iter()
            .zip(monitors)
            .enumerate()
            .map(|(index, (&window, monitor))| {
                let mut client = RECT::default();
                let mut origin = POINT::default();
                // SAFETY: plain queries on a live window of this thread; both out values are locals.
                unsafe {
                    GetClientRect(window, &mut client)?;
                    ClientToScreen(window, &mut origin).ok()?;
                }
                // SAFETY: a plain query on a live window.
                let dpi = unsafe { GetDpiForWindow(window) };
                let card = Card {
                    monitor: index,
                    left: origin.x,
                    top: origin.y,
                    dpi,
                };
                let size = (client.right - client.left, client.bottom - client.top);
                let fits = (size.0.cast_unsigned(), size.1.cast_unsigned())
                    == (CARD_WIDTH, CARD_HEIGHT)
                    && win32::contains(&monitor.rect, origin)
                    && dpi == monitor.scale.dpi();
                let check = Check::new(
                    "card window",
                    fits,
                    format!(
                        "{} × {} at ({}, {}), {dpi} DPI; the monitor is at {} DPI",
                        size.0,
                        size.1,
                        origin.x,
                        origin.y,
                        monitor.scale.dpi()
                    ),
                );
                Ok((card, check))
            })
            .collect()
    }
}

impl Drop for Windows {
    fn drop(&mut self) {
        for window in self.windows.drain(..) {
            // SAFETY: a window made on this thread, destroyed once.
            if let Err(e) = unsafe { DestroyWindow(window) } {
                eprintln!("the self-check couldn't destroy a card's window: {e}");
            }
        }
        // SAFETY: the class registered in `show`, whose windows are all destroyed above.
        if let Err(e) = unsafe { UnregisterClassW(CLASS_NAME, Some(self.instance)) } {
            eprintln!("the self-check couldn't unregister its window class: {e}");
        }
        pump_messages();
    }
}

/// Handles every message waiting for this thread's windows.
fn pump_messages() {
    let mut message = MSG::default();
    // SAFETY: `message` is a local that each call fills; the messages are this thread's own.
    while unsafe { PeekMessageW(&mut message, None, 0, 0, PM_REMOVE) }.as_bool() {
        // SAFETY: as above. TranslateMessage returns whether it translated, not an error.
        unsafe {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}

extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        // WM_PAINT fills every pixel, so the background is never drawn first.
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => {
            paint(window);
            LRESULT(0)
        }
        // SAFETY: Windows' handling of every other message, with its own arguments.
        _ => unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
}

/// Paints the card, white with its shapes, at the window's DPI. A failure goes to the output, and the pixels checked
/// later show it too.
fn paint(window: HWND) {
    let mut paint = PAINTSTRUCT::default();
    // SAFETY: BeginPaint and EndPaint bracket the painting of the window being painted.
    let dc = unsafe { BeginPaint(window, &mut paint) };
    if dc.is_invalid() {
        eprintln!("the self-check couldn't paint a card: BeginPaint failed");
        return;
    }
    // SAFETY: a plain query on the window being painted.
    let dpi = unsafe { GetDpiForWindow(window) };
    let whole = Shape {
        name: "card",
        rect: Rect {
            left: 0,
            top: 0,
            width: CARD_WIDTH,
            height: CARD_HEIGHT,
        },
        color: WHITE,
        logical: None,
    };
    for shape in std::iter::once(whole).chain(shapes(dpi)) {
        fill(dc, shape);
    }
    // SAFETY: the pair of the BeginPaint above. EndPaint always returns nonzero.
    let _ = unsafe { EndPaint(window, &paint) };
}

fn fill(dc: HDC, shape: Shape) {
    let [red, green, blue] = shape.color.map(u32::from);
    let rect = RECT {
        left: shape.rect.left,
        top: shape.rect.top,
        right: shape.rect.right() + 1,
        bottom: shape.rect.bottom() + 1,
    };
    // SAFETY: a brush made here and deleted right after use, on the paint DC of the window being painted.
    unsafe {
        let brush = CreateSolidBrush(COLORREF(red | green << 8 | blue << 16));
        if brush.is_invalid() {
            eprintln!(
                "the self-check couldn't paint {}: CreateSolidBrush failed",
                shape.name
            );
            return;
        }
        if FillRect(dc, &rect, brush) == 0 {
            eprintln!(
                "the self-check couldn't paint {}: FillRect failed",
                shape.name
            );
        }
        if !DeleteObject(brush.into()).as_bool() {
            eprintln!("the self-check couldn't delete the brush of {}", shape.name);
        }
    }
}

/// Whether this thread is Per-Monitor V2, as ScreenGauge.exe's manifest makes it: only then are the monitors, the
/// windows and the photos all in physical pixels.
pub(super) fn per_monitor_v2() -> bool {
    // SAFETY: plain calls that compare two DPI awareness contexts.
    unsafe {
        AreDpiAwarenessContextsEqual(
            GetThreadDpiAwarenessContext(),
            DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        )
        .as_bool()
    }
}

/// One path's part of the report.
struct Section {
    name: &'static str,
    /// How the path photographed each monitor, in the monitors' order.
    how: Vec<String>,
    /// The checks of each monitor's photo, in the monitors' order.
    checks: Vec<Vec<Check>>,
}

impl Section {
    fn new(name: &'static str, taken: Taken, cards: &[Card]) -> Section {
        Section {
            name,
            how: taken.how,
            checks: check_photos(taken.photos, cards),
        }
    }
}

/// The report: each monitor's checks under it, then the time and the result. Returns it with how many checks failed.
fn report(
    monitors: &[Monitor],
    sections: &[Section],
    windows: &[Check],
    time: &Check,
) -> (String, usize) {
    // A monitor missing from any list would get fewer checks, and could pass with none.
    assert_eq!(windows.len(), monitors.len(), "a card window per monitor");
    for section in sections {
        assert_eq!(section.how.len(), monitors.len(), "{}", section.name);
        assert_eq!(section.checks.len(), monitors.len(), "{}", section.name);
    }
    let mut text = String::new();
    let mut total = 0;
    let mut failed = 0;
    let mut add = |text: &mut String, indent: &str, check: &Check| {
        total += 1;
        failed += usize::from(!check.passed);
        text.push_str(&format!("{indent}{check}\n"));
    };
    for (index, monitor) in monitors.iter().enumerate() {
        let rect = monitor.rect;
        text.push_str(&format!(
            "{}: {} × {} at ({}, {}), {} DPI\n",
            monitor.device,
            rect.right - rect.left,
            rect.bottom - rect.top,
            rect.left,
            rect.top,
            monitor.scale.dpi()
        ));
        add(&mut text, "  ", &windows[index]);
        for section in sections {
            text.push_str(&format!("  {}: {}\n", section.name, section.how[index]));
            for check in &section.checks[index] {
                add(&mut text, "    ", check);
            }
        }
    }
    add(&mut text, "", time);
    if failed == 0 {
        text.push_str(&format!("RESULT: PASS, all {total} checks\n"));
    } else {
        text.push_str(&format!("RESULT: FAIL, {failed} of {total} checks\n"));
    }
    (text, failed)
}

#[test]
#[ignore = "paints on and photographs the real screen, so it runs on the owner's computer, never in CI"]
fn self_check() {
    let start = Instant::now();
    assert!(
        per_monitor_v2(),
        "the test program isn't Per-Monitor V2, so its pixels wouldn't be physical: the manifest didn't reach it"
    );
    let monitors = monitors().expect("the monitors");
    let shown = Windows::show(&monitors).expect("a card on every monitor");
    let (cards, windows): (Vec<Card>, Vec<Check>) = shown
        .cards(&monitors)
        .expect("where the cards are")
        .into_iter()
        .unzip();
    let (photographed, shots) = every_shot().expect("the monitors, for the app's photos");
    let gdi = gdi_photos(&monitors);
    drop(shown);
    let devices = |list: &[Monitor]| {
        list.iter()
            .map(|monitor| monitor.device.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        devices(&photographed),
        devices(&monitors),
        "the monitors changed during the self-check"
    );
    let sections = [
        Section::new("ScreenGauge's photo", app_photos(shots), &cards),
        Section::new("GDI's photo", gdi, &cards),
    ];
    let elapsed = start.elapsed();
    let time = Check::new(
        "time",
        elapsed < Duration::from_secs(1),
        format!(
            "{} ms, from the start to the last check; it must stay under 1 s",
            elapsed.as_millis()
        ),
    );
    let (text, failed) = report(&monitors, &sections, &windows, &time);
    println!("{text}");
    assert_eq!(failed, 0, "the self-check failed; the report is above");
}

mod tests {
    use screengauge_core::session::{Image, Scale};

    use super::*;

    /// The owner's 4K monitor at 175%, shrunk so the tests stay quick: Windows' main monitor.
    const MAIN: Rect = Rect {
        left: 0,
        top: 0,
        width: 960,
        height: 720,
    };
    /// The owner's Full HD monitor at 125%, shrunk the same way, to the left of the main one.
    const LEFT: Rect = Rect {
        left: -800,
        top: 0,
        width: 800,
        height: 600,
    };

    /// The cards as the self-check places them: centered on each monitor.
    fn cards() -> [Card; 2] {
        [
            Card {
                monitor: 0,
                left: 180,
                top: 160,
                dpi: 168,
            },
            Card {
                monitor: 1,
                left: -700,
                top: 40,
                dpi: 120,
            },
        ]
    }

    /// The photo of a monitor in white, with `cards` painted on it as they would be on screen.
    fn photo(monitor: Rect, dpi: u32, cards: &[Card]) -> Photo {
        let mut bgra = Vec::new();
        for y in monitor.top..=monitor.bottom() {
            for x in monitor.left..=monitor.right() {
                let at = Point { x, y };
                let [red, green, blue] = cards
                    .iter()
                    .find(|card| card.rect().contains(at))
                    .map_or(WHITE, |card| card.color_at(at));
                bgra.extend_from_slice(&[blue, green, red, 255]);
            }
        }
        Photo {
            rect: monitor,
            scale: Scale::from_dpi(dpi).expect("a real scale"),
            colors: Colors::Exact,
            image: Image::new(monitor.width, monitor.height, bgra)
                .expect("the pixels fill the image"),
        }
    }

    fn photos(cards: &[Card]) -> Vec<Option<Photo>> {
        vec![Some(photo(MAIN, 168, cards)), Some(photo(LEFT, 120, cards))]
    }

    fn failed(checks: &[Check]) -> Vec<&str> {
        checks
            .iter()
            .filter(|check| !check.passed)
            .map(|check| check.what.as_str())
            .collect()
    }

    #[test]
    fn a_photo_of_the_cards_as_painted_passes_every_check_on_both_monitors() {
        let checks = check_photos(photos(&cards()), &cards());
        for (monitor, checks) in checks.iter().enumerate() {
            assert_eq!(
                failed(checks),
                Vec::<&str>::new(),
                "monitor {monitor}: {checks:#?}"
            );
        }
        let found: Vec<&str> = checks[0].iter().map(|check| check.found.as_str()).collect();
        assert_eq!(
            found,
            [
                "312000 of 312000 pixels as painted",
                "300 × 200 from (200, 180), label \"300 × 200 | 171.4 × 114.3\"; expected 300 × 200 from (200, 180) at 168 DPI",
                "175 × 88 from (200, 400), label \"175 × 88 | 100 × 50.3\"; expected 175 × 88 from (200, 400) at 168 DPI",
            ]
        );
        assert_eq!(
            checks[1][2].found,
            "125 × 63 from (-680, 280), label \"125 × 63 | 100 × 50.4\"; expected 125 × 63 from (-680, 280) at 120 DPI"
        );
    }

    #[test]
    fn a_photo_one_pixel_off_fails_the_pixels_and_both_measurements_of_that_monitor_only() {
        // The photo holds the main monitor's card one column to the right of where it was painted.
        let [main, left] = cards();
        let shifted = Card {
            left: main.left + 1,
            ..main
        };
        let checks = check_photos(photos(&[shifted, left]), &cards());
        assert_eq!(
            failed(&checks[0]),
            [
                "identical to the screen",
                "measures blue, 300 × 200 physical px",
                "measures orange, 100 × 50 logical px"
            ]
        );
        // Each shape's first and last column differ on every one of its rows: 2 × 200 + 2 × 88.
        assert_eq!(
            checks[0][0].found,
            "576 of 312000 pixels differ; the first at (200, 180) is #FFFFFF where #1E90FF was painted"
        );
        assert_eq!(
            checks[0][1].found,
            "300 × 200 from (201, 180), label \"300 × 200 | 171.4 × 114.3\"; expected 300 × 200 from (200, 180) at 168 DPI"
        );
        assert_eq!(failed(&checks[1]), Vec::<&str>::new());
    }

    #[test]
    fn one_wrong_pixel_off_the_measured_lines_fails_only_the_pixels() {
        let mut photos = photos(&cards());
        let photo = photos[1].as_mut().expect("the left monitor's photo");
        let mut bgra = photo.image.bgra().to_vec();
        // (-500, 450): white on the left card, below the orange shape and away from both centers' row and column.
        let at = (450 * 800 + 300) * 4;
        bgra[at..at + 3].copy_from_slice(&[0xFE, 0xFF, 0xFF]);
        photo.image = Image::new(800, 600, bgra).expect("the same size");
        let checks = check_photos(photos, &cards());
        assert_eq!(failed(&checks[0]), Vec::<&str>::new());
        assert_eq!(failed(&checks[1]), ["identical to the screen"]);
        assert_eq!(
            checks[1][0].found,
            "1 of 312000 pixels differ; the first at (-500, 450) is #FFFFFE where #FFFFFF was painted"
        );
    }

    #[test]
    fn a_monitor_with_no_photo_fails_and_no_shape_is_measured_anywhere() {
        // All or nothing: the session core opens on no part of the screen, so nothing measures on the other monitor.
        let mut photos = photos(&cards());
        photos[1] = None;
        let checks = check_photos(photos, &cards());
        assert_eq!(
            failed(&checks[1]),
            [
                "photo",
                "measures blue, 300 × 200 physical px",
                "measures orange, 100 × 50 logical px"
            ]
        );
        assert_eq!(
            failed(&checks[0]),
            [
                "measures blue, 300 × 200 physical px",
                "measures orange, 100 × 50 logical px"
            ]
        );
        assert_eq!(
            checks[0][1].found,
            "the session refused the photos: monitor 1 has no photo"
        );
    }

    #[test]
    fn a_photo_at_another_scale_than_the_card_fails_both_labels() {
        // The photo says 150% where the card was painted at 175%: the spans are right, their logical pixels aren't.
        let mut photos = photos(&cards());
        photos[0].as_mut().expect("the main monitor's photo").scale =
            Scale::from_dpi(144).expect("150%");
        let checks = check_photos(photos, &cards());
        assert_eq!(
            failed(&checks[0]),
            [
                "measures blue, 300 × 200 physical px",
                "measures orange, 100 × 50 logical px"
            ]
        );
        assert_eq!(
            checks[0][1].found,
            "300 × 200 from (200, 180), label \"300 × 200 | 200 × 133.3\"; expected 300 × 200 from (200, 180) at 168 DPI"
        );
        assert_eq!(failed(&checks[1]), Vec::<&str>::new());
    }

    #[test]
    fn at_100_percent_the_labels_have_no_logical_pixels_and_pass() {
        let main = Card {
            dpi: 96,
            ..cards()[0]
        };
        let checks = check_photos(vec![Some(photo(MAIN, 96, &[main]))], &[main]);
        assert_eq!(failed(&checks[0]), Vec::<&str>::new(), "{checks:#?}");
        assert_eq!(
            checks[0][2].found,
            "100 × 50 from (200, 400), label \"100 × 50\"; expected 100 × 50 from (200, 400) at 96 DPI"
        );
    }
}
