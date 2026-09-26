//! Hard links inside one job: two paths that name one file stay one file.
//!
//! A backup tree made with `cp -al` or `rsync --link-dest` is mostly hard
//! links. Copying it as independent files multiplies its size by the number of
//! snapshots, which is the failure this file pins down.

mod support;

use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

use files_operations::{
    ConflictKind, ConflictPolicy, CopyPolicy, EngineConfig, JobEngine, JobSpec, JobState, JobStore,
    LogEvent, MoveStrategy, Operation, Resolution,
};

use support::{engine, local, run, run_ok, write_pattern};

fn inode(path: &Path) -> u64 {
    fs::metadata(path).unwrap().ino()
}

/// `tree/a.bin` and `tree/b/linked.bin` are one file; `tree/c.bin` is not.
fn linked_tree(root: &Path) -> std::path::PathBuf {
    let tree = root.join("tree");
    fs::create_dir_all(tree.join("b")).unwrap();
    write_pattern(&tree.join("a.bin"), 4096);
    fs::hard_link(tree.join("a.bin"), tree.join("b/linked.bin")).unwrap();
    write_pattern(&tree.join("c.bin"), 512);
    tree
}

fn hard_linked_events(snapshot: &files_operations::JobSnapshot) -> usize {
    snapshot
        .log
        .records()
        .iter()
        .filter(|record| matches!(record.event, LogEvent::HardLinked { .. }))
        .count()
}

#[test]
fn files_sharing_an_inode_are_copied_once_and_linked_afterwards() {
    let root = tempfile::tempdir().unwrap();
    let tree = linked_tree(root.path());
    let destination = root.path().join("out");
    fs::create_dir(&destination).unwrap();

    let snapshot = run_ok(
        &engine(),
        JobSpec::new(Operation::Copy {
            sources: vec![local(&tree)],
            destination: local(&destination),
        }),
    );
    let copied = destination.join("tree");
    assert_eq!(
        inode(&copied.join("a.bin")),
        inode(&copied.join("b/linked.bin"))
    );
    assert_ne!(inode(&copied.join("a.bin")), inode(&tree.join("a.bin")));
    assert_ne!(inode(&copied.join("c.bin")), inode(&copied.join("a.bin")));
    assert_eq!(fs::metadata(copied.join("a.bin")).unwrap().nlink(), 2);
    assert_eq!(
        fs::read(copied.join("b/linked.bin")).unwrap(),
        fs::read(tree.join("a.bin")).unwrap()
    );
    // The shared file's bytes are counted once: that is the work there is.
    assert_eq!(snapshot.progress.bytes_total, 4096 + 512);
    assert_eq!(hard_linked_events(&snapshot), 1);
}

#[test]
fn separate_sources_of_one_job_share_the_link_map() {
    let root = tempfile::tempdir().unwrap();
    let first = root.path().join("first.bin");
    let second = root.path().join("second.bin");
    write_pattern(&first, 1024);
    fs::hard_link(&first, &second).unwrap();
    let destination = root.path().join("out");
    fs::create_dir(&destination).unwrap();

    run_ok(
        &engine(),
        JobSpec::new(Operation::Copy {
            sources: vec![local(&first), local(&second)],
            destination: local(&destination),
        }),
    );
    assert_eq!(
        inode(&destination.join("first.bin")),
        inode(&destination.join("second.bin"))
    );
}

#[test]
fn a_cross_filesystem_move_keeps_links_the_way_a_copy_does() {
    let root = tempfile::tempdir().unwrap();
    let tree = linked_tree(root.path());
    let destination = root.path().join("out");
    fs::create_dir(&destination).unwrap();

    run_ok(
        &engine(),
        JobSpec::new(Operation::Move {
            sources: vec![local(&tree)],
            destination: local(&destination),
        })
        .with_policy(CopyPolicy {
            moves: MoveStrategy::AlwaysCopyThenDelete,
            ..CopyPolicy::default()
        }),
    );
    assert!(!tree.exists(), "the moved tree is gone from its source");
    let moved = destination.join("tree");
    assert_eq!(
        inode(&moved.join("a.bin")),
        inode(&moved.join("b/linked.bin"))
    );
    assert_eq!(fs::metadata(moved.join("a.bin")).unwrap().nlink(), 2);
}

#[test]
fn a_move_within_one_filesystem_is_a_rename_and_links_are_untouched() {
    let root = tempfile::tempdir().unwrap();
    let tree = linked_tree(root.path());
    let original = inode(&tree.join("a.bin"));
    let destination = root.path().join("out");
    fs::create_dir(&destination).unwrap();

    let snapshot = run_ok(
        &engine(),
        JobSpec::new(Operation::Move {
            sources: vec![local(&tree)],
            destination: local(&destination),
        }),
    );
    let moved = destination.join("tree");
    assert_eq!(inode(&moved.join("a.bin")), original);
    assert_eq!(inode(&moved.join("b/linked.bin")), original);
    assert_eq!(hard_linked_events(&snapshot), 0, "a rename links nothing");
}

#[test]
fn a_retry_does_not_link_to_a_first_copy_that_has_changed_since() {
    let root = tempfile::tempdir().unwrap();
    let tree = linked_tree(root.path());
    let destination = root.path().join("out");
    // A directory where the second path's copy goes, so that item fails and
    // is left for a retry while the first path's copy completes.
    fs::create_dir_all(destination.join("tree/b/linked.bin")).unwrap();

    let engine = engine();
    let handle = engine
        .submit(
            JobSpec::new(Operation::Copy {
                sources: vec![local(&tree)],
                destination: local(&destination),
            })
            .with_conflicts(ConflictPolicy::always(
                ConflictKind::Exists,
                Resolution::Overwrite,
            )),
        )
        .unwrap();
    let first_run = engine.wait(handle.id(), support::LIMIT).unwrap();
    assert_eq!(first_run.state, JobState::Failed);
    let copied = destination.join("tree/a.bin");
    assert!(copied.is_file());

    // Between the runs, something rewrites the first copy and clears the way.
    fs::remove_dir(destination.join("tree/b/linked.bin")).unwrap();
    fs::write(&copied, b"somebody else's edit").unwrap();

    assert!(engine.retry_failed(handle.id()));
    let second_run = engine.wait(handle.id(), support::LIMIT).unwrap();
    assert_eq!(
        second_run.state,
        JobState::Completed,
        "{:?}",
        second_run.failures
    );
    let second = destination.join("tree/b/linked.bin");
    assert_ne!(inode(&second), inode(&copied));
    assert_eq!(
        fs::read(&second).unwrap(),
        fs::read(tree.join("a.bin")).unwrap()
    );
    assert!(
        second_run.log.records().iter().any(|record| matches!(
            &record.event,
            LogEvent::HardLinkNotPreserved { reason }
                if reason == "files.operation.hard_link.first_copy_changed"
        )),
        "the job says why it copied instead"
    );
}

#[test]
fn a_job_resubmitted_after_a_restart_does_not_link_to_what_it_did_not_copy() {
    let root = tempfile::tempdir().unwrap();
    let tree = linked_tree(root.path());
    let destination = root.path().join("out");
    fs::create_dir_all(destination.join("tree")).unwrap();
    // What an earlier process left behind: the first path's destination
    // exists, and nothing proves it holds the first copy's content.
    fs::write(destination.join("tree/a.bin"), b"left over").unwrap();

    let store = JobStore::new(root.path().join("jobs"));
    let engine = JobEngine::new(EngineConfig {
        workers: 1,
        store: Some(store),
        conflicts: ConflictPolicy::new(),
    });
    let snapshot = run(
        &engine,
        JobSpec::new(Operation::Copy {
            sources: vec![local(&tree)],
            destination: local(&destination),
        })
        .with_conflicts(ConflictPolicy::always(
            ConflictKind::Exists,
            Resolution::Skip,
        )),
    );
    assert_eq!(snapshot.state, JobState::Completed);
    let second = destination.join("tree/b/linked.bin");
    assert_ne!(inode(&second), inode(&destination.join("tree/a.bin")));
    assert_eq!(
        fs::read(&second).unwrap(),
        fs::read(tree.join("a.bin")).unwrap()
    );
    assert_eq!(
        fs::read(destination.join("tree/a.bin")).unwrap(),
        b"left over"
    );
}
