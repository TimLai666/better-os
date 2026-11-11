mod defaults;

use better_core::ComponentId;
use clap::{Parser, Subcommand, ValueEnum};
use manager_core::catalog::{CatalogStatus, now_unix_seconds, refresh as refresh_catalog};
use manager_core::exec::{
    MockDriver, MockRestoreOutcome, RealDriver, RunnerEvent, StageDriver, StageProgress,
    TransactionRunner,
};
use manager_core::repair::{self, PackageSystemState, RepairReport, RepairResult};
use manager_core::{
    DesiredOperation, DiskSpaceCheck, DoctorCheck, DoctorCheckKind, ExecutionMode,
    InstallProvenance, Manager, ManagerState, MockOutcome, OperationProgress, OperationStage,
    TransactionPlan,
};
use manager_platform::catalog_fetch::HttpManifestFetcher;
use manager_platform::download::{ArtifactCache, HttpDownloader};
use manager_platform::dpkg::{DpkgProbe, PackageStateProbe};
use manager_platform::host::ClientPlatform;
use manager_platform::privileged::DbusPrivilegedExecutor;
use manager_store::{JsonCatalogStore, JsonStore, StateStore, cache_refresh, start_catalog};
use std::path::PathBuf;

#[derive(Debug, Parser)]
// `version` prints this crate's own `CARGO_PKG_VERSION`, which is the workspace
// version every Better OS crate inherits. Without it `--version` was an unknown
// argument and the manager had no way of saying which build it is.
//
// `/usr/bin/better-manager` is the window, so this binary is installed as
// `better-manager-cli`. `bin_name` fixes that name in the usage line and in
// clap's errors; left to itself clap would print whatever the process was
// started as, which is `manager-cli` from a build tree.
#[command(
    name = "better-manager-cli",
    bin_name = "better-manager-cli",
    version,
    about = "Inspect, plan, and apply Better OS component lifecycle changes"
)]
struct Cli {
    /// Use a disposable JSON state file instead of the local default.
    #[arg(long, global = true)]
    state_path: Option<PathBuf>,
    /// Use a disposable catalog cache file instead of the local default.
    #[arg(long, global = true)]
    catalog_path: Option<PathBuf>,
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
    ///
    /// Without a flag this only reports. `--adopt <id>` is the way out of a
    /// drift finding: it writes what dpkg says about that one component into
    /// the record, which is what unblocks planning for it.
    Reconcile {
        /// Take dpkg's answer about this component as the truth.
        #[arg(long, value_name = "ID")]
        adopt: Option<String>,
    },
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
    /// Finish a package installation a crash or power loss interrupted.
    ///
    /// Asks the privileged service what dpkg's own record says and, when dpkg
    /// left packages unfinished, asks it to run `dpkg --configure -a`. polkit
    /// asks for an administrator's password first. This command runs nothing
    /// itself, takes no package name, and passes nothing to dpkg.
    Repair {
        /// Only report whether a repair is needed; change nothing.
        #[arg(long)]
        check: bool,
    },
    Activity {
        #[arg(long)]
        clear: bool,
    },
    /// Inspect and refresh the component catalog this manager plans from.
    Catalog {
        #[command(subcommand)]
        command: CatalogCommand,
    },
    /// Inspect and change which component owns each declared system default.
    Defaults {
        #[command(flatten)]
        options: defaults::DefaultsOptions,
        #[command(subcommand)]
        command: defaults::DefaultsCommand,
    },
}

#[derive(Debug, Subcommand)]
enum CatalogCommand {
    /// Report where the catalog came from, when, and whether it may be stale.
    Status,
    /// Fetch the published manifests, validate them, and cache what was
    /// adopted. Nothing on the host is changed: this updates the catalog, not
    /// the machine.
    Refresh,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
enum ExecutionArg {
    /// Walk the lifecycle deterministically without touching the machine.
    Mock,
    /// Download, verify, and apply through the privileged service.
    Real,
}

/// The platform this run plans against.
///
/// `--execution mock` keeps the fixed mock profile, so a simulation produces
/// the same walkthrough on any machine. A real run reads the host: a plan
/// names a package built for exactly one Ubuntu release and one architecture,
/// and the privileged service checks that target against the machine.
fn platform_for(execution: ExecutionArg) -> ClientPlatform {
    match execution {
        ExecutionArg::Mock => ClientPlatform::demo(),
        ExecutionArg::Real => ClientPlatform::host(),
    }
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

fn store_for(path: Option<PathBuf>) -> JsonStore {
    path.map(JsonStore::at_path)
        .unwrap_or_else(JsonStore::from_default_path)
}

fn catalog_store_for(path: Option<PathBuf>) -> JsonCatalogStore {
    path.map(JsonCatalogStore::at_path)
        .unwrap_or_else(JsonCatalogStore::from_default_path)
}

/// Prints where the catalog came from and whether it may be behind the
/// published one. Both `catalog status` and `catalog refresh` end here, so the
/// two commands cannot describe the same state differently.
fn print_catalog_status(status: &CatalogStatus) {
    println!("catalog source: {}", status.source);
    println!(
        "catalog origin: {}",
        status.source_url.as_deref().unwrap_or("compiled in")
    );
    println!(
        "catalog fetched: {}",
        status
            .fetched_at_unix_seconds
            .map(|seconds| format!("{seconds} (seconds since the Unix epoch)"))
            .unwrap_or_else(|| "never".to_string())
    );
    match status.degraded {
        None => println!("catalog state: refreshed in this session"),
        Some(degradation) => println!("catalog state: may be outdated ({degradation})"),
    }
    for rejection in &status.rejections {
        println!("  rejected {rejection}");
    }
}

/// Plans one operation. A real run asks dpkg first, so a package it holds at a
/// version nothing can order is refused before a plan exists (ticket 69); a
/// simulation plans from its own state and never reads the host.
fn plan_for(
    manager: &Manager,
    state: &ManagerState,
    id: &ComponentId,
    operation: DesiredOperation,
    mode: ExecutionMode,
    probe: &dyn PackageStateProbe,
) -> Result<TransactionPlan, manager_core::ManagerError> {
    match mode {
        ExecutionMode::Mock => manager.plan_in_mode(state, id, operation, mode),
        ExecutionMode::Real => manager.plan_on_host(state, id, operation, mode, probe),
    }
}

/// One doctor check: its kind, status, and component on one line, as the
/// command has always printed them, and for a package dpkg holds at a version
/// the manager cannot order, the version and what to do about it.
fn doctor_lines(check: &DoctorCheck) -> Vec<String> {
    let mut lines = vec![format!(
        "{:?} {:?} {:?}",
        check.kind, check.status, check.component
    )];
    if let (DoctorCheckKind::UnorderableHostVersion, Some(component), Some(version)) =
        (check.kind, &check.component, &check.host_version)
    {
        lines.push(format!(
            "  {component} is installed at {version}, a version Better Manager cannot compare, \
             so it is not managed: nothing is installed, updated, or removed for it"
        ));
        lines.push(format!(
            "  to manage it here: remove it with `sudo apt remove {component}`, \
             then install it from Better Manager"
        ));
    }
    lines
}

/// How dpkg's record reads to a person: one heading line, then each package
/// dpkg left unfinished in dpkg's own words, then what to do about it.
fn package_system_lines(state: &PackageSystemState) -> Vec<String> {
    if !state.needs_repair() {
        return vec!["package system: nothing unfinished".to_string()];
    }
    let mut lines = vec!["package system: interrupted".to_string()];
    if state.journal_pending {
        lines.push("  dpkg's journal holds changes it has not finished applying".to_string());
    }
    for package in &state.packages {
        lines.push(format!(
            "  {} {}",
            package.package,
            package.status.as_dpkg()
        ));
    }
    if state.omitted > 0 {
        lines.push(format!("  and {} more", state.omitted));
    }
    lines.push(
        "  until it is repaired, no package can be installed, updated, or removed".to_string(),
    );
    lines.push("  to finish it: better-manager-cli repair".to_string());
    lines
}

/// Lines of output kept from each stream of the command the repair ran.
const REPAIR_TAIL_LINES: usize = 20;

fn tail_lines(text: &str) -> impl Iterator<Item = &str> {
    let lines = text.lines().collect::<Vec<_>>();
    let start = lines.len().saturating_sub(REPAIR_TAIL_LINES);
    lines.into_iter().skip(start)
}

/// What a repair did: the verdict first, then the command and the tail of what
/// it printed, then anything dpkg still has unfinished.
fn repair_report_lines(report: &RepairReport) -> Vec<String> {
    let mut lines = vec![match &report.result {
        RepairResult::Repaired => {
            "repaired: dpkg finished every package it had left unfinished".to_string()
        }
        RepairResult::StillInterrupted => {
            "not fully repaired: dpkg finished and left some packages unfinished".to_string()
        }
        RepairResult::NothingToRepair => {
            "nothing to repair: dpkg has nothing unfinished".to_string()
        }
        RepairResult::Failed { evidence } => format!(
            "repair failed: {}{}",
            evidence.key,
            evidence
                .detail
                .as_deref()
                .map(|detail| format!(" ({detail})"))
                .unwrap_or_default()
        ),
    }];
    for entry in &report.log {
        lines.push(format!(
            "ran: {} (exit {})",
            entry.argv.join(" "),
            entry.exit_code
        ));
        for (label, text) in [
            ("stdout", &entry.stdout_tail),
            ("stderr", &entry.stderr_tail),
        ] {
            if text.trim().is_empty() {
                continue;
            }
            lines.push(format!("  {label}:"));
            lines.extend(tail_lines(text).map(|line| format!("    {line}")));
        }
    }
    if matches!(report.result, RepairResult::Failed { .. }) {
        // The service runs dpkg inside its systemd sandbox, and a kernel or
        // initramfs hook can fail there and nowhere else (ADR 0014).
        lines.push(
            "  if it keeps failing, run the same command from a terminal: sudo dpkg --configure -a"
                .to_string(),
        );
    }
    if let Some(remaining) = report
        .remaining
        .as_ref()
        .filter(|remaining| remaining.needs_repair())
    {
        lines.extend(package_system_lines(remaining));
    }
    lines
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
            &manager_platform::dpkg::DpkgProbe,
            &better_core::package_health::SystemHealthProbe,
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
    let catalog_store = catalog_store_for(cli.catalog_path);
    // The cached catalog if a previous refresh left a usable one, and the
    // compiled-in manifests otherwise. Nothing is fetched here: a command that
    // was not asked to refresh does not go to the network.
    let (catalog, catalog_status) = start_catalog(&catalog_store);
    // An unsupported host stops the command here rather than letting it plan
    // for a release this machine is not running. `--execution mock` still
    // works everywhere, which is what a walkthrough on an unsupported machine
    // is for.
    let manager = Manager::probe(catalog.clone(), &platform_for(cli.execution))?;
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
            let plan = plan_for(
                &manager,
                &state,
                &ComponentId::new(id)?,
                operation.into(),
                mode,
                &DpkgProbe,
            )?;
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
            let plan = plan_for(
                &manager,
                &state,
                &ComponentId::new(id)?,
                DesiredOperation::Restore,
                mode,
                &DpkgProbe,
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
        Command::Reconcile { adopt: Some(id) } => {
            let component = ComponentId::new(id)?;
            let adoption = manager.adopt_host_state(&mut state, &component, &DpkgProbe)?;
            if adoption.changed {
                store.save(&state)?;
            }
            println!(
                "adopted host state for {}: {} -> {}",
                adoption.component,
                adoption.recorded.as_deref().unwrap_or("not installed"),
                adoption.adopted.as_deref().unwrap_or("not installed"),
            );
            if !adoption.changed {
                println!("the record already agreed with dpkg; nothing was written");
            }
        }
        Command::Reconcile { adopt: None } => {
            // Reconciling can adopt a package the host has and the record does
            // not, and can heal a component the host upgraded from outside the
            // manager; both change the state without producing a finding, so
            // the revision — not the finding list — decides whether to write.
            let revision_before = state.revision;
            let findings = manager.reconcile(&mut state, &DpkgProbe)?;
            if state.revision != revision_before {
                store.save(&state)?;
            }
            for (id, record) in &state.components {
                if record.provenance == InstallProvenance::Dpkg {
                    println!(
                        "installed outside better-manager: {id} {}",
                        record.installed_version.as_deref().unwrap_or("unknown")
                    );
                }
            }
            if findings.is_empty() {
                println!("no drift: dpkg agrees with every recorded component");
            }
            for finding in &findings {
                println!(
                    "drift {} recorded={} {:?}",
                    finding.component,
                    finding.recorded.as_deref().unwrap_or("not installed"),
                    finding.drift
                );
            }
            // A finding blocks planning for that component, so the report says
            // what to do about it rather than leaving a dead end.
            for finding in &findings {
                println!(
                    "  to accept what this machine has: better-manager-cli reconcile --adopt {}",
                    finding.component
                );
            }
        }
        Command::Cancel => {
            manager.cancel(&mut state)?;
            store.save(&state)?;
            println!("mock operation cancelled and snapshots restored");
        }
        Command::Repair { check } => {
            // A simulated repair would report that the machine changed when
            // nothing did, so a simulation has no repair to offer.
            if cli.execution == ExecutionArg::Mock {
                return Err(Box::new(std::io::Error::other(
                    "manager.error.repair_needs_real_execution",
                )));
            }
            let service = DbusPrivilegedExecutor::connect()?;
            let dpkg = repair::package_system_state(&service)?;
            for line in package_system_lines(&dpkg) {
                println!("{line}");
            }
            if !check && dpkg.needs_repair() {
                let report = repair::repair_package_system(&mut state, &service);
                store.save(&state)?;
                for line in repair_report_lines(&report) {
                    println!("{line}");
                }
            }
        }
        Command::Doctor => {
            let mut checks = manager.doctor(&state)?;
            // A simulation's state is a fabrication, so dpkg is not asked.
            if cli.execution == ExecutionArg::Real {
                checks.extend(manager.unorderable_host_checks(&state, &DpkgProbe)?);
            }
            for check in &checks {
                for line in doctor_lines(check) {
                    println!("{line}");
                }
            }
        }
        Command::Catalog { command } => match command {
            CatalogCommand::Status => print_catalog_status(&catalog_status),
            CatalogCommand::Refresh => {
                let fetcher = HttpManifestFetcher::from_environment();
                let outcome =
                    refresh_catalog(&fetcher, &catalog, &catalog_status, now_unix_seconds());
                println!(
                    "refreshed {} of {} manifest(s)",
                    outcome.accepted,
                    manager_core::catalog::catalog_files().len()
                );
                // A cache that cannot be written is said out loud. The fetched
                // catalog is still correct for this run; only the next start
                // pays for it.
                if let Err(error) = cache_refresh(&catalog_store, &outcome) {
                    println!("catalog cache not written: {error}");
                }
                print_catalog_status(&outcome.status);
            }
        },
        Command::Defaults { options, command } => {
            let mode = execution_mode(cli.execution, false, false)?;
            defaults::run(
                command,
                options,
                mode,
                &manager.profile().distribution.clone(),
                &state,
                catalog,
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
        let catalog = manager_core::catalog::built_in_catalog();
        for component in [
            "better-manager",
            "better-monitor",
            "better-launcher",
            "better-files",
            "better-git",
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

    /// The seam that replaced `MockPlatform::default()`. The mock answers
    /// Ubuntu 24.04 amd64 on every machine, so a real run on a 22.04 or arm64
    /// host used to plan for packages that host cannot use. Asserted through
    /// the seam so the test does not depend on the machine it runs on.
    #[test]
    fn a_real_run_plans_from_the_host_and_a_mock_run_keeps_the_fixed_profile() {
        assert!(matches!(
            platform_for(ExecutionArg::Real),
            ClientPlatform::Host(_)
        ));
        assert!(matches!(
            platform_for(ExecutionArg::Mock),
            ClientPlatform::Mock(_)
        ));

        let ClientPlatform::Host(host) = platform_for(ExecutionArg::Real) else {
            panic!("a real run must plan from the host");
        };
        assert_eq!(
            host.os_release_path(),
            Some(std::path::Path::new("/etc/os-release"))
        );
    }

    fn interrupted() -> manager_core::repair::PackageSystemState {
        manager_core::repair::PackageSystemState {
            protocol_version: manager_core::repair::PROTOCOL_VERSION,
            journal_pending: true,
            packages: vec![manager_core::repair::InterruptedPackage {
                package: "better-monitor".to_string(),
                status: manager_core::repair::DpkgPackageStatus::HalfConfigured,
            }],
            omitted: 2,
        }
    }

    /// Ticket 69: a real run asks dpkg before planning and refuses a package
    /// it holds at a version nothing can order; a simulation does not ask.
    #[test]
    fn a_real_run_refuses_to_plan_a_package_dpkg_holds_at_an_unorderable_version() {
        let manager = Manager::new(
            manager_core::catalog::built_in_catalog(),
            manager_platform::SystemProfile::default(),
        );
        let state = ManagerState::default();
        let monitor = ComponentId::new("better-monitor").unwrap();
        let probe =
            manager_platform::dpkg::FixedPackageStateProbe::new(&[("better-monitor", "0.2.8~rc1")]);

        let refused = plan_for(
            &manager,
            &state,
            &monitor,
            DesiredOperation::Install,
            ExecutionMode::Real,
            &probe,
        );
        assert!(
            matches!(
                refused,
                Err(manager_core::ManagerError::HostVersionUnreadable(_))
            ),
            "{refused:?}"
        );

        // The demo plans from its fabricated state and never reads the host.
        assert!(
            plan_for(
                &manager,
                &state,
                &monitor,
                DesiredOperation::Install,
                ExecutionMode::Mock,
                &probe,
            )
            .is_ok()
        );
    }

    /// Ticket 69: an unorderable package is listed with dpkg's own version and
    /// what to do about it, not only as a kind and a status.
    #[test]
    fn an_unorderable_package_is_listed_with_its_version_and_what_to_do() {
        let check = manager_core::DoctorCheck {
            kind: manager_core::DoctorCheckKind::UnorderableHostVersion,
            status: manager_core::DoctorCheckStatus::Warning,
            component: Some(ComponentId::new("better-monitor").unwrap()),
            host_version: Some("1:0.2.8~rc1-1".to_string()),
        };
        let lines = doctor_lines(&check);
        assert_eq!(
            lines[0],
            "UnorderableHostVersion Warning Some(ComponentId(\"better-monitor\"))"
        );
        let rest = lines[1..].join("\n");
        assert!(rest.contains("better-monitor"), "{rest}");
        assert!(rest.contains("1:0.2.8~rc1-1"), "{rest}");
        assert!(rest.contains("not managed"), "{rest}");
        assert!(rest.contains("apt remove better-monitor"), "{rest}");

        // Every other check keeps its one line.
        let other = manager_core::DoctorCheck {
            kind: manager_core::DoctorCheckKind::Catalog,
            status: manager_core::DoctorCheckStatus::Passed,
            component: None,
            host_version: None,
        };
        assert_eq!(
            doctor_lines(&other),
            vec!["Catalog Passed None".to_string()]
        );
    }

    #[test]
    fn the_package_system_report_names_each_unfinished_package_as_dpkg_does() {
        let lines = package_system_lines(&interrupted());
        assert_eq!(lines[0], "package system: interrupted");
        assert!(lines.iter().any(|line| line.contains("journal")));
        assert!(
            lines
                .iter()
                .any(|line| line.contains("better-monitor") && line.contains("half-configured"))
        );
        assert!(lines.iter().any(|line| line.contains("2 more")));
        assert!(
            lines
                .iter()
                .any(|line| line.contains("better-manager-cli repair")),
            "the report says what to do about it: {lines:?}"
        );

        let clean = package_system_lines(&manager_core::repair::PackageSystemState::clean());
        assert_eq!(
            clean,
            vec!["package system: nothing unfinished".to_string()]
        );
    }

    #[test]
    fn a_repair_report_carries_the_verdict_and_the_tail_of_dpkgs_output() {
        let report = manager_core::repair::RepairReport {
            result: manager_core::repair::RepairResult::Failed {
                evidence: manager_core::FailureEvidence::new("daemon.error.repair_failed"),
            },
            log: vec![manager_core::repair::LogEntry {
                argv: vec![
                    "dpkg".to_string(),
                    "--configure".to_string(),
                    "-a".to_string(),
                ],
                exit_code: 1,
                stdout_tail: (1..=30)
                    .map(|line| format!("line {line}"))
                    .collect::<Vec<_>>()
                    .join("\n"),
                stderr_tail: "dpkg: error processing package better-monitor".to_string(),
                started_at_unix: 1,
                finished_at_unix: 2,
            }],
            remaining: Some(interrupted()),
        };
        let lines = repair_report_lines(&report);
        assert_eq!(lines[0], "repair failed: daemon.error.repair_failed");
        // The daemon's sandbox can be why dpkg failed (ADR 0014), so the way
        // to try without it is said.
        assert!(
            lines
                .iter()
                .any(|line| line.contains("sudo dpkg --configure -a")),
            "{lines:?}"
        );
        assert!(lines.contains(&"ran: dpkg --configure -a (exit 1)".to_string()));
        assert!(lines.iter().any(|line| line.contains("line 30")));
        assert!(
            !lines.iter().any(|line| line.ends_with("line 1")),
            "only the tail is printed: {lines:?}"
        );
        assert!(
            lines
                .iter()
                .any(|line| line.contains("dpkg: error processing"))
        );
        // What is still unfinished is said, not left for the person to guess.
        assert!(lines.iter().any(|line| line.contains("half-configured")));

        let repaired = manager_core::repair::RepairReport {
            result: manager_core::repair::RepairResult::Repaired,
            log: Vec::new(),
            remaining: Some(manager_core::repair::PackageSystemState::clean()),
        };
        assert_eq!(
            repair_report_lines(&repaired)[0],
            "repaired: dpkg finished every package it had left unfinished"
        );
    }

    /// A host outside the release matrix stops the command instead of planning
    /// for a release it is not running.
    #[test]
    fn an_unidentifiable_host_refuses_rather_than_planning_for_a_guess() {
        let platform = manager_platform::host::HostPlatform::from_fixture(
            "NAME=\"Fedora Linux\"\nID=fedora\nVERSION_ID=41\n",
            "amd64",
        );
        let Err(error) = Manager::probe(manager_core::catalog::built_in_catalog(), &platform)
        else {
            panic!("a host outside the matrix must not resolve");
        };
        assert!(
            error
                .to_string()
                .starts_with("platform.error.unsupported_host:"),
            "{error}"
        );
    }
}
