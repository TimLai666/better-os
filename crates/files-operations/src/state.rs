//! The eight job states Issue #6 lists, and the transitions between them.
//!
//! The states are not a status string. Every transition goes through
//! [`JobState::can_transition_to`], so a job cannot move from completed back to
//! running, and a rolled-back job cannot quietly become a completed one. That
//! matters for the recovery story: a state read off disk after a crash is
//! trusted only because the writer could not have written an impossible one.

use serde::{Deserialize, Serialize};

/// Where a job is.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobState {
    /// Accepted and waiting for a worker.
    Queued,
    /// A worker is on it.
    Running,
    /// Stopped at an item or chunk boundary by request; the worker is parked
    /// and holds no file open.
    Paused,
    /// Stopped at a conflict that needs a decision. Distinct from `Paused`
    /// because the user did not ask for this one and the job cannot continue
    /// without an answer.
    WaitingOnConflict,
    /// Every item finished. Items the user chose to skip count as finished;
    /// items that failed do not.
    Completed,
    /// Finished with at least one item unrecovered.
    Failed,
    /// Stopped by request. Nothing partial is left behind: a cancelled copy
    /// removes its temporary destination before it reports this.
    Cancelled,
    /// Stopped, and everything the job had created was removed again. Only
    /// operations with a safe compensating action can reach this state; see
    /// [`crate::policy::FailurePolicy`].
    RolledBack,
}

impl JobState {
    /// Every state, in the order Issue #6 lists them. Used by the state test
    /// so a new variant cannot be added without the test noticing.
    pub const ALL: [JobState; 8] = [
        JobState::Queued,
        JobState::Running,
        JobState::Paused,
        JobState::WaitingOnConflict,
        JobState::Completed,
        JobState::Failed,
        JobState::Cancelled,
        JobState::RolledBack,
    ];

    /// Whether the job has stopped for good. A terminal job never transitions
    /// again except through an explicit retry, which builds a new run.
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            JobState::Completed | JobState::Failed | JobState::Cancelled | JobState::RolledBack
        )
    }

    /// Whether a worker is currently allowed to touch the filesystem for this
    /// job.
    pub fn is_active(self) -> bool {
        matches!(self, JobState::Running)
    }

    /// A stable key, for a consumer keying a translated label.
    pub fn key(self) -> &'static str {
        match self {
            JobState::Queued => "files.job.state.queued",
            JobState::Running => "files.job.state.running",
            JobState::Paused => "files.job.state.paused",
            JobState::WaitingOnConflict => "files.job.state.waiting_on_conflict",
            JobState::Completed => "files.job.state.completed",
            JobState::Failed => "files.job.state.failed",
            JobState::Cancelled => "files.job.state.cancelled",
            JobState::RolledBack => "files.job.state.rolled_back",
        }
    }

    /// Whether this transition is legal.
    ///
    /// Retry is the one edge out of a terminal state, and it only leads back
    /// to `Queued`: the failed items are re-planned rather than resumed from
    /// wherever the previous run happened to stop.
    pub fn can_transition_to(self, next: JobState) -> bool {
        use JobState::*;
        matches!(
            (self, next),
            (Queued, Running | Cancelled | Paused)
                | (
                    Running,
                    Paused | WaitingOnConflict | Completed | Failed | Cancelled | RolledBack
                )
                | (Paused, Running | Cancelled | RolledBack)
                | (
                    WaitingOnConflict,
                    Running | Cancelled | Paused | RolledBack | Failed
                )
                | (Completed | Failed | Cancelled | RolledBack, Queued)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_eight_states_are_representable_and_distinct() {
        assert_eq!(JobState::ALL.len(), 8);
        let mut keys: Vec<&str> = JobState::ALL.iter().map(|state| state.key()).collect();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), 8);
    }

    #[test]
    fn four_states_are_terminal_and_only_running_is_active() {
        let terminal: Vec<_> = JobState::ALL
            .into_iter()
            .filter(|state| state.is_terminal())
            .collect();
        assert_eq!(
            terminal,
            vec![
                JobState::Completed,
                JobState::Failed,
                JobState::Cancelled,
                JobState::RolledBack
            ]
        );
        let active: Vec<_> = JobState::ALL
            .into_iter()
            .filter(|state| state.is_active())
            .collect();
        assert_eq!(active, vec![JobState::Running]);
    }

    #[test]
    fn a_finished_job_cannot_start_running_again_without_a_retry() {
        assert!(!JobState::Completed.can_transition_to(JobState::Running));
        assert!(!JobState::RolledBack.can_transition_to(JobState::Completed));
        assert!(!JobState::Cancelled.can_transition_to(JobState::Running));
        // Retry is the only way out, and it goes back to the start.
        assert!(JobState::Failed.can_transition_to(JobState::Queued));
    }

    #[test]
    fn a_paused_job_resumes_and_a_waiting_job_continues_once_answered() {
        assert!(JobState::Paused.can_transition_to(JobState::Running));
        assert!(JobState::WaitingOnConflict.can_transition_to(JobState::Running));
        assert!(!JobState::Paused.can_transition_to(JobState::Completed));
    }

    #[test]
    fn serialization_round_trips_every_state() {
        for state in JobState::ALL {
            let text = serde_json::to_string(&state).unwrap();
            let back: JobState = serde_json::from_str(&text).unwrap();
            assert_eq!(state, back);
        }
    }
}
