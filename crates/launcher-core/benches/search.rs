//! Launcher benchmarks over a 5,000-record synthetic index.
//!
//! Issue #2 sets one hard number: warm search update p95 below 50 ms for a
//! synthetic index of 5,000 application records. That is a per-keystroke
//! claim, so it is measured per keystroke, over a script that types, keeps
//! typing, backspaces, and types multi-word and CJK queries — not over one
//! convenient string.
//!
//! There is no benchmark harness dependency here on purpose, matching
//! `app-catalog-core/benches`. These are wall-clock timings of the real public
//! API; a statistics framework would add a large dependency without changing
//! any decision this crate makes.
//!
//! The records are built in memory from generated desktop-entry text. No file
//! is written, because discovery cost belongs to `app-catalog-core`'s
//! benchmarks and would only hide the numbers this crate is responsible for.
//!
//! Run with `cargo bench -p launcher-core`, or
//! `cargo bench -p launcher-core -- --test` for a single-iteration smoke run.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use app_catalog_core::{
    ApplicationRecord, DesktopFile, DesktopId, EntryScope, ExecutableProbe, Locale,
};
use launcher_core::{
    InMemoryUsage, IndexOptions, LaunchEvent, LauncherState, NoUsage, RankingOptions, SearchIndex,
    UsageStore,
};

/// The record count the ticket sets as the working target.
const RECORD_COUNT: usize = 5_000;

/// Names that look like a real application list rather than 5,000 copies of
/// one string, so query selectivity varies the way it does on a real desktop.
const BASE_NAMES: [&str; 40] = [
    "Text Editor",
    "Files",
    "Terminal",
    "Calculator",
    "Disk Usage Analyzer",
    "System Monitor",
    "Image Viewer",
    "Document Scanner",
    "Archive Manager",
    "Font Viewer",
    "Screen Reader",
    "Video Player",
    "Music Player",
    "Photo Manager",
    "Vector Graphics Editor",
    "Raster Graphics Editor",
    "Web Browser",
    "Mail Client",
    "Calendar",
    "Contacts",
    "Maps",
    "Weather",
    "Clocks",
    "Notes",
    "Passwords and Keys",
    "Software",
    "Settings",
    "Printers",
    "Power Statistics",
    "Startup Applications",
    "Remote Desktop",
    "Virtual Machine Manager",
    "Package Installer",
    "Partition Editor",
    "Backup Tool",
    "Torrent Client",
    "Chat Client",
    "Spreadsheet",
    "Presentation",
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

/// Resolves every synthetic program without touching the filesystem, so the
/// numbers measure indexing rather than the host's `PATH`.
struct SyntheticProbe;

impl ExecutableProbe for SyntheticProbe {
    fn resolve(&self, program: &str) -> Option<PathBuf> {
        Some(PathBuf::from("/usr/bin").join(program))
    }
}

fn synthetic_entry(index: usize) -> String {
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
         Name[de]=Texteditor {index}\n\
         GenericName=Synthetic Tool\n\
         GenericName[zh_TW]=合成工具\n\
         Comment=A generated entry used only for benchmarking\n\
         Icon=application-x-executable\n\
         Exec=synthetic-app-{index} --profile default %F\n\
         TryExec=synthetic-app-{index}\n\
         Terminal=false\n\
         Categories={category};\n\
         Keywords=synthetic;benchmark;generated;tool{index};\n\
         Keywords[zh_TW]=合成;測試;\n",
        name = name,
        index = index,
        category = CATEGORIES[index % CATEGORIES.len()]
    )
}

fn synthetic_records(count: usize) -> Vec<ApplicationRecord> {
    (0..count)
        .map(|index| {
            let text = synthetic_entry(index);
            let file = DesktopFile::parse(&text).expect("synthetic entry parses");
            ApplicationRecord::from_desktop_file(
                DesktopId::new(format!("synthetic-{index}.desktop")).expect("valid desktop id"),
                PathBuf::from(format!("/usr/share/applications/synthetic-{index}.desktop")),
                EntryScope::System,
                &file,
                &SyntheticProbe,
            )
            .expect("synthetic record")
        })
        .collect()
}

/// One realistic editing session per query: type it out one character at a
/// time, then delete it back down. Both directions are keystrokes a user
/// waits on.
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

/// The script the per-keystroke numbers are measured over. It covers a
/// single-word query, a multi-word query, an acronym, a CJK query, a long
/// specific query, a query that only a fuzzy match can answer, and a query
/// that matches nothing.
const QUERY_SCRIPT: [&str; 7] = [
    "text editor",
    "monitor",
    "vge",
    "文字編輯",
    "disk usage analyzer 4200",
    "txtedtr",
    "qxzjv",
];

fn percentile(sorted: &[Duration], percent: f64) -> Duration {
    if sorted.is_empty() {
        return Duration::ZERO;
    }
    let rank = (percent / 100.0 * sorted.len() as f64).ceil() as usize;
    sorted[rank.clamp(1, sorted.len()) - 1]
}

fn median(mut samples: Vec<Duration>) -> Duration {
    samples.sort();
    samples[samples.len() / 2]
}

fn measure<T>(iterations: usize, mut body: impl FnMut() -> T) -> (Duration, T) {
    let mut samples = Vec::with_capacity(iterations);
    let mut last = None;
    for _ in 0..iterations {
        let started = Instant::now();
        let value = body();
        samples.push(started.elapsed());
        last = Some(value);
    }
    (median(samples), last.expect("at least one iteration"))
}

fn report(label: &str, duration: Duration, detail: &str) {
    println!(
        "{label:<38} {:>10.3} ms   {detail}",
        duration.as_secs_f64() * 1000.0
    );
}

/// Runs the whole script once and returns one duration per keystroke.
fn run_script(
    index: &SearchIndex,
    options: &RankingOptions,
    usage: &dyn UsageStore,
) -> (Vec<Duration>, usize) {
    let mut state = LauncherState::new();
    let mut samples = Vec::new();
    let mut hits = 0usize;
    for query in QUERY_SCRIPT {
        for typed in keystrokes(query) {
            state.set_query(typed);
            let started = Instant::now();
            let view = state.view(index, options, usage);
            let count = view.applications().len();
            samples.push(started.elapsed());
            hits += count;
        }
    }
    (samples, hits)
}

fn main() {
    // `cargo bench -- --test` is the compile-and-run smoke check. Cargo also
    // passes a bare `--bench`, which is not a request for a short run.
    let smoke = std::env::args().any(|argument| argument == "--test");
    let iterations = if smoke { 1 } else { 10 };
    let rounds = if smoke { 1 } else { 20 };

    let options = IndexOptions::new().with_locale(Locale::parse("zh_TW.UTF-8"));
    let ranking = RankingOptions::default();

    println!(
        "launcher-core benchmarks: {RECORD_COUNT} synthetic records, median of {iterations} iteration(s), \
         {rounds} script round(s)"
    );

    let (prepare, records) = measure(iterations, || synthetic_records(RECORD_COUNT));
    report(
        "record preparation (catalog cost)",
        prepare,
        &format!("{RECORD_COUNT} records, not this crate's work"),
    );

    // Cold index construction: the first build in a process that has never
    // built one. Reported as a single sample, because a second is warm.
    let started = Instant::now();
    let cold = SearchIndex::build(&records, &options);
    let cold_time = started.elapsed();
    report(
        "cold index construction",
        cold_time,
        &format!("{} indexed applications", cold.len()),
    );
    drop(cold);

    let (warm_build, index) = measure(iterations, || SearchIndex::build(&records, &options));
    report(
        "warm index load",
        warm_build,
        &format!("{} indexed applications", index.len()),
    );

    // Returning to the application library, which is what clearing the query
    // costs. The browse model is built once with the index, so this is a
    // borrow and a walk rather than a rebuild.
    let (browse, browse_count) = measure(iterations, || {
        let model = index.browse();
        model.applications().len() + model.sections().len()
    });
    report(
        "browse model (empty query)",
        browse,
        &format!("{browse_count} entries and sections"),
    );

    // Per-keystroke latency: the number Issue #2 sets a target for.
    let mut samples = Vec::new();
    let mut hits = 0usize;
    for _ in 0..rounds {
        let (round, round_hits) = run_script(&index, &ranking, &NoUsage);
        samples.extend(round);
        hits += round_hits;
    }
    samples.sort();
    let p50 = percentile(&samples, 50.0);
    let p95 = percentile(&samples, 95.0);
    let p99 = percentile(&samples, 99.0);
    let worst = *samples.last().expect("at least one keystroke");
    report(
        "query latency p50",
        p50,
        &format!("{} keystrokes", samples.len()),
    );
    report("query latency p95", p95, "target: below 50 ms");
    report("query latency p99", p99, "");
    report(
        "query latency worst",
        worst,
        &format!("{} results returned in total", hits),
    );

    // Ranking throughput over the same script, which is the same work seen as
    // a rate rather than a latency.
    let total: Duration = samples.iter().sum();
    println!(
        "{:<38} {:>10.0} queries/s   {:.1} M record-comparisons/s",
        "ranking throughput",
        samples.len() as f64 / total.as_secs_f64(),
        (samples.len() * index.len()) as f64 / total.as_secs_f64() / 1_000_000.0
    );

    // The same script with usage weighting switched on, so the cost of the
    // optional path is stated rather than assumed to be free.
    let mut usage = InMemoryUsage::new();
    for (position, record) in records.iter().enumerate().take(200) {
        for at in 0..(position as u64 % 7 + 1) {
            usage.record_launch(&record.desktop_id, LaunchEvent { at });
        }
    }
    let weighted_options = RankingOptions {
        usage_weighting: true,
        ..RankingOptions::default()
    };
    let mut weighted: Vec<Duration> = Vec::new();
    for _ in 0..rounds {
        weighted.extend(run_script(&index, &weighted_options, &usage).0);
    }
    weighted.sort();
    report(
        "query latency p95 (usage weighting)",
        percentile(&weighted, 95.0),
        "off by default",
    );

    // What installing or removing an application costs: the index is rebuilt
    // from the refreshed catalog, and the browse model with it.
    let mut with_extra = records.clone();
    with_extra.push({
        let text = synthetic_entry(RECORD_COUNT);
        let file = DesktopFile::parse(&text).expect("added entry parses");
        ApplicationRecord::from_desktop_file(
            DesktopId::new("synthetic-added.desktop").expect("valid desktop id"),
            PathBuf::from("/usr/share/applications/synthetic-added.desktop"),
            EntryScope::System,
            &file,
            &SyntheticProbe,
        )
        .expect("added record")
    });
    let (after_install, installed) =
        measure(iterations, || SearchIndex::build(&with_extra, &options));
    report(
        "list update after install",
        after_install,
        &format!("{} indexed applications", installed.len()),
    );

    let fewer = &records[1..];
    let (after_removal, removed) = measure(iterations, || SearchIndex::build(fewer, &options));
    report(
        "list update after removal",
        after_removal,
        &format!("{} indexed applications", removed.len()),
    );

    assert_eq!(
        index.len(),
        RECORD_COUNT,
        "the synthetic index lost records"
    );
    assert!(
        !index.search("text editor", &ranking, &NoUsage).is_empty(),
        "the benchmark script matched nothing, so it measured nothing"
    );
    println!(
        "\nverdict: warm search update p95 {:.3} ms against a 50 ms target — {}",
        p95.as_secs_f64() * 1000.0,
        if p95 < Duration::from_millis(50) {
            "met"
        } else {
            "MISSED"
        }
    );
}
