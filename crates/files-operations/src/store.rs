//! Job records on disk, and what happens to them after a crash.
//!
//! Issue #6: "a crashed window must not leave jobs in an unknowable state".
//! The record is what makes that true. Every state change writes the job's
//! current record, so the file on disk always says either what the job was
//! doing or what it finished doing.
//!
//! ## Two files per job
//!
//! A job is stored as a small header, `job-<id>.json`, and an append-only item
//! journal, `job-<id>.items.jsonl`, one JSON document per line:
//!
//! - an `item` line per planned item, in order, written when the journal is
//!   written whole;
//! - a `status` line each time an item's status changes, appended;
//! - a `checksum` line per digest a checksum job produced, appended.
//!
//! The header holds the state, the progress, and the bounded operation log,
//! and nothing that grows with the item count, so it is rewritten atomically —
//! a temporary and a rename — on every persist. A running job's persist
//! therefore writes the header plus the lines that changed since the last one,
//! not every item again. The journal is written whole — compacted to one line
//! per item — when the job is submitted, when its items are planned, when it
//! finishes, at recovery, and when it has grown past [`COMPACT_FACTOR`] times
//! its live entries.
//!
//! Replay reads the item lines and applies the status lines over them. A crash
//! in the middle of an append leaves a final line that does not parse, and that
//! line is ignored: the header never claimed more than what was appended before
//! it. A line that does not parse anywhere else is damage, and the record is
//! reported as such rather than read around.
//!
//! A record in the first, single-file format — schema version 1, with every
//! item inside the header — is read as it is and migrated to the two files on
//! load.
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
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::error::OperationError;
use crate::log::OperationLog;
use crate::progress::Progress;
use crate::spec::OperationKind;
use crate::state::JobState;

/// The format [`JobStore`] writes: a header plus an item journal.
pub const RECORD_SCHEMA_VERSION: u32 = 2;

/// The first format: one JSON document holding every item. Read and migrated.
const SINGLE_FILE_SCHEMA_VERSION: u32 = 1;

/// How many journal lines per live entry are tolerated before a persist
/// compacts the journal instead of appending to it. A normal run writes two
/// lines per item, its plan and its outcome; every retry adds two more.
pub const COMPACT_FACTOR: u64 = 4;

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
///
/// This is also the first format's on-disk shape, which is how a version 1
/// record is read without a second definition of it.
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

/// What the header file holds: the record without its items and checksums.
#[derive(Serialize)]
struct HeaderRef<'a> {
    schema_version: u32,
    id: u64,
    kind: OperationKind,
    state: JobState,
    progress: &'a Progress,
    log: &'a OperationLog,
    updated_at: u64,
}

#[derive(Deserialize)]
struct Header {
    id: u64,
    kind: OperationKind,
    state: JobState,
    progress: Progress,
    log: OperationLog,
    updated_at: u64,
}

/// Enough of a header to know which format it is.
#[derive(Deserialize)]
struct VersionProbe {
    schema_version: u32,
}

/// One line of the item journal.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "entry", rename_all = "snake_case")]
pub enum JournalEntry {
    /// A planned item. Its index is its position among the item lines.
    Item { item: ItemRecord },
    /// An item's status changed.
    Status {
        index: u64,
        status: ItemStatus,
        error: Option<OperationError>,
    },
    /// A digest a checksum job produced.
    Checksum { path: String, digest: String },
}

/// The borrowed form of an `item` line, so a whole-journal write serializes
/// the items it has rather than cloning every one of them first.
#[derive(Serialize)]
#[serde(tag = "entry", rename_all = "snake_case")]
enum JournalEntryRef<'a> {
    Item { item: &'a ItemRecord },
    Checksum { path: &'a str, digest: &'a str },
}

/// Where one job's journal stands between persists, kept by the engine.
///
/// It decides whether the next persist appends or writes the journal whole,
/// and remembers what has changed since the last one.
#[derive(Clone, Debug, Default)]
pub(crate) struct JournalCursor {
    /// The journal on disk matches the lines counted here.
    in_step: bool,
    lines: u64,
    /// Items whose status changed since the last persist, in order.
    changed: Vec<usize>,
    /// Checksums already in the journal.
    checksums: usize,
}

impl JournalCursor {
    /// Whether the next persist must write the journal whole: nothing is on
    /// disk yet, the items were replaced, an append failed, or the journal
    /// has grown past [`COMPACT_FACTOR`] times its `live` entries.
    pub(crate) fn must_rewrite(&self, live: usize) -> bool {
        !self.in_step || self.lines > COMPACT_FACTOR * (live.max(1) as u64)
    }

    /// The item list was replaced, so the journal has to be written again.
    pub(crate) fn items_replaced(&mut self) {
        self.in_step = false;
        self.changed.clear();
    }

    pub(crate) fn changed(&mut self, index: usize) {
        self.changed.push(index);
    }

    /// Takes the items that changed since the last persist.
    pub(crate) fn take_changed(&mut self) -> Vec<usize> {
        std::mem::take(&mut self.changed)
    }

    pub(crate) fn checksums_written(&self) -> usize {
        self.checksums
    }

    /// The journal was written whole with `items` item lines and `checksums`
    /// checksum lines.
    pub(crate) fn rewritten(&mut self, items: usize, checksums: usize) {
        self.in_step = true;
        self.lines = (items + checksums) as u64;
        self.checksums = checksums;
    }

    /// `lines` more lines were appended, and the journal now holds
    /// `checksums` checksum lines in total.
    pub(crate) fn appended(&mut self, lines: usize, checksums: usize) {
        self.lines += lines as u64;
        self.checksums = checksums;
    }

    /// An append may have left part of a line behind, so the next persist
    /// writes the journal whole rather than appending after it.
    pub(crate) fn append_failed(&mut self) {
        self.in_step = false;
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

    fn journal_for(&self, id: u64) -> PathBuf {
        self.root.join(format!("job-{id:020}.items.jsonl"))
    }

    /// Writes a record whole: the journal compacted to one line per item and
    /// checksum, then the header. Returns the bytes written.
    ///
    /// Both files go through a temporary and a rename, so a crash during the
    /// write leaves the previous files rather than half of the new ones. A job
    /// record that cannot be parsed is exactly the "unknowable state" the
    /// requirement forbids.
    pub fn write(&self, record: &JobRecord) -> Result<u64, StoreError> {
        fs::create_dir_all(&self.root).map_err(|error| StoreError::io(&self.root, &error))?;
        let path = self.journal_for(record.id);
        let mut journal = Vec::new();
        for item in &record.items {
            push_line(&mut journal, &JournalEntryRef::Item { item }, &path)?;
        }
        for (checksum_path, digest) in &record.checksums {
            push_line(
                &mut journal,
                &JournalEntryRef::Checksum {
                    path: checksum_path,
                    digest,
                },
                &path,
            )?;
        }
        self.replace(&path, record.id, "items", &journal)?;
        Ok(journal.len() as u64 + self.write_header(record)?)
    }

    /// Rewrites only the header: state, progress, and log. The record's items
    /// and checksums are ignored; they live in the journal. Returns the bytes
    /// written.
    pub fn write_header(&self, record: &JobRecord) -> Result<u64, StoreError> {
        fs::create_dir_all(&self.root).map_err(|error| StoreError::io(&self.root, &error))?;
        let path = self.path_for(record.id);
        let header = HeaderRef {
            schema_version: RECORD_SCHEMA_VERSION,
            id: record.id,
            kind: record.kind,
            state: record.state,
            progress: &record.progress,
            log: &record.log,
            updated_at: record.updated_at,
        };
        let text = serde_json::to_vec(&header).map_err(|error| StoreError::Unreadable {
            path: path.to_string_lossy().into_owned(),
            reason: error.to_string(),
        })?;
        self.replace(&path, record.id, "header", &text)?;
        Ok(text.len() as u64)
    }

    /// Appends lines to a job's journal, which must already have been
    /// written whole. Returns the bytes appended.
    ///
    /// The lines go out in one write. A crash partway leaves a final line that
    /// does not parse, which replay ignores.
    pub fn append(&self, id: u64, entries: &[JournalEntry]) -> Result<u64, StoreError> {
        let path = self.journal_for(id);
        let mut buffer = Vec::new();
        for entry in entries {
            push_line(&mut buffer, entry, &path)?;
        }
        let mut file = fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .map_err(|error| StoreError::io(&path, &error))?;
        file.write_all(&buffer)
            .map_err(|error| StoreError::io(&path, &error))?;
        Ok(buffer.len() as u64)
    }

    /// Writes `bytes` to `path` through a temporary and a rename.
    fn replace(&self, path: &Path, id: u64, part: &str, bytes: &[u8]) -> Result<(), StoreError> {
        let temporary = self
            .root
            .join(format!(".job-{id:020}.{part}.{}.tmp", std::process::id()));
        fs::write(&temporary, bytes).map_err(|error| StoreError::io(&temporary, &error))?;
        fs::rename(&temporary, path).map_err(|error| StoreError::io(path, &error))
    }

    pub fn read(&self, id: u64) -> Result<JobRecord, StoreError> {
        self.read_record(&self.path_for(id))
    }

    pub fn remove(&self, id: u64) -> Result<(), StoreError> {
        for path in [self.path_for(id), self.journal_for(id)] {
            match fs::remove_file(&path) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(StoreError::io(&path, &error)),
            }
        }
        Ok(())
    }

    /// Reads a record by its header path, migrating a single-file record.
    fn read_record(&self, path: &Path) -> Result<JobRecord, StoreError> {
        let unreadable = |reason: String| StoreError::Unreadable {
            path: path.to_string_lossy().into_owned(),
            reason,
        };
        let text = fs::read(path).map_err(|error| StoreError::io(path, &error))?;
        let probe: VersionProbe =
            serde_json::from_slice(&text).map_err(|error| unreadable(error.to_string()))?;
        match probe.schema_version {
            RECORD_SCHEMA_VERSION => {
                let header: Header =
                    serde_json::from_slice(&text).map_err(|error| unreadable(error.to_string()))?;
                let replayed = read_journal(&self.journal_for(header.id))?;
                Ok(JobRecord {
                    schema_version: RECORD_SCHEMA_VERSION,
                    id: header.id,
                    kind: header.kind,
                    state: header.state,
                    progress: header.progress,
                    items: replayed.items,
                    log: header.log,
                    updated_at: header.updated_at,
                    checksums: replayed.checksums,
                })
            }
            SINGLE_FILE_SCHEMA_VERSION => {
                let mut record: JobRecord =
                    serde_json::from_slice(&text).map_err(|error| unreadable(error.to_string()))?;
                record.schema_version = RECORD_SCHEMA_VERSION;
                // Migrated in place: the journal first, then the header over
                // the old file. A crash between the two leaves the old file,
                // which migrates again next time. A store that cannot be
                // written still reads.
                let _ = self.write(&record);
                Ok(record)
            }
            version => Err(StoreError::UnsupportedSchema {
                path: path.to_string_lossy().into_owned(),
                version,
            }),
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
            match self.read_record(&path) {
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

fn push_line<T: Serialize>(buffer: &mut Vec<u8>, entry: &T, path: &Path) -> Result<(), StoreError> {
    serde_json::to_writer(&mut *buffer, entry).map_err(|error| StoreError::Unreadable {
        path: path.to_string_lossy().into_owned(),
        reason: error.to_string(),
    })?;
    buffer.push(b'\n');
    Ok(())
}

/// What a journal replays to.
#[derive(Default)]
struct Replayed {
    items: Vec<ItemRecord>,
    checksums: Vec<(String, String)>,
}

/// Replays a journal into the item list and the checksums.
fn read_journal(path: &Path) -> Result<Replayed, StoreError> {
    let unreadable = |reason: String| StoreError::Unreadable {
        path: path.to_string_lossy().into_owned(),
        reason,
    };
    let bytes = fs::read(path).map_err(|error| StoreError::io(path, &error))?;
    let mut replayed = Replayed::default();
    let lines: Vec<&[u8]> = bytes.split(|byte| *byte == b'\n').collect();
    let last = lines.len() - 1;
    for (number, line) in lines.into_iter().enumerate() {
        if line.is_empty() {
            continue;
        }
        let entry: JournalEntry = match serde_json::from_slice(line) {
            Ok(entry) => entry,
            // No newline after it: an append the process did not live to
            // finish. What it was about to say was never promised by the
            // header, so it is dropped.
            Err(_) if number == last => break,
            Err(error) => return Err(unreadable(format!("line {}: {error}", number + 1))),
        };
        match entry {
            JournalEntry::Item { item } => replayed.items.push(item),
            JournalEntry::Status {
                index,
                status,
                error,
            } => {
                let Some(item) = usize::try_from(index)
                    .ok()
                    .and_then(|index| replayed.items.get_mut(index))
                else {
                    return Err(unreadable(format!(
                        "line {}: status for unplanned item {index}",
                        number + 1
                    )));
                };
                item.status = status;
                item.error = error;
            }
            JournalEntry::Checksum { path, digest } => replayed.checksums.push((path, digest)),
        }
    }
    Ok(replayed)
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

    // --- The journal ------------------------------------------------------

    fn many(id: u64, count: usize) -> JobRecord {
        let mut record = record(id, JobState::Running);
        record.items = (0..count)
            .map(|index| ItemRecord {
                source: PathBuf::from(format!("/home/user/source/file-{index:06}.bin")),
                destination: Some(PathBuf::from(format!(
                    "/home/user/destination/file-{index:06}.bin"
                ))),
                status: ItemStatus::Pending,
                bytes: 512,
                error: None,
            })
            .collect();
        record
    }

    fn journal_of(store: &JobStore, id: u64) -> PathBuf {
        store.root().join(format!("job-{id:020}.items.jsonl"))
    }

    fn status(index: u64, status: ItemStatus) -> JournalEntry {
        JournalEntry::Status {
            index,
            status,
            error: None,
        }
    }

    #[test]
    fn item_progress_is_appended_and_the_header_is_rewritten_alone() {
        let directory = tempfile::tempdir().unwrap();
        let store = JobStore::new(directory.path());
        let mut written = many(5, 1_000);
        store.write(&written).unwrap();
        let journal = journal_of(&store, 5);
        let before = fs::read(&journal).unwrap();

        written.items[3].status = ItemStatus::Done;
        written.progress.items_done = 1;
        let appended = store.append(5, &[status(3, ItemStatus::Done)]).unwrap();
        let header = store.write_header(&written).unwrap();

        let after = fs::read(&journal).unwrap();
        assert_eq!(&after[..before.len()], &before[..], "nothing was rewritten");
        assert_eq!(after.len() as u64, before.len() as u64 + appended);
        assert!(appended < 100, "one status line, got {appended} bytes");
        assert!(
            header < 4_096,
            "the header carries no items, got {header} bytes"
        );
        assert_eq!(store.read(5).unwrap(), written);
    }

    #[test]
    fn a_torn_final_line_is_ignored_and_the_line_before_it_kept() {
        let directory = tempfile::tempdir().unwrap();
        let store = JobStore::new(directory.path());
        let written = many(6, 3);
        store.write(&written).unwrap();
        store.append(6, &[status(0, ItemStatus::Done)]).unwrap();
        // A crash in the middle of the next append.
        let mut file = fs::OpenOptions::new()
            .append(true)
            .open(journal_of(&store, 6))
            .unwrap();
        std::io::Write::write_all(&mut file, br#"{"entry":"status","index":1,"sta"#).unwrap();

        let read = store.read(6).unwrap();
        assert_eq!(read.items[0].status, ItemStatus::Done);
        assert_eq!(read.items[1].status, ItemStatus::Pending);

        // Recovery settles the record and rewrites the journal whole, so the
        // torn fragment is gone rather than waiting under the next append.
        let recovery = store.recover();
        assert_eq!(recovery.interrupted.len(), 1);
        let text = fs::read_to_string(journal_of(&store, 6)).unwrap();
        assert!(text.ends_with('\n'));
        assert_eq!(text.lines().count(), 3);
    }

    #[test]
    fn a_damaged_line_before_the_end_is_reported_rather_than_skipped() {
        let directory = tempfile::tempdir().unwrap();
        let store = JobStore::new(directory.path());
        store.write(&many(7, 2)).unwrap();
        let mut file = fs::OpenOptions::new()
            .append(true)
            .open(journal_of(&store, 7))
            .unwrap();
        std::io::Write::write_all(&mut file, b"not json\n").unwrap();
        drop(file);
        store.append(7, &[status(1, ItemStatus::Done)]).unwrap();
        assert!(matches!(store.read(7), Err(StoreError::Unreadable { .. })));
    }

    #[test]
    fn a_status_for_an_item_the_journal_never_planned_is_damage() {
        let directory = tempfile::tempdir().unwrap();
        let store = JobStore::new(directory.path());
        store.write(&many(8, 2)).unwrap();
        store.append(8, &[status(2, ItemStatus::Done)]).unwrap();
        assert!(matches!(store.read(8), Err(StoreError::Unreadable { .. })));
    }

    #[test]
    fn a_record_in_the_single_file_format_is_read_and_migrated_on_load() {
        let directory = tempfile::tempdir().unwrap();
        let store = JobStore::new(directory.path());
        let mut legacy = record(9, JobState::Completed);
        legacy.schema_version = 1;
        legacy.checksums = vec![("/src/a".to_string(), "ab12".to_string())];
        // Exactly what the first format's writer produced.
        fs::write(
            directory.path().join("job-00000000000000000009.json"),
            serde_json::to_vec_pretty(&legacy).unwrap(),
        )
        .unwrap();

        let read = store.read(9).unwrap();
        assert_eq!(read.schema_version, RECORD_SCHEMA_VERSION);
        assert_eq!(read.items, legacy.items);
        assert_eq!(read.checksums, legacy.checksums);
        assert_eq!(read.state, JobState::Completed);

        // Migrated on disk: a header without items, and a journal.
        assert!(journal_of(&store, 9).exists());
        let header =
            fs::read_to_string(directory.path().join("job-00000000000000000009.json")).unwrap();
        assert!(!header.contains("\"items\""));
        assert_eq!(store.read(9).unwrap(), read);
        assert_eq!(store.recover().settled.len(), 1);
    }

    #[test]
    fn a_full_write_compacts_the_journal_to_one_line_per_item() {
        let directory = tempfile::tempdir().unwrap();
        let store = JobStore::new(directory.path());
        let mut written = many(10, 4);
        store.write(&written).unwrap();
        for index in 0..4 {
            store
                .append(10, &[status(index, ItemStatus::Failed)])
                .unwrap();
            store
                .append(10, &[status(index, ItemStatus::Done)])
                .unwrap();
            written.items[index as usize].status = ItemStatus::Done;
        }
        assert_eq!(
            fs::read_to_string(journal_of(&store, 10))
                .unwrap()
                .lines()
                .count(),
            12
        );
        store.write(&written).unwrap();
        assert_eq!(
            fs::read_to_string(journal_of(&store, 10))
                .unwrap()
                .lines()
                .count(),
            4
        );
        assert_eq!(store.read(10).unwrap(), written);
    }

    #[test]
    fn the_cursor_asks_for_a_rewrite_past_a_fixed_multiple_of_the_live_items() {
        let mut cursor = JournalCursor::default();
        // Nothing is on disk yet: the first persist is always whole.
        assert!(cursor.must_rewrite(10));
        cursor.rewritten(10, 0);
        assert!(!cursor.must_rewrite(10));
        cursor.appended((COMPACT_FACTOR * 10 - 10) as usize, 0);
        assert!(!cursor.must_rewrite(10));
        cursor.appended(1, 0);
        assert!(cursor.must_rewrite(10));
        cursor.rewritten(10, 0);
        assert!(!cursor.must_rewrite(10));
        // A failed append leaves the file in an unknown shape.
        cursor.append_failed();
        assert!(cursor.must_rewrite(10));
    }

    #[test]
    fn removing_a_record_removes_its_journal_too() {
        let directory = tempfile::tempdir().unwrap();
        let store = JobStore::new(directory.path());
        store.write(&many(11, 2)).unwrap();
        store.remove(11).unwrap();
        assert!(!journal_of(&store, 11).exists());
        assert!(matches!(store.read(11), Err(StoreError::Io { .. })));
    }
}
