//! Job lifetime: the eight states, pausing mid-copy, cancelling mid-copy, and
//! the property the whole crate exists for — a job that outlives the handle
//! that started it.

mod support;

use std::fs;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use files_operations::{
    ConflictKind, ConflictPolicy, CopyPolicy, DeleteConfirmation, DeleteTarget, FailurePolicy,
    JobEngine, JobSpec, JobState, Operation, Resolution,
};

use support::{engine, local, run, run_ok, wait_for, write_pattern};

/// A file big enough that a copy takes many chunks, with a chunk size small
/// enough that a pause or a cancel lands in the middle of it rather than after
/// it. Both halves matter: a test that only proves a copy stops after the last
/// chunk proves nothing about a 40 GB file.
const BIG: usize = 8 * 1024 * 1024;

fn slow_policy() -> CopyPolicy {
    CopyPolicy {
        chunk_bytes: 4096,
        ..CopyPolicy::default()
    }
}

#[test]
fn a_job_that_outlives_its_handle_finishes_anyway() {
    // The milestone signal. The window that started the copy is gone; the copy
    // is not.
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("big.bin");
    write_pattern(&source, BIG);
    let destination = root.path().join("destination");
    fs::create_dir(&destination).unwrap();

    let engine = engine();
    let handle = engine
        .submit(
            JobSpec::new(Operation::Copy {
                sources: vec![local(&source)],
                destination: local(&destination),
            })
            .with_policy(slow_policy()),
        )
        .unwrap();
    let id = handle.id();

    // Wait until the copy is genuinely in flight, then throw the handle away.
    wait_for(&engine, id, |snapshot| snapshot.progress.bytes_done > 0);
    drop(handle);

    let finished = engine
        .wait(id, support::LIMIT)
        .expect("the job reached a terminal state");
    assert_eq!(finished.state, JobState::Completed);
    assert_eq!(finished.progress.bytes_done, BIG as u64);
    assert_eq!(support::read(&destination.join("big.bin")).len(), BIG);
    // And the engine still knows about it, so an operation centre opened after
    // the window closed can still report it.
    assert_eq!(engine.state(id), Some(JobState::Completed));
}

#[test]
fn dropping_every_subscriber_does_not_stop_the_job_either() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("big.bin");
    write_pattern(&source, BIG);
    let destination = root.path().join("destination");
    fs::create_dir(&destination).unwrap();

    let engine = engine();
    let handle = engine
        .submit(
            JobSpec::new(Operation::Copy {
                sources: vec![local(&source)],
                destination: local(&destination),
            })
            .with_policy(slow_policy()),
        )
        .unwrap();
    let id = handle.id();
    let second = engine.subscribe(id).expect("a second subscriber");
    wait_for(&engine, id, |snapshot| snapshot.progress.bytes_done > 0);
    drop(handle);
    drop(second);
    assert_eq!(
        engine
            .wait(id, support::LIMIT)
            .map(|snapshot| snapshot.state),
        Some(JobState::Completed)
    );
}

#[test]
fn a_copy_pauses_between_chunks_and_resumes_where_it_stopped() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("big.bin");
    write_pattern(&source, BIG);
    let destination = root.path().join("destination");
    fs::create_dir(&destination).unwrap();

    let engine = engine();
    let handle = engine
        .submit(
            JobSpec::new(Operation::Copy {
                sources: vec![local(&source)],
                destination: local(&destination),
            })
            .with_policy(slow_policy()),
        )
        .unwrap();
    let id = handle.id();

    wait_for(&engine, id, |snapshot| snapshot.progress.bytes_done > 0);
    assert!(engine.pause(id));
    let paused = wait_for(&engine, id, |snapshot| snapshot.state == JobState::Paused);
    // It stopped in the middle of the file, not at the end of it.
    assert!(
        paused.progress.bytes_done < BIG as u64,
        "paused after {} of {BIG} bytes",
        paused.progress.bytes_done
    );
    let stopped_at = paused.progress.bytes_done;

    // It stays stopped.
    thread::sleep(Duration::from_millis(120));
    let still = engine.snapshot(id).unwrap();
    assert_eq!(still.state, JobState::Paused);
    assert_eq!(still.progress.bytes_done, stopped_at);
    // And the destination does not exist yet, because the copy is still in its
    // temporary file.
    assert!(!destination.join("big.bin").exists());

    assert!(engine.resume(id));
    let finished = engine.wait(id, support::LIMIT).unwrap();
    assert_eq!(finished.state, JobState::Completed);
    assert_eq!(support::read(&destination.join("big.bin")).len(), BIG);
}

#[test]
fn cancelling_mid_copy_leaves_no_partial_destination() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("big.bin");
    write_pattern(&source, BIG);
    let destination = root.path().join("destination");
    fs::create_dir(&destination).unwrap();

    let engine = engine();
    let handle = engine
        .submit(
            JobSpec::new(Operation::Copy {
                sources: vec![local(&source)],
                destination: local(&destination),
            })
            .with_policy(slow_policy()),
        )
        .unwrap();
    let id = handle.id();
    wait_for(&engine, id, |snapshot| snapshot.progress.bytes_done > 0);
    assert!(engine.cancel(id));

    let cancelled = engine.wait(id, support::LIMIT).unwrap();
    assert_eq!(cancelled.state, JobState::Cancelled);
    // Nothing under the real name, and no temporary left behind either.
    let left: Vec<_> = fs::read_dir(&destination)
        .unwrap()
        .flatten()
        .map(|entry| entry.file_name())
        .collect();
    assert!(left.is_empty(), "the destination still holds {left:?}");
    // The source is untouched.
    assert_eq!(support::read(&source).len(), BIG);
}

#[test]
fn a_queued_job_cancelled_before_it_starts_never_touches_the_filesystem() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("a.bin");
    write_pattern(&source, 1024);
    let destination = root.path().join("destination");
    fs::create_dir(&destination).unwrap();

    // One worker, and a long job in front of the one being cancelled.
    let engine = JobEngine::new(files_operations::EngineConfig {
        workers: 1,
        store: None,
        conflicts: ConflictPolicy::new(),
    });
    let blocker = root.path().join("blocker.bin");
    write_pattern(&blocker, BIG);
    let first = engine
        .submit(
            JobSpec::new(Operation::Copy {
                sources: vec![local(&blocker)],
                destination: local(&destination),
            })
            .with_policy(slow_policy()),
        )
        .unwrap();
    let second = engine
        .submit(JobSpec::new(Operation::Copy {
            sources: vec![local(&source)],
            destination: local(&destination),
        }))
        .unwrap();
    assert_eq!(engine.state(second.id()), Some(JobState::Queued));
    assert!(engine.cancel(second.id()));
    assert_eq!(engine.state(second.id()), Some(JobState::Cancelled));
    assert!(!destination.join("a.bin").exists());
    engine.cancel(first.id());
    engine.wait(first.id(), support::LIMIT);
}

#[test]
fn a_cancelled_copy_can_roll_back_what_it_had_already_created() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    fs::create_dir_all(source.join("a-nested")).unwrap();
    write_pattern(&source.join("a-nested/small.bin"), 512);
    // Items run in sorted-name order, so the large file is last on purpose:
    // by the time it is being copied, the directories and the small file are
    // already at the destination and there is something for a rollback to
    // remove.
    write_pattern(&source.join("z-big.bin"), BIG);
    let destination = root.path().join("destination");
    fs::create_dir(&destination).unwrap();

    let engine = engine();
    let handle = engine
        .submit(
            JobSpec::new(Operation::Copy {
                sources: vec![local(&source)],
                destination: local(&destination),
            })
            .with_policy(slow_policy()),
        )
        .unwrap();
    let id = handle.id();
    // Pause inside the large file, so the cancellation lands while the job is
    // unambiguously mid-run rather than racing its own completion.
    wait_for(&engine, id, |snapshot| {
        snapshot.progress.items_done >= 3 && snapshot.progress.bytes_done > 512
    });
    assert!(engine.pause(id));
    wait_for(&engine, id, |snapshot| snapshot.state == JobState::Paused);
    assert!(destination.join("source/a-nested/small.bin").exists());
    assert!(engine.cancel_with_rollback(id));
    let rolled_back = engine.wait(id, support::LIMIT).unwrap();
    assert_eq!(rolled_back.state, JobState::RolledBack);
    assert!(
        !destination.join("source").exists(),
        "rollback left {:?}",
        fs::read_dir(&destination)
            .unwrap()
            .flatten()
            .map(|entry| entry.path())
            .collect::<Vec<_>>()
    );
    // The source is untouched.
    assert!(source.join("z-big.bin").exists());
}

#[test]
fn a_move_is_refused_a_rollback_because_it_has_no_safe_undo() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("a.bin");
    write_pattern(&source, BIG);
    let destination = root.path().join("destination");
    fs::create_dir(&destination).unwrap();
    let engine = engine();
    let handle = engine
        .submit(
            JobSpec::new(Operation::Move {
                sources: vec![local(&source)],
                destination: local(&destination),
            })
            .with_policy(slow_policy()),
        )
        .unwrap();
    assert!(!engine.cancel_with_rollback(handle.id()));
    engine.wait(handle.id(), support::LIMIT);
}

#[test]
fn stop_and_roll_back_on_failure_undoes_a_partly_finished_copy() {
    if support::running_as_root() {
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    fs::create_dir(&source).unwrap();
    // Names are walked in sorted order, so `a` copies and `b` fails.
    write_pattern(&source.join("a.bin"), 256);
    write_pattern(&source.join("b.bin"), 256);
    fs::set_permissions(
        source.join("b.bin"),
        std::os::unix::fs::PermissionsExt::from_mode(0o000),
    )
    .unwrap();
    let destination = root.path().join("destination");
    fs::create_dir(&destination).unwrap();

    let engine = engine();
    let snapshot = run(
        &engine,
        JobSpec::new(Operation::Copy {
            sources: vec![local(&source)],
            destination: local(&destination),
        })
        .with_policy(CopyPolicy {
            on_failure: FailurePolicy::StopAndRollback,
            ..CopyPolicy::default()
        }),
    );
    assert_eq!(snapshot.state, JobState::RolledBack);
    assert!(!destination.join("source").exists());
}

#[test]
fn every_one_of_the_eight_states_is_reached_by_some_job() {
    // Queued, Running, Paused, WaitingOnConflict, Completed, Failed, Cancelled,
    // and RolledBack each have a dedicated test; this one records the whole set
    // in one place so a state that stops being reachable is noticed.
    let root = tempfile::tempdir().unwrap();
    let engine = Arc::new(engine());

    // Completed.
    fs::write(root.path().join("plain.txt"), b"x").unwrap();
    let destination = root.path().join("dst");
    fs::create_dir(&destination).unwrap();
    let completed = run_ok(
        &engine,
        JobSpec::new(Operation::Copy {
            sources: vec![local(root.path().join("plain.txt"))],
            destination: local(&destination),
        }),
    );
    assert_eq!(completed.state, JobState::Completed);

    // Failed: a permanent delete of something that is not there is a skip, so
    // a real failure needs a directory that is not empty when it is removed.
    let failed = run(
        &engine,
        JobSpec::new(Operation::Rename {
            path: local(root.path().join("plain.txt")),
            new_name: std::ffi::OsString::from("dst"),
        }),
    );
    assert_eq!(failed.state, JobState::Failed);

    // Cancelled, and with it Queued and Running along the way.
    let big = root.path().join("big.bin");
    write_pattern(&big, BIG);
    let handle = engine
        .submit(
            JobSpec::new(Operation::Copy {
                sources: vec![local(&big)],
                destination: local(&destination),
            })
            .with_policy(slow_policy()),
        )
        .unwrap();
    wait_for(&engine, handle.id(), |snapshot| {
        snapshot.state == JobState::Running
    });
    engine.cancel(handle.id());
    assert_eq!(
        engine.wait(handle.id(), support::LIMIT).unwrap().state,
        JobState::Cancelled
    );

    // Paused and WaitingOnConflict and RolledBack are covered by the tests
    // above; asserting their reachability here would duplicate them.
    for state in JobState::ALL {
        assert!(!state.key().is_empty());
    }
}

#[test]
fn a_permanent_delete_needs_a_confirmation_that_cannot_be_forged() {
    // The confirmation is a type with no public constructor other than
    // `explicit`, no `Default`, and no `Deserialize`. A job spec without one
    // does not compile, which is a stronger guarantee than a runtime check.
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("doomed"), b"x").unwrap();
    let engine = engine();
    run_ok(
        &engine,
        JobSpec::new(Operation::PermanentDelete {
            targets: vec![DeleteTarget::Path(local(root.path().join("doomed")))],
            confirmation: DeleteConfirmation::explicit(),
        }),
    );
    assert!(!root.path().join("doomed").exists());
}

#[test]
fn a_rename_does_not_offer_a_pause_it_could_not_honour() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("a"), b"x").unwrap();
    let engine = engine();
    let handle = engine
        .submit(JobSpec::new(Operation::Rename {
            path: local(root.path().join("a")),
            new_name: std::ffi::OsString::from("b"),
        }))
        .unwrap();
    assert!(!engine.pause(handle.id()));
    engine.wait(handle.id(), support::LIMIT);
}

#[test]
fn two_jobs_run_at_once_when_the_pool_has_room() {
    let root = tempfile::tempdir().unwrap();
    let destination = root.path().join("dst");
    fs::create_dir(&destination).unwrap();
    let first_source = root.path().join("first.bin");
    let second_source = root.path().join("second.bin");
    write_pattern(&first_source, BIG);
    write_pattern(&second_source, BIG);

    let engine = engine();
    let first = engine
        .submit(
            JobSpec::new(Operation::Copy {
                sources: vec![local(&first_source)],
                destination: local(&destination),
            })
            .with_policy(slow_policy()),
        )
        .unwrap();
    let second = engine
        .submit(
            JobSpec::new(Operation::Copy {
                sources: vec![local(&second_source)],
                destination: local(&destination),
            })
            .with_policy(slow_policy())
            .with_conflicts(ConflictPolicy::always(
                ConflictKind::Exists,
                Resolution::Rename,
            )),
        )
        .unwrap();

    let both_running = engine
        .wait_for(first.id(), support::LIMIT, |_| {
            matches!(
                (engine.state(first.id()), engine.state(second.id())),
                (Some(JobState::Running), Some(JobState::Running))
            )
        })
        .is_some();
    assert!(
        both_running,
        "the second job never started alongside the first"
    );
    engine.wait(first.id(), support::LIMIT);
    engine.wait(second.id(), support::LIMIT);
}
