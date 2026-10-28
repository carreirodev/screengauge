//! The measurements around the frozen screen that compare pixels: the delay series (how long the Windows
//! panels take to go away) and the identical check (does the overlay show the photo exactly).

use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;
use std::rc::Rc;

use super::Source;
use super::sys::f64_from_u64;
use crate::common::{Ctx, DiffStats, Photo, PxRect};
use crate::{image, log};

/// Grab times after t0, in ms. The last one is the reference and becomes the photo.
pub const SERIES_DELAYS_MS: [u32; 11] = [0, 50, 100, 150, 200, 300, 400, 500, 700, 1000, 1500];

/// A change whose bounding box fits in this square is ignored by the tolerant settle result
/// (a blinking caret or a clock digit, not a panel). A heuristic; the strict result is the real one.
const SMALL_CHANGE_PX: i32 = 64;

/// `\\.\DISPLAY1` → `DISPLAY1`, safe for file names.
pub fn short_name(device: &str) -> String {
    device.trim_start_matches(r"\\.\").chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect()
}

/// Writes a photo as PNG in the output directory and logs where and how long it took.
pub fn save(photo: &Photo, name: &str) -> Result<PathBuf, String> {
    let path = log::out_dir().join(name);
    let start = log::now_ms();
    image::save_png(photo, &path)?;
    log::line(format!("saved {} ({:.0} ms)", path.display(), log::now_ms() - start));
    Ok(path)
}

/// One `grab_all` of the series.
pub struct Grab {
    pub delay: u32,
    /// When the grab started, ms after t0.
    pub at: f64,
    /// How long `grab_all` took.
    pub took: f64,
    pub frames: Vec<(String, Result<Photo, String>)>,
}

struct Record {
    delay: u32,
    at: f64,
    /// None when the grab failed or could not be compared.
    differing: Option<u64>,
    bbox: Option<PxRect>,
}

/// The delay series of one trigger, analyzed and saved one grab at a time after the tools are shown,
/// so neither the timing nor the message loop is held up.
pub struct SeriesJob {
    pub source: Source,
    pub n: u32,
    pub t0: f64,
    save: bool,
    pending: VecDeque<Grab>,
    finals: Vec<(String, Rc<Photo>)>,
    final_delay: u32,
    final_at: f64,
    final_errors: Vec<(String, String)>,
    records: BTreeMap<String, Vec<Record>>,
    finals_saved: bool,
}

impl SeriesJob {
    /// Splits the grabs into the reference (the last one) and the rest.
    pub fn new(source: Source, n: u32, t0: f64, save: bool, mut grabs: Vec<Grab>) -> Self {
        let mut job = Self {
            source,
            n,
            t0,
            save,
            pending: VecDeque::new(),
            finals: Vec::new(),
            final_delay: 0,
            final_at: 0.0,
            final_errors: Vec::new(),
            records: BTreeMap::new(),
            finals_saved: false,
        };
        let Some(last) = grabs.pop() else { return job };
        job.final_delay = last.delay;
        job.final_at = last.at;
        for (device, frame) in last.frames {
            match frame {
                Ok(photo) => job.finals.push((device, Rc::new(photo))),
                Err(e) => job.final_errors.push((device, e)),
            }
        }
        job.pending = grabs.into();
        job
    }

    /// The photo for the tools: every monitor's reference frame, or why there is none.
    pub fn photo(&self) -> Result<Vec<Rc<Photo>>, String> {
        if !self.final_errors.is_empty() {
            let list: Vec<String> = self.final_errors.iter().map(|(d, e)| format!("{}: {e}", short_name(d))).collect();
            return Err(format!("the {} ms grab failed on {}", self.final_delay, list.join("; ")));
        }
        if self.finals.is_empty() {
            return Err("the series has no grab at all".to_string());
        }
        Ok(self.finals.iter().map(|(_, p)| Rc::clone(p)).collect())
    }

    pub fn final_at(&self) -> f64 {
        self.final_at
    }

    /// One unit of work. Returns false when the job is finished.
    pub fn step(&mut self) -> bool {
        if let Some(grab) = self.pending.pop_front() {
            self.compare(grab);
            return true;
        }
        if self.save && !self.finals_saved {
            self.finals_saved = true;
            for (device, photo) in &self.finals {
                let name = self.file_name(self.final_delay, device);
                if let Err(e) = save(photo, &name) {
                    log::line(format!("ERROR: saving {name}: {e}"));
                }
            }
            return true;
        }
        self.finish();
        false
    }

    /// Carries the trigger number so a second tray or Start trigger does not overwrite the first one's frames.
    fn file_name(&self, delay: u32, device: &str) -> String {
        format!("series-{}-{}-{delay}ms-{}.png", self.source.name(), self.n, short_name(device))
    }

    fn compare(&mut self, grab: Grab) {
        let tag = format!("series {} #{}", self.source.name(), self.n);
        for (device, frame) in &grab.frames {
            let short = short_name(device);
            let reference = self.finals.iter().find(|(d, _)| d == device).map(|(_, p)| p);
            let (differing, bbox) = match (frame, reference) {
                (Err(e), _) => {
                    log::line(format!("{tag} {short} +{} ms: grab failed: {e}", grab.delay));
                    (None, None)
                }
                (Ok(_), None) => {
                    log::line(format!("{tag} {short} +{} ms: no reference frame to compare with", grab.delay));
                    (None, None)
                }
                (Ok(photo), Some(reference)) => match image::diff(photo, reference, &[]) {
                    Ok(stats) => {
                        let bbox = if stats.differing > 0 { diff_bbox(photo, reference) } else { None };
                        log::line(format!(
                            "{tag} {short} +{} ms (grab at +{:.1}, took {:.1} ms, pixels taken at +{:.1}): {} vs the +{} ms frame{}",
                            grab.delay,
                            grab.at,
                            grab.took,
                            photo.taken_ms - self.t0,
                            describe_stats(&stats),
                            self.final_delay,
                            bbox.map(|b| format!(", changed area {b}")).unwrap_or_default()
                        ));
                        (Some(stats.differing), bbox)
                    }
                    Err(e) => {
                        log::line(format!("{tag} {short} +{} ms: diff failed: {e}", grab.delay));
                        (None, None)
                    }
                },
            };
            self.records.entry(device.clone()).or_default().push(Record {
                delay: grab.delay,
                at: grab.at,
                differing,
                bbox,
            });
            if self.save
                && let Ok(photo) = frame
            {
                let name = self.file_name(grab.delay, device);
                if let Err(e) = save(photo, &name) {
                    log::line(format!("ERROR: saving {name}: {e}"));
                }
            }
        }
    }

    fn finish(&mut self) {
        let tag = format!("{} #{}", self.source.name(), self.n);
        for (device, _) in &self.finals {
            self.records.entry(device.clone()).or_default().push(Record {
                delay: self.final_delay,
                at: self.final_at,
                differing: Some(0),
                bbox: None,
            });
        }
        let mut strict_all: Option<u32> = Some(0);
        let mut tolerant_all: Option<u32> = Some(0);
        for (device, records) in &mut self.records {
            records.sort_by_key(|r| r.delay);
            let short = short_name(device);
            let strict = settle(records, |r| r.differing == Some(0));
            let tolerant = settle(records, |r| r.differing == Some(0) || r.bbox.is_some_and(is_small));
            log::line(format!("RESULT settle delay {tag} {short}: {}", describe_settle(strict, records)));
            log::line(format!(
                "RESULT settle delay ignoring small changes {tag} {short}: {} (heuristic: changes within {SMALL_CHANGE_PX}x{SMALL_CHANGE_PX} px ignored)",
                describe_settle(tolerant, records)
            ));
            strict_all = max_settle(strict_all, strict);
            tolerant_all = max_settle(tolerant_all, tolerant);
        }
        if self.records.is_empty() {
            strict_all = None;
            tolerant_all = None;
        }
        log::line(format!(
            "RESULT settle delay {tag} overall: {}",
            strict_all
                .map_or_else(|| "unknown (a monitor never matched its reference)".to_string(), |d| format!("{d} ms"))
        ));
        log::line(format!(
            "RESULT settle delay ignoring small changes {tag} overall: {}",
            tolerant_all.map_or_else(|| "unknown".to_string(), |d| format!("{d} ms"))
        ));
        log::line(format!(
            "series {tag}: {} ms is the reference grab, so a settle of {} ms means the screen was still changing after the {} ms grab",
            self.final_delay,
            self.final_delay,
            SERIES_DELAYS_MS.get(SERIES_DELAYS_MS.len().saturating_sub(2)).copied().unwrap_or(0)
        ));
    }
}

/// The smallest delay from which every later record satisfies `ok`, or None when even the last one fails.
fn settle(records: &[Record], ok: impl Fn(&Record) -> bool) -> Option<u32> {
    let mut found = None;
    for r in records.iter().rev() {
        if !ok(r) {
            break;
        }
        found = Some(r.delay);
    }
    found
}

fn max_settle(acc: Option<u32>, one: Option<u32>) -> Option<u32> {
    match (acc, one) {
        (Some(a), Some(b)) => Some(a.max(b)),
        _ => None,
    }
}

fn describe_settle(delay: Option<u32>, records: &[Record]) -> String {
    match delay {
        None => "unknown (the reference grab is missing or failed)".to_string(),
        Some(d) => match records.iter().find(|r| r.delay == d) {
            Some(r) => format!("{d} ms (grab at +{:.1} ms)", r.at),
            None => format!("{d} ms"),
        },
    }
}

fn is_small(b: PxRect) -> bool {
    b.width() <= SMALL_CHANGE_PX && b.height() <= SMALL_CHANGE_PX
}

pub fn describe_stats(s: &DiffStats) -> String {
    let pct = if s.total == 0 { 0.0 } else { 100.0 * f64_from_u64(s.differing) / f64_from_u64(s.total) };
    let first = s.first.map(|(x, y)| format!(", first at ({x},{y})")).unwrap_or_default();
    format!("differing {} / {} px ({pct:.4}%), max delta {}{first}", s.differing, s.total, s.max_delta)
}

/// Bounding box of the pixels whose B, G or R differ, in photo coordinates. None if equal or sizes differ.
pub fn diff_bbox(a: &Photo, b: &Photo) -> Option<PxRect> {
    if a.width != b.width || a.height != b.height || a.bgra.len() != b.bgra.len() {
        return None;
    }
    let stride = usize::try_from(a.width).ok()?.checked_mul(4)?;
    if stride == 0 {
        return None;
    }
    let mut bbox: Option<PxRect> = None;
    for (y, (row_a, row_b)) in a.bgra.chunks_exact(stride).zip(b.bgra.chunks_exact(stride)).enumerate() {
        if row_a == row_b {
            continue;
        }
        let y = i32::try_from(y).ok()?;
        for (x, (pa, pb)) in row_a.as_chunks::<4>().0.iter().zip(row_b.as_chunks::<4>().0).enumerate() {
            if pa[..3] != pb[..3] {
                let x = i32::try_from(x).ok()?;
                bbox = Some(match bbox {
                    None => PxRect { left: x, top: y, right: x + 1, bottom: y + 1 },
                    Some(r) => PxRect {
                        left: r.left.min(x),
                        top: r.top.min(y),
                        right: r.right.max(x + 1),
                        bottom: r.bottom.max(y + 1),
                    },
                });
            }
        }
    }
    bbox
}

/// `rect` (virtual-screen pixels) clipped to the photo and moved into its coordinates.
pub fn rect_in_photo(rect: PxRect, photo: &Photo) -> Option<PxRect> {
    let p = photo.rect;
    let clipped = PxRect {
        left: rect.left.max(p.left),
        top: rect.top.max(p.top),
        right: rect.right.min(p.right),
        bottom: rect.bottom.min(p.bottom),
    };
    if clipped.width() <= 0 || clipped.height() <= 0 {
        return None;
    }
    Some(PxRect {
        left: clipped.left - p.left,
        top: clipped.top - p.top,
        right: clipped.right - p.left,
        bottom: clipped.bottom - p.top,
    })
}

/// Differing pixels red on black; excluded rectangles stay black.
pub fn diff_image(a: &Photo, b: &Photo, exclude: &[PxRect]) -> Result<Photo, String> {
    if a.width != b.width || a.height != b.height || a.bgra.len() != b.bgra.len() {
        return Err(format!("sizes differ: {}x{} vs {}x{}", a.width, a.height, b.width, b.height));
    }
    let stride = usize::try_from(a.width).ctx("width")?.checked_mul(4).ok_or("stride overflows")?;
    let mut out = vec![0u8; a.bgra.len()];
    if stride > 0 {
        let rows = a.bgra.chunks_exact(stride).zip(b.bgra.chunks_exact(stride)).zip(out.chunks_exact_mut(stride));
        for (y, ((row_a, row_b), row_out)) in rows.enumerate() {
            let y = i32::try_from(y).ctx("row")?;
            let pixels =
                row_a.as_chunks::<4>().0.iter().zip(row_b.as_chunks::<4>().0).zip(row_out.as_chunks_mut::<4>().0);
            for (x, ((pa, pb), po)) in pixels.enumerate() {
                let x = i32::try_from(x).ctx("column")?;
                let excluded = exclude.iter().any(|r| x >= r.left && x < r.right && y >= r.top && y < r.bottom);
                po[3] = 255;
                if !excluded && pa[..3] != pb[..3] {
                    po[2] = 255;
                }
            }
        }
    }
    Ok(Photo {
        device_name: a.device_name.clone(),
        rect: a.rect,
        width: a.width,
        height: a.height,
        bgra: out,
        method: a.method,
        taken_ms: log::now_ms(),
    })
}
