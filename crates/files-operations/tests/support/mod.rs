//! Shared scaffolding for the integration tests.
//!
//! Each test binary uses a different subset, so the unused half is expected.
#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use files_core::location::LocalPath;
use files_operations::{EngineConfig, JobEngine, JobId, JobSnapshot, JobSpec, JobState};

/// A generous ceiling. Every job in this suite finishes in milliseconds; the
/// timeout exists so a deadlock fails the run instead of hanging it.
pub const LIMIT: Duration = Duration::from_secs(20);

pub fn local(path: impl AsRef<Path>) -> LocalPath {
    LocalPath::new(path.as_ref()).expect("an absolute path")
}

/// An engine whose Replace and Merge send what they displace to a trash of
/// this test binary's own, never the person's.
pub fn engine() -> JobEngine {
    engine_with_trash(&test_trash())
}

/// This test binary's own trash, so no test ever reaches the person's.
pub fn test_trash() -> PathBuf {
    static TRASH: std::sync::OnceLock<tempfile::TempDir> = std::sync::OnceLock::new();
    TRASH
        .get_or_init(|| tempfile::tempdir().expect("a scratch trash"))
        .path()
        .join("Trash")
}

/// An engine that sends displaced items to `trash`.
pub fn engine_with_trash(trash: &Path) -> JobEngine {
    JobEngine::new(EngineConfig {
        workers: 2,
        store: None,
        conflicts: files_operations::ConflictPolicy::new(),
        displaced_trash: Some(trash.to_path_buf()),
    })
}

/// Runs a spec to completion and returns the final snapshot.
pub fn run(engine: &JobEngine, spec: JobSpec) -> JobSnapshot {
    let handle = engine.submit(spec).expect("the spec was accepted");
    engine
        .wait(handle.id(), LIMIT)
        .expect("the job reached a terminal state")
}

/// Runs a spec and asserts it completed with nothing failed.
pub fn run_ok(engine: &JobEngine, spec: JobSpec) -> JobSnapshot {
    let snapshot = run(engine, spec);
    assert_eq!(
        snapshot.state,
        JobState::Completed,
        "failures: {:?}",
        snapshot.failures
    );
    snapshot
}

pub fn wait_for(
    engine: &JobEngine,
    id: JobId,
    accept: impl Fn(&JobSnapshot) -> bool,
) -> JobSnapshot {
    engine
        .wait_for(id, LIMIT, accept)
        .expect("the job reached the expected state")
}

/// A file of `size` bytes with a repeating, position-dependent pattern, so a
/// truncated or shuffled copy is detectable rather than merely the wrong length.
pub fn write_pattern(path: &Path, size: usize) {
    let data: Vec<u8> = (0..size).map(|index| (index % 251) as u8).collect();
    fs::write(path, data).expect("the fixture was written");
}

pub fn read(path: &Path) -> Vec<u8> {
    fs::read(path).unwrap_or_else(|error| panic!("reading {}: {error}", path.display()))
}

/// Whether this process is root, which makes every permission test vacuous.
pub fn running_as_root() -> bool {
    unsafe { libc::geteuid() == 0 }
}

/// Names every entry directly inside a directory, sorted.
pub fn entries(path: &Path) -> Vec<PathBuf> {
    let mut names: Vec<PathBuf> = fs::read_dir(path)
        .expect("a readable directory")
        .flatten()
        .map(|entry| entry.path())
        .collect();
    names.sort();
    names
}

/// Holds a job in the observer's `starting` call, which the engine makes
/// before the job's first write, until the test lets it go.
///
/// A pause or cancel requested while a job is held is guaranteed to land at
/// the job's first checkpoint, whatever the machine's load. Waiting for some
/// progress and then pausing is a race the job can win by finishing first.
#[derive(Default)]
pub struct StartGate {
    state: std::sync::Mutex<GateState>,
    changed: std::sync::Condvar,
}

#[derive(Default)]
struct GateState {
    closed: bool,
    held: Option<JobId>,
}

impl StartGate {
    /// Holds the next job that starts.
    pub fn close(&self) {
        self.state.lock().expect("gate").closed = true;
    }

    /// Waits until a job is held, and returns which.
    pub fn held(&self) -> JobId {
        let state = self.state.lock().expect("gate");
        let (state, timeout) = self
            .changed
            .wait_timeout_while(state, LIMIT, |state| state.held.is_none())
            .expect("gate");
        assert!(!timeout.timed_out(), "no job reached the gate");
        state.held.expect("a held job")
    }

    /// Lets the held job go on.
    pub fn open(&self) {
        let mut state = self.state.lock().expect("gate");
        state.closed = false;
        state.held = None;
        self.changed.notify_all();
    }
}

impl files_operations::JobObserver for StartGate {
    fn starting(&self, id: JobId, _spec: &JobSpec) {
        let mut state = self.state.lock().expect("gate");
        if !state.closed {
            return;
        }
        state.held = Some(id);
        self.changed.notify_all();
        let _state = self
            .changed
            .wait_while(state, |state| state.closed)
            .expect("gate");
    }

    fn finished(&self, _id: JobId, _spec: &JobSpec, _state: JobState) {}
}

/// An engine whose jobs a test can hold at their start.
pub fn gated_engine() -> (JobEngine, std::sync::Arc<StartGate>) {
    let gate = std::sync::Arc::new(StartGate::default());
    let engine = JobEngine::with_observer(
        EngineConfig {
            workers: 2,
            store: None,
            conflicts: files_operations::ConflictPolicy::new(),
            displaced_trash: Some(test_trash()),
        },
        gate.clone(),
    );
    (engine, gate)
}
