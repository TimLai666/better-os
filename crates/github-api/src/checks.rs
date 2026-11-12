//! CI checks on a commit: check runs and commit statuses combined, and
//! re-running the failed ones.

use serde::Deserialize;
use serde_json::json;

use crate::{ApiError, Client, segment};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CheckState {
    Pending,
    Success,
    Failure,
    /// Neutral, skipped, or cancelled: nothing to fix.
    Neutral,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct CheckSuiteRef {
    pub id: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct CheckRun {
    pub id: u64,
    pub name: String,
    /// `queued`, `in_progress`, `completed`.
    pub status: String,
    #[serde(default)]
    pub conclusion: Option<String>,
    #[serde(default)]
    pub html_url: Option<String>,
    #[serde(default)]
    pub details_url: Option<String>,
    #[serde(default)]
    pub check_suite: Option<CheckSuiteRef>,
}

impl CheckRun {
    pub fn state(&self) -> CheckState {
        if self.status != "completed" {
            return CheckState::Pending;
        }
        match self.conclusion.as_deref() {
            Some("success") => CheckState::Success,
            Some("failure")
            | Some("timed_out")
            | Some("action_required")
            | Some("startup_failure") => CheckState::Failure,
            _ => CheckState::Neutral,
        }
    }

    /// The Actions workflow run this check belongs to, read from its
    /// `details_url` (`.../actions/runs/<id>/job/<id>`).
    pub fn workflow_run(&self) -> Option<u64> {
        let url = self.details_url.as_deref()?;
        let rest = url.split("/actions/runs/").nth(1)?;
        rest.split('/').next()?.parse().ok()
    }
}

#[derive(Deserialize)]
struct CheckRuns {
    check_runs: Vec<CheckRun>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct Status {
    pub id: u64,
    pub context: String,
    /// `pending`, `success`, `failure`, `error`.
    pub state: String,
    #[serde(default)]
    pub target_url: Option<String>,
}

#[derive(Deserialize)]
struct Combined {
    statuses: Vec<Status>,
}

/// Everything CI reported on one commit.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CombinedChecks {
    pub runs: Vec<CheckRun>,
    pub statuses: Vec<Status>,
}

impl CombinedChecks {
    /// The one state the branch shows: failure if anything failed, else
    /// pending if anything is still running, else success if anything
    /// passed; `None` with no checks at all.
    pub fn overall(&self) -> Option<CheckState> {
        let mut states: Vec<CheckState> = self.runs.iter().map(CheckRun::state).collect();
        states.extend(self.statuses.iter().map(|s| match s.state.as_str() {
            "success" => CheckState::Success,
            "failure" | "error" => CheckState::Failure,
            "pending" => CheckState::Pending,
            _ => CheckState::Neutral,
        }));
        if states.is_empty() {
            None
        } else if states.contains(&CheckState::Failure) {
            Some(CheckState::Failure)
        } else if states.contains(&CheckState::Pending) {
            Some(CheckState::Pending)
        } else if states.contains(&CheckState::Success) {
            Some(CheckState::Success)
        } else {
            Some(CheckState::Neutral)
        }
    }

    pub fn failed(&self) -> Vec<&CheckRun> {
        self.runs
            .iter()
            .filter(|r| r.state() == CheckState::Failure)
            .collect()
    }
}

impl Client {
    pub fn checks(
        &self,
        owner: &str,
        name: &str,
        reference: &str,
    ) -> Result<CombinedChecks, ApiError> {
        let base = format!(
            "/repos/{}/{}/commits/{}",
            segment(owner),
            segment(name),
            segment(reference)
        );
        let runs = self.get_pages(
            &format!("{base}/check-runs?per_page=100"),
            |p: CheckRuns| p.check_runs,
        )?;
        let statuses = self.get_pages(&format!("{base}/status?per_page=100"), |p: Combined| {
            p.statuses
        })?;
        Ok(CombinedChecks { runs, statuses })
    }

    /// Re-runs the failed checks: each Actions workflow run's failed jobs
    /// once, and any other app's check suite.
    pub fn rerun_failed(
        &self,
        owner: &str,
        name: &str,
        checks: &CombinedChecks,
    ) -> Result<usize, ApiError> {
        let mut runs: Vec<u64> = Vec::new();
        let mut suites: Vec<u64> = Vec::new();
        for check in checks.failed() {
            match check.workflow_run() {
                Some(run) if !runs.contains(&run) => runs.push(run),
                Some(_) => {}
                None => {
                    if let Some(suite) = &check.check_suite
                        && !suites.contains(&suite.id)
                    {
                        suites.push(suite.id);
                    }
                }
            }
        }
        let (o, n) = (segment(owner), segment(name));
        for run in &runs {
            let _: serde_json::Value = self.post(
                &format!("/repos/{o}/{n}/actions/runs/{run}/rerun-failed-jobs"),
                &json!({}),
            )?;
        }
        for suite in &suites {
            let _: serde_json::Value = self.post(
                &format!("/repos/{o}/{n}/check-suites/{suite}/rerequest"),
                &json!({}),
            )?;
        }
        Ok(runs.len() + suites.len())
    }
}
