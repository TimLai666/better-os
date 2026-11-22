//! A companion package's GNOME Shell extension is turned on with the component
//! it travels with (ticket 131).
//!
//! `better-shell-adapter` ships `shell-adapter@betteros.org` and nothing else,
//! and it is a companion of `better-manager` (ADR 0018). Reading only the
//! component's own file list left that extension installed and never enabled
//! on the owner's machine. The person's GNOME Shell settings are never read or
//! written here: the controller runs over in-memory settings.

use better_core::package_health::{FakeHealthProbe, InstalledFiles};
use better_core::{ComponentCatalog, ComponentId, ComponentManifest};
use manager_core::exec::{CancelToken, RealDriver, StageDriver};
use manager_core::{
    ComponentRecord, DesiredOperation, ExecutionMode, HealthState, InstallProvenance, Manager,
    ManagerState, OperationProgress, StageOutcome, SystemProfile,
};
use manager_ipc::{ExecutionStage, TransactionOutcome, WirePlan};
use manager_platform::download::{ArtifactCache, HttpDownloader};
use manager_platform::dpkg::FixedPackageStateProbe;
use manager_platform::shell_extensions::{
    FakeShellExtensions, MemoryShellSettings, ShellExtensions,
};
use manager_platform::{PlatformError, PrivilegedTransactionExecutor};

const SHARED: &str = "shell-adapter@betteros.org";
const THEIRS: &str = "user-theme@gnome-shell-extensions.gcampax.github.com";

fn manager() -> Manager {
    let manifest = ComponentManifest::parse_yaml(include_str!(
        "../../../components/manifests/better-manager.yaml"
    ))
    .unwrap();
    Manager::new(
        ComponentCatalog::from_manifests(vec![manifest]).unwrap(),
        SystemProfile::default(),
    )
}

fn better_manager() -> ComponentId {
    ComponentId::new("better-manager").unwrap()
}

fn files(paths: &[&str]) -> InstalledFiles {
    InstalledFiles {
        paths: paths.iter().map(std::path::PathBuf::from).collect(),
        ..InstalledFiles::default()
    }
}

fn own_files() -> InstalledFiles {
    files(&["/usr/bin/better-manager", "/usr/bin/better-manager-cli"])
}

fn adapter_files() -> InstalledFiles {
    files(&[
        "/usr/share/gnome-shell/extensions/shell-adapter@betteros.org",
        "/usr/share/gnome-shell/extensions/shell-adapter@betteros.org/extension.js",
        "/usr/share/gnome-shell/extensions/shell-adapter@betteros.org/metadata.json",
    ])
}

/// What `dpkg-query` reported on the owner's machine: all three packages at
/// one version (0.2.16 then; the workspace's own now), the extension in the companion's list and not in the manager's.
fn packages(with_adapter: bool) -> FixedPackageStateProbe {
    let mut installed = vec![
        ("better-manager", env!("CARGO_PKG_VERSION")),
        ("better-manager-daemon", env!("CARGO_PKG_VERSION")),
    ];
    if with_adapter {
        installed.push(("better-shell-adapter", env!("CARGO_PKG_VERSION")));
    }
    let probe = FixedPackageStateProbe::new(&installed)
        .with_files("better-manager", own_files())
        .with_files(
            "better-manager-daemon",
            files(&["/usr/libexec/better-os/daemon"]),
        );
    if with_adapter {
        probe.with_files("better-shell-adapter", adapter_files())
    } else {
        probe
    }
}

fn installed(enabled: bool) -> ManagerState {
    let mut state = ManagerState::default();
    state.components.insert(
        better_manager(),
        ComponentRecord {
            installed_version: Some(env!("CARGO_PKG_VERSION").to_string()),
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

fn controller(enabled: &[&str]) -> FakeShellExtensions {
    ShellExtensions::new(MemoryShellSettings::new(enabled, &[]))
}

#[test]
fn the_startup_pass_turns_on_the_extension_a_companion_installed() {
    let extensions = controller(&[THEIRS]);
    let report = manager()
        .heal_shell_extensions(&installed(true), &packages(true), &extensions)
        .unwrap();
    assert_eq!(report.len(), 1, "{report:?}");
    assert_eq!(report[0].component, better_manager());
    assert_eq!(report[0].turned_on, [SHARED]);
    assert_eq!(extensions.settings().enabled_list(), [THEIRS, SHARED]);
}

#[test]
fn a_companion_that_is_not_installed_turns_nothing_on() {
    let extensions = controller(&[]);
    let report = manager()
        .heal_shell_extensions(&installed(true), &packages(false), &extensions)
        .unwrap();
    assert!(report.is_empty(), "{report:?}");
    assert!(extensions.settings().enabled_list().is_empty());
}

#[test]
fn a_disabled_component_leaves_its_companions_extension_off() {
    let extensions = controller(&[]);
    let report = manager()
        .heal_shell_extensions(&installed(false), &packages(true), &extensions)
        .unwrap();
    assert!(report.is_empty());
    assert!(extensions.settings().enabled_list().is_empty());
}

/// Nothing reaches the privileged service in a Verify; refuse loudly if it
/// does.
struct NoExecutor;

impl PrivilegedTransactionExecutor for NoExecutor {
    fn stage_artifact(
        &self,
        _transaction_id: &str,
        _filename: &str,
        _sha256: &str,
        _artifact_path: &std::path::Path,
    ) -> Result<(), PlatformError> {
        panic!("a verify stages nothing");
    }

    fn execute_plan(
        &self,
        _plan: &WirePlan,
        _progress: &mut dyn FnMut(u32, ExecutionStage),
    ) -> Result<TransactionOutcome, PlatformError> {
        panic!("a verify sends no plan");
    }
}

#[test]
fn verifying_the_manager_turns_its_companions_extension_on() {
    verify_manager(controller(&[]));
}

/// Ticket 131: with GNOME's switch that turns off all extensions on, the
/// manager's own check still passes. The companion's extension is put in the
/// list for when the switch goes off, and a failed check here would have
/// rolled Better Manager's own update back.
#[test]
fn the_all_extensions_off_switch_never_fails_the_managers_own_check() {
    verify_manager(ShellExtensions::new(
        MemoryShellSettings::new(&[], &[]).all_off(),
    ));
}

fn verify_manager(extensions: FakeShellExtensions) {
    let manager = manager();
    let packages = packages(true);
    let mut health_paths = own_files().paths;
    health_paths.extend(adapter_files().paths);
    health_paths.push("/usr/libexec/better-os/daemon".into());
    let health = FakeHealthProbe::with_files(health_paths);
    let downloader = HttpDownloader::new(ArtifactCache::new(
        std::env::temp_dir().join("better-os-companion-extension-test-cache"),
    ));
    let mut driver = RealDriver::new(
        &downloader,
        &NoExecutor,
        &packages,
        &health,
        "3f2504e0-4f89-41d3-9a0c-0305e82c3302",
        SystemProfile::default(),
    )
    .with_shell_extensions(&extensions);

    let mut state = installed(true);
    let plan = manager
        .plan_in_mode(
            &state,
            &better_manager(),
            DesiredOperation::Verify,
            ExecutionMode::Real,
        )
        .expect("the verify plans");
    manager.begin(&mut state, plan).expect("the plan begins");
    manager
        .advance(&mut state, StageOutcome::Completed)
        .expect("downloading completes");
    let progress = loop {
        let (stage, plan) = {
            let active = state.active_operation.as_ref().expect("still active");
            (active.stage, active.plan.clone())
        };
        let outcome = driver.run_stage(stage, &plan, &mut |_| {}, &CancelToken::new());
        match manager
            .advance(&mut state, outcome)
            .expect("the stage advances")
        {
            OperationProgress::InProgress { .. } => continue,
            finished => break finished,
        }
    };
    assert!(
        matches!(progress, OperationProgress::Finished { .. }),
        "{progress:?}"
    );
    assert_eq!(extensions.settings().enabled_list(), [SHARED]);
}
