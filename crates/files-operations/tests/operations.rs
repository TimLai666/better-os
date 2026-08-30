//! Every operation, run as a real job against a real temporary directory.
//!
//! Ticket 33's acceptance list is "create, rename, copy, move, duplicate,
//! trash, restore, and permanent delete all run as jobs". This file is that
//! list, plus the two Issue #6 operations the engine also carries: bulk rename
//! and checksum.

mod support;

use std::ffi::OsString;
use std::fs;

use files_operations::{
    ChecksumAlgorithm, ConflictKind, ConflictPolicy, DeleteConfirmation, DeleteTarget, JobSpec,
    JobState, Operation, RenamePattern, Resolution, TrashItemRef,
};

use support::{engine, entries, local, run_ok, write_pattern};

#[test]
fn create_file_and_create_folder_each_run_as_a_job() {
    let root = tempfile::tempdir().unwrap();
    let engine = engine();

    let snapshot = run_ok(
        &engine,
        JobSpec::new(Operation::CreateFolder {
            parent: local(root.path()),
            name: OsString::from("Projects"),
        }),
    );
    assert_eq!(snapshot.progress.items_done, 1);
    assert!(root.path().join("Projects").is_dir());

    run_ok(
        &engine,
        JobSpec::new(Operation::CreateFile {
            parent: local(root.path().join("Projects")),
            name: OsString::from("notes.txt"),
        }),
    );
    assert!(root.path().join("Projects/notes.txt").is_file());
}

#[test]
fn creating_something_that_is_already_there_fails_with_the_named_error() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("taken"), b"x").unwrap();
    let engine = engine();
    let snapshot = support::run(
        &engine,
        JobSpec::new(Operation::CreateFile {
            parent: local(root.path()),
            name: OsString::from("taken"),
        }),
    );
    assert_eq!(snapshot.state, JobState::Failed);
    assert_eq!(
        snapshot.failures[0].1.key(),
        "files.operation.error.already_exists"
    );
    // The original content is untouched.
    assert_eq!(fs::read(root.path().join("taken")).unwrap(), b"x");
}

#[test]
fn rename_moves_the_name_and_leaves_the_content() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("old.txt"), b"content").unwrap();
    let engine = engine();
    run_ok(
        &engine,
        JobSpec::new(Operation::Rename {
            path: local(root.path().join("old.txt")),
            new_name: OsString::from("new.txt"),
        }),
    );
    assert!(!root.path().join("old.txt").exists());
    assert_eq!(fs::read(root.path().join("new.txt")).unwrap(), b"content");
}

#[test]
fn bulk_rename_numbers_a_selection_in_order() {
    let root = tempfile::tempdir().unwrap();
    for name in ["IMG_0001.jpg", "IMG_0002.jpg", "IMG_0003.jpg"] {
        fs::write(root.path().join(name), b"x").unwrap();
    }
    let targets = vec![
        local(root.path().join("IMG_0001.jpg")),
        local(root.path().join("IMG_0002.jpg")),
        local(root.path().join("IMG_0003.jpg")),
    ];
    let engine = engine();
    run_ok(
        &engine,
        JobSpec::new(Operation::BulkRename {
            targets,
            pattern: RenamePattern::numbering("Holiday-{n}{ext}", 1, 2),
        }),
    );
    let names: Vec<String> = entries(root.path())
        .into_iter()
        .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        names,
        vec!["Holiday-01.jpg", "Holiday-02.jpg", "Holiday-03.jpg"]
    );
}

#[test]
fn copy_carries_a_whole_tree_and_reports_bytes_and_items() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    fs::create_dir_all(source.join("nested")).unwrap();
    write_pattern(&source.join("a.bin"), 4096);
    write_pattern(&source.join("nested/b.bin"), 1024);
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
    // Two directories and two files.
    assert_eq!(snapshot.progress.items_total, 4);
    assert_eq!(snapshot.progress.items_done, 4);
    assert_eq!(snapshot.progress.bytes_total, 5120);
    assert_eq!(snapshot.progress.bytes_done, 5120);
    assert_eq!(
        support::read(&destination.join("source/a.bin")),
        support::read(&source.join("a.bin"))
    );
    assert_eq!(
        support::read(&destination.join("source/nested/b.bin")),
        support::read(&source.join("nested/b.bin"))
    );
    // The source is still there: a copy is not a move.
    assert!(source.join("a.bin").exists());
}

#[test]
fn move_within_one_filesystem_takes_the_rename_fast_path() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    fs::create_dir(&source).unwrap();
    write_pattern(&source.join("big.bin"), 64 * 1024);
    let destination = root.path().join("destination");
    fs::create_dir(&destination).unwrap();

    let engine = engine();
    let snapshot = run_ok(
        &engine,
        JobSpec::new(Operation::Move {
            sources: vec![local(source.join("big.bin"))],
            destination: local(&destination),
        }),
    );
    assert!(!source.join("big.bin").exists());
    assert_eq!(support::read(&destination.join("big.bin")).len(), 64 * 1024);
    // A rename moves no bytes, and the log says which path it took.
    assert_eq!(snapshot.progress.bytes_done, 0);
    let took_fast_path = snapshot
        .log
        .records()
        .iter()
        .any(|record| matches!(record.event, files_operations::LogEvent::RenameFastPath));
    assert!(took_fast_path, "expected the rename fast path");
}

#[test]
fn duplicate_puts_a_numbered_copy_beside_the_original() {
    let root = tempfile::tempdir().unwrap();
    write_pattern(&root.path().join("report.txt"), 32);
    let engine = engine();
    run_ok(
        &engine,
        JobSpec::new(Operation::Duplicate {
            sources: vec![local(root.path().join("report.txt"))],
        }),
    );
    assert!(root.path().join("report.txt").exists());
    assert!(root.path().join("report (copy).txt").exists());
    assert_eq!(
        support::read(&root.path().join("report (copy).txt")),
        support::read(&root.path().join("report.txt"))
    );
}

#[test]
fn trash_restore_and_permanent_delete_are_three_jobs_over_one_item() {
    let root = tempfile::tempdir().unwrap();
    let trash_root = root.path().join("Trash");
    let file = root.path().join("notes.txt");
    fs::write(&file, b"keep me").unwrap();

    let engine = engine();
    run_ok(
        &engine,
        JobSpec::new(Operation::Trash {
            sources: vec![local(&file)],
            trash_root: Some(trash_root.clone()),
        }),
    );
    assert!(!file.exists());
    assert!(trash_root.join("files/notes.txt").exists());
    assert!(trash_root.join("info/notes.txt.trashinfo").exists());

    run_ok(
        &engine,
        JobSpec::new(Operation::RestoreFromTrash {
            items: vec![TrashItemRef::new(&trash_root, "notes.txt")],
        }),
    );
    assert_eq!(fs::read(&file).unwrap(), b"keep me");
    assert!(!trash_root.join("files/notes.txt").exists());
    assert!(!trash_root.join("info/notes.txt.trashinfo").exists());

    run_ok(
        &engine,
        JobSpec::new(Operation::PermanentDelete {
            targets: vec![DeleteTarget::Path(local(&file))],
            confirmation: DeleteConfirmation::explicit(),
        }),
    );
    assert!(!file.exists());
}

#[test]
fn permanent_delete_removes_a_whole_tree_children_first() {
    let root = tempfile::tempdir().unwrap();
    let tree = root.path().join("tree");
    fs::create_dir_all(tree.join("a/b")).unwrap();
    fs::write(tree.join("a/b/leaf.txt"), b"x").unwrap();
    fs::write(tree.join("a/other.txt"), b"y").unwrap();

    let engine = engine();
    let snapshot = run_ok(
        &engine,
        JobSpec::new(Operation::PermanentDelete {
            targets: vec![DeleteTarget::Path(local(&tree))],
            confirmation: DeleteConfirmation::explicit(),
        }),
    );
    assert!(!tree.exists());
    // Two files and three directories.
    assert_eq!(snapshot.progress.items_done, 5);
}

#[test]
fn emptying_one_item_from_the_trash_takes_its_record_with_it() {
    let root = tempfile::tempdir().unwrap();
    let trash_root = root.path().join("Trash");
    let file = root.path().join("gone.txt");
    fs::write(&file, b"x").unwrap();
    let engine = engine();
    run_ok(
        &engine,
        JobSpec::new(Operation::Trash {
            sources: vec![local(&file)],
            trash_root: Some(trash_root.clone()),
        }),
    );
    run_ok(
        &engine,
        JobSpec::new(Operation::PermanentDelete {
            targets: vec![DeleteTarget::TrashItem(TrashItemRef::new(
                &trash_root,
                "gone.txt",
            ))],
            confirmation: DeleteConfirmation::explicit(),
        }),
    );
    assert!(!trash_root.join("files/gone.txt").exists());
    assert!(!trash_root.join("info/gone.txt.trashinfo").exists());
}

#[test]
fn a_checksum_job_reports_the_digest_of_every_target() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("abc"), b"abc").unwrap();
    fs::write(root.path().join("empty"), b"").unwrap();

    let engine = engine();
    let snapshot = run_ok(
        &engine,
        JobSpec::new(Operation::Checksum {
            targets: vec![
                local(root.path().join("abc")),
                local(root.path().join("empty")),
            ],
            algorithm: ChecksumAlgorithm::Sha256,
        }),
    );
    let digests: Vec<&str> = snapshot
        .checksums
        .iter()
        .map(|(_, digest)| digest.as_str())
        .collect();
    assert_eq!(
        digests,
        vec![
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
        ]
    );
}

#[test]
fn duplicating_a_directory_carries_its_whole_contents() {
    let root = tempfile::tempdir().unwrap();
    let tree = root.path().join("album");
    fs::create_dir_all(tree.join("2024")).unwrap();
    write_pattern(&tree.join("2024/one.jpg"), 512);
    let engine = engine();
    run_ok(
        &engine,
        JobSpec::new(Operation::Duplicate {
            sources: vec![local(&tree)],
        })
        .with_conflicts(ConflictPolicy::always(
            ConflictKind::Exists,
            Resolution::Rename,
        )),
    );
    let copy = root.path().join("album (copy)");
    assert!(copy.is_dir());
    assert_eq!(
        support::read(&copy.join("2024/one.jpg")),
        support::read(&tree.join("2024/one.jpg"))
    );
}
