//! The per-job operation log.
//!
//! Issue #6 wants two things from this: a failed operation that "retains
//! useful error details and the affected paths", and "enough evidence for
//! Better Monitor to analyze a slow operation later". Both mean the log is a
//! sequence of typed records with timings, not a stream of formatted English.
//!
//! Each record carries the milliseconds since the job started rather than a
//! wall-clock instant, so a log read back after a restart still says how long
//! the third item took even though the clock has moved on.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::conflict::{Conflict, Resolution};
use crate::error::OperationError;
use crate::state::JobState;

/// What happened.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum LogEvent {
    /// The job was accepted, with the plan it will execute.
    Planned {
        items: u64,
        bytes: u64,
    },
    StateChanged {
        from: JobState,
        to: JobState,
    },
    /// An item started. `bytes` is what the item is expected to cost, which is
    /// how a slow item is later distinguished from a big one.
    ItemStarted {
        bytes: u64,
    },
    ItemCompleted {
        bytes: u64,
        verified: bool,
    },
    ItemFailed {
        error: OperationError,
    },
    ItemSkipped {
        reason: SkipReason,
    },
    /// A destination the job created, recorded so a rollback knows exactly
    /// what to remove and never removes anything it did not make.
    Created,
    ConflictRaised {
        conflict: Conflict,
    },
    ConflictResolved {
        resolution: Resolution,
        standing: bool,
    },
    /// A metadata property that could not be carried across. Not a failure:
    /// a destination filesystem without extended attributes is normal, and the
    /// user is told rather than misled.
    MetadataNotCarried {
        property: MetadataProperty,
    },
    /// The move took the rename fast path instead of copying.
    RenameFastPath,
    /// The move fell back to copy, verify, delete because the destination is
    /// on another filesystem.
    CrossDeviceFallback,
    /// The copy reproduced the source's holes rather than writing zeroes.
    SparseRegionsPreserved {
        holes: u64,
    },
    RollbackRemoved,
    Note {
        text: String,
    },
}

/// A metadata property a destination would not take.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MetadataProperty {
    Timestamps,
    Permissions,
    ExtendedAttributes,
    /// Reported when extended attributes were carried but the ACL ones among
    /// them were refused, so an ACL that silently vanished is still on record.
    AccessControlList,
}

/// Why an item was not done.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkipReason {
    /// The user answered "skip" to a conflict.
    ConflictSkipped,
    /// The item vanished between planning and execution. Skipped rather than
    /// failed: a source that is already gone is the outcome the user wanted
    /// from a move or a delete.
    SourceGone,
    /// A retry run that left the already-finished items alone.
    AlreadyDone,
}

/// One line.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LogRecord {
    /// Milliseconds since the job started.
    pub at_ms: u64,
    /// The path this line is about, when it is about one. Stored as raw bytes
    /// so a log entry for a name that is not UTF-8 names the real file.
    #[serde(with = "crate::store::path_bytes_option")]
    pub path: Option<PathBuf>,
    pub event: LogEvent,
}

/// The whole log for one job.
///
/// Bounded on purpose. A 100,000-file copy would otherwise produce 300,000
/// records and several hundred megabytes of them; the cap keeps the head,
/// which explains what the job set out to do, and the tail, which explains
/// what it was doing when it went wrong, and counts what it dropped in
/// between.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct OperationLog {
    head: Vec<LogRecord>,
    tail: std::collections::VecDeque<LogRecord>,
    dropped: u64,
    head_limit: usize,
    tail_limit: usize,
}

impl Default for OperationLog {
    fn default() -> Self {
        Self::with_limits(256, 2048)
    }
}

impl OperationLog {
    pub fn with_limits(head_limit: usize, tail_limit: usize) -> Self {
        Self {
            head: Vec::new(),
            tail: std::collections::VecDeque::new(),
            dropped: 0,
            head_limit,
            tail_limit: tail_limit.max(1),
        }
    }

    pub fn push(&mut self, at_ms: u64, path: Option<PathBuf>, event: LogEvent) {
        let record = LogRecord { at_ms, path, event };
        if self.head.len() < self.head_limit {
            self.head.push(record);
            return;
        }
        if self.tail.len() == self.tail_limit {
            self.tail.pop_front();
            self.dropped += 1;
        }
        self.tail.push_back(record);
    }

    /// Every record still held, oldest first.
    pub fn records(&self) -> Vec<LogRecord> {
        let mut all = self.head.clone();
        all.extend(self.tail.iter().cloned());
        all
    }

    /// How many records the cap discarded. A consumer that shows the log has
    /// to say so rather than presenting a gap as continuity.
    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    pub fn len(&self) -> usize {
        self.head.len() + self.tail.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Every failure the job recorded, with its path. This is the "affected
    /// paths" half of Issue #6's requirement, and it survives the cap: failures
    /// are rare enough that the tail keeps them, and a job with more than 2,048
    /// failures has a bigger problem than its log.
    pub fn failures(&self) -> Vec<(Option<PathBuf>, OperationError)> {
        self.records()
            .into_iter()
            .filter_map(|record| match record.event {
                LogEvent::ItemFailed { error } => Some((record.path, error)),
                _ => None,
            })
            .collect()
    }

    /// Everything the job created, oldest first. A rollback replays this
    /// backwards so a directory is removed after its contents.
    pub fn created_paths(&self) -> Vec<PathBuf> {
        self.records()
            .into_iter()
            .filter_map(|record| match record.event {
                LogEvent::Created => record.path,
                _ => None,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failure_keeps_its_error_and_its_path() {
        let mut log = OperationLog::default();
        log.push(
            12,
            Some("/src/a".into()),
            LogEvent::ItemFailed {
                error: OperationError::PermissionDenied {
                    path: "/src/a".into(),
                },
            },
        );
        let failures = log.failures();
        assert_eq!(failures.len(), 1);
        assert_eq!(
            failures[0].0.as_deref(),
            Some(std::path::Path::new("/src/a"))
        );
        assert_eq!(
            failures[0].1.key(),
            "files.operation.error.permission_denied"
        );
    }

    #[test]
    fn the_cap_keeps_the_start_and_the_end_and_counts_the_middle() {
        let mut log = OperationLog::with_limits(2, 3);
        for step in 0..10u64 {
            log.push(step, None, LogEvent::ItemStarted { bytes: step });
        }
        assert_eq!(log.len(), 5);
        assert_eq!(log.dropped(), 5);
        let records = log.records();
        assert_eq!(records[0].at_ms, 0);
        assert_eq!(records[1].at_ms, 1);
        assert_eq!(records[4].at_ms, 9);
    }

    #[test]
    fn created_paths_come_back_in_the_order_they_were_made() {
        let mut log = OperationLog::default();
        log.push(1, Some("/dst/dir".into()), LogEvent::Created);
        log.push(2, Some("/dst/dir/file".into()), LogEvent::Created);
        assert_eq!(
            log.created_paths(),
            vec![PathBuf::from("/dst/dir"), PathBuf::from("/dst/dir/file")]
        );
    }

    #[test]
    fn a_log_round_trips_through_json_including_a_non_utf8_path() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;
        let mut log = OperationLog::default();
        let path = PathBuf::from(OsStr::from_bytes(b"/dst/\xff\xfe"));
        log.push(3, Some(path.clone()), LogEvent::Created);
        let text = serde_json::to_string(&log).unwrap();
        let back: OperationLog = serde_json::from_str(&text).unwrap();
        assert_eq!(back.created_paths(), vec![path]);
    }
}
