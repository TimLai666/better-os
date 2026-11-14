//! An installed component runs.
//!
//! The packages install their systemd user units without enabling them, and
//! the manager's records said "enabled" while nothing was running. These tests
//! drive a real [`RealDriver`] over its stages with a fake dpkg, a fake health
//! probe, a fake privileged executor, and a fake unit controller, so every
//! lifecycle operation and the startup pass are checked without touching this
//! machine's user manager.

use std::sync::{Arc, Mutex};

use better_core::package_health::{FakeHealthProbe, InstalledFiles};
use better_core::{ComponentCatalog, ComponentId, ComponentManifest};
use manager_core::exec::{CancelToken, DiscardingSink, RealDriver, StageDriver, TransactionRunner};
use manager_core::{
    ComponentRecord, ComponentSnapshot, DesiredOperation, ExecutionMode, HealthState,
    InstallProvenance, Manager, ManagerState, OperationProgress, OperationStage, PlanArtifact,
    StageOutcome, SystemProfile,
};
use manager_ipc::{ExecutionStage, OutcomeStatus, TransactionOutcome, WirePlan};
use manager_platform::download::{ArtifactCache, HttpDownloader};
use manager_platform::dpkg::FixedPackageStateProbe;
use manager_platform::user_units::{
    FakeUnitCall, FakeUserUnitController, UnitActivity, UnitFailure, UnitState, UserUnitController,
};
use manager_platform::{PlatformError, PrivilegedTransactionExecutor};

fn manager() -> Manager {
    let manifests = [
        include_str!("../../../components/manifests/better-awake.yaml"),
        include_str!("../../../components/manifests/better-monitor.yaml"),
    ]
    .into_iter()
    .map(|manifest| ComponentManifest::parse_yaml(manifest).unwrap())
    .collect::<Vec<_>>();
    Manager::new(
        ComponentCatalog::from_manifests(manifests).unwrap(),
        SystemProfile::default(),
    )
}

fn awake() -> ComponentId {
    ComponentId::new("better-awake").unwrap()
}

const AWAKE_UNIT: &str = "better-awake.service";
const MONITOR_UNIT: &str = "better-monitor.service";

fn awake_files() -> InstalledFiles {
    InstalledFiles {
        paths: [
            "/usr/bin/better-awake-service",
            "/usr/lib/systemd/user",
            "/usr/lib/systemd/user/better-awake.service",
        ]
        .iter()
        .map(std::path::PathBuf::from)
        .collect(),
        ..InstalledFiles::default()
    }
}

fn monitor_files() -> InstalledFiles {
    InstalledFiles {
        paths: [
            "/usr/bin/better-monitor-service",
            "/usr/lib/systemd/user/better-monitor.service",
        ]
        .iter()
        .map(std::path::PathBuf::from)
        .collect(),
        ..InstalledFiles::default()
    }
}

fn probe_for(files: &InstalledFiles) -> FakeHealthProbe {
    FakeHealthProbe::with_files(files.paths.clone())
}

fn stopped(unit: &str) -> UnitState {
    UnitState::stopped(unit)
}

fn running(unit: &str) -> UnitState {
    UnitState::running(unit)
}

fn enabled_but_stopped(unit: &str) -> UnitState {
    UnitState::new(unit, true, UnitActivity::Stopped)
}

/// Enabled, and finished on its own with success: the touchpad gesture
/// service when it has nothing to do.
fn exited(unit: &str) -> UnitState {
    UnitState::new(unit, true, UnitActivity::Exited)
}

fn record(version: &str, enabled: bool) -> ComponentRecord {
    ComponentRecord {
        installed_version: Some(version.to_string()),
        enabled,
        health: HealthState::Healthy,
        installed_artifact: None,
        provenance: InstallProvenance::Manager,
        restore_snapshot: None,
        failure: None,
        recovery: None,
        drift: None,
    }
}

fn state_with(component: &ComponentId, record: ComponentRecord) -> ManagerState {
    let mut state = ManagerState::default();
    state.components.insert(component.clone(), record);
    state
}

/// A privileged executor that says the daemon applied the plan. It records the
/// handover in a shared log so a test can prove an uninstall stopped the units
/// before the package went.
struct SucceedingExecutor {
    log: Arc<Mutex<Vec<String>>>,
}

impl SucceedingExecutor {
    fn new(log: Arc<Mutex<Vec<String>>>) -> Self {
        Self { log }
    }
}

impl PrivilegedTransactionExecutor for SucceedingExecutor {
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
        _progress: &mut dyn FnMut(u32, ExecutionStage),
    ) -> Result<TransactionOutcome, PlatformError> {
        let components = plan
            .steps
            .iter()
            .map(|step| step.component.clone())
            .collect::<Vec<_>>()
            .join(",");
        self.log.lock().unwrap().push(format!("apply:{components}"));
        Ok(TransactionOutcome {
            protocol_version: manager_ipc::PROTOCOL_VERSION,
            transaction_id: plan.transaction_id.clone(),
            status: OutcomeStatus::Succeeded,
            reports: Vec::new(),
            rollback_records: Vec::new(),
        })
    }
}

/// The fake controller, plus a shared log, so a test can see the order of
/// unit work against the handover to the privileged service.
struct LoggingUnits {
    inner: FakeUserUnitController,
    log: Arc<Mutex<Vec<String>>>,
}

impl LoggingUnits {
    fn new(inner: FakeUserUnitController, log: Arc<Mutex<Vec<String>>>) -> Self {
        Self { inner, log }
    }
}

impl UserUnitController for LoggingUnits {
    fn enable_now(&self, units: &[String]) -> Result<(), UnitFailure> {
        self.log
            .lock()
            .unwrap()
            .push(format!("enable:{}", units.join(",")));
        self.inner.enable_now(units)
    }

    fn disable_now(&self, units: &[String]) -> Result<(), UnitFailure> {
        self.log
            .lock()
            .unwrap()
            .push(format!("disable:{}", units.join(",")));
        self.inner.disable_now(units)
    }

    fn states(&self, units: &[String]) -> Result<Vec<UnitState>, UnitFailure> {
        self.log
            .lock()
            .unwrap()
            .push(format!("states:{}", units.join(",")));
        self.inner.states(units)
    }

    fn enable_and_restart(&self, units: &[String]) -> Result<(), UnitFailure> {
        self.log
            .lock()
            .unwrap()
            .push(format!("restart:{}", units.join(",")));
        self.inner.enable_and_restart(units)
    }
}

/// A controller whose start is accepted and changes nothing, so the unit is
/// still stopped when the health stage looks. The real controller watches a
/// unit after starting it; this is the health check's own line of defence.
struct IgnoresStart(FakeUserUnitController);

impl UserUnitController for IgnoresStart {
    fn enable_now(&self, _units: &[String]) -> Result<(), UnitFailure> {
        Ok(())
    }

    fn disable_now(&self, units: &[String]) -> Result<(), UnitFailure> {
        self.0.disable_now(units)
    }

    fn states(&self, units: &[String]) -> Result<Vec<UnitState>, UnitFailure> {
        self.0.states(units)
    }

    fn enable_and_restart(&self, _units: &[String]) -> Result<(), UnitFailure> {
        Ok(())
    }
}

/// One downloader for the whole test binary. It is never asked to fetch
/// anything — the tests start after the downloading stage — and leaking it
/// keeps the borrow simple.
fn downloader() -> &'static HttpDownloader {
    Box::leak(Box::new(HttpDownloader::new(ArtifactCache::new(
        std::env::temp_dir().join("better-os-user-unit-test-cache"),
    ))))
}

/// Runs the stages after Downloading.
///
/// Downloading is skipped deliberately: a real plan names a URL, and a test
/// that reached the network would be testing the network. The stage still
/// advances, so the lifecycle is the one the product drives.
fn run_after_download(
    manager: &Manager,
    state: &mut ManagerState,
    driver: &mut RealDriver<'_>,
) -> OperationProgress {
    manager
        .advance(state, StageOutcome::Completed)
        .expect("downloading completes");
    loop {
        let (stage, plan) = {
            let active = state
                .active_operation
                .as_ref()
                .expect("the transaction is still active");
            (active.stage, active.plan.clone())
        };
        let outcome = driver.run_stage(stage, &plan, &mut |_| {}, &CancelToken::new());
        let progress = manager.advance(state, outcome).expect("the stage advances");
        match progress {
            OperationProgress::InProgress { .. } => continue,
            finished => return finished,
        }
    }
}

fn plan_and_run(
    manager: &Manager,
    state: &mut ManagerState,
    operation: DesiredOperation,
    driver: &mut RealDriver<'_>,
) -> OperationProgress {
    let plan = manager
        .plan_in_mode(state, &awake(), operation, ExecutionMode::Real)
        .expect("the operation plans");
    manager.begin(state, plan).expect("the plan begins");
    run_after_download(manager, state, driver)
}

#[test]
fn an_install_enables_and_starts_the_component_units() {
    let manager = manager();
    let mut state = ManagerState::default();
    let packages = FixedPackageStateProbe::new(&[("better-awake", "0.2.9")])
        .with_files("better-awake", awake_files());
    let health = probe_for(&awake_files());
    let units = FakeUserUnitController::new(&[stopped(AWAKE_UNIT)]);
    let executor = SucceedingExecutor::new(Arc::new(Mutex::new(Vec::new())));
    let mut driver = RealDriver::new(
        downloader(),
        &executor,
        &packages,
        &health,
        "3f2504e0-4f89-41d3-9a0c-0305e82c3301",
        SystemProfile::default(),
    )
    .with_user_units(&units);

    let progress = plan_and_run(&manager, &mut state, DesiredOperation::Install, &mut driver);

    assert!(matches!(progress, OperationProgress::Finished { .. }));
    assert_eq!(
        units.state(AWAKE_UNIT),
        Some(running(AWAKE_UNIT)),
        "the unit is enabled and started"
    );
    assert_eq!(
        units.calls(),
        vec![
            FakeUnitCall::Enable(vec![AWAKE_UNIT.to_string()]),
            FakeUnitCall::States(vec![AWAKE_UNIT.to_string()]),
        ],
        "enabling happens once, and the health stage only observes"
    );
    let record = state.components.get(&awake()).unwrap();
    assert!(record.enabled);
    assert_eq!(record.health, HealthState::Healthy);
}

#[test]
fn an_update_enables_and_starts_the_component_units() {
    let manager = manager();
    let mut state = state_with(&awake(), record("0.2.8", true));
    let packages = FixedPackageStateProbe::new(&[("better-awake", "0.2.9")])
        .with_files("better-awake", awake_files());
    let health = probe_for(&awake_files());
    let units = FakeUserUnitController::new(&[stopped(AWAKE_UNIT)]);
    let executor = SucceedingExecutor::new(Arc::new(Mutex::new(Vec::new())));
    let mut driver = RealDriver::new(
        downloader(),
        &executor,
        &packages,
        &health,
        "3f2504e0-4f89-41d3-9a0c-0305e82c3301",
        SystemProfile::default(),
    )
    .with_user_units(&units);

    let progress = plan_and_run(&manager, &mut state, DesiredOperation::Update, &mut driver);

    assert!(matches!(progress, OperationProgress::Finished { .. }));
    assert_eq!(units.state(AWAKE_UNIT), Some(running(AWAKE_UNIT)));
}

#[test]
fn enabling_a_disabled_component_starts_its_units() {
    let manager = manager();
    let mut state = state_with(&awake(), record("0.2.9", false));
    let packages = FixedPackageStateProbe::new(&[("better-awake", "0.2.9")])
        .with_files("better-awake", awake_files());
    let health = probe_for(&awake_files());
    let units = FakeUserUnitController::new(&[stopped(AWAKE_UNIT)]);
    let executor = SucceedingExecutor::new(Arc::new(Mutex::new(Vec::new())));
    let mut driver = RealDriver::new(
        downloader(),
        &executor,
        &packages,
        &health,
        "3f2504e0-4f89-41d3-9a0c-0305e82c3301",
        SystemProfile::default(),
    )
    .with_user_units(&units);

    let progress = plan_and_run(&manager, &mut state, DesiredOperation::Enable, &mut driver);

    assert!(matches!(progress, OperationProgress::Finished { .. }));
    assert_eq!(units.state(AWAKE_UNIT), Some(running(AWAKE_UNIT)));
    assert!(state.components.get(&awake()).unwrap().enabled);
}

#[test]
fn a_restore_that_leaves_the_component_enabled_starts_its_units() {
    let manager = manager();
    let mut snapshot_record = record("0.2.8", true);
    snapshot_record.restore_snapshot = Some(ComponentSnapshot {
        installed_version: Some("0.2.8".to_string()),
        enabled: true,
        health: HealthState::Healthy,
        artifact: Some(PlanArtifact {
            url: None,
            sha256: "a".repeat(64),
            release_asset: "better-awake_0.2.8_ubuntu-24.04_amd64.deb".to_string(),
            expected_bytes: Some(1024),
            companions: Vec::new(),
        }),
    });
    let mut state = state_with(&awake(), snapshot_record);
    let packages = FixedPackageStateProbe::new(&[("better-awake", "0.2.9")])
        .with_files("better-awake", awake_files());
    let health = probe_for(&awake_files());
    let units = FakeUserUnitController::new(&[stopped(AWAKE_UNIT)]);
    let executor = SucceedingExecutor::new(Arc::new(Mutex::new(Vec::new())));
    let mut driver = RealDriver::new(
        downloader(),
        &executor,
        &packages,
        &health,
        "3f2504e0-4f89-41d3-9a0c-0305e82c3301",
        SystemProfile::default(),
    )
    .with_user_units(&units);

    let progress = plan_and_run(&manager, &mut state, DesiredOperation::Restore, &mut driver);

    assert!(matches!(progress, OperationProgress::Finished { .. }));
    assert_eq!(units.state(AWAKE_UNIT), Some(running(AWAKE_UNIT)));
}

#[test]
fn disabling_a_component_stops_and_disables_its_units() {
    let manager = manager();
    let mut state = state_with(&awake(), record("0.2.9", true));
    let packages = FixedPackageStateProbe::new(&[("better-awake", "0.2.9")])
        .with_files("better-awake", awake_files());
    let health = probe_for(&awake_files());
    let units = FakeUserUnitController::new(&[running(AWAKE_UNIT)]);
    let executor = SucceedingExecutor::new(Arc::new(Mutex::new(Vec::new())));
    let mut driver = RealDriver::new(
        downloader(),
        &executor,
        &packages,
        &health,
        "3f2504e0-4f89-41d3-9a0c-0305e82c3301",
        SystemProfile::default(),
    )
    .with_user_units(&units);

    let progress = plan_and_run(&manager, &mut state, DesiredOperation::Disable, &mut driver);

    assert!(matches!(progress, OperationProgress::Finished { .. }));
    assert_eq!(units.state(AWAKE_UNIT), Some(stopped(AWAKE_UNIT)));
    assert_eq!(
        units.calls(),
        vec![FakeUnitCall::Disable(vec![AWAKE_UNIT.to_string()])]
    );
    let record = state.components.get(&awake()).unwrap();
    assert!(!record.enabled);
    assert_eq!(record.health, HealthState::Healthy);
}

#[test]
fn removing_a_component_stops_its_units_before_the_package_goes() {
    let manager = manager();
    let mut state = state_with(&awake(), record("0.2.9", true));
    let packages = FixedPackageStateProbe::new(&[("better-awake", "0.2.9")])
        .with_files("better-awake", awake_files());
    let health = probe_for(&awake_files());
    let log = Arc::new(Mutex::new(Vec::new()));
    let units = LoggingUnits::new(
        FakeUserUnitController::new(&[running(AWAKE_UNIT)]),
        log.clone(),
    );
    let executor = SucceedingExecutor::new(log.clone());
    let mut driver = RealDriver::new(
        downloader(),
        &executor,
        &packages,
        &health,
        "3f2504e0-4f89-41d3-9a0c-0305e82c3301",
        SystemProfile::default(),
    )
    .with_user_units(&units);

    let progress = plan_and_run(&manager, &mut state, DesiredOperation::Remove, &mut driver);

    assert!(matches!(progress, OperationProgress::Finished { .. }));
    let log = log.lock().unwrap().clone();
    assert_eq!(
        log,
        vec![
            format!("states:{AWAKE_UNIT}"),
            format!("disable:{AWAKE_UNIT}"),
            "apply:better-awake".to_string(),
        ],
        "the units stop before dpkg is asked to remove the package"
    );
    assert_eq!(units.inner.state(AWAKE_UNIT), Some(stopped(AWAKE_UNIT)));
    let record = state.components.get(&awake()).unwrap();
    assert_eq!(record.installed_version, None);
    assert!(!record.enabled);
}

#[test]
fn a_unit_that_fails_to_start_fails_the_step_and_names_it() {
    let manager = manager();
    let mut state = ManagerState::default();
    let packages = FixedPackageStateProbe::new(&[("better-awake", "0.2.9")])
        .with_files("better-awake", awake_files());
    let health = probe_for(&awake_files());
    let units = FakeUserUnitController::new(&[stopped(AWAKE_UNIT)])
        .with_failing_enable(AWAKE_UNIT, "Job for better-awake.service failed");
    let executor = SucceedingExecutor::new(Arc::new(Mutex::new(Vec::new())));
    let mut driver = RealDriver::new(
        downloader(),
        &executor,
        &packages,
        &health,
        "3f2504e0-4f89-41d3-9a0c-0305e82c3301",
        SystemProfile::default(),
    )
    .with_user_units(&units);

    let progress = plan_and_run(&manager, &mut state, DesiredOperation::Install, &mut driver);

    let OperationProgress::Failed { failure } = progress else {
        panic!("a unit that did not start must fail the operation");
    };
    assert_eq!(failure.stage, OperationStage::ApplyingSettings);
    assert_eq!(failure.evidence, "user_units.failed");
    let detail = failure.detail.expect("the failure names the unit");
    assert!(detail.contains(AWAKE_UNIT), "{detail}");
    assert!(
        detail.contains("Job for better-awake.service failed"),
        "{detail}"
    );
    assert_eq!(
        state.components.get(&awake()).map(|record| record.health),
        Some(HealthState::Failed),
        "the component is not marked healthy"
    );
}

#[test]
fn health_reports_a_stopped_unit_by_name() {
    let manager = manager();
    let mut state = state_with(&awake(), record("0.2.9", true));
    let packages = FixedPackageStateProbe::new(&[("better-awake", "0.2.9")])
        .with_files("better-awake", awake_files());
    let health = probe_for(&awake_files());
    // Enabled, not running, and it stays that way although the start was
    // accepted.
    let units = IgnoresStart(FakeUserUnitController::new(&[enabled_but_stopped(
        AWAKE_UNIT,
    )]));
    let executor = SucceedingExecutor::new(Arc::new(Mutex::new(Vec::new())));
    let mut driver = RealDriver::new(
        downloader(),
        &executor,
        &packages,
        &health,
        "3f2504e0-4f89-41d3-9a0c-0305e82c3301",
        SystemProfile::default(),
    )
    .with_user_units(&units);

    let progress = plan_and_run(&manager, &mut state, DesiredOperation::Verify, &mut driver);

    let OperationProgress::Failed { failure } = progress else {
        panic!("a stopped unit is not a healthy component");
    };
    assert_eq!(failure.stage, OperationStage::CheckingHealth);
    assert_eq!(failure.evidence, "user_units.not_running");
    assert!(
        failure
            .detail
            .as_deref()
            .is_some_and(|detail| detail.contains(AWAKE_UNIT)),
        "the failure names the unit: {:?}",
        failure.detail
    );
}

/// A service that exits 0 on purpose — the touchpad gesture service with
/// nothing to do — is an enabled component working as designed. Verify
/// passes and starts nothing, and the startup pass leaves it alone.
#[test]
fn a_service_that_finished_successfully_is_healthy_and_is_not_restarted() {
    let manager = manager();
    let mut state = state_with(&awake(), record("0.2.9", true));
    let packages = FixedPackageStateProbe::new(&[("better-awake", "0.2.9")])
        .with_files("better-awake", awake_files());
    let health = probe_for(&awake_files());
    let units = FakeUserUnitController::new(&[exited(AWAKE_UNIT)]);

    let healed = manager
        .heal_user_units(&mut state, &packages, &units)
        .expect("the startup pass runs");
    assert!(healed.is_empty(), "{healed:?}");

    let executor = SucceedingExecutor::new(Arc::new(Mutex::new(Vec::new())));
    let mut driver = RealDriver::new(
        downloader(),
        &executor,
        &packages,
        &health,
        "3f2504e0-4f89-41d3-9a0c-0305e82c3301",
        SystemProfile::default(),
    )
    .with_user_units(&units);
    let progress = plan_and_run(&manager, &mut state, DesiredOperation::Verify, &mut driver);

    assert!(
        matches!(progress, OperationProgress::Finished { .. }),
        "{progress:?}"
    );
    assert!(
        units
            .calls()
            .iter()
            .all(|call| matches!(call, FakeUnitCall::States(_))),
        "nothing was started: {:?}",
        units.calls()
    );
}

#[test]
fn a_disabled_component_is_not_required_to_have_a_running_unit() {
    let manager = manager();
    let mut state = state_with(&awake(), record("0.2.9", false));
    let packages = FixedPackageStateProbe::new(&[("better-awake", "0.2.9")])
        .with_files("better-awake", awake_files());
    let health = probe_for(&awake_files());
    let units = FakeUserUnitController::new(&[stopped(AWAKE_UNIT)]);
    let executor = SucceedingExecutor::new(Arc::new(Mutex::new(Vec::new())));
    let mut driver = RealDriver::new(
        downloader(),
        &executor,
        &packages,
        &health,
        "3f2504e0-4f89-41d3-9a0c-0305e82c3301",
        SystemProfile::default(),
    )
    .with_user_units(&units);

    let progress = plan_and_run(&manager, &mut state, DesiredOperation::Verify, &mut driver);

    assert!(matches!(progress, OperationProgress::Finished { .. }));
    assert!(units.calls().is_empty(), "{:?}", units.calls());
}

#[test]
fn start_failure_is_a_warning_that_degrades_the_component_and_names_the_unit() {
    let manager = manager();
    let mut state = state_with(&awake(), record("0.2.9", true));
    let packages = FixedPackageStateProbe::new(&[("better-awake", "0.2.9")])
        .with_files("better-awake", awake_files());
    let units = FakeUserUnitController::new(&[stopped(AWAKE_UNIT)])
        .with_failing_enable(AWAKE_UNIT, "Start request repeated too quickly");
    let revision = state.revision;

    let healed = manager
        .heal_user_units(&mut state, &packages, &units)
        .expect("the startup pass runs");

    assert_eq!(healed.len(), 1);
    assert!(!healed[0].started);
    assert_eq!(
        healed[0]
            .failure
            .as_ref()
            .map(|failure| failure.unit.as_str()),
        Some(AWAKE_UNIT)
    );
    assert_eq!(
        state.components.get(&awake()).map(|record| record.health),
        Some(HealthState::Degraded)
    );
    let activity = state.activity.last().expect("the failure is recorded");
    assert_eq!(activity.kind, manager_core::ActivityKind::Warning);
    assert!(
        activity
            .evidence
            .as_deref()
            .is_some_and(|evidence| evidence.starts_with("user_units.failed")),
        "{:?}",
        activity.evidence
    );
    assert!(state.revision > revision);
}

#[test]
fn startup_enables_and_starts_an_enabled_component_and_leaves_a_disabled_one_alone() {
    let manager = manager();
    let mut state = state_with(&awake(), record("0.2.9", true));
    state.components.insert(
        ComponentId::new("better-monitor").unwrap(),
        record("0.2.9", false),
    );
    let packages =
        FixedPackageStateProbe::new(&[("better-awake", "0.2.9"), ("better-monitor", "0.2.9")])
            .with_files("better-awake", awake_files())
            .with_files("better-monitor", monitor_files());
    let units = FakeUserUnitController::new(&[stopped(AWAKE_UNIT), stopped(MONITOR_UNIT)]);
    let revision = state.revision;

    let healed = manager
        .heal_user_units(&mut state, &packages, &units)
        .expect("the startup pass runs");

    assert_eq!(healed.len(), 1, "{healed:?}");
    assert_eq!(healed[0].component, awake());
    assert!(healed[0].started);
    assert_eq!(
        units.calls(),
        vec![
            FakeUnitCall::States(vec![AWAKE_UNIT.to_string()]),
            FakeUnitCall::Enable(vec![AWAKE_UNIT.to_string()]),
        ],
        "the disabled component is never asked about"
    );
    assert_eq!(units.state(AWAKE_UNIT), Some(running(AWAKE_UNIT)));
    assert_eq!(units.state(MONITOR_UNIT), Some(stopped(MONITOR_UNIT)));
    assert!(state.revision > revision);
    let activity = state.activity.last().expect("the start is recorded");
    assert_eq!(activity.kind, manager_core::ActivityKind::Success);
    assert!(
        activity
            .evidence
            .as_deref()
            .is_some_and(|evidence| evidence.starts_with("user_units.started")),
        "{:?}",
        activity.evidence
    );
}

#[test]
fn startup_does_nothing_when_every_unit_is_already_running() {
    let manager = manager();
    let mut state = state_with(&awake(), record("0.2.9", true));
    let packages = FixedPackageStateProbe::new(&[("better-awake", "0.2.9")])
        .with_files("better-awake", awake_files());
    let units = FakeUserUnitController::new(&[running(AWAKE_UNIT)]);
    let revision = state.revision;

    let healed = manager
        .heal_user_units(&mut state, &packages, &units)
        .expect("the startup pass runs");

    assert!(healed.is_empty());
    assert_eq!(
        units.calls(),
        vec![FakeUnitCall::States(vec![AWAKE_UNIT.to_string()])]
    );
    assert_eq!(
        state.revision, revision,
        "nothing happened, nothing changed"
    );
    assert!(state.activity.is_empty());
}

#[test]
fn startup_skips_a_package_the_host_will_not_describe() {
    let manager = manager();
    let mut state = state_with(&awake(), record("0.2.9", true));
    let packages = FixedPackageStateProbe::new(&[("better-awake", "0.2.9")]);
    let units = FakeUserUnitController::new(&[stopped(AWAKE_UNIT)]);
    let revision = state.revision;

    let healed = manager
        .heal_user_units(&mut state, &packages, &units)
        .expect("the startup pass runs");

    assert!(healed.is_empty());
    assert!(units.calls().is_empty());
    assert_eq!(state.revision, revision);
}

#[test]
fn startup_changes_nothing_for_a_component_with_no_units() {
    let manager = manager();
    let mut state = state_with(&awake(), record("0.2.9", true));
    let packages = FixedPackageStateProbe::new(&[("better-awake", "0.2.9")]).with_files(
        "better-awake",
        InstalledFiles {
            paths: ["/usr/bin/better-awake-service"]
                .iter()
                .map(std::path::PathBuf::from)
                .collect(),
            ..InstalledFiles::default()
        },
    );
    let units = FakeUserUnitController::new(&[]);
    let revision = state.revision;

    let healed = manager
        .heal_user_units(&mut state, &packages, &units)
        .expect("the startup pass runs");

    assert!(healed.is_empty());
    assert!(units.calls().is_empty());
    assert_eq!(state.revision, revision);
}

/// A privileged executor that says the daemon refused the plan before it
/// changed anything.
struct FailingExecutor;

impl PrivilegedTransactionExecutor for FailingExecutor {
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
        _progress: &mut dyn FnMut(u32, ExecutionStage),
    ) -> Result<TransactionOutcome, PlatformError> {
        Ok(TransactionOutcome {
            protocol_version: manager_ipc::PROTOCOL_VERSION,
            transaction_id: plan.transaction_id.clone(),
            status: OutcomeStatus::Failed {
                step_index: Some(0),
                error_key: "daemon.error.apt_failed:dpkg returned 100".to_string(),
                recovery: None,
            },
            reports: Vec::new(),
            rollback_records: Vec::new(),
        })
    }
}

/// "Check again" on a component whose unit is enabled and stopped used to
/// fail forever: Verify only looked, and nothing on screen could start the
/// unit. The record says the component runs, so Verify makes that true first.
#[test]
fn verify_starts_an_enabled_components_stopped_unit_before_checking() {
    let manager = manager();
    let mut state = state_with(&awake(), record("0.2.9", true));
    state.components.get_mut(&awake()).unwrap().health = HealthState::Degraded;
    let packages = FixedPackageStateProbe::new(&[("better-awake", "0.2.9")])
        .with_files("better-awake", awake_files());
    let health = probe_for(&awake_files());
    let units = FakeUserUnitController::new(&[enabled_but_stopped(AWAKE_UNIT)]);
    let executor = SucceedingExecutor::new(Arc::new(Mutex::new(Vec::new())));
    let mut driver = RealDriver::new(
        downloader(),
        &executor,
        &packages,
        &health,
        "3f2504e0-4f89-41d3-9a0c-0305e82c3301",
        SystemProfile::default(),
    )
    .with_user_units(&units);

    let progress = plan_and_run(&manager, &mut state, DesiredOperation::Verify, &mut driver);

    assert!(
        matches!(progress, OperationProgress::Finished { .. }),
        "{progress:?}"
    );
    assert_eq!(units.state(AWAKE_UNIT), Some(running(AWAKE_UNIT)));
    assert_eq!(
        state.components.get(&awake()).map(|record| record.health),
        Some(HealthState::Healthy)
    );
}

/// A restore whose snapshot says the component was disabled leaves its units
/// disabled and stopped, the mirror of a restore that leaves it enabled.
#[test]
fn a_restore_that_leaves_the_component_disabled_stops_its_units() {
    let manager = manager();
    let mut snapshot_record = record("0.2.9", true);
    snapshot_record.restore_snapshot = Some(ComponentSnapshot {
        installed_version: Some("0.2.8".to_string()),
        enabled: false,
        health: HealthState::Healthy,
        artifact: Some(PlanArtifact {
            url: None,
            sha256: "a".repeat(64),
            release_asset: "better-awake_0.2.8_ubuntu-24.04_amd64.deb".to_string(),
            expected_bytes: Some(1024),
            companions: Vec::new(),
        }),
    });
    let mut state = state_with(&awake(), snapshot_record);
    let packages = FixedPackageStateProbe::new(&[("better-awake", "0.2.8")])
        .with_files("better-awake", awake_files());
    let health = probe_for(&awake_files());
    let units = FakeUserUnitController::new(&[running(AWAKE_UNIT)]);
    let executor = SucceedingExecutor::new(Arc::new(Mutex::new(Vec::new())));
    let mut driver = RealDriver::new(
        downloader(),
        &executor,
        &packages,
        &health,
        "3f2504e0-4f89-41d3-9a0c-0305e82c3301",
        SystemProfile::default(),
    )
    .with_user_units(&units);

    let progress = plan_and_run(&manager, &mut state, DesiredOperation::Restore, &mut driver);

    assert!(
        matches!(progress, OperationProgress::Finished { .. }),
        "{progress:?}"
    );
    assert_eq!(units.state(AWAKE_UNIT), Some(stopped(AWAKE_UNIT)));
    assert!(!state.components.get(&awake()).unwrap().enabled);
}

/// Removal stops the units before the package goes. When the daemon then
/// fails, the package is still there and the record still says enabled, so
/// the units have to be running again rather than left stopped.
#[test]
fn a_removal_the_daemon_refuses_starts_the_stopped_units_again() {
    let manager = manager();
    let mut state = state_with(&awake(), record("0.2.9", true));
    let packages = FixedPackageStateProbe::new(&[("better-awake", "0.2.9")])
        .with_files("better-awake", awake_files());
    let health = probe_for(&awake_files());
    let units = FakeUserUnitController::new(&[running(AWAKE_UNIT)]);
    let executor = FailingExecutor;
    let mut driver = RealDriver::new(
        downloader(),
        &executor,
        &packages,
        &health,
        "3f2504e0-4f89-41d3-9a0c-0305e82c3301",
        SystemProfile::default(),
    )
    .with_user_units(&units);

    let progress = plan_and_run(&manager, &mut state, DesiredOperation::Remove, &mut driver);

    assert!(
        matches!(progress, OperationProgress::Failed { .. }),
        "{progress:?}"
    );
    assert_eq!(
        units.state(AWAKE_UNIT),
        Some(running(AWAKE_UNIT)),
        "the unit the removal stopped is running again"
    );
    let record = state.components.get(&awake()).unwrap();
    assert!(record.enabled);
    assert_eq!(record.installed_version.as_deref(), Some("0.2.9"));
}

/// A transaction in progress owns the component; a startup pass that started
/// or stopped its units underneath it would race the driver.
#[test]
fn startup_leaves_units_alone_while_a_transaction_is_active() {
    let manager = manager();
    let mut state = state_with(&awake(), record("0.2.9", true));
    let plan = manager
        .plan_in_mode(
            &state,
            &awake(),
            DesiredOperation::Disable,
            ExecutionMode::Real,
        )
        .unwrap();
    manager.begin(&mut state, plan).unwrap();
    let packages = FixedPackageStateProbe::new(&[("better-awake", "0.2.9")])
        .with_files("better-awake", awake_files());
    let units = FakeUserUnitController::new(&[stopped(AWAKE_UNIT)]);
    let revision = state.revision;

    let healed = manager
        .heal_user_units(&mut state, &packages, &units)
        .expect("the startup pass runs");

    assert!(healed.is_empty(), "{healed:?}");
    assert!(units.calls().is_empty(), "{:?}", units.calls());
    assert_eq!(state.revision, revision);
}

fn real_driver<'a>(
    executor: &'a dyn PrivilegedTransactionExecutor,
    packages: &'a FixedPackageStateProbe,
    health: &'a FakeHealthProbe,
    units: &'a dyn UserUnitController,
) -> RealDriver<'a> {
    RealDriver::new(
        downloader(),
        executor,
        packages,
        health,
        "3f2504e0-4f89-41d3-9a0c-0305e82c3301",
        SystemProfile::default(),
    )
    .with_user_units(units)
}

/// An update replaces the program a running unit executes. Enabling a unit
/// that is already running starts nothing, so without a restart the old
/// program keeps running after a "successful" update.
#[test]
fn an_update_restarts_a_unit_that_was_already_running() {
    let manager = manager();
    let mut state = state_with(&awake(), record("0.2.8", true));
    let packages = FixedPackageStateProbe::new(&[("better-awake", "0.2.9")])
        .with_files("better-awake", awake_files());
    let health = probe_for(&awake_files());
    let units = FakeUserUnitController::new(&[running(AWAKE_UNIT)]);
    let executor = SucceedingExecutor::new(Arc::new(Mutex::new(Vec::new())));
    let mut driver = real_driver(&executor, &packages, &health, &units);

    let progress = plan_and_run(&manager, &mut state, DesiredOperation::Update, &mut driver);

    assert!(
        matches!(progress, OperationProgress::Finished { .. }),
        "{progress:?}"
    );
    assert!(
        units.was_restarted(AWAKE_UNIT),
        "the running unit must be restarted: {:?}",
        units.calls()
    );
    assert_eq!(units.state(AWAKE_UNIT), Some(running(AWAKE_UNIT)));
}

#[test]
fn a_restore_restarts_a_unit_that_was_already_running() {
    let manager = manager();
    let mut snapshot_record = record("0.2.9", true);
    snapshot_record.restore_snapshot = Some(ComponentSnapshot {
        installed_version: Some("0.2.8".to_string()),
        enabled: true,
        health: HealthState::Healthy,
        artifact: Some(PlanArtifact {
            url: None,
            sha256: "a".repeat(64),
            release_asset: "better-awake_0.2.8_ubuntu-24.04_amd64.deb".to_string(),
            expected_bytes: Some(1024),
            companions: Vec::new(),
        }),
    });
    let mut state = state_with(&awake(), snapshot_record);
    let packages = FixedPackageStateProbe::new(&[("better-awake", "0.2.8")])
        .with_files("better-awake", awake_files());
    let health = probe_for(&awake_files());
    let units = FakeUserUnitController::new(&[running(AWAKE_UNIT)]);
    let executor = SucceedingExecutor::new(Arc::new(Mutex::new(Vec::new())));
    let mut driver = real_driver(&executor, &packages, &health, &units);

    let progress = plan_and_run(&manager, &mut state, DesiredOperation::Restore, &mut driver);

    assert!(
        matches!(progress, OperationProgress::Finished { .. }),
        "{progress:?}"
    );
    assert!(units.was_restarted(AWAKE_UNIT), "{:?}", units.calls());
}

/// With no user session there is nobody to stop the units, and saying the
/// component is disabled would be a lie: the units keep running.
#[test]
fn disabling_without_a_user_session_fails_and_says_so() {
    let manager = manager();
    let mut state = state_with(&awake(), record("0.2.9", true));
    let packages = FixedPackageStateProbe::new(&[("better-awake", "0.2.9")])
        .with_files("better-awake", awake_files());
    let health = probe_for(&awake_files());
    let units = FakeUserUnitController::new(&[running(AWAKE_UNIT)])
        .with_no_session("Failed to connect to bus: No medium found");
    let executor = SucceedingExecutor::new(Arc::new(Mutex::new(Vec::new())));
    let mut driver = real_driver(&executor, &packages, &health, &units);

    let progress = plan_and_run(&manager, &mut state, DesiredOperation::Disable, &mut driver);

    let OperationProgress::Failed { failure } = progress else {
        panic!("a disable that stopped nothing must not succeed");
    };
    assert_eq!(failure.evidence, "user_units.no_session");
    let detail = failure.detail.unwrap_or_default();
    assert!(detail.contains(AWAKE_UNIT), "{detail}");
    assert!(detail.contains("no user session"), "{detail}");
    assert_eq!(
        state.components.get(&awake()).map(|record| record.health),
        Some(HealthState::Failed),
        "the disable is not reported as done"
    );
}

#[test]
fn removing_without_a_user_session_fails_before_the_package_goes() {
    let manager = manager();
    let mut state = state_with(&awake(), record("0.2.9", true));
    let packages = FixedPackageStateProbe::new(&[("better-awake", "0.2.9")])
        .with_files("better-awake", awake_files());
    let health = probe_for(&awake_files());
    let units = FakeUserUnitController::new(&[running(AWAKE_UNIT)])
        .with_no_session("Failed to connect to bus: No medium found");
    let log = Arc::new(Mutex::new(Vec::new()));
    let executor = SucceedingExecutor::new(log.clone());
    let mut driver = real_driver(&executor, &packages, &health, &units);

    let progress = plan_and_run(&manager, &mut state, DesiredOperation::Remove, &mut driver);

    let OperationProgress::Failed { failure } = progress else {
        panic!("a removal that could not stop the units must not proceed");
    };
    assert_eq!(failure.evidence, "user_units.no_session");
    assert!(
        log.lock().unwrap().is_empty(),
        "the package was never handed to the daemon"
    );
    assert_eq!(
        state
            .components
            .get(&awake())
            .unwrap()
            .installed_version
            .as_deref(),
        Some("0.2.9")
    );
}

/// No user session at startup — a login over SSH, a session bus that is not
/// up yet — says nothing about any unit. The pass skips, the component keeps
/// its health, and there is nothing to save.
#[test]
fn startup_without_a_user_session_changes_nothing() {
    let manager = manager();
    let mut state = state_with(&awake(), record("0.2.9", true));
    let packages = FixedPackageStateProbe::new(&[("better-awake", "0.2.9")])
        .with_files("better-awake", awake_files());
    let units = FakeUserUnitController::new(&[stopped(AWAKE_UNIT)])
        .with_no_session("Failed to connect to bus: No medium found");
    let before = state.clone();

    let healed = manager
        .heal_user_units(&mut state, &packages, &units)
        .expect("the startup pass runs");

    assert!(healed.is_empty(), "{healed:?}");
    assert_eq!(
        state, before,
        "health, activity, and revision are unchanged"
    );
}

/// A component the pass degraded because its unit would not start is healthy
/// again once a later pass starts it, unless something else failed.
#[test]
fn a_successful_start_returns_a_degraded_component_to_healthy() {
    let manager = manager();
    let mut state = state_with(&awake(), record("0.2.9", true));
    state.components.get_mut(&awake()).unwrap().health = HealthState::Degraded;
    let packages = FixedPackageStateProbe::new(&[("better-awake", "0.2.9")])
        .with_files("better-awake", awake_files());
    let units = FakeUserUnitController::new(&[stopped(AWAKE_UNIT)]);

    let healed = manager
        .heal_user_units(&mut state, &packages, &units)
        .expect("the startup pass runs");

    assert_eq!(healed.len(), 1);
    assert!(healed[0].started);
    assert_eq!(
        state.components.get(&awake()).map(|record| record.health),
        Some(HealthState::Healthy)
    );
}

/// A masked unit was blocked on purpose and enabling it fails. It is worth
/// one warning, not one at every start.
#[test]
fn a_masked_unit_is_reported_once_and_not_at_every_start() {
    let manager = manager();
    let mut state = state_with(&awake(), record("0.2.9", true));
    let packages = FixedPackageStateProbe::new(&[("better-awake", "0.2.9")])
        .with_files("better-awake", awake_files());
    let units = FakeUserUnitController::new(&[UnitState::masked(AWAKE_UNIT)]);

    let first = manager
        .heal_user_units(&mut state, &packages, &units)
        .expect("the startup pass runs");
    let after_first = state.clone();
    let second = manager
        .heal_user_units(&mut state, &packages, &units)
        .expect("the startup pass runs again");

    assert_eq!(first.len(), 1);
    assert!(first[0].masked);
    assert_eq!(
        after_first
            .activity
            .iter()
            .filter(|entry| entry.kind == manager_core::ActivityKind::Warning)
            .count(),
        1
    );
    assert!(second.is_empty(), "{second:?}");
    assert_eq!(
        state, after_first,
        "the same masked unit is not recorded again"
    );
    assert!(
        !units
            .calls()
            .iter()
            .any(|call| matches!(call, FakeUnitCall::Enable(_))),
        "a masked unit is never enabled: {:?}",
        units.calls()
    );
}

/// The real driver is what the window and the command line build, so the two
/// must be able to give it a controller without changing `new`.
#[test]
fn the_real_driver_takes_a_controller_through_a_builder() {
    let packages = FixedPackageStateProbe::new(&[]);
    let health = FakeHealthProbe::empty();
    let units = FakeUserUnitController::new(&[]);
    let executor = SucceedingExecutor::new(Arc::new(Mutex::new(Vec::new())));
    let downloader = downloader();
    let driver = RealDriver::new(
        downloader,
        &executor,
        &packages,
        &health,
        "3f2504e0-4f89-41d3-9a0c-0305e82c3301",
        SystemProfile::default(),
    )
    .with_user_units(&units);

    assert!(driver.supports_cancel_at(OperationStage::Downloading));
    // A runner can be built around it, which is the shape the window uses.
    let manager = manager();
    let sink = DiscardingSink;
    let _runner = TransactionRunner::new(&manager, Box::new(driver), &sink);
}
