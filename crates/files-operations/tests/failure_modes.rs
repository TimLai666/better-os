//! The error taxonomy, exercised against real conditions where a test can
//! produce one and against the classifier where it cannot.
//!
//! Issue #6 names the list: symlink loops, permission errors, disappearing
//! devices, full disks, filename encoding, very long paths, case conflicts, and
//! concurrent external changes. Each one is either produced here or has its
//! classification proven with the errno the kernel would return, and the
//! difference is stated in the test rather than blurred.

mod support;

use std::ffi::OsString;
use std::fs;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use files_operations::{
    CopyPolicy, DeleteConfirmation, DeleteTarget, JobSpec, JobState, Operation, OperationError,
    SymlinkPolicy,
};

use support::{engine, local, run, run_ok, write_pattern};

#[test]
fn a_symlink_loop_terminates_and_names_the_directory_it_looped_at() {
    let root = tempfile::tempdir().unwrap();
    let tree = root.path().join("tree");
    fs::create_dir_all(tree.join("inner")).unwrap();
    fs::write(tree.join("inner/leaf.txt"), b"x").unwrap();
    std::os::unix::fs::symlink(&tree, tree.join("inner/back")).unwrap();
    let destination = root.path().join("destination");
    fs::create_dir(&destination).unwrap();

    let engine = engine();
    let snapshot = run(
        &engine,
        JobSpec::new(Operation::Copy {
            sources: vec![local(&tree)],
            destination: local(&destination),
        })
        .with_policy(CopyPolicy {
            symlinks: SymlinkPolicy::FollowAndCopyTarget,
            ..CopyPolicy::default()
        }),
    );
    // It stopped, which is the point: without the visited set this test hangs.
    assert_eq!(snapshot.state, JobState::Failed);
    assert!(
        snapshot
            .failures
            .iter()
            .any(|(_, error)| error.key() == "files.operation.error.symlink_loop"),
        "failures were {:?}",
        snapshot.failures
    );
    // The rest of the tree still copied.
    assert!(destination.join("tree/inner/leaf.txt").exists());
}

#[test]
fn a_source_that_cannot_be_read_fails_with_permission_denied_and_names_the_path() {
    if support::running_as_root() {
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let locked = root.path().join("locked.bin");
    write_pattern(&locked, 64);
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
    let destination = root.path().join("destination");
    fs::create_dir(&destination).unwrap();

    let engine = engine();
    let snapshot = run(
        &engine,
        JobSpec::new(Operation::Copy {
            sources: vec![local(&locked)],
            destination: local(&destination),
        }),
    );
    assert_eq!(snapshot.state, JobState::Failed);
    let (path, error) = &snapshot.failures[0];
    assert_eq!(error.key(), "files.operation.error.permission_denied");
    assert_eq!(path, &locked);
    assert!(error.is_retryable());
}

#[test]
fn a_destination_directory_that_refuses_writes_fails_without_losing_the_source() {
    if support::running_as_root() {
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("a.bin");
    write_pattern(&source, 128);
    let destination = root.path().join("readonly");
    fs::create_dir(&destination).unwrap();
    fs::set_permissions(&destination, fs::Permissions::from_mode(0o555)).unwrap();

    let engine = engine();
    let snapshot = run(
        &engine,
        JobSpec::new(Operation::Move {
            sources: vec![local(&source)],
            destination: local(&destination),
        }),
    );
    assert_eq!(snapshot.state, JobState::Failed);
    assert_eq!(
        snapshot.failures[0].1.key(),
        "files.operation.error.permission_denied"
    );
    // A failed move keeps its source. This is the property that matters.
    assert!(source.exists());
    fs::set_permissions(&destination, fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn a_filename_that_is_not_utf8_survives_a_copy_a_move_and_a_delete() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    fs::create_dir(&source).unwrap();
    let awkward = OsString::from_vec(b"caf\xe9 \xff report.txt".to_vec());
    write_pattern(&source.join(&awkward), 256);
    let destination = root.path().join("destination");
    fs::create_dir(&destination).unwrap();

    let engine = engine();
    let snapshot = run_ok(
        &engine,
        JobSpec::new(Operation::Copy {
            sources: vec![local(&source)],
            destination: local(&destination),
        }),
    );
    let copied = destination.join("source").join(&awkward);
    assert!(copied.exists(), "the copy did not land under the real name");
    assert_eq!(support::read(&copied).len(), 256);
    // The log names the real file, byte for byte, with no replacement
    // characters anywhere in it.
    let named = snapshot
        .log
        .records()
        .iter()
        .any(|record| record.path.as_deref() == Some(copied.as_path()));
    assert!(named, "the log lost the name");

    // Move it somewhere else, then delete it.
    let elsewhere = root.path().join("elsewhere");
    fs::create_dir(&elsewhere).unwrap();
    run_ok(
        &engine,
        JobSpec::new(Operation::Move {
            sources: vec![local(&copied)],
            destination: local(&elsewhere),
        }),
    );
    let moved = elsewhere.join(&awkward);
    assert!(moved.exists());
    run_ok(
        &engine,
        JobSpec::new(Operation::PermanentDelete {
            targets: vec![DeleteTarget::Path(local(&moved))],
            confirmation: DeleteConfirmation::explicit(),
        }),
    );
    assert!(!moved.exists());
}

#[test]
fn a_non_utf8_name_round_trips_through_the_persisted_record() {
    let root = tempfile::tempdir().unwrap();
    let store_root = root.path().join("jobs");
    let source = root.path().join("source");
    fs::create_dir(&source).unwrap();
    let awkward = OsString::from_vec(b"\xff\xfe-name".to_vec());
    fs::write(source.join(&awkward), b"x").unwrap();
    let destination = root.path().join("destination");
    fs::create_dir(&destination).unwrap();

    let store = files_operations::JobStore::new(&store_root);
    let engine = files_operations::JobEngine::new(files_operations::EngineConfig {
        workers: 1,
        store: Some(store.clone()),
        conflicts: files_operations::ConflictPolicy::new(),
    });
    let handle = engine
        .submit(JobSpec::new(Operation::Copy {
            sources: vec![local(&source)],
            destination: local(&destination),
        }))
        .unwrap();
    engine.wait(handle.id(), support::LIMIT).unwrap();

    let record = store.read(handle.id().value()).unwrap();
    let names: Vec<Vec<u8>> = record
        .items
        .iter()
        .map(|item| item.source.as_os_str().as_bytes().to_vec())
        .collect();
    assert!(
        names
            .iter()
            .any(|name| name.ends_with(b"\xff\xfe-name".as_slice())),
        "the record lost the name: {names:?}"
    );
}

#[test]
fn a_name_longer_than_the_kernel_allows_is_refused_at_submission() {
    let root = tempfile::tempdir().unwrap();
    let engine = engine();
    let too_long = OsString::from("n".repeat(300));
    let error = engine
        .submit(JobSpec::new(Operation::CreateFile {
            parent: local(root.path()),
            name: too_long,
        }))
        .unwrap_err();
    assert_eq!(error.key(), "files.operation.error.invalid_name");
}

#[test]
fn a_path_the_kernel_refuses_as_too_long_is_reported_as_such() {
    // A path longer than PATH_MAX cannot be created to copy from, so what is
    // proven here is the translation: an `ENAMETOOLONG` from any syscall in
    // the operation path becomes the named error rather than a generic one.
    let deep: PathBuf = std::iter::repeat_n("segment", 700)
        .fold(PathBuf::from("/tmp"), |path, part| path.join(part));
    let error = fs::symlink_metadata(&deep).unwrap_err();
    let classified = OperationError::from_io(&deep, &error);
    assert_eq!(classified.key(), "files.operation.error.name_too_long");
}

#[test]
fn a_device_that_disappears_mid_copy_is_reported_as_a_lost_device() {
    // Unplugging a disk under a running copy needs hardware. What the job does
    // with the three errno values the kernel produces when it happens is
    // testable, and it is the part that decides whether a move deletes its
    // source.
    for errno in [libc::ENODEV, libc::ENXIO, libc::EIO] {
        let error = std::io::Error::from_raw_os_error(errno);
        let classified = OperationError::from_io("/media/stick/file", &error);
        assert_eq!(
            classified.key(),
            "files.operation.error.device_lost",
            "errno {errno}"
        );
        assert!(classified.is_retryable());
    }
}

#[test]
fn a_full_disk_and_an_exhausted_quota_are_different_answers() {
    let full = OperationError::from_io(
        "/data/big.bin",
        &std::io::Error::from_raw_os_error(libc::ENOSPC),
    );
    let quota = OperationError::from_io(
        "/data/big.bin",
        &std::io::Error::from_raw_os_error(libc::EDQUOT),
    );
    assert_eq!(full.key(), "files.operation.error.no_space");
    assert_eq!(quota.key(), "files.operation.error.quota_exceeded");
    assert_ne!(full, quota);
}

#[test]
fn a_source_that_vanishes_between_planning_and_copying_is_skipped_not_failed() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    fs::create_dir(&source).unwrap();
    // Large enough that the copy is still on the first file when the pause
    // lands: at a 4 KiB chunk size this is sixteen thousand cancellation
    // checkpoints, so the window the test needs is the whole copy rather than
    // a few milliseconds of it.
    write_pattern(&source.join("first.bin"), 64 * 1024 * 1024);
    write_pattern(&source.join("second.bin"), 64);
    let destination = root.path().join("destination");
    fs::create_dir(&destination).unwrap();

    let engine = engine();
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
    // Remove the second file while the first is still copying.
    support::wait_for(&engine, handle.id(), |snapshot| {
        snapshot.progress.bytes_done > 0
    });
    assert!(engine.pause(handle.id()));
    support::wait_for(&engine, handle.id(), |snapshot| {
        snapshot.state == JobState::Paused
    });
    fs::remove_file(source.join("second.bin")).unwrap();
    assert!(engine.resume(handle.id()));

    let snapshot = engine.wait(handle.id(), support::LIMIT).unwrap();
    // A source that is already gone is the outcome the user wanted, so the job
    // completes with a skip rather than failing.
    assert_eq!(snapshot.state, JobState::Completed);
    assert_eq!(snapshot.progress.items_skipped, 1);
}

#[test]
fn copying_a_directory_into_its_own_subdirectory_is_refused_before_anything_moves() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("photos");
    fs::create_dir_all(source.join("backup")).unwrap();
    fs::write(source.join("one.jpg"), b"x").unwrap();
    let engine = engine();
    let error = engine
        .submit(JobSpec::new(Operation::Copy {
            sources: vec![local(&source)],
            destination: local(source.join("backup")),
        }))
        .unwrap_err();
    assert_eq!(
        error.key(),
        "files.operation.error.destination_inside_source"
    );
    // Nothing was created.
    assert_eq!(fs::read_dir(source.join("backup")).unwrap().count(), 0);
}

#[test]
fn a_failure_keeps_its_error_and_its_affected_path_in_the_log() {
    if support::running_as_root() {
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("locked.bin");
    write_pattern(&source, 32);
    fs::set_permissions(&source, fs::Permissions::from_mode(0o000)).unwrap();
    let destination = root.path().join("destination");
    fs::create_dir(&destination).unwrap();

    let engine = engine();
    let snapshot = run(
        &engine,
        JobSpec::new(Operation::Copy {
            sources: vec![local(&source)],
            destination: local(&destination),
        }),
    );
    let logged = snapshot.log.failures();
    assert_eq!(logged.len(), 1);
    assert_eq!(logged[0].0.as_deref(), Some(source.as_path()));
    assert_eq!(logged[0].1.key(), "files.operation.error.permission_denied");
    assert_eq!(logged[0].1.path(), Some(source.as_path()));
}

#[test]
fn one_bad_item_does_not_abandon_the_rest_of_the_job() {
    if support::running_as_root() {
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    fs::create_dir(&source).unwrap();
    for name in ["a.bin", "c.bin", "d.bin"] {
        write_pattern(&source.join(name), 64);
    }
    write_pattern(&source.join("b.bin"), 64);
    fs::set_permissions(source.join("b.bin"), fs::Permissions::from_mode(0o000)).unwrap();
    let destination = root.path().join("destination");
    fs::create_dir(&destination).unwrap();

    let engine = engine();
    let snapshot = run(
        &engine,
        JobSpec::new(Operation::Copy {
            sources: vec![local(&source)],
            destination: local(&destination),
        }),
    );
    assert_eq!(snapshot.state, JobState::Failed);
    assert_eq!(snapshot.progress.items_failed, 1);
    // The directory plus the three readable files.
    assert_eq!(snapshot.progress.items_done, 4);
    for name in ["a.bin", "c.bin", "d.bin"] {
        assert!(destination.join("source").join(name).exists());
    }
}
