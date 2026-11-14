//! A copy an earlier Better Files did not finish is offered at the next
//! start, resumed on request, or dismissed for good. Records live in a
//! temporary store; nothing touches the person's own.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use files_gui::interrupted::InterruptedJobs;
use files_operations::{
    EngineConfig, ItemRecord, ItemStatus, JobEngine, JobRecord, JobState, JobStore, ResumeSpec,
};

/// What a process that died mid-copy leaves: job `id`, still running, one
/// item done and one not.
fn dead_copy(store: &JobStore, id: u64, source: &Path, destination: &Path) {
    let item = |name: &str, status| ItemRecord {
        source: source.join(name),
        destination: Some(destination.join(name)),
        status,
        bytes: 1,
        error: None,
    };
    store
        .write(&JobRecord {
            schema_version: files_operations::store::RECORD_SCHEMA_VERSION,
            id,
            kind: files_operations::OperationKind::Copy,
            state: JobState::Running,
            progress: files_operations::Progress::default(),
            items: vec![
                item("a.txt", ItemStatus::Done),
                item("b.txt", ItemStatus::Pending),
            ],
            log: files_operations::OperationLog::default(),
            updated_at: 1,
            checksums: Vec::new(),
            resume: Some(ResumeSpec {
                moving: false,
                sources: vec![source.join("a.txt"), source.join("b.txt")],
                destination: destination.to_path_buf(),
            }),
            folder_answers: Vec::new(),
        })
        .unwrap();
}

fn folders(root: &Path) -> (PathBuf, PathBuf) {
    let source = root.join("source");
    let destination = root.join("destination");
    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(&destination).unwrap();
    fs::write(source.join("a.txt"), b"a").unwrap();
    fs::write(source.join("b.txt"), b"b").unwrap();
    // a.txt was copied before the process died.
    fs::write(destination.join("a.txt"), b"a").unwrap();
    (source, destination)
}

#[test]
fn an_unfinished_copy_is_offered_until_it_is_resumed() {
    let root = tempfile::tempdir().unwrap();
    let store = JobStore::new(root.path().join("jobs"));
    let (source, destination) = folders(root.path());
    dead_copy(&store, 3, &source, &destination);

    let offered = InterruptedJobs::from_recovery(store.recover(), Some(store.clone()));
    let list = offered.list();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].id, 3);
    assert!(!list[0].moving);
    assert_eq!(list[0].destination, destination);
    assert_eq!(list[0].remaining, 1);

    // Not answered this time: the next start offers it again.
    let again = InterruptedJobs::from_recovery(store.recover(), Some(store.clone()));
    assert_eq!(again.list().len(), 1);

    let engine = JobEngine::new(EngineConfig {
        store: Some(store.clone()),
        displaced_trash: Some(root.path().join("Trash")),
        ..EngineConfig::default()
    });
    let handle = again.resume(3, &engine).unwrap().unwrap();
    let finished = engine.wait(handle.id(), Duration::from_secs(20)).unwrap();
    assert_eq!(
        finished.state,
        JobState::Completed,
        "{:?}",
        finished.failures
    );
    assert_eq!(fs::read(destination.join("b.txt")).unwrap(), b"b");
    assert!(again.is_empty());
    // Resumed: never offered again.
    assert!(
        InterruptedJobs::from_recovery(store.recover(), None)
            .list()
            .is_empty()
    );
}

#[test]
fn a_dismissed_copy_is_forgotten() {
    let root = tempfile::tempdir().unwrap();
    let store = JobStore::new(root.path().join("jobs"));
    let (source, destination) = folders(root.path());
    dead_copy(&store, 5, &source, &destination);

    let offered = InterruptedJobs::from_recovery(store.recover(), Some(store.clone()));
    offered.dismiss(5);
    assert!(offered.is_empty());
    assert!(!store.holds(5));
    assert!(!destination.join("b.txt").exists(), "nothing was copied");
}
