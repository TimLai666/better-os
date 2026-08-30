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

pub fn engine() -> JobEngine {
    JobEngine::new(EngineConfig {
        workers: 2,
        store: None,
        conflicts: files_operations::ConflictPolicy::new(),
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
