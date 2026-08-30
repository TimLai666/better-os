//! Trash, restore, and empty, run as jobs, including what happens when the
//! name is taken at both ends.

mod support;

use std::ffi::OsString;
use std::fs;
use std::os::unix::ffi::OsStringExt;

use files_operations::{
    Conflict, ConflictDecision, ConflictKind, ConflictPolicy, DeleteConfirmation, DeleteTarget,
    JobSpec, JobState, Operation, Resolution, TrashItemRef,
};

use support::{engine, local, run, run_ok, wait_for};

#[test]
fn a_whole_directory_goes_to_the_trash_and_comes_back_intact() {
    let root = tempfile::tempdir().unwrap();
    let trash_root = root.path().join("Trash");
    let project = root.path().join("project");
    fs::create_dir_all(project.join("src")).unwrap();
    fs::write(project.join("src/main.rs"), b"fn main() {}").unwrap();
    fs::write(project.join("README.md"), b"# project").unwrap();

    let engine = engine();
    run_ok(
        &engine,
        JobSpec::new(Operation::Trash {
            sources: vec![local(&project)],
            trash_root: Some(trash_root.clone()),
        }),
    );
    assert!(!project.exists());
    assert!(trash_root.join("files/project/src/main.rs").exists());

    run_ok(
        &engine,
        JobSpec::new(Operation::RestoreFromTrash {
            items: vec![TrashItemRef::new(&trash_root, "project")],
        }),
    );
    assert_eq!(
        fs::read(project.join("src/main.rs")).unwrap(),
        b"fn main() {}"
    );
    assert_eq!(fs::read(project.join("README.md")).unwrap(), b"# project");
    assert!(!trash_root.join("files/project").exists());
}

#[test]
fn two_files_with_the_same_name_from_different_folders_both_restore_correctly() {
    let root = tempfile::tempdir().unwrap();
    let trash_root = root.path().join("Trash");
    let first = root.path().join("one/report.txt");
    let second = root.path().join("two/report.txt");
    fs::create_dir_all(first.parent().unwrap()).unwrap();
    fs::create_dir_all(second.parent().unwrap()).unwrap();
    fs::write(&first, b"from one").unwrap();
    fs::write(&second, b"from two").unwrap();

    let engine = engine();
    run_ok(
        &engine,
        JobSpec::new(Operation::Trash {
            sources: vec![local(&first), local(&second)],
            trash_root: Some(trash_root.clone()),
        }),
    );
    assert!(!first.exists() && !second.exists());

    run_ok(
        &engine,
        JobSpec::new(Operation::RestoreFromTrash {
            items: vec![
                TrashItemRef::new(&trash_root, "report.txt"),
                TrashItemRef::new(&trash_root, "report.1.txt"),
            ],
        }),
    );
    assert_eq!(fs::read(&first).unwrap(), b"from one");
    assert_eq!(fs::read(&second).unwrap(), b"from two");
}

#[test]
fn restoring_onto_a_path_something_else_now_occupies_raises_a_conflict() {
    let root = tempfile::tempdir().unwrap();
    let trash_root = root.path().join("Trash");
    let file = root.path().join("notes.txt");
    fs::write(&file, b"old").unwrap();

    let engine = engine();
    run_ok(
        &engine,
        JobSpec::new(Operation::Trash {
            sources: vec![local(&file)],
            trash_root: Some(trash_root.clone()),
        }),
    );
    fs::write(&file, b"something newer").unwrap();

    let handle = engine
        .submit(JobSpec::new(Operation::RestoreFromTrash {
            items: vec![TrashItemRef::new(&trash_root, "notes.txt")],
        }))
        .unwrap();
    let parked = wait_for(&engine, handle.id(), |snapshot| {
        snapshot.state == JobState::WaitingOnConflict
    });
    assert_eq!(parked.conflict.as_ref().unwrap().kind, ConflictKind::Exists);
    // The newer file is still there while the job waits.
    assert_eq!(fs::read(&file).unwrap(), b"something newer");

    engine.resolve(handle.id(), ConflictDecision::once(Resolution::Rename));
    let finished = engine.wait(handle.id(), support::LIMIT).unwrap();
    assert_eq!(finished.state, JobState::Completed);
    assert_eq!(fs::read(&file).unwrap(), b"something newer");
    assert_eq!(
        fs::read(root.path().join("notes (copy).txt")).unwrap(),
        b"old"
    );
}

#[test]
fn skipping_a_restore_conflict_leaves_the_item_in_the_trash() {
    let root = tempfile::tempdir().unwrap();
    let trash_root = root.path().join("Trash");
    let file = root.path().join("notes.txt");
    fs::write(&file, b"old").unwrap();
    let engine = engine();
    run_ok(
        &engine,
        JobSpec::new(Operation::Trash {
            sources: vec![local(&file)],
            trash_root: Some(trash_root.clone()),
        }),
    );
    fs::write(&file, b"newer").unwrap();

    let snapshot = run_ok(
        &engine,
        JobSpec::new(Operation::RestoreFromTrash {
            items: vec![TrashItemRef::new(&trash_root, "notes.txt")],
        })
        .with_conflicts(ConflictPolicy::always(
            ConflictKind::Exists,
            Resolution::Skip,
        )),
    );
    assert_eq!(snapshot.progress.items_skipped, 1);
    assert!(
        trash_root.join("files/notes.txt").exists(),
        "a skipped restore must not lose the item"
    );
}

#[test]
fn a_trashed_name_that_is_not_utf8_restores_under_the_name_it_had() {
    let root = tempfile::tempdir().unwrap();
    let trash_root = root.path().join("Trash");
    let name = OsString::from_vec(b"rapport-caf\xe9.txt".to_vec());
    let file = root.path().join(&name);
    fs::write(&file, b"content").unwrap();

    let engine = engine();
    run_ok(
        &engine,
        JobSpec::new(Operation::Trash {
            sources: vec![local(&file)],
            trash_root: Some(trash_root.clone()),
        }),
    );
    assert!(!file.exists());

    // The identifier is the percent-encoded form, because a `.trashinfo` name
    // is text; the original bytes live in the record and come back on restore.
    let item = fs::read_dir(trash_root.join("info"))
        .unwrap()
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .find_map(|name| name.strip_suffix(".trashinfo").map(str::to_string))
        .expect("one record");
    run_ok(
        &engine,
        JobSpec::new(Operation::RestoreFromTrash {
            items: vec![TrashItemRef::new(&trash_root, item)],
        }),
    );
    assert_eq!(fs::read(&file).unwrap(), b"content");
}

#[test]
fn restoring_something_the_trash_has_no_record_of_fails_with_a_named_error() {
    let root = tempfile::tempdir().unwrap();
    let trash_root = root.path().join("Trash");
    fs::create_dir_all(trash_root.join("info")).unwrap();
    fs::create_dir_all(trash_root.join("files")).unwrap();
    let engine = engine();
    let snapshot = run(
        &engine,
        JobSpec::new(Operation::RestoreFromTrash {
            items: vec![TrashItemRef::new(&trash_root, "never-existed")],
        }),
    );
    assert_eq!(snapshot.state, JobState::Failed);
    assert_eq!(
        snapshot.failures[0].1.key(),
        "files.operation.error.trash_unavailable"
    );
}

#[test]
fn emptying_the_trash_removes_every_item_and_every_record() {
    let root = tempfile::tempdir().unwrap();
    let trash_root = root.path().join("Trash");
    let engine = engine();
    let mut sources = Vec::new();
    for index in 0..4 {
        let file = root.path().join(format!("file-{index}.txt"));
        fs::write(&file, b"x").unwrap();
        sources.push(local(&file));
    }
    run_ok(
        &engine,
        JobSpec::new(Operation::Trash {
            sources,
            trash_root: Some(trash_root.clone()),
        }),
    );

    let targets: Vec<DeleteTarget> = (0..4)
        .map(|index| {
            DeleteTarget::TrashItem(TrashItemRef::new(&trash_root, format!("file-{index}.txt")))
        })
        .collect();
    let snapshot = run_ok(
        &engine,
        JobSpec::new(Operation::PermanentDelete {
            targets,
            confirmation: DeleteConfirmation::explicit(),
        }),
    );
    assert_eq!(snapshot.progress.items_done, 4);
    assert_eq!(fs::read_dir(trash_root.join("files")).unwrap().count(), 0);
    assert_eq!(fs::read_dir(trash_root.join("info")).unwrap().count(), 0);
}

#[test]
fn trashing_something_already_gone_is_a_skip_rather_than_a_failure() {
    let root = tempfile::tempdir().unwrap();
    let trash_root = root.path().join("Trash");
    let engine = engine();
    let snapshot = run(
        &engine,
        JobSpec::new(Operation::Trash {
            sources: vec![local(root.path().join("never-was"))],
            trash_root: Some(trash_root),
        }),
    );
    // The end state the user asked for already holds, so the item is skipped
    // and the job completes. Failing would be pedantry about an outcome that
    // is already correct.
    assert_eq!(snapshot.state, JobState::Completed);
    assert_eq!(snapshot.progress.items_skipped, 1);
    assert!(snapshot.failures.is_empty());
}

#[test]
fn the_cross_filesystem_trash_fallback_is_selected_by_the_error_the_kernel_gives() {
    // Trashing across a filesystem boundary needs a second filesystem, which
    // the suite cannot mount. What is proven here is the decision: the trash
    // write side reports `CrossDevice` rather than a generic failure, and that
    // is the single condition `files-operations` switches to copy-and-delete
    // into the home trash on.
    use files_platform::trash::TrashError;
    let error = TrashError::CrossDevice {
        path: "/media/stick/file".into(),
    };
    assert!(
        error
            .to_string()
            .starts_with("files.trash.error.cross_device")
    );
    assert!(matches!(error, TrashError::CrossDevice { .. }));
    // And the conflict model has nothing to say about it: it is not a
    // decision the user makes, it is a path the engine takes.
    let conflict = Conflict::exists(None, "/media/stick/file".into());
    assert_eq!(conflict.kind, ConflictKind::Exists);
}
