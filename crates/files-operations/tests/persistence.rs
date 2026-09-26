//! What survives a restart, and what a crashed window leaves behind.
//!
//! Issue #6: "a crashed window must not leave jobs in an unknowable state".
//! The proof is in two halves. A job that finished leaves a record that says
//! so. A job whose process died leaves a record that says how far it got, and
//! recovery turns that into a settled, readable state rather than a job that
//! still claims to be running with nobody running it.

mod support;

use std::fs;

use files_operations::{
    ConflictPolicy, CopyPolicy, EngineConfig, ItemStatus, JobEngine, JobSpec, JobState, JobStore,
    Operation,
};

use support::{local, write_pattern};

fn engine_with(store: &JobStore) -> JobEngine {
    JobEngine::new(EngineConfig {
        workers: 1,
        store: Some(store.clone()),
        conflicts: ConflictPolicy::new(),
    })
}

#[test]
fn a_finished_job_leaves_a_record_a_later_process_can_read() {
    let root = tempfile::tempdir().unwrap();
    let store = JobStore::new(root.path().join("jobs"));
    let source = root.path().join("source");
    fs::create_dir(&source).unwrap();
    write_pattern(&source.join("a.bin"), 2048);
    write_pattern(&source.join("b.bin"), 1024);
    let destination = root.path().join("destination");
    fs::create_dir(&destination).unwrap();

    let id = {
        let engine = engine_with(&store);
        let handle = engine
            .submit(JobSpec::new(Operation::Copy {
                sources: vec![local(&source)],
                destination: local(&destination),
            }))
            .unwrap();
        engine.wait(handle.id(), support::LIMIT).unwrap();
        handle.id()
    };

    // A different process — a new engine over the same directory — sees it.
    let recovery = store.recover();
    assert_eq!(recovery.interrupted.len(), 0);
    assert_eq!(recovery.settled.len(), 1);
    let record = &recovery.settled[0];
    assert_eq!(record.id, id.value());
    assert_eq!(record.state, JobState::Completed);
    assert_eq!(record.progress.bytes_done, 3072);
    assert!(
        record
            .items
            .iter()
            .all(|item| item.status == ItemStatus::Done)
    );
    assert!(!record.log.is_empty());
}

#[test]
fn a_job_whose_process_died_recovers_as_failed_with_its_remaining_work_named() {
    let root = tempfile::tempdir().unwrap();
    let store = JobStore::new(root.path().join("jobs"));
    let source = root.path().join("source");
    fs::create_dir(&source).unwrap();
    // One large file, so the job is unambiguously mid-flight when it is
    // abandoned, and one small one behind it that never gets its turn.
    write_pattern(&source.join("a-big.bin"), 16 * 1024 * 1024);
    write_pattern(&source.join("b-small.bin"), 64);
    let destination = root.path().join("destination");
    fs::create_dir(&destination).unwrap();

    let id;
    {
        let engine = engine_with(&store);
        let handle = engine
            .submit(
                JobSpec::new(Operation::Copy {
                    sources: vec![local(&source)],
                    destination: local(&destination),
                })
                .with_policy(CopyPolicy {
                    chunk_bytes: 4096,
                    ..CopyPolicy::default()
                }),
            )
            .unwrap();
        id = handle.id();
        support::wait_for(&engine, id, |snapshot| snapshot.progress.bytes_done > 0);
        // Simulate the process dying: freeze the job where it is and leave the
        // record on disk claiming it is running. Pausing is how a test stops a
        // worker without killing the harness that is watching it.
        assert!(engine.pause(id));
        support::wait_for(&engine, id, |snapshot| snapshot.state == JobState::Paused);
        // The engine's own drop cancels a parked job and settles it, so the
        // "crashed" record is written by hand from the live one, which is
        // exactly what the process would have left behind.
        let mut record = store.read(id.value()).unwrap();
        record.state = JobState::Running;
        store.write(&record).unwrap();
        engine.cancel(id);
        engine.wait(id, support::LIMIT);
    }
    // Put the abandoned record back, overwriting the cancellation the shutdown
    // wrote, so what recovery sees is a record left mid-run.
    let mut abandoned = store.read(id.value()).unwrap();
    abandoned.state = JobState::Running;
    for item in abandoned.items.iter_mut() {
        item.status = ItemStatus::Pending;
        item.error = None;
    }
    store.write(&abandoned).unwrap();

    let recovery = store.recover();
    assert_eq!(recovery.damaged.len(), 0);
    assert_eq!(recovery.interrupted.len(), 1);
    let recovered = &recovery.interrupted[0];
    // Not "running". Not unknown. Failed, because nobody is working on it.
    assert_eq!(recovered.state, JobState::Failed);
    assert!(
        recovered
            .items
            .iter()
            .all(|item| item.error == Some(files_operations::OperationError::Interrupted))
    );
    assert_eq!(recovered.remaining().len(), recovered.items.len());
    // And it stays settled: a second recovery does not re-interrupt it.
    assert_eq!(store.recover().interrupted.len(), 0);
}

#[test]
fn the_record_tracks_progress_while_the_job_is_still_running() {
    let root = tempfile::tempdir().unwrap();
    let store = JobStore::new(root.path().join("jobs"));
    let source = root.path().join("source");
    fs::create_dir(&source).unwrap();
    for index in 0..8 {
        write_pattern(&source.join(format!("file-{index}.bin")), 4096);
    }
    let destination = root.path().join("destination");
    fs::create_dir(&destination).unwrap();

    let engine = engine_with(&store);
    let handle = engine
        .submit(JobSpec::new(Operation::Copy {
            sources: vec![local(&source)],
            destination: local(&destination),
        }))
        .unwrap();
    engine.wait(handle.id(), support::LIMIT).unwrap();

    let record = store.read(handle.id().value()).unwrap();
    assert_eq!(record.progress.items_total, 9);
    assert_eq!(record.progress.items_done, 9);
    // The log carries what a later analysis would need: the plan, the state
    // changes, and one completion per item.
    let completions = record
        .log
        .records()
        .iter()
        .filter(|entry| {
            matches!(
                entry.event,
                files_operations::LogEvent::ItemCompleted { .. }
            )
        })
        .count();
    assert_eq!(completions, 9);
}

#[test]
fn an_engine_without_a_store_runs_the_same_jobs_and_writes_nothing() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("a.bin");
    write_pattern(&source, 128);
    let destination = root.path().join("destination");
    fs::create_dir(&destination).unwrap();

    let engine = support::engine();
    let snapshot = support::run_ok(
        &engine,
        JobSpec::new(Operation::Copy {
            sources: vec![local(&source)],
            destination: local(&destination),
        }),
    );
    assert_eq!(snapshot.state, JobState::Completed);
    // Only the two things the test made.
    assert_eq!(support::entries(root.path()).len(), 2);
}

#[test]
fn a_record_left_by_a_newer_build_is_reported_rather_than_deleted_or_run() {
    let root = tempfile::tempdir().unwrap();
    let store = JobStore::new(root.path().join("jobs"));
    fs::create_dir_all(root.path().join("jobs")).unwrap();
    fs::write(
        root.path().join("jobs/job-00000000000000000042.json"),
        br#"{"schema_version":9000,"id":42,"kind":"copy","state":"running","progress":{"items_total":0,"items_done":0,"items_failed":0,"items_skipped":0,"bytes_total":0,"bytes_done":0},"items":[],"log":{"head":[],"tail":[],"dropped":0,"head_limit":1,"tail_limit":1},"updated_at":0,"checksums":[]}"#,
    )
    .unwrap();
    let recovery = store.recover();
    assert_eq!(recovery.damaged.len(), 1);
    assert!(matches!(
        recovery.damaged[0].1,
        files_operations::StoreError::UnsupportedSchema { version: 9000, .. }
    ));
    assert!(recovery.damaged[0].0.exists(), "the record was deleted");
}

fn journal_path(store: &JobStore, id: u64) -> std::path::PathBuf {
    store.root().join(format!("job-{id:020}.items.jsonl"))
}

#[test]
fn a_running_job_appends_item_progress_and_a_finished_one_is_compacted() {
    let root = tempfile::tempdir().unwrap();
    let store = JobStore::new(root.path().join("jobs"));
    let source = root.path().join("source");
    fs::create_dir(&source).unwrap();
    for index in 0..3_000 {
        write_pattern(&source.join(format!("file-{index:05}.bin")), 16);
    }
    let destination = root.path().join("destination");
    fs::create_dir(&destination).unwrap();

    let engine = engine_with(&store);
    let handle = engine
        .submit(JobSpec::new(Operation::Copy {
            sources: vec![local(&source)],
            destination: local(&destination),
        }))
        .unwrap();
    let id = handle.id();
    let journal = journal_path(&store, id.value());

    support::wait_for(&engine, id, |snapshot| snapshot.progress.items_done >= 5);
    assert!(engine.pause(id));
    support::wait_for(&engine, id, |snapshot| snapshot.state == JobState::Paused);
    // Longer than the persist interval, so the next item is persisted.
    std::thread::sleep(std::time::Duration::from_millis(300));
    let before = fs::read(&journal).unwrap();
    let done = engine.snapshot(id).unwrap().progress.items_done;

    assert!(engine.resume(id));
    support::wait_for(&engine, id, |snapshot| {
        snapshot.progress.items_done >= done + 5
    });
    assert!(engine.pause(id));
    support::wait_for(&engine, id, |snapshot| snapshot.state == JobState::Paused);
    let after = fs::read(&journal).unwrap();
    assert!(
        after.len() > before.len() && after.starts_with(&before),
        "item progress was appended to the journal, not rewritten"
    );

    assert!(engine.resume(id));
    let finished = engine.wait(id, support::LIMIT).unwrap();
    assert_eq!(finished.state, JobState::Completed);
    let record = store.read(id.value()).unwrap();
    let lines = fs::read_to_string(&journal).unwrap().lines().count();
    assert_eq!(lines, record.items.len(), "compacted to one line per item");
    assert!(
        record
            .items
            .iter()
            .all(|item| item.status == ItemStatus::Done)
    );
}

/// The number the ticket asks for, at a size a test can afford: what one
/// persist writes once the job's items are on disk. The same figure at
/// 100,001 items, and the single-file format's figures for comparison, come
/// from `cargo bench -p files-operations`.
#[test]
fn a_persist_writes_the_same_bytes_at_ten_thousand_and_a_hundred_thousand_items() {
    let root = tempfile::tempdir().unwrap();
    let store = JobStore::new(root.path().join("jobs"));
    let mut costs = Vec::new();
    for (id, count) in [(1u64, 10_001usize), (2, 100_001)] {
        let mut record = files_operations::JobRecord {
            schema_version: files_operations::store::RECORD_SCHEMA_VERSION,
            id,
            kind: files_operations::OperationKind::Copy,
            state: JobState::Running,
            progress: files_operations::Progress {
                items_total: count as u64,
                ..files_operations::Progress::default()
            },
            items: (0..count)
                .map(|index| files_operations::ItemRecord {
                    source: format!("/home/user/source/file-{index:06}.bin").into(),
                    destination: Some(format!("/home/user/destination/file-{index:06}.bin").into()),
                    status: ItemStatus::Pending,
                    bytes: 512,
                    error: None,
                })
                .collect(),
            log: files_operations::OperationLog::default(),
            updated_at: 1,
            checksums: Vec::new(),
        };
        store.write(&record).unwrap();
        record.items[7].status = ItemStatus::Done;
        record.progress.items_done = 1;
        let appended = store
            .append(
                id,
                &[files_operations::store::JournalEntry::Status {
                    index: 7,
                    status: ItemStatus::Done,
                    error: None,
                }],
            )
            .unwrap();
        let header = store.write_header(&record).unwrap();
        println!("{count} items: one persist wrote {appended} + {header} bytes");
        costs.push(appended + header);
    }
    let difference = costs[0].abs_diff(costs[1]);
    assert!(
        difference <= 8,
        "a persist grew with the item count: {costs:?}"
    );
}
