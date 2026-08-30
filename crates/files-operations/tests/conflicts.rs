//! Conflicts, the decision that answers the rest of them, and retry.

mod support;

use std::fs;
use std::time::Duration;

use files_operations::{
    ConflictDecision, ConflictKind, ConflictPolicy, JobSpec, JobState, Operation, Resolution,
};

use support::{engine, local, run, run_ok, wait_for, write_pattern};

/// A source directory with three files that all clash with the destination.
fn clashing_pair(root: &std::path::Path) -> (std::path::PathBuf, std::path::PathBuf) {
    let source = root.join("source");
    let destination = root.join("destination");
    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(destination.join("source")).unwrap();
    for name in ["one.txt", "two.txt", "three.txt"] {
        fs::write(source.join(name), b"new content").unwrap();
        fs::write(destination.join("source").join(name), b"old").unwrap();
    }
    (source, destination)
}

#[test]
fn a_job_parks_on_the_first_conflict_and_says_what_it_is() {
    let root = tempfile::tempdir().unwrap();
    let (source, destination) = clashing_pair(root.path());
    let engine = engine();
    let handle = engine
        .submit(JobSpec::new(Operation::Copy {
            sources: vec![local(&source)],
            destination: local(&destination),
        }))
        .unwrap();

    let parked = wait_for(&engine, handle.id(), |snapshot| {
        snapshot.state == JobState::WaitingOnConflict
    });
    let conflict = parked.conflict.expect("a conflict to answer");
    assert_eq!(conflict.kind, ConflictKind::Exists);
    assert_eq!(conflict.destination, destination.join("source/one.txt"));
    // Nothing was overwritten while it waited.
    assert_eq!(
        fs::read(destination.join("source/one.txt")).unwrap(),
        b"old"
    );

    engine.resolve(handle.id(), ConflictDecision::once(Resolution::Skip));
    let next =
        wait_for(&engine, handle.id(), |snapshot| {
            snapshot.conflict.as_ref().is_some_and(|conflict| {
                conflict.destination == destination.join("source/three.txt")
            }) || snapshot.state.is_terminal()
        });
    // Answering one conflict does not answer the next: the job asks again.
    assert_eq!(next.state, JobState::WaitingOnConflict);
    engine.cancel(handle.id());
}

#[test]
fn one_decision_can_answer_every_remaining_conflict() {
    let root = tempfile::tempdir().unwrap();
    let (source, destination) = clashing_pair(root.path());
    let engine = engine();
    let handle = engine
        .submit(JobSpec::new(Operation::Copy {
            sources: vec![local(&source)],
            destination: local(&destination),
        }))
        .unwrap();

    wait_for(&engine, handle.id(), |snapshot| {
        snapshot.state == JobState::WaitingOnConflict
    });
    assert!(engine.resolve(
        handle.id(),
        ConflictDecision::for_remaining(Resolution::Overwrite)
    ));

    let finished = engine
        .wait(handle.id(), support::LIMIT)
        .expect("the job finished");
    assert_eq!(finished.state, JobState::Completed);
    assert_eq!(finished.progress.items_done, 4);
    for name in ["one.txt", "two.txt", "three.txt"] {
        assert_eq!(
            fs::read(destination.join("source").join(name)).unwrap(),
            b"new content",
            "{name} was not overwritten"
        );
    }
    // The job asked exactly once.
    let asked = finished
        .log
        .records()
        .iter()
        .filter(|record| {
            matches!(
                record.event,
                files_operations::LogEvent::ConflictRaised { .. }
            )
        })
        .count();
    assert_eq!(asked, 1);
}

#[test]
fn skip_applied_to_the_remaining_conflicts_leaves_every_destination_alone() {
    let root = tempfile::tempdir().unwrap();
    let (source, destination) = clashing_pair(root.path());
    let engine = engine();
    let snapshot = run_ok(
        &engine,
        JobSpec::new(Operation::Copy {
            sources: vec![local(&source)],
            destination: local(&destination),
        })
        .with_conflicts(ConflictPolicy::always(
            ConflictKind::Exists,
            Resolution::Skip,
        )),
    );
    assert_eq!(snapshot.progress.items_skipped, 3);
    // A skipped item is not a failure: the job completed.
    assert_eq!(snapshot.state, JobState::Completed);
    for name in ["one.txt", "two.txt", "three.txt"] {
        assert_eq!(
            fs::read(destination.join("source").join(name)).unwrap(),
            b"old"
        );
    }
}

#[test]
fn rename_writes_beside_the_file_that_is_already_there() {
    let root = tempfile::tempdir().unwrap();
    let (source, destination) = clashing_pair(root.path());
    let engine = engine();
    run_ok(
        &engine,
        JobSpec::new(Operation::Copy {
            sources: vec![local(&source)],
            destination: local(&destination),
        })
        .with_conflicts(ConflictPolicy::always(
            ConflictKind::Exists,
            Resolution::Rename,
        )),
    );
    assert_eq!(
        fs::read(destination.join("source/one.txt")).unwrap(),
        b"old"
    );
    assert_eq!(
        fs::read(destination.join("source/one (copy).txt")).unwrap(),
        b"new content"
    );
}

#[test]
fn answering_cancel_to_a_conflict_stops_the_job() {
    let root = tempfile::tempdir().unwrap();
    let (source, destination) = clashing_pair(root.path());
    let engine = engine();
    let snapshot = run(
        &engine,
        JobSpec::new(Operation::Copy {
            sources: vec![local(&source)],
            destination: local(&destination),
        })
        .with_conflicts(ConflictPolicy::always(
            ConflictKind::Exists,
            Resolution::Cancel,
        )),
    );
    assert_eq!(snapshot.state, JobState::Cancelled);
}

#[test]
fn a_failed_item_can_be_retried_on_its_own_once_the_cause_is_gone() {
    if support::running_as_root() {
        // Root ignores the mode bits, so there is nothing to fail.
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    fs::create_dir(&source).unwrap();
    write_pattern(&source.join("readable.bin"), 128);
    write_pattern(&source.join("locked.bin"), 128);
    fs::set_permissions(
        source.join("locked.bin"),
        std::os::unix::fs::PermissionsExt::from_mode(0o000),
    )
    .unwrap();
    let destination = root.path().join("destination");
    fs::create_dir(&destination).unwrap();

    let engine = engine();
    let handle = engine
        .submit(JobSpec::new(Operation::Copy {
            sources: vec![local(&source)],
            destination: local(&destination),
        }))
        .unwrap();
    let failed = engine.wait(handle.id(), support::LIMIT).unwrap();
    assert_eq!(failed.state, JobState::Failed);
    assert_eq!(failed.progress.items_failed, 1);
    assert_eq!(
        failed.failures[0].1.key(),
        "files.operation.error.permission_denied"
    );
    assert!(destination.join("source/readable.bin").exists());

    // Fix the cause, retry only what failed.
    fs::set_permissions(
        source.join("locked.bin"),
        std::os::unix::fs::PermissionsExt::from_mode(0o644),
    )
    .unwrap();
    assert!(engine.retry_failed(handle.id()));
    let retried = engine.wait(handle.id(), support::LIMIT).unwrap();
    assert_eq!(retried.state, JobState::Completed);
    assert_eq!(retried.progress.items_failed, 0);
    assert!(destination.join("source/locked.bin").exists());
}

#[test]
fn retry_is_refused_when_there_is_nothing_to_retry() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("a"), b"x").unwrap();
    let destination = root.path().join("dst");
    fs::create_dir(&destination).unwrap();
    let engine = engine();
    let handle = engine
        .submit(JobSpec::new(Operation::Copy {
            sources: vec![local(root.path().join("a"))],
            destination: local(&destination),
        }))
        .unwrap();
    engine.wait(handle.id(), support::LIMIT).unwrap();
    assert!(!engine.retry_failed(handle.id()));
}

#[test]
fn a_standing_answer_about_names_does_not_answer_a_different_kind_of_conflict() {
    // The policy is keyed by conflict kind, so a job told to overwrite existing
    // files still has to ask about anything else. Proven at the policy level,
    // because producing a real full disk in a test needs a filesystem the suite
    // may not create.
    let mut policy = ConflictPolicy::new();
    let existing = files_operations::Conflict::exists(None, "/dst/a".into());
    policy.apply(
        &existing,
        ConflictDecision::for_remaining(Resolution::Overwrite),
    );
    let no_space = files_operations::Conflict {
        kind: ConflictKind::NoSpace,
        source: None,
        destination: "/dst/b".into(),
        existing: None,
    };
    assert_eq!(policy.answer(&existing), Some(Resolution::Overwrite));
    assert_eq!(policy.answer(&no_space), None);
}

#[test]
fn a_conflict_the_engine_is_parked_on_is_released_by_cancelling() {
    let root = tempfile::tempdir().unwrap();
    let (source, destination) = clashing_pair(root.path());
    let engine = engine();
    let handle = engine
        .submit(JobSpec::new(Operation::Copy {
            sources: vec![local(&source)],
            destination: local(&destination),
        }))
        .unwrap();
    wait_for(&engine, handle.id(), |snapshot| {
        snapshot.state == JobState::WaitingOnConflict
    });
    assert!(engine.cancel(handle.id()));
    let snapshot = engine
        .wait(handle.id(), Duration::from_secs(5))
        .expect("the job stopped");
    assert_eq!(snapshot.state, JobState::Cancelled);
}
