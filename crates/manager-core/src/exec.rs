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
use manager_platform::removal_hooks::{HookFailure, RemovalHookRunner, before_removal_hook};
use manager_platform::shell_extensions::{
    ExtensionFailure, ShellExtensionController, shell_extensions,
};
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
    shell_extensions: Option<&'a dyn ShellExtensionController>,
    removal_hooks: Option<&'a dyn RemovalHookRunner>,
    transaction_id: String,
    profile: SystemProfile,
    /// Set once the transaction has been handed over, so the outcome can be
    /// reported at the stages that follow.
    outcome: Option<TransactionOutcome>,
    translation: Option<WireTranslation>,
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
            shell_extensions: None,
            removal_hooks: None,
            transaction_id: transaction_id.into(),
            profile,
            outcome: None,
            translation: None,
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

    /// Adds the GNOME Shell extension controller (ticket 88). A driver built
    /// without one turns no extension on or off.
    /// Adds the runner for a package's before-removal program (ticket 88).
    pub fn with_removal_hooks(mut self, hooks: &'a dyn RemovalHookRunner) -> Self {
        self.removal_hooks = Some(hooks);
        self
    }

    pub fn with_shell_extensions(mut self, extensions: &'a dyn ShellExtensionController) -> Self {
        self.shell_extensions = Some(extensions);
        self
    }

    /// Translates a plan into the wire contract the daemon revalidates.
    ///
    /// `Ok(None)` when no step crosses the privileged boundary — a plan of
    /// only Verify, Enable, and Disable steps — or when the only change is the
    /// service handoff (ADR 0028), so nothing is sent at all.
    pub fn wire_plan(&self, plan: &TransactionPlan) -> Result<Option<WirePlan>, FailureEvidence> {
        Ok(self.translate(plan)?.plan)
    }

    /// Whether a companion package differs from what the step leaves: absent,
    /// or at another version. Only those are fetched and sent.
    fn companion_needed(
        &self,
        package: &str,
        after_version: Option<&str>,
    ) -> Result<bool, FailureEvidence> {
        let host = self.packages.installed_version(package).map_err(|_| {
            FailureEvidence::with_detail(
                "health.undetermined",
                format!("the dpkg version of {package} could not be read"),
            )
        })?;
        Ok(match (host.as_deref(), after_version) {
            (Some(host), Some(after)) => manager_ipc::strip_epoch(host) != after,
            _ => true,
        })
    }

    /// The whole translation (ADR 0028): which companions go as companions,
    /// whether the privileged service goes as its own last step or through
    /// the handoff, and which companions wait for the new service.
    pub fn translate(&self, plan: &TransactionPlan) -> Result<WireTranslation, FailureEvidence> {
        let service_version = self
            .packages
            .installed_version(SERVICE_PACKAGE)
            .map_err(|_| {
                FailureEvidence::with_detail(
                    "health.undetermined",
                    format!("the dpkg version of {SERVICE_PACKAGE} could not be read"),
                )
            })?;
        let service = ServiceGeneration::of(service_version.as_deref());

        // First pass: what each step needs, so a companion that cannot go is
        // judged knowing whether this plan updates the service.
        let mut needs = Vec::new();
        let mut updates_service = false;
        for step in plan.steps() {
            let mut companions = Vec::new();
            let mut service_companion = None;
            if wire_action(step.operation).is_some()
                && let Some(artifact) = &step.artifact
            {
                for companion in &artifact.companions {
                    if !self.companion_needed(&companion.package, step.after_version.as_deref())? {
                        continue;
                    }
                    if companion.package == SERVICE_PACKAGE {
                        service_companion = Some(companion);
                        updates_service = true;
                    } else {
                        companions.push(companion);
                    }
                }
            }
            needs.push((step, companions, service_companion));
        }

        let mut translation = WireTranslation::default();
        let mut steps = Vec::new();
        let mut service_step = None;
        for (step, companions, service_companion) in needs {
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
            let names = || {
                companions
                    .iter()
                    .map(|c| c.package.clone())
                    .collect::<Vec<_>>()
            };
            let wire_companions = if companions.is_empty() || service.takes_companions() {
                companions
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
            } else if updates_service {
                // The service this plan replaces cannot take them; the new
                // one will. Recorded, never dropped without a word.
                translation.deferred_companions.extend(names());
                Vec::new()
            } else {
                return Err(service.outdated(&names()));
            };

            if let Some(companion) = service_companion {
                match service {
                    ServiceGeneration::Unorderable => {
                        return Err(FailureEvidence::with_detail(
                            "daemon.version_unreadable",
                            service_version.clone().unwrap_or_default(),
                        ));
                    }
                    ServiceGeneration::Legacy => {
                        // Its own prerm would stop the unit dpkg runs in. The
                        // new Better Manager's postinst hands the staged
                        // package to a unit of its own instead.
                        let handoff_version =
                            step.after_version.as_deref().is_some_and(carries_handoff);
                        let manager = crate::is_self_component(&step.component) && handoff_version;
                        let carries_handoff = manager && step.before_version != step.after_version;
                        // Better Manager is current, so installing it again is
                        // nothing to dpkg. The adapter it is missing carries the
                        // same postinst hook: the old service installs it as a
                        // step of its own (it needs no companions field), and
                        // its postinst starts the handoff again.
                        let adapter = companions
                            .iter()
                            .find(|companion| companion.package == HANDOFF_CARRIER);
                        let retry = manager
                            && step.operation == DesiredOperation::AddCompanions
                            && adapter.is_some();
                        if retry && let Some(adapter) = adapter {
                            translation.service_handoff = true;
                            translation
                                .deferred_companions
                                .retain(|package| package != HANDOFF_CARRIER);
                            steps.push(WireStep {
                                component: HANDOFF_CARRIER.to_string(),
                                action: WireAction::Install,
                                before_version: None,
                                after_version: step.after_version.clone(),
                                artifact: Some(WireArtifact {
                                    filename: adapter.release_asset.clone(),
                                    sha256: adapter.sha256.clone(),
                                    size_bytes: adapter.expected_bytes.unwrap_or(1),
                                }),
                                companions: Vec::new(),
                            });
                            continue;
                        }
                        if !carries_handoff {
                            return Err(service.outdated(&[SERVICE_PACKAGE.to_string()]));
                        }
                        translation.service_handoff = true;
                    }
                    _ => {
                        service_step = Some(WireStep {
                            component: SERVICE_PACKAGE.to_string(),
                            action: match (service_version.is_some(), step.operation) {
                                (_, DesiredOperation::Restore) => WireAction::Restore,
                                (true, _) => WireAction::Update,
                                (false, _) => WireAction::Install,
                            },
                            before_version: service_version.clone(),
                            after_version: step.after_version.clone(),
                            artifact: Some(WireArtifact {
                                filename: companion.release_asset.clone(),
                                sha256: companion.sha256.clone(),
                                size_bytes: companion.expected_bytes.unwrap_or(1),
                            }),
                            companions: Vec::new(),
                        });
                    }
                }
            }
            steps.push(WireStep {
                component: step.component.to_string(),
                action,
                before_version: step.before_version.clone(),
                after_version: step.after_version.clone(),
                artifact,
                companions: wire_companions,
            });
        }
        // Last, so every other package is in place before the service that
        // installed them is replaced.
        steps.extend(service_step);

        if steps.is_empty() {
            return Ok(translation);
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
        translation.plan = Some(wire);
        Ok(translation)
    }

    /// What the last translation deferred and handed off, for the
    /// presentation layer to say.
    pub fn translation(&self) -> Option<&WireTranslation> {
        self.translation.as_ref()
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
            .chain(
                artifact
                    .companions
                    .iter()
                    .filter(|companion| {
                        // A companion already at the step's version is not sent, so it
                        // is not fetched either. One that cannot be read is fetched,
                        // and the translation reports the unreadable version.
                        self.companion_needed(&companion.package, step.after_version.as_deref())
                            .unwrap_or(true)
                    })
                    .map(|companion| {
                        (
                            companion.url.as_ref(),
                            &companion.sha256,
                            &companion.release_asset,
                            companion.expected_bytes,
                        )
                    }),
            );
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
        let translation = match self.translate(plan) {
            Ok(translation) => translation,
            Err(evidence) => return StageOutcome::Failed(evidence),
        };
        let wire = translation.plan.clone();
        self.translation = Some(translation);
        let Some(wire) = wire else {
            // Nothing here needs the privileged boundary.
            return StageOutcome::Completed;
        };

        let component = wire.steps[0].component.clone();
        progress(StageProgress::Applying {
            component: component.clone(),
        });

        match self.executor.execute_plan(&wire, &mut |_, _| {}) {
            Ok(outcome) => {
                let result = outcome_to_stage(&outcome);
                self.outcome = Some(outcome);
                result
            }
            // A service older than the version it reported refused a field
            // it does not know. Nothing ran; the plan is not resent without
            // what it needed (ticket 124).
            Err(error) if refuses_companions(&error) => {
                StageOutcome::Failed(FailureEvidence::with_detail(
                    "daemon.outdated",
                    wire.steps
                        .iter()
                        .flat_map(|step| step.companions.iter().map(|c| c.package.clone()))
                        .collect::<Vec<_>>()
                        .join(", "),
                ))
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
        Ok(service_units(&self.step_files(step)?))
    }

    /// The GNOME Shell extensions a step's package installed, from the same
    /// file list.
    fn step_extensions(&self, step: &PlanStep) -> Result<Vec<String>, FailureEvidence> {
        if self.shell_extensions.is_none() {
            return Ok(Vec::new());
        }
        Ok(shell_extensions(&self.step_files(step)?))
    }

    /// Runs each removed package's before-removal program (ticket 88), so a
    /// setting it wrote in the person's session is put back while the
    /// program is still on disk. A driver built without a runner runs none.
    fn run_removal_hooks(&self, plan: &TransactionPlan) -> Result<(), FailureEvidence> {
        let Some(runner) = self.removal_hooks else {
            return Ok(());
        };
        for step in plan
            .steps()
            .iter()
            .filter(|step| step.operation == DesiredOperation::Remove)
        {
            let files = self.step_files(step)?;
            let Some(program) = before_removal_hook(step.component.as_str(), &files) else {
                continue;
            };
            runner.run(&program).map_err(hook_failure)?;
        }
        Ok(())
    }

    fn step_files(
        &self,
        step: &PlanStep,
    ) -> Result<better_core::package_health::InstalledFiles, FailureEvidence> {
        self.packages
            .installed_files(step.component.as_str())
            .map_err(|_| {
                FailureEvidence::with_detail(
                    "health.undetermined",
                    format!(
                        "the dpkg file list for {} could not be read",
                        step.component
                    ),
                )
            })
    }

    /// Turns the step's extensions on. A session with no GNOME Shell settings
    /// has nothing to turn on and is not a failure.
    fn enable_extensions(&self, step: &PlanStep) -> Result<(), FailureEvidence> {
        let Some(controller) = self.shell_extensions else {
            return Ok(());
        };
        let uuids = self.step_extensions(step)?;
        match controller.enable(&uuids) {
            Err(failure) if failure.is_no_shell() => Ok(()),
            result => result.map_err(|failure| extension_failure(&uuids, failure)),
        }
    }

    fn disable_extensions(&self, step: &PlanStep) -> Result<(), FailureEvidence> {
        let Some(controller) = self.shell_extensions else {
            return Ok(());
        };
        let uuids = self.step_extensions(step)?;
        match controller.disable(&uuids) {
            Err(failure) if failure.is_no_shell() => Ok(()),
            result => result.map_err(|failure| extension_failure(&uuids, failure)),
        }
    }

    /// Turns off the extensions of every component this plan removes, and
    /// returns the ones that were on, so a removal the daemon refuses can put
    /// them back.
    fn disable_extensions_for_removal(
        &self,
        plan: &TransactionPlan,
    ) -> Result<Vec<String>, FailureEvidence> {
        let Some(controller) = self.shell_extensions else {
            return Ok(Vec::new());
        };
        let mut turned_off = Vec::new();
        for step in plan
            .steps()
            .iter()
            .filter(|step| step.operation == DesiredOperation::Remove)
        {
            let uuids = self.step_extensions(step)?;
            if uuids.is_empty() {
                continue;
            }
            let was_on = match controller.enabled(&uuids) {
                Ok(on) => on,
                Err(failure) if failure.is_no_shell() => continue,
                Err(failure) => return Err(extension_failure(&uuids, failure)),
            };
            if let Err(failure) = controller.disable(&uuids) {
                let _ = controller.enable(&turned_off);
                return Err(extension_failure(&uuids, failure));
            }
            turned_off.extend(was_on);
        }
        Ok(turned_off)
    }

    /// Fails when a step that leaves its component enabled has an extension
    /// the person's settings do not have on.
    fn check_extensions(&self, plan: &TransactionPlan) -> Option<StageOutcome> {
        let controller = self.shell_extensions?;
        for step in plan.steps().iter().filter(|step| step.leaves_enabled) {
            let uuids = match self.step_extensions(step) {
                Ok(uuids) => uuids,
                Err(evidence) => return Some(StageOutcome::Failed(evidence)),
            };
            if uuids.is_empty() {
                continue;
            }
            let on = match controller.enabled(&uuids) {
                Ok(on) => on,
                Err(failure) if failure.is_no_shell() => continue,
                Err(failure) => {
                    return Some(StageOutcome::Failed(extension_failure(&uuids, failure)));
                }
            };
            if let Some(off) = uuids.iter().find(|uuid| !on.contains(uuid)) {
                return Some(StageOutcome::Failed(FailureEvidence::with_detail(
                    "shell_extension.not_enabled",
                    format!("the GNOME Shell extension {off} is not turned on"),
                )));
            }
        }
        None
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
                HealthVerdict::Healthy => {
                    // A component whose companion is missing is not healthy,
                    // whatever its own files say (ticket 124).
                    for companion in &step.verify_companions {
                        match self.companion_needed(companion, step.before_version.as_deref()) {
                            Ok(false) => {}
                            Ok(true) => {
                                return StageOutcome::Failed(FailureEvidence::with_detail(
                                    "companion.missing",
                                    companion.clone(),
                                ));
                            }
                            Err(evidence) => return StageOutcome::Failed(evidence),
                        }
                    }
                }
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
        self.check_units(plan)
            .or_else(|| self.check_extensions(plan))
            .unwrap_or(StageOutcome::Completed)
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
                // Settings the component wrote go back first, while its own
                // program is still there to do it. Nothing else has changed
                // yet, so a refusal here leaves the component as it was.
                if let Err(evidence) = self.run_removal_hooks(plan) {
                    return StageOutcome::Failed(evidence);
                }
                let stopped = match self.stop_units_for_removal(plan) {
                    Ok(stopped) => stopped,
                    Err(evidence) => return StageOutcome::Failed(evidence),
                };
                let turned_off = match self.disable_extensions_for_removal(plan) {
                    Ok(turned_off) => turned_off,
                    Err(evidence) => {
                        let _ = self.restart_stopped(&stopped);
                        return StageOutcome::Failed(evidence);
                    }
                };
                let outcome = self.apply(plan, progress);
                if matches!(outcome, StageOutcome::Completed) {
                    return outcome;
                }
                if let Some(controller) = self.shell_extensions {
                    let _ = controller.enable(&turned_off);
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
                        DesiredOperation::Disable => self
                            .disable_units(step)
                            .and_then(|()| self.disable_extensions(step)),
                        // The units went before the package did.
                        DesiredOperation::Remove => Ok(()),
                        // The record says this component runs. Making that
                        // true is the repair, so a check on an enabled
                        // component starts what is stopped before it looks;
                        // a check on a disabled one only looks.
                        DesiredOperation::Verify if step.leaves_enabled => self
                            .start_recorded_units(step)
                            .and_then(|()| self.enable_extensions(step)),
                        DesiredOperation::Verify | DesiredOperation::AddCompanions => Ok(()),
                        // A new program is on disk, and a unit that is
                        // already running is still executing the old one.
                        DesiredOperation::Update | DesiredOperation::Restore
                            if step.leaves_enabled =>
                        {
                            self.restart_units(step)
                                .and_then(|()| self.enable_extensions(step))
                        }
                        // A restore to a snapshot that was disabled is the
                        // mirror image: the units stop.
                        DesiredOperation::Restore => self
                            .disable_units(step)
                            .and_then(|()| self.disable_extensions(step)),
                        DesiredOperation::Install
                        | DesiredOperation::Update
                        | DesiredOperation::Enable
                            if step.leaves_enabled =>
                        {
                            self.enable_units(step)
                                .and_then(|()| self.enable_extensions(step))
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
        // The component's own package goes again at its own version, so the
        // service checks and installs it with the companions it lacks.
        DesiredOperation::AddCompanions => Some(WireAction::Update),
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
fn hook_failure(failure: HookFailure) -> FailureEvidence {
    let evidence = if failure.no_session {
        "before_removal.no_session"
    } else {
        "before_removal.failed"
    };
    FailureEvidence::with_detail(evidence, failure.message)
}

fn extension_failure(uuids: &[String], failure: ExtensionFailure) -> FailureEvidence {
    let names = uuids.join(", ");
    if failure.is_no_session() {
        return FailureEvidence::with_detail(
            "shell_extension.no_session",
            format!(
                "{names}: no user session is available to turn it on or off ({})",
                failure.message
            ),
        );
    }
    FailureEvidence::with_detail(
        "shell_extension.failed",
        format!("{names}: {}", failure.message),
    )
}

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

/// The privileged service's package, which travels as a companion of Better
/// Manager (ADR 0028) and is never sent as a companion.
pub const SERVICE_PACKAGE: &str = "better-manager-daemon";

/// The first service that runs its own upgrade in a transient unit outside
/// its cgroup. A release that is not this version changes it in the
/// version-bump commit.
pub const SELF_UPGRADING_SINCE: &str = "0.2.16";

/// The package whose postinst, beside Better Manager's, starts the handoff:
/// the retry when Better Manager is already current (ADR 0028).
pub const HANDOFF_CARRIER: &str = "better-shell-adapter";

/// The first Better Manager whose postinst carries the service handoff.
pub const HANDOFF_SINCE: &str = "0.2.16";

/// Whether a Better Manager of `version` ships the handoff.
pub fn carries_handoff(version: &str) -> bool {
    semver::Version::parse(version)
        .is_ok_and(|version| version >= semver::Version::parse(HANDOFF_SINCE).expect("a version"))
}

/// What the installed privileged service can be sent, from dpkg's version of
/// it (ADR 0028). Read unprivileged, before anything is sent.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ServiceGeneration {
    /// No package: whatever answers on the bus was not installed by dpkg (a
    /// development build). It is sent what the current service takes.
    NotInstalled,
    /// 0.2.12 and older: its prerm stops the unit dpkg runs in, and it does
    /// not know companions. Updated only through the handoff.
    Legacy,
    /// 0.2.13: safe maintainer scripts, no companions.
    Stepwise,
    /// 0.2.14 and 0.2.15: companions, and upgraded in its own unit by the
    /// ticket 105 scripts.
    Companions,
    /// 0.2.16 and later: upgrades itself in a transient unit.
    SelfUpgrading,
    /// dpkg holds a version nothing can order.
    Unorderable,
}

impl ServiceGeneration {
    pub fn of(installed: Option<&str>) -> Self {
        let Some(installed) = installed else {
            return Self::NotInstalled;
        };
        let Ok(version) = semver::Version::parse(manager_ipc::strip_epoch(installed)) else {
            return Self::Unorderable;
        };
        let at_least = |text: &str| version >= semver::Version::parse(text).expect("a version");
        if at_least(SELF_UPGRADING_SINCE) {
            Self::SelfUpgrading
        } else if at_least("0.2.14") {
            Self::Companions
        } else if at_least("0.2.13") {
            Self::Stepwise
        } else {
            Self::Legacy
        }
    }

    /// Whether a plan may carry `companions` to it.
    pub fn takes_companions(self) -> bool {
        matches!(
            self,
            Self::NotInstalled | Self::Companions | Self::SelfUpgrading
        )
    }

    /// Whether the service can be sent its own package as a step.
    pub fn takes_its_own_update(self) -> bool {
        matches!(
            self,
            Self::NotInstalled | Self::Stepwise | Self::Companions | Self::SelfUpgrading
        )
    }

    fn outdated(self, packages: &[String]) -> FailureEvidence {
        FailureEvidence::with_detail("daemon.outdated", packages.join(", "))
    }
}

/// Where the handoff (ADR 0028) writes its state, world-readable.
pub const HANDOFF_STATUS_PATH: &str = "/run/better-os/daemon-handoff";

/// What the handoff last wrote: two lines, the state and its detail.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HandoffStatus {
    Waiting,
    Installing,
    Done { version: String },
    Failed { reason: String },
}

impl HandoffStatus {
    /// `None` for a file that is not one the handoff writes, which is shown
    /// as nothing rather than as a guess.
    pub fn parse(text: &str) -> Option<Self> {
        let mut lines = text.lines();
        let state = lines.next()?.trim();
        let detail = lines.next().unwrap_or("").trim().to_string();
        match state {
            "waiting" => Some(Self::Waiting),
            "installing" => Some(Self::Installing),
            "done" => Some(Self::Done { version: detail }),
            "failed" => Some(Self::Failed { reason: detail }),
            _ => None,
        }
    }

    pub fn in_progress(&self) -> bool {
        matches!(self, Self::Waiting | Self::Installing)
    }
}

/// A plan as it goes to the service, and what it could not carry.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct WireTranslation {
    pub plan: Option<WirePlan>,
    /// Companions the current service cannot take, left for the service
    /// this plan installs. Doctor reports them until they are installed.
    pub deferred_companions: Vec<String>,
    /// The service goes through Better Manager's postinst handoff.
    pub service_handoff: bool,
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
        // The version follows the workspace, which every manifest carries,
        // so the fixture keeps matching after a version bump.
        let version = env!("CARGO_PKG_VERSION");
        let asset = format!("better-manager_{version}_ubuntu-24.04_amd64.deb");
        // The shipped manifest carries companions on every artifact since the
        // first release with the adapter; the fixture starts from one without
        // them so it controls exactly one.
        let shipped = include_str!("../../../components/manifests/better-manager.yaml");
        let mut bare = String::new();
        let mut skipping = false;
        for line in shipped.lines() {
            if line == "    companions:" {
                skipping = true;
                continue;
            }
            if skipping && line.starts_with("      ") {
                continue;
            }
            skipping = false;
            bare.push_str(line);
            bare.push('\n');
        }
        let manifest = bare.replace(
            &format!("    release_asset: {asset}\n"),
            &format!(
                "    release_asset: {asset}\n    companions:\n      - package: better-shell-adapter\n        url: https://example.com/better-shell-adapter_{version}_ubuntu-24.04_amd64.deb\n        sha256: {}\n        release_asset: better-shell-adapter_{version}_ubuntu-24.04_amd64.deb\n",
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
            format!(
                "better-shell-adapter_{}_ubuntu-24.04_amd64.deb",
                env!("CARGO_PKG_VERSION")
            )
        );
        plans[0].validate().unwrap();
    }

    /// A privileged service that claims to know companions and refuses them
    /// anyway: the plan is not resent without them (ticket 124).
    #[test]
    fn a_service_that_refuses_companions_is_never_worked_around() {
        let daemon = OldDaemon {
            plans: Default::default(),
            knows_companions: false,
        };
        match run_install(&daemon) {
            StageOutcome::Failed(evidence) => {
                assert_eq!(evidence.key, "daemon.outdated");
                assert_eq!(evidence.detail.as_deref(), Some("better-shell-adapter"));
            }
            other => panic!("expected the refusal to stand, got {other:?}"),
        }
        assert_eq!(daemon.plans.lock().unwrap().len(), 1, "no second plan");
    }

    /// Better Manager at `version` for 24.04 amd64, with exactly the named
    /// companions on that one artifact.
    fn manager_at(version: &str, companions: &[&str]) -> Manager {
        let shipped = include_str!("../../../components/manifests/better-manager.yaml");
        let mut bare = String::new();
        let mut skipping = false;
        for line in shipped.lines() {
            if line == "    companions:" {
                skipping = true;
                continue;
            }
            if skipping && line.starts_with("      ") {
                continue;
            }
            skipping = false;
            bare.push_str(line);
            bare.push('\n');
        }
        let bare = bare.replace(env!("CARGO_PKG_VERSION"), version);
        let asset = format!("better-manager_{version}_ubuntu-24.04_amd64.deb");
        let mut block = String::from("    companions:\n");
        for (index, package) in companions.iter().enumerate() {
            let file = format!("{package}_{version}_ubuntu-24.04_amd64.deb");
            block.push_str(&format!(
                "      - package: {package}\n        url: https://example.com/{file}\n        sha256: {}\n        release_asset: {file}\n",
                char::from(b'a' + index as u8).to_string().repeat(64)
            ));
        }
        let manifest = bare.replace(
            &format!("    release_asset: {asset}\n"),
            &format!("    release_asset: {asset}\n{block}"),
        );
        let manifest = ComponentManifest::parse_yaml(&manifest).unwrap();
        Manager::new(
            ComponentCatalog::from_manifests(vec![manifest]).unwrap(),
            crate::SystemProfile::default(),
        )
    }

    const ADAPTER: &str = "better-shell-adapter";

    /// Plans `operation` on Better Manager, installed at `installed`, and
    /// translates it against a host whose dpkg holds `host`.
    fn translate_on(
        version: &str,
        installed: &str,
        operation: DesiredOperation,
        host: &[(&str, &str)],
    ) -> Result<WireTranslation, FailureEvidence> {
        let manager = manager_at(version, &[ADAPTER, SERVICE_PACKAGE]);
        let mut state = ManagerState::default();
        state.set_installed(ComponentId::new("better-manager").unwrap(), installed, true);
        let plan = manager
            .plan_in_mode(
                &state,
                &ComponentId::new("better-manager").unwrap(),
                operation,
                ExecutionMode::Real,
            )
            .unwrap();
        let downloader = HttpDownloader::new(manager_platform::download::ArtifactCache::new(
            std::env::temp_dir().join("better-os-service-test"),
        ));
        let executor = manager_platform::UnapprovedPrivilegedExecutor;
        let packages = manager_platform::dpkg::FixedPackageStateProbe::new(host);
        let health = better_core::package_health::FakeHealthProbe::with_files(Vec::new());
        RealDriver::new(
            &downloader,
            &executor,
            &packages,
            &health,
            "3f2504e0-4f89-41d3-9a0c-0305e82c3301",
            crate::SystemProfile::default(),
        )
        .translate(&plan)
    }

    #[test]
    fn the_service_generation_follows_its_dpkg_version() {
        use ServiceGeneration::*;
        for (version, expected) in [
            (None, NotInstalled),
            (Some("0.2.9"), Legacy),
            (Some("0.2.12"), Legacy),
            (Some("0.2.13"), Stepwise),
            (Some("0.2.14"), Companions),
            (Some("0.2.15"), Companions),
            (Some("0.2.16"), SelfUpgrading),
            (Some("1:0.3.0"), SelfUpgrading),
            (Some("0.2.8~rc1"), Unorderable),
        ] {
            assert_eq!(ServiceGeneration::of(version), expected, "{version:?}");
        }
    }

    /// ADR 0028: a current service gets its own package as the plan's last
    /// step, and the adapter as a companion.
    #[test]
    fn a_current_service_is_offered_its_own_update_last() {
        let translation = translate_on(
            "0.2.17",
            "0.2.16",
            DesiredOperation::Update,
            &[("better-manager", "0.2.16"), (SERVICE_PACKAGE, "0.2.16")],
        )
        .unwrap();
        let plan = translation.plan.unwrap();
        assert_eq!(plan.steps.len(), 2);
        assert_eq!(plan.steps[0].component, "better-manager");
        assert_eq!(plan.steps[0].companions.len(), 1);
        assert_eq!(plan.steps[0].companions[0].package, ADAPTER);
        let service = &plan.steps[1];
        assert_eq!(service.component, SERVICE_PACKAGE);
        assert_eq!(service.action, WireAction::Update);
        assert_eq!(service.before_version.as_deref(), Some("0.2.16"));
        assert_eq!(service.after_version.as_deref(), Some("0.2.17"));
        assert!(!translation.service_handoff);
        assert!(translation.deferred_companions.is_empty());
    }

    /// A service already at the version, and an adapter already there, are
    /// not sent again.
    #[test]
    fn companions_already_in_place_are_not_sent() {
        let translation = translate_on(
            "0.2.17",
            "0.2.16",
            DesiredOperation::Update,
            &[(SERVICE_PACKAGE, "0.2.17"), (ADAPTER, "0.2.17")],
        )
        .unwrap();
        let plan = translation.plan.unwrap();
        assert_eq!(plan.steps.len(), 1);
        assert!(plan.steps[0].companions.is_empty());
    }

    /// 0.2.12 and older: the service is never a step. The Better Manager
    /// update goes alone and its postinst hands the staged service and
    /// adapter to a unit of their own.
    #[test]
    fn a_legacy_service_is_updated_through_the_handoff() {
        let translation = translate_on(
            "0.2.16",
            "0.2.15",
            DesiredOperation::Update,
            &[("better-manager", "0.2.15"), (SERVICE_PACKAGE, "0.2.9")],
        )
        .unwrap();
        assert!(translation.service_handoff);
        assert_eq!(translation.deferred_companions, vec![ADAPTER.to_string()]);
        let plan = translation.plan.unwrap();
        assert_eq!(plan.steps.len(), 1);
        assert_eq!(plan.steps[0].component, "better-manager");
        assert!(plan.steps[0].companions.is_empty());
        assert!(
            plan.steps
                .iter()
                .all(|step| step.component != SERVICE_PACKAGE)
        );
    }

    /// A Better Manager release from before the handoff cannot carry it, so
    /// a legacy service refuses the plan rather than losing the service.
    #[test]
    fn a_legacy_service_with_no_handoff_to_ride_is_refused() {
        let evidence = translate_on(
            "0.2.15",
            "0.2.14",
            DesiredOperation::Update,
            &[(SERVICE_PACKAGE, "0.2.9")],
        )
        .unwrap_err();
        assert_eq!(evidence.key, "daemon.outdated");
        assert_eq!(evidence.detail.as_deref(), Some(SERVICE_PACKAGE));
    }

    /// The owner's machine before the fix: Better Manager current, the
    /// adapter missing, the service 0.2.9. Nothing can carry the adapter, and
    /// the plan says so instead of succeeding without it.
    #[test]
    fn a_missing_adapter_is_added_and_a_failed_handoff_can_be_retried() {
        let evidence = translate_on(
            "0.2.16",
            "0.2.16",
            DesiredOperation::AddCompanions,
            &[(SERVICE_PACKAGE, "0.2.16"), ("better-manager", "0.2.16")],
        );
        // The service is current here, so the adapter is simply added.
        let plan = evidence.unwrap().plan.unwrap();
        assert_eq!(plan.steps.len(), 1);
        assert_eq!(plan.steps[0].action, WireAction::Update);
        assert_eq!(plan.steps[0].before_version.as_deref(), Some("0.2.16"));
        assert_eq!(plan.steps[0].after_version.as_deref(), Some("0.2.16"));
        assert_eq!(plan.steps[0].companions.len(), 1);
        assert_eq!(plan.steps[0].companions[0].package, ADAPTER);

        // A failed handoff with Better Manager current: the retry is the
        // adapter as a step of its own, which a 0.2.9 service takes and whose
        // postinst starts the handoff again (ADR 0028).
        let retry = translate_on(
            "0.2.16",
            "0.2.16",
            DesiredOperation::AddCompanions,
            &[(SERVICE_PACKAGE, "0.2.9")],
        )
        .unwrap();
        assert!(retry.service_handoff);
        assert!(retry.deferred_companions.is_empty());
        let plan = retry.plan.unwrap();
        assert_eq!(plan.steps.len(), 1);
        assert_eq!(plan.steps[0].component, ADAPTER);
        assert_eq!(plan.steps[0].action, WireAction::Install);
        assert!(plan.steps[0].companions.is_empty());

        // With the adapter already there, nothing can carry the handoff.
        let refused = translate_on(
            "0.2.16",
            "0.2.16",
            DesiredOperation::AddCompanions,
            &[(SERVICE_PACKAGE, "0.2.9"), (ADAPTER, "0.2.16")],
        )
        .unwrap_err();
        assert_eq!(refused.key, "daemon.outdated");
    }

    /// 0.2.13 takes its own update and not companions: the adapter waits for
    /// the new service, and the translation says so.
    #[test]
    fn a_stepwise_service_defers_the_adapter_to_its_successor() {
        let translation = translate_on(
            "0.2.16",
            "0.2.15",
            DesiredOperation::Update,
            &[(SERVICE_PACKAGE, "0.2.13")],
        )
        .unwrap();
        assert_eq!(translation.deferred_companions, vec![ADAPTER.to_string()]);
        let plan = translation.plan.unwrap();
        assert_eq!(plan.steps.len(), 2);
        assert!(plan.steps[0].companions.is_empty());
        assert_eq!(plan.steps[1].component, SERVICE_PACKAGE);
    }

    #[test]
    fn an_unorderable_service_version_is_reported_not_guessed() {
        let evidence = translate_on(
            "0.2.16",
            "0.2.15",
            DesiredOperation::Update,
            &[(SERVICE_PACKAGE, "0.2.8~rc1")],
        )
        .unwrap_err();
        assert_eq!(evidence.key, "daemon.version_unreadable");
    }

    /// Verify checks the companions too: a component missing one fails its
    /// check (ticket 124).
    #[test]
    fn verify_fails_a_component_missing_its_companion() {
        let manager = manager_at("0.2.16", &[ADAPTER]);
        let id = ComponentId::new("better-manager").unwrap();
        let mut state = ManagerState::default();
        state.set_installed(id.clone(), "0.2.16", true);
        let plan = manager
            .plan_in_mode(&state, &id, DesiredOperation::Verify, ExecutionMode::Real)
            .unwrap();
        assert_eq!(plan.steps()[0].verify_companions, vec![ADAPTER.to_string()]);
        let downloader = HttpDownloader::new(manager_platform::download::ArtifactCache::new(
            std::env::temp_dir().join("better-os-verify-companion-test"),
        ));
        let executor = manager_platform::UnapprovedPrivilegedExecutor;
        let files = better_core::package_health::InstalledFiles::default();
        let check = |host: &[(&str, &str)]| {
            let packages = manager_platform::dpkg::FixedPackageStateProbe::new(host)
                .with_files("better-manager", files.clone());
            let health = better_core::package_health::FakeHealthProbe::with_files(Vec::new());
            let mut driver = RealDriver::new(
                &downloader,
                &executor,
                &packages,
                &health,
                "3f2504e0-4f89-41d3-9a0c-0305e82c3301",
                crate::SystemProfile::default(),
            );
            driver.run_stage(
                OperationStage::CheckingHealth,
                &plan,
                &mut |_| {},
                &CancelToken::new(),
            )
        };
        match check(&[("better-manager", "0.2.16")]) {
            StageOutcome::Failed(evidence) => {
                assert_eq!(evidence.key, "companion.missing");
                assert_eq!(evidence.detail.as_deref(), Some(ADAPTER));
            }
            other => panic!("expected a finding, got {other:?}"),
        }
        assert_eq!(
            check(&[("better-manager", "0.2.16"), (ADAPTER, "0.2.16")]),
            StageOutcome::Completed
        );
    }

    #[test]
    fn the_handoff_status_is_read_as_written_and_nothing_else() {
        assert_eq!(
            HandoffStatus::parse("waiting\nthe old service\n"),
            Some(HandoffStatus::Waiting)
        );
        assert_eq!(
            HandoffStatus::parse("done\n0.2.16\n"),
            Some(HandoffStatus::Done {
                version: "0.2.16".to_string()
            })
        );
        assert_eq!(
            HandoffStatus::parse("failed\nthe old service did not go idle\n"),
            Some(HandoffStatus::Failed {
                reason: "the old service did not go idle".to_string()
            })
        );
        assert!(HandoffStatus::parse("installing\n").unwrap().in_progress());
        assert_eq!(HandoffStatus::parse(""), None);
        assert_eq!(HandoffStatus::parse("finished\n"), None);
    }

    /// Doctor reports a missing companion of an installed component, and
    /// the repair is planned only for the catalog's own version.
    #[test]
    fn doctor_reports_a_missing_companion_and_plans_its_repair() {
        let manager = manager_at("0.2.16", &[ADAPTER, SERVICE_PACKAGE]);
        let id = ComponentId::new("better-manager").unwrap();
        let mut state = ManagerState::default();
        state.set_installed(id.clone(), "0.2.16", true);

        let host =
            manager_platform::dpkg::FixedPackageStateProbe::new(&[(SERVICE_PACKAGE, "0.2.16")]);
        let checks = manager.companion_checks(&state, &host).unwrap();
        assert_eq!(checks.len(), 1);
        assert_eq!(checks[0].kind, crate::DoctorCheckKind::CompanionMissing);
        assert_eq!(checks[0].status, crate::DoctorCheckStatus::Failed);
        assert_eq!(checks[0].package.as_deref(), Some(ADAPTER));
        assert_eq!(checks[0].host_version, None);

        let old_service = manager_platform::dpkg::FixedPackageStateProbe::new(&[
            (SERVICE_PACKAGE, "0.2.9"),
            (ADAPTER, "0.2.16"),
        ]);
        let gaps = manager.companion_gaps(&state, &old_service).unwrap();
        assert_eq!(gaps.len(), 1);
        assert_eq!(gaps[0].package, SERVICE_PACKAGE);
        assert_eq!(gaps[0].host_version.as_deref(), Some("0.2.9"));

        // In a whole report the finding comes first and the component's own
        // health check fails with it.
        let mut report = manager.doctor(&state).unwrap();
        let gaps = manager.companion_gaps(&state, &host).unwrap();
        crate::merge_companion_findings(&mut report, &gaps);
        assert_eq!(report[0].kind, crate::DoctorCheckKind::CompanionMissing);
        assert!(report.iter().any(|check| {
            check.kind == crate::DoctorCheckKind::ComponentHealth
                && check.component.as_ref() == Some(&id)
                && check.status == crate::DoctorCheckStatus::Failed
        }));

        let whole = manager_platform::dpkg::FixedPackageStateProbe::new(&[
            (SERVICE_PACKAGE, "0.2.16"),
            (ADAPTER, "0.2.16"),
        ]);
        assert!(manager.companion_checks(&state, &whole).unwrap().is_empty());

        let plan = manager
            .plan_in_mode(
                &state,
                &id,
                DesiredOperation::AddCompanions,
                ExecutionMode::Real,
            )
            .unwrap();
        assert_eq!(plan.steps().len(), 1);
        assert_eq!(plan.steps()[0].before_version.as_deref(), Some("0.2.16"));
        assert_eq!(plan.steps()[0].after_version.as_deref(), Some("0.2.16"));
        let mut begun = state.clone();
        manager
            .begin(&mut begun, plan)
            .expect("the reviewed plan begins");

        // An older install is an update away from its companions.
        state.set_installed(id.clone(), "0.2.15", true);
        assert!(manager.companion_gaps(&state, &host).unwrap().is_empty());
        assert!(matches!(
            manager.plan_in_mode(
                &state,
                &id,
                DesiredOperation::AddCompanions,
                ExecutionMode::Real
            ),
            Err(ManagerError::NoCompanionsToAdd(_))
        ));
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
