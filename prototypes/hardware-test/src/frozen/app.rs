//! The resident app's state and flows: trigger → photo → tool windows → checks → close.
//! Only the main window's handlers reach this (through `with_app`), never a tool window.

use std::collections::VecDeque;
use std::rc::Rc;
use std::time::Duration;

use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, POINT};
use windows::Win32::Graphics::Direct2D::{D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1CreateFactory, ID2D1Factory};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT_SEMI_BOLD,
    DWRITE_PARAGRAPH_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_CENTER, DWriteCreateFactory, IDWriteFactory,
    IDWriteTextFormat,
};
use windows::Win32::Graphics::Dwm::DwmFlush;
use windows::Win32::Graphics::Gdi::UpdateWindow;
use windows::Win32::System::SystemInformation::GetTickCount;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, GetCursorPos, GetForegroundWindow, LWA_ALPHA, PostQuitMessage, SW_SHOW, SW_SHOWNA,
    SetForegroundWindow, SetLayeredWindowAttributes, ShowWindow, WS_EX_LAYERED, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::w;

use super::checks::{self, Grab, SERIES_DELAYS_MS, SeriesJob, describe_stats, short_name};
use super::sys::{self, TrayIcon, Usage};
use super::{
    CHECK_DELAY_MS, CloseReason, DPICHANGED_ON_TOOLS, FG_GATE, Opts, Signal, Source, TIMER_AUTO_CLOSE, TIMER_CHECK,
    TIMER_EXIT, TIMER_IDLE, TIMER_POLL, TIMER_SERIES, TIMER_TRIGGER, TRAY_ID, TRIGGER_TICK, WM_APP_TRAY, tools,
};
use crate::capture::{self, DxgiSession};
use crate::common::{Ctx, MonitorInfo, Photo, PxRect};
use crate::{image, log, monitors};

/// Toolbar size in DIPs, scaled by its monitor's DPI.
const TOOLBAR_DIP: (u32, u32) = (360, 56);
const TOOLBAR_TOP_MARGIN_DIP: u32 = 16;
/// An idle CPU sample shorter than this is not reported (the first tick after the tools close).
const MIN_IDLE_SAMPLE_MS: f64 = 30_000.0;

pub struct App {
    main: HWND,
    hinstance: HINSTANCE,
    opts: Opts,
    d2d: ID2D1Factory,
    text: IDWriteTextFormat,
    tray: TrayIcon,
    monitors: Vec<MonitorInfo>,
    monitors_error: Option<String>,
    pending_change: Option<PendingChange>,
    changes: u32,
    triggers: u32,
    session: Option<Session>,
    check_pending: bool,
    series_jobs: VecDeque<SeriesJob>,
    idle: IdleTracker,
    quit_when_idle: bool,
}

/// The open tools. Dropping it destroys the windows and releases the photo.
struct Session {
    n: u32,
    source: Source,
    t2: f64,
    photos: Vec<Rc<Photo>>,
    overlays: Vec<Overlay>,
    toolbar: Option<HWND>,
}

struct Overlay {
    hwnd: HWND,
    device: String,
}

impl Drop for Session {
    fn drop(&mut self) {
        // The toolbar first: destroying its owner overlay would take it along.
        if let Some(toolbar) = self.toolbar.take() {
            sys::destroy_window(toolbar, "toolbar");
        }
        for overlay in self.overlays.drain(..) {
            sys::destroy_window(overlay.hwnd, "overlay");
        }
    }
}

/// A layout change seen by the poll, reported one tick later so messages arriving just after it count.
struct PendingChange {
    k: u32,
    at: f64,
    kind: String,
    before: Vec<Signal>,
}

#[derive(Default)]
struct IdleTracker {
    /// (now_ms, cpu_ms) at the start of the current idle sample.
    since: Option<(f64, f64)>,
    total_ms: f64,
    total_cpu_ms: f64,
}

impl App {
    pub fn new(main: HWND, hinstance: HINSTANCE, opts: Opts) -> Result<Self, String> {
        // SAFETY: plain factory creation on the STA thread that uses it.
        let d2d: ID2D1Factory =
            unsafe { D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None) }.ctx("D2D1CreateFactory")?;
        // SAFETY: as above.
        let dwrite: IDWriteFactory =
            unsafe { DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED) }.ctx("DWriteCreateFactory")?;
        // SAFETY: plain call; font size in DIPs, scaled by the toolbar's render-target DPI.
        let text = unsafe {
            dwrite.CreateTextFormat(
                w!("Segoe UI"),
                None,
                DWRITE_FONT_WEIGHT_SEMI_BOLD,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                15.0,
                w!("pt-BR"),
            )
        }
        .ctx("CreateTextFormat(Segoe UI)")?;
        // SAFETY: plain setters on our own text format.
        unsafe {
            text.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER).ctx("SetTextAlignment")?;
            text.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER).ctx("SetParagraphAlignment")?;
        }

        let monitors = monitors::enumerate()?;
        log::line(format!("monitors at start: {}", describe_monitors(&monitors)));

        let tray = TrayIcon::add(main, TRAY_ID, WM_APP_TRAY)?;
        log::line("tray icon added (NOTIFYICON_VERSION_4, IDI_APPLICATION, tooltip \"hwtest\")");

        let mut app = Self {
            main,
            hinstance,
            opts,
            d2d,
            text,
            tray,
            monitors,
            monitors_error: None,
            pending_change: None,
            changes: 0,
            triggers: 0,
            session: None,
            check_pending: false,
            series_jobs: VecDeque::new(),
            idle: IdleTracker::default(),
            quit_when_idle: false,
        };
        if let Some(u) = app.log_usage("at startup") {
            app.idle.since = Some((log::now_ms(), u.cpu_ms));
        }
        log::line("note: idle CPU includes this test's 1 s monitor poll, which the real app would not run");
        Ok(app)
    }

    pub fn taskbar_created(&mut self) {
        match self.tray.register() {
            Ok(()) => log::line("tray icon added again"),
            Err(e) => log::line(format!("ERROR: re-adding the tray icon: {e}")),
        }
    }

    pub fn timer(&mut self, id: usize) {
        match id {
            TIMER_POLL => self.poll(),
            TIMER_IDLE => self.idle_tick(),
            TIMER_TRIGGER => {
                sys::kill_timer(self.main, TIMER_TRIGGER);
                self.trigger(Source::Timer, log::now_ms());
            }
            TIMER_AUTO_CLOSE => {
                sys::kill_timer(self.main, TIMER_AUTO_CLOSE);
                self.close(CloseReason::AutoClose, log::now_ms());
            }
            TIMER_EXIT => {
                sys::kill_timer(self.main, TIMER_EXIT);
                log::line(format!("--exit-after-secs {} reached; exiting", self.opts.exit_after_secs));
                // SAFETY: plain call on this thread.
                unsafe { PostQuitMessage(0) };
            }
            TIMER_CHECK => self.identical_check(),
            TIMER_SERIES => self.series_tick(),
            other => log::line(format!("unknown timer {other}")),
        }
    }

    // ---- Trigger and photo -------------------------------------------------------------------------

    pub fn trigger(&mut self, source: Source, t0: f64) {
        if let Some(session) = &self.session {
            let n = session.n;
            if source == Source::Hotkey {
                log::line(format!("hotkey pressed again while tools #{n} are open"));
                self.close(CloseReason::HotkeyAgain, t0);
            } else {
                log::line(format!("trigger {} ignored: tools #{n} are already open", source.name()));
            }
            return;
        }
        self.triggers = self.triggers.saturating_add(1);
        let n = self.triggers;
        // SAFETY: plain tick query.
        let tick = unsafe { GetTickCount() };
        TRIGGER_TICK.with(|c| c.set(Some(tick)));
        self.idle.since = None;
        log::line(format!(
            "trigger #{n} {}: t0 = {t0:.1} ms, handled {:.1} ms after arrival",
            source.name(),
            log::now_ms() - t0
        ));

        // Held until the tools are open, so the foreground never goes back to Explorer in between.
        // Only on even triggers, so one session measures the same source with and without it.
        let activate = self.opts.activate_first && n % 2 == 0;
        if matches!(source, Source::Tray | Source::Start) {
            log::line(format!("RESULT activate-first {} #{n}: {}", source.name(), if activate { "on" } else { "off" }));
        }
        let activator = match source {
            Source::Tray | Source::Start if activate => match Activator::take(self.hinstance) {
                Ok(a) => Some(a),
                Err(e) => {
                    log::line(format!("ERROR: activate-first #{n}: {e}; running the series without it"));
                    None
                }
            },
            _ => None,
        };
        let photos = match source {
            Source::Hotkey | Source::Timer => match capture::frozen_photo() {
                Ok(list) => list.into_iter().map(Rc::new).collect::<Vec<_>>(),
                Err(e) => {
                    log::line(format!(
                        "ERROR: trigger #{n} {}: frozen_photo: {e}; nothing shown, staying resident",
                        source.name()
                    ));
                    return;
                }
            },
            Source::Tray | Source::Start => {
                let job = self.delay_series(source, n, t0);
                let photo = job.photo();
                let final_at = job.final_at();
                self.series_jobs.push_back(job);
                match photo {
                    Ok(list) => {
                        log::line(format!("series #{n}: the photo is the +{final_at:.1} ms grab"));
                        list
                    }
                    Err(e) => {
                        log::line(format!(
                            "ERROR: trigger #{n} {}: {e}; nothing shown, staying resident",
                            source.name()
                        ));
                        self.kick_series();
                        return;
                    }
                }
            }
        };
        let t1 = log::now_ms();
        self.log_photo(source, n, t0, &photos);
        if let Err(e) = self.open(source, n, photos) {
            log::line(format!("ERROR: opening tools #{n} ({}): {e}; nothing shown, staying resident", source.name()));
            FG_GATE.with(|c| c.set(None));
            self.kick_series();
            return;
        }
        self.after_open(t0, t1);
        drop(activator);
    }

    /// One DxgiSession, grabs at fixed delays after t0, every frame kept for the analysis.
    /// The session lives only inside this function: a process can duplicate an output once, so it must be
    /// gone before frozen_photo or the identical check create theirs.
    fn delay_series(&self, source: Source, n: u32, t0: f64) -> SeriesJob {
        let mut grabs = Vec::with_capacity(SERIES_DELAYS_MS.len());
        let start = log::now_ms();
        match DxgiSession::new() {
            Err(e) => log::line(format!("ERROR: series {} #{n}: DxgiSession::new: {e}", source.name())),
            Ok(mut dxgi) => {
                let cold = log::now_ms() - start;
                for delay in SERIES_DELAYS_MS {
                    let wait_ms = t0 + f64::from(delay) - log::now_ms();
                    if wait_ms > 0.0 {
                        match Duration::try_from_secs_f64(wait_ms / 1000.0) {
                            Ok(d) => std::thread::sleep(d),
                            Err(e) => log::line(format!("series: bad wait {wait_ms} ms: {e}; grabbing now")),
                        }
                    }
                    let grab_start = log::now_ms();
                    let frames = dxgi.grab_all();
                    let took = log::now_ms() - grab_start;
                    grabs.push(Grab { delay, at: grab_start - t0, took, frames });
                }
                // Logged after the loop so the log writes do not shift the grab times.
                let warm: Vec<f64> = grabs.iter().skip(1).map(|g| g.took).collect();
                let warm_avg = match u32::try_from(warm.len()) {
                    Ok(count) if count > 0 => warm.iter().sum::<f64>() / f64::from(count),
                    _ => 0.0,
                };
                log::line(format!(
                    "series {} #{n}: DxgiSession::new {cold:.1} ms (cold), first grab_all {:.1} ms, later grab_all average {warm_avg:.1} ms, max {:.1} ms (warm)",
                    source.name(),
                    grabs.first().map_or(0.0, |g| g.took),
                    warm.iter().copied().fold(0.0, f64::max)
                ));
            }
        }
        SeriesJob::new(source, n, t0, !self.opts.no_save, grabs)
    }

    fn log_photo(&self, source: Source, n: u32, t0: f64, photos: &[Rc<Photo>]) {
        for p in photos {
            log::line(format!(
                "photo #{n} {}: {:?}, {}, {}x{}, pixels taken at +{:.1} ms",
                short_name(&p.device_name),
                p.method,
                p.rect,
                p.width,
                p.height,
                p.taken_ms - t0
            ));
        }
        let earliest = photos.iter().map(|p| p.taken_ms).fold(f64::INFINITY, f64::min);
        let latest = photos.iter().map(|p| p.taken_ms).fold(f64::NEG_INFINITY, f64::max);
        if photos.is_empty() {
            log::line(format!("RESULT photo spread {} #{n}: no monitors", source.name()));
        } else {
            log::line(format!(
                "RESULT photo spread {} #{n}: {:.1} ms between the first and last monitor ({} monitors)",
                source.name(),
                latest - earliest,
                photos.len()
            ));
        }
        if photos.len() != self.monitors.len() {
            log::line(format!(
                "WARNING: the photo covers {} monitors but the last poll saw {}",
                photos.len(),
                self.monitors.len()
            ));
        }
    }

    // ---- Tool windows ------------------------------------------------------------------------------

    /// Creates the overlays and the toolbar, shows them, takes the foreground and waits for DWM.
    /// On error the partly built session is dropped here, which destroys whatever was created.
    fn open(&mut self, source: Source, n: u32, photos: Vec<Rc<Photo>>) -> Result<(), String> {
        DPICHANGED_ON_TOOLS.with(|c| c.set(0));
        let tag = format!("{} #{n}", source.name());
        let mut session = Session { n, source, t2: 0.0, photos, overlays: Vec::new(), toolbar: None };

        for photo in session.photos.clone() {
            let short = short_name(&photo.device_name);
            let device = photo.device_name.clone();
            let hwnd = tools::create_overlay(self.hinstance, &self.d2d, self.opts.present, Rc::clone(&photo), &short)?;
            session.overlays.push(Overlay { hwnd, device });
            let (w, h) = tools::client_size(hwnd)?;
            let equal = u32::try_from(w) == Ok(photo.width) && u32::try_from(h) == Ok(photo.height);
            log::line(format!(
                "RESULT overlay size {tag} {short}: client {w}x{h}, photo {}x{}, equal {}",
                photo.width,
                photo.height,
                yes_no(equal)
            ));
        }

        let (monitor_rect, dpi, device) = self.toolbar_monitor(&session.photos);
        let width = sys::scale_dip(TOOLBAR_DIP.0, dpi);
        let height = sys::scale_dip(TOOLBAR_DIP.1, dpi);
        let left = monitor_rect.left + (monitor_rect.width() - width) / 2;
        let top = monitor_rect.top + sys::scale_dip(TOOLBAR_TOP_MARGIN_DIP, dpi);
        let rect = PxRect { left, top, right: left + width, bottom: top + height };
        let owner = session.overlays.iter().find(|o| o.device == device).map(|o| o.hwnd);
        if owner.is_none() {
            log::line("toolbar: no overlay on its monitor to own it; relying on creation order for z-order");
        }
        let toolbar =
            tools::create_toolbar(self.hinstance, &self.d2d, self.opts.present, &self.text, owner, rect, dpi)?;
        session.toolbar = Some(toolbar);
        log::line(format!("toolbar {tag}: {rect} on {} at {dpi} dpi", short_name(&device)));

        for overlay in &session.overlays {
            // SAFETY: our own window. ShowWindow's BOOL is the previous visibility, not an error.
            let _was_visible = unsafe { ShowWindow(overlay.hwnd, SW_SHOWNA) };
            // SAFETY: our own window; paints synchronously.
            if !unsafe { UpdateWindow(overlay.hwnd) }.as_bool() {
                log::line(format!("UpdateWindow(overlay {}) returned FALSE", short_name(&overlay.device)));
            }
        }
        // From here, a foreground change to another process closes the tools.
        // SAFETY: plain tick query.
        FG_GATE.with(|c| c.set(Some(unsafe { GetTickCount() })));
        // SAFETY: our own window, as above.
        let _was_visible = unsafe { ShowWindow(toolbar, SW_SHOW) };
        // SAFETY: as above.
        if !unsafe { UpdateWindow(toolbar) }.as_bool() {
            log::line("UpdateWindow(toolbar) returned FALSE");
        }
        // SAFETY: plain call on our own window.
        let set_foreground = unsafe { SetForegroundWindow(toolbar) }.as_bool();
        // SAFETY: plain call; blocks until DWM has presented the pending updates.
        if let Err(e) = unsafe { DwmFlush() } {
            log::line(format!("DwmFlush: {e}"));
        }
        session.t2 = log::now_ms();
        // SAFETY: plain query.
        let foreground = unsafe { GetForegroundWindow() };
        let acquired = foreground == toolbar;
        log::line(format!(
            "RESULT foreground {tag}: {} (SetForegroundWindow returned {set_foreground}; foreground is {})",
            yes_no(acquired),
            sys::describe_window(foreground)
        ));
        self.session = Some(session);
        Ok(())
    }

    /// The toolbar goes on the monitor under the cursor (from the last poll's list).
    fn toolbar_monitor(&self, photos: &[Rc<Photo>]) -> (PxRect, u32, String) {
        let mut pt = POINT::default();
        // SAFETY: pt is a valid out pointer.
        let cursor = unsafe { GetCursorPos(&mut pt) };
        let under = match cursor {
            Ok(()) => self.monitors.iter().find(|m| contains(m.rect, pt)),
            Err(e) => {
                log::line(format!("GetCursorPos: {e}; using the primary monitor for the toolbar"));
                None
            }
        };
        let chosen = under.or_else(|| self.monitors.iter().find(|m| m.primary)).or_else(|| self.monitors.first());
        match chosen {
            Some(m) => {
                log::line(format!("cursor at ({},{}); toolbar monitor {}", pt.x, pt.y, short_name(&m.device_name)));
                (m.rect, m.dpi, m.device_name.clone())
            }
            None => {
                log::line("no monitor list; the toolbar goes on the first photo's monitor at 96 dpi");
                match photos.first() {
                    Some(p) => (p.rect, 96, p.device_name.clone()),
                    None => (PxRect { left: 0, top: 0, right: 800, bottom: 600 }, 96, String::new()),
                }
            }
        }
    }

    fn after_open(&mut self, t0: f64, t1: f64) {
        let Some(session) = &self.session else { return };
        let (n, source, t2) = (session.n, session.source, session.t2);
        let photos = session.photos.clone();
        let tag = format!("{} #{n}", source.name());
        let note = match source {
            Source::Tray | Source::Start => " (includes the 1500 ms delay series)",
            Source::Hotkey | Source::Timer => " (frozen_photo, which creates a cold DXGI session)",
        };
        log::line(format!("RESULT capture {tag}: {:.1} ms{note}", t1 - t0));
        log::line(format!(
            "RESULT windows {tag}: {:.1} ms (cold: render targets and bitmap uploads included; see the first-paint lines)",
            t2 - t1
        ));
        log::line(format!("RESULT total {tag}: {:.1} ms{note}", t2 - t0));
        let held: usize = self.series_jobs.len();
        self.log_usage(&format!("after tools #{n} open ({held} series job(s) still holding frames)"));

        if let Err(e) = sys::set_timer(self.main, TIMER_AUTO_CLOSE, self.opts.auto_close_secs.saturating_mul(1000)) {
            log::line(format!("ERROR: the auto-close safety net is NOT armed: {e}"));
        }
        // Armed before the optional saves, so the check runs as soon as they finish.
        // WPC-286: HWTEST_NO_CHECK skips the re-capture, which the real app would not run.
        if std::env::var_os("HWTEST_NO_CHECK").is_some() {
            log::line(format!("identical check {tag} skipped: HWTEST_NO_CHECK"));
        } else {
            match sys::set_timer(self.main, TIMER_CHECK, CHECK_DELAY_MS) {
                Ok(()) => self.check_pending = true,
                Err(e) => log::line(format!("ERROR: identical check {tag} not scheduled: {e}")),
            }
        }
        if self.opts.save_photo {
            for photo in &photos {
                let name = format!("photo-{}-{n}-{}.png", source.name(), short_name(&photo.device_name));
                if let Err(e) = checks::save(photo, &name) {
                    log::line(format!("ERROR: saving {name}: {e}"));
                }
            }
        }
        if !self.check_pending {
            self.kick_series();
        }
    }

    // ---- Closing -----------------------------------------------------------------------------------

    pub fn close(&mut self, reason: CloseReason, at: f64) {
        let Some(session) = self.session.take() else {
            log::line(format!("close request ({}) with no tools open; nothing to do", reason.name()));
            return;
        };
        FG_GATE.with(|c| c.set(None));
        let tag = format!("{} #{}", session.source.name(), session.n);
        sys::kill_timer(self.main, TIMER_AUTO_CLOSE);
        if self.check_pending {
            sys::kill_timer(self.main, TIMER_CHECK);
            self.check_pending = false;
            log::line(format!("identical check {tag} skipped: the tools closed before it ran"));
        }
        log::line(format!("RESULT close {tag}: {} ({:.1} ms after t2)", reason.name(), at - session.t2));
        log::line(format!(
            "RESULT WM_DPICHANGED on tool windows {tag}: {}",
            DPICHANGED_ON_TOOLS.with(std::cell::Cell::get)
        ));
        let start = log::now_ms();
        drop(session);
        log::line(format!(
            "tools {tag} closed: windows destroyed and photo released in {:.1} ms",
            log::now_ms() - start
        ));
        self.log_usage(&format!("after tools {tag} closed"));
        // WPC-286: HWTEST_REPEAT=N re-triggers until N calls; HWTEST_KEEP_DXGI then holds a warm
        // DXGI session (as an app that keeps the capture ready would) for the idle samples.
        let repeat: u32 = match std::env::var("HWTEST_REPEAT") {
            Ok(v) => v.parse().unwrap_or_else(|e| {
                log::line(format!("ERROR: HWTEST_REPEAT={v} is not a count ({e}); no repeats"));
                0
            }),
            Err(_) => 0,
        };
        if self.triggers < repeat {
            // HWTEST_FIRST_GAP_MS leaves room for settled idle samples after the first call.
            let gap = match std::env::var("HWTEST_FIRST_GAP_MS") {
                Ok(v) if self.triggers == 1 => v.parse().unwrap_or_else(|e| {
                    log::line(format!("ERROR: HWTEST_FIRST_GAP_MS={v} is not a count ({e}); using 4000"));
                    4000
                }),
                _ => 4000,
            };
            if let Err(e) = sys::set_timer(self.main, TIMER_TRIGGER, gap) {
                log::line(format!("ERROR: HWTEST_REPEAT re-trigger not armed: {e}"));
            }
        } else if std::env::var_os("HWTEST_KEEP_DXGI").is_some() {
            match DxgiSession::new() {
                Ok(mut s) => {
                    let grabs = s.grab_all();
                    log::line(format!("HWTEST_KEEP_DXGI: warm session held after {} grab(s)", grabs.len()));
                    drop(grabs);
                    // Deliberately leaked: held until the process exits, like a kept capture.
                    std::mem::forget(s);
                    self.log_usage("after HWTEST_KEEP_DXGI session opened");
                }
                Err(e) => log::line(format!("ERROR: HWTEST_KEEP_DXGI: {e}")),
            }
        }
        if self.opts.once {
            self.quit_when_idle = true;
        }
        self.kick_series();
        self.maybe_quit();
    }

    fn maybe_quit(&mut self) {
        if self.quit_when_idle && self.session.is_none() && self.series_jobs.is_empty() && !self.check_pending {
            self.quit_when_idle = false;
            log::line("--once: the first tools session closed and its work is done; exiting");
            // SAFETY: plain call on this thread.
            unsafe { PostQuitMessage(0) };
        }
    }

    pub fn shutdown(&mut self) {
        if self.session.is_some() {
            self.close(CloseReason::Exit, log::now_ms());
        }
        if !self.series_jobs.is_empty() {
            log::line(format!(
                "exit: {} series job(s) unfinished; their remaining frames are neither analyzed nor saved",
                self.series_jobs.len()
            ));
        }
        if self.idle.total_ms > 0.0 {
            log::line(format!(
                "RESULT idle cpu average: {:.2} ms per idle minute over {:.1} idle minutes",
                self.idle.total_cpu_ms / (self.idle.total_ms / 60_000.0),
                self.idle.total_ms / 60_000.0
            ));
        } else {
            log::line("RESULT idle cpu average: no complete idle sample (needs two idle 60 s ticks in a row)");
        }
        self.log_usage("at exit");
    }

    // ---- Identical check ---------------------------------------------------------------------------

    /// Re-captures every monitor through a fresh DXGI session and through GDI, and compares with the photo.
    fn identical_check(&mut self) {
        sys::kill_timer(self.main, TIMER_CHECK);
        self.check_pending = false;
        let Some(session) = &self.session else {
            log::line("identical check: the tools are closed; skipped");
            self.kick_series();
            return;
        };
        let (n, source, t2) = (session.n, session.source, session.t2);
        let photos = session.photos.clone();
        let toolbar_rect = match session.toolbar.map(sys::window_rect) {
            Some(Ok(r)) => Some(r),
            Some(Err(e)) => {
                log::line(format!("identical check: toolbar rectangle unknown ({e}); nothing excluded"));
                None
            }
            None => None,
        };
        let tag = format!("{} #{n}", source.name());
        log::line(format!(
            "identical check {tag}: {:.0} ms after t2; toolbar excluded: {}",
            log::now_ms() - t2,
            toolbar_rect.map_or_else(|| "nothing".to_string(), |r| r.to_string())
        ));
        let mut original_saved = vec![false; photos.len()];

        // A fresh session (the series one is long gone: one duplication per output per process), released at
        // the end of the closure, before the GDI pass.
        let start = log::now_ms();
        let dxgi = DxgiSession::new().map(|mut dxgi| {
            let created = log::now_ms();
            let grabs = dxgi.grab_all();
            (grabs, created - start, log::now_ms() - created)
        });
        match dxgi {
            Ok((grabs, cold, first)) => {
                log::line(format!(
                    "identical check {tag}: DxgiSession::new {cold:.1} ms (cold), first grab_all {first:.1} ms"
                ));
                for (i, photo) in photos.iter().enumerate() {
                    let short = short_name(&photo.device_name);
                    match grabs.iter().find(|(d, _)| *d == photo.device_name) {
                        None => log::line(format!("RESULT identical {tag} {short} dxgi: no frame for this monitor")),
                        Some((_, Err(e))) => {
                            log::line(format!("RESULT identical {tag} {short} dxgi: capture failed ({e})"))
                        }
                        Some((_, Ok(again))) => {
                            let base = format!("identical-{}-{n}-{short}", source.name());
                            compare_identical(&tag, &base, photo, again, "dxgi", toolbar_rect, &mut original_saved[i]);
                        }
                    }
                }
            }
            Err(e) => log::line(format!("RESULT identical {tag} dxgi: DxgiSession::new failed ({e})")),
        }

        for (i, photo) in photos.iter().enumerate() {
            let short = short_name(&photo.device_name);
            match self.monitors.iter().find(|m| m.device_name == photo.device_name) {
                None => log::line(format!("RESULT identical {tag} {short} gdi: monitor not in the current list")),
                Some(m) => match timed(|| capture::gdi_monitor(m)) {
                    (Ok(again), took) => {
                        log::line(format!("identical check {tag} {short}: gdi_monitor {took:.1} ms"));
                        let base = format!("identical-{}-{n}-{short}", source.name());
                        compare_identical(&tag, &base, photo, &again, "gdi", toolbar_rect, &mut original_saved[i]);
                    }
                    (Err(e), _) => log::line(format!("RESULT identical {tag} {short} gdi: capture failed ({e})")),
                },
            }
        }
        self.kick_series();
    }

    // ---- Delay series analysis ---------------------------------------------------------------------

    /// Starts (or keeps) the series timer. WM_TIMER is the lowest-priority message, so input such as Esc
    /// is never starved by this background work.
    fn kick_series(&mut self) {
        if self.series_jobs.is_empty() || self.check_pending {
            return;
        }
        if let Err(e) = sys::set_timer(self.main, TIMER_SERIES, 1) {
            log::line(format!("ERROR: the series analysis cannot be scheduled: {e}"));
        }
    }

    fn series_tick(&mut self) {
        if self.check_pending {
            sys::kill_timer(self.main, TIMER_SERIES);
            return;
        }
        let more = match self.series_jobs.front_mut() {
            Some(job) => job.step(),
            None => false,
        };
        if !more && self.series_jobs.pop_front().is_some() {
            log::line("series job finished");
        }
        if self.series_jobs.is_empty() {
            sys::kill_timer(self.main, TIMER_SERIES);
            self.maybe_quit();
        }
    }

    // ---- Monitor changes ---------------------------------------------------------------------------

    fn poll(&mut self) {
        let now = log::now_ms();
        let fresh = super::take_signals();
        if let Some(pending) = self.pending_change.take() {
            report_change(&pending, &fresh);
        }
        let list = match monitors::enumerate() {
            Ok(list) => list,
            Err(e) => {
                if self.monitors_error.as_deref() != Some(e.as_str()) {
                    log::line(format!("ERROR: monitor poll: {e}"));
                    self.monitors_error = Some(e);
                }
                return;
            }
        };
        if let Some(e) = self.monitors_error.take() {
            log::line(format!("monitor poll works again (last error: {e})"));
        }
        let Some(kind) = layout_change(&self.monitors, &list) else { return };
        self.changes = self.changes.saturating_add(1);
        log::line(format!(
            "monitor change #{} detected by poll ({kind}): before [{}] after [{}]",
            self.changes,
            describe_monitors(&self.monitors),
            describe_monitors(&list)
        ));
        self.pending_change = Some(PendingChange { k: self.changes, at: now, kind, before: fresh });
        self.monitors = list;
        if self.session.is_some() {
            self.close(CloseReason::MonitorsChanged, now);
        }
    }

    // ---- Consumption -------------------------------------------------------------------------------

    fn idle_tick(&mut self) {
        let busy = self.session.is_some() || !self.series_jobs.is_empty() || self.check_pending;
        if busy {
            self.idle.since = None;
            log::line("usage tick: tools open or work pending; not an idle sample");
            return;
        }
        let now = log::now_ms();
        let Some(u) = self.log_usage("idle") else { return };
        log::line(format!(
            "RESULT idle memory: working set {:.1} MB, private {:.1} MB",
            sys::mb(u.working_set),
            sys::mb(u.private)
        ));
        if let Some((since_ms, since_cpu)) = self.idle.since {
            let span = now - since_ms;
            if span >= MIN_IDLE_SAMPLE_MS {
                let cpu = u.cpu_ms - since_cpu;
                log::line(format!(
                    "RESULT idle cpu: {:.2} ms per idle minute ({cpu:.1} ms over {:.1} s)",
                    cpu / (span / 60_000.0),
                    span / 1000.0
                ));
                self.idle.total_ms += span;
                self.idle.total_cpu_ms += cpu;
            }
        }
        self.idle.since = Some((now, u.cpu_ms));
    }

    fn log_usage(&self, label: &str) -> Option<Usage> {
        match sys::usage() {
            Ok(u) => {
                log::line(format!("usage {label}: {u}"));
                Some(u)
            }
            Err(e) => {
                log::line(format!("ERROR: usage {label}: {e}"));
                None
            }
        }
    }
}

fn compare_identical(
    tag: &str,
    base: &str,
    photo: &Photo,
    again: &Photo,
    method: &str,
    toolbar: Option<PxRect>,
    original_saved: &mut bool,
) {
    let short = short_name(&photo.device_name);
    let exclude: Vec<PxRect> = toolbar.and_then(|r| checks::rect_in_photo(r, photo)).into_iter().collect();
    let stats = match image::diff(photo, again, &exclude) {
        Ok(s) => s,
        Err(e) => {
            log::line(format!("RESULT identical {tag} {short} {method}: diff failed ({e})"));
            return;
        }
    };
    log::line(format!("RESULT identical {tag} {short} {method}: {}", describe_stats(&stats)));
    if stats.differing == 0 {
        return;
    }
    if !*original_saved {
        *original_saved = true;
        if let Err(e) = checks::save(photo, &format!("{base}-original.png")) {
            log::line(format!("ERROR: saving the original: {e}"));
        }
    }
    if let Err(e) = checks::save(again, &format!("{base}-{method}.png")) {
        log::line(format!("ERROR: saving the {method} re-capture: {e}"));
    }
    match checks::diff_image(photo, again, &exclude) {
        Ok(img) => {
            if let Err(e) = checks::save(&img, &format!("{base}-{method}-diff.png")) {
                log::line(format!("ERROR: saving the {method} diff image: {e}"));
            }
        }
        Err(e) => log::line(format!("ERROR: building the {method} diff image: {e}")),
    }
}

fn report_change(pending: &PendingChange, after: &[Signal]) {
    let all: Vec<&Signal> = pending.before.iter().chain(after).collect();
    let head = format!("RESULT monitor change #{} ({})", pending.k, pending.kind);
    if all.is_empty() {
        log::line(format!(
            "{head}: detected by poll only (no WM_DISPLAYCHANGE, WM_DPICHANGED, WM_SETTINGCHANGE or WM_DEVICECHANGE within one poll interval before or after)"
        ));
        return;
    }
    let mut names: Vec<&str> = all.iter().map(|s| s.name).collect();
    names.sort_unstable();
    names.dedup();
    log::line(format!("{head}: signals {}", names.join(", ")));
    let detail: Vec<String> =
        all.iter().map(|s| format!("{} on {} at {:+.0} ms", s.name, s.role, s.at - pending.at)).collect();
    log::line(format!(
        "monitor change #{} signals, relative to the poll that saw it: {}",
        pending.k,
        detail.join("; ")
    ));
}

/// What differs between two monitor lists (count, rectangles, DPI, primary), or None.
fn layout_change(before: &[MonitorInfo], after: &[MonitorInfo]) -> Option<String> {
    if before.len() != after.len() {
        return Some(format!("count {} -> {}", before.len(), after.len()));
    }
    let mut kinds = Vec::new();
    for b in before {
        match after.iter().find(|a| a.device_name == b.device_name) {
            None => kinds.push(format!("{} gone", short_name(&b.device_name))),
            Some(a) => {
                let short = short_name(&b.device_name);
                if a.rect != b.rect {
                    kinds.push(format!("{short} rect {} -> {}", b.rect, a.rect));
                }
                if a.dpi != b.dpi {
                    kinds.push(format!("{short} dpi {} -> {}", b.dpi, a.dpi));
                }
                if a.primary != b.primary {
                    kinds.push(format!("{short} primary {} -> {}", b.primary, a.primary));
                }
            }
        }
    }
    if kinds.is_empty() { None } else { Some(kinds.join(", ")) }
}

fn describe_monitors(list: &[MonitorInfo]) -> String {
    list.iter()
        .map(|m| {
            format!(
                "{} {} {} dpi{}{}",
                short_name(&m.device_name),
                m.rect,
                m.dpi,
                if m.primary { " primary" } else { "" },
                if m.friendly_name.is_empty() { String::new() } else { format!(" \"{}\"", m.friendly_name) }
            )
        })
        .collect::<Vec<_>>()
        .join("; ")
}

/// Runs `f` and returns its result with the time it took, in ms.
fn timed<T>(f: impl FnOnce() -> T) -> (T, f64) {
    let start = log::now_ms();
    let out = f();
    (out, log::now_ms() - start)
}

fn contains(r: PxRect, p: POINT) -> bool {
    p.x >= r.left && p.x < r.right && p.y >= r.top && p.y < r.bottom
}

fn yes_no(v: bool) -> &'static str {
    if v { "yes" } else { "no" }
}

/// A 1×1 transparent window that takes the foreground for `--activate-first`, so Explorer's panels see
/// another app activate and close before the photo.
struct Activator(HWND);

impl Activator {
    fn take(hinstance: HINSTANCE) -> Result<Self, String> {
        let mut pt = POINT::default();
        // SAFETY: fills one POINT on this stack frame.
        unsafe { GetCursorPos(&mut pt) }.ctx("GetCursorPos")?;
        // SAFETY: STATIC is a system class; Drop destroys the window on this thread.
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_LAYERED | WS_EX_TOPMOST,
                w!("STATIC"),
                w!("hwtest activator"),
                WS_POPUP,
                pt.x,
                pt.y,
                1,
                1,
                None,
                None,
                Some(hinstance),
                None,
            )
        }
        .ctx("CreateWindowExW activator")?;
        let activator = Self(hwnd);
        // SAFETY: hwnd is ours and alive.
        unsafe { SetLayeredWindowAttributes(hwnd, COLORREF(0), 0, LWA_ALPHA) }.ctx("SetLayeredWindowAttributes")?;
        let t = log::now_ms();
        // SAFETY: hwnd is ours and alive. The return value says whether it was already visible.
        let _ = unsafe { ShowWindow(hwnd, SW_SHOW) };
        // SAFETY: hwnd is ours and alive.
        let ok = unsafe { SetForegroundWindow(hwnd) }.as_bool();
        // SAFETY: plain query.
        let ours = unsafe { GetForegroundWindow() } == hwnd;
        log::line(format!(
            "activate-first: SetForegroundWindow returned {ok}, foreground is the activator: {ours}, {:.1} ms",
            log::now_ms() - t
        ));
        Ok(activator)
    }
}

impl Drop for Activator {
    fn drop(&mut self) {
        // SAFETY: created by take() on this thread and not destroyed elsewhere.
        if let Err(e) = unsafe { DestroyWindow(self.0) } {
            log::line(format!("activate-first: DestroyWindow: {e}"));
        }
    }
}
