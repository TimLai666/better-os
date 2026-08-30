//! The `defaults` subcommands.
//!
//! These are the command-line half of Better Defaults. They build the same
//! [`DefaultsEngine`] the GUI will, review the same plan, and print exactly what
//! each entry did — including the entries that did nothing and why.

use std::error::Error;
use std::path::PathBuf;

use better_core::{ComponentCatalog, ComponentId, ComponentManifest};
use clap::{Args, Subcommand};
use defaults_core::{
    AdapterMode, AdapterSession, AggregateState, ComponentReadiness, Confirmations, DefaultsEngine,
    DefaultsOutcome, DefaultsPlan, DefaultsReport, EntryOutcome, IntegrationState, PlanAction,
    PlanKind, Selection, SystemContext,
};
use defaults_store::SnapshotStore;
use manager_core::{ExecutionMode, HealthState, ManagerState};

#[derive(Debug, Subcommand)]
pub enum DefaultsCommand {
    /// Show what each component's declared integrations currently say.
    Inspect,
    /// Show what applying Better OS defaults would change, without changing it.
    Plan {
        /// Plan putting back the captured previous values instead.
        #[arg(long)]
        restore: bool,
    },
    /// Apply Better OS defaults for the selected components.
    Apply,
    /// Read every integration again and record what was seen.
    Verify,
    /// Put back the values captured before Better OS changed them.
    Restore,
}

#[derive(Debug, Args)]
pub struct DefaultsOptions {
    /// Where defaults snapshots are kept.
    #[arg(long)]
    pub snapshot_dir: Option<PathBuf>,
    /// Read these manifests instead of the built-in catalog. Repeatable.
    #[arg(long = "manifest")]
    pub manifests: Vec<PathBuf>,
    /// Where mock execution keeps its simulated desktop between runs.
    #[arg(long)]
    pub mock_desktop: Option<PathBuf>,
    /// Limit the operation to these components. Repeatable.
    #[arg(long = "component")]
    pub components: Vec<String>,
    /// Overwrite an entry that changed outside Better Manager, as
    /// `component:integration`. Repeatable, and required per entry.
    #[arg(long = "confirm-external")]
    pub confirm_external: Vec<String>,
}

pub fn run(
    command: DefaultsCommand,
    options: DefaultsOptions,
    mode: ExecutionMode,
    distribution: &str,
    state: &ManagerState,
    fallback_catalog: ComponentCatalog,
) -> Result<(), Box<dyn Error>> {
    let catalog = match options.manifests.is_empty() {
        true => fallback_catalog,
        false => {
            let manifests = options
                .manifests
                .iter()
                .map(|path| {
                    let text = std::fs::read_to_string(path)?;
                    Ok::<_, Box<dyn Error>>(ComponentManifest::parse_yaml(&text)?)
                })
                .collect::<Result<Vec<_>, _>>()?;
            ComponentCatalog::from_manifests(manifests)?
        }
    };

    let mut engine = DefaultsEngine::new(
        &catalog,
        SystemContext::new(distribution, defaults_core::adapters::desktop_session()),
    );
    for manifest in catalog.manifests() {
        engine = engine.with_readiness(manifest.id.clone(), readiness(state, &manifest.id));
    }

    let selection = if options.components.is_empty() {
        Selection::All
    } else {
        Selection::Components(
            options
                .components
                .iter()
                .map(|value| ComponentId::new(value.clone()))
                .collect::<Result<Vec<_>, _>>()?,
        )
    };
    let confirmations = parse_confirmations(&options.confirm_external)?;
    let store = SnapshotStore::at_path(
        options
            .snapshot_dir
            .clone()
            .unwrap_or_else(|| SnapshotStore::from_default_path().directory().to_path_buf()),
    );
    let mut session = AdapterSession::open(&adapter_mode(mode, options.mock_desktop.clone()))?;
    if session.is_ephemeral() {
        println!(
            "mock execution: the simulated desktop is not kept between runs; \
             pass --mock-desktop PATH to keep it"
        );
    }
    match command {
        DefaultsCommand::Inspect => {
            print_report(&engine.inspect(&selection, session.adapters(), &store.history()?));
        }
        DefaultsCommand::Plan { restore } => {
            let history = store.history()?;
            let plan = if restore {
                engine.plan_restore(&selection, session.adapters(), &history, &confirmations)
            } else {
                engine.plan_apply(&selection, session.adapters(), &history, &confirmations)
            };
            print_plan(&plan);
        }
        DefaultsCommand::Apply | DefaultsCommand::Restore => {
            let history = store.history()?;
            let plan = match command {
                DefaultsCommand::Apply => {
                    engine.plan_apply(&selection, session.adapters(), &history, &confirmations)
                }
                _ => engine.plan_restore(&selection, session.adapters(), &history, &confirmations),
            };
            print_plan(&plan);
            let outcome = engine.execute(&plan, session.adapters_mut(), &store)?;
            print_outcome(&outcome);
        }
        DefaultsCommand::Verify => {
            print_report(&engine.verify(&selection, session.adapters(), &store)?);
        }
    }

    session.persist()?;
    Ok(())
}

/// What this run works against. Mock execution simulates a desktop and says so;
/// real execution gets the production adapters ADR 0009 decided.
fn adapter_mode(mode: ExecutionMode, mock_desktop: Option<PathBuf>) -> AdapterMode {
    match mode {
        ExecutionMode::Mock => AdapterMode::Simulated {
            desktop_path: mock_desktop,
        },
        ExecutionMode::Real => AdapterMode::Production,
    }
}

fn readiness(state: &ManagerState, component: &ComponentId) -> ComponentReadiness {
    match state.component(component) {
        None => ComponentReadiness::default(),
        Some(record) => ComponentReadiness {
            installed: record.installed_version.is_some(),
            enabled: record.enabled,
            healthy: record.health == HealthState::Healthy,
        },
    }
}

fn parse_confirmations(values: &[String]) -> Result<Confirmations, Box<dyn Error>> {
    let mut confirmations = Confirmations::none();
    for value in values {
        let (component, integration) = value.split_once(':').ok_or_else(|| {
            std::io::Error::other(format!("expected component:integration, found {value}"))
        })?;
        confirmations = confirmations.with(
            ComponentId::new(component)?,
            better_core::IntegrationId::new(integration)?,
        );
    }
    Ok(confirmations)
}

fn print_report(report: &DefaultsReport) {
    for path in &report.damaged_snapshots {
        println!("unreadable snapshot: {path}");
    }
    if report.components.is_empty() {
        println!("no component in this catalog declares a default integration");
    }
    for component in &report.components {
        println!(
            "{} {}",
            component.component,
            aggregate_label(&component.aggregate)
        );
        for status in &component.integrations {
            println!(
                "  {} {:?} {} current={:?} desired={:?} session_effect={:?} restore_available={}",
                status.integration,
                status.kind,
                state_label(&status.state),
                status.current,
                status.desired,
                status.session_effect,
                status.restore_available
            );
        }
    }
}

fn aggregate_label(aggregate: &AggregateState) -> String {
    match aggregate {
        AggregateState::Default => "default".to_string(),
        AggregateState::NotDefault => "not-default".to_string(),
        AggregateState::PartiallyDefault => "partially-default".to_string(),
        AggregateState::ChangedExternally => "changed-externally".to_string(),
        AggregateState::Unavailable { reason } => format!("unavailable ({reason})"),
        AggregateState::Conflict { claimant } => format!("conflict (claimed by {claimant})"),
        AggregateState::Unknown { reason } => format!("unknown ({reason})"),
        AggregateState::NeedsSignOut => "needs-sign-out".to_string(),
    }
}

fn state_label(state: &IntegrationState) -> String {
    match state {
        IntegrationState::Default => "default".to_string(),
        IntegrationState::NotDefault => "not-default".to_string(),
        IntegrationState::ChangedExternally { last_known } => {
            format!("changed-externally (last written {last_known:?})")
        }
        IntegrationState::Unavailable { reason } => format!("unavailable ({reason})"),
        IntegrationState::Conflict { claimant } => format!("conflict (claimed by {claimant})"),
        IntegrationState::Unknown { reason } => format!("unknown ({reason})"),
        IntegrationState::NeedsSignOut => "needs-sign-out".to_string(),
    }
}

fn print_plan(plan: &DefaultsPlan) {
    for path in &plan.damaged_snapshots {
        println!("unreadable snapshot: {path}");
    }
    println!(
        "{:?} plan: {} of {} entries would change",
        plan.kind,
        plan.changes().count(),
        plan.entries.len()
    );
    for entry in &plan.entries {
        let action = match &entry.action {
            PlanAction::Apply { to } => format!("apply {to:?}"),
            PlanAction::Restore { to } => format!("restore {to:?}"),
            PlanAction::Skip { reason } => format!("skip {reason:?}"),
        };
        println!("{}:{} {action}", entry.component, entry.integration);
        println!("  current: {:?}", entry.current);
        println!(
            "  captured previous: {}",
            entry
                .captured_previous
                .as_ref()
                .map(|value| format!("{value:?}"))
                .unwrap_or_else(|| "nothing captured yet".to_string())
        );
        println!("  session effect: {:?}", entry.session_effect);
        if entry.requires_confirmation {
            println!(
                "  changed outside Better Manager: confirmed={} (to overwrite it, run \
                 `defaults --confirm-external {}:{} apply`)",
                entry.confirmed, entry.component, entry.integration
            );
        }
        for warning in &entry.warnings {
            println!("  warning: {warning:?}");
        }
    }
    if plan.kind == PlanKind::Apply && plan.is_empty() {
        println!("nothing to apply");
    }
}

fn print_outcome(outcome: &DefaultsOutcome) {
    println!(
        "{:?} finished: {} of {} entries succeeded",
        outcome.kind,
        outcome.succeeded(),
        outcome.results.len()
    );
    if let Some(id) = &outcome.baseline_snapshot {
        println!("captured previous values into snapshot {id}");
    }
    if let Some(id) = &outcome.recorded_snapshot {
        println!("recorded results into snapshot {id}");
    }
    for result in &outcome.results {
        let line = match &result.outcome {
            EntryOutcome::Applied { value } => format!("applied {value:?}"),
            EntryOutcome::AppliedNeedsSignOut { value } => {
                format!("applied {value:?}, effective after sign-out")
            }
            EntryOutcome::Restored { value } => format!("restored {value:?}"),
            EntryOutcome::AlreadyCorrect => "already correct".to_string(),
            EntryOutcome::NotVerified { observed } => {
                format!("NOT VERIFIED: the setting now reads {observed:?}")
            }
            EntryOutcome::VerificationInconclusive { observed } => {
                format!("could not verify: the setting reads {observed:?}")
            }
            EntryOutcome::Skipped { reason } => format!("skipped {reason:?}"),
            EntryOutcome::ManualActionRequired { reason, detail } => format!(
                "manual action required: {reason}{}",
                detail
                    .as_ref()
                    .map(|detail| format!(" ({detail})"))
                    .unwrap_or_default()
            ),
            EntryOutcome::Failed { reason, detail } => format!(
                "failed: {reason}{}",
                detail
                    .as_ref()
                    .map(|detail| format!(" ({detail})"))
                    .unwrap_or_default()
            ),
        };
        println!("{}:{} {line}", result.component, result.integration);
    }
}
