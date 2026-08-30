//! Listing benchmarks over a 100,000-entry synthetic directory.
//!
//! The numbers that decide whether this architecture works are how long the
//! user waits before seeing anything, how long the whole directory takes, what
//! keeping it in memory costs, and how quickly an abandoned listing actually
//! stops. All four are measured here against the real reader in
//! `files-platform`, not a model of it.
//!
//! There is no benchmark harness dependency, matching `app-catalog-core`:
//! these are wall-clock timings and a counting allocator, both exact about
//! what they measure, and a statistics framework would add a large dependency
//! without changing any decision this crate makes.
//!
//! Run with `cargo bench -p files-core`, or
//! `cargo bench -p files-core -- --test` for a single-iteration smoke run on a
//! smaller tree.

use std::alloc::{GlobalAlloc, Layout, System};
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use files_core::listing::{
    DirectoryReader, ListingBatch, ListingEvent, ListingRequest, ListingSession,
};
use files_core::{
    DirectoryModel, HiddenPreference, ListingStatus, Location, SortDirection, SortKey, SortOrder,
};
use files_platform::{LocalDirectoryReader, ReaderConfig, detector_from_env};

/// Records live bytes so the model's footprint is measured rather than
/// estimated from `size_of`.
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

/// The directory size the ticket sets as the target.
const ENTRY_COUNT: usize = 100_000;
/// The batch size a view would use.
const BATCH_SIZE: usize = 256;
/// Files in the mixed-media directory, which exercises type detection.
const MIXED_COUNT: usize = 20_000;
/// Directories per level and depth of the deep tree.
const DEEP_FANOUT: usize = 4;
const DEEP_DEPTH: usize = 7;

fn live_bytes() -> usize {
    LIVE_BYTES.load(Ordering::Relaxed)
}

fn report(label: &str, duration: Duration, detail: &str) {
    println!(
        "{label:<38} {:>10.3} ms   {detail}",
        duration.as_secs_f64() * 1000.0
    );
}

fn report_bytes(label: &str, bytes: usize, detail: &str) {
    println!(
        "{label:<38} {:>10.2} MB   {detail}",
        bytes as f64 / (1024.0 * 1024.0)
    );
}

fn median(mut samples: Vec<Duration>) -> Duration {
    samples.sort();
    samples[samples.len() / 2]
}

/// Writes a flat directory of plain files.
fn build_flat(root: &Path, count: usize) {
    fs::create_dir_all(root).expect("benchmark directory");
    for index in 0..count {
        fs::write(root.join(format!("entry{index:06}.txt")), b"x").expect("synthetic entry");
    }
}

/// Writes a directory of mixed media, so type detection and the type sort are
/// measured against something varied rather than one extension repeated.
fn build_mixed(root: &Path, count: usize) {
    const EXTENSIONS: [&str; 10] = [
        "txt",
        "jpg",
        "png",
        "mp3",
        "mp4",
        "pdf",
        "zip",
        "rs",
        "json",
        "unknownext",
    ];
    fs::create_dir_all(root).expect("benchmark directory");
    for index in 0..count {
        let extension = EXTENSIONS[index % EXTENSIONS.len()];
        if index % 20 == 0 {
            fs::create_dir_all(root.join(format!("folder{index:06}"))).expect("folder");
        } else {
            fs::write(
                root.join(format!("media{index:06}.{extension}")),
                vec![0u8; index % 512],
            )
            .expect("synthetic entry");
        }
    }
}

/// Writes a tree, returning the deepest directory. Listing one level of a deep
/// tree must cost the same as listing one level of a shallow one; this is the
/// measurement that says so.
fn build_deep(root: &Path, fanout: usize, depth: usize) -> std::path::PathBuf {
    let mut deepest = root.to_path_buf();
    let mut current = root.to_path_buf();
    for level in 0..depth {
        fs::create_dir_all(&current).expect("level");
        for branch in 0..fanout {
            fs::create_dir_all(current.join(format!("branch{branch}"))).expect("branch");
            fs::write(current.join(format!("file{branch}.txt")), b"x").expect("file");
        }
        current = current.join(format!("branch{}", level % fanout));
        deepest.clone_from(&current);
    }
    // The deepest level gets the same contents as every other, so listing it
    // is a like-for-like comparison against listing the shallow ones.
    fs::create_dir_all(&deepest).expect("deepest");
    for branch in 0..fanout {
        fs::create_dir_all(deepest.join(format!("branch{branch}"))).expect("branch");
        fs::write(deepest.join(format!("file{branch}.txt")), b"x").expect("file");
    }
    deepest
}

/// One full listing, reporting when the first batch landed, when it finished,
/// and the batches themselves for the replay measurements.
struct ListingRun {
    first_batch: Duration,
    complete: Duration,
    batches: Vec<ListingBatch>,
    total: usize,
}

fn run_listing(path: &Path, config: &ReaderConfig, batch_size: usize) -> ListingRun {
    let reader = LocalDirectoryReader::with_config(config.clone());
    let request =
        ListingRequest::new(Location::local(path).expect("location")).with_batch_size(batch_size);
    let (mut session, sink) = ListingSession::start(&request);

    let started = Instant::now();
    reader.start(request, sink);

    let mut first_batch = None;
    let mut complete = None;
    let mut batches = Vec::new();
    let mut total = 0usize;
    while complete.is_none() {
        for event in session.drain() {
            match event {
                ListingEvent::Batch(batch) => {
                    if first_batch.is_none() {
                        first_batch = Some(started.elapsed());
                    }
                    total += batch.entries.len();
                    batches.push(batch);
                }
                ListingEvent::Complete(_) => complete = Some(started.elapsed()),
                other => panic!("listing did not complete: {other:?}"),
            }
        }
        std::hint::spin_loop();
    }
    ListingRun {
        first_batch: first_batch.expect("no batch arrived"),
        complete: complete.expect("no completion"),
        batches,
        total,
    }
}

/// Feeds captured batches into a model, which is the incremental sort cost
/// with the I/O removed.
fn replay(location: &Location, batches: &[ListingBatch], order: SortOrder) -> (Duration, usize) {
    let mut model = DirectoryModel::new(location.clone(), order, HiddenPreference::default());
    if let Some(first) = batches.first() {
        model.restart(first.listing);
    }
    let before = live_bytes();
    let started = Instant::now();
    for batch in batches {
        model.apply(ListingEvent::Batch(batch.clone()));
    }
    model.commit();
    let elapsed = started.elapsed();
    let held = live_bytes().saturating_sub(before);
    assert_eq!(model.status(), &ListingStatus::Loading);
    std::hint::black_box(&model);
    (elapsed, held)
}

/// Starts a listing on a thread this function can join, cancels it after the
/// first batch, and measures how long the reader took to actually stop.
fn cancellation_latency(path: &Path, batch_size: usize) -> (Duration, usize) {
    let request =
        ListingRequest::new(Location::local(path).expect("location")).with_batch_size(batch_size);
    let (mut session, sink) = ListingSession::start(&request);
    let token = sink.token().clone();
    let config = ReaderConfig::new();

    let reader = std::thread::spawn(move || {
        files_platform::list_directory_blocking(&request, &config, sink);
    });

    // Wait for the listing to be genuinely under way before abandoning it, so
    // the number measures a running reader stopping rather than one that had
    // not started.
    let mut delivered = 0usize;
    while delivered == 0 {
        for event in session.drain() {
            if let ListingEvent::Batch(batch) = event {
                delivered += batch.entries.len();
            }
        }
        std::hint::spin_loop();
    }

    let cancelled_at = Instant::now();
    token.cancel();
    reader.join().expect("reader thread");
    let latency = cancelled_at.elapsed();

    // Everything still in the channel was produced before the cancel landed.
    for event in session.drain() {
        if let ListingEvent::Batch(batch) = event {
            delivered += batch.entries.len();
        }
    }
    (latency, delivered)
}

fn main() {
    // `cargo bench -- --test` is the compile-and-run smoke check. Cargo also
    // passes a bare `--bench`, which is not a request for a short run.
    let smoke = std::env::args().any(|argument| argument == "--test");
    let iterations = if smoke { 1 } else { 5 };
    let entry_count = if smoke { 2_000 } else { ENTRY_COUNT };
    let mixed_count = if smoke { 500 } else { MIXED_COUNT };

    let root = tempfile::tempdir().expect("benchmark root");
    let flat = root.path().join("flat");
    let mixed = root.path().join("mixed");
    let tree = root.path().join("tree");

    let started = Instant::now();
    build_flat(&flat, entry_count);
    build_mixed(&mixed, mixed_count);
    let deepest = build_deep(&tree, DEEP_FANOUT, DEEP_DEPTH);
    println!(
        "files listing benchmarks: {entry_count} flat entries, {mixed_count} mixed-media \
         entries, depth {DEEP_DEPTH} tree, batch size {BATCH_SIZE}, median of {iterations} \
         iteration(s)"
    );
    println!(
        "fixture written in {:.1} s\n",
        started.elapsed().as_secs_f64()
    );

    let plain = ReaderConfig::new();
    let location = Location::local(&flat).expect("location");

    // --- The flat 100,000-entry directory. -------------------------------
    let mut first_batches = Vec::new();
    let mut completes = Vec::new();
    let mut last_run = None;
    for _ in 0..iterations {
        let run = run_listing(&flat, &plain, BATCH_SIZE);
        assert_eq!(run.total, entry_count);
        first_batches.push(run.first_batch);
        completes.push(run.complete);
        last_run = Some(run);
    }
    let run = last_run.expect("at least one run");
    let first = median(first_batches);
    let complete = median(completes);
    report(
        "flat: time to first batch",
        first,
        &format!("{BATCH_SIZE} entries visible"),
    );
    report(
        "flat: time to complete listing",
        complete,
        &format!("{} entries", run.total),
    );
    println!(
        "{:<38} {:>10.1} x     first batch is this much sooner than completion\n",
        "flat: responsiveness ratio",
        complete.as_secs_f64() / first.as_secs_f64().max(f64::EPSILON)
    );

    // --- Incremental sort, with the I/O removed. -------------------------
    let mut replay_samples = Vec::new();
    let mut held = 0usize;
    for _ in 0..iterations {
        let (elapsed, bytes) = replay(&location, &run.batches, SortOrder::default());
        replay_samples.push(elapsed);
        held = bytes;
    }
    report(
        "flat: incremental sort, all batches",
        median(replay_samples),
        &format!("{} batches merged in order", run.batches.len()),
    );
    report_bytes(
        "flat: model memory",
        held,
        &format!(
            "{:.0} bytes per entry",
            held as f64 / run.total.max(1) as f64
        ),
    );

    // What a frame actually pays. A frame drains every batch that arrived
    // since the last one and merges once, so the cost is measured that way.
    // Committing per batch is measured too, because it is the version that
    // does not work and the number says why.
    for (label, batches_per_commit, detail) in [
        (
            "flat: worst frame, coalesced",
            16usize,
            "one merge per frame's worth of batches",
        ),
        (
            "flat: worst frame, merge per batch",
            1usize,
            "the quadratic version, kept for comparison",
        ),
    ] {
        let mut worst_samples = Vec::new();
        let mut total_samples = Vec::new();
        for _ in 0..iterations {
            let mut model = DirectoryModel::new(
                location.clone(),
                SortOrder::default(),
                HiddenPreference::default(),
            );
            model.restart(run.batches[0].listing);
            let mut worst = Duration::ZERO;
            let mut total = Duration::ZERO;
            for chunk in run.batches.chunks(batches_per_commit) {
                let started = Instant::now();
                for batch in chunk {
                    model.apply(ListingEvent::Batch(batch.clone()));
                }
                model.commit();
                let elapsed = started.elapsed();
                worst = worst.max(elapsed);
                total += elapsed;
            }
            assert_eq!(model.total_len(), run.total);
            worst_samples.push(worst);
            total_samples.push(total);
        }
        report(label, median(worst_samples), detail);
        report(
            &format!("{label}, total"),
            median(total_samples),
            "assembling the whole directory",
        );
    }

    // Re-sorting a fully loaded directory, which is what clicking a column
    // header costs.
    let mut model = DirectoryModel::new(
        location.clone(),
        SortOrder::default(),
        HiddenPreference::default(),
    );
    model.restart(run.batches[0].listing);
    for batch in &run.batches {
        model.apply(ListingEvent::Batch(batch.clone()));
    }
    for (label, order) in [
        (
            "flat: re-sort by size",
            SortOrder::new(SortKey::Size, SortDirection::Descending),
        ),
        (
            "flat: re-sort by modified",
            SortOrder::new(SortKey::Modified, SortDirection::Descending),
        ),
        (
            "flat: re-sort by name, reversed",
            SortOrder::new(SortKey::Name, SortDirection::Descending),
        ),
    ] {
        let mut samples = Vec::new();
        for index in 0..iterations {
            // Alternate so each measured sort starts from a different order.
            let previous = if index % 2 == 0 {
                SortOrder::default()
            } else {
                SortOrder::new(SortKey::Extension, SortDirection::Ascending)
            };
            model.set_order(previous);
            let started = Instant::now();
            model.set_order(order);
            samples.push(started.elapsed());
        }
        report(
            label,
            median(samples),
            &format!("{} entries", model.total_len()),
        );
    }

    let mut samples = Vec::new();
    for _ in 0..iterations {
        let started = Instant::now();
        model.set_hidden_preference(HiddenPreference::showing_hidden());
        model.set_hidden_preference(HiddenPreference::default());
        samples.push(started.elapsed() / 2);
    }
    report(
        "flat: toggle hidden entries",
        median(samples),
        "no reload, re-filter only",
    );
    println!();

    // --- Cancellation. ----------------------------------------------------
    let mut latencies = Vec::new();
    let mut delivered = 0usize;
    for _ in 0..iterations {
        let (latency, produced) = cancellation_latency(&flat, BATCH_SIZE);
        latencies.push(latency);
        delivered = produced;
    }
    report(
        "flat: cancellation latency",
        median(latencies),
        &format!("reader stopped after {delivered} of {} entries", run.total),
    );
    println!();

    // --- Mixed media, with type detection on. -----------------------------
    let detector = ReaderConfig::new().with_mime(detector_from_env());
    let mut first_batches = Vec::new();
    let mut completes = Vec::new();
    let mut mixed_run = None;
    for _ in 0..iterations {
        let run = run_listing(&mixed, &detector, BATCH_SIZE);
        first_batches.push(run.first_batch);
        completes.push(run.complete);
        mixed_run = Some(run);
    }
    let mixed_run = mixed_run.expect("a mixed run");
    report(
        "mixed: time to first batch",
        median(first_batches),
        "with MIME detection",
    );
    report(
        "mixed: time to complete listing",
        median(completes),
        &format!("{} entries", mixed_run.total),
    );
    let mut plain_completes = Vec::new();
    for _ in 0..iterations {
        plain_completes.push(run_listing(&mixed, &plain, BATCH_SIZE).complete);
    }
    report(
        "mixed: same listing, no detection",
        median(plain_completes),
        "the cost detection adds is the difference",
    );
    let mixed_location = Location::local(&mixed).expect("location");
    let mut samples = Vec::new();
    for _ in 0..iterations {
        samples.push(
            replay(
                &mixed_location,
                &mixed_run.batches,
                SortOrder::new(SortKey::Type, SortDirection::Ascending),
            )
            .0,
        );
    }
    report(
        "mixed: incremental sort by type",
        median(samples),
        "folders first, then type",
    );
    println!();

    // --- One level of a deep tree. ----------------------------------------
    let mut first_batches = Vec::new();
    let mut completes = Vec::new();
    for _ in 0..iterations {
        let run = run_listing(&deepest, &plain, BATCH_SIZE);
        first_batches.push(run.first_batch);
        completes.push(run.complete);
    }
    report(
        "deep tree: time to first batch",
        median(first_batches),
        &format!("at depth {DEEP_DEPTH}"),
    );
    report(
        "deep tree: time to complete listing",
        median(completes),
        &format!("{} entries at that level", DEEP_FANOUT * 2),
    );
}
