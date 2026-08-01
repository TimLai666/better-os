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

use crate::{
    FailureEvidence, Manager, ManagerError, ManagerState, MockOutcome, OperationProgress,
    OperationStage, StageOutcome, TransactionPlan,
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
