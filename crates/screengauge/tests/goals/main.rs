//! The measure of WPC-286's speed and consumption goals (v1 spec WPC-296, test layer 3, part 2; ticket WPC-333): from
//! outside, on the very ScreenGauge.exe that is to be published, with no special build. The routine starts the exe as
//! another process, presses the global shortcut with real key events, waits for the toolbar to show on screen, and reads
//! the process's private working set and processor time. It runs a whole pass with HDR off and another with HDR on,
//! turning HDR on and off itself, each in a fresh process, then shows every goal with its limit and the values
//! measured, and says what was missed. A missed goal holds the version; an exceeded HDR allowance goes back to the owner.
//!
//! It is a test, so it never reaches ScreenGauge.exe, and it is ignored, so neither CI nor the pre-commit hook runs it:
//! neither has a real monitor or graphics card. Run it on the owner's computer, with no ScreenGauge running, nothing
//! else compiling, and nobody touching the mouse or keyboard for its 35 minutes or so:
//!
//! `cargo test -p screengauge --release --test goals -- --ignored --nocapture --test-threads=1`
//!
//! Before a version, `SCREENGAUGE_EXE` names the very file to be published. Rust builds don't come out byte for byte
//! the same, and this command builds its own `target/release/ScreenGauge.exe` with the tests' extra Windows features,
//! so without it the routine measures that build of the same code, and says so. `SCREENGAUGE_GOALS_QUICK=1` shortens
//! every wait to try the routine itself; such a run is never a release result, and it always ends failed.
//!
//! Its calls are by the global shortcut, besides the first call that opening the exe makes. The 250 ms goal holds for
//! the tray icon, the Start menu and the hidden icons panel too; those stay on the manual list before each version
//! (layer 4).
//!
//! It puts HDR, the settings file and ScreenGauge back as they were however it ends, unless its process is killed: then
//! turn HDR back in Windows' Settings and exit ScreenGauge from its tray icon.

mod app;
mod appearance;
mod hdr;
mod judge;
mod screen;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use screengauge_core::session::{Point, Rect, Scale, toolbar};
use screengauge_core::settings::{self, Shortcut, Stored};
use windows::Win32::Security::Cryptography::{BCRYPT_SHA256_ALG_HANDLE, BCryptHash};
use windows::Win32::System::Threading::GetCurrentThread;
use windows::Win32::System::WindowsProgramming::QueryThreadCycleTime;

use app::App;
use appearance::Read;
use judge::{CALLS, CpuWindow, Pass, exact_f64};

/// The stage's color, magenta, which the toolbar never draws in either theme.
const STAGE_COLOR: [u8; 3] = [0xFF, 0x00, 0xFF];
/// How long a call may take to show the toolbar before the routine gives up on it: far past every limit, so a slow
/// call is measured and judged rather than lost.
const CALL_WAIT: Duration = Duration::from_secs(5);
/// How long the tools get to close after Esc.
const CLOSE_WAIT: Duration = Duration::from_secs(3);
/// The pause after the tools closed, before the next call.
const GAP: Duration = Duration::from_millis(300);
/// How long the tools stay open in a call that reads the memory.
const HOLD_OPEN: Duration = Duration::from_millis(1500);
/// How often the memory is read while the tools are open.
const MEMORY_EVERY: Duration = Duration::from_millis(10);
/// How many calls read the memory with the tools open, apart from the timed ones.
const MEMORY_CALLS: usize = 3;
/// How long the routine rests between two reads of the pixels while it waits for the toolbar.
const READ_EVERY: Duration = Duration::from_millis(1);
/// How long ScreenGauge gets to make its resident window after starting.
const START_WAIT: Duration = Duration::from_secs(10);

/// A struct's size for a `cbSize` field. Used as `const { struct_size::<T>() }`, the assert runs at compile time. It
/// mirrors `src/win32.rs`'s, which a test can't reach: ScreenGauge is a program, with no library.
pub const fn struct_size<T>() -> u32 {
    let size = size_of::<T>();
    assert!(size <= u32::MAX as usize);
    size as u32
}

/// Text Windows wrote into a buffer of UTF-16, up to its first 0.
pub fn wide_text(buffer: &[u16]) -> String {
    let length = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
    String::from_utf16_lossy(&buffer[..length])
}

/// A size in pixels as a coordinate.
fn signed(pixels: u32) -> Result<i32, String> {
    i32::try_from(pixels).map_err(|e| format!("{pixels} pixels as a coordinate: {e}"))
}

/// The waits of the spec's conditions, or a run's shortened ones.
#[derive(Debug, Clone, Copy)]
struct Waits {
    /// After the resident window shows, before the processor window before the first call starts.
    settle_after_start: Duration,
    /// The processor window before the first call; the memory is read at its end.
    idle_before_first_call: Duration,
    /// When the memory is read after the tools closed, and after the round of calls.
    memory_after: Duration,
    /// The processor window after use.
    idle_after_use: Duration,
    /// How many calls in a row the round has.
    round: usize,
    quick: bool,
}

impl Waits {
    fn now() -> Waits {
        let quick = std::env::var_os("SCREENGAUGE_GOALS_QUICK").is_some_and(|v| v == "1");
        if quick {
            Waits {
                settle_after_start: Duration::from_secs(2),
                idle_before_first_call: Duration::from_secs(5),
                memory_after: Duration::from_secs(5),
                idle_after_use: Duration::from_secs(15),
                round: 5,
                quick,
            }
        } else {
            Waits {
                settle_after_start: Duration::from_secs(5),
                idle_before_first_call: Duration::from_secs(60),
                memory_after: Duration::from_secs(60),
                idle_after_use: Duration::from_secs(600),
                round: 100,
                quick,
            }
        }
    }
}

#[test]
#[ignore = "starts ScreenGauge, presses its shortcut and turns HDR on and off on the real screen, so it runs on the owner's computer before each version, never in CI"]
fn goals_of_wpc_286() {
    if let Err(e) = run() {
        panic!("{e}");
    }
}

fn run() -> Result<(), String> {
    let started = Instant::now();
    let waits = Waits::now();
    let exe = exe()?;
    if app::any_running() {
        return Err(
            "a ScreenGauge is running: exit it from its tray icon first, so the routine measures its own".to_string(),
        );
    }
    screen::see_physical_pixels()?;
    let settings_path = settings_path()?;
    let _settings = SettingsKept::now(&settings_path)?;
    let stored = stored_settings(&settings_path)?;
    let _hdr = hdr::Restore::now()?;
    let tsc_hz = tsc_hz();
    let spun = cycle_clock_check(tsc_hz)?;

    let monitors = screen::monitors()?;
    let monitor = monitors
        .iter()
        .find(|m| m.primary)
        .ok_or("Windows names no main monitor")?;
    let scale = Scale::from_dpi(monitor.dpi).ok_or("a monitor of 0 dpi")?;
    let layout = toolbar::layout_on(monitor.rect, scale, stored.remembered.toolbar_place);
    let cursor = appearance::cursor_spot(monitor.rect, layout.toolbar)
        .ok_or("the toolbar crosses every row and column the cursor could go to")?;
    let tools = layout.tools;
    let patch_rect = Rect {
        left: tools.left + signed(tools.width / 2)? - 1,
        top: tools.top + signed(tools.height / 2)? - 1,
        width: 3,
        height: 3,
    };
    let margin = toolbar::scaled(24, monitor.dpi);
    let stage_rect = Rect {
        left: layout.toolbar.left - signed(margin)?,
        top: layout.toolbar.top - signed(margin)?,
        width: layout.toolbar.width + 2 * margin,
        height: layout.toolbar.height + 2 * margin,
    };

    println!("== ScreenGauge goals of WPC-286 (WPC-333)");
    if waits.quick {
        println!("QUICK RUN: every wait is shortened, so this is NOT a release result.");
    }
    describe_exe(&exe)?;
    if std::env::var_os("SCREENGAUGE_EXE").is_none() {
        println!(
            "    the test build's exe: before a version, name the file to be published in SCREENGAUGE_EXE"
        );
    }
    for m in &monitors {
        println!(
            "monitor {}: {}x{} at ({}, {}), {}%{}",
            m.device,
            m.rect.width,
            m.rect.height,
            m.rect.left,
            m.rect.top,
            m.dpi * 100 / 96,
            if m.primary {
                ", main: the calls open here"
            } else {
                ""
            }
        );
    }
    let displays = hdr::displays()?;
    for d in &displays {
        println!(
            "display {} ({}): HDR {}",
            d.name,
            d.device,
            match (d.supported, d.on) {
                (false, _) => "not available",
                (true, true) => "on now",
                (true, false) => "off now",
            }
        );
    }
    println!(
        "global shortcut {}; toolbar place ({:.3}, {:.3}); toolbar at {:?}; pixels read at {:?}; cursor at {:?}",
        stored.options.global_shortcut,
        stored.remembered.toolbar_place.x(),
        stored.remembered.toolbar_place.y(),
        layout.toolbar,
        patch_rect,
        cursor
    );
    println!(
        "processor cycle counter {:.0} MHz; check: {} of spinning counted as {:.1} ms",
        tsc_hz / 1e6,
        judge::ms_text(SPIN),
        spun.as_secs_f64() * 1000.0
    );
    println!("waits: {waits:?}\n");

    let mut rig = Rig {
        exe,
        shortcut: stored.options.global_shortcut,
        device: monitor.device.clone(),
        stage: screen::Stage::new(STAGE_COLOR)?,
        stage_rect,
        patch: screen::Patch::new(patch_rect)?,
        cursor,
        tsc_hz,
        waits,
        started,
    };

    let mut off = Pass::default();
    let mut on = Pass::default();
    // The calls open on the main monitor, so the pass with HDR on needs HDR there.
    let any_hdr = displays
        .iter()
        .any(|d| d.device == monitor.device && d.supported);
    let mut failure = rig.pass(false, &mut off).err();
    if failure.is_none() && any_hdr {
        failure = rig.pass(true, &mut on).err();
    }
    rig.stage.hide();
    let rows = judge::judge(&off, any_hdr.then_some(&on));

    println!("\n== Goals of WPC-286\n");
    print!("{}", judge::report(&rows));
    if !any_hdr {
        println!("The main monitor can't turn HDR on, so the pass with HDR on didn't run.");
    }
    println!(
        "Took {:.1} minutes.",
        started.elapsed().as_secs_f64() / 60.0
    );

    if let Some(e) = failure {
        return Err(format!("the routine stopped early: {e}"));
    }
    if waits.quick {
        return Err("a quick run is never a release result".to_string());
    }
    let outcome = judge::Outcome::of(&rows);
    if !outcome.all_met() {
        return Err(format!(
            "goals missed or not measured (the version is held): {:?}; HDR allowances for the owner: {:?}",
            outcome.held, outcome.owner_decides
        ));
    }
    Ok(())
}

/// What the passes share: the exe, the shortcut, the stage and the pixels read.
struct Rig {
    exe: PathBuf,
    shortcut: Shortcut,
    /// The main monitor, where the calls open.
    device: String,
    stage: screen::Stage,
    stage_rect: Rect,
    patch: screen::Patch,
    cursor: Point,
    tsc_hz: f64,
    waits: Waits,
    started: Instant,
}

impl Rig {
    fn log(&self, text: &str) {
        let at = self.started.elapsed().as_secs();
        println!("[{:>2}:{:02}] {text}", at / 60, at % 60);
    }

    /// One pass with HDR `hdr_on`, filling `pass` as it goes, so a pass that stops early still shows what it got.
    fn pass(&mut self, hdr_on: bool, pass: &mut Pass) -> Result<(), String> {
        let name = if hdr_on { "HDR on" } else { "HDR off" };
        self.log(&format!("--- pass with {name}"));
        hdr::set_all(hdr_on)?;
        let main_on = hdr::displays()?
            .iter()
            .any(|d| d.device == self.device && d.on);
        if main_on != hdr_on {
            return Err(format!(
                "HDR is {} on the main monitor, where the calls open, for the pass with {name}",
                if main_on { "on" } else { "off" }
            ));
        }

        // The first call, by opening the exe, as the Start menu does.
        // The toolbar opens on the cursor's monitor.
        screen::put_cursor(self.cursor)?;
        self.show_stage()?;
        let at = Instant::now();
        let app = App::start(&self.exe, false)?;
        let opening = self.watch(at, CALL_WAIT).map_err(|e| shown(&app, &e))?;
        pass.first_call_opening = Some(opening);
        self.log(&format!(
            "first call, opening the exe: {}",
            judge::ms_text(opening)
        ));
        app.close_tools(CLOSE_WAIT)?;
        app.exit()?;
        screen::wait(GAP);

        // Started as at sign-in: no call until the shortcut.
        self.stage.hide();
        let mut app = App::start(&self.exe, true)?;
        app.wait_until_resident(START_WAIT)?;
        screen::wait(self.waits.settle_after_start);
        let idle = self.idle(&app, self.waits.idle_before_first_call, None)?;
        pass.cpu_idle_before_first_call = Some(idle.cpu);
        let memory = app.private_working_set()?;
        pass.memory_before_first_call = Some(memory);
        self.log(&format!(
            "idle before the first call: {}, {}",
            per_minute(idle.cpu),
            judge::mb_text(memory)
        ));

        // Each timed call reads the memory once the toolbar has settled, after its time is taken; the highest counts
        // with the untimed calls'.
        self.show_stage()?;
        let (first, memory) = self.call(&app)?;
        pass.first_call_shortcut = Some(first);
        let mut highest = memory;
        self.log(&format!(
            "first call, by the shortcut: {}; {} with the toolbar shown",
            judge::ms_text(first),
            judge::mb_text(memory)
        ));
        for n in 2..=CALLS + 1 {
            let (time, memory) = self.call(&app)?;
            pass.calls.push(time);
            highest = highest.max(memory);
            self.log(&format!(
                "call {n}: {}; {} with the toolbar shown",
                judge::ms_text(time),
                judge::mb_text(memory)
            ));
        }
        if let Some(median) = judge::median(&pass.calls) {
            self.log(&format!(
                "median of calls 2 to {}: {}",
                CALLS + 1,
                judge::ms_text(median)
            ));
        }
        for _ in 0..MEMORY_CALLS {
            let (peak, settled) = self.memory_call(&app)?;
            highest = highest.max(peak);
            self.log(&format!(
                "memory with the tools open: highest {}, {} when they closed",
                judge::mb_text(peak),
                judge::mb_text(settled)
            ));
        }
        pass.memory_tools_open = Some(highest);

        self.stage.hide();
        let idle = self.idle(
            &app,
            self.waits.idle_after_use,
            Some(self.waits.memory_after),
        )?;
        pass.memory_idle_after_use = idle.memory;
        pass.cpu_idle_after_use = Some(idle.cpu);
        self.log(&format!(
            "idle after use: {}; memory after {:?}: {}",
            per_minute(idle.cpu),
            self.waits.memory_after,
            idle.memory.map_or("not read".to_string(), judge::mb_text)
        ));

        self.show_stage()?;
        for n in 1..=self.waits.round {
            self.call(&app)
                .map_err(|e| format!("call {n} of the round of {}: {e}", self.waits.round))?;
        }
        self.stage.hide();
        self.log(&format!("round of {} calls done", self.waits.round));
        screen::wait(self.waits.memory_after);
        let memory = app.private_working_set()?;
        pass.memory_idle_after_100_calls = Some(memory);
        self.log(&format!(
            "memory after the round: {}",
            judge::mb_text(memory)
        ));

        app.exit()?;
        Ok(())
    }

    /// Shows the stage and checks that the pixels read are the stage's: no other window covers them.
    fn show_stage(&mut self) -> Result<(), String> {
        self.stage.show(self.stage_rect)?;
        let pixels = self.patch.read()?;
        if !appearance::is_stage(&pixels, self.stage.color) {
            return Err(format!(
                "the stage isn't on screen at {:?}: another window covers it there (pixels {pixels:?})",
                self.patch.rect
            ));
        }
        Ok(())
    }

    /// Reads the pixels from `from` until they show the toolbar and settle, and says when the toolbar appeared.
    fn watch(&mut self, from: Instant, within: Duration) -> Result<Duration, String> {
        let mut reads = Vec::new();
        let mut changed_at = None;
        loop {
            let pixels = self.patch.read()?;
            let at = from.elapsed();
            if changed_at.is_none() && !appearance::is_stage(&pixels, self.stage.color) {
                changed_at = Some(at);
            }
            reads.push(Read { at, pixels });
            match changed_at {
                Some(changed) if at >= changed + appearance::SETTLE => break,
                None if at > within => {
                    return Err(format!("no toolbar on screen within {within:?}"));
                }
                _ => {
                    // A breath between reads, so reading the screen doesn't crowd out the composition it waits for.
                    std::thread::sleep(READ_EVERY);
                    screen::pump();
                }
            }
        }
        appearance::appeared_at(&reads, self.stage.color).ok_or_else(|| {
            "the pixels changed but went back to the stage: no toolbar stayed".to_string()
        })
    }

    /// One call by the global shortcut, timed to the toolbar on screen, then closed: its time, and the memory read
    /// once the toolbar settled.
    fn call(&mut self, app: &App) -> Result<(Duration, u64), String> {
        screen::put_cursor(self.cursor)?;
        self.stage_still_shown()?;
        let at = screen::press(self.shortcut)?;
        let time = self.watch(at, CALL_WAIT).map_err(|e| shown(app, &e))?;
        let memory = app.private_working_set()?;
        app.close_tools(CLOSE_WAIT)?;
        screen::wait(GAP);
        Ok((time, memory))
    }

    /// One call that reads the memory from the shortcut on while the tools stay open, then closes them: the highest
    /// value read, and the last one, just before they closed.
    fn memory_call(&mut self, app: &App) -> Result<(u64, u64), String> {
        screen::put_cursor(self.cursor)?;
        self.stage_still_shown()?;
        let at = screen::press(self.shortcut)?;
        let mut highest = 0;
        while at.elapsed() < HOLD_OPEN {
            highest = highest.max(app.private_working_set()?);
            screen::wait(MEMORY_EVERY);
        }
        if app.frozen_screens().is_empty() {
            return Err("the shortcut didn't open the tools".to_string());
        }
        let settled = app.private_working_set()?;
        app.close_tools(CLOSE_WAIT)?;
        screen::wait(GAP);
        Ok((highest.max(settled), settled))
    }

    fn stage_still_shown(&mut self) -> Result<(), String> {
        let pixels = self.patch.read()?;
        if appearance::is_stage(&pixels, self.stage.color) {
            Ok(())
        } else {
            Err(format!(
                "before the call, the pixels read aren't the stage's ({pixels:?}): the tools didn't close, or a window covers the stage"
            ))
        }
    }

    /// The processor time `app` used over `length` with nothing asked of it, and its memory `memory_at` into it.
    fn idle(
        &self,
        app: &App,
        length: Duration,
        memory_at: Option<Duration>,
    ) -> Result<Idle, String> {
        let start = Instant::now();
        let (cycles, times) = (app.cycles()?, app.times()?);
        let mut memory = None;
        if let Some(at) = memory_at {
            screen::wait(at);
            memory = Some(app.private_working_set()?);
        }
        screen::wait(length.saturating_sub(start.elapsed()));
        let (cycles_end, times_end) = (app.cycles()?, app.times()?);
        let wall = start.elapsed();
        let used = exact_f64(cycles_end - cycles);
        let cpu = CpuWindow {
            cpu: Duration::from_secs_f64(used / self.tsc_hz),
            wall,
        };
        self.log(&format!(
            "  {:.1} s: {:.0} cycles = {:.2} ms; Windows' process times, in steps of 15.6 ms, say {} ms",
            wall.as_secs_f64(),
            used,
            cpu.cpu.as_secs_f64() * 1000.0,
            (times_end - times).as_millis()
        ));
        Ok(Idle { cpu, memory })
    }
}

/// What a stretch of idle time measured.
struct Idle {
    cpu: CpuWindow,
    memory: Option<u64>,
}

fn per_minute(window: CpuWindow) -> String {
    window
        .ms_per_minute()
        .map_or("a window of no time".to_string(), judge::per_minute_text)
}

/// The processor's cycle counter's rate, which QueryProcessCycleTime counts in: the counter's ticks over half a second
/// of the system's clock.
fn tsc_hz() -> f64 {
    // SAFETY: RDTSC reads a counter every x64 processor has, and touches no memory.
    let read = || unsafe { std::arch::x86_64::_rdtsc() };
    let (start, cycles) = (Instant::now(), read());
    std::thread::sleep(Duration::from_millis(500));
    let (elapsed, ticks) = (start.elapsed(), read() - cycles);
    exact_f64(ticks) / elapsed.as_secs_f64()
}

/// `failure`, with how many frozen screen windows `app` shows: none means the call didn't open the tools, and some
/// mean they opened where the pixels aren't read, such as on another monitor.
fn shown(app: &App, failure: &str) -> String {
    format!(
        "{failure}; ScreenGauge shows {} frozen screen window(s)",
        app.frozen_screens().len()
    )
}

/// How long the check of the cycle counter spins.
const SPIN: Duration = Duration::from_millis(300);

/// Spins this thread for `SPIN` and counts its cycles as the processor windows do: a thread that runs the whole time
/// must come out as long as it ran, or the processor goals would be read in the wrong unit.
fn cycle_clock_check(tsc_hz: f64) -> Result<Duration, String> {
    let cycles = || {
        let mut cycles = 0;
        // SAFETY: this thread's pseudo-handle, and a u64 for the call to fill.
        unsafe { QueryThreadCycleTime(GetCurrentThread(), &mut cycles) }
            .map(|()| cycles)
            .map_err(|e| format!("QueryThreadCycleTime: {e}"))
    };
    let (start, before) = (Instant::now(), cycles()?);
    while start.elapsed() < SPIN {
        std::hint::spin_loop();
    }
    let (wall, used) = (start.elapsed(), cycles()? - before);
    let counted = Duration::from_secs_f64(exact_f64(used) / tsc_hz);
    let ratio = counted.as_secs_f64() / wall.as_secs_f64();
    if !(0.9..=1.05).contains(&ratio) {
        return Err(format!(
            "{} of spinning counted as {:.1} ms of cycles: the cycle counts can't be read as processor time here",
            judge::ms_text(wall),
            counted.as_secs_f64() * 1000.0
        ));
    }
    Ok(counted)
}

fn exe() -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("SCREENGAUGE_EXE") {
        return Ok(PathBuf::from(path));
    }
    if cfg!(debug_assertions) {
        return Err(
            "this measures the exe users download: run it with --release, or name an exe in SCREENGAUGE_EXE".to_string(),
        );
    }
    Ok(PathBuf::from(env!("CARGO_BIN_EXE_ScreenGauge")))
}

fn describe_exe(exe: &Path) -> Result<(), String> {
    let bytes = std::fs::read(exe).map_err(|e| format!("reading {}: {e}", exe.display()))?;
    let mut hash = [0u8; 32];
    // SAFETY: Windows' own SHA-256 pseudo-handle, no key, and an output the hash's size.
    unsafe { BCryptHash(BCRYPT_SHA256_ALG_HANDLE, None, &bytes, &mut hash) }
        .ok()
        .map_err(|e| format!("BCryptHash: {e}"))?;
    let hex: String = hash.iter().map(|b| format!("{b:02x}")).collect();
    println!(
        "exe {}\n    {} bytes, SHA-256 {hex}",
        exe.display(),
        bytes.len()
    );
    Ok(())
}

/// The installer copy's settings file, the one ScreenGauge.exe run from anywhere but the Store reads.
fn settings_path() -> Result<PathBuf, String> {
    let local = std::env::var_os("LOCALAPPDATA").ok_or("LOCALAPPDATA isn't set")?;
    Ok(PathBuf::from(local)
        .join("ScreenGauge")
        .join("settings.ini"))
}

/// What ScreenGauge will start with: its settings file, or the defaults when there is none yet.
fn stored_settings(path: &Path) -> Result<Stored, String> {
    match std::fs::read(path) {
        Ok(bytes) => settings::read(&bytes)
            .map(|loaded| loaded.stored)
            .map_err(|e| {
                format!(
                    "{} is damaged ({e:?}): open ScreenGauge once to set it aside",
                    path.display()
                )
            }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Stored::default()),
        Err(e) => Err(format!("reading {}: {e}", path.display())),
    }
}

/// The settings file as it was before the routine, written back when dropped if ScreenGauge changed it meanwhile,
/// such as its remembered state.
struct SettingsKept {
    path: PathBuf,
    before: Option<Vec<u8>>,
}

impl SettingsKept {
    fn now(path: &Path) -> Result<SettingsKept, String> {
        let before = match std::fs::read(path) {
            Ok(bytes) => Some(bytes),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(format!("reading {}: {e}", path.display())),
        };
        Ok(SettingsKept {
            path: path.to_path_buf(),
            before,
        })
    }
}

impl Drop for SettingsKept {
    fn drop(&mut self) {
        // With no file before, the one ScreenGauge made at its first start is what it would make anyway.
        let Some(before) = &self.before else {
            return;
        };
        match std::fs::read(&self.path) {
            Ok(now) if &now == before => {}
            Ok(_) => match std::fs::write(&self.path, before) {
                Ok(()) => println!("settings.ini put back as it was before the routine"),
                Err(e) => eprintln!("warning: settings.ini wasn't put back: {e}"),
            },
            Err(e) => eprintln!("warning: settings.ini can't be read to check it: {e}"),
        }
    }
}
