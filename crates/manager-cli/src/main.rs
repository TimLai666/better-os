use better_core::{ComponentCatalog, ComponentId, ComponentManifest};
use clap::{Parser, Subcommand, ValueEnum};
use manager_core::{
    DesiredOperation, DiskSpaceCheck, Manager, ManagerState, MockOutcome, MockSystemProfile,
    OperationProgress, OperationStage, TransactionPlan,
};
use manager_store::{JsonStore, StateStore};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    name = "better-manager",
    about = "Inspect and simulate Better OS component lifecycle changes"
)]
struct Cli {
    /// Use a disposable JSON state file instead of the local default.
    #[arg(long, global = true)]
    state_path: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    List,
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

fn advance_until_done(
    manager: &Manager,
    state: &mut ManagerState,
    store: &JsonStore,
    fail_at: Option<OperationStage>,
    restore_outcome: Option<MockOutcome>,
) -> Result<(), Box<dyn std::error::Error>> {
    loop {
        let stage = state
            .active_operation
            .as_ref()
            .ok_or_else(|| std::io::Error::other("no active mock operation"))?
            .stage;
        let outcome = match fail_at == Some(stage) {
            true => MockOutcome::FailAt(stage),
            false if stage == OperationStage::CheckingHealth => {
                restore_outcome.unwrap_or(MockOutcome::Succeed)
            }
            false => MockOutcome::Succeed,
        };
        match manager.advance(state, outcome)? {
            OperationProgress::InProgress { stage } => {
                store.save(state)?;
                println!("mock stage: {stage:?}");
            }
            OperationProgress::Finished { operation } => {
                store.save(state)?;
                println!("mock operation finished: {operation:?}");
                return Ok(());
            }
            OperationProgress::Failed { failure } => {
                store.save(state)?;
                println!(
                    "mock operation failed at {:?}: {}",
                    failure.stage, failure.evidence
                );
                return Ok(());
            }
        }
    }
}

fn run_plan(
    manager: &Manager,
    state: &mut ManagerState,
    store: &JsonStore,
    plan: TransactionPlan,
    fail_at: Option<OperationStage>,
    restore_outcome: Option<MockOutcome>,
) -> Result<(), Box<dyn std::error::Error>> {
    print_plan(&plan);
    manager.begin(state, plan)?;
    store.save(state)?;
    advance_until_done(manager, state, store, fail_at, restore_outcome)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let manager = Manager::new(load_catalog()?, MockSystemProfile::default());
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
            let plan = manager.plan(&state, &ComponentId::new(id)?, operation.into())?;
            run_plan(
                &manager,
                &mut state,
                &store,
                plan,
                fail_at.map(Into::into),
                None,
            )?;
        }
        Command::Continue {
            fail_at,
            restore_outcome,
        } => {
            advance_until_done(
                &manager,
                &mut state,
                &store,
                fail_at.map(Into::into),
                restore_outcome.map(Into::into),
            )?;
        }
        Command::Restore {
            id,
            fail_at,
            restore_outcome,
        } => {
            let plan = manager.plan(&state, &ComponentId::new(id)?, DesiredOperation::Restore)?;
            run_plan(
                &manager,
                &mut state,
                &store,
                plan,
                fail_at.map(Into::into),
                restore_outcome.map(Into::into),
            )?;
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
