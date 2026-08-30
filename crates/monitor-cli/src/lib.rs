//! `better-monitor`: the command line over the same contracts as the window.
//!
//! Every subcommand goes through `monitor-ipc` when the service is running and
//! through `monitor-store` when it is not, and it says which one it did. That
//! distinction is not decoration: a store read while the service is running is
//! a snapshot that may be seconds stale, and a store *write* while the service
//! is running would be a second writer to a single-writer log. So the rule is
//! one line long — if the service is up, the service does it — and the output
//! never hides which happened.
//!
//! Nothing here formats a number the GUI would format differently, because
//! nothing here decides what a number means. The CLI reads the same documents
//! and prints them.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};
use monitor_export::{ExportRequest, Rule};
use monitor_ipc::{ExportState, StatusDocument};
use monitor_service::{ClientError, MonitorClient, MonitorEngine, ServiceConfig, now_unix_ms};
use monitor_store::{
    HistoryStore, Incident, IncidentWindow, Inventory, RetentionPolicy, StoreError, TimeRange,
};

/// Better Monitor's command line.
#[derive(Debug, Parser)]
#[command(
    name = "better-monitor",
    about = "Inspect, record, mark, and export what Better Monitor observed",
    version
)]
pub struct Cli {
    /// The history directory to read or write when the service is not running.
    #[arg(long, global = true, value_name = "DIR")]
    pub store: Option<PathBuf>,

    /// Never talk to the service, even if it is running.
    ///
    /// Useful for looking at a store the service does not own, and for
    /// reproducing what a machine without the service would show.
    #[arg(long, global = true)]
    pub offline: bool,

    /// Print machine-readable JSON instead of a summary.
    #[arg(long, global = true)]
    pub json: bool,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// What is being observed right now, and what history exists.
    Inspect(InspectArgs),
    /// Collect into the store for a fixed time, without the service.
    Record(RecordArgs),
    /// Mark this moment as an incident.
    Mark(MarkArgs),
    /// Write a redacted diagnostics package.
    Export(ExportArgs),
    /// What works, what does not, and what cannot be observed here.
    Doctor,
}

#[derive(Debug, Args)]
pub struct InspectArgs {
    /// How far back to summarize, in seconds.
    #[arg(long, default_value_t = 300)]
    pub last: u64,
}

#[derive(Debug, Args)]
pub struct RecordArgs {
    /// How long to record for, in seconds.
    #[arg(long, default_value_t = 30)]
    pub seconds: u64,
    /// Seconds between stored samples. Zero stores every raw round.
    #[arg(long)]
    pub resolution: Option<u64>,
    /// Collect command lines. Off by default: they carry tokens and personal
    /// paths, and an export redacts but cannot un-collect them.
    #[arg(long)]
    pub command_lines: bool,
}

#[derive(Debug, Args)]
pub struct MarkArgs {
    /// A sentence about what happened.
    #[arg(long)]
    pub note: Option<String>,
    /// Seconds of history before the marker that belong to the incident.
    #[arg(long, default_value_t = monitor_store::DEFAULT_WINDOW_BEFORE_SECONDS)]
    pub before: u64,
    /// Seconds after the marker that belong to the incident.
    #[arg(long, default_value_t = monitor_store::DEFAULT_WINDOW_AFTER_SECONDS)]
    pub after: u64,
    /// The process the marker is about.
    #[arg(long)]
    pub pid: Option<u32>,
}

#[derive(Debug, Args)]
pub struct ExportArgs {
    /// The directory to create. Must not already hold a package.
    #[arg(long, value_name = "DIR")]
    pub to: PathBuf,
    /// How far back to export, in seconds.
    #[arg(long, default_value_t = 3_600)]
    pub last: u64,
    /// Report what redaction would remove, and write nothing.
    #[arg(long)]
    pub preview: bool,
    /// Leave the per-sample process list out entirely.
    #[arg(long)]
    pub no_processes: bool,
}

/// Everything that can stop a subcommand.
#[derive(Debug)]
pub enum CliError {
    Store(StoreError),
    Client(ClientError),
    Export(monitor_export::ExportError),
    /// The requested operation needs the service, or needs it to be stopped.
    Refused(String),
}

impl std::fmt::Display for CliError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CliError::Store(error) => write!(formatter, "{error}"),
            CliError::Client(error) => write!(formatter, "{error}"),
            CliError::Export(error) => write!(formatter, "{error}"),
            CliError::Refused(detail) => write!(formatter, "{detail}"),
        }
    }
}

impl From<StoreError> for CliError {
    fn from(error: StoreError) -> Self {
        CliError::Store(error)
    }
}

impl From<ClientError> for CliError {
    fn from(error: ClientError) -> Self {
        CliError::Client(error)
    }
}

impl From<monitor_export::ExportError> for CliError {
    fn from(error: monitor_export::ExportError) -> Self {
        CliError::Export(error)
    }
}

/// Where the answer came from. Printed, always, because a reader has to know
/// whether they are looking at a live service or a file on disk.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Source {
    Service,
    StoreOnDisk,
}

impl Source {
    pub fn label(self) -> &'static str {
        match self {
            Source::Service => "the running service",
            Source::StoreOnDisk => "the history store on disk (the service is not running)",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Source::Service => "service",
            Source::StoreOnDisk => "store",
        }
    }
}

/// Try the service, unless told not to.
pub async fn reach_service(cli: &Cli) -> Option<MonitorClient> {
    if cli.offline {
        return None;
    }
    let client = MonitorClient::connect().await.ok()?;
    // Building a proxy proves nothing; asking for a property proves the
    // service is actually there and speaking this protocol version.
    match client.protocol_version().await {
        Ok(version) if version == monitor_ipc::PROTOCOL_VERSION => Some(client),
        _ => None,
    }
}

pub fn store_root(cli: &Cli) -> PathBuf {
    cli.store.clone().unwrap_or_else(HistoryStore::default_root)
}

fn open_store(cli: &Cli) -> Result<HistoryStore, CliError> {
    Ok(HistoryStore::open(
        store_root(cli),
        RetentionPolicy::default(),
    )?)
}

/// Run one subcommand and produce the text to print.
pub async fn run(cli: &Cli) -> Result<String, CliError> {
    match &cli.command {
        Command::Inspect(args) => inspect(cli, args).await,
        Command::Record(args) => record(cli, args).await,
        Command::Mark(args) => mark(cli, args).await,
        Command::Export(args) => export(cli, args).await,
        Command::Doctor => doctor(cli).await,
    }
}

async fn inspect(cli: &Cli, args: &InspectArgs) -> Result<String, CliError> {
    let now = now_unix_ms();
    let range = TimeRange::last(args.last, now);

    if let Some(client) = reach_service(cli).await {
        let status = client.status(false).await?;
        let history = client
            .history(range.from_unix_ms, range.to_unix_ms, 20_000)
            .await?;
        let incidents = client.incidents().await?;
        if cli.json {
            return Ok(json(&serde_json::json!({
                "source": Source::Service.key(),
                "status": status,
                "history": history,
                "incidents": incidents.incidents.len(),
            })));
        }
        let mut out = String::new();
        out.push_str(&status_summary(&status));
        out.push_str(&slice_summary(
            history.slice.samples.len(),
            history.slice.gaps.len(),
            args.last,
        ));
        out.push_str(&format!("incidents        {}\n", incidents.incidents.len()));
        out.push_str(&format!("read from        {}\n", Source::Service.label()));
        return Ok(out);
    }

    let store = open_store(cli)?;
    let slice = store.slice(range, usize::MAX);
    let stats = store.stats();
    if cli.json {
        return Ok(json(&serde_json::json!({
            "source": Source::StoreOnDisk.key(),
            "store": stats,
            "samples": slice.samples.len(),
            "gaps": slice.gaps.len(),
            "incidents": store.incidents().len(),
        })));
    }
    let mut out = String::new();
    out.push_str("recording        no, the service is not running\n");
    out.push_str(&format!("store            {}\n", store.root().display()));
    out.push_str(&format!("stored samples   {}\n", stats.samples));
    out.push_str(&format!(
        "on disk          {}\n",
        bytes_label(stats.bytes_on_disk)
    ));
    out.push_str(&slice_summary(
        slice.samples.len(),
        slice.gaps.len(),
        args.last,
    ));
    out.push_str(&format!("incidents        {}\n", store.incidents().len()));
    out.push_str(&format!(
        "read from        {}\n",
        Source::StoreOnDisk.label()
    ));
    Ok(out)
}

async fn record(cli: &Cli, args: &RecordArgs) -> Result<String, CliError> {
    // A single-writer log has one writer. Recording while the service is up
    // would be a second one, and the honest answer is to say so rather than to
    // interleave two runs into one file.
    if reach_service(cli).await.is_some() {
        return Err(CliError::Refused(
            "the service is already recording into this store; stop it, or pass --store \
             to record somewhere else"
                .to_string(),
        ));
    }

    let mut config = ServiceConfig::system();
    config.store_root = store_root(cli);
    if let Some(resolution) = args.resolution {
        config.retention.resolution_seconds = resolution;
    }
    config.privacy = monitor_collectors_linux::ProcessPrivacy {
        include_command_line: args.command_lines,
    };
    let interval = config.sample_interval;

    let engine = MonitorEngine::start(config)?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(args.seconds);
    let mut ticker = tokio::time::interval(interval);
    while std::time::Instant::now() < deadline {
        ticker.tick().await;
        engine.tick().await?;
    }
    engine.shutdown().await?;

    let stats = engine.with_store(|store| store.stats()).await;
    if cli.json {
        return Ok(json(&serde_json::json!({
            "rounds": engine.rounds(),
            "store": stats,
        })));
    }
    Ok(format!(
        "recorded         {} rounds over {} s\n\
         stored samples   {}\n\
         on disk          {}\n\
         store            {}\n\
         command lines    {}\n",
        engine.rounds(),
        args.seconds,
        stats.samples,
        bytes_label(stats.bytes_on_disk),
        store_root(cli).display(),
        if args.command_lines {
            "collected, because --command-lines was given"
        } else {
            "not collected"
        }
    ))
}

async fn mark(cli: &Cli, args: &MarkArgs) -> Result<String, CliError> {
    let window = IncidentWindow {
        before_seconds: args.before,
        after_seconds: args.after,
    };
    if !window.is_valid() {
        return Err(CliError::Refused(format!(
            "a window has to be between {} and {} seconds on each side",
            monitor_store::MIN_WINDOW_SECONDS,
            monitor_store::MAX_WINDOW_SECONDS
        )));
    }

    if let Some(client) = reach_service(cli).await {
        let document = client
            .mark(args.note.clone(), args.before, args.after, args.pid)
            .await?;
        return Ok(mark_summary(
            cli,
            &document.incident,
            document.slice.samples.len(),
            Source::Service,
        ));
    }

    // Without the service there is nothing collecting, so one round is taken
    // here. It is a real round from the real collectors, not a placeholder,
    // and the summary says the service was not running so nobody reads the
    // marker as having a history behind it.
    let mut config = ServiceConfig::system();
    config.store_root = store_root(cli);
    let engine = MonitorEngine::start(config)?;
    engine.tick().await?;
    let id = engine.mark(args.note.as_deref(), window, args.pid).await?;
    engine.shutdown().await?;

    let (incident, samples) = engine
        .with_store(|store| {
            let (incident, slice) = store
                .incident_window(id)
                .expect("the incident was just recorded");
            (incident.clone(), slice.samples.len())
        })
        .await;
    Ok(mark_summary(cli, &incident, samples, Source::StoreOnDisk))
}

fn mark_summary(cli: &Cli, incident: &Incident, samples: usize, source: Source) -> String {
    if cli.json {
        return json(&serde_json::json!({
            "source": source.key(),
            "incident": incident,
            "samples_in_window": samples,
        }));
    }
    let mut out = format!(
        "incident         {}\n\
         marked at        {} (unix ms)\n\
         window           {} s before, {} s after\n\
         samples in it    {}\n",
        incident.id,
        incident.marked_at_unix_ms,
        incident.window.before_seconds,
        incident.window.after_seconds,
        samples
    );
    if let Some(note) = &incident.note {
        out.push_str(&format!("note             {note}\n"));
    }
    let shifts = incident.largest_shifts(3);
    if shifts.is_empty() {
        out.push_str("baseline         none: there was no recorded history to compare this with\n");
    } else {
        out.push_str("largest shifts\n");
        for (metric, shift) in shifts {
            out.push_str(&format!(
                "  {metric:<34} {:.3} -> {:.3} over {} samples\n",
                shift.baseline, shift.at_marker, shift.baseline_samples
            ));
        }
    }
    out.push_str(&format!("recorded by      {}\n", source.label()));
    out
}

async fn export(cli: &Cli, args: &ExportArgs) -> Result<String, CliError> {
    let now = now_unix_ms();
    let range = TimeRange::last(args.last, now);
    let destination = absolute(&args.to);

    if let Some(client) = reach_service(cli).await {
        let document = client
            .export(
                range.from_unix_ms,
                range.to_unix_ms,
                destination.display().to_string(),
                !args.no_processes,
                args.preview,
            )
            .await?;
        if cli.json {
            return Ok(json(&document));
        }
        return Ok(export_summary(&document.state, Source::Service));
    }

    let store = open_store(cli)?;
    let request = ExportRequest {
        range,
        destination,
        include_processes: !args.no_processes,
    };
    if args.preview {
        let report = monitor_export::preview(&store, &request, now)?;
        if cli.json {
            return Ok(json(&report));
        }
        return Ok(preview_summary(&report));
    }
    let outcome = monitor_export::write_package(&store, &request, now)?;
    if cli.json {
        return Ok(json(&outcome));
    }
    Ok(export_summary(
        &ExportState::Completed {
            directory: outcome.directory.display().to_string(),
            files: outcome.files,
            samples: outcome.samples,
            gaps: outcome.gaps,
            incidents: outcome.incidents,
            redactions: outcome.report.replacements,
        },
        Source::StoreOnDisk,
    ))
}

fn preview_summary(report: &monitor_export::RedactionReport) -> String {
    let mut out = String::from("nothing was written; this is what an export would remove\n");
    for rule in Rule::ALL {
        let count = report.replacements_for(rule);
        if count > 0 {
            out.push_str(&format!("  {:<20} {count}\n", rule.key()));
        }
    }
    if report.replacements == 0 {
        out.push_str("  nothing matched a redaction rule\n");
    }
    out.push_str(&format!(
        "fields scanned   {}\n{}\n",
        report.fields_scanned, report.caveat
    ));
    out
}

fn export_summary(state: &ExportState, source: Source) -> String {
    match state {
        ExportState::Completed {
            directory,
            files,
            samples,
            gaps,
            incidents,
            redactions,
        } => format!(
            "written to       {directory}\n\
             files            {}\n\
             samples          {samples}\n\
             observation gaps {gaps}\n\
             incidents        {incidents}\n\
             redactions       {redactions}\n\
             built by         {}\n\
             Nothing was uploaded. Read redaction-report.json before sharing this.\n",
            files.len(),
            source.label()
        ),
        ExportState::Previewed {
            redactions,
            rules,
            samples,
        } => format!(
            "nothing was written; this is what an export would remove\n\
             samples          {samples}\n\
             redactions       {redactions}\n\
             rules that hit   {}\n",
            if rules.is_empty() {
                "none".to_string()
            } else {
                rules.join(", ")
            }
        ),
        ExportState::Running { step, percent } => {
            format!("in progress      {step} ({percent}%)\n")
        }
        ExportState::Failed { error_key } => format!("failed           {error_key}\n"),
    }
}

async fn doctor(cli: &Cli) -> Result<String, CliError> {
    let mut lines = Vec::new();
    let service = reach_service(cli).await;
    let mut findings: Vec<String> = Vec::new();

    match &service {
        Some(client) => {
            let status = client.status(false).await?;
            lines.push(format!(
                "service          running, {} rounds since {} (unix ms)",
                status.rounds_collected, status.service_started_at_unix_ms
            ));
            lines.push(format!(
                "protocol         version {}",
                monitor_ipc::PROTOCOL_VERSION
            ));
            lines.push(format!(
                "retention        {} s window, {} budget, {} s resolution",
                status.retention.window_seconds,
                bytes_label(status.retention.disk_budget_bytes),
                status.retention.resolution_seconds
            ));
            lines.push(format!(
                "store            {} samples, {}",
                status.store.samples,
                bytes_label(status.store.bytes_on_disk)
            ));
            if status.recovered_truncated_bytes > 0 {
                findings.push(format!(
                    "the previous run was interrupted mid-write; {} bytes were recovered \
                     from the end of the history log and the hole is recorded as a gap",
                    status.recovered_truncated_bytes
                ));
            }
            for collector in &status.collectors {
                lines.push(format!(
                    "  {:<18} {}",
                    collector.collector.as_str(),
                    health_label(&collector.health)
                ));
                if !collector.unavailable_metrics.is_empty() {
                    lines.push(format!(
                        "  {:<18} {} metric(s) cannot be observed here: {}",
                        "",
                        collector.unavailable_metrics.len(),
                        collector.unavailable_metrics.join(" ")
                    ));
                }
            }
            let inventory = client.inventory().await?;
            lines.push(inventory_line(inventory.inventory.as_ref()));
        }
        None => {
            lines.push(if cli.offline {
                "service          not consulted, --offline was given".to_string()
            } else {
                "service          not running".to_string()
            });
            findings.push(
                "nothing is recording history. Start better-monitor-service, or use \
                 `better-monitor record` for a fixed-length session."
                    .to_string(),
            );
            match open_store(cli) {
                Ok(store) => {
                    let stats = store.stats();
                    lines.push(format!("store            {}", store.root().display()));
                    lines.push(format!(
                        "                 schema version {}, {} samples, {}",
                        store.schema_version(),
                        stats.samples,
                        bytes_label(stats.bytes_on_disk)
                    ));
                    let recovery = store.recovery();
                    if recovery.recovered_anything() {
                        findings.push(format!(
                            "a torn write was recovered from the end of the history log \
                             ({} bytes) and the hole is recorded as a gap",
                            recovery.history.truncated_bytes
                        ));
                    }
                    lines.push(inventory_line(store.latest_inventory()));
                }
                Err(error) => {
                    lines.push(format!("store            unreadable: {error}"));
                    findings.push(format!("the history store could not be opened: {error}"));
                }
            }
        }
    }

    if cli.json {
        return Ok(json(&serde_json::json!({
            "service_running": service.is_some(),
            "lines": lines,
            "findings": findings,
        })));
    }

    let mut out = lines.join("\n");
    out.push('\n');
    if findings.is_empty() {
        out.push_str("\nNothing needs attention.\n");
    } else {
        out.push_str("\nWhat needs attention\n");
        for finding in findings {
            out.push_str(&format!("  - {finding}\n"));
        }
    }
    Ok(out)
}

fn inventory_line(inventory: Option<&Inventory>) -> String {
    match inventory {
        Some(inventory) => format!(
            "inventory        {} entries, captured at {} (unix ms)",
            inventory.entries.len(),
            inventory.captured_at_unix_ms
        ),
        None => "inventory        never captured".to_string(),
    }
}

fn health_label(health: &monitor_core::CollectorHealth) -> String {
    match health {
        monitor_core::CollectorHealth::Healthy => "working".to_string(),
        monitor_core::CollectorHealth::Degraded { detail } => format!("partly working: {detail}"),
        monitor_core::CollectorHealth::Failed { detail } => format!("failed: {detail}"),
        monitor_core::CollectorHealth::Unsupported(_) => "not supported here".to_string(),
    }
}

fn status_summary(status: &StatusDocument) -> String {
    format!(
        "recording        {}\n\
         rounds           {}\n\
         stored samples   {}\n\
         on disk          {}\n\
         retention        {} s window, {} budget\n",
        if status.recording {
            "yes"
        } else {
            "shutting down"
        },
        status.rounds_collected,
        status.store.samples,
        bytes_label(status.store.bytes_on_disk),
        status.retention.window_seconds,
        bytes_label(status.retention.disk_budget_bytes)
    )
}

fn slice_summary(samples: usize, gaps: usize, seconds: u64) -> String {
    format!(
        "last {seconds} s{}samples {samples}, observation gaps {gaps}\n",
        " ".repeat(12usize.saturating_sub(seconds.to_string().len()).max(1))
    )
}

/// Bytes, rounded to something a person reads without counting digits.
pub fn bytes_label(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// The protocol will not accept a relative destination, and neither will the
/// exporter, so a path the user typed is resolved here rather than refused for
/// a reason they did not cause.
fn absolute(path: &std::path::Path) -> PathBuf {
    if path.is_absolute() {
        return path.to_path_buf();
    }
    std::env::current_dir()
        .map(|current| current.join(path))
        .unwrap_or_else(|_| path.to_path_buf())
}

fn json<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|error| format!("{{\"error\":\"{error}\"}}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn the_command_line_is_well_formed() {
        Cli::command().debug_assert();
    }

    #[test]
    fn all_five_subcommands_the_ticket_names_are_reachable() {
        for (arguments, matched) in [
            (vec!["better-monitor", "inspect"], "inspect"),
            (vec!["better-monitor", "record"], "record"),
            (vec!["better-monitor", "mark"], "mark"),
            (vec!["better-monitor", "export", "--to", "/tmp/x"], "export"),
            (vec!["better-monitor", "doctor"], "doctor"),
        ] {
            let cli = Cli::try_parse_from(arguments).expect(matched);
            let name = match cli.command {
                Command::Inspect(_) => "inspect",
                Command::Record(_) => "record",
                Command::Mark(_) => "mark",
                Command::Export(_) => "export",
                Command::Doctor => "doctor",
            };
            assert_eq!(name, matched);
        }
    }

    #[test]
    fn command_lines_are_off_unless_they_are_asked_for() {
        let cli = Cli::try_parse_from(["better-monitor", "record"]).unwrap();
        let Command::Record(args) = cli.command else {
            unreachable!()
        };
        assert!(!args.command_lines);

        let cli = Cli::try_parse_from(["better-monitor", "record", "--command-lines"]).unwrap();
        let Command::Record(args) = cli.command else {
            unreachable!()
        };
        assert!(args.command_lines);
    }

    #[test]
    fn an_export_needs_a_destination() {
        assert!(Cli::try_parse_from(["better-monitor", "export"]).is_err());
    }

    #[test]
    fn the_incident_window_defaults_match_the_store() {
        let cli = Cli::try_parse_from(["better-monitor", "mark"]).unwrap();
        let Command::Mark(args) = cli.command else {
            unreachable!()
        };
        assert_eq!(args.before, monitor_store::DEFAULT_WINDOW_BEFORE_SECONDS);
        assert_eq!(args.after, monitor_store::DEFAULT_WINDOW_AFTER_SECONDS);
        assert!(
            IncidentWindow {
                before_seconds: args.before,
                after_seconds: args.after
            }
            .is_valid()
        );
    }

    #[tokio::test]
    async fn an_impossible_window_is_refused_before_anything_is_written() {
        let directory = tempfile::tempdir().unwrap();
        let cli = Cli::try_parse_from([
            "better-monitor",
            "--offline",
            "--store",
            directory.path().to_str().unwrap(),
            "mark",
            "--before",
            "0",
        ])
        .unwrap();
        let error = run(&cli).await.expect_err("a refusal");
        assert!(matches!(error, CliError::Refused(_)));
        assert!(
            !directory
                .path()
                .join(monitor_store::INCIDENTS_FILE_NAME)
                .exists(),
            "a refused marker must not have written anything"
        );
    }

    #[test]
    fn byte_labels_stay_readable() {
        assert_eq!(bytes_label(0), "0 B");
        assert_eq!(bytes_label(512), "512 B");
        assert_eq!(bytes_label(2048), "2.0 KiB");
        assert_eq!(bytes_label(64 * 1024 * 1024), "64.0 MiB");
    }

    #[test]
    fn a_relative_export_destination_is_resolved_rather_than_refused() {
        let resolved = absolute(std::path::Path::new("exports/today"));
        assert!(resolved.is_absolute());
        assert!(resolved.ends_with("exports/today"));
        assert_eq!(
            absolute(std::path::Path::new("/tmp/x")),
            PathBuf::from("/tmp/x")
        );
    }

    #[test]
    fn the_source_of_an_answer_is_always_stateable() {
        assert!(Source::Service.label().contains("service"));
        assert!(Source::StoreOnDisk.label().contains("not running"));
        assert_ne!(Source::Service.key(), Source::StoreOnDisk.key());
    }
}
