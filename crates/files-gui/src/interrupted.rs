//! Jobs a previous Better Files left unfinished because its process died.
//!
//! The job engine writes a record for every job as it runs (ticket 33), and
//! `JobStore::recover` settles the ones whose process is gone. This is the
//! window's side of that: the copies and moves it can start again are
//! offered in the operation center, each with Resume and Dismiss, until the
//! person picks one. Resuming hands the record back to the engine, which
//! leaves the finished items alone and keeps the answers given about
//! folders; dismissing removes the record.

use std::sync::{Arc, Mutex};

use files_operations::{
    ItemStatus, JobEngine, JobHandle, JobRecord, JobState, JobStore, OperationError, Recovery,
};

/// The interrupted jobs this process offers, shared by every window so a
/// job resumed or dismissed in one is gone from all of them.
#[derive(Clone, Default)]
pub struct InterruptedJobs {
    records: Arc<Mutex<Vec<JobRecord>>>,
    store: Option<JobStore>,
}

/// One offered job, as the window shows it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InterruptedJob {
    pub id: u64,
    pub moving: bool,
    /// The folder it was copying or moving into.
    pub destination: std::path::PathBuf,
    /// Items it had not finished.
    pub remaining: usize,
}

impl InterruptedJobs {
    /// What a recovery found that can be started again: copies and moves
    /// whose process died (this start, or an earlier one that was not
    /// answered), in job order.
    pub fn from_recovery(recovery: Recovery, store: Option<JobStore>) -> Self {
        let mut records: Vec<JobRecord> = recovery
            .interrupted
            .into_iter()
            .chain(recovery.settled.into_iter().filter(was_interrupted))
            .filter(|record| record.resume.is_some())
            .collect();
        records.sort_by_key(|record| record.id);
        Self {
            records: Arc::new(Mutex::new(records)),
            store,
        }
    }

    pub fn list(&self) -> Vec<InterruptedJob> {
        self.records
            .lock()
            .map(|records| {
                records
                    .iter()
                    .filter_map(|record| {
                        let resume = record.resume.as_ref()?;
                        Some(InterruptedJob {
                            id: record.id,
                            moving: resume.moving,
                            destination: resume.destination.clone(),
                            remaining: record.remaining().len(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn is_empty(&self) -> bool {
        self.records
            .lock()
            .map(|records| records.is_empty())
            .unwrap_or(true)
    }

    /// Starts job `id` again on `engine`. It leaves the list whether or not
    /// the engine accepts it; a refusal is returned.
    pub fn resume(&self, id: u64, engine: &JobEngine) -> Option<Result<JobHandle, OperationError>> {
        let record = self.take(id)?;
        Some(engine.resume_interrupted(&record))
    }

    /// Forgets job `id`: its record is removed, and it is not offered again.
    pub fn dismiss(&self, id: u64) {
        if self.take(id).is_some()
            && let Some(store) = &self.store
        {
            let _ = store.remove(id);
        }
    }

    fn take(&self, id: u64) -> Option<JobRecord> {
        let mut records = self.records.lock().ok()?;
        let index = records.iter().position(|record| record.id == id)?;
        Some(records.remove(index))
    }
}

/// A record recovery settled earlier: failed, with work it never got to
/// because its process died.
fn was_interrupted(record: &JobRecord) -> bool {
    record.state == JobState::Failed
        && record.items.iter().any(|item| {
            item.status == ItemStatus::Pending
                && matches!(item.error, Some(OperationError::Interrupted))
        })
}
