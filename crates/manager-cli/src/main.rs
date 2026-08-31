mod defaults;

use better_core::{ComponentCatalog, ComponentId, ComponentManifest};
use clap::{Parser, Subcommand, ValueEnum};
use manager_core::exec::{
    MockDriver, MockRestoreOutcome, RealDriver, RunnerEvent, StageDriver, StageProgress,
    TransactionRunner,
};
use manager_core::{
    DesiredOperation, DiskSpaceCheck, ExecutionMode, Manager, ManagerState, MockOutcome,
    OperationProgress, OperationStage, TransactionPlan,
};
use manager_platform::MockPlatform;
use manager_platform::download::{ArtifactCache, HttpDownloader};
use manager_platform::dpkg::DpkgProbe;
use manager_platform::privileged::DbusPrivilegedExecutor;
use manager_store::{JsonStore, StateStore};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    name = "better-manager",
    about = "Inspect, plan, and apply Better OS component lifecycle changes"
)]
struct Cli {
    /// Use a disposable JSON state file instead of the local default.
    #[arg(long, global = true)]
    state_path: Option<PathBuf>,
    /// Whether lifecycle commands simulate or actually change this machine.
    ///
    /// Real is the default: a manager that quietly simulated would report a
    /// change that never happened. Without the privileged service the command
    /// says so and stops.
    #[arg(long, global = true, value_enum, env = "BETTER_MANAGER_EXECUTION", default_value_t = ExecutionArg::Real)]
    execution: ExecutionArg,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    List,
    /// Compare recorded component versions against what dpkg reports.
    Reconcile,
    Validate,
    Status {
        id: Option<String>,
    },
    Plan {
        id: String,
        #[arg(value_enum, default_value_t = OperationArg::Install)]
        operation: OperationArg,
    },
    UpdateAll,
    Run {
        id: String,
        #[arg(value_enum, default_value_t = OperationArg::Install)]
        operation: OperationArg,
        /// Deterministically fail the mock operation at this stage.
        #[arg(long, value_enum)]
        fail_at: Option<StageArg>,
    },
    Continue {
        #[arg(long, value_enum)]
        fail_at: Option<StageArg>,
        /// At the final health check, report a partial or manual restore result.
        #[arg(long, value_enum)]
        restore_outcome: Option<RestoreOutcomeArg>,
    },
    Restore {
        id: String,
        #[arg(long, value_enum)]
        fail_at: Option<StageArg>,
        /// At the final health check, report a partial or manual restore result.
        #[arg(long, value_enum)]
        restore_outcome: Option<RestoreOutcomeArg>,
    },
    Cancel,
    Doctor,
    Activity {
        #[arg(long)]
        clear: bool,
    },
    /// Inspect and change which component owns each declared system default.
    Defaults {
        #[command(flatten)]
        options: defaults::DefaultsOptions,
        #[command(subcommand)]
        command: defaults::DefaultsCommand,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
enum ExecutionArg {
    /// Walk the lifecycle deterministically without touching the machine.
    Mock,
    /// Download, verify, and apply through the privileged service.
    Real,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum OperationArg {
    Install,
    Update,
    Enable,
    Disable,
    Verify,
    Restore,
    Remove,
}

impl From<OperationArg> for DesiredOperation {
    fn from(value: OperationArg) -> Self {
        match value {
            OperationArg::Install => Self::Install,
            OperationArg::Update => Self::Update,
            OperationArg::Enable => Self::Enable,
            OperationArg::Disable => Self::Disable,
            OperationArg::Verify => Self::Verify,
            OperationArg::Restore => Self::Restore,
            OperationArg::Remove => Self::Remove,
        }
    }
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum StageArg {
    Downloading,
    Installing,
    ApplyingSettings,
    CheckingHealth,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum RestoreOutcomeArg {
    Success,
    Partial,
    Manual,
}

impl From<RestoreOutcomeArg> for MockOutcome {
    fn from(value: RestoreOutcomeArg) -> Self {
        match value {
            RestoreOutcomeArg::Success => Self::Succeed,
            RestoreOutcomeArg::Partial => Self::RestorePartially,
            RestoreOutcomeArg::Manual => Self::RestoreRequiresManualRecovery,
        }
    }
}

impl From<StageArg> for OperationStage {
    fn from(value: StageArg) -> Self {
        match value {
            StageArg::Downloading => Self::Downloading,
            StageArg::Installing => Self::Installing,
            StageArg::ApplyingSettings => Self::ApplyingSettings,
            StageArg::CheckingHealth => Self::CheckingHealth,
        }
    }
}

fn load_catalog() -> Result<ComponentCatalog, Box<dyn std::error::Error>> {
    let manifests = [
        include_str!("../../../components/manifests/better-manager.yaml"),
        include_str!("../../../components/manifests/better-monitor.yaml"),
        include_str!("../../../components/manifests/better-launcher.yaml"),
        include_str!("../../../components/manifests/better-files.yaml"),
        include_str!("../../../components/manifests/better-touchpad.yaml"),
        include_str!("../../../components/manifests/better-awake.yaml"),
        include_str!("../../../components/manifests/better-storage.yaml"),
        include_str!("../../../components/manifests/better-files-example.yaml"),
    ]
    .into_iter()
    .map(|manifest| Ok(ComponentManifest::parse_yaml(manifest)?))
    .collect::<Result<Vec<_>, Box<dyn std::error::Error>>>()?;
    Ok(ComponentCatalog::from_manifests(manifests)?)
}

fn store_for(path: Option<PathBuf>) -> JsonStore {
    path.map(JsonStore::at_path)
        .unwrap_or_else(JsonStore::from_default_path)
}

fn load_state(
    store: &JsonStore,
    manager: &Manager,
) -> Result<ManagerState, Box<dyn std::error::Error>> {
    let outcome = store.load()?;
    if let Some(backup) = outcome.recovered_corrupt_state {
        println!("recovered malformed state into {}", backup.display());
    }
    manager.validate_state(&outcome.state)?;
    Ok(outcome.state)
}

fn print_plan(plan: &TransactionPlan) {
    println!(
        "review state revision {}: {} action(s)",
        plan.state_revision(),
        plan.steps().len()
    );
    println!(
        "transaction restart requirement: {:?}",
        plan.restart_requirement()
    );
    for step in plan.steps() {
        let before = step.before_version.as_deref().unwrap_or("not installed");
        let after = step.after_version.as_deref().unwrap_or("removed");
        println!(
            "{} {:?}: {} -> {}",
            step.component, step.operation, before, after
        );
        println!(
            "  dependencies: {}",
            if step.dependencies.is_empty() {
                "none".to_string()
            } else {
                step.dependencies
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            }
        );
        println!(
            "  conflicts: {}",
            if step.conflicts.is_empty() {
                "none".to_string()
            } else {
                step.conflicts
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            }
        );
        println!(
            "  replaces: {}",
            if step.replaces.is_empty() {
                "none".to_string()
            } else {
                step.replaces.join(", ")
            }
        );
        println!(
            "  enhances: {}",
            if step.enhances.is_empty() {
                "none".to_string()
            } else {
                step.enhances.join(", ")
            }
        );
        println!(
            "  paths: {}",
            if step.paths.is_empty() {
                "none".to_string()
            } else {
                step.paths.join(", ")
            }
        );
        println!("  restart requirement: {:?}", step.restart_requirement);
        println!(
            "  download size: {}",
            step.estimated_download_bytes
                .map(|bytes| bytes.to_string())
                .unwrap_or_else(|| "not declared by catalog".to_string())
        );
        println!(
            "  required disk space: {}",
            step.required_disk_bytes
                .map(|bytes| bytes.to_string())
                .unwrap_or_else(|| "not declared by catalog".to_string())
        );
        println!(
            "  release notes: {}",
            if step.release_notes.is_empty() {
                "not declared by catalog".to_string()
            } else {
                step.release_notes.join("; ")
            }
        );
        println!(
            "  restore available after apply: {}",
            step.rollback_available
        );
    }
    match plan.disk_space() {
        DiskSpaceCheck::NotRequired => println!("disk space check: not required"),
        DiskSpaceCheck::NotDeclared => {
            println!("disk space check: not declared by catalog or mock profile")
        }
        DiskSpaceCheck::Sufficient {
            required_bytes,
            available_bytes,
        } => println!("disk space check: {required_bytes} required, {available_bytes} available"),
    }
}

/// Everything a real transaction needs to reach the host.
///
/// Built before anything is written, so a missing daemon is reported while the
/// state still says nothing is in progress. Discovering it after `begin` would
/// leave a transaction recorded as started that never was.
type RealContext = (HttpDownloader, DbusPrivilegedExecutor);

fn real_context(mode: ExecutionMode) -> Result<Option<RealContext>, Box<dyn std::error::Error>> {
    match mode {
        ExecutionMode::Mock => Ok(None),
        ExecutionMode::Real => Ok(Some((
            HttpDownloader::new(ArtifactCache::from_default_path()),
            DbusPrivilegedExecutor::connect()?,
        ))),
    }
}

fn advance_until_done(
    manager: &Manager,
    state: &mut ManagerState,
    store: &JsonStore,
    fail_at: Option<OperationStage>,
    restore_outcome: Option<MockOutcome>,
    mode: ExecutionMode,
    real: Option<RealContext>,
) -> Result<(), Box<dyn std::error::Error>> {
    let restore_outcome = match restore_outcome {
        Some(MockOutcome::RestorePartially) => MockRestoreOutcome::Partial,
        Some(MockOutcome::RestoreRequiresManualRecovery) => MockRestoreOutcome::ManualRecovery,
        _ => MockRestoreOutcome::Succeed,
    };

    let driver: Box<dyn StageDriver> = match &real {
        None => Box::new(MockDriver::new(fail_at, restore_outcome)),
        Some((downloader, executor)) => Box::new(RealDriver::new(
            downloader,
            executor,
            uuid::Uuid::new_v4().to_string(),
            manager.profile().clone(),
        )),
    };
    let mut runner = TransactionRunner::new(manager, driver, store);

    let progress = runner.run(state, &mut |event| match event {
        RunnerEvent::StageEntered(stage) => println!("stage: {stage:?}"),
        RunnerEvent::Progress(StageProgress::Downloading {
            component,
            received_bytes,
            total_bytes,
        }) => match total_bytes {
            Some(total) => println!("  downloading {component}: {received_bytes}/{total} bytes"),
            None => println!("  downloading {component}: {received_bytes} bytes"),
        },
        _ => {}
    })?;

    let label = match mode {
        ExecutionMode::Mock => "mock operation",
        ExecutionMode::Real => "operation",
    };
    match progress {
        OperationProgress::Finished { operation } => {
            println!("{label} finished: {operation:?}");
        }
        OperationProgress::Failed { failure } => {
            println!(
                "{label} failed at {:?}: {}{}",
                failure.stage,
                failure.evidence,
                failure
                    .detail
                    .map(|detail| format!(" ({detail})"))
                    .unwrap_or_default()
            );
        }
        OperationProgress::InProgress { stage } => {
            println!("{label} is still at {stage:?}");
        }
    }
    Ok(())
}

fn run_plan(
    manager: &Manager,
    state: &mut ManagerState,
    store: &JsonStore,
    plan: TransactionPlan,
    fail_at: Option<OperationStage>,
    restore_outcome: Option<MockOutcome>,
    mode: ExecutionMode,
) -> Result<(), Box<dyn std::error::Error>> {
    // Reaching the host is arranged first: if it cannot be reached, nothing
    // should have been recorded as started.
    let real = real_context(mode)?;
    print_plan(&plan);
    manager.begin(state, plan)?;
    store.save(state)?;
    advance_until_done(manager, state, store, fail_at, restore_outcome, mode, real)
}

/// Resolves the requested execution mode, refusing combinations that would
/// misrepresent what is about to happen.
fn execution_mode(
    requested: ExecutionArg,
    has_fail_at: bool,
    has_restore_outcome: bool,
) -> Result<ExecutionMode, Box<dyn std::error::Error>> {
    match requested {
        ExecutionArg::Mock => Ok(ExecutionMode::Mock),
        ExecutionArg::Real if has_fail_at || has_restore_outcome => {
            // Scripting an outcome only means something for a simulation. A
            // real run reports what the machine did.
            Err(Box::new(std::io::Error::other(
                "manager.error.mock_flag_in_real_mode",
            )))
        }
        ExecutionArg::Real => Ok(ExecutionMode::Real),
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let manager = Manager::probe(load_catalog()?, &MockPlatform::default())?;
    let store = store_for(cli.state_path);
    let mut state = load_state(&store, &manager)?;

    match cli.command {
        Command::List => {
            for manifest in manager.manifests() {
                println!(
                    "{} {} {:?}",
                    manifest.id, manifest.display_name, manifest.component_type
                );
            }
        }
        Command::Validate => println!("valid: {} component manifests", manager.manifests().count()),
        Command::Status { id } => {
            let ids = id
                .into_iter()
                .map(ComponentId::new)
                .collect::<Result<Vec<_>, _>>()?;
            for manifest in manager
                .manifests()
                .filter(|manifest| ids.is_empty() || ids.contains(&manifest.id))
            {
                let record = state.component(&manifest.id);
                println!(
                    "{} {:?} version={} enabled={} health={:?}",
                    manifest.id,
                    manager.status(&state, &manifest.id)?,
                    record
                        .and_then(|record| record.installed_version.as_deref())
                        .unwrap_or("not installed"),
                    record.is_some_and(|record| record.enabled),
                    record.map(|record| record.health)
                );
            }
        }
        Command::Plan { id, operation } => {
            let plan = manager.plan(&state, &ComponentId::new(id)?, operation.into())?;
            print_plan(&plan);
        }
        Command::UpdateAll => {
            let plan = manager.plan_all(&state)?;
            print_plan(&plan);
        }
        Command::Run {
            id,
            operation,
            fail_at,
        } => {
            let mode = execution_mode(cli.execution, fail_at.is_some(), false)?;
            let plan =
                manager.plan_in_mode(&state, &ComponentId::new(id)?, operation.into(), mode)?;
            run_plan(
                &manager,
                &mut state,
                &store,
                plan,
                fail_at.map(Into::into),
                None,
                mode,
            )?;
        }
        Command::Continue {
            fail_at,
            restore_outcome,
        } => {
            let mode = execution_mode(cli.execution, fail_at.is_some(), restore_outcome.is_some())?;
            let real = real_context(mode)?;
            advance_until_done(
                &manager,
                &mut state,
                &store,
                fail_at.map(Into::into),
                restore_outcome.map(Into::into),
                mode,
                real,
            )?;
        }
        Command::Restore {
            id,
            fail_at,
            restore_outcome,
        } => {
            let mode = execution_mode(cli.execution, fail_at.is_some(), restore_outcome.is_some())?;
            let plan = manager.plan_in_mode(
                &state,
                &ComponentId::new(id)?,
                DesiredOperation::Restore,
                mode,
            )?;
            run_plan(
                &manager,
                &mut state,
                &store,
                plan,
                fail_at.map(Into::into),
                restore_outcome.map(Into::into),
                mode,
            )?;
        }
        Command::Reconcile => {
            let findings = manager.reconcile(&mut state, &DpkgProbe)?;
            // Reconciling changes nothing when the host agrees, and writing an
            // unchanged state would collide with its own revision.
            if !findings.is_empty() {
                store.save(&state)?;
            }
            if findings.is_empty() {
                println!("no drift: dpkg agrees with every recorded component");
            }
            for finding in findings {
                println!(
                    "drift {} recorded={} {:?}",
                    finding.component,
                    finding.recorded.as_deref().unwrap_or("not installed"),
                    finding.drift
                );
            }
        }
        Command::Cancel => {
            manager.cancel(&mut state)?;
            store.save(&state)?;
            println!("mock operation cancelled and snapshots restored");
        }
        Command::Doctor => {
            for check in manager.doctor(&state)? {
                println!("{:?} {:?} {:?}", check.kind, check.status, check.component);
            }
        }
        Command::Defaults { options, command } => {
            let mode = execution_mode(cli.execution, false, false)?;
            defaults::run(
                command,
                options,
                mode,
                &manager.profile().distribution.clone(),
                &state,
                load_catalog()?,
            )?;
        }
        Command::Activity { clear } => {
            if clear {
                state.clear_activity();
                store.save(&state)?;
                println!("activity cleared");
            } else {
                for entry in &state.activity {
                    println!(
                        "{} {:?} component={:?} operation={:?} stage={:?} evidence={}",
                        entry.sequence,
                        entry.kind,
                        entry.component,
                        entry.operation,
                        entry.stage,
                        entry.evidence.as_deref().unwrap_or("none")
                    );
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every component the release publishes must be in the catalog the CLI
    /// ships, or Better Manager cannot offer a package that exists.
    /// `better-manager-daemon` is deliberately absent: it is a dependency of
    /// `better-manager`, not a component a user installs on its own.
    #[test]
    fn the_built_in_catalog_offers_every_released_component() {
        let catalog = load_catalog().expect("the built-in catalog must be valid");
        for component in [
            "better-manager",
            "better-monitor",
            "better-launcher",
            "better-files",
            "better-touchpad",
            "better-awake",
            "better-storage",
        ] {
            let id = ComponentId::new(component).expect("id must be valid");
            let manifest = catalog
                .get(&id)
                .unwrap_or_else(|| panic!("{component} is missing from the built-in catalog"));
            assert_eq!(
                manifest.artifacts.len(),
                4,
                "{component} must declare all four release/architecture variants"
            );
        }
    }
}
