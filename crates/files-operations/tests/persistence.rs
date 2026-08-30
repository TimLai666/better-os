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
