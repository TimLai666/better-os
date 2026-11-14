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
    ExecutionStage, HealthResult, OutcomeStatus, PROTOCOL_VERSION, RepairOutcome, RepairStatus,
    RollbackRecord, StepReport, TransactionOutcome, WireAction, WireArtifact, WirePlan,
    WireRecovery, WireStep,
};

use crate::apt::AptDriver;
use crate::health::{self, HealthProbe};
use crate::host::HostProbe;
use crate::store::{ArtifactStore, InstalledArtifact, Journal, JournalState};
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
        // apt refuses to run after an interrupted dpkg run. Letting it try
        // would write a restore point and then fail the rollback for the same
        // reason, so the plan is refused here, from dpkg's own record.
        if self.apt.package_system_state()?.needs_repair() {
            return Err(DaemonError::DpkgInterrupted);
        }
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
            let record = self.rollback_record_for(plan, &step.component)?;
            self.journal.write_rollback(&record)?;
            rollback_records.push(record);
            // A companion is changed by the same APT transaction, so it gets
            // its own way back too.
            for companion in &step.companions {
                let record = self.rollback_record_for(plan, &companion.package)?;
                self.journal.write_rollback(&record)?;
                rollback_records.push(record);
            }

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
            let applied_version = self.apt.installed_version(&step.component)?;
            let recorded = self
                .record_applied_artifact(step, applied_version.as_deref())
                .and_then(|()| self.record_applied_companions(step));
            if let Err(error) = recorded {
                reports.push(StepReport {
                    component: step.component.clone(),
                    action: step.action,
                    applied_version: applied_version.clone(),
                    health: HealthResult::Undetermined(
                        "the installed artifact record could not be written".to_string(),
                    ),
                    log: log.clone(),
                });
                return self.finish_failed(
                    plan,
                    reports,
                    rollback_records,
                    Some(step_index),
                    error,
                );
            }

            let mut health = health::check(
                &step.component,
                step.action,
                self.apt.as_ref(),
                self.health.as_ref(),
            );
            // A companion that did not arrive healthy fails the step as its
            // own package would: they were one transaction.
            let mut unhealthy_package = step.component.clone();
            if matches!(health, HealthResult::Healthy) {
                for companion in &step.companions {
                    let companion_health = health::check(
                        &companion.package,
                        step.action,
                        self.apt.as_ref(),
                        self.health.as_ref(),
                    );
                    if !matches!(companion_health, HealthResult::Healthy) {
                        health = companion_health;
                        unhealthy_package = companion.package.clone();
                        break;
                    }
                }
            }
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
                        component: unhealthy_package,
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

    /// Finishes an interrupted dpkg run with `dpkg --configure -a`.
    ///
    /// Refuses before running anything when dpkg's record cannot be read or
    /// shows nothing unfinished. Once dpkg has run, the result is an outcome
    /// rather than an error, so the caller gets the log whatever happened, and
    /// the verdict comes from reading dpkg's record again rather than from the
    /// exit code alone: a run that exits 0 can still leave a half-installed
    /// package behind. ADR 0014.
    pub fn repair(&self) -> Result<RepairOutcome, DaemonError> {
        if !self.apt.package_system_state()?.needs_repair() {
            return Err(DaemonError::NothingToRepair);
        }

        let run = self.apt.configure_pending()?;
        let lock_contention = run.is_lock_contention();
        let succeeded = run.succeeded();
        let log = vec![run.into_log_entry()];
        let remaining = self.apt.package_system_state();

        let status = match (&remaining, succeeded) {
            (_, false) if lock_contention => RepairStatus::Failed {
                error_key: DaemonError::AptBusy.to_string(),
            },
            (_, false) => RepairStatus::Failed {
                error_key: DaemonError::RepairFailed.to_string(),
            },
            (Ok(state), true) if state.needs_repair() => RepairStatus::StillInterrupted,
            (Ok(_), true) => RepairStatus::Repaired,
            // dpkg said it finished and its record cannot be read to confirm
            // it. That is not a repair anybody verified.
            (Err(error), true) => RepairStatus::Failed {
                error_key: error.to_string(),
            },
        };
        Ok(RepairOutcome {
            protocol_version: PROTOCOL_VERSION,
            status,
            log,
            remaining: remaining.ok(),
        })
    }

    fn apply(&self, step: &WireStep) -> Result<crate::apt::AptRun, DaemonError> {
        match step.action {
            WireAction::Remove => self.apt.remove(&step.component),
            action => {
                let artifact = step.artifact.as_ref().ok_or(DaemonError::ArtifactMissing {
                    component: step.component.clone(),
                })?;
                let mut paths = vec![self.artifacts.path_for(&artifact.filename)?];
                for companion in &step.companions {
                    paths.push(self.artifacts.path_for(&companion.artifact.filename)?);
                }
                let paths: Vec<&std::path::Path> =
                    paths.iter().map(|path| path.as_path()).collect();
                self.apt
                    .install_local_debs(&paths, revalidate::allows_downgrade(action))
            }
        }
    }

    fn record_applied_artifact(
        &self,
        step: &WireStep,
        applied_version: Option<&str>,
    ) -> Result<(), DaemonError> {
        if step.action == WireAction::Remove {
            return self.journal.remove_installed_artifact(&step.component);
        }

        let version = applied_version.ok_or_else(|| {
            DaemonError::Storage(format!(
                "no installed version for {} after apply",
                step.component
            ))
        })?;
        let artifact = step.artifact.as_ref().ok_or_else(|| {
            DaemonError::Storage(format!("no artifact for {} after apply", step.component))
        })?;
        self.journal.write_installed_artifact(&InstalledArtifact {
            component: step.component.clone(),
            version: version.to_string(),
            filename: artifact.filename.clone(),
            sha256: artifact.sha256.clone(),
        })
    }

    /// Records each companion's installed artifact, so a later rollback of it
    /// can reinstall exactly these bytes.
    fn record_applied_companions(&self, step: &WireStep) -> Result<(), DaemonError> {
        for companion in &step.companions {
            let version = self
                .apt
                .installed_version(&companion.package)?
                .ok_or_else(|| {
                    DaemonError::Storage(format!(
                        "no installed version for {} after apply",
                        companion.package
                    ))
                })?;
            self.journal.write_installed_artifact(&InstalledArtifact {
                component: companion.package.clone(),
                version,
                filename: companion.artifact.filename.clone(),
                sha256: companion.artifact.sha256.clone(),
            })?;
        }
        Ok(())
    }

    /// What it would take to undo a package's change, or an honest statement
    /// that it was not installed before.
    fn rollback_record_for(
        &self,
        plan: &WirePlan,
        package: &str,
    ) -> Result<RollbackRecord, DaemonError> {
        let previous_version = self.apt.installed_version(package)?;
        // Only a version we can actually reinstall counts. A previously
        // installed version whose .deb is no longer cached is recorded as a
        // version without an artifact, so a rollback attempt reports that it
        // needs a person rather than silently doing nothing.
        let previous_artifact = match previous_version.as_deref() {
            Some(previous_version) => self
                .journal
                .read_installed_artifact(package)?
                .filter(|installed| {
                    installed.component == package
                        && manager_ipc::strip_epoch(&installed.version)
                            == manager_ipc::strip_epoch(previous_version)
                        && self.artifacts.contains(&installed.filename)
                })
                .map(|installed| {
                    let size_bytes = self.artifacts.size_bytes(&installed.filename)?;
                    Ok::<WireArtifact, DaemonError>(WireArtifact {
                        filename: installed.filename,
                        sha256: installed.sha256,
                        size_bytes,
                    })
                })
                .transpose()?,
            None => None,
        };

        Ok(RollbackRecord {
            component: package.to_string(),
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
                (None, _) => self.rollback_remove(record),
                // It was installed before and we still have that package. Its
                // checksum is re-verified even here: rolling back onto bytes we
                // have not re-checked would be its own kind of damage.
                (Some(version), Some(artifact)) => self.rollback_install(record, version, artifact),
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

    fn rollback_remove(&self, record: &RollbackRecord) -> bool {
        // Not installed now: APT failed before it unpacked anything, as it does
        // when one package of a transaction cannot be installed. That already
        // is the state from before the step, and `apt-get remove` of a package
        // dpkg has never heard of fails, which would call a restored host
        // broken.
        match self.apt.installed_version(&record.component) {
            Ok(None) => {
                return self
                    .journal
                    .remove_installed_artifact(&record.component)
                    .is_ok();
            }
            Ok(Some(_)) => {}
            Err(_) => return false,
        }
        let removed = self
            .apt
            .remove(&record.component)
            .map(|run| run.succeeded())
            .unwrap_or(false);
        if !removed {
            return false;
        }
        self.apt
            .installed_version(&record.component)
            .map(|installed| installed.is_none())
            .unwrap_or(false)
            && self
                .journal
                .remove_installed_artifact(&record.component)
                .is_ok()
    }

    fn rollback_install(
        &self,
        record: &RollbackRecord,
        version: &str,
        artifact: &WireArtifact,
    ) -> bool {
        if self
            .artifacts
            .verify(&artifact.filename, &artifact.sha256)
            .is_err()
        {
            return false;
        }
        let Ok(path) = self.artifacts.path_for(&artifact.filename) else {
            return false;
        };
        let Ok(fields) = self.apt.deb_control_fields(&path) else {
            return false;
        };
        if fields.package != record.component
            || manager_ipc::strip_epoch(&fields.version) != manager_ipc::strip_epoch(version)
        {
            return false;
        }
        let installed = self
            .apt
            .install_local_debs(&[path.as_path()], true)
            .map(|run| run.succeeded())
            .unwrap_or(false)
            && self
                .apt
                .installed_version(&record.component)
                .map(|current| {
                    current.is_some_and(|current| {
                        manager_ipc::strip_epoch(&current) == manager_ipc::strip_epoch(version)
                    })
                })
                .unwrap_or(false);
        installed
            && self
                .journal
                .write_installed_artifact(&InstalledArtifact {
                    component: record.component.clone(),
                    version: version.to_string(),
                    filename: artifact.filename.clone(),
                    sha256: artifact.sha256.clone(),
                })
                .is_ok()
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
    const OLD_MONITOR_DEB: &str = "better-monitor_0.0.9_ubuntu-24.04_amd64.deb";
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
                health: Arc::new(FakeHealthProbe::with_files(
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
        fields_with_version(package, "0.1.0")
    }

    fn fields_with_version(package: &str, version: &str) -> DebFields {
        DebFields {
            package: package.to_string(),
            version: version.to_string(),
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
            companions: Vec::new(),
        }
    }

    const MANAGER_DEB: &str = "better-manager_0.1.0_ubuntu-24.04_amd64.deb";
    const ADAPTER_DEB: &str = "better-shell-adapter_0.1.0_ubuntu-24.04_amd64.deb";

    fn with_adapter(mut step: WireStep, sha256: String) -> WireStep {
        step.companions.push(manager_ipc::WireCompanion {
            package: "better-shell-adapter".to_string(),
            artifact: WireArtifact {
                filename: ADAPTER_DEB.to_string(),
                sha256,
                size_bytes: 64,
            },
        });
        step
    }

    fn manager_and_adapter(label: &str) -> Harness {
        Harness::new(
            label,
            FakeAptDriver::new()
                .with_deb(MANAGER_DEB, fields("better-manager"))
                .with_deb(ADAPTER_DEB, fields("better-shell-adapter"))
                .with_files("better-shell-adapter", &["/usr/bin/adapter-file"], &[]),
            vec!["better-manager", "adapter-file"],
        )
    }

    /// ADR 0018: Better Manager's own update carries the shell adapter, and
    /// both go to APT in one transaction, so the adapter is never missing
    /// beside a manager that recommends it.
    #[test]
    fn a_companion_is_installed_in_the_same_apt_transaction() {
        let harness = manager_and_adapter("companion");
        let manager = harness.stage(MANAGER_DEB);
        let adapter = harness.stage(ADAPTER_DEB);
        let plan = plan(vec![with_adapter(
            install_step("better-manager", MANAGER_DEB, manager),
            adapter.clone(),
        )]);

        let outcome = harness.executor().execute(&plan, &mut |_, _| {}).unwrap();

        assert_eq!(outcome.status, OutcomeStatus::Succeeded);
        let installs: Vec<String> = harness
            .apt
            .calls()
            .into_iter()
            .filter(|call| call.starts_with("install:"))
            .collect();
        assert_eq!(
            installs,
            ["install:better-manager+better-shell-adapter:downgrade=false"]
        );
        assert_eq!(
            harness
                .journal
                .read_installed_artifact("better-shell-adapter")
                .unwrap()
                .map(|installed| (installed.filename, installed.sha256)),
            Some((ADAPTER_DEB.to_string(), adapter))
        );
        // The restore points the transaction keeps: one for each package it
        // changed, neither installed before.
        let points: Vec<(&str, Option<&str>)> = outcome
            .rollback_records
            .iter()
            .map(|record| {
                (
                    record.component.as_str(),
                    record.previous_version.as_deref(),
                )
            })
            .collect();
        assert_eq!(
            points,
            [("better-manager", None), ("better-shell-adapter", None)]
        );
        assert_eq!(
            harness
                .apt
                .installed_version("better-shell-adapter")
                .unwrap(),
            Some("0.1.0".to_string())
        );
    }

    #[test]
    fn a_failing_companion_fails_the_step_and_both_are_undone() {
        let harness = manager_and_adapter("companion-fails");
        harness.apt.fail_install("better-shell-adapter");
        let manager = harness.stage(MANAGER_DEB);
        let adapter = harness.stage(ADAPTER_DEB);
        let plan = plan(vec![with_adapter(
            install_step("better-manager", MANAGER_DEB, manager),
            adapter,
        )]);

        let outcome = harness.executor().execute(&plan, &mut |_, _| {}).unwrap();

        // APT failed before unpacking either package, so neither was ever
        // installed: that is a full restore, not a partial one.
        assert!(
            matches!(
                outcome.status,
                OutcomeStatus::Failed {
                    recovery: Some(WireRecovery::Restored),
                    ..
                }
            ),
            "{:?}",
            outcome.status
        );
        assert!(
            !harness
                .apt
                .calls()
                .iter()
                .any(|call| call.starts_with("remove:")),
            "nothing to remove: {:?}",
            harness.apt.calls()
        );
        assert_eq!(
            harness.apt.installed_version("better-manager").unwrap(),
            None
        );
        assert_eq!(
            harness
                .apt
                .installed_version("better-shell-adapter")
                .unwrap(),
            None
        );
        let undone: Vec<&str> = outcome
            .rollback_records
            .iter()
            .map(|record| record.component.as_str())
            .collect();
        assert_eq!(undone, ["better-manager", "better-shell-adapter"]);
    }

    /// Only a Better OS package may ride along with root's authority.
    #[test]
    fn a_companion_that_is_not_a_better_os_package_is_refused() {
        let mut step = install_step("better-manager", MANAGER_DEB, "a".repeat(64));
        step.companions.push(manager_ipc::WireCompanion {
            package: "gnome-shell".to_string(),
            artifact: WireArtifact {
                filename: "gnome-shell_0.1.0_ubuntu-24.04_amd64.deb".to_string(),
                sha256: "b".repeat(64),
                size_bytes: 64,
            },
        });
        let host = FixedHostProbe::ubuntu_2404().facts().unwrap();
        let refused = revalidate::check_plan(&plan(vec![step]), &host);
        assert!(
            matches!(&refused, Err(DaemonError::PlanRejected(reason)) if reason.contains("gnome-shell")),
            "{refused:?}"
        );
    }

    #[test]
    fn a_companion_that_is_not_the_package_it_names_is_refused_before_apt() {
        let harness = Harness::new(
            "companion-lies",
            FakeAptDriver::new()
                .with_deb(MANAGER_DEB, fields("better-manager"))
                .with_deb(ADAPTER_DEB, fields("better-monitor")),
            vec!["better-manager"],
        );
        let manager = harness.stage(MANAGER_DEB);
        let adapter = harness.stage(ADAPTER_DEB);
        let plan = plan(vec![with_adapter(
            install_step("better-manager", MANAGER_DEB, manager),
            adapter,
        )]);

        let outcome = harness.executor().execute(&plan, &mut |_, _| {}).unwrap();

        assert!(matches!(outcome.status, OutcomeStatus::Failed { .. }));
        assert!(
            !harness
                .apt
                .calls()
                .iter()
                .any(|call| call.starts_with("install:")),
            "{:?}",
            harness.apt.calls()
        );
    }

    /// The field failure this replaces: `better-awake` installs fine and ships
    /// no binary of its own name, so the old health rule failed every install
    /// of it and the daemon rolled back what it had just installed.
    #[test]
    fn a_multi_binary_package_named_after_none_of_its_binaries_installs() {
        const AWAKE_DEB: &str = "better-awake_0.1.0_ubuntu-24.04_amd64.deb";
        let harness = Harness::new(
            "awake-shape",
            FakeAptDriver::new()
                .with_deb(AWAKE_DEB, fields("better-awake"))
                .with_files(
                    "better-awake",
                    &[
                        "/usr/bin/better-awake-service",
                        "/usr/bin/awake-tray",
                        "/usr/bin/awake-gui",
                    ],
                    &[],
                ),
            vec!["better-awake-service", "awake-tray", "awake-gui"],
        );
        let checksum = harness.stage(AWAKE_DEB);
        let plan = plan(vec![install_step("better-awake", AWAKE_DEB, checksum)]);

        let outcome = harness.executor().execute(&plan, &mut |_, _| {}).unwrap();

        assert_eq!(outcome.status, OutcomeStatus::Succeeded);
        assert_eq!(outcome.reports[0].health, HealthResult::Healthy);
        assert!(
            outcome.rollback_records.is_empty()
                || !harness
                    .apt
                    .calls()
                    .iter()
                    .any(|call| call == "remove:better-awake"),
            "a healthy install must not be rolled back"
        );
    }

    #[test]
    fn a_successful_install_reports_healthy_and_records_what_it_did() {
        let harness = Harness::new(
            "ok",
            FakeAptDriver::new().with_deb(MONITOR_DEB, fields("better-monitor")),
            vec!["better-monitor"],
        );
        let checksum = harness.stage(MONITOR_DEB);
        let plan = plan(vec![install_step(
            "better-monitor",
            MONITOR_DEB,
            checksum.clone(),
        )]);

        let outcome = harness.executor().execute(&plan, &mut |_, _| {}).unwrap();

        assert_eq!(outcome.status, OutcomeStatus::Succeeded);
        assert_eq!(outcome.reports.len(), 1);
        assert_eq!(outcome.reports[0].health, HealthResult::Healthy);
        assert_eq!(outcome.reports[0].applied_version.as_deref(), Some("0.1.0"));
        assert!(!outcome.reports[0].log.is_empty());
        assert_eq!(
            harness
                .journal
                .read_installed_artifact("better-monitor")
                .unwrap(),
            Some(InstalledArtifact {
                component: "better-monitor".to_string(),
                version: "0.1.0".to_string(),
                filename: MONITOR_DEB.to_string(),
                sha256: checksum,
            })
        );
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
        // installed now either. APT failed before unpacking it, so that is
        // already true: there is nothing to remove, and `apt-get remove` of a
        // package dpkg does not know would fail and report the restore as
        // partial.
        assert_eq!(*recovery, Some(WireRecovery::Restored));
        assert_eq!(
            harness.apt.installed_version("better-monitor").unwrap(),
            None
        );
        assert!(
            !harness
                .apt
                .calls()
                .contains(&"remove:better-monitor".to_string())
        );
    }

    /// The other half: a package APT did unpack before the step failed is
    /// still removed.
    #[test]
    fn an_unhealthy_new_component_is_removed_to_restore_the_host() {
        let apt = FakeAptDriver::new().with_deb(MONITOR_DEB, fields("better-monitor"));
        // Installed, but its binary is missing, so the health check fails.
        let harness = Harness::new(
            "unhealthy-install",
            apt.with_files("better-monitor", &["/usr/bin/better-monitor"], &[]),
            Vec::new(),
        );
        let checksum = harness.stage(MONITOR_DEB);
        let plan = plan(vec![install_step("better-monitor", MONITOR_DEB, checksum)]);

        let outcome = harness.executor().execute(&plan, &mut |_, _| {}).unwrap();

        assert!(
            matches!(
                outcome.status,
                OutcomeStatus::Failed {
                    recovery: Some(WireRecovery::Restored),
                    ..
                }
            ),
            "{:?}",
            outcome.status
        );
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
        let harness = Harness::new(
            "manual",
            FakeAptDriver::new()
                .with_installed("better-monitor", "0.0.9")
                .with_deb(MONITOR_DEB, fields("better-monitor")),
            Vec::new(),
        );
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
    fn a_failed_update_reinstalls_the_artifact_for_the_previous_version() {
        let harness = Harness::new(
            "rollback-target",
            FakeAptDriver::new()
                .with_installed("better-monitor", "0.0.9")
                .with_deb(MONITOR_DEB, fields("better-monitor"))
                .with_deb(
                    OLD_MONITOR_DEB,
                    fields_with_version("better-monitor", "0.0.9"),
                ),
            Vec::new(),
        );
        let new_checksum = harness.stage(MONITOR_DEB);
        let old_checksum = harness.stage(OLD_MONITOR_DEB);
        harness
            .journal
            .write_installed_artifact(&InstalledArtifact {
                component: "better-monitor".to_string(),
                version: "0.0.9".to_string(),
                filename: OLD_MONITOR_DEB.to_string(),
                sha256: old_checksum,
            })
            .unwrap();

        let mut step = install_step("better-monitor", MONITOR_DEB, new_checksum);
        step.action = WireAction::Update;
        step.before_version = Some("0.0.9".to_string());
        let plan = plan(vec![step]);

        let outcome = harness.executor().execute(&plan, &mut |_, _| {}).unwrap();

        let OutcomeStatus::Failed {
            error_key,
            recovery,
            ..
        } = &outcome.status
        else {
            panic!("expected a failure");
        };
        assert_eq!(error_key, "daemon.error.health_failed:better-monitor");
        assert_eq!(*recovery, Some(WireRecovery::Restored));
        assert_eq!(
            harness.apt.installed_version("better-monitor").unwrap(),
            Some("0.0.9".to_string())
        );
        assert_eq!(
            harness
                .journal
                .read_installed_artifact("better-monitor")
                .unwrap()
                .map(|record| (record.version, record.filename)),
            Some(("0.0.9".to_string(), OLD_MONITOR_DEB.to_string()))
        );
    }

    #[test]
    fn a_mismatched_installed_artifact_record_is_not_used_for_rollback() {
        let harness = Harness::new(
            "rollback-mismatch",
            FakeAptDriver::new()
                .with_installed("better-monitor", "0.0.9")
                .with_deb(MONITOR_DEB, fields("better-monitor")),
            Vec::new(),
        );
        let checksum = harness.stage(MONITOR_DEB);
        harness
            .journal
            .write_installed_artifact(&InstalledArtifact {
                component: "better-monitor".to_string(),
                version: "0.0.8".to_string(),
                filename: MONITOR_DEB.to_string(),
                sha256: checksum.clone(),
            })
            .unwrap();

        let mut step = install_step("better-monitor", MONITOR_DEB, checksum);
        step.action = WireAction::Update;
        step.before_version = Some("0.0.9".to_string());
        let outcome = harness
            .executor()
            .execute(&plan(vec![step]), &mut |_, _| {})
            .unwrap();

        let OutcomeStatus::Failed { recovery, .. } = &outcome.status else {
            panic!("expected a failure");
        };
        assert_eq!(*recovery, Some(WireRecovery::ManualRecoveryRequired));
        assert_eq!(
            harness.apt.installed_version("better-monitor").unwrap(),
            Some("0.1.0".to_string()),
            "a mismatched record must not make the new artifact look restorable"
        );
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

    fn interrupted() -> manager_ipc::PackageSystemState {
        manager_ipc::PackageSystemState {
            protocol_version: PROTOCOL_VERSION,
            journal_pending: true,
            packages: vec![manager_ipc::InterruptedPackage {
                package: "better-monitor".to_string(),
                status: manager_ipc::DpkgPackageStatus::HalfConfigured,
            }],
            omitted: 0,
        }
    }

    /// apt would refuse anyway, but only after the daemon had written a restore
    /// point and then failed to roll back for the same reason.
    #[test]
    fn a_transaction_is_refused_before_anything_changes_while_dpkg_is_interrupted() {
        let harness = Harness::new(
            "dpkg-interrupted",
            FakeAptDriver::new()
                .with_deb(MONITOR_DEB, fields("better-monitor"))
                .with_package_system(interrupted()),
            vec!["better-monitor"],
        );
        let checksum = harness.stage(MONITOR_DEB);
        let plan = plan(vec![install_step("better-monitor", MONITOR_DEB, checksum)]);

        let error = harness
            .executor()
            .execute(&plan, &mut |_, _| {})
            .unwrap_err();

        assert_eq!(error, DaemonError::DpkgInterrupted);
        assert_eq!(error.to_string(), "daemon.error.dpkg_interrupted");
        assert!(harness.apt.calls().is_empty(), "APT must not have run");
        assert!(
            harness
                .journal
                .read_rollback("better-monitor")
                .unwrap()
                .is_none(),
            "no restore point may be written for a refused plan"
        );
    }

    #[test]
    fn a_repair_runs_dpkg_configure_and_reports_the_package_system_repaired() {
        let harness = Harness::new(
            "repair-ok",
            FakeAptDriver::new().with_package_system(interrupted()),
            Vec::new(),
        );

        let outcome = harness.executor().repair().unwrap();

        assert_eq!(outcome.status, manager_ipc::RepairStatus::Repaired);
        assert_eq!(harness.apt.calls(), vec!["configure-pending".to_string()]);
        assert_eq!(outcome.log.len(), 1);
        assert_eq!(
            outcome.log[0].argv,
            vec![
                "dpkg",
                "--force-confdef",
                "--force-confold",
                "--configure",
                "-a"
            ]
        );
        assert_eq!(
            outcome.remaining,
            Some(manager_ipc::PackageSystemState::clean())
        );
        assert!(
            !harness.apt.package_system_state().unwrap().needs_repair(),
            "the host the fake describes is repaired afterwards"
        );
    }

    #[test]
    fn a_repair_on_a_package_system_that_needs_none_runs_nothing() {
        let harness = Harness::new("repair-clean", FakeAptDriver::new(), Vec::new());

        let error = harness.executor().repair().unwrap_err();

        assert_eq!(error, DaemonError::NothingToRepair);
        assert_eq!(error.to_string(), "daemon.error.nothing_to_repair");
        assert!(harness.apt.calls().is_empty(), "dpkg must not have run");
    }

    #[test]
    fn a_repair_that_cannot_read_dpkgs_record_runs_nothing() {
        let harness = Harness::new(
            "repair-unreadable",
            FakeAptDriver::new().with_unreadable_package_system(),
            Vec::new(),
        );

        let error = harness.executor().repair().unwrap_err();

        assert!(matches!(error, DaemonError::HostUnreadable(_)), "{error}");
        assert!(harness.apt.calls().is_empty(), "dpkg must not have run");
    }

    /// `--configure` cannot finish a half-installed package. Reporting that as
    /// repaired would send the person back to an install that fails again.
    #[test]
    fn a_repair_that_leaves_packages_unfinished_says_what_remains() {
        let mut remains = manager_ipc::PackageSystemState::clean();
        remains.packages.push(manager_ipc::InterruptedPackage {
            package: "better-awake".to_string(),
            status: manager_ipc::DpkgPackageStatus::HalfInstalled,
        });
        let harness = Harness::new(
            "repair-partial",
            FakeAptDriver::new()
                .with_package_system(interrupted())
                .with_repair_leaving(remains.clone()),
            Vec::new(),
        );

        let outcome = harness.executor().repair().unwrap();

        assert_eq!(outcome.status, manager_ipc::RepairStatus::StillInterrupted);
        assert_eq!(outcome.remaining, Some(remains));
    }

    #[test]
    fn a_repair_that_fails_reports_the_failure_and_its_log() {
        let apt = FakeAptDriver::new().with_package_system(interrupted());
        apt.fail_configure();
        let harness = Harness::new("repair-failed", apt, Vec::new());

        let outcome = harness.executor().repair().unwrap();

        assert_eq!(
            outcome.status,
            manager_ipc::RepairStatus::Failed {
                error_key: "daemon.error.repair_failed".to_string()
            }
        );
        assert_ne!(outcome.log[0].exit_code, 0);
        assert!(!outcome.log[0].stderr_tail.is_empty());
        assert_eq!(outcome.remaining, Some(interrupted()));
    }

    #[test]
    fn a_repair_blocked_by_another_package_manager_is_reported_busy() {
        let apt = FakeAptDriver::new().with_package_system(interrupted());
        apt.fail_configure();
        *apt.lock_contention.lock().unwrap() = true;
        let harness = Harness::new("repair-busy", apt, Vec::new());

        let outcome = harness.executor().repair().unwrap();

        assert_eq!(
            outcome.status,
            manager_ipc::RepairStatus::Failed {
                error_key: "daemon.error.apt_busy".to_string()
            }
        );
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
