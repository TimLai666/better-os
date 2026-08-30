//! The operation center: `files-operations` jobs, as rows a panel can draw.
//!
//! The engine is the authority on every value here. This module decides only
//! which controls a row offers, and it decides that from the job's own state
//! and its operation kind — `OperationKind::supports_pause` is what makes the
//! Pause button appear, not a list of kinds copied into the GUI that would
//! drift the first time a new operation is added.
//!
//! A control that is offered must work. `JobEngine::pause` answers `false`
//! when a job is not pausable, so offering a button the engine would refuse is
//! a bug this mapping is written to prevent: every predicate here is the same
//! question the engine asks itself.

use std::path::PathBuf;

use files_operations::{
    Conflict, ConflictDecision, JobId, JobSnapshot, JobState, OperationError, OperationKind,
    Progress, RemainingTime, Resolution, ResolutionScope, Throughput,
};

use crate::format;
use crate::i18n::{Copy, confidence_label, conflict_label, job_kind_label, job_state_label};

/// The buttons one job row offers.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct JobControls {
    pub pause: bool,
    pub resume: bool,
    pub cancel: bool,
    pub retry: bool,
}

/// One failed item.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FailureRow {
    pub path: PathBuf,
    /// The stable machine key the error renders as, so a log and a screen
    /// agree about what went wrong.
    pub reason: String,
}

/// A conflict waiting for an answer, with the choices to offer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConflictPrompt {
    pub job: JobId,
    pub title: String,
    pub destination: PathBuf,
    pub source: Option<PathBuf>,
    /// Which resolutions make sense for this conflict. Overwrite is absent
    /// when the conflict is not one an overwrite can settle.
    pub choices: Vec<Resolution>,
}

impl ConflictPrompt {
    pub fn decision(&self, resolution: Resolution, apply_to_remaining: bool) -> ConflictDecision {
        ConflictDecision {
            resolution,
            scope: if apply_to_remaining {
                ResolutionScope::ApplyToRemaining
            } else {
                ResolutionScope::ThisItem
            },
        }
    }
}

/// One job, ready to draw.
#[derive(Clone, Debug, PartialEq)]
pub struct JobRow {
    pub id: JobId,
    pub kind: OperationKind,
    pub state: JobState,
    pub title: String,
    pub state_label: String,
    /// 0.0 to 1.0, or `None` when the job has no measurable total yet. A bar
    /// with no fraction is drawn indeterminate rather than at zero.
    pub fraction: Option<f64>,
    pub items_label: String,
    pub bytes_label: String,
    pub throughput_label: String,
    pub remaining_label: String,
    pub current: Option<PathBuf>,
    pub controls: JobControls,
    pub conflict: Option<ConflictPrompt>,
    pub failures: Vec<FailureRow>,
}

/// Which controls a snapshot's job offers.
///
/// Pause and resume follow the engine's own rule: only an operation that
/// declares `supports_pause` can be paused, and only a paused job can resume.
/// Cancel is offered for anything not finished. Retry is offered only for a
/// job that failed with items to retry, which is the one edge out of a
/// terminal state the engine allows.
pub fn controls_for(snapshot: &JobSnapshot) -> JobControls {
    JobControls {
        pause: snapshot.kind.supports_pause() && snapshot.state == JobState::Running,
        resume: snapshot.state == JobState::Paused,
        cancel: !snapshot.state.is_terminal(),
        retry: snapshot.state == JobState::Failed && !snapshot.failures.is_empty(),
    }
}

/// The resolutions a conflict can be answered with.
///
/// Overwrite is offered only where replacing the destination actually settles
/// the conflict; a full disk is not made emptier by overwriting, so offering
/// it there would be a button that fails.
pub fn choices_for(conflict: &Conflict) -> Vec<Resolution> {
    let mut choices = Vec::new();
    choices.push(Resolution::Skip);
    if conflict.kind.accepts_overwrite() {
        choices.push(Resolution::Overwrite);
        choices.push(Resolution::Rename);
    }
    choices.push(Resolution::Cancel);
    choices
}

/// Maps one snapshot to one row.
pub fn job_row(snapshot: &JobSnapshot, c: &'static Copy) -> JobRow {
    JobRow {
        id: snapshot.id,
        kind: snapshot.kind,
        state: snapshot.state,
        title: job_kind_label(snapshot.kind, c).to_string(),
        state_label: job_state_label(snapshot.state, c).to_string(),
        fraction: fraction_of(&snapshot.progress),
        items_label: items_label(&snapshot.progress),
        bytes_label: bytes_label(&snapshot.progress),
        throughput_label: throughput_label(&snapshot.throughput),
        remaining_label: remaining_label(&snapshot.remaining, c),
        current: snapshot.current.clone(),
        controls: controls_for(snapshot),
        conflict: snapshot.conflict.as_ref().map(|conflict| ConflictPrompt {
            job: snapshot.id,
            title: conflict_label(conflict.kind, c).to_string(),
            destination: conflict.destination.clone(),
            source: conflict.source.clone(),
            choices: choices_for(conflict),
        }),
        failures: snapshot
            .failures
            .iter()
            .map(|(path, error)| FailureRow {
                path: path.clone(),
                reason: reason_key(error),
            })
            .collect(),
    }
}

/// Every job the engine knows about, newest first.
pub fn job_rows(snapshots: &[JobSnapshot], c: &'static Copy) -> Vec<JobRow> {
    let mut rows: Vec<JobRow> = snapshots.iter().map(|s| job_row(s, c)).collect();
    rows.sort_by_key(|row| std::cmp::Reverse(row.id.value()));
    rows
}

/// The active jobs, which is what the toolbar badge counts.
pub fn active_count(snapshots: &[JobSnapshot]) -> usize {
    snapshots
        .iter()
        .filter(|snapshot| !snapshot.state.is_terminal())
        .count()
}

/// Whether anything is waiting for a decision, which is what makes the panel
/// open itself.
pub fn first_conflict(snapshots: &[JobSnapshot], c: &'static Copy) -> Option<ConflictPrompt> {
    snapshots
        .iter()
        .find(|snapshot| snapshot.state == JobState::WaitingOnConflict)
        .and_then(|snapshot| job_row(snapshot, c).conflict)
}

/// The jobs that finished during this session, which the panel keeps as
/// history under the running ones.
#[derive(Clone, Debug, Default)]
pub struct SessionHistory {
    finished: Vec<JobRow>,
}

impl SessionHistory {
    /// Records a job that has reached a terminal state. Recording the same job
    /// twice replaces the earlier entry rather than adding a second row.
    pub fn record(&mut self, row: JobRow) {
        if !row.state.is_terminal() {
            return;
        }
        match self.finished.iter_mut().find(|entry| entry.id == row.id) {
            Some(existing) => *existing = row,
            None => self.finished.push(row),
        }
    }

    /// Newest first.
    pub fn rows(&self) -> Vec<&JobRow> {
        self.finished.iter().rev().collect()
    }

    pub fn len(&self) -> usize {
        self.finished.len()
    }

    pub fn is_empty(&self) -> bool {
        self.finished.is_empty()
    }
}

fn fraction_of(progress: &Progress) -> Option<f64> {
    progress
        .byte_fraction()
        .or_else(|| progress.item_fraction())
}

fn items_label(progress: &Progress) -> String {
    format!("{} / {}", progress.settled_items(), progress.items_total)
}

fn bytes_label(progress: &Progress) -> String {
    if progress.bytes_total == 0 {
        return "—".to_string();
    }
    format!(
        "{} / {}",
        format::bytes(progress.bytes_done),
        format::bytes(progress.bytes_total)
    )
}

fn throughput_label(throughput: &Throughput) -> String {
    match throughput.bytes_per_second {
        Some(rate) => format::bytes_per_second(rate),
        None => match throughput.items_per_second {
            Some(rate) => format!("{rate:.1} /s"),
            None => "—".to_string(),
        },
    }
}

/// The remaining-time readout, always carrying its confidence.
///
/// Issue #6 asks for "realistic remaining-time confidence", so the estimate is
/// never shown bare: a number with no qualifier reads as a promise, and this
/// one is not one.
fn remaining_label(remaining: &RemainingTime, c: &'static Copy) -> String {
    let confidence = confidence_label(remaining.confidence, c);
    match remaining.estimate {
        Some(estimate) => format!("{} · {confidence}", format::duration(estimate)),
        None => confidence.to_string(),
    }
}

/// The machine key an error renders as. `OperationError`'s `Display` is
/// already a stable key, which is what the panel shows next to the path.
fn reason_key(error: &OperationError) -> String {
    error.to_string()
}
