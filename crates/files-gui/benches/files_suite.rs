//! The Better Files benchmark suite: one command, one summary table.
//!
//! Issue #6 names a list of scenarios and a list of measurements, and the point
//! of gathering them here is that a performance claim has one place to be
//! checked against. `cargo bench -p files-gui --bench files_suite` runs every
//! scenario and prints a table; `-- --quick` runs a smaller version of each for
//! a smoke run.
//!
//! **What this measures and what it does not.** Every number here is this
//! machine, this filesystem, warm page cache, debug-assertions-off release
//! build. The copy figures in particular are page-cache figures: a large copy
//! that reports gigabytes per second is measuring memory bandwidth, not a disk.
//! `docs/files-benchmarks.md` records the methodology and the numbers.
//!
//! **What is deliberately absent.** There is no comparison against Nautilus,
//! COSMIC Files, or Windows File Explorer. Producing one needs a defined
//! dataset, a defined machine, and a way to measure another program's
//! time-to-first-content that is not a stopwatch — none of which exists yet.
//! Issue #6 requires any public claim to state its workflow, dataset, hardware,
//! and metric, so the comparison is recorded as a follow-up rather than
//! invented here.

use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use files_core::{Location, Pane, ViewPreferences};
use files_gui::apps::CatalogHandle;
use files_gui::devices::{CollectionMode, DeviceInventory, UnsafeRemoval};
use files_gui::reader::FilesReader;
use files_gui::search::SearchState;
use files_operations::{EngineConfig, JobEngine, JobSpec, JobState, Operation};
use files_platform::ReaderConfig;
use files_preview::{CancelToken, PreviewEngine, PreviewRequest};
use storage_core::{
    DeviceEvent, DeviceHandle, DeviceIdentity, DeviceMachine, DeviceRegistry, IdentityEvidence,
    PreferenceSet, RemovalPolicy, Timestamp, Transport,
};

/// One row of the summary table.
struct Row {
    scenario: String,
    metric: &'static str,
    value: String,
    detail: String,
}

fn ms(duration: Duration) -> String {
    format!("{:.3} ms", duration.as_secs_f64() * 1000.0)
}

fn median(mut samples: Vec<Duration>) -> Duration {
    samples.sort();
    samples[samples.len() / 2]
}

/// The value at the given percentile, nearest-rank.
fn percentile(samples: &mut [Duration], percent: f64) -> Duration {
    samples.sort();
    let rank = ((percent / 100.0) * samples.len() as f64).ceil() as usize;
    samples[rank.saturating_sub(1).min(samples.len() - 1)]
}

fn build_flat(root: &Path, count: usize) {
    fs::create_dir_all(root).expect("benchmark directory");
    for index in 0..count {
        if index % 10 == 0 {
            fs::create_dir_all(root.join(format!("folder{index:06}"))).expect("folder");
        } else {
            fs::write(
                root.join(format!("entry{index:06}.txt")),
                b"benchmark entry contents",
            )
            .expect("file");
        }
    }
}

fn drain(pane: &mut Pane) {
    let deadline = Instant::now() + Duration::from_secs(120);
    while pane.is_listing() && Instant::now() < deadline {
        pane.pump();
        std::thread::yield_now();
    }
    pane.pump();
}

fn reader() -> Arc<FilesReader> {
    Arc::new(FilesReader::new(
        ReaderConfig::new(),
        None,
        CatalogHandle::empty(Default::default()),
    ))
}

fn main() {
    let quick = std::env::args().any(|argument| argument == "--quick" || argument == "--test");
    let entry_count = if quick { 2_000 } else { 100_000 };
    let small_files = if quick { 2_000 } else { 100_000 };
    let large_bytes: u64 = if quick {
        32 * 1024 * 1024
    } else {
        512 * 1024 * 1024
    };
    let iterations = if quick { 1 } else { 5 };

    let root = tempfile::tempdir().expect("benchmark root");
    let mut rows: Vec<Row> = Vec::new();

    println!("Better Files benchmark suite");
    println!(
        "{} entries · {small_files} small files · {} large copy · {iterations} iterations",
        entry_count,
        format_bytes(large_bytes)
    );
    println!();

    rows.extend(large_directory(root.path(), entry_count, iterations));
    rows.extend(search_latency(root.path(), entry_count));
    rows.extend(preview_generation(root.path(), iterations));
    rows.extend(copy_scenarios(root.path(), small_files, large_bytes));
    rows.extend(device_lifecycle(root.path()));

    println!();
    println!(
        "{:<44} {:<34} {:>14}   notes",
        "scenario", "metric", "value"
    );
    println!("{}", "-".repeat(120));
    for row in &rows {
        println!(
            "{:<44} {:<34} {:>14}   {}",
            row.scenario, row.metric, row.value, row.detail
        );
    }
    println!();
    println!(
        "Comparison against Nautilus, COSMIC Files, and Windows File Explorer: \
         methodology needed, not measured. See docs/files-benchmarks.md."
    );
}

fn format_bytes(value: u64) -> String {
    if value >= 1024 * 1024 * 1024 {
        format!("{:.1} GB", value as f64 / (1024.0 * 1024.0 * 1024.0))
    } else {
        format!("{} MB", value / (1024 * 1024))
    }
}

// --- Scenario: a directory with 100,000 small entries --------------------

fn large_directory(root: &Path, count: usize, iterations: usize) -> Vec<Row> {
    let flat = root.join("flat");
    let started = Instant::now();
    build_flat(&flat, count);
    println!(
        "fixture: {count} entries written in {:.1} s",
        started.elapsed().as_secs_f64()
    );

    let location = Location::local(&flat).expect("location");
    let reader = reader();
    let preferences = ViewPreferences::default();

    let mut first = Vec::new();
    let mut full = Vec::new();
    for _ in 0..iterations {
        let clock = Instant::now();
        let mut pane = Pane::open(location.clone(), preferences, reader.as_ref());
        // Time to first visible entries: the moment the model has anything the
        // window could draw.
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            pane.pump();
            if pane.model().visible_len() > 0 || Instant::now() > deadline {
                break;
            }
            std::thread::yield_now();
        }
        first.push(clock.elapsed());
        drain(&mut pane);
        full.push(clock.elapsed());
    }

    // Navigation interactivity: how long a keystroke takes while the model is
    // fully loaded. This is the number that decides whether a big directory is
    // usable rather than merely finished.
    let mut pane = Pane::open(location.clone(), preferences, reader.as_ref());
    drain(&mut pane);
    let loaded = pane.model().visible_len();
    let mut keystroke = Vec::new();
    for _ in 0..(iterations * 20) {
        let clock = Instant::now();
        std::hint::black_box(pane.model().visible(loaded / 2));
        keystroke.push(clock.elapsed());
    }

    // Multi-tab navigation: four panes over the same directory, all pumping.
    let clock = Instant::now();
    let mut panes: Vec<Pane> = (0..4)
        .map(|_| Pane::open(location.clone(), preferences, reader.as_ref()))
        .collect();
    let deadline = Instant::now() + Duration::from_secs(120);
    while panes.iter().any(Pane::is_listing) && Instant::now() < deadline {
        for pane in &mut panes {
            pane.pump();
        }
        std::thread::yield_now();
    }
    let multi_tab = clock.elapsed();

    // Startup with an unavailable location, which must fail fast rather than
    // hanging the window.
    let clock = Instant::now();
    let mut missing = Pane::open(
        Location::local(root.join("not-there")).expect("location"),
        preferences,
        reader.as_ref(),
    );
    drain(&mut missing);
    let unavailable = clock.elapsed();

    vec![
        Row {
            scenario: format!("{count}-entry directory"),
            metric: "time to first visible entries",
            value: ms(median(first)),
            detail: format!("{loaded} entries visible when complete"),
        },
        Row {
            scenario: format!("{count}-entry directory"),
            metric: "time to complete model",
            value: ms(median(full)),
            detail: "listing off the render thread throughout".to_string(),
        },
        Row {
            scenario: format!("{count}-entry directory"),
            metric: "one row lookup while loaded",
            value: ms(median(keystroke)),
            detail: "what a keystroke costs at full size".to_string(),
        },
        Row {
            scenario: "multi-tab navigation".to_string(),
            metric: "four tabs to complete",
            value: ms(multi_tab),
            detail: "same directory, four independent panes".to_string(),
        },
        Row {
            scenario: "startup, unavailable location".to_string(),
            metric: "time to reported failure",
            value: ms(unavailable),
            detail: "fails fast rather than hanging".to_string(),
        },
    ]
}

// --- Scenario: search in the current directory ---------------------------

fn search_latency(root: &Path, count: usize) -> Vec<Row> {
    let flat = root.join("flat");
    let location = Location::local(&flat).expect("location");
    let reader = reader();
    let mut pane = Pane::open(
        location.clone(),
        ViewPreferences::default(),
        reader.as_ref(),
    );
    drain(&mut pane);

    // Per-keystroke latency: what typing one more character costs. That is the
    // number that decides whether typing blocks, and it is the p95 the manifest
    // declares a budget for.
    let mut keystroke = Vec::new();
    let mut to_first_result = Vec::new();
    let mut to_complete = Vec::new();
    for query in ["e", "en", "ent", "entr", "entry", "entry0", "entry00"] {
        let mut state = SearchState::default();
        let clock = Instant::now();
        state.set_text(query, &location);
        keystroke.push(clock.elapsed());

        let clock = Instant::now();
        state.pump(pane.model());
        to_first_result.push(clock.elapsed());
        while !state.is_complete() {
            state.pump(pane.model());
        }
        to_complete.push(clock.elapsed());
    }

    let mut all = keystroke.clone();
    let found = {
        let mut state = SearchState::default();
        state.set_text("entry0", &location);
        while !state.is_complete() {
            state.pump(pane.model());
        }
        state.hits().len()
    };

    vec![
        Row {
            scenario: format!("search in {count} entries"),
            metric: "keystroke p50",
            value: ms(percentile(&mut all.clone(), 50.0)),
            detail: format!("restart only; {count} candidates"),
        },
        Row {
            scenario: format!("search in {count} entries"),
            metric: "keystroke p95",
            value: ms(percentile(&mut all, 95.0)),
            detail: "typing never waits for the scan".to_string(),
        },
        Row {
            scenario: format!("search in {count} entries"),
            metric: "first slice of results",
            value: ms(median(to_first_result)),
            detail: format!("{} entries per slice", files_gui::search::SLICE),
        },
        Row {
            scenario: format!("search in {count} entries"),
            metric: "all results",
            value: ms(median(to_complete)),
            detail: format!("{found} matches for \"entry0\""),
        },
    ]
}

// --- Scenario: preview generation ----------------------------------------

fn preview_generation(root: &Path, iterations: usize) -> Vec<Row> {
    let dir = root.join("preview");
    fs::create_dir_all(&dir).expect("preview directory");

    let png = dir.join("image.png");
    let mut buffer = image::RgbaImage::new(1920, 1080);
    for (x, y, pixel) in buffer.enumerate_pixels_mut() {
        *pixel = image::Rgba([(x % 256) as u8, (y % 256) as u8, 96, 255]);
    }
    buffer.save(&png).expect("write png");

    let text = dir.join("source.rs");
    let mut body = String::new();
    for index in 0..100_000 {
        body.push_str(&format!("line {index:06}: some source code goes here\n"));
    }
    fs::write(&text, body).expect("write text");

    let engine = PreviewEngine::default();
    let cancel = CancelToken::new();
    let mut rows = Vec::new();

    for (label, request) in [
        ("image (1920x1080 PNG)", PreviewRequest::file(&png)),
        ("text (128 KiB of 5 MB)", PreviewRequest::file(&text)),
        (
            "folder summary",
            PreviewRequest::directory(root.join("flat")),
        ),
    ] {
        let mut samples = Vec::new();
        for _ in 0..iterations.max(3) {
            let clock = Instant::now();
            std::hint::black_box(engine.preview(&request, &cancel).expect("preview"));
            samples.push(clock.elapsed());
        }
        let p95 = percentile(&mut samples.clone(), 95.0);
        rows.push(Row {
            scenario: "preview generation".to_string(),
            metric: label,
            value: ms(p95),
            detail: "p95, off the render thread in the window".to_string(),
        });
    }
    rows
}

// --- Scenario: copies ----------------------------------------------------

fn copy_scenarios(root: &Path, small_files: usize, large_bytes: u64) -> Vec<Row> {
    let engine = JobEngine::new(EngineConfig {
        store: None,
        ..EngineConfig::default()
    });

    // One large sequential copy.
    let source = root.join("large.bin");
    write_large(&source, large_bytes);
    let destination = root.join("large-destination");
    fs::create_dir_all(&destination).expect("destination");
    let clock = Instant::now();
    let large = run_job(
        &engine,
        JobSpec::new(Operation::Copy {
            sources: vec![local(&source)],
            destination: local(&destination),
        }),
    );
    let large_elapsed = clock.elapsed();
    let throughput = large_bytes as f64 / large_elapsed.as_secs_f64() / (1024.0 * 1024.0);

    // Many small files.
    let many_source = root.join("many");
    fs::create_dir_all(&many_source).expect("many");
    for index in 0..small_files {
        fs::write(many_source.join(format!("f{index:06}")), b"x").expect("small file");
    }
    let many_destination = root.join("many-destination");
    fs::create_dir_all(&many_destination).expect("destination");
    let clock = Instant::now();
    let many = run_job(
        &engine,
        JobSpec::new(Operation::Copy {
            sources: vec![local(&many_source)],
            destination: local(&many_destination),
        }),
    );
    let many_elapsed = clock.elapsed();
    let per_second = small_files as f64 / many_elapsed.as_secs_f64();

    // Same-filesystem move, which is a rename and should cost nothing.
    let move_source = root.join("large-destination/large.bin");
    let move_destination = root.join("moved");
    fs::create_dir_all(&move_destination).expect("destination");
    let clock = Instant::now();
    let moved = run_job(
        &engine,
        JobSpec::new(Operation::Move {
            sources: vec![local(&move_source)],
            destination: local(&move_destination),
        }),
    );
    let move_elapsed = clock.elapsed();

    vec![
        Row {
            scenario: "large sequential copy".to_string(),
            metric: "throughput",
            value: format!("{throughput:.0} MB/s"),
            detail: format!(
                "{} in {} · {large:?} · page-cache figure, not a disk figure",
                format_bytes(large_bytes),
                ms(large_elapsed)
            ),
        },
        Row {
            scenario: "many small files".to_string(),
            metric: "files per second",
            value: format!("{per_second:.0}/s"),
            detail: format!("{small_files} files in {} · {many:?}", ms(many_elapsed)),
        },
        Row {
            scenario: "same-filesystem move".to_string(),
            metric: "time",
            value: ms(move_elapsed),
            detail: format!("rename, no bytes copied · {moved:?}"),
        },
    ]
}

fn write_large(path: &Path, bytes: u64) {
    use std::io::Write;
    let mut file = fs::File::create(path).expect("large file");
    let chunk = vec![0xABu8; 1024 * 1024];
    let mut written = 0u64;
    while written < bytes {
        let take = chunk.len().min((bytes - written) as usize);
        file.write_all(&chunk[..take]).expect("write");
        written += take as u64;
    }
    file.flush().expect("flush");
}

fn local(path: &Path) -> files_core::LocalPath {
    files_core::LocalPath::new(path.to_path_buf()).expect("absolute path")
}

fn run_job(engine: &JobEngine, spec: JobSpec) -> JobState {
    let handle = engine.submit(spec).expect("the spec validates");
    let id = handle.id();
    let deadline = Instant::now() + Duration::from_secs(600);
    loop {
        let Some(snapshot) = engine.snapshot(id) else {
            return JobState::Failed;
        };
        if snapshot.state.is_terminal() || Instant::now() > deadline {
            return snapshot.state;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}

// --- Scenario: device connect, open, disconnect --------------------------

fn device_lifecycle(_root: &Path) -> Vec<Row> {
    // Driven through the state machine rather than through hardware: unplugging
    // a real disk mid-write is not a benchmark that can run repeatably, and the
    // cost being measured is the model's, not the bus's.
    let cycles = 2_000;
    let evidence = IdentityEvidence {
        filesystem_uuid: Some("A1B2-C3D4".to_string()),
        transport: Transport::Usb,
        device_path: "/dev/sdb1".to_string(),
        ..IdentityEvidence::default()
    };
    let preferences = PreferenceSet::new();

    let clock = Instant::now();
    let mut registry = DeviceRegistry::new(Default::default());
    for index in 0..cycles {
        let handle = DeviceHandle::new(format!("/dev/obj{index}"));
        let identity = DeviceIdentity::from_evidence(evidence.clone());
        let at = Timestamp::from_millis(index as u64 * 10);
        registry.connect(handle.clone(), identity, &preferences, at);
        registry.apply(
            &handle,
            DeviceEvent::Mounted {
                mount_point: "/media/x".to_string(),
            },
            at,
        );
        registry.apply(
            &handle,
            DeviceEvent::OperationStarted {
                operation: "job-1".to_string(),
            },
            at,
        );
        registry.disconnect(&handle, at);
    }
    let elapsed = clock.elapsed();

    // What the window pays to turn one report into a row and then drop it.
    let mut inventory = DeviceInventory::default();
    let clock = Instant::now();
    for _ in 0..cycles {
        inventory.apply_inventory(Vec::new());
    }
    let inventory_elapsed = clock.elapsed();

    // And a single machine, so the per-event cost is separable.
    let identity = DeviceIdentity::from_evidence(evidence);
    let mut machine = DeviceMachine::connect(
        identity,
        RemovalPolicy::DirectRemoval,
        Timestamp::from_millis(0),
    );
    let clock = Instant::now();
    for index in 0..cycles {
        machine.apply(
            DeviceEvent::Mounted {
                mount_point: "/media/x".to_string(),
            },
            Timestamp::from_millis(index as u64),
        );
    }
    let per_event = clock.elapsed() / cycles as u32;

    let _ = UnsafeRemoval {
        previous_state: String::new(),
        unfinished_operations: Vec::new(),
        recommend_filesystem_check: false,
    };
    let _ = CollectionMode::Service;

    vec![
        Row {
            scenario: "device connect/open/disconnect".to_string(),
            metric: "per cycle",
            value: ms(elapsed / cycles as u32),
            detail: format!("{cycles} cycles through the state machine"),
        },
        Row {
            scenario: "device state event".to_string(),
            metric: "per event",
            value: ms(per_event),
            detail: "one machine, one mount event".to_string(),
        },
        Row {
            scenario: "sidebar inventory rebuild".to_string(),
            metric: "per rebuild",
            value: ms(inventory_elapsed / cycles as u32),
            detail: "what a frame pays for the device rows".to_string(),
        },
    ]
}
