//! Installing Better Touchpad turns its GNOME Shell extension on, and
//! removing it puts its settings back and turns the extension off (ticket 88).
//!
//! A real [`RealDriver`] runs every stage after downloading, with a fake dpkg,
//! a fake health probe, a fake privileged executor, and the real extension
//! controller over in-memory settings, so the person's own GNOME Shell
//! settings are never read or written.

use std::sync::{Arc, Mutex};

use better_core::package_health::{FakeHealthProbe, InstalledFiles};
use better_core::{ComponentCatalog, ComponentId, ComponentManifest};
use manager_core::exec::{CancelToken, DiscardingSink, RealDriver, StageDriver, TransactionRunner};
use manager_core::{
    ActivityKind, ComponentRecord, DesiredOperation, ExecutionMode, HealthState, InstallProvenance,
    Manager, ManagerState, OperationProgress, OperationStage, StageOutcome, SystemProfile,
};
use manager_ipc::{ExecutionStage, OutcomeStatus, TransactionOutcome, WirePlan};
use manager_platform::download::{ArtifactCache, HttpDownloader};
use manager_platform::dpkg::FixedPackageStateProbe;
use manager_platform::removal_hooks::{FakeHookRunner, HookOutcome};
use manager_platform::shell_extensions::{
    ExtensionFailure, ExtensionFailureKind, FakeShellExtensions, MemoryShellSettings,
    ShellExtensions,
};
use manager_platform::{PlatformError, PrivilegedTransactionExecutor};

const OURS: &str = "touchpad-adapter@betteros.org";
const HOOK: &str = "/usr/lib/better-os/before-removal/better-touchpad";
const THEIRS: &str = "x11gestures@joseexposito.github.io";

fn manager() -> Manager {
    let manifest = ComponentManifest::parse_yaml(include_str!(
        "../../../components/manifests/better-touchpad.yaml"
    ))
    .unwrap();
    Manager::new(
        ComponentCatalog::from_manifests(vec![manifest]).unwrap(),
        SystemProfile::default(),
    )
}

fn touchpad() -> ComponentId {
    ComponentId::new("better-touchpad").unwrap()
}

fn files() -> InstalledFiles {
    InstalledFiles {
        paths: [
            "/usr/bin/better-touchpad",
            "/usr/share/gnome-shell/extensions/touchpad-adapter@betteros.org",
            "/usr/share/gnome-shell/extensions/touchpad-adapter@betteros.org/metadata.json",
            "/usr/share/gnome-shell/extensions/touchpad-adapter@betteros.org/extension.js",
            HOOK,
        ]
        .iter()
        .map(std::path::PathBuf::from)
        .collect(),
        ..InstalledFiles::default()
    }
}

fn packages() -> FixedPackageStateProbe {
    FixedPackageStateProbe::new(&[("better-touchpad", "0.2.16")])
        .with_files("better-touchpad", files())
}

fn record(enabled: bool) -> ComponentRecord {
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
    }
}

fn installed(enabled: bool) -> ManagerState {
    let mut state = ManagerState::default();
    state.components.insert(touchpad(), record(enabled));
    state
}

/// Records the handover in a shared log, and refuses it when asked to.
struct Executor {
    log: Arc<Mutex<Vec<String>>>,
    refuse: bool,
}

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
        self.log.lock().unwrap().push("apply".to_string());
        Ok(TransactionOutcome {
            protocol_version: manager_ipc::PROTOCOL_VERSION,
            transaction_id: plan.transaction_id.clone(),
            status: if self.refuse {
                OutcomeStatus::Failed {
                    step_index: None,
                    error_key: "daemon.error.apt_failed:better-touchpad".to_string(),
                    recovery: None,
                }
            } else {
                OutcomeStatus::Succeeded
            },
            reports: Vec::new(),
            rollback_records: Vec::new(),
        })
    }
}

fn downloader() -> &'static HttpDownloader {
    Box::leak(Box::new(HttpDownloader::new(ArtifactCache::new(
        std::env::temp_dir().join("better-os-shell-extension-test-cache"),
    ))))
}

fn run(
    state: &mut ManagerState,
    operation: DesiredOperation,
    extensions: &FakeShellExtensions,
    refuse: bool,
) -> OperationProgress {
    run_with_hooks(
        state,
        operation,
        extensions,
        refuse,
        &FakeHookRunner::default(),
    )
    .0
}

fn run_with_hooks(
    state: &mut ManagerState,
    operation: DesiredOperation,
    extensions: &FakeShellExtensions,
    refuse: bool,
    hooks: &FakeHookRunner,
) -> (OperationProgress, Vec<String>) {
    let log = Arc::new(Mutex::new(Vec::new()));
    let manager = manager();
    let packages = packages();
    let health = FakeHealthProbe::with_files(files().paths);
    let executor = Executor {
        log: log.clone(),
        refuse,
    };
    let mut driver = RealDriver::new(
        downloader(),
        &executor,
        &packages,
        &health,
        "3f2504e0-4f89-41d3-9a0c-0305e82c3301",
        SystemProfile::default(),
    )
    .with_shell_extensions(extensions)
    .with_removal_hooks(hooks);
    let plan = manager
        .plan_in_mode(state, &touchpad(), operation, ExecutionMode::Real)
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
            finished => return (finished, log.lock().unwrap().clone()),
        }
    }
}

fn controller(enabled: &[&str], disabled: &[&str]) -> FakeShellExtensions {
    ShellExtensions::new(MemoryShellSettings::new(enabled, disabled))
}

#[test]
fn installing_turns_the_extension_on_and_keeps_every_other_one() {
    let extensions = controller(&[THEIRS], &[OURS]);
    let mut state = ManagerState::default();

    let progress = run(&mut state, DesiredOperation::Install, &extensions, false);

    assert!(
        matches!(progress, OperationProgress::Finished { .. }),
        "{progress:?}"
    );
    assert_eq!(extensions.settings().enabled_list(), [THEIRS, OURS]);
    assert!(extensions.settings().disabled_list().is_empty());
}

#[test]
fn enabling_turns_it_on_and_disabling_turns_it_off() {
    let extensions = controller(&[THEIRS], &[]);
    let mut state = installed(false);
    let progress = run(&mut state, DesiredOperation::Enable, &extensions, false);
    assert!(
        matches!(progress, OperationProgress::Finished { .. }),
        "{progress:?}"
    );
    assert_eq!(extensions.settings().enabled_list(), [THEIRS, OURS]);

    let progress = run(&mut state, DesiredOperation::Disable, &extensions, false);
    assert!(
        matches!(progress, OperationProgress::Finished { .. }),
        "{progress:?}"
    );
    assert_eq!(extensions.settings().enabled_list(), [THEIRS]);
}

#[test]
fn removing_turns_it_off_and_leaves_the_others() {
    let extensions = controller(&[THEIRS, OURS], &[]);
    let mut state = installed(true);

    let progress = run(&mut state, DesiredOperation::Remove, &extensions, false);

    assert!(
        matches!(progress, OperationProgress::Finished { .. }),
        "{progress:?}"
    );
    assert_eq!(extensions.settings().enabled_list(), [THEIRS]);
}

#[test]
fn a_removal_the_service_refuses_turns_it_back_on() {
    let extensions = controller(&[OURS], &[]);
    let mut state = installed(true);

    let progress = run(&mut state, DesiredOperation::Remove, &extensions, true);

    assert!(
        matches!(progress, OperationProgress::Failed { .. }),
        "{progress:?}"
    );
    assert_eq!(extensions.settings().enabled_list(), [OURS]);
}

#[test]
fn verify_on_an_enabled_component_turns_a_switched_off_extension_back_on() {
    let extensions = controller(&[], &[]);
    let mut state = installed(true);

    let progress = run(&mut state, DesiredOperation::Verify, &extensions, false);

    assert!(
        matches!(progress, OperationProgress::Finished { .. }),
        "{progress:?}"
    );
    assert_eq!(extensions.settings().enabled_list(), [OURS]);
}

#[test]
fn a_write_the_settings_dropped_fails_the_install_and_says_so() {
    let extensions = ShellExtensions::new(MemoryShellSettings::new(&[], &[]).ignoring_writes());
    let mut state = ManagerState::default();

    let progress = run(&mut state, DesiredOperation::Install, &extensions, false);

    let OperationProgress::Failed { failure } = progress else {
        panic!("a dropped write must not pass: {progress:?}");
    };
    assert_eq!(failure.stage, OperationStage::ApplyingSettings);
    assert_eq!(failure.evidence, "shell_extension.failed");
    assert!(failure.detail.unwrap().contains(OURS));
}

#[test]
fn a_session_without_gnome_shell_installs_and_turns_nothing_on() {
    let extensions = ShellExtensions::new(MemoryShellSettings::new(&[], &[]).failing(
        ExtensionFailure::new(ExtensionFailureKind::NoShell, "No such schema"),
    ));
    let mut state = ManagerState::default();

    let progress = run(&mut state, DesiredOperation::Install, &extensions, false);

    assert!(
        matches!(progress, OperationProgress::Finished { .. }),
        "{progress:?}"
    );
}

#[test]
fn the_startup_pass_turns_on_only_what_an_enabled_record_owns() {
    let manager = manager();
    let packages = packages();

    let extensions = controller(&[THEIRS], &[]);
    let report = manager
        .heal_shell_extensions(&installed(true), &packages, &extensions)
        .unwrap();
    assert_eq!(report.len(), 1);
    assert_eq!(report[0].turned_on, [OURS]);
    assert_eq!(extensions.settings().enabled_list(), [THEIRS, OURS]);

    // Already on: nothing to report.
    assert!(
        manager
            .heal_shell_extensions(&installed(true), &packages, &extensions)
            .unwrap()
            .is_empty()
    );

    // A disabled component is the person's decision and stays off.
    let extensions = controller(&[], &[]);
    assert!(
        manager
            .heal_shell_extensions(&installed(false), &packages, &extensions)
            .unwrap()
            .is_empty()
    );
    assert!(extensions.settings().enabled_list().is_empty());
}

/// Runs a removal through the real runner, which is what records warnings.
fn remove_through_runner(
    state: &mut ManagerState,
    extensions: &FakeShellExtensions,
    refuse: bool,
    hooks: &FakeHookRunner,
) -> OperationProgress {
    through_runner(state, DesiredOperation::Remove, extensions, refuse, hooks)
}

/// Runs an operation through the real runner, which is what records warnings.
fn through_runner(
    state: &mut ManagerState,
    operation: DesiredOperation,
    extensions: &FakeShellExtensions,
    refuse: bool,
    hooks: &FakeHookRunner,
) -> OperationProgress {
    let manager = manager();
    let packages = packages();
    let health = FakeHealthProbe::with_files(files().paths);
    let executor = Executor {
        log: Arc::new(Mutex::new(Vec::new())),
        refuse,
    };
    let driver = RealDriver::new(
        downloader(),
        &executor,
        &packages,
        &health,
        "3f2504e0-4f89-41d3-9a0c-0305e82c3301",
        SystemProfile::default(),
    )
    .with_shell_extensions(extensions)
    .with_removal_hooks(hooks);
    let plan = manager
        .plan_in_mode(state, &touchpad(), operation, ExecutionMode::Real)
        .expect("the operation plans");
    manager.begin(state, plan).expect("the plan begins");
    manager
        .advance(state, StageOutcome::Completed)
        .expect("downloading completes");
    TransactionRunner::new(&manager, Box::new(driver), &DiscardingSink)
        .run(state, &mut |_| {})
        .expect("the runner runs")
}

fn warnings(state: &ManagerState) -> Vec<String> {
    state
        .activity
        .iter()
        .filter(|entry| entry.kind == ActivityKind::Warning)
        .filter_map(|entry| entry.evidence.clone())
        .collect()
}

#[test]
fn removing_runs_the_before_removal_program_once_and_warns_about_nothing() {
    let extensions = controller(&[OURS], &[]);
    let hooks = FakeHookRunner::default();
    let mut state = installed(true);

    let progress = remove_through_runner(&mut state, &extensions, false, &hooks);

    assert!(
        matches!(progress, OperationProgress::Finished { .. }),
        "{progress:?}"
    );
    assert_eq!(hooks.ran(), [(std::path::PathBuf::from(HOOK), false)]);
    assert!(warnings(&state).is_empty(), "{:?}", warnings(&state));
}

#[test]
fn settings_that_could_not_be_restored_never_stop_the_removal() {
    let extensions = controller(&[OURS], &[]);
    let hooks = FakeHookRunner::answering(HookOutcome::NotRestored(
        "the touchpad capture could not be read".to_string(),
    ));
    let mut state = installed(true);

    let progress = remove_through_runner(&mut state, &extensions, false, &hooks);

    assert!(
        matches!(progress, OperationProgress::Finished { .. }),
        "{progress:?}"
    );
    assert!(
        state
            .components
            .get(&touchpad())
            .unwrap()
            .installed_version
            .is_none()
    );
    let said = warnings(&state);
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(
        said[0].starts_with("before_removal.not_restored"),
        "{said:?}"
    );
    assert!(
        said[0].contains("settings could not be restored"),
        "{said:?}"
    );
}

#[test]
fn with_no_session_the_restore_is_deferred_and_the_removal_finishes() {
    let extensions = controller(&[OURS], &[]);
    let hooks = FakeHookRunner::answering(HookOutcome::Deferred);
    let mut state = installed(true);

    let progress = remove_through_runner(&mut state, &extensions, false, &hooks);

    assert!(
        matches!(progress, OperationProgress::Finished { .. }),
        "{progress:?}"
    );
    assert!(warnings(&state)[0].starts_with("before_removal.deferred"));
}

#[test]
fn a_removal_the_service_refuses_undoes_the_before_removal_program() {
    let extensions = controller(&[OURS], &[]);
    let hooks = FakeHookRunner::default();
    let mut state = installed(true);

    let progress = remove_through_runner(&mut state, &extensions, true, &hooks);

    assert!(
        matches!(progress, OperationProgress::Failed { .. }),
        "{progress:?}"
    );
    assert_eq!(
        hooks.ran(),
        [
            (std::path::PathBuf::from(HOOK), false),
            (std::path::PathBuf::from(HOOK), true)
        ],
        "the settings it put back are applied again"
    );
    assert_eq!(extensions.settings().enabled_list(), [OURS]);
    assert!(
        state
            .components
            .get(&touchpad())
            .unwrap()
            .installed_version
            .is_some()
    );
    assert!(warnings(&state).is_empty());
}

#[test]
fn no_other_operation_runs_the_before_removal_program() {
    let extensions = controller(&[], &[]);
    let hooks = FakeHookRunner::default();
    for operation in [DesiredOperation::Disable, DesiredOperation::Verify] {
        let mut state = installed(true);
        let _ = run_with_hooks(&mut state, operation, &extensions, false, &hooks);
    }
    assert!(hooks.ran().is_empty());
}

/// GNOME's switch that turns off every extension is the person's decision
/// about their whole desktop. It is said as a warning and never fails the
/// check, because a failed check rolls an install or an update back (ticket
/// 131). The extension is still put in the list, so it runs once the switch
/// is off.
#[test]
fn gnome_s_all_extensions_off_switch_warns_and_never_fails_the_check() {
    for operation in [DesiredOperation::Install, DesiredOperation::Verify] {
        let extensions = ShellExtensions::new(MemoryShellSettings::new(&[], &[]).all_off());
        let mut state = if operation == DesiredOperation::Install {
            ManagerState::default()
        } else {
            installed(true)
        };

        let progress = through_runner(
            &mut state,
            operation,
            &extensions,
            false,
            &FakeHookRunner::default(),
        );

        assert!(
            matches!(progress, OperationProgress::Finished { .. }),
            "{operation:?}: {progress:?}"
        );
        assert_eq!(
            extensions.settings().enabled_list(),
            [OURS],
            "{operation:?}"
        );
        assert!(
            warnings(&state).iter().any(|warning| warning
                .starts_with("shell_extension.all_turned_off")
                && warning.contains(OURS)),
            "{operation:?}: {:?}",
            warnings(&state)
        );
    }
}

/// A dpkg that cannot be read.
struct UnreadableDpkg;

impl manager_platform::dpkg::PackageStateProbe for UnreadableDpkg {
    fn installed_version(&self, _package: &str) -> Result<Option<String>, PlatformError> {
        Err(PlatformError::CapabilityUnavailable("dpkg-query"))
    }

    fn installed_files(&self, _package: &str) -> Result<InstalledFiles, PlatformError> {
        Err(PlatformError::CapabilityUnavailable("dpkg-query"))
    }
}

/// A reinstall through apt, outside Better Manager, counts as installed
/// again, so the pending restore cannot overwrite it; an unreadable dpkg is
/// "unknown", never "not installed".
#[test]
fn installed_again_reads_the_record_and_dpkg() {
    let empty = FixedPackageStateProbe::new(&[]);
    let apt = packages();

    assert_eq!(
        installed(true).installed_by_record_or_dpkg("better-touchpad", &empty),
        Some(true)
    );
    assert_eq!(
        ManagerState::default().installed_by_record_or_dpkg("better-touchpad", &apt),
        Some(true)
    );
    assert_eq!(
        ManagerState::default().installed_by_record_or_dpkg("better-touchpad", &empty),
        Some(false)
    );
    assert_eq!(
        ManagerState::default().installed_by_record_or_dpkg("better-touchpad", &UnreadableDpkg),
        None
    );
    assert_eq!(
        installed(true).installed_by_record_or_dpkg("better-touchpad", &UnreadableDpkg),
        Some(true)
    );
}
