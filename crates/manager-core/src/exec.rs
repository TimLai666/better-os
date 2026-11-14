//! Driving a transaction through its stages.
//!
//! [`Manager`] plans transactions and advances a state machine, but it does not
//! decide whether a stage actually happened. That is a [`StageDriver`]'s job: a
//! simulation reports scripted results, and a real driver reports what it
//! observed at the privileged boundary. Keeping the two behind one trait is
//! what lets the executor be replaced without changing the lifecycle the GUI
//! and CLI already drive.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use better_core::package_health::{HealthProbe, HealthVerdict};
use manager_ipc::{
    OutcomeStatus, TransactionOutcome, WireAction, WireArtifact, WireCompanion, WirePlan,
    WireRecovery, WireStep,
};
use manager_platform::download::HttpDownloader;
use manager_platform::dpkg::PackageStateProbe;
use manager_platform::user_units::{UnitActivity, UnitFailure, UserUnitController, service_units};
use manager_platform::{DownloadRequest, PlatformError, PrivilegedTransactionExecutor};

use crate::{
    DesiredOperation, FailureEvidence, Manager, ManagerError, ManagerState, MockOutcome,
    OperationProgress, OperationStage, PlanStep, StageOutcome, SystemProfile, TransactionPlan,
};

/// A cooperative cancellation flag shared with whatever is running a stage.
///
/// Cancellation is a request, not a guarantee: a driver checks it at points
/// where stopping still leaves the host in a state the manager can describe.
#[derive(Clone, Debug, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// Progress within a stage, for a presentation layer that wants to show more
/// than the stage name.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StageProgress {
    Downloading {
        component: String,
        received_bytes: u64,
        total_bytes: Option<u64>,
    },
    Applying {
        component: String,
    },
}

/// Carries out one stage of a transaction and reports what happened.
pub trait StageDriver {
    fn run_stage(
        &mut self,
        stage: OperationStage,
        plan: &TransactionPlan,
        progress: &mut dyn FnMut(StageProgress),
        cancel: &CancelToken,
    ) -> StageOutcome;

    /// Whether abandoning the transaction at this stage can still put the host
    /// back. A driver that says no makes the manager refuse to offer a cancel
    /// it cannot honor.
    fn supports_cancel_at(&self, stage: OperationStage) -> bool;
}

/// The scripted driver behind the demo and the lifecycle suite.
///
/// It performs no I/O. Every result is decided up front, which is what makes
/// the mock lifecycle reproducible.
#[derive(Clone, Debug, Default)]
pub struct MockDriver {
    pub fail_at: Option<OperationStage>,
    pub restore_outcome: MockRestoreOutcome,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum MockRestoreOutcome {
    #[default]
    Succeed,
    Partial,
    ManualRecovery,
}

impl MockDriver {
    pub fn new(fail_at: Option<OperationStage>, restore_outcome: MockRestoreOutcome) -> Self {
        Self {
            fail_at,
            restore_outcome,
        }
    }

    /// The scripted intent for a stage, in the vocabulary the mock lifecycle
    /// has always used.
    pub fn mock_outcome(&self, stage: OperationStage) -> MockOutcome {
        if self.fail_at == Some(stage) {
            return MockOutcome::FailAt(stage);
        }
        match (stage, self.restore_outcome) {
            (OperationStage::CheckingHealth, MockRestoreOutcome::Partial) => {
                MockOutcome::RestorePartially
            }
            (OperationStage::CheckingHealth, MockRestoreOutcome::ManualRecovery) => {
                MockOutcome::RestoreRequiresManualRecovery
            }
            _ => MockOutcome::Succeed,
        }
    }
}

impl StageDriver for MockDriver {
    fn run_stage(
        &mut self,
        stage: OperationStage,
        _plan: &TransactionPlan,
        _progress: &mut dyn FnMut(StageProgress),
        cancel: &CancelToken,
    ) -> StageOutcome {
        if cancel.is_cancelled() {
            return StageOutcome::Failed(FailureEvidence::new("mock_operation_cancelled"));
        }
        if self.fail_at == Some(stage) {
            return StageOutcome::Failed(FailureEvidence::new(crate::failure_evidence(stage)));
        }
        match (stage, self.restore_outcome) {
            (OperationStage::CheckingHealth, MockRestoreOutcome::Partial) => {
                StageOutcome::RestoredPartially
            }
            (OperationStage::CheckingHealth, MockRestoreOutcome::ManualRecovery) => {
                StageOutcome::RestoreRequiresManualRecovery
            }
            _ => StageOutcome::Completed,
        }
    }

    fn supports_cancel_at(&self, _stage: OperationStage) -> bool {
        true
    }
}

/// Carries a transaction out for real: fetches the artifacts, hands them to the
/// privileged service, and reports what the host actually did.
///
/// The daemon owns the applying, the health checks, and the rollback. This
/// driver does not re-derive any of that; it reports what came back. Deciding
/// here that a step succeeded would be inventing a result nothing observed.
///
/// It does own the component's user units, because starting them needs no
/// privilege and therefore never crosses that boundary. `None` means this
/// driver manages no units at all, which is what the tests that predate the
/// controller get.
pub struct RealDriver<'a> {
    downloader: &'a HttpDownloader,
    executor: &'a dyn PrivilegedTransactionExecutor,
    /// Read-only dpkg queries, for the steps that check a component without
    /// changing it.
    packages: &'a dyn PackageStateProbe,
    health: &'a dyn HealthProbe,
    user_units: Option<&'a dyn UserUnitController>,
    transaction_id: String,
    profile: SystemProfile,
    /// Set once the transaction has been handed over, so the outcome can be
    /// reported at the stages that follow.
    outcome: Option<TransactionOutcome>,
}

impl<'a> RealDriver<'a> {
    pub fn new(
        downloader: &'a HttpDownloader,
        executor: &'a dyn PrivilegedTransactionExecutor,
        packages: &'a dyn PackageStateProbe,
        health: &'a dyn HealthProbe,
        transaction_id: impl Into<String>,
        profile: SystemProfile,
    ) -> Self {
        Self {
            downloader,
            executor,
            packages,
            health,
            user_units: None,
            transaction_id: transaction_id.into(),
            profile,
            outcome: None,
        }
    }

    /// Adds the user-session unit controller.
    ///
    /// A builder method added beside `new` rather than an argument to it, so
    /// every existing construction keeps compiling with its own meaning: a
    /// driver that was never given a controller manages no units.
    pub fn with_user_units(mut self, user_units: &'a dyn UserUnitController) -> Self {
        self.user_units = Some(user_units);
        self
    }

    /// Translates a plan into the wire contract the daemon revalidates.
    ///
    /// `Ok(None)` when no step crosses the privileged boundary — a plan of
    /// only Verify, Enable, and Disable steps — so nothing is sent at all.
    pub fn wire_plan(&self, plan: &TransactionPlan) -> Result<Option<WirePlan>, FailureEvidence> {
        self.wire_plan_with(plan, true)
    }

    /// The same plan, with or without the companion packages (ADR 0018). A
    /// privileged service from before companions refuses the field, and the
    /// plan without it is exactly what that service always installed.
    fn wire_plan_with(
        &self,
        plan: &TransactionPlan,
        with_companions: bool,
    ) -> Result<Option<WirePlan>, FailureEvidence> {
        let mut steps = Vec::new();
        for step in plan.steps() {
            let Some(action) = wire_action(step.operation) else {
                // Enabling, disabling, and verifying do not cross the
                // privileged boundary in protocol version 1.
                continue;
            };
            let artifact = step.artifact.as_ref().map(|artifact| WireArtifact {
                filename: artifact.release_asset.clone(),
                sha256: artifact.sha256.clone(),
                size_bytes: artifact.expected_bytes.unwrap_or(1),
            });
            let companions = step
                .artifact
                .as_ref()
                .filter(|_| with_companions)
                .map(|artifact| {
                    artifact
                        .companions
                        .iter()
                        .map(|companion| WireCompanion {
                            package: companion.package.clone(),
                            artifact: WireArtifact {
                                filename: companion.release_asset.clone(),
                                sha256: companion.sha256.clone(),
                                size_bytes: companion.expected_bytes.unwrap_or(1),
                            },
                        })
                        .collect()
                })
                .unwrap_or_default();
            steps.push(WireStep {
                component: step.component.to_string(),
                action,
                before_version: step.before_version.clone(),
                after_version: step.after_version.clone(),
                artifact,
                companions,
            });
        }

        if steps.is_empty() {
            return Ok(None);
        }

        let wire = WirePlan {
            protocol_version: manager_ipc::PROTOCOL_VERSION,
            transaction_id: self.transaction_id.clone(),
            target_release: self.profile.release.clone(),
            target_architecture: self.profile.architecture.clone(),
            steps,
        };
        wire.validate().map_err(|error| {
            FailureEvidence::with_detail("daemon.plan_rejected", error.to_string())
        })?;
        Ok(Some(wire))
    }

    fn download_all(
        &self,
        plan: &TransactionPlan,
        progress: &mut dyn FnMut(StageProgress),
        cancel: &CancelToken,
    ) -> StageOutcome {
        for step in plan.steps() {
            if cancel.is_cancelled() {
                return StageOutcome::Failed(FailureEvidence::new("operation.cancelled"));
            }
            let Some(artifact) = &step.artifact else {
                continue;
            };
            // The step's own package, then each companion the daemon will
            // install with it, each verified against its own checksum.
            let files = std::iter::once((
                artifact.url.as_ref(),
                &artifact.sha256,
                &artifact.release_asset,
                artifact.expected_bytes,
            ))
            .chain(artifact.companions.iter().map(|companion| {
                (
                    companion.url.as_ref(),
                    &companion.sha256,
                    &companion.release_asset,
                    companion.expected_bytes,
                )
            }));
            for (url, sha256, release_asset, expected_bytes) in files {
                let outcome = self.fetch_and_stage(
                    step,
                    url,
                    sha256,
                    release_asset,
                    expected_bytes,
                    progress,
                );
                if !matches!(outcome, StageOutcome::Completed) {
                    return outcome;
                }
            }
        }
        StageOutcome::Completed
    }

    fn fetch_and_stage(
        &self,
        step: &PlanStep,
        url: Option<&String>,
        sha256: &str,
        release_asset: &str,
        expected_bytes: Option<u64>,
        progress: &mut dyn FnMut(StageProgress),
    ) -> StageOutcome {
        let Some(url) = url else {
            // A restore reinstalls something already fetched. If it is no
            // longer cached there is nothing to reinstall, and pretending
            // otherwise would promise a restore that cannot happen.
            if self.downloader.cache().holds(sha256) {
                return StageOutcome::Completed;
            }
            return StageOutcome::Failed(FailureEvidence::with_detail(
                "restore.artifact_missing",
                step.component.to_string(),
            ));
        };

        let request = DownloadRequest {
            component: step.component.clone(),
            url: url.clone(),
            sha256: sha256.to_string(),
            expected_bytes,
        };
        let component = step.component.clone();
        let outcome = self.downloader.fetch(&request, &mut |downloaded| {
            progress(StageProgress::Downloading {
                component: component.to_string(),
                received_bytes: downloaded.received_bytes,
                total_bytes: downloaded.total_bytes,
            });
        });

        match outcome {
            Ok(receipt) => {
                if let Err(error) = self.executor.stage_artifact(
                    &self.transaction_id,
                    release_asset,
                    sha256,
                    &receipt.artifact_path,
                ) {
                    return StageOutcome::Failed(evidence_for(&error));
                }
                StageOutcome::Completed
            }
            Err(error) => StageOutcome::Failed(evidence_for(&error)),
        }
    }

    fn apply(
        &mut self,
        plan: &TransactionPlan,
        progress: &mut dyn FnMut(StageProgress),
    ) -> StageOutcome {
        let wire = match self.wire_plan(plan) {
            Ok(Some(wire)) => wire,
            // Nothing here needs the privileged boundary.
            Ok(None) => return StageOutcome::Completed,
            Err(evidence) => return StageOutcome::Failed(evidence),
        };

        let component = wire.steps[0].component.clone();
        progress(StageProgress::Applying {
            component: component.clone(),
        });

        let mut answer = self.executor.execute_plan(&wire, &mut |_, _| {});
        if let Err(error) = &answer
            && refuses_companions(error)
            && wire.steps.iter().any(|step| !step.companions.is_empty())
        {
            // A privileged service from before ADR 0018. It never read the
            // plan, so nothing ran: send what it can install, the step's own
            // package alone, which recommends rather than depends on its
            // companion.
            match self.wire_plan_with(plan, false) {
                Ok(Some(plain)) => answer = self.executor.execute_plan(&plain, &mut |_, _| {}),
                Ok(None) => return StageOutcome::Completed,
                Err(evidence) => return StageOutcome::Failed(evidence),
            }
        }
        match answer {
            Ok(outcome) => {
                let result = outcome_to_stage(&outcome);
                self.outcome = Some(outcome);
                result
            }
            Err(error) => StageOutcome::Failed(evidence_for(&error)),
        }
    }

    /// The units a step's package installed, from dpkg's own file list.
    ///
    /// Reading the list is the only way to know which units belong to this
    /// component: no table of unit names by component id exists, and the
    /// manifest schema does not carry one.
    fn step_units(&self, step: &PlanStep) -> Result<Vec<String>, FailureEvidence> {
        if self.user_units.is_none() {
            return Ok(Vec::new());
        }
        let files = self
            .packages
            .installed_files(step.component.as_str())
            .map_err(|_| {
                FailureEvidence::with_detail(
                    "health.undetermined",
                    format!(
                        "the dpkg file list for {} could not be read",
                        step.component
                    ),
                )
            })?;
        Ok(service_units(&files))
    }

    fn enable_units(&self, step: &PlanStep) -> Result<(), FailureEvidence> {
        let Some(controller) = self.user_units else {
            return Ok(());
        };
        let units = self.step_units(step)?;
        if units.is_empty() {
            return Ok(());
        }
        controller.enable_now(&units).map_err(unit_failure)
    }

    /// Enables the units and restarts them: an update or a restore replaced
    /// the program a running unit executes, and only a restart runs the new
    /// one. A unit that was not running is simply started.
    fn restart_units(&self, step: &PlanStep) -> Result<(), FailureEvidence> {
        let Some(controller) = self.user_units else {
            return Ok(());
        };
        let units = self.step_units(step)?;
        if units.is_empty() {
            return Ok(());
        }
        controller.enable_and_restart(&units).map_err(unit_failure)
    }

    /// Verify on a component the record says is enabled: the record is the
    /// person's decision, so a unit that is not enabled and running is
    /// started before it is checked. A unit that is already fine is left
    /// alone, so "check again" on a healthy component starts nothing.
    fn start_recorded_units(&self, step: &PlanStep) -> Result<(), FailureEvidence> {
        let Some(controller) = self.user_units else {
            return Ok(());
        };
        let units = self.step_units(step)?;
        if units.is_empty() {
            return Ok(());
        }
        let needing = controller
            .states(&units)
            .map_err(unit_failure)?
            .into_iter()
            .filter(|state| !state.is_satisfied() && state.activity != UnitActivity::Starting)
            .map(|state| state.unit)
            .collect::<Vec<_>>();
        if needing.is_empty() {
            return Ok(());
        }
        controller.enable_now(&needing).map_err(unit_failure)
    }

    fn disable_units(&self, step: &PlanStep) -> Result<(), FailureEvidence> {
        let Some(controller) = self.user_units else {
            return Ok(());
        };
        let units = self.step_units(step)?;
        if units.is_empty() {
            return Ok(());
        }
        controller.disable_now(&units).map_err(unit_failure)
    }

    /// Stops the units of every component this plan removes, and returns the
    /// ones that were enabled, so a removal the daemon then refuses can put
    /// them back. Asking first is what makes the undo exact: a unit that was
    /// already disabled stays disabled.
    fn stop_units_for_removal(
        &self,
        plan: &TransactionPlan,
    ) -> Result<Vec<String>, FailureEvidence> {
        let Some(controller) = self.user_units else {
            return Ok(Vec::new());
        };
        let mut stopped = Vec::new();
        for step in plan
            .steps()
            .iter()
            .filter(|step| step.operation == DesiredOperation::Remove)
        {
            let units = self.step_units(step)?;
            if units.is_empty() {
                continue;
            }
            let enabled = controller
                .states(&units)
                .map_err(unit_failure)?
                .into_iter()
                .filter(|state| state.enabled)
                .map(|state| state.unit);
            if let Err(failure) = controller.disable_now(&units) {
                // Whatever this removal already stopped is started again
                // before it reports, so a refused removal changes nothing.
                let _ = self.restart_stopped(&stopped);
                return Err(unit_failure(failure));
            }
            stopped.extend(enabled);
        }
        Ok(stopped)
    }

    fn restart_stopped(&self, units: &[String]) -> Result<(), UnitFailure> {
        match self.user_units {
            Some(controller) if !units.is_empty() => controller.enable_now(units),
            _ => Ok(()),
        }
    }

    /// Fails when a step that leaves its component enabled has a unit that is
    /// not enabled and running, naming the unit rather than saying "unhealthy".
    ///
    /// A unit that is `enabled` but `inactive` is the case the owner's machine
    /// was in: the package was installed, dpkg was satisfied, and nothing was
    /// running. The health rule has to see that, so it asks systemd and not
    /// only dpkg.
    fn check_units(&self, plan: &TransactionPlan) -> Option<StageOutcome> {
        let controller = self.user_units?;
        for step in plan.steps().iter().filter(|step| step.leaves_enabled) {
            let units = match self.step_units(step) {
                Ok(units) => units,
                Err(evidence) => return Some(StageOutcome::Failed(evidence)),
            };
            if units.is_empty() {
                continue;
            }
            let states = match controller.states(&units) {
                Ok(states) => states,
                Err(failure) if failure.is_no_session() => {
                    return Some(StageOutcome::Failed(unit_failure(failure)));
                }
                Err(failure) => {
                    return Some(StageOutcome::Failed(FailureEvidence::with_detail(
                        "health.undetermined",
                        format!("{}: {}", failure.unit, failure.message),
                    )));
                }
            };
            for state in states {
                if state.is_satisfied() {
                    continue;
                }
                let reason = if state.masked {
                    "is masked"
                } else if !state.enabled {
                    "is not enabled"
                } else {
                    match state.activity {
                        UnitActivity::Starting => "is still starting",
                        UnitActivity::Failed => "has failed",
                        _ => "is not running",
                    }
                };
                return Some(StageOutcome::Failed(FailureEvidence::with_detail(
                    "user_units.not_running",
                    format!("{} {reason}", state.unit),
                )));
            }
        }
        None
    }

    /// The health stage: the daemon's verdict for what it applied, and the
    /// shared health rule, read without root, for every step the daemon never
    /// saw: Verify, Enable, and Disable.
    ///
    /// A finished transaction marks its components healthy and clears their
    /// failure, so a step nothing checked would turn a failed component into
    /// a healthy one by being enabled. A step the rule finds healthy passes; a
    /// failed check fails the stage with `health.failed` and the rule's detail;
    /// a check that could not run fails it with `health.undetermined`.
    fn check_health(&self, plan: &TransactionPlan) -> StageOutcome {
        if let Some(outcome) = &self.outcome {
            let stage = outcome_to_stage(outcome);
            if !matches!(stage, StageOutcome::Completed) {
                return stage;
            }
        }
        for step in plan.steps() {
            if wire_action(step.operation).is_some() {
                continue;
            }
            let package = step.component.to_string();
            match manager_platform::dpkg::verify_package(self.packages, self.health, &package) {
                HealthVerdict::Healthy => {}
                HealthVerdict::Failed(detail) => {
                    return StageOutcome::Failed(FailureEvidence::with_detail(
                        "health.failed",
                        detail,
                    ));
                }
                HealthVerdict::Undetermined(detail) => {
                    return StageOutcome::Failed(FailureEvidence::with_detail(
                        "health.undetermined",
                        detail,
                    ));
                }
            }
        }
        // dpkg can be completely satisfied while nothing is running: that is
        // the state this check exists to make visible.
        self.check_units(plan).unwrap_or(StageOutcome::Completed)
    }

    /// What the daemon reported, once the transaction has been handed over.
    pub fn outcome(&self) -> Option<&TransactionOutcome> {
        self.outcome.as_ref()
    }
}

impl StageDriver for RealDriver<'_> {
    fn run_stage(
        &mut self,
        stage: OperationStage,
        plan: &TransactionPlan,
        progress: &mut dyn FnMut(StageProgress),
        cancel: &CancelToken,
    ) -> StageOutcome {
        match stage {
            OperationStage::Downloading => self.download_all(plan, progress, cancel),
            OperationStage::Installing => {
                // A component being removed loses its units before the package
                // goes: once dpkg has taken the unit files away there is
                // nothing left to name, and a service still running from a
                // deleted file is exactly the state that cannot be described.
                let stopped = match self.stop_units_for_removal(plan) {
                    Ok(stopped) => stopped,
                    Err(evidence) => return StageOutcome::Failed(evidence),
                };
                let outcome = self.apply(plan, progress);
                if matches!(outcome, StageOutcome::Completed) {
                    return outcome;
                }
                // The daemon did not remove the package, so the record still
                // says enabled and the units it stopped have to run again.
                match (self.restart_stopped(&stopped), outcome) {
                    (Err(failure), StageOutcome::Failed(mut evidence)) => {
                        let again = format!(
                            "{} could not be started again: {}",
                            failure.unit, failure.message
                        );
                        evidence.detail = Some(match evidence.detail.take() {
                            Some(detail) => format!("{detail}; {again}"),
                            None => again,
                        });
                        StageOutcome::Failed(evidence)
                    }
                    (_, outcome) => outcome,
                }
            }
            // The daemon already applied configuration and health-checked what
            // it applied. Repeating either here would be guessing. What it does
            // not do is start the component's user units, which needs no
            // privilege and belongs to this process.
            OperationStage::ApplyingSettings => {
                if let Some(outcome) = &self.outcome {
                    let stage = outcome_to_stage(outcome);
                    if !matches!(stage, StageOutcome::Completed) {
                        return stage;
                    }
                }
                for step in plan.steps() {
                    let result = match step.operation {
                        DesiredOperation::Disable => self.disable_units(step),
                        // The units went before the package did.
                        DesiredOperation::Remove => Ok(()),
                        // The record says this component runs. Making that
                        // true is the repair, so a check on an enabled
                        // component starts what is stopped before it looks;
                        // a check on a disabled one only looks.
                        DesiredOperation::Verify if step.leaves_enabled => {
                            self.start_recorded_units(step)
                        }
                        DesiredOperation::Verify => Ok(()),
                        // A new program is on disk, and a unit that is
                        // already running is still executing the old one.
                        DesiredOperation::Update | DesiredOperation::Restore
                            if step.leaves_enabled =>
                        {
                            self.restart_units(step)
                        }
                        // A restore to a snapshot that was disabled is the
                        // mirror image: the units stop.
                        DesiredOperation::Restore => self.disable_units(step),
                        DesiredOperation::Install
                        | DesiredOperation::Update
                        | DesiredOperation::Enable
                            if step.leaves_enabled =>
                        {
                            self.enable_units(step)
                        }
                        DesiredOperation::Install
                        | DesiredOperation::Update
                        | DesiredOperation::Enable => Ok(()),
                    };
                    if let Err(evidence) = result {
                        return StageOutcome::Failed(evidence);
                    }
                }
                StageOutcome::Completed
            }
            OperationStage::CheckingHealth => self.check_health(plan),
        }
    }

    fn supports_cancel_at(&self, stage: OperationStage) -> bool {
        // Once the plan has gone to the daemon, the host may already have
        // changed, and abandoning would claim a restoration nothing performed.
        matches!(stage, OperationStage::Downloading)
    }
}

fn wire_action(operation: DesiredOperation) -> Option<WireAction> {
    match operation {
        DesiredOperation::Install => Some(WireAction::Install),
        DesiredOperation::Update => Some(WireAction::Update),
        DesiredOperation::Remove => Some(WireAction::Remove),
        DesiredOperation::Restore => Some(WireAction::Restore),
        DesiredOperation::Enable | DesiredOperation::Disable | DesiredOperation::Verify => None,
    }
}

/// Reads the daemon's verdict without softening it.
fn outcome_to_stage(outcome: &TransactionOutcome) -> StageOutcome {
    match &outcome.status {
        OutcomeStatus::Succeeded => StageOutcome::Completed,
        OutcomeStatus::Failed {
            error_key,
            recovery,
            ..
        } => match recovery {
            Some(WireRecovery::PartiallyRestored) => StageOutcome::RestoredPartially,
            Some(WireRecovery::ManualRecoveryRequired) => {
                StageOutcome::RestoreRequiresManualRecovery
            }
            _ => StageOutcome::Failed(split_daemon_key(error_key)),
        },
        OutcomeStatus::Cancelled => {
            StageOutcome::Failed(FailureEvidence::new("operation.cancelled"))
        }
        // Still running is not a result. Treating it as success would report a
        // transaction as done while the host is still being changed.
        OutcomeStatus::Accepted | OutcomeStatus::Executing { .. } => {
            StageOutcome::Failed(FailureEvidence::new("daemon.incomplete"))
        }
    }
}

/// Splits a daemon error key shaped `key:detail`.
///
/// The service reports one string. Recording it whole left the reason, the
/// half a person can act on, inside a key that presentation only
/// prefix-matches, so it is split here.
pub(crate) fn split_daemon_key(error_key: &str) -> FailureEvidence {
    match error_key.split_once(':') {
        Some((key, detail)) if !detail.trim().is_empty() => {
            FailureEvidence::with_detail(key, detail)
        }
        _ => FailureEvidence::new(error_key),
    }
}

/// A failed unit, as the step's evidence: the key names the kind of failure,
/// the detail names the unit and repeats systemctl's own words.
///
/// No user session is its own key: nothing was refused, there was nobody to
/// ask, and the person has to be told that rather than that a service broke.
fn unit_failure(failure: UnitFailure) -> FailureEvidence {
    if failure.is_no_session() {
        return FailureEvidence::with_detail(
            "user_units.no_session",
            format!(
                "{}: no user session is available to manage it ({})",
                failure.unit, failure.message
            ),
        );
    }
    FailureEvidence::with_detail(
        "user_units.failed",
        format!("{}: {}", failure.unit, failure.message),
    )
}

/// Whether the privileged service refused a plan because it does not know
/// the `companions` field: one from before ADR 0018, whose plan parser
/// refuses fields it does not know.
pub(crate) fn refuses_companions(error: &PlatformError) -> bool {
    error.to_string().contains("unknown field `companions`")
}

/// Maps a platform failure onto the stable keys a presentation layer localizes.
pub(crate) fn evidence_for(error: &PlatformError) -> FailureEvidence {
    match error {
        PlatformError::ChecksumMismatch { component } => {
            FailureEvidence::with_detail("download.checksum_mismatch", component.to_string())
        }
        PlatformError::DownloadFailed { component } => {
            FailureEvidence::with_detail("download.network", component.to_string())
        }
        PlatformError::DaemonUnavailable(detail) => {
            FailureEvidence::with_detail("daemon.unavailable", detail.clone())
        }
        PlatformError::DaemonInterrupted(detail) => {
            FailureEvidence::with_detail("daemon.interrupted", detail.clone())
        }
        PlatformError::PolkitDenied => FailureEvidence::new("daemon.polkit_denied"),
        PlatformError::PrivilegedExecutionNotApproved => {
            FailureEvidence::new("daemon.not_approved")
        }
        PlatformError::DaemonRefused(detail) => {
            FailureEvidence::with_detail("daemon.refused", detail.clone())
        }
        PlatformError::CapabilityUnavailable(detail) => {
            FailureEvidence::with_detail("platform.capability_unavailable", *detail)
        }
        // A transaction cannot reach this: a manager only exists once the host
        // was identified. It is here so the mapping stays total, and it keeps
        // the machine's own description rather than inventing a reason.
        PlatformError::UnsupportedHost(detail) => {
            FailureEvidence::with_detail("platform.unsupported_host", detail.clone())
        }
    }
}

/// What a caller watching a transaction gets told.
#[derive(Clone, Debug)]
pub enum RunnerEvent {
    StageEntered(OperationStage),
    Progress(StageProgress),
    /// The state was persisted. The payload is the state as saved, so a UI can
    /// adopt it rather than keeping its own copy in step.
    StateSaved(Box<ManagerState>),
    Finished(OperationProgress),
}

/// Persists state between stages.
///
/// `manager-core` cannot depend on `manager-store` without a cycle, so a runner
/// takes whatever its caller uses to save.
pub trait StateSink {
    fn save(&self, state: &ManagerState) -> Result<(), String>;
}

/// A sink that keeps nothing, for tests and for callers that persist elsewhere.
pub struct DiscardingSink;

impl StateSink for DiscardingSink {
    fn save(&self, _state: &ManagerState) -> Result<(), String> {
        Ok(())
    }
}

/// Runs an already-begun transaction to completion.
///
/// The runner is the single writer for the duration: it advances the state
/// machine, saves after each stage, and reports events. A caller that also
/// wrote to the same state would race it.
pub struct TransactionRunner<'a> {
    manager: &'a Manager,
    driver: Box<dyn StageDriver + 'a>,
    sink: &'a dyn StateSink,
    cancel: CancelToken,
}

impl<'a> TransactionRunner<'a> {
    pub fn new(
        manager: &'a Manager,
        driver: Box<dyn StageDriver + 'a>,
        sink: &'a dyn StateSink,
    ) -> Self {
        Self {
            manager,
            driver,
            sink,
            cancel: CancelToken::new(),
        }
    }

    pub fn with_cancel_token(mut self, cancel: CancelToken) -> Self {
        self.cancel = cancel;
        self
    }

    pub fn cancel_token(&self) -> CancelToken {
        self.cancel.clone()
    }

    /// Drives every remaining stage of the active transaction.
    ///
    /// Returns once the transaction finished, failed, or was cancelled. A
    /// cancellation that the driver can still honor abandons the transaction
    /// and restores the pre-operation snapshot.
    pub fn run(
        &mut self,
        state: &mut ManagerState,
        events: &mut dyn FnMut(RunnerEvent),
    ) -> Result<OperationProgress, ManagerError> {
        loop {
            let Some(active) = state.active_operation.as_ref() else {
                return Err(ManagerError::NoActiveOperation);
            };
            let stage = active.stage;
            let plan = active.plan.clone();
            events(RunnerEvent::StageEntered(stage));

            if self.cancel.is_cancelled() && self.driver.supports_cancel_at(stage) {
                self.manager.cancel(state)?;
                self.save(state, events)?;
                let progress = OperationProgress::Failed {
                    failure: crate::FailureRecord {
                        component: plan.steps()[0].component.clone(),
                        stage,
                        evidence: "operation.cancelled".to_string(),
                        detail: None,
                        recovery: None,
                    },
                };
                events(RunnerEvent::Finished(progress.clone()));
                return Ok(progress);
            }

            let outcome = self.driver.run_stage(
                stage,
                &plan,
                &mut |progress| events(RunnerEvent::Progress(progress)),
                &self.cancel,
            );
            let progress = self.manager.advance(state, outcome)?;
            self.save(state, events)?;

            match progress {
                OperationProgress::InProgress { .. } => continue,
                finished => {
                    events(RunnerEvent::Finished(finished.clone()));
                    return Ok(finished);
                }
            }
        }
    }

    fn save(
        &self,
        state: &ManagerState,
        events: &mut dyn FnMut(RunnerEvent),
    ) -> Result<(), ManagerError> {
        self.sink.save(state).map_err(ManagerError::StateNotSaved)?;
        events(RunnerEvent::StateSaved(Box::new(state.clone())));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ComponentStatus, DesiredOperation, ExecutionMode};
    use better_core::{ComponentCatalog, ComponentId, ComponentManifest};

    fn manager() -> Manager {
        let manifests = [
            include_str!("../../../components/manifests/better-manager.yaml"),
            include_str!("../../../components/manifests/better-monitor.yaml"),
        ]
        .into_iter()
        .map(|manifest| ComponentManifest::parse_yaml(manifest).unwrap())
        .collect::<Vec<_>>();
        Manager::new(
            ComponentCatalog::from_manifests(manifests).unwrap(),
            crate::SystemProfile::default(),
        )
    }

    fn monitor() -> ComponentId {
        ComponentId::new("better-monitor").unwrap()
    }

    fn install(
        manager: &Manager,
        state: &mut ManagerState,
        driver: MockDriver,
    ) -> OperationProgress {
        let plan = manager
            .plan(state, &monitor(), DesiredOperation::Install)
            .unwrap();
        manager.begin(state, plan).unwrap();
        let sink = DiscardingSink;
        let mut runner = TransactionRunner::new(manager, Box::new(driver), &sink);
        runner.run(state, &mut |_| {}).unwrap()
    }

    #[test]
    fn the_runner_walks_every_stage_and_finishes() {
        let manager = manager();
        let mut state = ManagerState::default();
        let mut stages = Vec::new();

        let plan = manager
            .plan(&state, &monitor(), DesiredOperation::Install)
            .unwrap();
        manager.begin(&mut state, plan).unwrap();
        let sink = DiscardingSink;
        let mut runner = TransactionRunner::new(&manager, Box::new(MockDriver::default()), &sink);
        let progress = runner
            .run(&mut state, &mut |event| {
                if let RunnerEvent::StageEntered(stage) = event {
                    stages.push(stage);
                }
            })
            .unwrap();

        assert_eq!(stages, OperationStage::ALL.to_vec());
        assert!(matches!(progress, OperationProgress::Finished { .. }));
        assert_eq!(
            manager.status(&state, &monitor()).unwrap(),
            ComponentStatus::Healthy
        );
        assert!(state.active_operation.is_none());
    }

    #[test]
    fn a_driver_failure_stops_the_transaction_at_that_stage() {
        let manager = manager();
        let mut state = ManagerState::default();
        let progress = install(
            &manager,
            &mut state,
            MockDriver::new(
                Some(OperationStage::Installing),
                MockRestoreOutcome::Succeed,
            ),
        );

        let OperationProgress::Failed { failure } = progress else {
            panic!("expected a failure");
        };
        assert_eq!(failure.stage, OperationStage::Installing);
        assert_eq!(failure.evidence, "mock_failure_at_installing");
    }

    #[test]
    fn the_mock_driver_agrees_with_the_scripted_mock_outcomes() {
        // Both paths must mean the same thing, or the demo and the lifecycle
        // suite would be exercising different lifecycles.
        let manager = manager();
        let state = ManagerState::default();
        let plan = manager
            .plan(&state, &monitor(), DesiredOperation::Install)
            .unwrap();

        for fail_at in [
            None,
            Some(OperationStage::Downloading),
            Some(OperationStage::CheckingHealth),
        ] {
            for restore_outcome in [
                MockRestoreOutcome::Succeed,
                MockRestoreOutcome::Partial,
                MockRestoreOutcome::ManualRecovery,
            ] {
                let mut driver = MockDriver::new(fail_at, restore_outcome);
                for stage in OperationStage::ALL {
                    let scripted = driver.mock_outcome(stage);
                    let observed = driver.run_stage(stage, &plan, &mut |_| {}, &CancelToken::new());
                    let expected = match scripted {
                        MockOutcome::FailAt(stage) => StageOutcome::Failed(FailureEvidence::new(
                            crate::failure_evidence(stage),
                        )),
                        MockOutcome::RestorePartially => StageOutcome::RestoredPartially,
                        MockOutcome::RestoreRequiresManualRecovery => {
                            StageOutcome::RestoreRequiresManualRecovery
                        }
                        MockOutcome::Succeed => StageOutcome::Completed,
                    };
                    assert_eq!(observed, expected, "{stage:?} with {fail_at:?}");
                }
            }
        }
    }

    #[test]
    fn a_cancelled_run_restores_the_state_from_before_the_transaction() {
        let manager = manager();
        let mut state = ManagerState::default();
        let plan = manager
            .plan(&state, &monitor(), DesiredOperation::Install)
            .unwrap();
        manager.begin(&mut state, plan).unwrap();

        let sink = DiscardingSink;
        let mut runner = TransactionRunner::new(&manager, Box::new(MockDriver::default()), &sink);
        runner.cancel_token().cancel();
        let progress = runner.run(&mut state, &mut |_| {}).unwrap();

        assert!(matches!(progress, OperationProgress::Failed { .. }));
        assert!(state.active_operation.is_none());
        assert_eq!(
            manager.status(&state, &monitor()).unwrap(),
            ComponentStatus::Available
        );
    }

    #[test]
    fn a_real_plan_is_not_cancelable_once_it_leaves_the_download_stage() {
        let manager = manager();
        let mut state = ManagerState::default();
        let plan = manager
            .plan_in_mode(
                &state,
                &monitor(),
                DesiredOperation::Install,
                ExecutionMode::Real,
            )
            .unwrap();
        manager.begin(&mut state, plan).unwrap();

        // Still downloading: abandoning is honest, because nothing was applied.
        manager.cancel(&mut state).unwrap();

        let plan = manager
            .plan_in_mode(
                &state,
                &monitor(),
                DesiredOperation::Install,
                ExecutionMode::Real,
            )
            .unwrap();
        manager.begin(&mut state, plan).unwrap();
        manager
            .advance(&mut state, StageOutcome::Completed)
            .unwrap();

        assert!(matches!(
            manager.cancel(&mut state),
            Err(ManagerError::NotCancelable {
                stage: OperationStage::Installing
            })
        ));
    }

    #[test]
    fn a_real_plan_becomes_a_wire_plan_the_daemon_will_accept() {
        let manager = manager();
        let state = ManagerState::default();
        let plan = manager
            .plan_in_mode(
                &state,
                &monitor(),
                DesiredOperation::Install,
                ExecutionMode::Real,
            )
            .unwrap();

        let downloader = HttpDownloader::new(manager_platform::download::ArtifactCache::new(
            std::env::temp_dir().join("better-os-wire-test"),
        ));
        let executor = manager_platform::UnapprovedPrivilegedExecutor;
        let packages = manager_platform::dpkg::FixedPackageStateProbe::new(&[]);
        let health = better_core::package_health::FakeHealthProbe::with_files(Vec::new());
        let driver = RealDriver::new(
            &downloader,
            &executor,
            &packages,
            &health,
            "3f2504e0-4f89-41d3-9a0c-0305e82c3301",
            crate::SystemProfile::default(),
        );

        let wire = driver
            .wire_plan(&plan)
            .expect("the plan translates")
            .expect("an install crosses the privileged boundary");
        // The daemon revalidates this from scratch, so it has to pass the
        // shared contract before it is worth sending.
        wire.validate().unwrap();
        assert_eq!(wire.steps.len(), 1);
        assert_eq!(wire.steps[0].component, "better-monitor");
        assert_eq!(wire.steps[0].action, WireAction::Install);
        assert!(wire.steps[0].artifact.is_some());
    }

    /// A privileged service that remembers every plan and, like one from
    /// before ADR 0018, refuses a plan that names companions.
    struct OldDaemon {
        plans: std::sync::Mutex<Vec<WirePlan>>,
        knows_companions: bool,
    }

    impl manager_platform::PrivilegedTransactionExecutor for OldDaemon {
        fn stage_artifact(
            &self,
            _transaction_id: &str,
            _filename: &str,
            _sha256: &str,
            _artifact_path: &std::path::Path,
        ) -> Result<(), PlatformError> {
            Ok(())
        }

        fn execute_plan(
            &self,
            plan: &WirePlan,
            _progress: &mut dyn FnMut(u32, manager_ipc::ExecutionStage),
        ) -> Result<TransactionOutcome, PlatformError> {
            self.plans.lock().unwrap().push(plan.clone());
            if !self.knows_companions && plan.steps.iter().any(|step| !step.companions.is_empty()) {
                return Err(PlatformError::DaemonRefused(
                    "daemon.error.plan_rejected:ipc.error.malformed:unknown field `companions`, expected one of `component`, `action`".to_string(),
                ));
            }
            Ok(TransactionOutcome {
                protocol_version: manager_ipc::PROTOCOL_VERSION,
                transaction_id: plan.transaction_id.clone(),
                status: OutcomeStatus::Succeeded,
                reports: Vec::new(),
                rollback_records: Vec::new(),
            })
        }
    }

    /// Better Manager's manifest as a release that ships the shell adapter
    /// would write it: the 24.04 amd64 artifact names its companion.
    fn manager_with_companion() -> Manager {
        let asset = "better-manager_0.2.12_ubuntu-24.04_amd64.deb";
        let manifest = include_str!("../../../components/manifests/better-manager.yaml").replace(
            &format!("    release_asset: {asset}\n"),
            &format!(
                "    release_asset: {asset}\n    companions:\n      - package: better-shell-adapter\n        url: https://example.com/better-shell-adapter_0.2.12_ubuntu-24.04_amd64.deb\n        sha256: {}\n        release_asset: better-shell-adapter_0.2.12_ubuntu-24.04_amd64.deb\n",
                "c".repeat(64)
            ),
        );
        let manifest = ComponentManifest::parse_yaml(&manifest).unwrap();
        assert_eq!(
            manifest
                .artifacts
                .iter()
                .filter(|a| !a.companions.is_empty())
                .count(),
            1
        );
        Manager::new(
            ComponentCatalog::from_manifests(vec![manifest]).unwrap(),
            crate::SystemProfile::default(),
        )
    }

    fn run_install(daemon: &OldDaemon) -> StageOutcome {
        let manager = manager_with_companion();
        let plan = manager
            .plan_in_mode(
                &ManagerState::default(),
                &ComponentId::new("better-manager").unwrap(),
                DesiredOperation::Install,
                ExecutionMode::Real,
            )
            .unwrap();
        let downloader = HttpDownloader::new(manager_platform::download::ArtifactCache::new(
            std::env::temp_dir().join("better-os-companion-test"),
        ));
        let packages = manager_platform::dpkg::FixedPackageStateProbe::new(&[]);
        let health = better_core::package_health::FakeHealthProbe::with_files(Vec::new());
        let mut driver = RealDriver::new(
            &downloader,
            daemon,
            &packages,
            &health,
            "3f2504e0-4f89-41d3-9a0c-0305e82c3301",
            crate::SystemProfile::default(),
        );
        driver.run_stage(
            OperationStage::Installing,
            &plan,
            &mut |_| {},
            &CancelToken::new(),
        )
    }

    /// ADR 0018: the companion travels in the same plan step, so the daemon
    /// installs both in one APT transaction.
    #[test]
    fn a_companion_travels_in_the_same_step_as_its_package() {
        let daemon = OldDaemon {
            plans: Default::default(),
            knows_companions: true,
        };
        assert_eq!(run_install(&daemon), StageOutcome::Completed);
        let plans = daemon.plans.lock().unwrap();
        assert_eq!(plans.len(), 1);
        let step = &plans[0].steps[0];
        assert_eq!(step.component, "better-manager");
        assert_eq!(step.companions.len(), 1);
        assert_eq!(step.companions[0].package, "better-shell-adapter");
        assert_eq!(
            step.companions[0].artifact.filename,
            "better-shell-adapter_0.2.12_ubuntu-24.04_amd64.deb"
        );
        plans[0].validate().unwrap();
    }

    /// A privileged service from before companions refuses the field without
    /// running anything; the manager then installs its own package alone,
    /// which only recommends the adapter, instead of failing its own update.
    #[test]
    fn an_older_privileged_service_gets_the_plan_without_companions() {
        let daemon = OldDaemon {
            plans: Default::default(),
            knows_companions: false,
        };
        assert_eq!(run_install(&daemon), StageOutcome::Completed);
        let plans = daemon.plans.lock().unwrap();
        assert_eq!(plans.len(), 2);
        assert_eq!(plans[0].steps[0].companions.len(), 1);
        assert!(plans[1].steps[0].companions.is_empty());
        assert_eq!(plans[1].transaction_id, plans[0].transaction_id);
    }

    #[test]
    fn operations_that_do_not_need_root_never_reach_the_wire() {
        assert_eq!(
            wire_action(DesiredOperation::Install),
            Some(WireAction::Install)
        );
        assert_eq!(
            wire_action(DesiredOperation::Update),
            Some(WireAction::Update)
        );
        assert_eq!(
            wire_action(DesiredOperation::Remove),
            Some(WireAction::Remove)
        );
        assert_eq!(
            wire_action(DesiredOperation::Restore),
            Some(WireAction::Restore)
        );

        assert_eq!(wire_action(DesiredOperation::Verify), None);
        assert_eq!(wire_action(DesiredOperation::Enable), None);
        assert_eq!(wire_action(DesiredOperation::Disable), None);
    }

    #[test]
    fn the_daemons_verdict_is_reported_as_it_stands() {
        let outcome = |status| TransactionOutcome {
            protocol_version: manager_ipc::PROTOCOL_VERSION,
            transaction_id: "3f2504e0-4f89-41d3-9a0c-0305e82c3301".to_string(),
            status,
            reports: Vec::new(),
            rollback_records: Vec::new(),
        };

        assert_eq!(
            outcome_to_stage(&outcome(OutcomeStatus::Succeeded)),
            StageOutcome::Completed
        );
        assert_eq!(
            outcome_to_stage(&outcome(OutcomeStatus::Failed {
                step_index: Some(0),
                error_key: "daemon.error.apt_failed:better-monitor".to_string(),
                recovery: Some(WireRecovery::PartiallyRestored),
            })),
            StageOutcome::RestoredPartially
        );
        assert_eq!(
            outcome_to_stage(&outcome(OutcomeStatus::Failed {
                step_index: Some(0),
                error_key: "daemon.error.apt_failed:better-monitor".to_string(),
                recovery: Some(WireRecovery::ManualRecoveryRequired),
            })),
            StageOutcome::RestoreRequiresManualRecovery
        );

        // A transaction still running is not a success. Reading it as one would
        // report the operation finished while the host is still changing.
        assert!(matches!(
            outcome_to_stage(&outcome(OutcomeStatus::Executing {
                step_index: 0,
                stage: manager_ipc::ExecutionStage::Applying,
            })),
            StageOutcome::Failed(_)
        ));
    }

    #[test]
    fn a_real_transaction_cannot_be_cancelled_once_it_has_been_handed_over() {
        let downloader = HttpDownloader::new(manager_platform::download::ArtifactCache::new(
            std::env::temp_dir().join("better-os-cancel-test"),
        ));
        let executor = manager_platform::UnapprovedPrivilegedExecutor;
        let packages = manager_platform::dpkg::FixedPackageStateProbe::new(&[]);
        let health = better_core::package_health::FakeHealthProbe::with_files(Vec::new());
        let driver = RealDriver::new(
            &downloader,
            &executor,
            &packages,
            &health,
            "3f2504e0-4f89-41d3-9a0c-0305e82c3301",
            crate::SystemProfile::default(),
        );

        assert!(driver.supports_cancel_at(OperationStage::Downloading));
        for stage in [
            OperationStage::Installing,
            OperationStage::ApplyingSettings,
            OperationStage::CheckingHealth,
        ] {
            assert!(
                !driver.supports_cancel_at(stage),
                "{stage:?} must not offer a cancel it cannot honor"
            );
        }
    }

    #[test]
    fn platform_failures_map_to_keys_a_presentation_layer_can_localize() {
        assert_eq!(
            evidence_for(&PlatformError::PolkitDenied).key,
            "daemon.polkit_denied"
        );
        assert_eq!(
            evidence_for(&PlatformError::ChecksumMismatch {
                component: monitor()
            })
            .key,
            "download.checksum_mismatch"
        );
        assert_eq!(
            evidence_for(&PlatformError::DaemonUnavailable("no service".to_string())).key,
            "daemon.unavailable"
        );
        assert_eq!(
            evidence_for(&PlatformError::DaemonInterrupted("no reply".to_string())).key,
            "daemon.interrupted"
        );
    }

    #[test]
    fn the_runner_reports_the_state_it_saved() {
        let manager = manager();
        let mut state = ManagerState::default();
        let mut saved = Vec::new();

        let plan = manager
            .plan(&state, &monitor(), DesiredOperation::Install)
            .unwrap();
        manager.begin(&mut state, plan).unwrap();
        let sink = DiscardingSink;
        let mut runner = TransactionRunner::new(&manager, Box::new(MockDriver::default()), &sink);
        runner
            .run(&mut state, &mut |event| {
                if let RunnerEvent::StateSaved(state) = event {
                    saved.push(state.revision);
                }
            })
            .unwrap();

        assert_eq!(saved.len(), OperationStage::ALL.len());
        assert_eq!(saved.last().copied(), Some(state.revision));
    }
}
