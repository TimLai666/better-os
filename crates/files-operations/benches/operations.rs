//! Wall-clock benchmarks for the job engine.
//!
//! Four numbers decide whether this architecture is usable: how fast one large
//! file copies, how fast many small files copy, what a same-filesystem move
//! costs, and what the engine adds on top of the raw syscalls. All four are
//! measured against the real engine — real jobs, real plan walk, real progress
//! bookkeeping — because a benchmark of `fsops` alone would measure the half
//! that was never in doubt.
//!
//! There is no benchmark harness dependency, matching `files-core` and
//! `app-catalog-core`: these are wall-clock timings, exact about what they
//! measure, and a statistics framework would add a large dependency without
//! changing any decision this crate makes.
//!
//! Run with `cargo bench -p files-operations`, or
//! `cargo bench -p files-operations -- --test` for a fast single-iteration
//! smoke run over a much smaller tree.
//!
//! **Every number is timed against a temporary directory**, which on most
//! developer machines is `/tmp`. If `/tmp` is a tmpfs, the large-file figures
//! measure memory bandwidth rather than a disk, and the report says so.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use files_core::location::LocalPath;
use files_operations::{
    ConflictPolicy, CopyPolicy, EngineConfig, JobEngine, JobSpec, JobState, Operation,
};

const LARGE_BYTES: usize = 512 * 1024 * 1024;
const SMALL_COUNT: usize = 100_000;
const SMALL_BYTES: usize = 512;

struct Sizes {
    large_bytes: usize,
    small_count: usize,
    iterations: usize,
    move_iterations: usize,
}

fn main() {
    let smoke = std::env::args().any(|argument| argument == "--test");
    let sizes = if smoke {
        Sizes {
            large_bytes: 8 * 1024 * 1024,
            small_count: 200,
            iterations: 1,
            move_iterations: 1,
        }
    } else {
        Sizes {
            large_bytes: LARGE_BYTES,
            small_count: SMALL_COUNT,
            iterations: 3,
            move_iterations: 1,
        }
    };

    let root = tempfile::tempdir().expect("a temporary directory");
    println!("files-operations benchmarks");
    println!("working directory: {}", root.path().display());
    println!("filesystem: {}", describe_filesystem(root.path()));
    println!();

    large_file_copy(root.path(), &sizes);
    many_small_files_copy(root.path(), &sizes);
    same_filesystem_move(root.path(), &sizes);
    cross_filesystem_move(root.path(), &sizes);
    persistence_cost(root.path(), &sizes);
    persist_bytes(root.path());
    engine_overhead(root.path(), &sizes);
}

// --- The benchmarks ------------------------------------------------------

fn large_file_copy(root: &Path, sizes: &Sizes) {
    let source = root.join("large.bin");
    write_file(&source, sizes.large_bytes);
    let mut samples = Vec::new();
    for _ in 0..sizes.iterations {
        let destination = fresh(root, "large-destination");
        samples.push(time_job(
            Operation::Copy {
                sources: vec![local(&source)],
                destination: local(&destination),
            },
            CopyPolicy::default(),
        ));
        fs::remove_dir_all(&destination).ok();
    }
    report(
        "one large file: copy",
        median(&samples),
        &throughput(sizes.large_bytes as u64, median(&samples)),
    );

    // The same copy with verification off, so the cost of the final `stat` and
    // comparison is a number rather than an assumption.
    let mut unverified = Vec::new();
    for _ in 0..sizes.iterations {
        let destination = fresh(root, "large-unverified");
        unverified.push(time_job(
            Operation::Copy {
                sources: vec![local(&source)],
                destination: local(&destination),
            },
            CopyPolicy {
                verify: false,
                ..CopyPolicy::default()
            },
        ));
        fs::remove_dir_all(&destination).ok();
    }
    report(
        "one large file: copy without verification",
        median(&unverified),
        "the difference is what verification costs",
    );
    fs::remove_file(&source).ok();
    println!();
}

fn many_small_files_copy(root: &Path, sizes: &Sizes) {
    let source = root.join("many");
    fs::create_dir_all(&source).expect("the fixture directory");
    for index in 0..sizes.small_count {
        write_file(&source.join(format!("file-{index:06}.bin")), SMALL_BYTES);
    }
    let total_bytes = (sizes.small_count * SMALL_BYTES) as u64;

    let mut samples = Vec::new();
    for _ in 0..sizes.iterations {
        let destination = fresh(root, "many-destination");
        samples.push(time_job(
            Operation::Copy {
                sources: vec![local(&source)],
                destination: local(&destination),
            },
            CopyPolicy::default(),
        ));
        fs::remove_dir_all(&destination).ok();
    }
    let elapsed = median(&samples);
    report(
        &format!("{} small files: copy", sizes.small_count),
        elapsed,
        &format!(
            "{:.0} files/s, {}",
            sizes.small_count as f64 / elapsed.as_secs_f64(),
            throughput(total_bytes, elapsed)
        ),
    );

    // How much of that was the walk that counts the work before it starts.
    let mut plan_samples = Vec::new();
    for _ in 0..sizes.iterations {
        let started = Instant::now();
        let plan = files_operations::build_plan(
            &Operation::Copy {
                sources: vec![local(&source)],
                destination: local(&root.join("nowhere")),
            },
            &CopyPolicy::default(),
        );
        plan_samples.push(started.elapsed());
        assert_eq!(plan.total_items() as usize, sizes.small_count + 1);
    }
    report(
        &format!("{} small files: plan only", sizes.small_count),
        median(&plan_samples),
        "the walk that gives the job an honest total",
    );
    fs::remove_dir_all(&source).ok();
    println!();
}

fn same_filesystem_move(root: &Path, sizes: &Sizes) {
    let mut samples = Vec::new();
    // One iteration: rebuilding a hundred thousand fixture files per sample
    // would spend most of the benchmark's time on the fixture rather than on
    // the operation, and a move is `rename(2)`, which does not vary.
    for _ in 0..sizes.move_iterations {
        let source = fresh(root, "move-source");
        for index in 0..sizes.small_count {
            write_file(&source.join(format!("file-{index:06}.bin")), SMALL_BYTES);
        }
        let destination = fresh(root, "move-destination");
        samples.push(time_job(
            Operation::Move {
                sources: vec![local(&source)],
                destination: local(&destination),
            },
            CopyPolicy::default(),
        ));
        fs::remove_dir_all(&destination).ok();
        fs::remove_dir_all(&source).ok();
    }
    let elapsed = median(&samples);
    report(
        &format!("{} small files: same-filesystem move", sizes.small_count),
        elapsed,
        &format!(
            "{:.0} files/s, no bytes copied",
            sizes.small_count as f64 / elapsed.as_secs_f64()
        ),
    );
    println!();
}

fn cross_filesystem_move(root: &Path, sizes: &Sizes) {
    // Forced down the copy-verify-delete path by policy, because mounting a
    // second filesystem needs privilege a benchmark must not ask for. The code
    // is the code that runs when the kernel answers `EXDEV`; what is missing is
    // the slower device on the other end.
    let source = root.join("cross-source.bin");
    write_file(&source, sizes.large_bytes / 4);
    let mut samples = Vec::new();
    for _ in 0..sizes.iterations {
        let staging = root.join("cross-staging.bin");
        fs::copy(&source, &staging).expect("the fixture");
        let destination = fresh(root, "cross-destination");
        samples.push(time_job(
            Operation::Move {
                sources: vec![local(&staging)],
                destination: local(&destination),
            },
            CopyPolicy {
                moves: files_operations::MoveStrategy::AlwaysCopyThenDelete,
                ..CopyPolicy::default()
            },
        ));
        fs::remove_dir_all(&destination).ok();
    }
    let bytes = (sizes.large_bytes / 4) as u64;
    report(
        "one file: cross-filesystem move (copy, verify, delete)",
        median(&samples),
        &throughput(bytes, median(&samples)),
    );
    fs::remove_file(&source).ok();
    println!();
}

/// What persistence costs.
///
/// The ticket asks for completion *and* persistence time. A whole write — the
/// header and the compacted item journal — happens at submission, at the end,
/// and at compaction; a running job's persists append instead, and
/// [`persist_bytes`] measures those.
fn persistence_cost(root: &Path, sizes: &Sizes) {
    let source = root.join("persist-source");
    fs::create_dir_all(&source).expect("the fixture directory");
    for index in 0..sizes.small_count.min(10_000) {
        write_file(&source.join(format!("file-{index:06}.bin")), 8);
    }
    let store_root = root.join("persist-jobs");
    let store = files_operations::JobStore::new(&store_root);
    let engine = JobEngine::new(EngineConfig {
        workers: 1,
        store: Some(store.clone()),
        conflicts: ConflictPolicy::new(),
    });
    let destination = fresh(root, "persist-destination");
    let started = Instant::now();
    let handle = engine
        .submit(JobSpec::new(Operation::Copy {
            sources: vec![local(&source)],
            destination: local(&destination),
        }))
        .expect("the spec was accepted");
    let snapshot = engine
        .wait(handle.id(), Duration::from_secs(600))
        .expect("the job finished");
    let with_store = started.elapsed();
    assert_eq!(snapshot.state, JobState::Completed);

    let record = store.read(handle.id().value()).expect("the record");
    let items = record.items.len();
    let mut writes = Vec::new();
    for _ in 0..sizes.iterations.max(3) {
        let started = Instant::now();
        store.write(&record).expect("the record was written");
        writes.push(started.elapsed());
    }
    report(
        &format!("{items} items: copy with a job store attached"),
        with_store,
        "compare with the same count above, which had none",
    );
    report(
        &format!("{items} items: one whole write"),
        median(&writes),
        "after planning, at the end, and at compaction",
    );
    let size: u64 = ["json", "items.jsonl"]
        .iter()
        .map(|extension| {
            fs::metadata(store_root.join(format!("job-{:020}.{extension}", handle.id().value())))
                .map(|metadata| metadata.len())
                .unwrap_or(0)
        })
        .sum();
    println!(
        "{:<56} {:>10} bytes",
        "the record on disk, header and journal", size
    );
    fs::remove_dir_all(&source).ok();
    fs::remove_dir_all(&destination).ok();
    println!();
}

/// Bytes one persist of a running job writes, before and after the item
/// journal, at 10,001 and 100,001 items.
///
/// Before: the single-file record every persist used to rewrite, serialized
/// exactly as that writer did. After: the header plus the status lines that
/// changed since the last persist, for one changed item and for one 250 ms
/// interval at the small-file rate measured above (about 28,600 files/s, so
/// 7,157 items). Both use the same record, including a log at its cap. Always
/// run at full size: it serializes and writes records, and copies nothing.
fn persist_bytes(root: &Path) {
    const INTERVAL_ITEMS: usize = 7_157;
    let store = files_operations::JobStore::new(root.join("persist-bytes"));
    for (id, count) in [(1u64, 10_001usize), (2, 100_001)] {
        let mut record = synthetic_record(root, id, count);
        let mut legacy = record.clone();
        legacy.schema_version = 1;
        let before = serde_json::to_vec_pretty(&legacy)
            .expect("the single-file record")
            .len();
        let whole = store.write(&record).expect("the whole write");

        record.items[0].status = files_operations::ItemStatus::Done;
        let one = store
            .append(id, &[status_line(0)])
            .expect("one status line")
            + store.write_header(&record).expect("the header");

        let lines: Vec<files_operations::JournalEntry> =
            (1..=INTERVAL_ITEMS).map(status_line).collect();
        let interval = store
            .append(id, &lines)
            .expect("an interval of status lines")
            + store.write_header(&record).expect("the header");

        println!("{count} items, bytes written per persist:");
        println!(
            "{:<56} {:>12} bytes",
            "  before: the whole single-file record", before
        );
        println!(
            "{:<56} {:>12} bytes",
            "  after: whole write, after planning and at the end", whole
        );
        println!("{:<56} {:>12} bytes", "  after: one item changed", one);
        println!(
            "{:<56} {:>12} bytes",
            format!("  after: {INTERVAL_ITEMS} items changed (one interval)"),
            interval
        );
    }
    println!();
}

fn status_line(index: usize) -> files_operations::JournalEntry {
    files_operations::JournalEntry::Status {
        index: index as u64,
        status: files_operations::ItemStatus::Done,
        error: None,
    }
}

/// A copy job's record the way the engine builds one: every item planned, and
/// a log that has hit its cap with a start and a completion per item.
fn synthetic_record(root: &Path, id: u64, count: usize) -> files_operations::JobRecord {
    let source = root.join("persist-source");
    let destination = root.join("persist-destination");
    let mut log = files_operations::OperationLog::default();
    for index in 0..count {
        let path = source.join(format!("file-{index:06}.bin"));
        log.push(
            index as u64,
            Some(path.clone()),
            files_operations::LogEvent::ItemStarted { bytes: 512 },
        );
        log.push(
            index as u64,
            Some(path),
            files_operations::LogEvent::ItemCompleted {
                bytes: 512,
                verified: true,
            },
        );
    }
    files_operations::JobRecord {
        schema_version: files_operations::store::RECORD_SCHEMA_VERSION,
        id,
        kind: files_operations::OperationKind::Copy,
        state: JobState::Running,
        progress: files_operations::Progress {
            items_total: count as u64,
            bytes_total: 512 * count as u64,
            ..files_operations::Progress::default()
        },
        items: (0..count)
            .map(|index| files_operations::ItemRecord {
                source: source.join(format!("file-{index:06}.bin")),
                destination: Some(destination.join(format!("file-{index:06}.bin"))),
                status: files_operations::ItemStatus::Pending,
                bytes: 512,
                error: None,
            })
            .collect(),
        log,
        updated_at: 1,
        checksums: Vec::new(),
    }
}

fn engine_overhead(root: &Path, sizes: &Sizes) {
    // What the engine costs on top of the syscalls: the same copy driven by
    // `fsops` directly, with no plan, no progress, no log, and no worker.
    let source = root.join("overhead.bin");
    write_file(&source, sizes.large_bytes / 8);
    let policy = CopyPolicy::default();
    let mut raw = Vec::new();
    for index in 0..sizes.iterations {
        let destination = root.join(format!("overhead-out-{index}.bin"));
        let started = Instant::now();
        let mut hook = |_: u64| Ok(());
        files_operations::fsops::copy_file(&source, &destination, &policy, &mut hook)
            .expect("the copy");
        raw.push(started.elapsed());
        fs::remove_file(&destination).ok();
    }
    let mut through_engine = Vec::new();
    for _ in 0..sizes.iterations {
        let destination = fresh(root, "overhead-destination");
        through_engine.push(time_job(
            Operation::Copy {
                sources: vec![local(&source)],
                destination: local(&destination),
            },
            policy,
        ));
        fs::remove_dir_all(&destination).ok();
    }
    report(
        "one file: fsops::copy_file directly",
        median(&raw),
        "no job",
    );
    report(
        "one file: the same copy as a job",
        median(&through_engine),
        "the difference is what the engine costs",
    );
    fs::remove_file(&source).ok();
}

// --- Scaffolding ---------------------------------------------------------

fn time_job(operation: Operation, policy: CopyPolicy) -> Duration {
    let engine = JobEngine::new(EngineConfig {
        workers: 1,
        store: None,
        conflicts: ConflictPolicy::new(),
    });
    let started = Instant::now();
    let handle = engine
        .submit(JobSpec::new(operation).with_policy(policy))
        .expect("the spec was accepted");
    let snapshot = engine
        .wait(handle.id(), Duration::from_secs(600))
        .expect("the job finished");
    let elapsed = started.elapsed();
    assert_eq!(
        snapshot.state,
        JobState::Completed,
        "the benchmark job failed: {:?}",
        snapshot.failures
    );
    elapsed
}

fn local(path: &Path) -> LocalPath {
    LocalPath::new(path).expect("an absolute path")
}

fn fresh(root: &Path, name: &str) -> PathBuf {
    let path = root.join(name);
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).expect("a fresh directory");
    path
}

fn write_file(path: &Path, size: usize) {
    // A repeating, position-dependent pattern rather than zeroes, so a
    // filesystem with transparent compression or hole detection cannot make
    // the benchmark look faster than a real file would be.
    let block: Vec<u8> = (0..64 * 1024).map(|index| (index % 251) as u8).collect();
    let mut written = 0;
    let mut file = fs::File::create(path).expect("the fixture file");
    use std::io::Write;
    while written < size {
        let want = block.len().min(size - written);
        file.write_all(&block[..want]).expect("writing the fixture");
        written += want;
    }
    file.sync_all().expect("flushing the fixture");
}

fn median(samples: &[Duration]) -> Duration {
    let mut sorted = samples.to_vec();
    sorted.sort();
    sorted[sorted.len() / 2]
}

fn throughput(bytes: u64, elapsed: Duration) -> String {
    let seconds = elapsed.as_secs_f64();
    if seconds <= 0.0 {
        return "instant".to_string();
    }
    format!("{:.0} MB/s", bytes as f64 / seconds / 1_000_000.0)
}

fn report(label: &str, elapsed: Duration, note: &str) {
    println!(
        "{label:<56} {:>10.3} ms   {note}",
        elapsed.as_secs_f64() * 1000.0
    );
}

/// Names the filesystem behind a path, so a tmpfs result is not read as a disk
/// result. Read from `/proc/self/mounts` by longest matching mount point.
fn describe_filesystem(path: &Path) -> String {
    let Ok(mounts) = fs::read_to_string("/proc/self/mounts") else {
        return "unknown".to_string();
    };
    let mut best: Option<(usize, String)> = None;
    for line in mounts.lines() {
        let mut fields = line.split_whitespace();
        let _source = fields.next();
        let Some(mount_point) = fields.next() else {
            continue;
        };
        let Some(kind) = fields.next() else { continue };
        if path.starts_with(mount_point)
            && best
                .as_ref()
                .is_none_or(|(len, _)| mount_point.len() > *len)
        {
            best = Some((mount_point.len(), format!("{kind} at {mount_point}")));
        }
    }
    best.map(|(_, description)| description)
        .unwrap_or_else(|| "unknown".to_string())
}
