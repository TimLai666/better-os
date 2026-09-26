//! The observer seam: told before a job's first write, and after its last.
//!
//! Better Files uses it to register a job that writes to a removable disk with
//! the storage service. The engine knows nothing about devices; what it
//! promises is the ordering, and that is what these tests hold it to.

mod support;

use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use files_operations::{
    ConflictPolicy, EngineConfig, JobEngine, JobId, JobObserver, JobSpec, JobState, Operation,
};

use support::{LIMIT, local, wait_for, write_pattern};

#[derive(Clone, Debug, PartialEq)]
enum Heard {
    Starting {
        id: JobId,
        written: bool,
    },
    Finished {
        id: JobId,
        state: JobState,
        written: bool,
    },
}

/// Records each call, and whether the file the job writes was already there
/// when the call was made.
struct Recorder {
    watched: PathBuf,
    heard: Mutex<Vec<Heard>>,
}

impl Recorder {
    fn new(watched: PathBuf) -> Arc<Self> {
        Arc::new(Self {
            watched,
            heard: Mutex::new(Vec::new()),
        })
    }

    fn heard(&self) -> Vec<Heard> {
        self.heard.lock().unwrap().clone()
    }
}

impl JobObserver for Recorder {
    fn starting(&self, id: JobId, _spec: &JobSpec) {
        let written = self.watched.exists();
        self.heard
            .lock()
            .unwrap()
            .push(Heard::Starting { id, written });
    }

    fn finished(&self, id: JobId, _spec: &JobSpec, state: JobState) {
        let written = self.watched.exists();
        self.heard
            .lock()
            .unwrap()
            .push(Heard::Finished { id, state, written });
    }
}

fn observed(workers: usize, recorder: Arc<Recorder>) -> JobEngine {
    JobEngine::with_observer(
        EngineConfig {
            workers,
            store: None,
            conflicts: ConflictPolicy::new(),
        },
        recorder,
    )
}

#[test]
fn the_observer_hears_a_job_start_before_its_first_write_and_finish_before_it_is_done() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("big.bin");
    write_pattern(&source, 256 * 1024);
    let destination = root.path().join("usb");
    fs::create_dir(&destination).unwrap();
    let recorder = Recorder::new(destination.join("big.bin"));
    let engine = observed(1, recorder.clone());

    let handle = engine
        .submit(JobSpec::new(Operation::Copy {
            sources: vec![local(&source)],
            destination: local(&destination),
        }))
        .unwrap();
    let id = handle.id();
    let done = engine.wait(id, LIMIT).expect("the job finished");
    assert_eq!(done.state, JobState::Completed);

    // Read the moment the terminal state is visible: the finish call has
    // already returned by then.
    assert_eq!(
        recorder.heard(),
        vec![
            Heard::Starting { id, written: false },
            Heard::Finished {
                id,
                state: JobState::Completed,
                written: true,
            },
        ]
    );
}

#[test]
fn a_failed_job_is_still_heard_finishing() {
    let root = tempfile::tempdir().unwrap();
    let destination = root.path().join("usb");
    fs::create_dir(&destination).unwrap();
    let recorder = Recorder::new(destination.join("gone.bin"));
    let engine = observed(1, recorder.clone());

    let handle = engine
        .submit(JobSpec::new(Operation::Copy {
            sources: vec![local(root.path().join("gone.bin"))],
            destination: local(&destination),
        }))
        .unwrap();
    let done = engine.wait(handle.id(), LIMIT).unwrap();
    assert_eq!(done.state, JobState::Failed);
    assert_eq!(
        recorder.heard().last(),
        Some(&Heard::Finished {
            id: handle.id(),
            state: JobState::Failed,
            written: false,
        })
    );
}

/// A source with a file the destination already has, so the job parks on a
/// conflict and holds its worker.
fn parked_copy(root: &std::path::Path) -> (PathBuf, PathBuf) {
    let source = root.join("source");
    fs::create_dir(&source).unwrap();
    fs::write(source.join("one.txt"), b"new").unwrap();
    let destination = root.join("usb");
    fs::create_dir_all(destination.join("source")).unwrap();
    fs::write(destination.join("source/one.txt"), b"old").unwrap();
    (source, destination)
}

#[test]
fn a_cancelled_job_is_heard_finishing_and_one_that_never_started_is_not_heard_at_all() {
    let root = tempfile::tempdir().unwrap();
    let (source, destination) = parked_copy(root.path());
    let recorder = Recorder::new(destination.join("never"));
    // One worker: the parked job holds it, so the second job stays queued.
    let engine = observed(1, recorder.clone());

    let parked = engine
        .submit(JobSpec::new(Operation::Copy {
            sources: vec![local(&source)],
            destination: local(&destination),
        }))
        .unwrap();
    wait_for(&engine, parked.id(), |snapshot| {
        snapshot.state == JobState::WaitingOnConflict
    });
    let queued = engine
        .submit(JobSpec::new(Operation::CreateFolder {
            parent: local(&destination),
            name: "later".into(),
        }))
        .unwrap();
    assert!(engine.cancel(queued.id()));
    assert!(engine.cancel(parked.id()));
    assert_eq!(
        engine.wait(parked.id(), LIMIT).unwrap().state,
        JobState::Cancelled
    );

    let heard = recorder.heard();
    assert_eq!(
        heard,
        vec![
            Heard::Starting {
                id: parked.id(),
                written: false,
            },
            Heard::Finished {
                id: parked.id(),
                state: JobState::Cancelled,
                written: false,
            },
        ],
        "the queued job never ran, so it was never announced"
    );
}

#[test]
fn a_retried_job_is_announced_again() {
    use std::os::unix::fs::PermissionsExt;
    if support::running_as_root() {
        // Root ignores the mode bits, so there is nothing to fail.
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let destination = root.path().join("usb");
    fs::create_dir(&destination).unwrap();
    let source = root.path().join("locked.bin");
    write_pattern(&source, 128);
    fs::set_permissions(&source, fs::Permissions::from_mode(0o000)).unwrap();
    let recorder = Recorder::new(destination.join("locked.bin"));
    let engine = observed(1, recorder.clone());

    let handle = engine
        .submit(JobSpec::new(Operation::Copy {
            sources: vec![local(&source)],
            destination: local(&destination),
        }))
        .unwrap();
    assert_eq!(
        engine.wait(handle.id(), LIMIT).unwrap().state,
        JobState::Failed
    );
    fs::set_permissions(&source, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(engine.retry_failed(handle.id()));
    wait_for(&engine, handle.id(), |snapshot| {
        snapshot.state == JobState::Completed
    });

    let id = handle.id();
    let heard = recorder.heard();
    assert_eq!(heard.len(), 4, "{heard:?}");
    assert_eq!(heard[2], Heard::Starting { id, written: false });
    assert_eq!(
        heard[3],
        Heard::Finished {
            id,
            state: JobState::Completed,
            written: true,
        }
    );
}
