//! The history store.
//!
//! Three append logs under one directory: samples and gaps, incidents, and
//! inventory captures. They are separate files because they have different
//! lifetimes — a retention pass that drops yesterday's samples must not drop
//! the incident somebody marked yesterday — and because a torn write in one
//! must not cost the other two.
//!
//! The whole retained window is also held in memory. At the default
//! resolution and window that is a few thousand samples, which is smaller than
//! one collector round of process data, and it is what lets a query answer
//! from RAM instead of re-reading the log. Retention bounds both at once: what
//! is on disk and what is in memory are the same set of records.

use std::path::{Path, PathBuf};

use crate::incident::Incident;
use crate::inventory::{Inventory, InventoryDiff, diff};
use crate::log::{AppendLog, Recovery};
use crate::sample::{CoverageCounts, Gap, GapReason, HistoryRecord, Sample};
use crate::{StoreError, TimeRange};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The record schema for all three logs. One number, because the three files
/// are written and migrated together as one store.
pub const STORE_SCHEMA_VERSION: u32 = 1;

pub const HISTORY_FILE_NAME: &str = "history.log";
pub const INCIDENTS_FILE_NAME: &str = "incidents.log";
pub const INVENTORY_FILE_NAME: &str = "inventory.log";

/// The default retention window.
///
/// Six hours, deliberately short. The product promise is "explain the slowdown
/// that happened while you were away this afternoon", not "keep a month of
/// telemetry", and a short default is the version of that promise a user does
/// not have to opt out of. Issue #16 defers the final number to an ADR; ADR
/// 0008 records this as the interim value with the measurements behind it.
pub const DEFAULT_RETENTION_SECONDS: u64 = 6 * 60 * 60;

/// The default disk budget for the sample log.
pub const DEFAULT_DISK_BUDGET_BYTES: u64 = 64 * 1024 * 1024;

/// The fixed resolution recent data is stored at.
pub const DEFAULT_RESOLUTION_SECONDS: u64 = 5;

pub const DEFAULT_MAX_INCIDENTS: usize = 200;
pub const DEFAULT_MAX_INVENTORY_RECORDS: usize = 64;

/// Everything about how much the store is allowed to keep.
///
/// All four bounds are explicit and none of them defaults to "as much as
/// fits". A monitor that quietly fills a disk is a worse problem than the one
/// it was installed to diagnose.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RetentionPolicy {
    pub window_seconds: u64,
    pub disk_budget_bytes: u64,
    pub resolution_seconds: u64,
    pub max_incidents: usize,
    pub max_inventory_records: usize,
}

impl Default for RetentionPolicy {
    fn default() -> Self {
        Self {
            window_seconds: DEFAULT_RETENTION_SECONDS,
            disk_budget_bytes: DEFAULT_DISK_BUDGET_BYTES,
            resolution_seconds: DEFAULT_RESOLUTION_SECONDS,
            max_incidents: DEFAULT_MAX_INCIDENTS,
            max_inventory_records: DEFAULT_MAX_INVENTORY_RECORDS,
        }
    }
}

/// What opening the store had to repair.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct StoreRecovery {
    pub history: RecoverySummary,
    pub incidents: RecoverySummary,
    pub inventory: RecoverySummary,
}

impl StoreRecovery {
    pub fn recovered_anything(&self) -> bool {
        self.history.truncated_bytes > 0
            || self.incidents.truncated_bytes > 0
            || self.inventory.truncated_bytes > 0
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct RecoverySummary {
    pub records: u64,
    pub truncated_bytes: u64,
}

impl From<Recovery> for RecoverySummary {
    fn from(recovery: Recovery) -> Self {
        Self {
            records: recovery.records,
            truncated_bytes: recovery.truncated_bytes,
        }
    }
}

/// What the store currently holds.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct StoreStats {
    pub samples: u64,
    pub gaps: u64,
    pub incidents: u64,
    pub inventory_records: u64,
    pub bytes_on_disk: u64,
    pub oldest_sample_unix_ms: Option<u64>,
    pub newest_sample_unix_ms: Option<u64>,
}

/// Samples and gaps over one interval, kept together because reading one
/// without the other is how missing data becomes zero.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct HistorySlice {
    pub range: TimeRange,
    pub samples: Vec<Sample>,
    pub gaps: Vec<Gap>,
    /// True when `samples` was cut short by the caller's limit. A chart that
    /// does not know it is looking at a prefix will draw a cliff.
    pub truncated: bool,
}

/// The durable history.
#[derive(Debug)]
pub struct HistoryStore {
    root: PathBuf,
    policy: RetentionPolicy,
    history_log: AppendLog,
    incidents_log: AppendLog,
    inventory_log: AppendLog,
    history: Vec<HistoryRecord>,
    incidents: Vec<Incident>,
    inventory: Vec<Inventory>,
    recovery: StoreRecovery,
    appends_since_compaction: u64,
}

/// How many appends may land before retention is applied.
///
/// Compaction rewrites the log, so doing it on every append would turn a
/// constant-cost write into a linear one. Doing it never would let the file
/// outgrow its budget between long-running compactions. This is the middle:
/// at the default five-second resolution it runs about once an hour.
const COMPACTION_INTERVAL_APPENDS: u64 = 720;

/// Headroom kept back from the disk budget for the gap record a retention pass
/// writes. Generous: one encoded gap is under a hundred bytes.
const GAP_RECORD_ALLOWANCE_BYTES: u64 = 256;

impl HistoryStore {
    /// `$XDG_STATE_HOME/better-monitor`, falling back to `~/.local/state`.
    pub fn default_root() -> PathBuf {
        let base = std::env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .or_else(|| {
                std::env::var_os("HOME")
                    .map(|home| PathBuf::from(home).join(".local").join("state"))
            })
            .unwrap_or_else(|| PathBuf::from("."));
        base.join("better-monitor")
    }

    pub fn open_default(policy: RetentionPolicy) -> Result<Self, StoreError> {
        Self::open(Self::default_root(), policy)
    }

    /// Opens the three logs, recovering any torn tail, and loads the retained
    /// window into memory.
    pub fn open(root: impl Into<PathBuf>, policy: RetentionPolicy) -> Result<Self, StoreError> {
        let root = root.into();
        std::fs::create_dir_all(&root).map_err(|source| StoreError::Io {
            path: root.clone(),
            source,
        })?;

        let (mut history_log, history_recovery) =
            AppendLog::open(root.join(HISTORY_FILE_NAME), STORE_SCHEMA_VERSION)?;
        let (mut incidents_log, incidents_recovery) =
            AppendLog::open(root.join(INCIDENTS_FILE_NAME), STORE_SCHEMA_VERSION)?;
        let (mut inventory_log, inventory_recovery) =
            AppendLog::open(root.join(INVENTORY_FILE_NAME), STORE_SCHEMA_VERSION)?;

        let mut history = history_log.read_all::<HistoryRecord>(STORE_SCHEMA_VERSION)?;
        let incidents = incidents_log.read_all::<Incident>(STORE_SCHEMA_VERSION)?;
        let inventory = inventory_log.read_all::<Inventory>(STORE_SCHEMA_VERSION)?;

        // A torn tail is a hole in the timeline, so it is recorded as one
        // rather than left to look like a quiet period.
        if history_recovery.truncated_bytes > 0
            && let Some(last) = history.last().map(HistoryRecord::wall_unix_ms)
        {
            let gap = Gap {
                from_unix_ms: last,
                to_unix_ms: last,
                reason: GapReason::InterruptedWrite,
            };
            history_log.append(&HistoryRecord::Gap(gap))?;
            history.push(HistoryRecord::Gap(gap));
        }

        let mut store = Self {
            root,
            policy,
            history_log,
            incidents_log,
            inventory_log,
            history,
            incidents,
            inventory,
            recovery: StoreRecovery {
                history: history_recovery.into(),
                incidents: incidents_recovery.into(),
                inventory: inventory_recovery.into(),
            },
            appends_since_compaction: 0,
        };
        store.compact()?;
        Ok(store)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn policy(&self) -> RetentionPolicy {
        self.policy
    }

    pub fn recovery(&self) -> StoreRecovery {
        self.recovery
    }

    pub fn schema_version(&self) -> u32 {
        self.history_log.schema_version()
    }

    /// The wall-clock time of the newest stored sample.
    pub fn newest_sample_unix_ms(&self) -> Option<u64> {
        self.history.iter().rev().find_map(|record| match record {
            HistoryRecord::Sample(sample) => Some(sample.wall_unix_ms),
            HistoryRecord::Gap(_) => None,
        })
    }

    pub fn oldest_sample_unix_ms(&self) -> Option<u64> {
        self.history.iter().find_map(|record| match record {
            HistoryRecord::Sample(sample) => Some(sample.wall_unix_ms),
            HistoryRecord::Gap(_) => None,
        })
    }

    /// Append one sample, then apply retention if it is due.
    pub fn record_sample(&mut self, sample: Sample) -> Result<(), StoreError> {
        let record = HistoryRecord::Sample(Box::new(sample));
        self.history_log.append(&record)?;
        self.history.push(record);
        self.after_append()
    }

    /// Record that a stretch of time has no samples in it.
    pub fn record_gap(&mut self, gap: Gap) -> Result<(), StoreError> {
        let record = HistoryRecord::Gap(gap);
        self.history_log.append(&record)?;
        self.history.push(record);
        self.after_append()
    }

    /// The next incident identifier. Monotonic across restarts because it is
    /// derived from what is stored rather than from a counter in memory.
    pub fn next_incident_id(&self) -> u64 {
        self.incidents
            .iter()
            .map(|incident| incident.id)
            .max()
            .unwrap_or(0)
            + 1
    }

    pub fn record_incident(&mut self, incident: Incident) -> Result<(), StoreError> {
        self.incidents_log.append(&incident)?;
        self.incidents.push(incident);
        if self.incidents.len() > self.policy.max_incidents {
            let keep = self.policy.max_incidents;
            let kept = self.incidents.split_off(self.incidents.len() - keep);
            self.incidents = kept;
            self.incidents_log
                .rewrite(&self.incidents, STORE_SCHEMA_VERSION)?;
        }
        Ok(())
    }

    /// Store an inventory capture, but only when it differs from the last one.
    /// Returns whether anything was written.
    pub fn record_inventory(&mut self, inventory: Inventory) -> Result<bool, StoreError> {
        if let Some(latest) = self.inventory.last()
            && !inventory.differs_from(latest)
        {
            return Ok(false);
        }
        self.inventory_log.append(&inventory)?;
        self.inventory.push(inventory);
        if self.inventory.len() > self.policy.max_inventory_records {
            let keep = self.policy.max_inventory_records;
            let kept = self.inventory.split_off(self.inventory.len() - keep);
            self.inventory = kept;
            self.inventory_log
                .rewrite(&self.inventory, STORE_SCHEMA_VERSION)?;
        }
        Ok(true)
    }

    pub fn incidents(&self) -> &[Incident] {
        &self.incidents
    }

    pub fn incident(&self, id: u64) -> Option<&Incident> {
        self.incidents.iter().find(|incident| incident.id == id)
    }

    /// An incident together with the history around it.
    pub fn incident_window(&self, id: u64) -> Option<(&Incident, HistorySlice)> {
        let incident = self.incident(id)?;
        let (from, to) = incident.range_unix_ms();
        let slice = self.slice(
            TimeRange {
                from_unix_ms: from,
                to_unix_ms: to,
            },
            usize::MAX,
        );
        Some((incident, slice))
    }

    pub fn inventory_records(&self) -> &[Inventory] {
        &self.inventory
    }

    pub fn latest_inventory(&self) -> Option<&Inventory> {
        self.inventory.last()
    }

    /// The change between the two most recent captures. `None` when there has
    /// only ever been one, which is not the same as "nothing changed".
    pub fn latest_inventory_diff(&self) -> Option<InventoryDiff> {
        let count = self.inventory.len();
        if count < 2 {
            return None;
        }
        Some(diff(&self.inventory[count - 2], &self.inventory[count - 1]))
    }

    /// Samples and gaps over an interval, newest last, capped at `limit`
    /// samples.
    pub fn slice(&self, range: TimeRange, limit: usize) -> HistorySlice {
        let mut samples = Vec::new();
        let mut gaps = Vec::new();
        let mut truncated = false;
        for record in &self.history {
            match record {
                HistoryRecord::Sample(sample) if range.contains(sample.wall_unix_ms) => {
                    if samples.len() >= limit {
                        truncated = true;
                        continue;
                    }
                    samples.push(sample.as_ref().clone());
                }
                HistoryRecord::Gap(gap) if range.overlaps(gap.from_unix_ms, gap.to_unix_ms) => {
                    gaps.push(*gap);
                }
                _ => {}
            }
        }
        HistorySlice {
            range,
            samples,
            gaps,
            truncated,
        }
    }

    /// Per-metric observation coverage over an interval.
    pub fn coverage(&self, range: TimeRange) -> BTreeMap<String, CoverageCounts> {
        let mut coverage: BTreeMap<String, CoverageCounts> = BTreeMap::new();
        for record in &self.history {
            let HistoryRecord::Sample(sample) = record else {
                continue;
            };
            if !range.contains(sample.wall_unix_ms) {
                continue;
            }
            for (id, observation) in sample.observations() {
                coverage
                    .entry(id.to_string())
                    .or_default()
                    .record(observation.state());
            }
        }
        coverage
    }

    pub fn stats(&self) -> StoreStats {
        let samples = self
            .history
            .iter()
            .filter(|record| matches!(record, HistoryRecord::Sample(_)))
            .count() as u64;
        StoreStats {
            samples,
            gaps: self.history.len() as u64 - samples,
            incidents: self.incidents.len() as u64,
            inventory_records: self.inventory.len() as u64,
            bytes_on_disk: self.history_log.bytes()
                + self.incidents_log.bytes()
                + self.inventory_log.bytes(),
            oldest_sample_unix_ms: self.oldest_sample_unix_ms(),
            newest_sample_unix_ms: self.newest_sample_unix_ms(),
        }
    }

    /// Force retention now. The service calls this on a clean shutdown so the
    /// file it leaves behind is already inside its budget.
    pub fn flush(&mut self) -> Result<(), StoreError> {
        self.compact()
    }

    fn after_append(&mut self) -> Result<(), StoreError> {
        self.appends_since_compaction += 1;
        let over_budget = self.history_log.bytes() > self.policy.disk_budget_bytes;
        if over_budget || self.appends_since_compaction >= COMPACTION_INTERVAL_APPENDS {
            self.compact()?;
        }
        Ok(())
    }

    /// Drop everything outside the window, then drop oldest-first until the
    /// file fits its budget.
    fn compact(&mut self) -> Result<(), StoreError> {
        self.appends_since_compaction = 0;
        let Some(newest) = self.history.last().map(HistoryRecord::wall_unix_ms) else {
            return Ok(());
        };
        let cutoff = newest.saturating_sub(self.policy.window_seconds * 1_000);
        let before = self.history.len();
        let oldest = self
            .history
            .first()
            .map(HistoryRecord::wall_unix_ms)
            .unwrap_or(cutoff);
        self.history
            .retain(|record| record.wall_unix_ms() >= cutoff);

        // Everything dropped so far is one hole in the timeline. The record of
        // it goes in before the budget is measured, because the gap record
        // costs bytes too, and a store that overran its budget by exactly the
        // size of its own bookkeeping would still have overrun it.
        let dropped_by_window = self.history.len() != before;
        if dropped_by_window {
            self.history.insert(
                0,
                HistoryRecord::Gap(Gap {
                    from_unix_ms: oldest,
                    to_unix_ms: self
                        .history
                        .first()
                        .map(HistoryRecord::wall_unix_ms)
                        .unwrap_or(newest),
                    reason: GapReason::Retention,
                }),
            );
        }

        // Budget is enforced on the encoded size, oldest first, keeping any
        // leading retention gap and widening it over what it now covers.
        let sizes: Vec<u64> = self
            .history
            .iter()
            .map(|record| {
                serde_json::to_vec(record)
                    .map(|bytes| bytes.len() as u64 + 8)
                    .unwrap_or(0)
            })
            .collect();
        let first_droppable = usize::from(dropped_by_window);
        // Room for the gap record this pass may still have to add.
        let budget = self
            .policy
            .disk_budget_bytes
            .saturating_sub(GAP_RECORD_ALLOWANCE_BYTES);
        let mut total: u64 = sizes.iter().sum::<u64>() + crate::log::HEADER_BYTES;
        let mut drop_to = first_droppable;
        while total > budget && drop_to + 1 < self.history.len() {
            total -= sizes[drop_to];
            drop_to += 1;
        }
        if drop_to > first_droppable {
            let covered_to = self.history[drop_to].wall_unix_ms();
            let covered_from = self.history[first_droppable].wall_unix_ms();
            self.history.drain(first_droppable..drop_to);
            if dropped_by_window {
                if let HistoryRecord::Gap(gap) = &mut self.history[0] {
                    gap.to_unix_ms = covered_to;
                }
            } else {
                self.history.insert(
                    0,
                    HistoryRecord::Gap(Gap {
                        from_unix_ms: covered_from,
                        to_unix_ms: covered_to,
                        reason: GapReason::Retention,
                    }),
                );
            }
        }

        if !dropped_by_window && drop_to == first_droppable {
            return Ok(());
        }

        self.history_log
            .rewrite(&self.history, STORE_SCHEMA_VERSION)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::incident::{IncidentWindow, baseline_shifts};
    use crate::inventory::InventoryEntry;
    use monitor_core::{CollectorId, CollectorReport, MetricId, Observation, Timestamp};

    fn metric(raw: &str) -> MetricId {
        MetricId::new(raw).unwrap()
    }

    fn sample(at_ms: u64) -> Sample {
        let mut report = CollectorReport::new(
            CollectorId::new("linux.cpu").unwrap(),
            Timestamp {
                unix_ms: at_ms,
                monotonic_ns: at_ms * 1_000_000,
            },
        );
        report.metrics.insert(
            metric("cpu.utilization.busy"),
            Observation::float((at_ms % 100) as f64 / 100.0),
        );
        Sample::from_reports(&[report], 4)
    }

    fn store(policy: RetentionPolicy) -> (tempfile::TempDir, HistoryStore) {
        let directory = tempfile::tempdir().unwrap();
        let store = HistoryStore::open(directory.path(), policy).unwrap();
        (directory, store)
    }

    #[test]
    fn samples_survive_a_close_and_reopen() {
        let directory = tempfile::tempdir().unwrap();
        {
            let mut store =
                HistoryStore::open(directory.path(), RetentionPolicy::default()).unwrap();
            for index in 0..10 {
                store.record_sample(sample(1_000 + index * 5_000)).unwrap();
            }
            store.flush().unwrap();
        }
        let store = HistoryStore::open(directory.path(), RetentionPolicy::default()).unwrap();
        assert_eq!(store.stats().samples, 10);
        assert_eq!(store.oldest_sample_unix_ms(), Some(1_000));
        assert_eq!(store.newest_sample_unix_ms(), Some(46_000));
        assert!(!store.recovery().recovered_anything());
    }

    #[test]
    fn a_torn_tail_is_recovered_and_recorded_as_a_gap() {
        let directory = tempfile::tempdir().unwrap();
        {
            let mut store =
                HistoryStore::open(directory.path(), RetentionPolicy::default()).unwrap();
            for index in 0..6 {
                store.record_sample(sample(1_000 + index * 5_000)).unwrap();
            }
        }
        // Cut the last record in half, as a power loss during an append would.
        let path = directory.path().join(HISTORY_FILE_NAME);
        let length = std::fs::metadata(&path).unwrap().len();
        std::fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .unwrap()
            .set_len(length - 20)
            .unwrap();

        let store = HistoryStore::open(directory.path(), RetentionPolicy::default()).unwrap();
        assert_eq!(store.stats().samples, 5);
        assert!(store.recovery().history.truncated_bytes > 0);
        let slice = store.slice(TimeRange::all(), usize::MAX);
        assert_eq!(slice.gaps.len(), 1);
        assert_eq!(slice.gaps[0].reason, GapReason::InterruptedWrite);
    }

    #[test]
    fn retention_drops_what_falls_out_of_the_window_and_says_so() {
        let policy = RetentionPolicy {
            window_seconds: 30,
            ..RetentionPolicy::default()
        };
        let (_directory, mut store) = store(policy);
        for index in 0..20u64 {
            store.record_sample(sample(index * 5_000)).unwrap();
        }
        store.flush().unwrap();

        let stats = store.stats();
        assert!(stats.samples <= 7, "kept {} samples", stats.samples);
        assert!(stats.oldest_sample_unix_ms.unwrap() >= 95_000 - 30_000);
        let slice = store.slice(TimeRange::all(), usize::MAX);
        assert!(
            slice
                .gaps
                .iter()
                .any(|gap| gap.reason == GapReason::Retention),
            "dropping history must leave a gap behind"
        );
    }

    #[test]
    fn the_disk_budget_is_enforced_even_inside_the_window() {
        let policy = RetentionPolicy {
            window_seconds: 86_400,
            disk_budget_bytes: 4_096,
            ..RetentionPolicy::default()
        };
        let (directory, mut store) = store(policy);
        for index in 0..400u64 {
            store.record_sample(sample(index * 1_000)).unwrap();
        }
        store.flush().unwrap();

        let on_disk = std::fs::metadata(directory.path().join(HISTORY_FILE_NAME))
            .unwrap()
            .len();
        assert!(on_disk <= 4_096, "history log grew to {on_disk} bytes");
        assert!(
            store.stats().samples > 0,
            "the budget must not empty the store"
        );
    }

    #[test]
    fn a_range_query_returns_only_what_falls_inside_it() {
        let (_directory, mut store) = store(RetentionPolicy::default());
        for index in 0..20u64 {
            store.record_sample(sample(index * 1_000)).unwrap();
        }
        let slice = store.slice(
            TimeRange {
                from_unix_ms: 5_000,
                to_unix_ms: 9_000,
            },
            usize::MAX,
        );
        assert_eq!(slice.samples.len(), 5);
        assert_eq!(slice.samples[0].wall_unix_ms, 5_000);
        assert_eq!(slice.samples[4].wall_unix_ms, 9_000);
        assert!(!slice.truncated);
    }

    #[test]
    fn a_capped_query_says_it_was_capped() {
        let (_directory, mut store) = store(RetentionPolicy::default());
        for index in 0..20u64 {
            store.record_sample(sample(index * 1_000)).unwrap();
        }
        let slice = store.slice(TimeRange::all(), 5);
        assert_eq!(slice.samples.len(), 5);
        assert!(slice.truncated);
    }

    #[test]
    fn coverage_counts_the_states_the_range_actually_recorded() {
        let (_directory, mut store) = store(RetentionPolicy::default());
        for index in 0..4u64 {
            store.record_sample(sample(index * 1_000)).unwrap();
        }
        let coverage = store.coverage(TimeRange::all());
        assert_eq!(coverage["cpu.utilization.busy"].value, 4);
        assert_eq!(coverage["cpu.utilization.busy"].unsupported, 0);
    }

    #[test]
    fn incidents_are_kept_when_the_samples_around_them_are_dropped() {
        let policy = RetentionPolicy {
            window_seconds: 10,
            ..RetentionPolicy::default()
        };
        let (_directory, mut store) = store(policy);
        let marker = sample(1_000);
        store
            .record_incident(Incident {
                id: store.next_incident_id(),
                marked_at_unix_ms: 1_000,
                monotonic_ns: 1_000_000_000,
                note: Some("slow".into()),
                window: IncidentWindow::default(),
                baseline: baseline_shifts(&marker, &[]),
                snapshot: Box::new(marker),
                about_pid: None,
            })
            .unwrap();
        for index in 0..40u64 {
            store.record_sample(sample(10_000 + index * 5_000)).unwrap();
        }
        store.flush().unwrap();
        assert_eq!(store.incidents().len(), 1);
        assert_eq!(store.incidents()[0].note.as_deref(), Some("slow"));
    }

    #[test]
    fn an_incident_identifier_keeps_climbing_across_a_restart() {
        let directory = tempfile::tempdir().unwrap();
        {
            let mut store =
                HistoryStore::open(directory.path(), RetentionPolicy::default()).unwrap();
            let marker = sample(1_000);
            assert_eq!(store.next_incident_id(), 1);
            store
                .record_incident(Incident {
                    id: 1,
                    marked_at_unix_ms: 1_000,
                    monotonic_ns: 0,
                    note: None,
                    window: IncidentWindow::default(),
                    baseline: Default::default(),
                    snapshot: Box::new(marker),
                    about_pid: None,
                })
                .unwrap();
        }
        let store = HistoryStore::open(directory.path(), RetentionPolicy::default()).unwrap();
        assert_eq!(store.next_incident_id(), 2);
    }

    #[test]
    fn an_incident_window_returns_the_samples_around_the_marker() {
        let (_directory, mut store) = store(RetentionPolicy::default());
        for index in 0..40u64 {
            store.record_sample(sample(index * 5_000)).unwrap();
        }
        let marker = sample(100_000);
        store
            .record_incident(Incident {
                id: 1,
                marked_at_unix_ms: 100_000,
                monotonic_ns: 0,
                note: None,
                window: IncidentWindow {
                    before_seconds: 20,
                    after_seconds: 20,
                },
                baseline: Default::default(),
                snapshot: Box::new(marker),
                about_pid: None,
            })
            .unwrap();
        let (incident, slice) = store.incident_window(1).unwrap();
        assert_eq!(incident.id, 1);
        // 80s..120s at one sample every 5s, and the store only holds up to
        // 195_000, so the tail of the window is simply empty.
        assert!(slice.samples.iter().all(|s| s.wall_unix_ms >= 80_000));
        assert!(slice.samples.iter().all(|s| s.wall_unix_ms <= 120_000));
        assert_eq!(slice.samples.len(), 9);
    }

    #[test]
    fn an_unchanged_inventory_is_not_written_twice() {
        let (_directory, mut store) = store(RetentionPolicy::default());
        let mut first = Inventory::new(1_000);
        first.insert("os.name", InventoryEntry::public("Zorin OS 18"));
        assert!(store.record_inventory(first.clone()).unwrap());

        let mut same = first.clone();
        same.captured_at_unix_ms = 2_000;
        assert!(!store.record_inventory(same).unwrap());
        assert_eq!(store.inventory_records().len(), 1);

        let mut changed = first;
        changed.captured_at_unix_ms = 3_000;
        changed.insert("os.name", InventoryEntry::public("Zorin OS 19"));
        assert!(store.record_inventory(changed).unwrap());
        assert_eq!(store.inventory_records().len(), 2);

        let latest = store.latest_inventory_diff().unwrap();
        assert_eq!(latest.changed.len(), 1);
        assert_eq!(latest.changed[0].key, "os.name");
    }

    #[test]
    fn one_inventory_capture_has_nothing_to_diff_against() {
        let (_directory, mut store) = store(RetentionPolicy::default());
        let mut only = Inventory::new(1_000);
        only.insert("os.name", InventoryEntry::public("Zorin OS 18"));
        store.record_inventory(only).unwrap();
        assert!(store.latest_inventory_diff().is_none());
    }

    #[test]
    fn incident_and_inventory_records_are_bounded() {
        let policy = RetentionPolicy {
            max_incidents: 3,
            max_inventory_records: 2,
            ..RetentionPolicy::default()
        };
        let (_directory, mut store) = store(policy);
        for index in 1..=6u64 {
            store
                .record_incident(Incident {
                    id: index,
                    marked_at_unix_ms: index * 1_000,
                    monotonic_ns: 0,
                    note: None,
                    window: IncidentWindow::default(),
                    baseline: Default::default(),
                    snapshot: Box::new(sample(index * 1_000)),
                    about_pid: None,
                })
                .unwrap();
            let mut inventory = Inventory::new(index * 1_000);
            inventory.insert(
                "kernel.release",
                InventoryEntry::public(format!("6.{index}")),
            );
            store.record_inventory(inventory).unwrap();
        }
        assert_eq!(store.incidents().len(), 3);
        assert_eq!(store.incidents()[0].id, 4);
        assert_eq!(store.inventory_records().len(), 2);
    }

    #[test]
    fn a_store_written_by_a_newer_better_monitor_is_refused_not_reset() {
        let directory = tempfile::tempdir().unwrap();
        let mut newer = AppendLog::open(
            directory.path().join(HISTORY_FILE_NAME),
            STORE_SCHEMA_VERSION + 5,
        )
        .unwrap()
        .0;
        newer
            .append(&HistoryRecord::Sample(Box::new(sample(1))))
            .unwrap();
        drop(newer);

        let error = HistoryStore::open(directory.path(), RetentionPolicy::default()).unwrap_err();
        assert!(matches!(error, StoreError::UnsupportedSchema { .. }));
        assert!(directory.path().join(HISTORY_FILE_NAME).exists());
    }

    #[test]
    fn the_default_retention_window_is_short_by_design() {
        let policy = RetentionPolicy::default();
        assert_eq!(policy.window_seconds, 6 * 60 * 60);
        assert!(policy.disk_budget_bytes <= 128 * 1024 * 1024);
        assert_eq!(policy.resolution_seconds, 5);
    }

    #[test]
    fn a_default_root_stays_inside_the_users_own_state_directory() {
        assert!(HistoryStore::default_root().ends_with("better-monitor"));
    }
}
