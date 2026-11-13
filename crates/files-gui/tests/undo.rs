//! Undo and redo of folio's eight kinds of change, run against real jobs in
//! a scratch folder with a scratch trash. Nothing is ever deleted: what undo
//! removes is in the trash, and redo brings it back.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use files_core::location::LocalPath;
use files_gui::i18n::{EN_US, ZH_TW};
use files_gui::undo::{Change, ChangeKind, History, Refusal, change_from_job};
use files_operations::{
    ArchiveFormat, ConflictDecision, EngineConfig, JobEngine, JobSpec, JobState, Operation,
    Resolution,
};

struct Rig {
    _root: tempfile::TempDir,
    dir: PathBuf,
    trash: PathBuf,
    engine: JobEngine,
}

fn rig() -> Rig {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("work");
    fs::create_dir_all(&dir).unwrap();
    let trash = root.path().join("Trash");
    let engine = JobEngine::new(EngineConfig {
        store: None,
        displaced_trash: Some(trash.clone()),
        ..EngineConfig::default()
    });
    Rig {
        _root: root,
        dir,
        trash,
        engine,
    }
}

fn local(path: &Path) -> LocalPath {
    LocalPath::new(path).unwrap()
}

impl Rig {
    /// Runs a job (answering a clash with `answer`) and returns its change.
    fn run(&self, operation: Operation, answer: Option<Resolution>) -> Change {
        let handle = self.engine.submit(JobSpec::new(operation.clone())).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        loop {
            let snapshot = self.engine.snapshot(handle.id()).unwrap();
            if snapshot.state == JobState::WaitingOnConflict {
                self.engine.resolve(
                    handle.id(),
                    ConflictDecision::once(answer.expect("a clash was expected")),
                );
            }
            if snapshot.state.is_terminal() {
                assert_eq!(
                    snapshot.state,
                    JobState::Completed,
                    "{:?}",
                    snapshot.failures
                );
                return change_from_job(&operation, &snapshot).expect("an undoable change");
            }
            assert!(
                std::time::Instant::now() < deadline,
                "the job never finished"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn history(&self) -> History {
        History::with_trash(Some(self.trash.clone()))
    }

    fn trashed(&self) -> usize {
        fs::read_dir(self.trash.join("files"))
            .map(|entries| entries.count())
            .unwrap_or(0)
    }
}

#[test]
fn a_rename_is_undone_and_redone() {
    let rig = rig();
    let a = rig.dir.join("a.txt");
    fs::write(&a, b"x").unwrap();
    let change = rig.run(
        Operation::Rename {
            path: local(&a),
            new_name: "b.txt".into(),
        },
        None,
    );
    assert_eq!(change.kind, ChangeKind::Rename);
    let mut history = rig.history();
    history.record(change);
    assert_eq!(history.undo_name(&EN_US).as_deref(), Some("Undo Rename"));
    assert_eq!(history.undo(), Ok(ChangeKind::Rename));
    assert!(a.exists() && !rig.dir.join("b.txt").exists());
    assert_eq!(history.redo(), Ok(ChangeKind::Rename));
    assert!(!a.exists() && rig.dir.join("b.txt").exists());
}

#[test]
fn a_move_goes_back_where_it_came_from() {
    let rig = rig();
    let folder = rig.dir.join("folder");
    fs::create_dir(&folder).unwrap();
    let a = rig.dir.join("a.txt");
    fs::write(&a, b"x").unwrap();
    let mut history = rig.history();
    history.record(rig.run(
        Operation::Move {
            sources: vec![local(&a)],
            destination: local(&folder),
        },
        None,
    ));
    assert!(folder.join("a.txt").exists());
    assert_eq!(history.undo(), Ok(ChangeKind::Move));
    assert!(a.exists() && !folder.join("a.txt").exists());
}

#[test]
fn undoing_a_copy_puts_the_copy_in_the_trash_and_redo_brings_it_back() {
    let rig = rig();
    let source = rig.dir.join("src");
    fs::create_dir_all(source.join("inner")).unwrap();
    fs::write(source.join("inner/f.txt"), b"x").unwrap();
    let out = rig.dir.join("out");
    fs::create_dir(&out).unwrap();
    let change = rig.run(
        Operation::Copy {
            sources: vec![local(&source)],
            destination: local(&out),
        },
        None,
    );
    // One step for the whole folder, not one per file.
    assert_eq!(change.steps.len(), 1);
    let mut history = rig.history();
    history.record(change);
    assert_eq!(history.undo(), Ok(ChangeKind::Copy));
    assert!(!out.join("src").exists());
    assert_eq!(rig.trashed(), 1, "the copy is in the trash, not gone");
    assert_eq!(history.redo(), Ok(ChangeKind::Copy));
    assert_eq!(fs::read(out.join("src/inner/f.txt")).unwrap(), b"x");
    assert_eq!(rig.trashed(), 0);
}

#[test]
fn undoing_a_copy_that_replaced_a_file_brings_the_old_one_back() {
    let rig = rig();
    let source = rig.dir.join("new/report.txt");
    fs::create_dir_all(source.parent().unwrap()).unwrap();
    fs::write(&source, b"new").unwrap();
    let out = rig.dir.join("out");
    fs::create_dir(&out).unwrap();
    fs::write(out.join("report.txt"), b"old").unwrap();
    let mut history = rig.history();
    history.record(rig.run(
        Operation::Copy {
            sources: vec![local(&source)],
            destination: local(&out),
        },
        Some(Resolution::Overwrite),
    ));
    assert_eq!(fs::read(out.join("report.txt")).unwrap(), b"new");
    assert_eq!(history.undo(), Ok(ChangeKind::Copy));
    assert_eq!(fs::read(out.join("report.txt")).unwrap(), b"old");
    assert_eq!(history.redo(), Ok(ChangeKind::Copy));
    assert_eq!(fs::read(out.join("report.txt")).unwrap(), b"new");
}

#[test]
fn duplicate_new_folder_new_file_compress_and_extract_undo_into_the_trash() {
    let rig = rig();
    let a = rig.dir.join("a.txt");
    fs::write(&a, b"content").unwrap();
    let mut history = rig.history();

    history.record(rig.run(
        Operation::Duplicate {
            sources: vec![local(&a)],
        },
        None,
    ));
    history.record(rig.run(
        Operation::CreateFolder {
            parent: local(&rig.dir),
            name: "Made".into(),
        },
        None,
    ));
    history.record(rig.run(
        Operation::CreateFile {
            parent: local(&rig.dir),
            name: "made.txt".into(),
        },
        None,
    ));
    let archive = rig.dir.join("a.zip");
    history.record(rig.run(
        Operation::Archive {
            sources: vec![local(&a)],
            destination: local(&archive),
            format: ArchiveFormat::Zip,
        },
        None,
    ));
    let unpacked = rig.dir.join("unpacked");
    fs::create_dir(&unpacked).unwrap();
    history.record(rig.run(
        Operation::Extract {
            archives: vec![local(&archive)],
            destination: local(&unpacked),
        },
        None,
    ));
    let made: Vec<String> = fs::read_dir(&unpacked)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(made.len(), 1);

    for kind in [
        ChangeKind::Extract,
        ChangeKind::Compress,
        ChangeKind::NewFile,
        ChangeKind::NewFolder,
        ChangeKind::Duplicate,
    ] {
        assert_eq!(history.undo(), Ok(kind));
    }
    assert_eq!(fs::read_dir(&unpacked).unwrap().count(), 0);
    assert!(!archive.exists());
    assert!(!rig.dir.join("Made").exists());
    assert!(!rig.dir.join("made.txt").exists());
    assert!(!rig.dir.join("a (copy).txt").exists());
    assert_eq!(rig.trashed(), 5);
    assert!(a.exists(), "the original is untouched");
}

#[test]
fn undoing_a_delete_takes_it_back_out_of_the_trash() {
    let rig = rig();
    let a = rig.dir.join("a.txt");
    fs::write(&a, b"x").unwrap();
    let mut history = rig.history();
    history.record(rig.run(
        Operation::Trash {
            sources: vec![local(&a)],
            trash_root: Some(rig.trash.clone()),
        },
        None,
    ));
    assert!(!a.exists());
    assert_eq!(history.undo_name(&EN_US).as_deref(), Some("Undo Delete"));
    assert_eq!(history.undo(), Ok(ChangeKind::Trash));
    assert!(a.exists());
    assert_eq!(rig.trashed(), 0);
    assert_eq!(history.redo(), Ok(ChangeKind::Trash));
    assert!(!a.exists());
    assert_eq!(rig.trashed(), 1);
}

#[test]
fn an_item_moved_since_stops_the_undo_and_nothing_runs() {
    let rig = rig();
    let a = rig.dir.join("a.txt");
    fs::write(&a, b"x").unwrap();
    let mut history = rig.history();
    history.record(rig.run(
        Operation::Rename {
            path: local(&a),
            new_name: "b.txt".into(),
        },
        None,
    ));
    fs::rename(rig.dir.join("b.txt"), rig.dir.join("c.txt")).unwrap();
    let refusal = history.undo().unwrap_err();
    assert_eq!(
        refusal,
        Refusal::NotWhereLeft {
            name: "b.txt".into()
        }
    );
    assert_eq!(
        refusal.message(true, &EN_US),
        "Can't undo that: b.txt isn't where it was left."
    );
    assert!(refusal.message(true, &ZH_TW).contains("b.txt"));
    assert_eq!(
        fs::read(rig.dir.join("c.txt")).unwrap(),
        b"x",
        "nothing ran"
    );
}

/// A change that cannot be undone any more does not stand in front of the
/// ones before it: it is said so and forgotten, and the next Undo reaches
/// the older change.
#[test]
fn a_change_that_cannot_be_undone_does_not_block_the_ones_before_it() {
    let rig = rig();
    let older = rig.dir.join("older.txt");
    let newer = rig.dir.join("newer.txt");
    fs::write(&older, b"o").unwrap();
    fs::write(&newer, b"n").unwrap();
    let mut history = rig.history();
    history.record(rig.run(
        Operation::Rename {
            path: local(&older),
            new_name: "older-renamed.txt".into(),
        },
        None,
    ));
    history.record(rig.run(
        Operation::Rename {
            path: local(&newer),
            new_name: "newer-renamed.txt".into(),
        },
        None,
    ));
    // Somebody moves the newer one on.
    fs::rename(
        rig.dir.join("newer-renamed.txt"),
        rig.dir.join("elsewhere.txt"),
    )
    .unwrap();
    assert!(matches!(history.undo(), Err(Refusal::NotWhereLeft { .. })));
    assert_eq!(history.undo(), Ok(ChangeKind::Rename));
    assert!(older.exists(), "the older rename was undone");
    assert!(
        rig.dir.join("elsewhere.txt").exists(),
        "the moved one was left alone"
    );
}

/// A move into a folder that was already there, answered with Merge, went
/// item by item; its undo puts every item back where it was (making the
/// source folders again), leaves what was only there, brings back what it
/// replaced, and redo does it again.
#[test]
fn a_move_answered_with_merge_is_undone_item_by_item() {
    let rig = rig();
    let source = rig.dir.join("in/photos");
    fs::create_dir_all(source.join("2024")).unwrap();
    fs::write(source.join("a.jpg"), b"new a").unwrap();
    fs::write(source.join("clash.jpg"), b"new clash").unwrap();
    fs::write(source.join("2024/b.jpg"), b"b").unwrap();
    let out = rig.dir.join("out");
    fs::create_dir_all(out.join("photos")).unwrap();
    fs::write(out.join("photos/clash.jpg"), b"old clash").unwrap();
    fs::write(out.join("photos/only-there.jpg"), b"stay").unwrap();

    let change = rig.run(
        Operation::Move {
            sources: vec![local(&source)],
            destination: local(&out),
        },
        Some(Resolution::Merge),
    );
    assert_eq!(change.kind, ChangeKind::Move);
    assert!(!source.exists(), "the move emptied and removed the source");
    let mut history = rig.history();
    history.record(change);

    assert_eq!(history.undo(), Ok(ChangeKind::Move));
    assert_eq!(fs::read(source.join("a.jpg")).unwrap(), b"new a");
    assert_eq!(fs::read(source.join("clash.jpg")).unwrap(), b"new clash");
    assert_eq!(fs::read(source.join("2024/b.jpg")).unwrap(), b"b");
    assert_eq!(
        fs::read(out.join("photos/clash.jpg")).unwrap(),
        b"old clash"
    );
    assert_eq!(
        fs::read(out.join("photos/only-there.jpg")).unwrap(),
        b"stay"
    );
    assert!(!out.join("photos/a.jpg").exists());
    assert!(
        !out.join("photos/2024").exists(),
        "the folder the move made is gone"
    );

    assert_eq!(history.redo(), Ok(ChangeKind::Move));
    assert_eq!(fs::read(out.join("photos/a.jpg")).unwrap(), b"new a");
    assert_eq!(fs::read(out.join("photos/2024/b.jpg")).unwrap(), b"b");
    assert_eq!(
        fs::read(out.join("photos/clash.jpg")).unwrap(),
        b"new clash"
    );
}

/// Merge into a folder where nothing clashes inside is still a change.
#[test]
fn a_merged_move_with_no_clash_inside_is_still_undoable() {
    let rig = rig();
    let source = rig.dir.join("in/docs");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("new.txt"), b"new").unwrap();
    let out = rig.dir.join("out");
    fs::create_dir_all(out.join("docs")).unwrap();
    fs::write(out.join("docs/old.txt"), b"old").unwrap();
    let mut history = rig.history();
    history.record(rig.run(
        Operation::Move {
            sources: vec![local(&source)],
            destination: local(&out),
        },
        Some(Resolution::Merge),
    ));
    assert_eq!(history.undo(), Ok(ChangeKind::Move));
    assert_eq!(fs::read(source.join("new.txt")).unwrap(), b"new");
    assert!(!out.join("docs/new.txt").exists());
    assert_eq!(fs::read(out.join("docs/old.txt")).unwrap(), b"old");
}

#[test]
fn another_file_in_the_same_place_is_not_mistaken_for_the_one_left_there() {
    let rig = rig();
    let a = rig.dir.join("a.txt");
    fs::write(&a, b"x").unwrap();
    let mut history = rig.history();
    history.record(rig.run(
        Operation::Rename {
            path: local(&a),
            new_name: "b.txt".into(),
        },
        None,
    ));
    // Somebody replaces b.txt with a different file of the same name.
    // Made while b.txt still exists, so it cannot be given b.txt's inode.
    fs::write(rig.dir.join("other"), b"someone else's").unwrap();
    fs::rename(rig.dir.join("other"), rig.dir.join("b.txt")).unwrap();
    assert!(matches!(history.undo(), Err(Refusal::NotWhereLeft { .. })));
    assert_eq!(fs::read(rig.dir.join("b.txt")).unwrap(), b"someone else's");
}

#[test]
fn a_file_back_in_the_old_place_blocks_the_undo() {
    let rig = rig();
    let a = rig.dir.join("a.txt");
    fs::write(&a, b"x").unwrap();
    let mut history = rig.history();
    history.record(rig.run(
        Operation::Rename {
            path: local(&a),
            new_name: "b.txt".into(),
        },
        None,
    ));
    fs::write(&a, b"new a").unwrap();
    let refusal = history.undo().unwrap_err();
    assert!(matches!(refusal, Refusal::Occupied { .. }));
    assert!(refusal.message(true, &EN_US).contains("already exists in"));
    assert_eq!(fs::read(&a).unwrap(), b"new a");
}

#[test]
fn a_new_change_forgets_what_was_undone() {
    let rig = rig();
    let a = rig.dir.join("a.txt");
    fs::write(&a, b"x").unwrap();
    let mut history = rig.history();
    history.record(rig.run(
        Operation::Rename {
            path: local(&a),
            new_name: "b.txt".into(),
        },
        None,
    ));
    history.undo().unwrap();
    assert!(history.can_redo());
    history.record(rig.run(
        Operation::CreateFolder {
            parent: local(&rig.dir),
            name: "x".into(),
        },
        None,
    ));
    assert!(!history.can_redo());
    assert_eq!(history.redo(), Err(Refusal::Nothing));
}

#[test]
fn undoing_a_merge_takes_back_only_what_it_brought_and_restores_what_it_replaced() {
    let rig = rig();
    let source = rig.dir.join("new/reports");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("summary.txt"), b"new").unwrap();
    fs::write(source.join("added.txt"), b"added").unwrap();
    let out = rig.dir.join("out");
    fs::create_dir_all(out.join("reports")).unwrap();
    fs::write(out.join("reports/summary.txt"), b"old").unwrap();
    fs::write(out.join("reports/keep.txt"), b"only there").unwrap();
    let change = rig.run(
        Operation::Copy {
            sources: vec![local(&source)],
            destination: local(&out),
        },
        Some(Resolution::Merge),
    );
    // The folder merged into is not something the copy made.
    assert!(change.steps.iter().all(|step| !matches!(
        step,
        files_gui::undo::Step::Made { path, .. } if path == &out.join("reports")
    )));
    let mut history = rig.history();
    history.record(change);
    assert_eq!(history.undo(), Ok(ChangeKind::Copy));
    assert_eq!(fs::read(out.join("reports/summary.txt")).unwrap(), b"old");
    assert_eq!(
        fs::read(out.join("reports/keep.txt")).unwrap(),
        b"only there"
    );
    assert!(!out.join("reports/added.txt").exists());
}
