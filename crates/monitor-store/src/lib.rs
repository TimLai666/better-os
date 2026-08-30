//! Durable history for Better Monitor.
//!
//! The service records here; the GUI, the CLI, and the exporter read from
//! here. Nothing in this crate knows what Linux is or what a window looks
//! like, so all of it is testable against a temporary directory.
//!
//! # Why an append log rather than SQLite
//!
//! Issue #16 says the storage engine needs an ADR after benchmarks and that
//! SQLite is a candidate, not a decision. This crate is the interim answer:
//! length-and-checksum framed JSON records in three append-only files, with a
//! downsampler in front of them and a compaction pass behind them. It was
//! chosen because it can be read, recovered, and reasoned about without a
//! dependency, and because the retention window is short enough that the whole
//! of it fits in memory. ADR 0011 records the measurements and states plainly
//! that this is not the final decision.
//!
//! # What the store refuses to lose
//!
//! - The difference between a measured zero and an unobserved metric. Every
//!   reading is stored as a [`monitor_core::Observation`], not as a number.
//! - The fact that a stretch of time has no data. Gaps are records, not
//!   absences.
//! - History that was already written, when a write is interrupted. A torn
//!   final record is truncated and the rest is kept.

pub mod history;
pub mod incident;
pub mod inventory;
pub mod log;
pub mod sample;

use std::io;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub use history::{
    DEFAULT_DISK_BUDGET_BYTES, DEFAULT_MAX_INCIDENTS, DEFAULT_MAX_INVENTORY_RECORDS,
    DEFAULT_RESOLUTION_SECONDS, DEFAULT_RETENTION_SECONDS, HISTORY_FILE_NAME, HistorySlice,
    HistoryStore, INCIDENTS_FILE_NAME, INVENTORY_FILE_NAME, RecoverySummary, RetentionPolicy,
    STORE_SCHEMA_VERSION, StoreRecovery, StoreStats,
};
pub use incident::{
    BaselineShift, DEFAULT_WINDOW_AFTER_SECONDS, DEFAULT_WINDOW_BEFORE_SECONDS, Incident,
    IncidentWindow, MAX_NOTE_LENGTH, MAX_WINDOW_SECONDS, MIN_WINDOW_SECONDS, baseline_shifts,
    sanitize_note,
};
pub use inventory::{
    INVENTORY_SCHEMA_VERSION, Inventory, InventoryChange, InventoryDiff, InventoryEntry,
    Sensitivity, diff as inventory_diff,
};
pub use log::{AppendLog, Recovery, crc32};
pub use sample::{
    CollectorState, CoverageCounts, DEFAULT_TRACKED_PROCESSES, Downsampler, EntitySample, Gap,
    GapReason, HistoryRecord, ProcessSample, Sample,
};

/// A closed wall-clock interval, in milliseconds since the epoch.
///
/// Wall clock rather than monotonic, because a range is what a person picked
/// on a screen. The monotonic timestamp is kept on every sample for the one
/// job it is better at: measuring how far apart two samples really were.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TimeRange {
    pub from_unix_ms: u64,
    pub to_unix_ms: u64,
}

impl Default for TimeRange {
    fn default() -> Self {
        Self::all()
    }
}

impl TimeRange {
    pub fn all() -> Self {
        Self {
            from_unix_ms: 0,
            to_unix_ms: u64::MAX,
        }
    }

    /// The last `seconds` before `now`.
    pub fn last(seconds: u64, now_unix_ms: u64) -> Self {
        Self {
            from_unix_ms: now_unix_ms.saturating_sub(seconds * 1_000),
            to_unix_ms: now_unix_ms,
        }
    }

    pub fn is_valid(&self) -> bool {
        self.from_unix_ms <= self.to_unix_ms
    }

    pub fn contains(&self, unix_ms: u64) -> bool {
        unix_ms >= self.from_unix_ms && unix_ms <= self.to_unix_ms
    }

    pub fn overlaps(&self, from_unix_ms: u64, to_unix_ms: u64) -> bool {
        from_unix_ms <= self.to_unix_ms && to_unix_ms >= self.from_unix_ms
    }

    pub fn duration_ms(&self) -> u64 {
        self.to_unix_ms.saturating_sub(self.from_unix_ms)
    }
}

/// Everything that can go wrong reaching the store.
///
/// Every message is a stable machine key, so a CLI, a GUI, and a log line can
/// all identify the same failure without parsing prose.
#[derive(Debug, Error)]
pub enum StoreError {
    #[error("monitor.store.error.io:{path}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    /// The file exists but was not written by this store. It is never
    /// overwritten, because whatever it is, it is not ours to destroy.
    #[error("monitor.store.error.not_a_log:{path}")]
    NotALog { path: PathBuf },
    #[error("monitor.store.error.unsupported_framing:{version}")]
    UnsupportedFraming { path: PathBuf, version: u32 },
    /// A newer Better Monitor wrote this. It is kept and refused rather than
    /// migrated downwards.
    #[error("monitor.store.error.unsupported_schema:{version}")]
    UnsupportedSchema { path: PathBuf, version: u32 },
    #[error("monitor.store.error.record_too_large:{bytes}:{limit}")]
    RecordTooLarge { bytes: usize, limit: usize },
    #[error("monitor.store.error.serialize")]
    Serialize(#[source] serde_json::Error),
    #[error("monitor.store.error.deserialize")]
    Deserialize(#[source] serde_json::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_range_includes_both_of_its_ends() {
        let range = TimeRange {
            from_unix_ms: 100,
            to_unix_ms: 200,
        };
        assert!(range.contains(100));
        assert!(range.contains(200));
        assert!(!range.contains(99));
        assert!(!range.contains(201));
        assert_eq!(range.duration_ms(), 100);
    }

    #[test]
    fn the_open_range_contains_everything() {
        assert!(TimeRange::all().contains(0));
        assert!(TimeRange::all().contains(u64::MAX));
        assert!(TimeRange::all().is_valid());
    }

    #[test]
    fn a_backwards_range_is_invalid_rather_than_silently_empty() {
        let range = TimeRange {
            from_unix_ms: 500,
            to_unix_ms: 100,
        };
        assert!(!range.is_valid());
    }

    #[test]
    fn the_last_window_saturates_at_the_epoch() {
        let range = TimeRange::last(3_600, 1_000);
        assert_eq!(range.from_unix_ms, 0);
        assert_eq!(range.to_unix_ms, 1_000);
    }

    #[test]
    fn a_gap_that_straddles_the_edge_of_a_range_still_counts_as_overlapping() {
        let range = TimeRange {
            from_unix_ms: 100,
            to_unix_ms: 200,
        };
        assert!(range.overlaps(50, 150));
        assert!(range.overlaps(150, 500));
        assert!(range.overlaps(0, 1_000));
        assert!(!range.overlaps(0, 99));
        assert!(!range.overlaps(201, 300));
    }
}
