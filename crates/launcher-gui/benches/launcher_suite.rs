//! The Better Launcher benchmark harness: one command, one summary table.
//!
//! `components/manifests/better-launcher.yaml` declares five launcher-level
//! measurements. Before ticket 37 nothing ran any of them, so this file exists
//! to make each one a number somebody can check rather than a promise.
//! `launcher_gui::BENCHMARKS` is the list, and a test fails if the manifest
//! drifts from it.
//!
//! ```text
//! cargo bench -p launcher-gui --bench launcher_suite
//! cargo bench -p launcher-gui --bench launcher_suite -- --quick     # smaller, faster
//! cargo bench -p launcher-gui --bench launcher_suite -- --no-spawn  # skip the three process measurements
//! ```
//!
//! **What is measured, and where the boundary is.** Three of the five need a
//! running `better-launcher` process. It is launched with `ZED_HEADLESS=1`, so
//! there is no compositor, no surface, and therefore no frame. What
//! `warm-overlay-open` reports is *process start to first renderable model*:
//! the point at which the overlay entity exists, the search row holds focus,
//! and the application library is in the model — everything a first frame would
//! draw, drawn by nothing. Compositor handoff, surface allocation, GPU upload,
//! and present-to-photon are outside it and are not estimated here.
//! `docs/launcher-performance.md` states this again beside the numbers, because
//! a millisecond figure labelled "overlay open" invites being read as the whole
//! thing.
//!
//! **What the fixture is.** A synthetic XDG data directory with 5,000 generated
//! `.desktop` entries, written by this harness on every run. Nothing here reads
//! the host's own application list, so the numbers do not depend on what
//! happens to be installed. There is no benchmark-framework dependency, which
//! matches every other benchmark in this workspace.
//!
//! **What is deliberately excluded.** The spawned process is pointed at an
//! unreachable `DBUS_SESSION_BUS_ADDRESS`, so it always takes the
//! "no session bus, opening without single-instance" path. That makes each
//! spawn deterministically the primary instance instead of forwarding its
//! request to whatever else is running, at the cost of not measuring a
//! successful bus connection. The failed connection attempt is inside the
//! figure; a successful one is not.

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use app_catalog_platform::{ApplicationDirectories, SessionEnvironment};
use launcher_gui::model::OverlayModel;
use launcher_gui::startup::{STAGE_LIBRARY_READY, STAGE_SHELL_READY, TRACE_PREFIX};
use launcher_platform::catalog::{MetadataWatch, SETTLE, load_snapshot};

/// Issue #2's record count for the warm search claim.
const RECORD_COUNT: usize = 5_000;

/// Issue #2's per-keystroke target.
const SEARCH_TARGET_MS: f64 = 50.0;

/// One row of the summary table.
struct Row {
    benchmark: &'static str,
    measurement: String,
    value: String,
    detail: String,
}

impl Row {
    fn new(
        benchmark: &'static str,
        measurement: impl Into<String>,
        value: impl Into<String>,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            benchmark,
            measurement: measurement.into(),
            value: value.into(),
            detail: detail.into(),
        }
    }
}

fn ms(duration: Duration) -> String {
    format!("{:.3} ms", duration.as_secs_f64() * 1000.0)
}

fn percentile(samples: &mut [Duration], percent: f64) -> Duration {
    samples.sort();
    let rank = ((percent / 100.0) * samples.len() as f64).ceil() as usize;
    samples[rank.saturating_sub(1).min(samples.len() - 1)]
}

// --- The synthetic XDG directory -----------------------------------------

const BASE_NAMES: [&str; 20] = [
    "Text Editor",
    "Files",
    "Terminal",
    "Calculator",
    "Disk Usage Analyzer",
    "System Monitor",
    "Image Viewer",
    "Archive Manager",
    "Video Player",
    "Music Player",
    "Vector Graphics Editor",
    "Web Browser",
    "Mail Client",
    "Calendar",
    "Maps",
    "Notes",
    "Software",
    "Settings",
    "Remote Desktop",
    "Word Processor",
];

const CATEGORIES: [&str; 6] = [
    "Utility",
    "Development",
    "Graphics",
    "Network",
    "Office",
    "System",
];

fn entry_text(index: usize) -> String {
    let base = BASE_NAMES[index % BASE_NAMES.len()];
    let name = if index < BASE_NAMES.len() {
        base.to_string()
    } else {
        format!("{base} {index}")
    };
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name={name}\n\
         Name[zh_TW]=文字編輯器 {index}\n\
         GenericName=Synthetic Tool\n\
         Comment=A generated entry used only for benchmarking\n\
         Icon=application-x-executable\n\
         Exec=synthetic-app-{index} --profile default %F\n\
         Terminal=false\n\
         Categories={category};\n\
         Keywords=synthetic;benchmark;generated;tool{index};\n",
        name = name,
        index = index,
        category = CATEGORIES[index % CATEGORIES.len()]
    )
}

/// Writes `count` entries into `<root>/data/applications` and returns the
/// session that reads exactly that directory and nothing else.
fn build_fixture(root: &Path, count: usize) -> SessionEnvironment {
    let applications = root.join("data/applications");
    fs::create_dir_all(&applications).expect("fixture directory");
    // An empty system directory, so `$XDG_DATA_DIRS` never falls back to the
    // specification's default and pulls in the host's own applications.
    fs::create_dir_all(root.join("share/applications")).expect("fixture system directory");
    for index in 0..count {
        fs::write(
            applications.join(format!("synthetic-{index}.desktop")),
            entry_text(index),
        )
        .expect("fixture entry");
    }
    session_for(root)
}

fn session_for(root: &Path) -> SessionEnvironment {
    let data_home = root.join("data");
    let data_dirs = root.join("share");
    SessionEnvironment {
        directories: ApplicationDirectories::from_values(
            Some(&data_home),
            None,
            data_dirs.to_str(),
        ),
        ..SessionEnvironment::default()
    }
}

// --- warm-search-update ---------------------------------------------------

/// One editing session per query: type it out a character at a time, then
/// delete it back down. Both directions are keystrokes somebody waits on.
fn keystrokes(query: &str) -> Vec<String> {
    let characters: Vec<char> = query.chars().collect();
    let mut states: Vec<String> = (1..=characters.len())
        .map(|length| characters[..length].iter().collect())
        .collect();
    states.extend(
        (1..characters.len())
            .rev()
            .map(|length| characters[..length].iter().collect::<String>()),
    );
    states
}

/// A single-word query, a multi-word query, an acronym, a CJK query, a long
/// specific query, a query only a fuzzy match answers, and one that matches
/// nothing.
const QUERY_SCRIPT: [&str; 7] = [
    "text editor",
    "monitor",
    "vge",
    "文字編輯",
    "disk usage analyzer 4200",
    "txtedtr",
    "qxzjv",
];

fn warm_search_update(session: &SessionEnvironment, repeats: usize) -> Vec<Row> {
    let clock = Instant::now();
    let snapshot = load_snapshot(session, None);
    let index_build = clock.elapsed();
    let visible = snapshot.visible();

    let mut model = OverlayModel::new();
    model.set_columns(8);
    model.apply_snapshot(snapshot);

    let mut samples = Vec::new();
    let mut clear_samples = Vec::new();
    let mut widest = 0usize;
    for _ in 0..repeats {
        for query in QUERY_SCRIPT {
            for state in keystrokes(query) {
                let clock = Instant::now();
                model.set_query(state.as_str());
                samples.push(clock.elapsed());
                widest = widest.max(model.rows().len());
            }
            let clock = Instant::now();
            model.clear_query();
            clear_samples.push(clock.elapsed());
        }
    }

    let count = samples.len();
    let p50 = percentile(&mut samples.clone(), 50.0);
    let p95 = percentile(&mut samples.clone(), 95.0);
    let worst = percentile(&mut samples, 100.0);
    let p95_ms = p95.as_secs_f64() * 1000.0;

    vec![
        Row::new(
            "warm-search-update",
            "keystroke to updated result model, p95",
            ms(p95),
            format!(
                "{} the {SEARCH_TARGET_MS:.0} ms target · {count} keystrokes over {visible} applications",
                if p95_ms <= SEARCH_TARGET_MS {
                    "within"
                } else {
                    "OVER"
                }
            ),
        ),
        Row::new(
            "warm-search-update",
            "keystroke to updated result model, p50",
            ms(p50),
            format!("widest result set drawn: {widest} rows"),
        ),
        Row::new(
            "warm-search-update",
            "keystroke to updated result model, worst",
            ms(worst),
            "no warm-up discarded; the first keystroke of the run is in here".to_string(),
        ),
        Row::new(
            "warm-search-update",
            "clearing the query back to the library",
            ms(percentile(&mut clear_samples, 95.0)),
            "p95; the library is borrowed, not rebuilt".to_string(),
        ),
        Row::new(
            "warm-search-update",
            "index build behind the warm state",
            ms(index_build),
            format!("read and indexed {visible} entries once, before timing"),
        ),
    ]
}

// --- application-list-update ---------------------------------------------

/// A directory event through the real watcher and into the model.
///
/// The clock starts at the write and stops when `OverlayModel` is showing the
/// new list, so the settle window the watcher applies to a burst of events is
/// inside every figure. It is reported separately as well, because 150 ms of a
/// 160 ms number is a policy choice and not a cost.
fn application_list_update(root: &Path, cycles: usize) -> Vec<Row> {
    let session = build_fixture(root, 24);
    let applications = root.join("data/applications");
    let watch = MetadataWatch::start(&session).expect("the fixture directory is watchable");
    let backend = watch.backend();

    let mut model = OverlayModel::new();
    model.apply_snapshot(load_snapshot(&session, None));
    let baseline = model.rows().len();

    let mut installs = Vec::new();
    let mut removals = Vec::new();
    for cycle in 0..cycles {
        let path = applications.join(format!("installed-{cycle}.desktop"));

        let clock = Instant::now();
        fs::write(&path, entry_text(900_000 + cycle)).expect("install");
        let observed = watch.next_change(Duration::from_secs(10));
        model.begin_refresh();
        model.apply_snapshot(load_snapshot(&session, None));
        installs.push(clock.elapsed());
        assert!(observed.is_some(), "the watcher missed an installed entry");
        assert_eq!(
            model.rows().len(),
            baseline + 1,
            "the model did not pick the new application up"
        );

        let clock = Instant::now();
        fs::remove_file(&path).expect("remove");
        let observed = watch.next_change(Duration::from_secs(10));
        model.begin_refresh();
        model.apply_snapshot(load_snapshot(&session, None));
        removals.push(clock.elapsed());
        assert!(observed.is_some(), "the watcher missed a removed entry");
        assert_eq!(model.rows().len(), baseline);
    }

    let install_p95 = percentile(&mut installs.clone(), 95.0);
    let removal_p95 = percentile(&mut removals.clone(), 95.0);
    let install_work = install_p95.saturating_sub(SETTLE);

    vec![
        Row::new(
            "application-list-update",
            "install to refreshed model, p95",
            ms(install_p95),
            format!("{cycles} cycles over {baseline} applications · watcher backend {backend:?}"),
        ),
        Row::new(
            "application-list-update",
            "removal to refreshed model, p95",
            ms(removal_p95),
            "same path; a deletion is the same event class".to_string(),
        ),
        Row::new(
            "application-list-update",
            "the same, minus the settle window",
            ms(install_work),
            format!(
                "the other {} is the deliberate burst-coalescing wait, not a cost",
                ms(SETTLE)
            ),
        ),
    ]
}

// --- The three measurements that need a running process -------------------

/// A line the spawned launcher printed, with the moment this process saw it.
struct TraceLine {
    stage: String,
    child_ms: f64,
    detail: String,
    seen_at: Instant,
}

fn parse_trace(line: &str) -> Option<(String, f64, String)> {
    let rest = line.strip_prefix(TRACE_PREFIX)?.trim();
    // `detail` is last and may contain spaces, so it is split off whole.
    let (head, detail) = rest.split_once(" detail=")?;
    let mut stage = None;
    let mut child_ms = None;
    for field in head.split(' ') {
        if let Some(value) = field.strip_prefix("stage=") {
            stage = Some(value.to_string());
        } else if let Some(value) = field.strip_prefix("ms=") {
            child_ms = value.parse::<f64>().ok();
        }
    }
    Some((stage?, child_ms?, detail.to_string()))
}

/// A launcher process under `ZED_HEADLESS=1`, and the trace lines it produced.
struct Overlay {
    child: Child,
    lines: mpsc::Receiver<TraceLine>,
    spawned_at: Instant,
}

impl Overlay {
    fn spawn(fixture_root: &Path) -> Result<Self, String> {
        let binary = env!("CARGO_BIN_EXE_better-launcher");
        let spawned_at = Instant::now();
        let mut child = Command::new(binary)
            .arg("--open")
            .env("ZED_HEADLESS", "1")
            .env(launcher_gui::startup::TRACE_VARIABLE, "1")
            .env("XDG_DATA_HOME", fixture_root.join("data"))
            .env("XDG_DATA_DIRS", fixture_root.join("share"))
            .env("XDG_CURRENT_DESKTOP", "GNOME")
            // Deterministically the primary instance; see the module note.
            .env(
                "DBUS_SESSION_BUS_ADDRESS",
                "unix:path=/nonexistent/better-launcher-benchmark",
            )
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| format!("could not start {binary}: {error}"))?;

        let stderr = child.stderr.take().expect("stderr was piped");
        let (sender, lines) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                if let Some((stage, child_ms, detail)) = parse_trace(&line) {
                    let _ = sender.send(TraceLine {
                        stage,
                        child_ms,
                        detail,
                        seen_at: Instant::now(),
                    });
                }
            }
        });

        Ok(Self {
            child,
            lines,
            spawned_at,
        })
    }

    /// Waits for one stage, giving up rather than hanging if the process died
    /// or never got there.
    fn wait_for(&mut self, stage: &str, timeout: Duration) -> Result<TraceLine, String> {
        let deadline = Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(format!("no {stage} line within {timeout:?}"));
            }
            match self
                .lines
                .recv_timeout(remaining.min(Duration::from_millis(250)))
            {
                Ok(line) if line.stage == stage => return Ok(line),
                Ok(_) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(format!(
                        "the launcher exited before reporting {stage} (status {:?})",
                        self.child.try_wait().ok().flatten()
                    ));
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if let Ok(Some(status)) = self.child.try_wait() {
                        return Err(format!(
                            "the launcher exited with {status} before reporting {stage}"
                        ));
                    }
                }
            }
        }
    }

    fn pid(&self) -> u32 {
        self.child.id()
    }
}

impl Drop for Overlay {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn warm_overlay_open(fixture_root: &Path, spawns: usize) -> Vec<Row> {
    let mut shell = Vec::new();
    let mut library = Vec::new();
    let mut wall = Vec::new();
    let mut applications = String::new();
    let mut failures = Vec::new();

    // One discarded spawn first: the very first execution of a release binary
    // pays for reading it off disk, and "warm" is the claim being made.
    for attempt in 0..(spawns + 1) {
        let mut overlay = match Overlay::spawn(fixture_root) {
            Ok(overlay) => overlay,
            Err(reason) => {
                failures.push(reason);
                break;
            }
        };
        let shell_line = overlay.wait_for(STAGE_SHELL_READY, Duration::from_secs(60));
        let library_line = overlay.wait_for(STAGE_LIBRARY_READY, Duration::from_secs(60));
        match (shell_line, library_line) {
            (Ok(shell_line), Ok(library_line)) => {
                if attempt > 0 {
                    shell.push(Duration::from_secs_f64(shell_line.child_ms / 1000.0));
                    library.push(Duration::from_secs_f64(library_line.child_ms / 1000.0));
                    wall.push(library_line.seen_at - overlay.spawned_at);
                }
                applications = library_line.detail;
            }
            (Err(reason), _) | (_, Err(reason)) => {
                failures.push(reason);
                break;
            }
        }
    }

    if wall.is_empty() {
        return vec![Row::new(
            "warm-overlay-open",
            "process start to first renderable model",
            "not measured".to_string(),
            failures
                .first()
                .cloned()
                .unwrap_or_else(|| "no successful spawn".to_string()),
        )];
    }

    vec![
        Row::new(
            "warm-overlay-open",
            "spawn to first renderable model, p95",
            ms(percentile(&mut wall.clone(), 95.0)),
            format!(
                "wall clock in this process, {} spawns, first discarded · {applications} · no compositor: no surface, no frame",
                wall.len()
            ),
        ),
        Row::new(
            "warm-overlay-open",
            "spawn to first renderable model, p50",
            ms(percentile(&mut wall, 50.0)),
            "includes fork, exec, and dynamic linking".to_string(),
        ),
        Row::new(
            "warm-overlay-open",
            "of which: main() to focused search row",
            ms(percentile(&mut shell, 50.0)),
            "p50 by the child's own clock; window open, library still loading".to_string(),
        ),
        Row::new(
            "warm-overlay-open",
            "of which: main() to the library in the model",
            ms(percentile(&mut library, 50.0)),
            "p50 by the child's own clock; reading and indexing runs off the render thread"
                .to_string(),
        ),
    ]
}

/// Clock ticks per second, which is what `/proc/[pid]/stat` counts in.
fn clock_ticks() -> f64 {
    Command::new("getconf")
        .arg("CLK_TCK")
        .output()
        .ok()
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .and_then(|value| value.trim().parse::<f64>().ok())
        .unwrap_or(100.0)
}

/// CPU time a process has used, read two ways.
///
/// Both are kept because either one alone can be misread. `/proc/[pid]/stat`
/// counts in clock ticks, so over a 20-second window it can only say "0 % or
/// 0.05 %" — an idle figure of zero taken from it is reporting the instrument.
/// `/proc/[pid]/schedstat` counts nanoseconds on the CPU, but its first field
/// is only updated when the task is descheduled, and a kernel that never
/// populated it would read as a permanent zero. Reporting the two together
/// means a zero has to be a zero in both.
#[derive(Clone, Copy)]
struct CpuSample {
    scheduled: Duration,
    ticked: Duration,
}

/// Nanoseconds this task has spent on a CPU, from `/proc/[pid]/schedstat`.
fn scheduled_time(pid: u32) -> Option<Duration> {
    fs::read_to_string(format!("/proc/{pid}/schedstat"))
        .ok()?
        .split_whitespace()
        .next()?
        .parse::<u64>()
        .ok()
        .map(Duration::from_nanos)
}

/// User plus system time from `/proc/[pid]/stat`, in clock ticks.
fn ticked_time(pid: u32) -> Option<Duration> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // The comm field is parenthesised and may contain spaces, so the fields are
    // counted from after the last ')'.
    let fields: Vec<&str> = stat.rsplit_once(')')?.1.split_whitespace().collect();
    // stat(5) fields 14 and 15 are utime and stime; field 3 is the first here.
    let utime: u64 = fields.get(11)?.parse().ok()?;
    let stime: u64 = fields.get(12)?.parse().ok()?;
    Some(Duration::from_secs_f64(
        (utime + stime) as f64 / clock_ticks(),
    ))
}

fn cpu_sample(pid: u32) -> Option<CpuSample> {
    Some(CpuSample {
        scheduled: scheduled_time(pid)?,
        ticked: ticked_time(pid)?,
    })
}

/// One `Vm…` line of `/proc/[pid]/status`, in kilobytes.
fn memory_kilobytes(pid: u32, field: &str) -> Option<u64> {
    fs::read_to_string(format!("/proc/{pid}/status"))
        .ok()?
        .lines()
        .find_map(|line| line.strip_prefix(field))
        .and_then(|value| value.split_whitespace().next())
        .and_then(|value| value.parse::<u64>().ok())
}

fn idle_overhead(fixture_root: &Path, window: Duration) -> Vec<Row> {
    let mut overlay = match Overlay::spawn(fixture_root) {
        Ok(overlay) => overlay,
        Err(reason) => return not_measured(reason),
    };
    if let Err(reason) = overlay.wait_for(STAGE_LIBRARY_READY, Duration::from_secs(60)) {
        return not_measured(reason);
    }
    let pid = overlay.pid();

    // Let the process settle first: the cost of opening is `warm-overlay-open`'s
    // number, and charging it to idle would make idle look like work.
    std::thread::sleep(Duration::from_secs(2));

    let Some(before) = cpu_sample(pid) else {
        return not_measured(format!("/proc/{pid} is unreadable"));
    };
    let started = Instant::now();
    std::thread::sleep(window);
    let elapsed = started.elapsed();
    let Some(after) = cpu_sample(pid) else {
        return not_measured(format!("/proc/{pid} disappeared during the idle window"));
    };
    let Some(resident) = memory_kilobytes(pid, "VmRSS:") else {
        return not_measured(format!("/proc/{pid}/status has no VmRSS"));
    };
    let peak = memory_kilobytes(pid, "VmHWM:").unwrap_or(resident);

    let scheduled = after.scheduled.saturating_sub(before.scheduled);
    let ticked = after.ticked.saturating_sub(before.ticked);
    let cpu_percent = scheduled.as_secs_f64() / elapsed.as_secs_f64() * 100.0;

    vec![
        Row::new(
            "idle-overhead",
            "CPU over the idle window",
            format!("{cpu_percent:.4} %"),
            format!(
                "{:.0} s window · scheduled {} · ticks {} · opening the overlay had already \
                 accounted {} to the same counter, so it is live",
                elapsed.as_secs_f64(),
                ms(scheduled),
                ms(ticked),
                ms(before.scheduled)
            ),
        ),
        Row::new(
            "idle-memory",
            "resident set after the idle window",
            format!("{resident} kB"),
            format!("peak {peak} kB · headless: no GPU or surface allocation in this figure"),
        ),
    ]
}

fn not_measured(reason: String) -> Vec<Row> {
    vec![
        Row::new(
            "idle-overhead",
            "CPU over the idle window",
            "not measured".to_string(),
            reason.clone(),
        ),
        Row::new(
            "idle-memory",
            "resident set after the idle window",
            "not measured".to_string(),
            reason,
        ),
    ]
}

// --- Entry point ----------------------------------------------------------

fn main() {
    let arguments: Vec<String> = std::env::args().collect();
    let quick = arguments
        .iter()
        .any(|argument| argument == "--quick" || argument == "--test");
    let no_spawn = arguments.iter().any(|argument| argument == "--no-spawn");

    let records = if quick { 500 } else { RECORD_COUNT };
    let repeats = if quick { 1 } else { 3 };
    let cycles = if quick { 2 } else { 10 };
    let spawns = if quick { 1 } else { 5 };
    let idle_window = if quick {
        Duration::from_secs(3)
    } else {
        Duration::from_secs(20)
    };

    println!("Better Launcher benchmark suite");
    println!(
        "{records} synthetic applications · {repeats}× keystroke script · {cycles} install/remove cycles · \
         {spawns} headless spawns · {:.0} s idle window",
        idle_window.as_secs_f64()
    );
    println!();

    let search_root = tempfile::tempdir().expect("fixture root");
    let clock = Instant::now();
    let session = build_fixture(search_root.path(), records);
    println!(
        "fixture: {records} desktop entries written in {:.1} s",
        clock.elapsed().as_secs_f64()
    );

    let mut rows = warm_search_update(&session, repeats);

    let watch_root = tempfile::tempdir().expect("watch fixture root");
    rows.extend(application_list_update(watch_root.path(), cycles));

    if no_spawn {
        rows.push(Row::new(
            "warm-overlay-open",
            "process start to first renderable model",
            "skipped".to_string(),
            "--no-spawn was passed".to_string(),
        ));
        rows.extend(not_measured("--no-spawn was passed".to_string()));
    } else {
        rows.extend(warm_overlay_open(search_root.path(), spawns));
        rows.extend(idle_overhead(search_root.path(), idle_window));
    }

    println!();
    println!(
        "{:<26} {:<48} {:>14}   notes",
        "benchmark", "measurement", "value"
    );
    println!("{}", "-".repeat(140));
    for row in &rows {
        println!(
            "{:<26} {:<48} {:>14}   {}",
            row.benchmark, row.measurement, row.value, row.detail
        );
    }
    println!();
    println!(
        "Manifest definitions: {}.",
        launcher_gui::BENCHMARKS
            .iter()
            .map(|(name, _, _)| *name)
            .collect::<Vec<_>>()
            .join(", ")
    );
    println!(
        "Without a compositor there is no frame: `warm-overlay-open` ends at the first \
         renderable model, not at a photon. See docs/launcher-performance.md."
    );
}
