//! The copy correctness policy, enforced rather than documented and hoped for.
//!
//! Every claim in `policy.rs`'s table that can be checked without privilege or
//! a second filesystem is checked here. The ones that cannot are named in the
//! test that gets closest to them, so the gap is on record instead of implied.

mod support;

use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::Path;

use files_operations::{
    CopyPolicy, JobSpec, JobState, MoveStrategy, Operation, SparsePolicy, SymlinkPolicy, fsops,
};

use support::{engine, local, run, run_ok, write_pattern};

fn set_times(path: &Path, seconds: i64, nanos: i64) {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let path_c = CString::new(path.as_os_str().as_bytes()).unwrap();
    let times = [
        libc::timespec {
            tv_sec: seconds,
            tv_nsec: nanos,
        },
        libc::timespec {
            tv_sec: seconds,
            tv_nsec: nanos,
        },
    ];
    let result = unsafe { libc::utimensat(libc::AT_FDCWD, path_c.as_ptr(), times.as_ptr(), 0) };
    assert_eq!(result, 0, "utimensat failed");
}

#[test]
fn a_copy_preserves_modification_time_permissions_and_the_executable_bit() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    fs::create_dir(&source).unwrap();
    let script = source.join("run.sh");
    fs::write(&script, b"#!/bin/sh\necho hi\n").unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o750)).unwrap();
    set_times(&script, 1_600_000_000, 987_654_321);
    let destination = root.path().join("destination");
    fs::create_dir(&destination).unwrap();

    let engine = engine();
    run_ok(
        &engine,
        JobSpec::new(Operation::Copy {
            sources: vec![local(&source)],
            destination: local(&destination),
        }),
    );
    let copied = fs::metadata(destination.join("source/run.sh")).unwrap();
    assert_eq!(copied.permissions().mode() & 0o777, 0o750);
    assert_eq!(copied.mtime(), 1_600_000_000);
    assert_eq!(copied.mtime_nsec(), 987_654_321);
}

#[test]
fn a_copied_directory_keeps_its_own_timestamp_despite_being_filled_afterwards() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("album");
    fs::create_dir(&source).unwrap();
    fs::write(source.join("one.txt"), b"x").unwrap();
    set_times(&source, 1_500_000_000, 0);
    let destination = root.path().join("destination");
    fs::create_dir(&destination).unwrap();

    let engine = engine();
    run_ok(
        &engine,
        JobSpec::new(Operation::Copy {
            sources: vec![local(&source)],
            destination: local(&destination),
        }),
    );
    let copied = fs::metadata(destination.join("album")).unwrap();
    assert_eq!(
        copied.mtime(),
        1_500_000_000,
        "the directory's timestamp was overwritten by writing its children"
    );
}

#[test]
fn a_symlink_is_copied_as_a_link_and_its_target_is_not_duplicated() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    fs::create_dir(&source).unwrap();
    let target = root.path().join("target.bin");
    write_pattern(&target, 64 * 1024);
    std::os::unix::fs::symlink(&target, source.join("link")).unwrap();
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
    let copied = destination.join("source/link");
    assert!(
        fs::symlink_metadata(&copied)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(fs::read_link(&copied).unwrap(), target);
    // The link cost no bytes, which is what "not duplicated" means.
    assert_eq!(snapshot.progress.bytes_total, 0);
}

#[test]
fn following_links_instead_copies_the_target_when_the_policy_says_so() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    fs::create_dir(&source).unwrap();
    let target = root.path().join("target.bin");
    write_pattern(&target, 4096);
    std::os::unix::fs::symlink(&target, source.join("link")).unwrap();
    let destination = root.path().join("destination");
    fs::create_dir(&destination).unwrap();

    let engine = engine();
    let snapshot = run_ok(
        &engine,
        JobSpec::new(Operation::Copy {
            sources: vec![local(&source)],
            destination: local(&destination),
        })
        .with_policy(CopyPolicy {
            symlinks: SymlinkPolicy::FollowAndCopyTarget,
            ..CopyPolicy::default()
        }),
    );
    let copied = destination.join("source/link");
    assert!(
        !fs::symlink_metadata(&copied)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(snapshot.progress.bytes_total, 4096);
}

#[test]
fn a_sparse_file_keeps_its_holes_where_the_filesystem_supports_them() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("sparse.img");
    // 64 MiB of nothing with 4 KiB of data at each end. `set_len` after a short
    // write is how a hole is made without writing the zeroes.
    {
        use std::io::{Seek, SeekFrom, Write};
        let mut file = fs::File::create(&source).unwrap();
        file.write_all(&[1u8; 4096]).unwrap();
        file.seek(SeekFrom::Start(64 * 1024 * 1024 - 4096)).unwrap();
        file.write_all(&[2u8; 4096]).unwrap();
        file.sync_all().unwrap();
    }
    let apparent = fs::metadata(&source).unwrap().len();
    let source_blocks = fsops::allocated_blocks(&source).unwrap();
    assert_eq!(apparent, 64 * 1024 * 1024);
    if source_blocks * 512 >= apparent {
        // The temporary directory's filesystem does not do sparse files. That
        // is a real configuration, and the policy's fallback is a dense copy;
        // there is nothing here to assert about holes.
        eprintln!("filesystem is not sparse-capable; hole preservation not exercised");
        return;
    }

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
    let copied = destination.join("sparse.img");
    assert_eq!(fs::metadata(&copied).unwrap().len(), apparent);
    let copied_blocks = fsops::allocated_blocks(&copied).unwrap();
    assert!(
        copied_blocks * 512 < apparent / 2,
        "the copy allocated {} bytes for a {apparent}-byte sparse file",
        copied_blocks * 512
    );
    // The content still reads back correctly, holes included.
    let data = support::read(&copied);
    assert_eq!(&data[..4096], &[1u8; 4096]);
    assert!(
        data[4096..64 * 1024 * 1024 - 4096]
            .iter()
            .all(|byte| *byte == 0)
    );
    assert_eq!(&data[64 * 1024 * 1024 - 4096..], &[2u8; 4096]);
    // And the log says the holes were reproduced rather than merely implied.
    let reported = snapshot.log.records().iter().any(|record| {
        matches!(
            record.event,
            files_operations::LogEvent::SparseRegionsPreserved { .. }
        )
    });
    assert!(reported, "the log did not record the preserved holes");
}

#[test]
fn a_dense_policy_writes_every_byte_including_the_holes() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("sparse.img");
    {
        use std::io::Write;
        let mut file = fs::File::create(&source).unwrap();
        file.write_all(&[1u8; 4096]).unwrap();
        file.set_len(8 * 1024 * 1024).unwrap();
    }
    let destination = root.path().join("destination");
    fs::create_dir(&destination).unwrap();
    let engine = engine();
    run_ok(
        &engine,
        JobSpec::new(Operation::Copy {
            sources: vec![local(&source)],
            destination: local(&destination),
        })
        .with_policy(CopyPolicy {
            sparse: SparsePolicy::Dense,
            ..CopyPolicy::default()
        }),
    );
    let copied = destination.join("sparse.img");
    assert_eq!(fs::metadata(&copied).unwrap().len(), 8 * 1024 * 1024);
    assert_eq!(support::read(&copied).len(), 8 * 1024 * 1024);
}

#[test]
fn extended_attributes_cross_where_the_destination_takes_them() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("tagged.txt");
    fs::write(&source, b"content").unwrap();
    if fsops::write_xattr(&source, "user.better-os.origin", b"https://example.invalid").is_err() {
        eprintln!("this filesystem refuses user extended attributes; not exercised");
        return;
    }
    let destination = root.path().join("destination");
    fs::create_dir(&destination).unwrap();
    let engine = engine();
    run_ok(
        &engine,
        JobSpec::new(Operation::Copy {
            sources: vec![local(&source)],
            destination: local(&destination),
        }),
    );
    assert_eq!(
        fsops::read_xattr(&destination.join("tagged.txt"), "user.better-os.origin").as_deref(),
        Some(b"https://example.invalid".as_slice())
    );
}

#[test]
fn a_cross_filesystem_move_copies_verifies_and_only_then_deletes_the_source() {
    // Mounting a second filesystem needs privilege the test suite must not
    // ask for, so the copy-then-delete path is forced by policy instead. The
    // code under test is identical; what is simulated is the `EXDEV` that
    // would have selected it.
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    fs::create_dir_all(source.join("nested")).unwrap();
    write_pattern(&source.join("nested/data.bin"), 128 * 1024);
    set_times(&source.join("nested/data.bin"), 1_400_000_000, 111);
    let destination = root.path().join("destination");
    fs::create_dir(&destination).unwrap();

    let engine = engine();
    let snapshot = run_ok(
        &engine,
        JobSpec::new(Operation::Move {
            sources: vec![local(&source)],
            destination: local(&destination),
        })
        .with_policy(CopyPolicy {
            moves: MoveStrategy::AlwaysCopyThenDelete,
            ..CopyPolicy::default()
        }),
    );
    // The bytes really moved, unlike the rename fast path.
    assert_eq!(snapshot.progress.bytes_done, 128 * 1024);
    let moved = destination.join("source/nested/data.bin");
    assert_eq!(support::read(&moved).len(), 128 * 1024);
    assert_eq!(fs::metadata(&moved).unwrap().mtime(), 1_400_000_000);
    // The source, files and directories both, is gone.
    assert!(!source.exists());
    let fell_back = snapshot.log.records().iter().any(|record| {
        matches!(
            record.event,
            files_operations::LogEvent::CrossDeviceFallback
        )
    });
    assert!(
        fell_back,
        "the log did not record the copy-then-delete path"
    );
}

#[test]
fn a_source_rewritten_during_a_cross_filesystem_move_is_not_deleted() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("live.bin");
    // Large enough, against a 4 KiB chunk, that the pause lands well inside the
    // copy instead of racing its completion.
    write_pattern(&source, 64 * 1024 * 1024);
    let destination = root.path().join("destination");
    fs::create_dir(&destination).unwrap();

    let engine = engine();
    let handle = engine
        .submit(
            JobSpec::new(Operation::Move {
                sources: vec![local(&source)],
                destination: local(&destination),
            })
            .with_policy(CopyPolicy {
                moves: MoveStrategy::AlwaysCopyThenDelete,
                chunk_bytes: 4096,
                ..CopyPolicy::default()
            }),
        )
        .unwrap();
    // Rewrite the source while the copy is holding it. Pausing first makes the
    // race deterministic: the job is stopped between two chunks, the file is
    // rewritten under it, and the metadata re-check before the source delete is
    // what has to catch that.
    support::wait_for(&engine, handle.id(), |snapshot| {
        snapshot.progress.bytes_done > 0
    });
    assert!(engine.pause(handle.id()));
    support::wait_for(&engine, handle.id(), |snapshot| {
        snapshot.state == JobState::Paused
    });
    fs::write(&source, b"somebody else got here first").unwrap();
    assert!(engine.resume(handle.id()));

    let snapshot = engine.wait(handle.id(), support::LIMIT).unwrap();
    assert_eq!(snapshot.state, JobState::Failed);
    assert_eq!(
        snapshot.failures[0].1.key(),
        "files.operation.error.externally_modified"
    );
    // The other program's content survived.
    assert_eq!(fs::read(&source).unwrap(), b"somebody else got here first");
}

#[test]
fn verification_is_what_turns_a_bad_copy_into_a_failure() {
    // Verification compares size and modification time against the source.
    // Proven directly, because making a real copy produce the wrong size needs
    // a filesystem that lies.
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("a");
    let destination = root.path().join("b");
    fs::write(&source, b"12345").unwrap();
    fs::write(&destination, b"123").unwrap();
    let policy = CopyPolicy {
        preserve_timestamps: false,
        ..CopyPolicy::default()
    };
    let error = fsops::verify_copy(&source, &destination, &policy).unwrap_err();
    assert_eq!(error.key(), "files.operation.error.verification_failed");
}

#[test]
fn a_copy_to_a_removable_destination_uses_per_file_durability() {
    // The typed hook `storage-core` plugs into: naming the destination
    // removable forces `fsync` per file and per directory, and the copy still
    // produces the same bytes.
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("a.bin");
    write_pattern(&source, 32 * 1024);
    let destination = root.path().join("destination");
    fs::create_dir(&destination).unwrap();
    let policy =
        CopyPolicy::default().for_destination(files_operations::DestinationDurability::Removable);
    assert!(policy.wants_fsync());

    let engine = engine();
    let snapshot = run(
        &engine,
        JobSpec::new(Operation::Copy {
            sources: vec![local(&source)],
            destination: local(&destination),
        })
        .with_policy(policy),
    );
    assert_eq!(snapshot.state, JobState::Completed);
    assert_eq!(
        support::read(&destination.join("a.bin")),
        support::read(&source)
    );
}
