//! Job records on disk, and what happens to them after a crash.
//!
//! Issue #6: "a crashed window must not leave jobs in an unknowable state".
//! The record is what makes that true. Every state change writes the job's
//! current record, so the file on disk always says either what the job was
//! doing or what it finished doing.
//!
//! ## What recovery does, and what it deliberately does not
//!
//! A record found in `Running`, `Paused`, or `WaitingOnConflict` after a
//! restart belonged to a process that is gone. Recovery moves it to `Failed`
//! with [`OperationError::Interrupted`] and keeps the item list, so the
//! operation centre can say "this copy stopped partway, 412 of 900 files were
//! done, here are the rest".
//!
//! It does not restart the job. Resuming needs the original [`JobSpec`], and
//! the record deliberately does not hold one: a `JobSpec` for a permanent
//! delete carries a confirmation the user gave to a process that no longer
//! exists, and reconstructing it from a file would make that confirmation
//! forgeable by anyone who can write to the state directory. The user
//! re-submits, which costs one click and closes that hole.
//!
//! Ticket 33 scopes persistence to surviving a UI restart. Whether a job should
//! survive a logout or a reboot is one of Issue #6's deferred decisions and
//! stays deferred.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::error::OperationError;
use crate::log::OperationLog;
use crate::progress::Progress;
use crate::spec::OperationKind;
use crate::state::JobState;

pub const RECORD_SCHEMA_VERSION: u32 = 1;

/// Serde for a `PathBuf` that may not be valid UTF-8.
///
/// A path is stored as its bytes. `serde_json` would otherwise refuse the
/// value or, worse, accept a lossily converted one and hand back a path that
/// names a different file.
pub(crate) mod path_bytes_option {
    use std::ffi::OsString;
    use std::os::unix::ffi::{OsStrExt, OsStringExt};
    use std::path::PathBuf;

    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(
        value: &Option<PathBuf>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        match value {
            Some(path) => serializer.serialize_some(path.as_os_str().as_bytes()),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<PathBuf>, D::Error> {
        let bytes: Option<Vec<u8>> = Option::deserialize(deserializer)?;
        Ok(bytes.map(|bytes| PathBuf::from(OsString::from_vec(bytes))))
    }
}

/// The same, for a path that is always present.
pub(crate) mod path_bytes {
    use std::ffi::OsString;
    use std::os::unix::ffi::{OsStrExt, OsStringExt};
    use std::path::{Path, PathBuf};

    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(value: &Path, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_bytes(value.as_os_str().as_bytes())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<PathBuf, D::Error> {
        let bytes: Vec<u8> = Vec::deserialize(deserializer)?;
        Ok(PathBuf::from(OsString::from_vec(bytes)))
    }
}

/// Where one item got to.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemStatus {
    Pending,
    Done,
    Failed,
    Skipped,
}

/// One item, as recorded.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ItemRecord {
    #[serde(with = "path_bytes")]
    pub source: PathBuf,
    #[serde(with = "path_bytes_option")]
    pub destination: Option<PathBuf>,
    pub status: ItemStatus,
    pub bytes: u64,
    pub error: Option<OperationError>,
}

/// The whole job, as recorded.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct JobRecord {
    pub schema_version: u32,
    pub id: u64,
    pub kind: OperationKind,
    pub state: JobState,
    pub progress: Progress,
    pub items: Vec<ItemRecord>,
    pub log: OperationLog,
    /// Seconds since the epoch when the record was last written.
    pub updated_at: u64,
    /// Digests a checksum job produced.
    pub checksums: Vec<(String, String)>,
}

impl JobRecord {
    /// The items that still have work in them.
    pub fn remaining(&self) -> Vec<&ItemRecord> {
        self.items
            .iter()
            .filter(|item| matches!(item.status, ItemStatus::Pending | ItemStatus::Failed))
            .collect()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Error)]
pub enum StoreError {
    #[error("files.job.store.error.io:{path}:{reason}")]
    Io { path: String, reason: String },
    #[error("files.job.store.error.unreadable:{path}:{reason}")]
    Unreadable { path: String, reason: String },
    #[error("files.job.store.error.unsupported_schema:{path}:{version}")]
    UnsupportedSchema { path: String, version: u32 },
}

impl StoreError {
    fn io(path: &Path, error: &io::Error) -> Self {
        Self::Io {
            path: path.to_string_lossy().into_owned(),
            reason: error.kind().to_string(),
        }
    }
}

/// What a recovery pass found.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Recovery {
    /// Records that were already terminal and are reported as they stand.
    pub settled: Vec<JobRecord>,
    /// Records that belonged to a process that died, moved to `Failed` with
    /// [`OperationError::Interrupted`] and rewritten.
    pub interrupted: Vec<JobRecord>,
    /// Files that would not parse. Reported rather than deleted: a record this
    /// build cannot read may be a record a newer build wrote, and silently
    /// removing it would lose the user's history.
    pub damaged: Vec<(PathBuf, StoreError)>,
}

/// A directory of job records.
#[derive(Clone, Debug)]
pub struct JobStore {
    root: PathBuf,
}

impl JobStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn path_for(&self, id: u64) -> PathBuf {
        self.root.join(format!("job-{id:020}.json"))
    }

    /// Writes a record, replacing whatever was there.
    ///
    /// Written to a temporary and renamed, so a crash during the write leaves
    /// the previous record rather than half of the new one. A job record that
    /// cannot be parsed is exactly the "unknowable state" the requirement
    /// forbids.
    pub fn write(&self, record: &JobRecord) -> Result<(), StoreError> {
        fs::create_dir_all(&self.root).map_err(|error| StoreError::io(&self.root, &error))?;
        let path = self.path_for(record.id);
        let temporary =
            self.root
                .join(format!(".job-{:020}.{}.tmp", record.id, std::process::id()));
        let text = serde_json::to_vec_pretty(record).map_err(|error| StoreError::Unreadable {
            path: path.to_string_lossy().into_owned(),
            reason: error.to_string(),
        })?;
        fs::write(&temporary, &text).map_err(|error| StoreError::io(&temporary, &error))?;
        fs::rename(&temporary, &path).map_err(|error| StoreError::io(&path, &error))?;
        Ok(())
    }

    pub fn read(&self, id: u64) -> Result<JobRecord, StoreError> {
        let path = self.path_for(id);
        read_record(&path)
    }

    pub fn remove(&self, id: u64) -> Result<(), StoreError> {
        let path = self.path_for(id);
        match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(StoreError::io(&path, &error)),
        }
    }

    /// Reads every record, settling the ones whose process is gone.
    pub fn recover(&self) -> Recovery {
        let mut recovery = Recovery::default();
        let Ok(entries) = fs::read_dir(&self.root) else {
            return recovery;
        };
        let mut paths: Vec<PathBuf> = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with("job-") && name.ends_with(".json"))
            })
            .collect();
        paths.sort();
        for path in paths {
            match read_record(&path) {
                Ok(record) if record.state.is_terminal() => recovery.settled.push(record),
                Ok(mut record) => {
                    record.state = JobState::Failed;
                    for item in record.items.iter_mut() {
                        if item.status == ItemStatus::Pending && item.error.is_none() {
                            item.error = Some(OperationError::Interrupted);
                        }
                    }
                    record.progress.items_failed = record
                        .items
                        .iter()
                        .filter(|item| item.status == ItemStatus::Failed || item.error.is_some())
                        .count() as u64;
                    let _ = self.write(&record);
                    recovery.interrupted.push(record);
                }
                Err(error) => recovery.damaged.push((path, error)),
            }
        }
        recovery
    }
}

fn read_record(path: &Path) -> Result<JobRecord, StoreError> {
    let text = fs::read(path).map_err(|error| StoreError::io(path, &error))?;
    let record: JobRecord =
        serde_json::from_slice(&text).map_err(|error| StoreError::Unreadable {
            path: path.to_string_lossy().into_owned(),
            reason: error.to_string(),
        })?;
    if record.schema_version != RECORD_SCHEMA_VERSION {
        return Err(StoreError::UnsupportedSchema {
            path: path.to_string_lossy().into_owned(),
            version: record.schema_version,
        });
    }
    Ok(record)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;

    fn record(id: u64, state: JobState) -> JobRecord {
        JobRecord {
            schema_version: RECORD_SCHEMA_VERSION,
            id,
            kind: OperationKind::Copy,
            state,
            progress: Progress {
                items_total: 2,
                items_done: 1,
                ..Progress::default()
            },
            items: vec![
                ItemRecord {
                    source: PathBuf::from("/src/a"),
                    destination: Some(PathBuf::from("/dst/a")),
                    status: ItemStatus::Done,
                    bytes: 10,
                    error: None,
                },
                ItemRecord {
                    source: PathBuf::from(OsStr::from_bytes(b"/src/\xffb")),
                    destination: Some(PathBuf::from("/dst/b")),
                    status: ItemStatus::Pending,
                    bytes: 20,
                    error: None,
                },
            ],
            log: OperationLog::default(),
            updated_at: 1,
            checksums: Vec::new(),
        }
    }

    #[test]
    fn a_record_round_trips_including_a_path_that_is_not_utf8() {
        let directory = tempfile::tempdir().unwrap();
        let store = JobStore::new(directory.path());
        let written = record(7, JobState::Running);
        store.write(&written).unwrap();
        let read = store.read(7).unwrap();
        assert_eq!(read, written);
        assert_eq!(
            read.items[1].source.as_os_str().as_bytes(),
            b"/src/\xffb".as_slice()
        );
    }

    #[test]
    fn a_job_whose_process_died_comes_back_failed_and_interrupted_not_running() {
        let directory = tempfile::tempdir().unwrap();
        let store = JobStore::new(directory.path());
        store.write(&record(1, JobState::Running)).unwrap();
        store.write(&record(2, JobState::Completed)).unwrap();

        let recovery = store.recover();
        assert_eq!(recovery.settled.len(), 1);
        assert_eq!(recovery.interrupted.len(), 1);
        let interrupted = &recovery.interrupted[0];
        assert_eq!(interrupted.state, JobState::Failed);
        assert_eq!(
            interrupted.items[1].error,
            Some(OperationError::Interrupted)
        );
        // And the settlement is durable: a second recovery finds it settled.
        let again = store.recover();
        assert_eq!(again.interrupted.len(), 0);
        assert_eq!(again.settled.len(), 2);
    }

    #[test]
    fn the_remaining_work_is_what_a_resubmitted_job_would_have_to_do() {
        let directory = tempfile::tempdir().unwrap();
        let store = JobStore::new(directory.path());
        store.write(&record(3, JobState::Running)).unwrap();
        let recovery = store.recover();
        let remaining = recovery.interrupted[0].remaining();
        assert_eq!(remaining.len(), 1);
        assert_eq!(
            remaining[0].destination.as_deref(),
            Some(Path::new("/dst/b"))
        );
    }

    #[test]
    fn a_record_this_build_cannot_read_is_reported_rather_than_deleted() {
        let directory = tempfile::tempdir().unwrap();
        let store = JobStore::new(directory.path());
        fs::create_dir_all(directory.path()).unwrap();
        fs::write(directory.path().join("job-00000000000000000009.json"), b"{").unwrap();
        let mut newer = record(10, JobState::Completed);
        newer.schema_version = 99;
        fs::write(
            directory.path().join("job-00000000000000000010.json"),
            serde_json::to_vec(&newer).unwrap(),
        )
        .unwrap();

        let recovery = store.recover();
        assert_eq!(recovery.damaged.len(), 2);
        assert!(
            directory
                .path()
                .join("job-00000000000000000009.json")
                .exists()
        );
        assert!(matches!(
            recovery.damaged[1].1,
            StoreError::UnsupportedSchema { version: 99, .. }
        ));
    }

    #[test]
    fn a_half_written_record_never_replaces_a_readable_one() {
        // The write goes through a temporary and a rename, so the real path
        // only ever holds a complete document.
        let directory = tempfile::tempdir().unwrap();
        let store = JobStore::new(directory.path());
        store.write(&record(4, JobState::Running)).unwrap();
        store.write(&record(4, JobState::Completed)).unwrap();
        assert_eq!(store.read(4).unwrap().state, JobState::Completed);
        let leftovers: Vec<_> = fs::read_dir(directory.path())
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name())
            .filter(|name| name.to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "left {leftovers:?}");
    }
}
