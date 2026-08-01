//! Carrying out a transaction, and undoing it when a step fails.
//!
//! The ordering here is the part that matters. A rollback record for a
//! component is written immediately before the first APT call that touches it
//! and never earlier, so a transaction refused during revalidation leaves no
//! restore point behind. Once something has been applied, a failure walks the
//! recorded steps backwards and reports honestly how far it got: fully
//! restored, partially restored, or needing a person.

use std::sync::Arc;

use manager_ipc::{
    ExecutionStage, HealthResult, OutcomeStatus, PROTOCOL_VERSION, RollbackRecord, StepReport,
    TransactionOutcome, WireAction, WirePlan, WireRecovery, WireStep,
};

use crate::apt::AptDriver;
use crate::health::{self, HealthProbe};
use crate::host::HostProbe;
use crate::store::{ArtifactStore, Journal, JournalState};
use crate::{DaemonError, revalidate};

/// Reported as a step moves, so a client can show progress without polling.
pub type ProgressSink<'a> = dyn FnMut(u32, ExecutionStage) + Send + 'a;

pub struct Executor {
    pub apt: Arc<dyn AptDriver>,
    pub host: Arc<dyn HostProbe>,
    pub health: Arc<dyn HealthProbe>,
    pub artifacts: Arc<ArtifactStore>,
    pub journal: Arc<Journal>,
}

impl Executor {
    /// Runs a plan end to end.
    ///
    /// Returns an outcome rather than an error for anything that happened after
    /// the plan was accepted: the caller needs the reports and rollback records
    /// even — especially — when a step failed.
    pub fn execute(
        &self,
        plan: &WirePlan,
        progress: &mut ProgressSink<'_>,
    ) -> Result<TransactionOutcome, DaemonError> {
        let host = self.host.facts()?;

        // Everything checkable before touching the host. A refusal here writes
        // no rollback record, because nothing has changed to roll back.
        revalidate::check_plan(plan, &host)?;
        self.journal
            .set_state(&plan.transaction_id, JournalState::Validated)?;

        for step in &plan.steps {
            revalidate::check_no_drift(step, self.apt.as_ref())?;
        }

        let mut reports: Vec<StepReport> = Vec::new();
        let mut rollback_records: Vec<RollbackRecord> = Vec::new();

        for (index, step) in plan.steps.iter().enumerate() {
            let step_index = index as u32;
            self.journal
                .set_state(&plan.transaction_id, JournalState::Executing { step_index })?;

            progress(step_index, ExecutionStage::Verifying);
            if let Err(error) =
                revalidate::check_artifact(step, &host, &self.artifacts, self.apt.as_ref())
            {
                // Still before this step's mutation. Anything applied by an
                // earlier step does have to come back.
                return self.finish_failed(
                    plan,
                    reports,
                    rollback_records,
                    Some(step_index),
                    error,
                );
            }

            // The point of no return for this component: record how to undo it
            // before doing it.
            let record = self.rollback_record_for(plan, step)?;
            self.journal.write_rollback(&record)?;
            rollback_records.push(record);

            progress(step_index, ExecutionStage::Applying);
            let run = self.apply(step)?;
            let lock_contention = run.is_lock_contention();
            let succeeded = run.succeeded();
            let log = vec![run.into_log_entry()];

            if !succeeded {
                reports.push(StepReport {
                    component: step.component.clone(),
                    action: step.action,
                    applied_version: self.apt.installed_version(&step.component)?,
                    health: HealthResult::Undetermined("the package change failed".to_string()),
                    log,
                });
                let error = if lock_contention {
                    DaemonError::AptBusy
                } else {
                    DaemonError::AptFailed {
                        component: step.component.clone(),
                    }
                };
                return self.finish_failed(
                    plan,
                    reports,
                    rollback_records,
                    Some(step_index),
                    error,
                );
            }

            progress(step_index, ExecutionStage::CheckingHealth);
            let health = health::check(
                &step.component,
                step.action,
                self.apt.as_ref(),
                self.health.as_ref(),
            );
            let applied_version = self.apt.installed_version(&step.component)?;
            let healthy = matches!(health, HealthResult::Healthy);
            reports.push(StepReport {
                component: step.component.clone(),
                action: step.action,
                applied_version,
                health,
                log,
            });

            if !healthy {
                return self.finish_failed(
                    plan,
                    reports,
                    rollback_records,
                    Some(step_index),
                    DaemonError::HealthFailed {
                        component: step.component.clone(),
                    },
                );
            }
        }

        let outcome = TransactionOutcome {
            protocol_version: PROTOCOL_VERSION,
            transaction_id: plan.transaction_id.clone(),
            status: OutcomeStatus::Succeeded,
            reports,
            rollback_records,
        };
        self.journal.complete(&outcome)?;
        Ok(outcome)
    }

    fn apply(&self, step: &WireStep) -> Result<crate::apt::AptRun, DaemonError> {
        match step.action {
            WireAction::Remove => self.apt.remove(&step.component),
            action => {
                let artifact = step.artifact.as_ref().ok_or(DaemonError::ArtifactMissing {
                    component: step.component.clone(),
                })?;
                let path = self.artifacts.path_for(&artifact.filename)?;
                self.apt
                    .install_local_deb(&path, revalidate::allows_downgrade(action))
            }
        }
    }

    /// What it would take to undo this step, or an honest statement that the
    /// component was not installed before.
    fn rollback_record_for(
        &self,
        plan: &WirePlan,
        step: &WireStep,
    ) -> Result<RollbackRecord, DaemonError> {
        let previous_version = self.apt.installed_version(&step.component)?;
        // Only a version we can actually reinstall counts. A previously
        // installed version whose .deb is no longer cached is recorded as a
        // version without an artifact, so a rollback attempt reports that it
        // needs a person rather than silently doing nothing.
        let previous_artifact = self
            .journal
            .read_rollback(&step.component)
            .ok()
            .flatten()
            .and_then(|earlier| earlier.previous_artifact)
            .filter(|artifact| self.artifacts.contains(&artifact.filename))
            .or_else(|| {
                step.artifact
                    .as_ref()
                    .filter(|_| previous_version.is_some())
                    .filter(|artifact| self.artifacts.contains(&artifact.filename))
                    .cloned()
            });

        Ok(RollbackRecord {
            component: step.component.clone(),
            previous_version,
            previous_artifact,
            transaction_id: plan.transaction_id.clone(),
            recorded_at_unix: now_unix(),
        })
    }

    /// Undoes what was applied, as far as it can, and records how far that was.
    fn finish_failed(
        &self,
        plan: &WirePlan,
        reports: Vec<StepReport>,
        rollback_records: Vec<RollbackRecord>,
        step_index: Option<u32>,
        error: DaemonError,
    ) -> Result<TransactionOutcome, DaemonError> {
        let recovery = if rollback_records.is_empty() {
            // Nothing was applied, so there is nothing to recover and no
            // restore point to invent.
            None
        } else {
            Some(self.roll_back(&rollback_records))
        };

        let outcome = TransactionOutcome {
            protocol_version: PROTOCOL_VERSION,
            transaction_id: plan.transaction_id.clone(),
            status: OutcomeStatus::Failed {
                step_index,
                error_key: error.to_string(),
                recovery,
            },
            reports,
            rollback_records,
        };
        self.journal.complete(&outcome)?;
        Ok(outcome)
    }

    fn roll_back(&self, records: &[RollbackRecord]) -> WireRecovery {
        let mut restored = 0_usize;
        // Reverse order: the last thing applied is the first thing undone.
        for record in records.iter().rev() {
            let undone = match (&record.previous_version, &record.previous_artifact) {
                // It was not installed before, so undoing means removing it.
                (None, _) => self
                    .apt
                    .remove(&record.component)
                    .map(|run| run.succeeded())
                    .unwrap_or(false),
                // It was installed before and we still have that package. Its
                // checksum is re-verified even here: rolling back onto bytes we
                // have not re-checked would be its own kind of damage.
                (Some(_), Some(artifact)) => {
                    self.artifacts
                        .verify(&artifact.filename, &artifact.sha256)
                        .is_ok()
                        && self
                            .artifacts
                            .path_for(&artifact.filename)
                            .map(|path| {
                                self.apt
                                    .install_local_deb(&path, true)
                                    .map(|run| run.succeeded())
                                    .unwrap_or(false)
                            })
                            .unwrap_or(false)
                }
                // It was installed before and we cannot get that version back.
                (Some(_), None) => false,
            };
            if undone {
                restored += 1;
            }
        }

        if restored == records.len() {
            WireRecovery::Restored
        } else if restored > 0 {
            WireRecovery::PartiallyRestored
        } else {
            WireRecovery::ManualRecoveryRequired
        }
    }
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apt::{DebFields, FakeAptDriver};
    use crate::health::FakeHealthProbe;
    use crate::host::FixedHostProbe;
    use manager_ipc::WireArtifact;
    use sha2::{Digest, Sha256};
    use std::path::PathBuf;

    const MONITOR_DEB: &str = "better-monitor_0.1.0_ubuntu-24.04_amd64.deb";
    const FILES_DEB: &str = "better-files-example_0.1.0_ubuntu-24.04_amd64.deb";

    struct Harness {
        root: PathBuf,
        artifacts: Arc<ArtifactStore>,
        journal: Arc<Journal>,
        apt: Arc<FakeAptDriver>,
        health: Arc<FakeHealthProbe>,
    }

    fn digest(content: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(content);
        hasher
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    impl Harness {
        fn new(label: &str, apt: FakeAptDriver, binaries: Vec<&str>) -> Self {
            let root = std::env::temp_dir().join(format!(
                "better-os-exec-{label}-{}-{}",
                std::process::id(),
                now_unix()
            ));
            std::fs::create_dir_all(&root).unwrap();
            Self {
                artifacts: Arc::new(ArtifactStore::new(root.join("archives"))),
                journal: Arc::new(Journal::new(root.join("state"))),
                apt: Arc::new(apt),
                health: Arc::new(FakeHealthProbe(
                    binaries
                        .into_iter()
                        .map(|name| PathBuf::from("/usr/bin").join(name))
                        .collect(),
                )),
                root,
            }
        }

        fn stage(&self, filename: &str) -> String {
            let content = filename.as_bytes();
            let checksum = digest(content);
            self.artifacts
                .stage(filename, &checksum, &mut std::io::Cursor::new(content))
                .unwrap();
            checksum
        }

        fn executor(&self) -> Executor {
            Executor {
                apt: self.apt.clone(),
                host: Arc::new(FixedHostProbe::ubuntu_2404()),
                health: self.health.clone(),
                artifacts: self.artifacts.clone(),
                journal: self.journal.clone(),
            }
        }
    }

    impl Drop for Harness {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    fn fields(package: &str) -> DebFields {
        DebFields {
            package: package.to_string(),
            version: "0.1.0".to_string(),
            architecture: "amd64".to_string(),
        }
    }

    fn plan(steps: Vec<WireStep>) -> WirePlan {
        WirePlan {
            protocol_version: PROTOCOL_VERSION,
            transaction_id: "3f2504e0-4f89-41d3-9a0c-0305e82c3301".to_string(),
            target_release: "24.04".to_string(),
            target_architecture: "amd64".to_string(),
            steps,
        }
    }

    fn install_step(component: &str, filename: &str, sha256: String) -> WireStep {
        WireStep {
            component: component.to_string(),
            action: WireAction::Install,
            before_version: None,
            after_version: Some("0.1.0".to_string()),
            artifact: Some(WireArtifact {
                filename: filename.to_string(),
                sha256,
                size_bytes: 64,
            }),
        }
    }

    #[test]
    fn a_successful_install_reports_healthy_and_records_what_it_did() {
        let harness = Harness::new(
            "ok",
            FakeAptDriver::new().with_deb(MONITOR_DEB, fields("better-monitor")),
            vec!["better-monitor"],
        );
        let checksum = harness.stage(MONITOR_DEB);
        let plan = plan(vec![install_step("better-monitor", MONITOR_DEB, checksum)]);

        let outcome = harness.executor().execute(&plan, &mut |_, _| {}).unwrap();

        assert_eq!(outcome.status, OutcomeStatus::Succeeded);
        assert_eq!(outcome.reports.len(), 1);
        assert_eq!(outcome.reports[0].health, HealthResult::Healthy);
        assert_eq!(outcome.reports[0].applied_version.as_deref(), Some("0.1.0"));
        assert!(!outcome.reports[0].log.is_empty());
    }

    #[test]
    fn a_plan_refused_before_any_change_leaves_no_restore_point() {
        let harness = Harness::new("refused", FakeAptDriver::new(), Vec::new());
        // The artifact was never staged, so verification fails before APT runs.
        let plan = plan(vec![install_step(
            "better-monitor",
            MONITOR_DEB,
            "a".repeat(64),
        )]);

        let outcome = harness.executor().execute(&plan, &mut |_, _| {}).unwrap();

        let OutcomeStatus::Failed { recovery, .. } = &outcome.status else {
            panic!("expected a failure");
        };
        assert_eq!(*recovery, None, "nothing changed, so nothing recovered");
        assert!(outcome.rollback_records.is_empty());
        assert!(
            harness
                .journal
                .read_rollback("better-monitor")
                .unwrap()
                .is_none(),
            "no restore point may be invented"
        );
        assert!(harness.apt.calls().is_empty(), "APT must not have run");
    }

    #[test]
    fn a_failed_install_of_a_new_component_is_rolled_back_by_removing_it() {
        let apt = FakeAptDriver::new().with_deb(MONITOR_DEB, fields("better-monitor"));
        apt.fail_install("better-monitor");
        let harness = Harness::new("failed-install", apt, Vec::new());
        let checksum = harness.stage(MONITOR_DEB);
        let plan = plan(vec![install_step("better-monitor", MONITOR_DEB, checksum)]);

        let outcome = harness.executor().execute(&plan, &mut |_, _| {}).unwrap();

        let OutcomeStatus::Failed {
            recovery,
            error_key,
            step_index,
        } = &outcome.status
        else {
            panic!("expected a failure");
        };
        assert_eq!(*step_index, Some(0));
        assert_eq!(error_key, "daemon.error.apt_failed:better-monitor");
        // It was not installed before, so putting the host back means it is not
        // installed now either.
        assert_eq!(*recovery, Some(WireRecovery::Restored));
        assert!(
            harness
                .apt
                .calls()
                .contains(&"remove:better-monitor".to_string())
        );
    }

    #[test]
    fn a_failure_after_an_earlier_step_applied_rolls_that_step_back_too() {
        let apt = FakeAptDriver::new()
            .with_deb(MONITOR_DEB, fields("better-monitor"))
            .with_deb(FILES_DEB, fields("better-files-example"));
        apt.fail_install("better-files-example");
        let harness = Harness::new("multi", apt, vec!["better-monitor"]);
        let monitor = harness.stage(MONITOR_DEB);
        let files = harness.stage(FILES_DEB);

        let plan = plan(vec![
            install_step("better-monitor", MONITOR_DEB, monitor),
            install_step("better-files-example", FILES_DEB, files),
        ]);
        let outcome = harness.executor().execute(&plan, &mut |_, _| {}).unwrap();

        let OutcomeStatus::Failed {
            recovery,
            step_index,
            ..
        } = &outcome.status
        else {
            panic!("expected a failure");
        };
        assert_eq!(*step_index, Some(1));
        assert_eq!(*recovery, Some(WireRecovery::Restored));

        // The component that did install is taken back out.
        assert!(
            harness
                .apt
                .calls()
                .contains(&"remove:better-monitor".to_string())
        );
        assert_eq!(
            harness.apt.installed_version("better-monitor").unwrap(),
            None
        );
    }

    #[test]
    fn a_rollback_that_only_partly_succeeds_says_so() {
        // The second component fails to install, and the first cannot be taken
        // back out. Reporting "restored" here would tell the user the host is
        // as they left it when half of it is not.
        let apt = FakeAptDriver::new()
            .with_deb(MONITOR_DEB, fields("better-monitor"))
            .with_deb(FILES_DEB, fields("better-files-example"));
        apt.fail_install("better-files-example");
        apt.fail_remove("better-monitor");
        let harness = Harness::new("partial", apt, vec!["better-monitor"]);
        let monitor = harness.stage(MONITOR_DEB);
        let files = harness.stage(FILES_DEB);

        let plan = plan(vec![
            install_step("better-monitor", MONITOR_DEB, monitor),
            install_step("better-files-example", FILES_DEB, files),
        ]);
        let outcome = harness.executor().execute(&plan, &mut |_, _| {}).unwrap();

        let OutcomeStatus::Failed { recovery, .. } = &outcome.status else {
            panic!("expected a failure");
        };
        assert_eq!(*recovery, Some(WireRecovery::PartiallyRestored));
        assert_eq!(
            harness.apt.installed_version("better-monitor").unwrap(),
            Some("0.1.0".to_string()),
            "the component that could not be removed is still there"
        );
    }

    #[test]
    fn a_rollback_that_cannot_reinstall_the_previous_version_asks_for_a_person() {
        // The component was already installed, but no cached .deb of the old
        // version exists, so there is nothing to put back.
        let apt = FakeAptDriver::new()
            .with_installed("better-monitor", "0.0.9")
            .with_deb(MONITOR_DEB, fields("better-monitor"));
        apt.fail_install("better-monitor");
        let harness = Harness::new("manual", apt, Vec::new());
        let checksum = harness.stage(MONITOR_DEB);

        let mut step = install_step("better-monitor", MONITOR_DEB, checksum);
        step.action = WireAction::Update;
        step.before_version = Some("0.0.9".to_string());
        let plan = plan(vec![step]);

        let outcome = harness.executor().execute(&plan, &mut |_, _| {}).unwrap();

        let OutcomeStatus::Failed { recovery, .. } = &outcome.status else {
            panic!("expected a failure");
        };
        assert_eq!(*recovery, Some(WireRecovery::ManualRecoveryRequired));
    }

    #[test]
    fn an_install_that_leaves_no_working_binary_fails_the_transaction() {
        let harness = Harness::new(
            "unhealthy",
            FakeAptDriver::new().with_deb(MONITOR_DEB, fields("better-monitor")),
            // No binary is reported present.
            Vec::new(),
        );
        let checksum = harness.stage(MONITOR_DEB);
        let plan = plan(vec![install_step("better-monitor", MONITOR_DEB, checksum)]);

        let outcome = harness.executor().execute(&plan, &mut |_, _| {}).unwrap();

        let OutcomeStatus::Failed { error_key, .. } = &outcome.status else {
            panic!("expected a failure");
        };
        assert_eq!(error_key, "daemon.error.health_failed:better-monitor");
        assert!(matches!(outcome.reports[0].health, HealthResult::Failed(_)));
    }

    #[test]
    fn a_host_that_moved_since_planning_stops_before_apt_runs() {
        let harness = Harness::new(
            "drift",
            FakeAptDriver::new()
                .with_installed("better-monitor", "0.5.0")
                .with_deb(MONITOR_DEB, fields("better-monitor")),
            vec!["better-monitor"],
        );
        let checksum = harness.stage(MONITOR_DEB);
        let plan = plan(vec![install_step("better-monitor", MONITOR_DEB, checksum)]);

        let error = harness
            .executor()
            .execute(&plan, &mut |_, _| {})
            .unwrap_err();

        assert!(matches!(error, DaemonError::StateDrift { .. }));
        assert!(harness.apt.calls().is_empty());
    }

    #[test]
    fn lock_contention_is_reported_as_busy_rather_than_a_broken_package() {
        let apt = FakeAptDriver::new().with_deb(MONITOR_DEB, fields("better-monitor"));
        apt.fail_install("better-monitor");
        *apt.lock_contention.lock().unwrap() = true;
        let harness = Harness::new("busy", apt, Vec::new());
        let checksum = harness.stage(MONITOR_DEB);
        let plan = plan(vec![install_step("better-monitor", MONITOR_DEB, checksum)]);

        let outcome = harness.executor().execute(&plan, &mut |_, _| {}).unwrap();

        let OutcomeStatus::Failed { error_key, .. } = &outcome.status else {
            panic!("expected a failure");
        };
        assert_eq!(error_key, "daemon.error.apt_busy");
    }

    #[test]
    fn a_completed_transaction_is_readable_from_the_journal_afterwards() {
        let harness = Harness::new(
            "journal",
            FakeAptDriver::new().with_deb(MONITOR_DEB, fields("better-monitor")),
            vec!["better-monitor"],
        );
        let checksum = harness.stage(MONITOR_DEB);
        let plan = plan(vec![install_step("better-monitor", MONITOR_DEB, checksum)]);
        harness.executor().execute(&plan, &mut |_, _| {}).unwrap();

        let entry = harness
            .journal
            .read(&plan.transaction_id)
            .unwrap()
            .expect("the transaction is recorded");
        assert_eq!(entry.state, JournalState::Completed);
        assert!(entry.outcome.is_some());
        assert!(harness.journal.interrupted().unwrap().is_empty());
    }

    #[test]
    fn progress_is_reported_for_each_stage_of_each_step() {
        let harness = Harness::new(
            "progress",
            FakeAptDriver::new().with_deb(MONITOR_DEB, fields("better-monitor")),
            vec!["better-monitor"],
        );
        let checksum = harness.stage(MONITOR_DEB);
        let plan = plan(vec![install_step("better-monitor", MONITOR_DEB, checksum)]);

        let mut seen = Vec::new();
        harness
            .executor()
            .execute(&plan, &mut |index, stage| seen.push((index, stage)))
            .unwrap();

        assert_eq!(
            seen,
            vec![
                (0, ExecutionStage::Verifying),
                (0, ExecutionStage::Applying),
                (0, ExecutionStage::CheckingHealth),
            ]
        );
    }
}
