//! Catalog benchmarks over a 5,000-record synthetic tree.
//!
//! Two components read this catalog on startup, so the numbers that matter are
//! how long a full discovery takes and how much a live catalog costs to keep
//! in memory. The dataset is synthetic and written fresh each run, so results
//! do not depend on what happens to be installed on the machine.
//!
//! There is no benchmark harness dependency here on purpose. The measurements
//! are wall-clock timings and a counting allocator, both of which are exact
//! about what they measure; a statistics framework would add a large
//! dependency without changing any decision this crate makes.
//!
//! Run with `cargo bench -p app-catalog-core`, or
//! `cargo bench -p app-catalog-core -- --test` for a single-iteration smoke
//! run.

use std::alloc::{GlobalAlloc, Layout, System};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use app_catalog_core::{Catalog, DesktopEnvironments, ExecutableProbe, MimeType};
use app_catalog_platform::{ApplicationDirectories, ApplicationDirectory, HostProbe, discover};

/// Resolves every synthetic program without touching the filesystem, so the
/// discovery numbers measure parsing and normalization rather than the host's
/// `PATH`. The cost the real probe adds is measured separately.
struct SyntheticProbe;

impl ExecutableProbe for SyntheticProbe {
    fn resolve(&self, program: &str) -> Option<PathBuf> {
        program
            .starts_with("synthetic-app-")
            .then(|| PathBuf::from("/usr/bin").join(program))
    }
}

/// Records live bytes so the catalog's memory footprint is measured rather
/// than estimated from `size_of`.
struct CountingAllocator;

static LIVE_BYTES: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() {
            LIVE_BYTES.fetch_add(layout.size(), Ordering::Relaxed);
        }
        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        LIVE_BYTES.fetch_sub(layout.size(), Ordering::Relaxed);
        unsafe { System.dealloc(pointer, layout) };
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let new_pointer = unsafe { System.realloc(pointer, layout, new_size) };
        if !new_pointer.is_null() {
            LIVE_BYTES.fetch_add(new_size, Ordering::Relaxed);
            LIVE_BYTES.fetch_sub(layout.size(), Ordering::Relaxed);
        }
        new_pointer
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

/// The record count the ticket sets as the working target.
const RECORD_COUNT: usize = 5_000;
/// How many of those records live in the user directory rather than a
/// system one, so precedence and shadowing are part of the measurement.
const USER_RECORDS: usize = 250;

fn live_bytes() -> usize {
    LIVE_BYTES.load(Ordering::Relaxed)
}

fn synthetic_entry(index: usize) -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=Synthetic Application {index}\n\
         Name[zh_TW]=合成應用程式 {index}\n\
         Name[de]=Synthetische Anwendung {index}\n\
         GenericName=Synthetic Tool\n\
         Comment=A generated entry used only for benchmarking\n\
         Icon=application-x-executable\n\
         Exec=synthetic-app-{index} --profile default %F\n\
         TryExec=synthetic-app-{index}\n\
         Terminal=false\n\
         StartupNotify=true\n\
         Categories=Utility;Development;\n\
         Keywords=synthetic;benchmark;generated;\n\
         MimeType=text/plain;text/x-synthetic-{group};application/x-synthetic;\n\
         Actions=NewWindow;NewDocument;\n\
         \n\
         [Desktop Action NewWindow]\n\
         Name=New Window\n\
         Exec=synthetic-app-{index} --new-window\n\
         \n\
         [Desktop Action NewDocument]\n\
         Name=New Document\n\
         Exec=synthetic-app-{index} --new-document\n",
        index = index,
        group = index % 25
    )
}

/// Writes the synthetic tree and returns the directory list to discover.
fn build_tree(root: &Path) -> ApplicationDirectories {
    let user = root.join("user/applications");
    let system = root.join("system/applications");
    let nested = system.join("vendor");
    for directory in [&user, &system, &nested] {
        fs::create_dir_all(directory).expect("synthetic directory");
    }
    for index in 0..RECORD_COUNT {
        let directory = if index < USER_RECORDS {
            &user
        } else if index % 10 == 0 {
            &nested
        } else {
            &system
        };
        fs::write(
            directory.join(format!("synthetic-{index}.desktop")),
            synthetic_entry(index),
        )
        .expect("synthetic entry");
    }
    // A slice of the user directory overrides system entries, so precedence
    // resolution is exercised rather than skipped.
    for index in 0..USER_RECORDS {
        let overridden = RECORD_COUNT + index;
        fs::write(
            system.join(format!("synthetic-{overridden}.desktop")),
            synthetic_entry(overridden),
        )
        .expect("synthetic entry");
        fs::write(
            user.join(format!("synthetic-{overridden}.desktop")),
            synthetic_entry(overridden),
        )
        .expect("synthetic entry");
    }
    ApplicationDirectories::new(vec![
        ApplicationDirectory {
            path: user,
            rank: 0,
            scope: app_catalog_core::EntryScope::User,
        },
        ApplicationDirectory {
            path: system,
            rank: 1,
            scope: app_catalog_core::EntryScope::System,
        },
    ])
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
        "{label:<34} {:>10.3} ms   {detail}",
        duration.as_secs_f64() * 1000.0
    );
}

fn main() {
    // `cargo bench -- --test` is the compile-and-run smoke check. Cargo also
    // passes a bare `--bench`, which is not a request for a short run.
    let smoke = std::env::args().any(|argument| argument == "--test");
    let iterations = if smoke { 1 } else { 10 };

    let root = tempfile::tempdir().expect("benchmark root");
    let directories = build_tree(root.path());
    let environments = DesktopEnvironments::parse("ubuntu:GNOME");
    let mime = MimeType::parse("text/plain").expect("mime type");

    println!(
        "app-catalog benchmarks: {RECORD_COUNT} synthetic records, {USER_RECORDS} user overrides, median of {iterations} iteration(s)"
    );

    // Cold discovery: the first read of a tree this process has never walked.
    // Reported as one sample, because a second sample is by definition warm.
    let started = Instant::now();
    let cold_catalog = discover(&directories, &SyntheticProbe);
    let cold = started.elapsed();
    report(
        "cold discovery",
        cold,
        &format!("{} records", cold_catalog.len()),
    );
    drop(cold_catalog);

    // Warm load: the same full discovery with the directory entries and file
    // contents already in the page cache. This is what a component restart
    // costs.
    let (warm, warm_catalog) = measure(iterations, || discover(&directories, &SyntheticProbe));
    report(
        "warm load",
        warm,
        &format!("{} records", warm_catalog.len()),
    );

    // The same warm load with the real host probe, which stats every `PATH`
    // entry for every program name. This is the price of reporting a resolved
    // executable path instead of guessing one.
    let host_probe = HostProbe::from_env();
    let (warm_host, warm_host_catalog) =
        measure(iterations, || discover(&directories, &host_probe));
    report(
        "warm load (host executable probe)",
        warm_host,
        &format!("{} records", warm_host_catalog.len()),
    );
    drop(warm_host_catalog);

    // Memory footprint of one live catalog.
    drop(warm_catalog);
    let before = live_bytes();
    let resident: Catalog = discover(&directories, &SyntheticProbe);
    let after = live_bytes();
    let bytes = after.saturating_sub(before);
    println!(
        "{:<34} {:>10.2} MB   {} bytes per record",
        "memory footprint",
        bytes as f64 / (1024.0 * 1024.0),
        if resident.is_empty() {
            0
        } else {
            bytes / resident.len()
        }
    );

    // Filtering a live catalog, which every consumer does on every keystroke
    // or selection change.
    let (visible_time, visible_count) =
        measure(iterations, || resident.visible(&environments).count());
    report(
        "visible filter",
        visible_time,
        &format!("{visible_count} records"),
    );
    let (mime_time, mime_count) = measure(iterations, || {
        resident.supporting_mime_type(&mime, &environments).count()
    });
    report("mime filter", mime_time, &format!("{mime_count} records"));
    drop(resident);

    // Refresh after an entry appears, which is what a package installation
    // looks like to the catalog.
    let added = directories.directories()[1]
        .path
        .join("synthetic-added.desktop");
    let (refresh_add, added_catalog) = measure(iterations, || {
        fs::write(&added, synthetic_entry(999_999)).expect("added entry");
        discover(&directories, &SyntheticProbe)
    });
    report(
        "refresh after add",
        refresh_add,
        &format!("{} records", added_catalog.len()),
    );
    drop(added_catalog);

    // Refresh after an entry disappears.
    let (refresh_remove, removed_catalog) = measure(iterations, || {
        let _ = fs::remove_file(&added);
        let catalog = discover(&directories, &SyntheticProbe);
        fs::write(&added, synthetic_entry(999_999)).expect("added entry");
        catalog
    });
    report(
        "refresh after remove",
        refresh_remove,
        &format!("{} records", removed_catalog.len()),
    );

    assert!(
        removed_catalog.len() >= RECORD_COUNT,
        "the synthetic catalog lost records"
    );
    let _ = PathBuf::from(root.path());
}
