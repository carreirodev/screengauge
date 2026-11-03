//! The goals of WPC-286 and how a run's numbers are judged against them (v1 spec WPC-296, "Metas de velocidade e de
//! consumo"). Nothing here touches Windows: the routine measures, this decides.

use std::time::Duration;

/// What one pass measured, with HDR off or on. A value is `None` when its step didn't get that far.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Pass {
    /// From starting the exe, which is a call, to the toolbar on screen: the Start menu's way.
    pub first_call_opening: Option<Duration>,
    /// From the first global shortcut of a process started at sign-in to the toolbar on screen.
    pub first_call_shortcut: Option<Duration>,
    /// Calls 2 to 11, each from the global shortcut to the toolbar on screen.
    pub calls: Vec<Duration>,
    /// Private working set, in bytes, of a process started at sign-in, before any call.
    pub memory_before_first_call: Option<u64>,
    /// The highest private working set read while the tools were open, in bytes.
    pub memory_tools_open: Option<u64>,
    /// Private working set one minute after the tools closed, in bytes.
    pub memory_idle_after_use: Option<u64>,
    /// Private working set one minute after the end of one run of 100 calls in a row, in bytes.
    pub memory_idle_after_100_calls: Option<u64>,
    /// Processor time over a window before any call.
    pub cpu_idle_before_first_call: Option<CpuWindow>,
    /// Processor time over the ten minutes after the tools closed.
    pub cpu_idle_after_use: Option<CpuWindow>,
}

/// The process's processor time (kernel and user) over a window of wall-clock time.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CpuWindow {
    pub cpu: Duration,
    pub wall: Duration,
}

impl CpuWindow {
    /// Milliseconds of processor time per minute of the window.
    pub fn ms_per_minute(self) -> f64 {
        if self.wall.is_zero() {
            return 0.0;
        }
        self.cpu.as_secs_f64() * 1000.0 * 60.0 / self.wall.as_secs_f64()
    }
}

/// How many calls the median of the call time is taken over.
pub const CALLS: usize = 10;

/// A megabyte as Task Manager counts it.
pub const MB: u64 = 1024 * 1024;

/// What a goal, or one pass's value of it, came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Verdict {
    Met,
    /// An HDR allowance was exceeded: it goes back to the owner, with no decision taken here.
    OwnerDecides,
    /// The run didn't get the value, so the goal isn't known to be met.
    NotMeasured,
    Missed,
}

impl Verdict {
    pub fn label(self) -> &'static str {
        match self {
            Verdict::Met => "met",
            Verdict::OwnerDecides => "OWNER DECIDES",
            Verdict::NotMeasured => "NOT MEASURED",
            Verdict::Missed => "MISSED",
        }
    }
}

/// One pass's value of a goal: the text shown, and its verdict, or `None` when the goal's condition doesn't judge this
/// pass (the call time with HDR on is judged by the HDR row instead).
#[derive(Debug, Clone, PartialEq)]
pub struct Cell {
    pub text: String,
    pub verdict: Option<Verdict>,
}

/// One goal of the table: its name, its condition, its limit, the value of each pass, and its verdict, the worst of its
/// cells'.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub goal: &'static str,
    pub condition: &'static str,
    pub limit: String,
    pub off: Cell,
    pub on: Cell,
    pub verdict: Verdict,
}

/// The middle of `values`, the mean of the middle two for an even count; `None` when empty.
pub fn median(values: &[Duration]) -> Option<Duration> {
    let mut sorted = values.to_vec();
    sorted.sort();
    let n = sorted.len();
    match n {
        0 => None,
        _ if n % 2 == 1 => Some(sorted[n / 2]),
        _ => Some((sorted[n / 2 - 1] + sorted[n / 2]) / 2),
    }
}

/// The call time a pass measured: the median of its calls, once it has all `CALLS` of them.
fn call_median(pass: &Pass) -> Option<Duration> {
    (pass.calls.len() == CALLS)
        .then(|| median(&pass.calls))
        .flatten()
}

/// Every goal, judged: `off` is the pass with HDR off, `on` the pass with HDR on, `None` when no monitor could turn
/// HDR on or the pass didn't run.
pub fn judge(off: &Pass, on: Option<&Pass>) -> Vec<Row> {
    let none = Pass::default();
    let on_pass = on.unwrap_or(&none);
    let mut rows = Vec::new();

    let mut row = |goal, condition, limit: String, off: Cell, on: Cell| {
        let verdict = [off.verdict, on.verdict]
            .into_iter()
            .flatten()
            .max()
            .unwrap_or(Verdict::Met);
        rows.push(Row {
            goal,
            condition,
            limit,
            off,
            on,
            verdict,
        });
    };

    let call_limit = Duration::from_millis(250);
    row(
        "Call to the toolbar",
        "global shortcut, median of calls 2 to 11, HDR off",
        ms_text(call_limit),
        time_cell(call_median(off), call_limit, true),
        time_cell(call_median(on_pass), call_limit, false),
    );
    let first_limit = Duration::from_secs(1);
    row(
        "First call of the process, opening the exe",
        "from starting ScreenGauge.exe, as the Start menu does",
        ms_text(first_limit),
        time_cell(off.first_call_opening, first_limit, true),
        time_cell(on_pass.first_call_opening, first_limit, true),
    );
    row(
        "First call of the process, by the shortcut",
        "first global shortcut after a start at sign-in",
        ms_text(first_limit),
        time_cell(off.first_call_shortcut, first_limit, true),
        time_cell(on_pass.first_call_shortcut, first_limit, true),
    );
    let hdr_time = Duration::from_millis(100);
    row(
        "Call with HDR on",
        "median with HDR on minus median with HDR off",
        format!("+{}", ms_text(hdr_time)),
        info(""),
        allowance(
            call_median(on_pass).zip(call_median(off)),
            |(on, off)| on.saturating_sub(off) <= hdr_time,
            |(on, off)| signed_ms(on, off),
        ),
    );
    row(
        "Memory before the first call",
        "started at sign-in, no call",
        mb_text(5 * MB),
        memory_cell(off.memory_before_first_call, 5 * MB, true),
        memory_cell(on_pass.memory_before_first_call, 5 * MB, true),
    );
    row(
        "Memory idle after use",
        "1 minute after the tools closed",
        mb_text(80 * MB),
        memory_cell(off.memory_idle_after_use, 80 * MB, true),
        memory_cell(on_pass.memory_idle_after_use, 80 * MB, true),
    );
    row(
        "Memory idle after 100 calls",
        "1 minute after one run of 100 calls in a row",
        mb_text(80 * MB),
        memory_cell(off.memory_idle_after_100_calls, 80 * MB, true),
        memory_cell(on_pass.memory_idle_after_100_calls, 80 * MB, true),
    );
    row(
        "Memory with the tools open",
        "highest reading while open, HDR off",
        mb_text(250 * MB),
        memory_cell(off.memory_tools_open, 250 * MB, true),
        memory_cell(on_pass.memory_tools_open, 250 * MB, false),
    );
    let hdr_memory = 100 * MB;
    row(
        "Memory with the tools open, HDR on",
        "highest with HDR on minus highest with HDR off",
        format!("+{}", mb_text(hdr_memory)),
        info(""),
        allowance(
            on_pass.memory_tools_open.zip(off.memory_tools_open),
            |(on, off)| on.saturating_sub(off) <= hdr_memory,
            |(on, off)| signed_mb(on, off),
        ),
    );
    row(
        "Processor idle before the first call",
        "started at sign-in, no call",
        "5.0 ms per minute".to_string(),
        cpu_cell(off.cpu_idle_before_first_call, 5.0),
        cpu_cell(on_pass.cpu_idle_before_first_call, 5.0),
    );
    row(
        "Processor idle after use",
        "mean of the 10 minutes after the tools closed",
        "200.0 ms per minute".to_string(),
        cpu_cell(off.cpu_idle_after_use, 200.0),
        cpu_cell(on_pass.cpu_idle_after_use, 200.0),
    );
    rows
}

/// A cell shown but not judged.
fn info(text: &str) -> Cell {
    Cell {
        text: text.to_string(),
        verdict: None,
    }
}

fn not_measured() -> Cell {
    Cell {
        text: "not measured".to_string(),
        verdict: Some(Verdict::NotMeasured),
    }
}

/// A value against its limit; `judged` false shows it without a verdict.
fn limit_cell(text: String, within: bool, judged: bool) -> Cell {
    let verdict = match (judged, within) {
        (false, _) => None,
        (true, true) => Some(Verdict::Met),
        (true, false) => Some(Verdict::Missed),
    };
    Cell { text, verdict }
}

fn time_cell(value: Option<Duration>, limit: Duration, judged: bool) -> Cell {
    match value {
        Some(value) => limit_cell(ms_text(value), value <= limit, judged),
        None if judged => not_measured(),
        None => info("not measured"),
    }
}

fn memory_cell(value: Option<u64>, limit: u64, judged: bool) -> Cell {
    match value {
        Some(value) => limit_cell(mb_text(value), value <= limit, judged),
        None if judged => not_measured(),
        None => info("not measured"),
    }
}

fn cpu_cell(value: Option<CpuWindow>, limit: f64) -> Cell {
    match value {
        Some(window) => {
            let per_minute = window.ms_per_minute();
            limit_cell(
                format!("{per_minute:.1} ms per minute"),
                per_minute <= limit,
                true,
            )
        }
        None => not_measured(),
    }
}

/// An HDR allowance: the two passes' values, whether they keep within it, and the difference shown. Exceeding it goes
/// back to the owner.
fn allowance<T: Copy>(
    values: Option<T>,
    within: impl Fn(T) -> bool,
    text: impl Fn(T) -> String,
) -> Cell {
    match values {
        Some(values) => Cell {
            text: text(values),
            verdict: Some(if within(values) {
                Verdict::Met
            } else {
                Verdict::OwnerDecides
            }),
        },
        None => not_measured(),
    }
}

pub fn ms_text(value: Duration) -> String {
    format!("{} ms", value.as_millis())
}

fn signed_ms(on: Duration, off: Duration) -> String {
    if on >= off {
        format!("+{}", ms_text(on - off))
    } else {
        format!("-{}", ms_text(off - on))
    }
}

/// Bytes in Task Manager's megabytes, with one decimal, as its Memory column shows them.
pub fn mb_text(bytes: u64) -> String {
    // Bytes of a process's memory fit an f64's 53 bits exactly up to 8 petabytes.
    format!("{:.1} MB", bytes as f64 / MB as f64)
}

fn signed_mb(on: u64, off: u64) -> String {
    if on >= off {
        format!("+{}", mb_text(on - off))
    } else {
        format!("-{}", mb_text(off - on))
    }
}

/// What the whole run comes to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// Goals missed or not measured: each holds the version.
    pub held: Vec<&'static str>,
    /// HDR allowances exceeded: each goes back to the owner.
    pub owner_decides: Vec<&'static str>,
}

impl Outcome {
    pub fn of(rows: &[Row]) -> Outcome {
        let named = |pick: fn(Verdict) -> bool| {
            rows.iter()
                .filter(|row| pick(row.verdict))
                .map(|row| row.goal)
                .collect()
        };
        Outcome {
            held: named(|v| matches!(v, Verdict::Missed | Verdict::NotMeasured)),
            owner_decides: named(|v| v == Verdict::OwnerDecides),
        }
    }

    pub fn all_met(&self) -> bool {
        self.held.is_empty() && self.owner_decides.is_empty()
    }
}

/// The table as text, one goal per block, then what the run comes to.
pub fn report(rows: &[Row]) -> String {
    let mut text = String::new();
    for row in rows {
        text.push_str(&format!(
            "{:<13} {}\n              condition: {}\n              limit: {}",
            format!("[{}]", row.verdict.label()),
            row.goal,
            row.condition,
            row.limit
        ));
        for (name, cell) in [("HDR off", &row.off), ("HDR on", &row.on)] {
            if cell.text.is_empty() {
                continue;
            }
            text.push_str(&format!("   {name}: {}", cell.text));
            match cell.verdict {
                Some(Verdict::Met) | None => {}
                Some(verdict) => text.push_str(&format!(" ({})", verdict.label())),
            }
        }
        text.push('\n');
    }
    let outcome = Outcome::of(rows);
    text.push('\n');
    if outcome.all_met() {
        text.push_str("Every goal is met.\n");
    }
    if !outcome.held.is_empty() {
        text.push_str(&format!(
            "THE VERSION IS HELD: {} goal(s) missed or not measured: {}.\n",
            outcome.held.len(),
            outcome.held.join("; ")
        ));
    }
    if !outcome.owner_decides.is_empty() {
        text.push_str(&format!(
            "BACK TO THE OWNER: the HDR allowance of {} was exceeded. Nothing was decided automatically.\n",
            outcome.owner_decides.join("; ")
        ));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(value: u64) -> Duration {
        Duration::from_millis(value)
    }

    fn minutes(cpu_ms: u64, minutes: u64) -> CpuWindow {
        CpuWindow {
            cpu: ms(cpu_ms),
            wall: Duration::from_secs(60 * minutes),
        }
    }

    /// The prototype's numbers from the spec's table, every one within its goal.
    fn good_pass() -> Pass {
        Pass {
            first_call_opening: Some(ms(620)),
            first_call_shortcut: Some(ms(450)),
            calls: [150, 160, 170, 180, 175, 165, 155, 158, 162, 168]
                .map(ms)
                .to_vec(),
            memory_before_first_call: Some(1_468_006),
            memory_tools_open: Some(176 * MB),
            memory_idle_after_use: Some(61 * MB),
            memory_idle_after_100_calls: Some(61 * MB),
            cpu_idle_before_first_call: Some(minutes(0, 1)),
            cpu_idle_after_use: Some(minutes(1250, 10)),
        }
    }

    fn row<'a>(rows: &'a [Row], goal: &str) -> &'a Row {
        rows.iter()
            .find(|row| row.goal == goal)
            .unwrap_or_else(|| panic!("no row {goal}"))
    }

    #[test]
    fn the_median_of_an_odd_count_is_the_middle_value_and_of_an_even_one_the_mean_of_the_middle_two()
     {
        assert_eq!(median(&[ms(30), ms(10), ms(20)]), Some(ms(20)));
        assert_eq!(median(&[ms(40), ms(10), ms(30), ms(20)]), Some(ms(25)));
        assert_eq!(median(&[]), None);
    }

    #[test]
    fn processor_time_is_counted_in_milliseconds_per_minute_of_the_window() {
        assert_eq!(minutes(1250, 10).ms_per_minute(), 125.0);
        assert_eq!(
            CpuWindow {
                cpu: ms(3),
                wall: Duration::from_secs(90)
            }
            .ms_per_minute(),
            2.0
        );
    }

    #[test]
    fn memory_is_shown_in_task_manager_s_megabytes() {
        assert_eq!(mb_text(1_468_006), "1.4 MB");
        assert_eq!(mb_text(80 * MB), "80.0 MB");
    }

    #[test]
    fn the_prototype_s_numbers_meet_every_goal() {
        let hdr_on = Pass {
            calls: good_pass().calls.iter().map(|&t| t + ms(47)).collect(),
            memory_tools_open: Some(202 * MB),
            ..good_pass()
        };
        let rows = judge(&good_pass(), Some(&hdr_on));
        assert_eq!(rows.len(), 11);
        for row in &rows {
            assert_eq!(row.verdict, Verdict::Met, "{row:?}");
        }
        assert_eq!(row(&rows, "Call to the toolbar").off.text, "163 ms");
        assert_eq!(row(&rows, "Call with HDR on").on.text, "+47 ms");
        assert_eq!(
            row(&rows, "Memory with the tools open, HDR on").on.text,
            "+26.0 MB"
        );
        assert_eq!(
            row(&rows, "Processor idle after use").off.text,
            "125.0 ms per minute"
        );
        assert!(Outcome::of(&rows).all_met());
        assert!(report(&rows).contains("Every goal is met."));
    }

    #[test]
    fn a_value_on_its_limit_meets_it_and_one_past_it_misses() {
        let on_limit = Pass {
            calls: vec![ms(250); CALLS],
            memory_idle_after_use: Some(80 * MB),
            cpu_idle_before_first_call: Some(minutes(5, 1)),
            ..good_pass()
        };
        let rows = judge(&on_limit, None);
        assert_eq!(row(&rows, "Call to the toolbar").verdict, Verdict::Met);
        assert_eq!(
            row(&rows, "Memory idle after use").off.verdict,
            Some(Verdict::Met)
        );
        assert_eq!(
            row(&rows, "Processor idle before the first call")
                .off
                .verdict,
            Some(Verdict::Met)
        );

        let past = Pass {
            calls: vec![ms(251); CALLS],
            memory_idle_after_use: Some(80 * MB + 1),
            cpu_idle_before_first_call: Some(minutes(6, 1)),
            ..good_pass()
        };
        let rows = judge(&past, Some(&good_pass()));
        assert_eq!(row(&rows, "Call to the toolbar").verdict, Verdict::Missed);
        assert_eq!(row(&rows, "Memory idle after use").verdict, Verdict::Missed);
        assert_eq!(
            row(&rows, "Processor idle before the first call").verdict,
            Verdict::Missed
        );
        let outcome = Outcome::of(&rows);
        assert_eq!(
            outcome.held,
            [
                "Call to the toolbar",
                "Memory idle after use",
                "Processor idle before the first call"
            ]
        );
        let text = report(&rows);
        assert!(text.contains("[MISSED]      Call to the toolbar"), "{text}");
        assert!(text.contains("HDR off: 251 ms (MISSED)"), "{text}");
        assert!(text.contains("THE VERSION IS HELD: 3 goal(s)"), "{text}");
    }

    #[test]
    fn a_goal_judged_in_both_passes_misses_when_the_hdr_pass_misses_it() {
        let hdr_on = Pass {
            memory_idle_after_100_calls: Some(95 * MB),
            first_call_opening: Some(ms(1001)),
            ..good_pass()
        };
        let rows = judge(&good_pass(), Some(&hdr_on));
        let memory = row(&rows, "Memory idle after 100 calls");
        assert_eq!(memory.off.verdict, Some(Verdict::Met));
        assert_eq!(memory.on.verdict, Some(Verdict::Missed));
        assert_eq!(memory.verdict, Verdict::Missed);
        assert_eq!(
            row(&rows, "First call of the process, opening the exe").verdict,
            Verdict::Missed
        );
    }

    #[test]
    fn the_250_ms_and_250_mb_limits_judge_only_the_pass_with_hdr_off() {
        // With HDR on, these two are judged by their HDR allowances instead.
        let hdr_on = Pass {
            calls: vec![ms(300); CALLS],
            memory_tools_open: Some(300 * MB),
            ..good_pass()
        };
        let off = Pass {
            calls: vec![ms(240); CALLS],
            memory_tools_open: Some(240 * MB),
            ..good_pass()
        };
        let rows = judge(&off, Some(&hdr_on));
        let call = row(&rows, "Call to the toolbar");
        assert_eq!(call.on.verdict, None);
        assert_eq!(call.on.text, "300 ms");
        assert_eq!(call.verdict, Verdict::Met);
        assert_eq!(
            row(&rows, "Memory with the tools open").verdict,
            Verdict::Met
        );
        assert!(Outcome::of(&rows).all_met());
    }

    #[test]
    fn an_hdr_allowance_exceeded_goes_back_to_the_owner_without_holding_the_version() {
        let hdr_on = Pass {
            calls: vec![ms(271); CALLS],
            memory_tools_open: Some(277 * MB),
            ..good_pass()
        };
        let off = Pass {
            calls: vec![ms(170); CALLS],
            memory_tools_open: Some(176 * MB),
            ..good_pass()
        };
        let rows = judge(&off, Some(&hdr_on));
        let time = row(&rows, "Call with HDR on");
        assert_eq!(time.on.text, "+101 ms");
        assert_eq!(time.verdict, Verdict::OwnerDecides);
        let memory = row(&rows, "Memory with the tools open, HDR on");
        assert_eq!(memory.on.text, "+101.0 MB");
        assert_eq!(memory.verdict, Verdict::OwnerDecides);

        let outcome = Outcome::of(&rows);
        assert!(outcome.held.is_empty());
        assert_eq!(
            outcome.owner_decides,
            ["Call with HDR on", "Memory with the tools open, HDR on"]
        );
        assert!(!outcome.all_met());
        let text = report(&rows);
        assert!(text.contains("[OWNER DECIDES] Call with HDR on"), "{text}");
        assert!(text.contains("BACK TO THE OWNER"), "{text}");
        assert!(!text.contains("THE VERSION IS HELD"), "{text}");
    }

    #[test]
    fn hdr_on_faster_than_off_keeps_within_the_allowance() {
        let hdr_on = Pass {
            calls: vec![ms(160); CALLS],
            memory_tools_open: Some(170 * MB),
            ..good_pass()
        };
        let rows = judge(&good_pass(), Some(&hdr_on));
        assert_eq!(row(&rows, "Call with HDR on").on.text, "-3 ms");
        assert_eq!(
            row(&rows, "Memory with the tools open, HDR on").on.text,
            "-6.0 MB"
        );
        assert!(Outcome::of(&rows).all_met());
    }

    #[test]
    fn a_value_the_run_did_not_get_holds_the_version() {
        let short = Pass {
            calls: vec![ms(150); CALLS - 1],
            memory_idle_after_use: None,
            cpu_idle_after_use: None,
            ..good_pass()
        };
        let rows = judge(&short, Some(&good_pass()));
        assert_eq!(row(&rows, "Call to the toolbar").off.text, "not measured");
        let outcome = Outcome::of(&rows);
        assert_eq!(
            outcome.held,
            [
                "Call to the toolbar",
                "Call with HDR on",
                "Memory idle after use",
                "Processor idle after use"
            ]
        );
    }

    #[test]
    fn without_an_hdr_pass_every_hdr_value_is_not_measured_and_the_version_is_held() {
        let rows = judge(&good_pass(), None);
        let outcome = Outcome::of(&rows);
        assert!(outcome.held.contains(&"Call with HDR on"));
        assert!(outcome.held.contains(&"Memory with the tools open, HDR on"));
        assert!(outcome.held.contains(&"Memory before the first call"));
        // The rows HDR on doesn't judge stay as they were.
        assert!(!outcome.held.contains(&"Call to the toolbar"));
        assert!(!outcome.held.contains(&"Memory with the tools open"));
    }
}
