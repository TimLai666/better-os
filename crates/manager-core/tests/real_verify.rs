//! The real driver's verify path: a plan that never crosses the privileged
//! boundary must complete without the executor, and a verify must check the
//! host before it reports anything healthy.
//!
//! The field report behind these tests: Better Awake 0.2.7 was installed and
//! sound under dpkg, but recorded as failed. Every 重新檢查 then failed at the
//! install stage with `daemon.plan_rejected`, because a verify-only plan was
//! sent to the daemon as an empty plan.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use better_core::package_health::{FakeHealthProbe, HealthProbe, InstalledFiles};
use better_core::{ComponentCatalog, ComponentId, ComponentManifest};
use manager_core::exec::{DiscardingSink, RealDriver, RunnerEvent, TransactionRunner};
use manager_core::{
    ActivityKind, DesiredOperation, ExecutionMode, FailureRecord, HealthState, Manager,
    ManagerState, OperationProgress, OperationStage, SystemProfile, TransactionPlan,
};
use manager_ipc::{ExecutionStage, TransactionOutcome, WirePlan};
use manager_platform::download::{ArtifactCache, HttpDownloader};
use manager_platform::dpkg::{FixedPackageStateProbe, PackageStateProbe};
use manager_platform::{PlatformError, PrivilegedTransactionExecutor};

const TRANSACTION: &str = "3f2504e0-4f89-41d3-9a0c-0305e82c3301";
const INSTALLED: &str = "0.2.7";

/// Counts what reaches the privileged boundary. It refuses everything it is
/// asked, so a test that wrongly sends a step fails visibly as well as on the
/// count.
#[derive(Default)]
struct CountingExecutor {
    stage_calls: AtomicUsize,
    execute_calls: AtomicUsize,
}

impl CountingExecutor {
    fn calls(&self) -> usize {
        self.stage_calls.load(Ordering::SeqCst) + self.execute_calls.load(Ordering::SeqCst)
    }
}

impl PrivilegedTransactionExecutor for CountingExecutor {
    fn stage_artifact(
        &self,
        _transaction_id: &str,
        _filename: &str,
        _sha256: &str,
        _artifact_path: &Path,
    ) -> Result<(), PlatformError> {
        self.stage_calls.fetch_add(1, Ordering::SeqCst);
        Err(PlatformError::PrivilegedExecutionNotApproved)
    }

    fn execute_plan(
        &self,
        _plan: &WirePlan,
        _progress: &mut dyn FnMut(u32, ExecutionStage),
    ) -> Result<TransactionOutcome, PlatformError> {
        self.execute_calls.fetch_add(1, Ordering::SeqCst);
        Err(PlatformError::PrivilegedExecutionNotApproved)
    }
}

fn catalog() -> ComponentCatalog {
    let manifests = [
        include_str!("../../../components/manifests/better-manager.yaml"),
        include_str!("../../../components/manifests/better-monitor.yaml"),
        include_str!("../../../components/manifests/better-awake.yaml"),
    ]
    .into_iter()
    .map(|manifest| ComponentManifest::parse_yaml(manifest).unwrap())
    .collect::<Vec<_>>();

    ComponentCatalog::from_manifests(manifests).unwrap()
}

fn manager() -> Manager {
    Manager::new(catalog(), SystemProfile::default())
}

fn awake() -> ComponentId {
    ComponentId::new("better-awake").unwrap()
}

fn path(value: &str) -> PathBuf {
    PathBuf::from(value)
}

fn awake_files() -> InstalledFiles {
    InstalledFiles {
        paths: vec![
            path("/usr/bin/better-awake-service"),
            path("/usr/share/applications/better-awake.desktop"),
        ],
        ..InstalledFiles::default()
    }
}

/// Better Awake recorded as installed at 0.2.7 and failed, the way the field
/// machine was left after the cut-off update.
fn failed_awake(enabled: bool) -> ManagerState {
    let mut state = ManagerState::default();
    state.set_installed(awake(), INSTALLED, enabled);
    let record = state.components.get_mut(&awake()).unwrap();
    record.health = HealthState::Failed;
    record.failure = Some(FailureRecord {
        component: awake(),
        stage: OperationStage::CheckingHealth,
        evidence: "daemon.plan_rejected".to_string(),
        detail: None,
        recovery: None,
    });
    state
}

fn real_plan(
    manager: &Manager,
    state: &ManagerState,
    operation: DesiredOperation,
) -> TransactionPlan {
    manager
        .plan_in_mode(state, &awake(), operation, ExecutionMode::Real)
        .expect("the operation plans against this state")
}

/// Runs a transaction through the real driver and reports the progress, the
/// stages the runner entered, in order, and the executor it was given.
fn run(
    manager: &Manager,
    state: &mut ManagerState,
    plan: TransactionPlan,
    packages: &dyn PackageStateProbe,
    health: &dyn HealthProbe,
    executor: &CountingExecutor,
) -> (OperationProgress, Vec<OperationStage>) {
    let downloader = HttpDownloader::new(ArtifactCache::new(
        std::env::temp_dir().join("better-os-real-verify-test"),
    ));
    let driver = RealDriver::new(
        &downloader,
        executor,
        packages,
        health,
        TRANSACTION,
        SystemProfile::default(),
    );
    manager.begin(state, plan).unwrap();

    let sink = DiscardingSink;
    let mut runner = TransactionRunner::new(manager, Box::new(driver), &sink);
    let mut stages = Vec::new();
    let progress = runner
        .run(state, &mut |event| {
            if let RunnerEvent::StageEntered(stage) = event {
                stages.push(stage);
            }
        })
        .unwrap();
    (progress, stages)
}

fn last_success_evidence(state: &ManagerState) -> Option<String> {
    state
        .activity
        .iter()
        .rev()
        .find(|record| record.kind == ActivityKind::Success)
        .and_then(|record| record.evidence.clone())
}

#[test]
fn a_verify_only_plan_completes_without_reaching_the_executor() {
    let manager = manager();
    let mut state = failed_awake(true);
    let plan = real_plan(&manager, &state, DesiredOperation::Verify);
    let packages = FixedPackageStateProbe::new(&[("better-awake", INSTALLED)])
        .with_files("better-awake", awake_files());
    let health = FakeHealthProbe::with_files(vec![
        path("/usr/bin/better-awake-service"),
        path("/usr/share/applications/better-awake.desktop"),
    ]);
    let executor = CountingExecutor::default();

    let (progress, _) = run(&manager, &mut state, plan, &packages, &health, &executor);

    assert_eq!(
        executor.calls(),
        0,
        "a verify must never be sent to the daemon"
    );
    assert!(
        matches!(progress, OperationProgress::Finished { .. }),
        "a verify of a sound package must finish, got {progress:?}"
    );
    let refusals = state
        .activity
        .iter()
        .filter_map(|record| record.evidence.as_deref())
        .filter(|evidence| {
            evidence.contains("daemon.plan_rejected") || evidence.contains("ipc.error.empty_plan")
        })
        .collect::<Vec<_>>();
    assert!(
        refusals.is_empty(),
        "an empty verify plan must not be refused, got {refusals:?}"
    );
}

#[test]
fn a_sound_package_verifies_healthy_and_its_failure_is_cleared() {
    let manager = manager();
    let mut state = failed_awake(true);
    let plan = real_plan(&manager, &state, DesiredOperation::Verify);
    let packages = FixedPackageStateProbe::new(&[("better-awake", INSTALLED)])
        .with_files("better-awake", awake_files());
    let health = FakeHealthProbe::with_files(vec![
        path("/usr/bin/better-awake-service"),
        path("/usr/share/applications/better-awake.desktop"),
    ]);
    let executor = CountingExecutor::default();

    run(&manager, &mut state, plan, &packages, &health, &executor);

    let record = state.component(&awake()).unwrap();
    assert_eq!(record.health, HealthState::Healthy);
    assert!(record.failure.is_none(), "the failure must be cleared");
    assert_eq!(
        last_success_evidence(&state).as_deref(),
        Some("health_check_passed"),
        "a passed verify must be recorded under the non-mock key"
    );
}

#[test]
fn a_missing_file_fails_the_checking_health_stage_and_names_the_file() {
    let manager = manager();
    let mut state = failed_awake(true);
    let plan = real_plan(&manager, &state, DesiredOperation::Verify);
    let packages = FixedPackageStateProbe::new(&[("better-awake", INSTALLED)])
        .with_files("better-awake", awake_files());
    // The desktop entry dpkg lists is gone; the service binary is there.
    let health = FakeHealthProbe::with_files(vec![path("/usr/bin/better-awake-service")]);
    let executor = CountingExecutor::default();

    let (progress, stages) = run(&manager, &mut state, plan, &packages, &health, &executor);

    assert_eq!(executor.calls(), 0);
    assert_eq!(stages.last(), Some(&OperationStage::CheckingHealth));
    match progress {
        OperationProgress::Failed { failure } => {
            assert_eq!(failure.stage, OperationStage::CheckingHealth);
            assert_eq!(failure.evidence, "health.failed");
            let detail = failure.detail.unwrap_or_default();
            assert!(
                detail.contains("/usr/share/applications/better-awake.desktop"),
                "the detail should name the missing file, got: {detail}"
            );
        }
        other => panic!("a missing file must fail the verify, got {other:?}"),
    }
    let record = state.component(&awake()).unwrap();
    assert_ne!(
        record.health,
        HealthState::Healthy,
        "a component with a missing file must not be reported healthy"
    );
}

#[test]
fn a_file_list_that_cannot_be_read_fails_as_undetermined() {
    let manager = manager();
    let mut state = failed_awake(true);
    let plan = real_plan(&manager, &state, DesiredOperation::Verify);
    // Installed, but dpkg gives no file list to check against.
    let packages = FixedPackageStateProbe::new(&[("better-awake", INSTALLED)]);
    let health = FakeHealthProbe::empty();
    let executor = CountingExecutor::default();

    let (progress, stages) = run(&manager, &mut state, plan, &packages, &health, &executor);

    assert_eq!(executor.calls(), 0);
    assert_eq!(stages.last(), Some(&OperationStage::CheckingHealth));
    match progress {
        OperationProgress::Failed { failure } => {
            assert_eq!(failure.stage, OperationStage::CheckingHealth);
            assert_eq!(failure.evidence, "health.undetermined");
        }
        other => panic!("an unreadable file list must fail as undetermined, got {other:?}"),
    }
    assert_ne!(
        state.component(&awake()).unwrap().health,
        HealthState::Healthy,
        "an undetermined check must not report the component healthy"
    );
}

#[test]
fn an_enable_only_plan_completes_without_reaching_the_executor() {
    let manager = manager();
    let mut state = failed_awake(false);
    let plan = real_plan(&manager, &state, DesiredOperation::Enable);
    let packages = FixedPackageStateProbe::new(&[("better-awake", INSTALLED)])
        .with_files("better-awake", awake_files());
    let health = FakeHealthProbe::with_files(vec![
        path("/usr/bin/better-awake-service"),
        path("/usr/share/applications/better-awake.desktop"),
    ]);
    let executor = CountingExecutor::default();

    let (progress, _) = run(&manager, &mut state, plan, &packages, &health, &executor);

    assert_eq!(executor.calls(), 0);
    assert!(
        matches!(progress, OperationProgress::Finished { .. }),
        "an enable with no privileged step must finish, got {progress:?}"
    );
    assert!(state.component(&awake()).unwrap().enabled);
}

/// Finishing a transaction marks its components healthy and clears their
/// failure. Enabling a failed component whose files are gone must not be a way
/// to get that without a check.
#[test]
fn enabling_a_failed_component_with_a_missing_file_does_not_make_it_healthy() {
    let manager = manager();
    let mut state = failed_awake(false);
    let plan = real_plan(&manager, &state, DesiredOperation::Enable);
    let packages = FixedPackageStateProbe::new(&[("better-awake", INSTALLED)])
        .with_files("better-awake", awake_files());
    let health = FakeHealthProbe::with_files(vec![path("/usr/bin/better-awake-service")]);
    let executor = CountingExecutor::default();

    let (progress, _) = run(&manager, &mut state, plan, &packages, &health, &executor);

    assert_eq!(executor.calls(), 0);
    match progress {
        OperationProgress::Failed { failure } => {
            assert_eq!(failure.stage, OperationStage::CheckingHealth);
            assert_eq!(failure.evidence, "health.failed");
        }
        other => panic!("an unchecked enable must not finish, got {other:?}"),
    }
    let record = state.component(&awake()).unwrap();
    assert_ne!(record.health, HealthState::Healthy);
    assert!(
        record.failure.is_some(),
        "the failure must still be recorded"
    );
}

#[test]
fn a_disable_only_plan_completes_without_reaching_the_executor() {
    let manager = manager();
    let mut state = failed_awake(true);
    let plan = real_plan(&manager, &state, DesiredOperation::Disable);
    let packages = FixedPackageStateProbe::new(&[("better-awake", INSTALLED)])
        .with_files("better-awake", awake_files());
    let health = FakeHealthProbe::with_files(vec![
        path("/usr/bin/better-awake-service"),
        path("/usr/share/applications/better-awake.desktop"),
    ]);
    let executor = CountingExecutor::default();

    let (progress, _) = run(&manager, &mut state, plan, &packages, &health, &executor);

    assert_eq!(executor.calls(), 0);
    assert!(
        matches!(progress, OperationProgress::Finished { .. }),
        "a disable with no privileged step must finish, got {progress:?}"
    );
    assert!(!state.component(&awake()).unwrap().enabled);
}

#[test]
fn a_verify_or_enable_plan_has_no_wire_plan_and_an_install_has_one_step() {
    let manager = manager();
    let packages = FixedPackageStateProbe::new(&[("better-awake", INSTALLED)])
        .with_files("better-awake", awake_files());
    let health = FakeHealthProbe::with_files(vec![
        path("/usr/bin/better-awake-service"),
        path("/usr/share/applications/better-awake.desktop"),
    ]);
    let executor = CountingExecutor::default();
    let downloader = HttpDownloader::new(ArtifactCache::new(
        std::env::temp_dir().join("better-os-real-verify-wire-test"),
    ));
    let driver = RealDriver::new(
        &downloader,
        &executor,
        &packages,
        &health,
        TRANSACTION,
        SystemProfile::default(),
    );

    let verify = real_plan(&manager, &failed_awake(true), DesiredOperation::Verify);
    assert!(
        matches!(driver.wire_plan(&verify), Ok(None)),
        "a verify-only plan crosses no privileged boundary"
    );

    let enable = real_plan(&manager, &failed_awake(false), DesiredOperation::Enable);
    assert!(
        matches!(driver.wire_plan(&enable), Ok(None)),
        "an enable-only plan crosses no privileged boundary"
    );

    let monitor = ComponentId::new("better-monitor").unwrap();
    let install = manager
        .plan_in_mode(
            &ManagerState::default(),
            &monitor,
            DesiredOperation::Install,
            ExecutionMode::Real,
        )
        .unwrap();
    let wire = driver
        .wire_plan(&install)
        .expect("an install translates")
        .expect("an install crosses the privileged boundary");
    assert_eq!(wire.steps.len(), 1);
}
