//! Turning Better Files off gives back the defaults it took over, and
//! turning it on takes them again (ticket 129, ADR 0041): manager-core runs
//! the package's `/usr/lib/better-os/on-disable/better-files`, found in
//! dpkg's file list, and its `--undo`.
//!
//! A real [`RealDriver`] runs every stage after downloading, with a fake
//! dpkg, a fake health probe, a fake privileged executor and a recording
//! hook runner: nothing of the person's session is read or written.

use better_core::package_health::{FakeHealthProbe, InstalledFiles};
use better_core::{ComponentCatalog, ComponentId, ComponentManifest};
use manager_core::exec::{CancelToken, RealDriver, StageDriver};
use manager_core::{
    ComponentRecord, DesiredOperation, ExecutionMode, HealthState, InstallProvenance, Manager,
    ManagerState, OperationProgress, StageOutcome, SystemProfile,
};
use manager_ipc::{ExecutionStage, OutcomeStatus, TransactionOutcome, WirePlan};
use manager_platform::download::{ArtifactCache, HttpDownloader};
use manager_platform::dpkg::FixedPackageStateProbe;
use manager_platform::removal_hooks::{FakeHookRunner, HookOutcome};
use manager_platform::{PlatformError, PrivilegedTransactionExecutor};

const HOOK: &str = "/usr/lib/better-os/on-disable/better-files";

fn manager() -> Manager {
    let manifest = ComponentManifest::parse_yaml(include_str!(
        "../../../components/manifests/better-files.yaml"
    ))
    .unwrap();
    Manager::new(
        ComponentCatalog::from_manifests(vec![manifest]).unwrap(),
        SystemProfile::default(),
    )
}

fn files_component() -> ComponentId {
    ComponentId::new("better-files").unwrap()
}

fn files(with_hook: bool) -> InstalledFiles {
    let mut paths = vec!["/usr/bin/better-files", "/usr/bin/better-files-preview"];
    if with_hook {
        paths.push(HOOK);
    }
    InstalledFiles {
        paths: paths.iter().map(std::path::PathBuf::from).collect(),
        ..InstalledFiles::default()
    }
}

fn installed(enabled: bool) -> ManagerState {
    let mut state = ManagerState::default();
    state.components.insert(
        files_component(),
        ComponentRecord {
            installed_version: Some("0.2.16".to_string()),
            enabled,
            health: HealthState::Healthy,
            installed_artifact: None,
            provenance: InstallProvenance::Manager,
            restore_snapshot: None,
            failure: None,
            recovery: None,
            drift: None,
        },
    );
    state
}

struct Executor;

impl PrivilegedTransactionExecutor for Executor {
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
            status: OutcomeStatus::Succeeded,
            reports: Vec::new(),
            rollback_records: Vec::new(),
        })
    }
}

fn run(
    state: &mut ManagerState,
    operation: DesiredOperation,
    with_hook: bool,
    hooks: &FakeHookRunner,
) -> OperationProgress {
    let manager = manager();
    let packages = FixedPackageStateProbe::new(&[("better-files", "0.2.16")])
        .with_files("better-files", files(with_hook));
    let health = FakeHealthProbe::with_files(files(with_hook).paths);
    let downloader: &'static HttpDownloader = Box::leak(Box::new(HttpDownloader::new(
        ArtifactCache::new(std::env::temp_dir().join("better-os-on-disable-test-cache")),
    )));
    let mut driver = RealDriver::new(
        downloader,
        &Executor,
        &packages,
        &health,
        "3f2504e0-4f89-41d3-9a0c-0305e82c3301",
        SystemProfile::default(),
    )
    .with_removal_hooks(hooks);
    let plan = manager
        .plan_in_mode(state, &files_component(), operation, ExecutionMode::Real)
        .expect("the operation plans");
    manager.begin(state, plan).expect("the plan begins");
    manager
        .advance(state, StageOutcome::Completed)
        .expect("downloading completes");
    loop {
        let (stage, plan) = {
            let active = state.active_operation.as_ref().expect("still active");
            (active.stage, active.plan.clone())
        };
        let outcome = driver.run_stage(stage, &plan, &mut |_| {}, &CancelToken::new());
        match manager.advance(state, outcome).expect("the stage advances") {
            OperationProgress::InProgress { .. } => continue,
            finished => return finished,
        }
    }
}

#[test]
fn turning_better_files_off_gives_its_defaults_back_and_on_takes_them_again() {
    let hooks = FakeHookRunner::default();
    let mut state = installed(true);
    let off = run(&mut state, DesiredOperation::Disable, true, &hooks);
    assert!(matches!(off, OperationProgress::Finished { .. }), "{off:?}");
    assert_eq!(hooks.ran(), [(std::path::PathBuf::from(HOOK), false)]);

    let on = run(&mut state, DesiredOperation::Enable, true, &hooks);
    assert!(matches!(on, OperationProgress::Finished { .. }), "{on:?}");
    assert_eq!(
        hooks.ran(),
        [
            (std::path::PathBuf::from(HOOK), false),
            (std::path::PathBuf::from(HOOK), true)
        ]
    );
}

#[test]
fn a_package_without_the_program_runs_nothing_and_a_failure_never_stops_turning_off() {
    let hooks = FakeHookRunner::default();
    let mut state = installed(true);
    run(&mut state, DesiredOperation::Disable, false, &hooks);
    assert!(hooks.ran().is_empty());

    let refusing = FakeHookRunner::answering(HookOutcome::NotRestored("no defaults store".into()));
    let mut state = installed(true);
    let off = run(&mut state, DesiredOperation::Disable, true, &refusing);
    assert!(matches!(off, OperationProgress::Finished { .. }), "{off:?}");
    assert!(!state.components.get(&files_component()).unwrap().enabled);
}
