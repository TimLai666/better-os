//! The diagnostics export package.
//!
//! A directory, not an archive: there is no compression dependency in this
//! workspace, and a directory is something a person can open and read before
//! deciding to send it anywhere. That last part is the whole design. The
//! package is self-describing — it carries the schemas of its own files, the
//! coverage of every metric in it, the gaps where nothing was recorded, and a
//! report of exactly what redaction removed — so that a reader who has never
//! seen Better Monitor can tell what a number means and, more importantly, can
//! tell where there is no number at all.
//!
//! Nothing here uploads anything. There is no network code in this crate and
//! no request that would reach one.

pub mod redaction;

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

use monitor_core::CollectorHealth;
use monitor_store::{
    CoverageCounts, Gap, HistoryStore, Incident, Inventory, InventoryDiff, ProcessSample, Sample,
    Sensitivity, TimeRange,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub use redaction::{REDACTION_POLICY_VERSION, RedactionReport, Redactor, Rule, RuleSummary};

/// The package format version. A reader checks this before anything else.
pub const PACKAGE_FORMAT_VERSION: u32 = 1;

pub const MANIFEST_FILE: &str = "manifest.json";
pub const INVENTORY_FILE: &str = "inventory.json";
pub const INVENTORY_DIFF_FILE: &str = "inventory-diff.json";
pub const SAMPLES_FILE: &str = "samples.jsonl";
pub const INCIDENTS_FILE: &str = "incidents.json";
pub const COLLECTOR_HEALTH_FILE: &str = "collector-health.json";
pub const COVERAGE_FILE: &str = "coverage.json";
pub const REDACTION_REPORT_FILE: &str = "redaction-report.json";
pub const README_FILE: &str = "README.txt";
pub const SCHEMA_DIRECTORY: &str = "schema";

/// What the caller asked for.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExportRequest {
    pub range: TimeRange,
    /// The directory to create. It must not already exist as a non-empty
    /// package; the exporter refuses rather than merging into one.
    pub destination: PathBuf,
    /// Whether the bounded per-sample process list is included at all.
    ///
    /// Process names are the most identifying thing in the package after the
    /// inventory, and an export meant only to show resource shape does not
    /// need them.
    pub include_processes: bool,
}

impl ExportRequest {
    pub fn new(range: TimeRange, destination: impl Into<PathBuf>) -> Self {
        Self {
            range,
            destination: destination.into(),
            include_processes: true,
        }
    }
}

/// What was written.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ExportOutcome {
    pub directory: PathBuf,
    pub files: Vec<String>,
    pub samples: u64,
    pub gaps: u64,
    pub incidents: u64,
    pub report: RedactionReport,
}

#[derive(Debug, Error)]
pub enum ExportError {
    #[error("monitor.export.error.io:{path}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("monitor.export.error.destination_exists:{path}")]
    DestinationExists { path: PathBuf },
    #[error("monitor.export.error.destination_not_absolute:{path}")]
    DestinationNotAbsolute { path: PathBuf },
    #[error("monitor.export.error.invalid_range")]
    InvalidRange,
    #[error("monitor.export.error.serialize")]
    Serialize(#[source] serde_json::Error),
}

/// The manifest: what this package is and what is in it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    pub format_version: u32,
    pub produced_by: String,
    pub produced_by_version: String,
    pub created_at_unix_ms: u64,
    pub range: TimeRange,
    pub store_schema_version: u32,
    pub redaction_policy_version: u32,
    pub counts: ManifestCounts,
    pub files: Vec<String>,
    /// Said in the file itself, not only in the documentation.
    pub notes: Vec<String>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct ManifestCounts {
    pub samples: u64,
    pub gaps: u64,
    pub incidents: u64,
    pub inventory_captures: u64,
    pub metrics_with_coverage: u64,
}

/// Per-metric coverage plus the intervals nothing covers.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct CoverageFile {
    pub range: TimeRange,
    pub resolution_seconds: u64,
    /// Samples actually present, against how many the resolution implies the
    /// range should have held. A reader comparing them sees the shortfall
    /// without having to count gap records.
    pub samples_present: u64,
    pub samples_expected: u64,
    pub metrics: BTreeMap<String, CoverageCounts>,
    pub gaps: Vec<Gap>,
    pub notes: Vec<String>,
}

/// What each collector was doing across the range.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CollectorHealthEntry {
    pub collector: String,
    pub healthy_samples: u64,
    pub degraded_samples: u64,
    pub failed_samples: u64,
    pub unsupported_samples: u64,
    /// The last health value seen in the range, so a reader knows how it
    /// ended rather than only how often it failed.
    pub last_health: CollectorHealth,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CollectorHealthFile {
    pub collectors: Vec<CollectorHealthEntry>,
}

/// Build the package.
///
/// The whole thing is assembled in memory and written file by file. The range
/// is bounded by the store's own retention, so "in memory" is a few thousand
/// samples at worst.
pub fn write_package(
    store: &HistoryStore,
    request: &ExportRequest,
    now_unix_ms: u64,
) -> Result<ExportOutcome, ExportError> {
    if !request.range.is_valid() {
        return Err(ExportError::InvalidRange);
    }
    if !request.destination.is_absolute() {
        return Err(ExportError::DestinationNotAbsolute {
            path: request.destination.clone(),
        });
    }
    if request.destination.join(MANIFEST_FILE).exists() {
        return Err(ExportError::DestinationExists {
            path: request.destination.clone(),
        });
    }

    let built = build(store, request, now_unix_ms);

    let directory = &request.destination;
    create_dir(directory)?;
    create_dir(&directory.join(SCHEMA_DIRECTORY))?;

    write_json(&directory.join(INVENTORY_FILE), &built.inventory)?;
    write_json(&directory.join(INVENTORY_DIFF_FILE), &built.inventory_diff)?;
    write_bytes(
        &directory.join(SAMPLES_FILE),
        built.samples_jsonl.as_bytes(),
    )?;
    write_json(&directory.join(INCIDENTS_FILE), &built.incidents)?;
    write_json(&directory.join(COLLECTOR_HEALTH_FILE), &built.collectors)?;
    write_json(&directory.join(COVERAGE_FILE), &built.coverage)?;
    write_json(&directory.join(REDACTION_REPORT_FILE), &built.report)?;
    for (name, document) in schema_documents() {
        write_bytes(
            &directory.join(SCHEMA_DIRECTORY).join(name),
            document.as_bytes(),
        )?;
    }
    write_bytes(&directory.join(README_FILE), readme(&built).as_bytes())?;
    write_json(&directory.join(MANIFEST_FILE), &built.manifest)?;

    Ok(ExportOutcome {
        directory: directory.clone(),
        files: built.manifest.files.clone(),
        samples: built.manifest.counts.samples,
        gaps: built.manifest.counts.gaps,
        incidents: built.manifest.counts.incidents,
        report: built.report,
    })
}

/// What redaction would do, without writing anything.
///
/// The specification requires the result to be previewable before it is
/// written, and the only honest way to preview it is to build the package and
/// throw it away, so that is what this does.
pub fn preview(
    store: &HistoryStore,
    request: &ExportRequest,
    now_unix_ms: u64,
) -> Result<RedactionReport, ExportError> {
    if !request.range.is_valid() {
        return Err(ExportError::InvalidRange);
    }
    Ok(build(store, request, now_unix_ms).report)
}

struct Built {
    manifest: Manifest,
    inventory: Option<Inventory>,
    inventory_diff: Option<InventoryDiff>,
    samples_jsonl: String,
    incidents: Vec<Incident>,
    collectors: CollectorHealthFile,
    coverage: CoverageFile,
    report: RedactionReport,
}

fn build(store: &HistoryStore, request: &ExportRequest, now_unix_ms: u64) -> Built {
    let slice = store.slice(request.range, usize::MAX);
    let policy = store.policy();

    let mut redactor = store
        .latest_inventory()
        .map(Redactor::from_inventory)
        .unwrap_or_default();
    if !request.include_processes {
        redactor.withhold("process_identities");
    }

    let inventory = store.latest_inventory().map(|inventory| {
        let mut redacted = inventory.clone();
        for entry in redacted.entries.values_mut() {
            if entry.sensitivity != Sensitivity::Public {
                entry.value = redactor.apply(&entry.value);
            }
        }
        redacted
    });

    let inventory_diff = store.latest_inventory_diff().map(|mut changes| {
        for change in changes
            .added
            .iter_mut()
            .chain(changes.removed.iter_mut())
            .chain(changes.changed.iter_mut())
        {
            if change.sensitivity != Sensitivity::Public {
                change.before = redactor.apply_optional(change.before.as_deref());
                change.after = redactor.apply_optional(change.after.as_deref());
            }
        }
        changes
    });

    let mut samples_jsonl = String::new();
    for sample in &slice.samples {
        let redacted = redact_sample(sample, request.include_processes, &mut redactor);
        if let Ok(line) = serde_json::to_string(&redacted) {
            samples_jsonl.push_str(&line);
            samples_jsonl.push('\n');
        }
    }

    let incidents: Vec<Incident> = store
        .incidents()
        .iter()
        .filter(|incident| {
            let (from, to) = incident.range_unix_ms();
            request.range.overlaps(from, to)
        })
        .map(|incident| {
            let mut redacted = incident.clone();
            redacted.note = redactor.apply_optional(incident.note.as_deref());
            redacted.snapshot = Box::new(redact_sample(
                &incident.snapshot,
                request.include_processes,
                &mut redactor,
            ));
            redacted
        })
        .collect();

    let collectors = collector_health(&slice.samples);
    let metrics = store.coverage(request.range);
    let expected = expected_samples(request.range, &slice.samples, policy.resolution_seconds);
    let coverage = CoverageFile {
        range: request.range,
        resolution_seconds: policy.resolution_seconds,
        samples_present: slice.samples.len() as u64,
        samples_expected: expected,
        metrics,
        gaps: slice.gaps.clone(),
        notes: vec![
            "A metric's counts are per observation state. `unsupported`, \
             `permission_denied`, and `unknown` are not zeroes; they are readings that do \
             not exist."
                .to_string(),
            "`gaps` lists intervals with no samples at all. Do not interpolate across \
             them."
                .to_string(),
        ],
    };

    let report = redactor.report();
    let files: Vec<String> = std::iter::once(MANIFEST_FILE.to_string())
        .chain(
            [
                INVENTORY_FILE,
                INVENTORY_DIFF_FILE,
                SAMPLES_FILE,
                INCIDENTS_FILE,
                COLLECTOR_HEALTH_FILE,
                COVERAGE_FILE,
                REDACTION_REPORT_FILE,
                README_FILE,
            ]
            .into_iter()
            .map(str::to_string),
        )
        .chain(
            schema_documents()
                .into_iter()
                .map(|(name, _)| format!("{SCHEMA_DIRECTORY}/{name}")),
        )
        .collect();

    let manifest = Manifest {
        format_version: PACKAGE_FORMAT_VERSION,
        produced_by: "Better Monitor".to_string(),
        produced_by_version: env!("CARGO_PKG_VERSION").to_string(),
        created_at_unix_ms: now_unix_ms,
        range: request.range,
        store_schema_version: store.schema_version(),
        redaction_policy_version: REDACTION_POLICY_VERSION,
        counts: ManifestCounts {
            samples: slice.samples.len() as u64,
            gaps: slice.gaps.len() as u64,
            incidents: incidents.len() as u64,
            inventory_captures: store.inventory_records().len() as u64,
            metrics_with_coverage: coverage.metrics.len() as u64,
        },
        files,
        notes: vec![
            "This package was created by an explicit user action and was not uploaded \
             anywhere."
                .to_string(),
            "Readings are observation states, not numbers. A missing value is never a \
             zero."
                .to_string(),
            "Command-line arguments were removed. Read redaction-report.json before \
             sharing this package."
                .to_string(),
        ],
    };

    Built {
        manifest,
        inventory,
        inventory_diff,
        samples_jsonl,
        incidents,
        collectors,
        coverage,
        report,
    }
}

fn redact_sample(sample: &Sample, include_processes: bool, redactor: &mut Redactor) -> Sample {
    let mut redacted = sample.clone();
    if include_processes {
        redacted.processes = sample
            .processes
            .iter()
            .map(|process| ProcessSample {
                pid: process.pid,
                name: redactor.apply(&process.name),
                command_line: process
                    .command_line
                    .as_deref()
                    .map(|line| redactor.apply_command_line(line)),
                user: redactor.apply_optional(process.user.as_deref()),
                cpu_utilization: process.cpu_utilization,
                memory_resident: process.memory_resident,
            })
            .collect();
    } else {
        redacted.processes.clear();
    }
    redacted
}

fn collector_health(samples: &[Sample]) -> CollectorHealthFile {
    let mut entries: BTreeMap<String, CollectorHealthEntry> = BTreeMap::new();
    for sample in samples {
        for state in &sample.collectors {
            let entry = entries
                .entry(state.collector.to_string())
                .or_insert_with(|| CollectorHealthEntry {
                    collector: state.collector.to_string(),
                    healthy_samples: 0,
                    degraded_samples: 0,
                    failed_samples: 0,
                    unsupported_samples: 0,
                    last_health: state.health.clone(),
                });
            match &state.health {
                CollectorHealth::Healthy => entry.healthy_samples += 1,
                CollectorHealth::Degraded { .. } => entry.degraded_samples += 1,
                CollectorHealth::Failed { .. } => entry.failed_samples += 1,
                CollectorHealth::Unsupported(_) => entry.unsupported_samples += 1,
            }
            entry.last_health = state.health.clone();
        }
    }
    CollectorHealthFile {
        collectors: entries.into_values().collect(),
    }
}

/// How many samples the range should have held.
///
/// Bounded by the samples actually present at both ends, because an open range
/// covers all of time and the answer "you are missing 10^15 samples" helps
/// nobody.
fn expected_samples(range: TimeRange, samples: &[Sample], resolution_seconds: u64) -> u64 {
    if resolution_seconds == 0 {
        return samples.len() as u64;
    }
    let (Some(first), Some(last)) = (samples.first(), samples.last()) else {
        return 0;
    };
    let from = range.from_unix_ms.max(first.wall_unix_ms);
    let to = range.to_unix_ms.min(last.wall_unix_ms);
    to.saturating_sub(from) / (resolution_seconds * 1_000) + 1
}

fn create_dir(path: &Path) -> Result<(), ExportError> {
    std::fs::create_dir_all(path).map_err(|source| ExportError::Io {
        path: path.to_path_buf(),
        source,
    })
}

fn write_bytes(path: &Path, bytes: &[u8]) -> Result<(), ExportError> {
    std::fs::write(path, bytes).map_err(|source| ExportError::Io {
        path: path.to_path_buf(),
        source,
    })
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), ExportError> {
    let bytes = serde_json::to_vec_pretty(value).map_err(ExportError::Serialize)?;
    write_bytes(path, &bytes)
}

/// The schemas shipped inside the package.
///
/// Hand-written rather than derived, because their job is to be readable by
/// someone who has never seen this codebase, and because a derived schema
/// would not carry the sentences that matter — the ones about what a missing
/// reading means.
fn schema_documents() -> Vec<(&'static str, String)> {
    let sample = serde_json::json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "title": "Better Monitor stored sample",
        "description":
            "One recorded moment. Readings are observation states rather than bare \
             numbers: a metric can be measured, stale, unknown, unsupported, or refused \
             for permission, and only the first is a value.",
        "type": "object",
        "required": ["wall_unix_ms", "monotonic_ns", "rounds", "metrics", "entities",
                     "processes", "collectors"],
        "properties": {
            "wall_unix_ms": {
                "type": "integer",
                "description": "Wall-clock time. Subject to correction; never divide a rate by it."
            },
            "monotonic_ns": {
                "type": "integer",
                "description": "The recording process's monotonic clock. Only differences within \
                                one service run are meaningful."
            },
            "rounds": {
                "type": "integer",
                "description": "Raw collector rounds this sample was downsampled from. \
                                Numbers are means over those rounds; identities are the last \
                                reading."
            },
            "metrics": {
                "type": "object",
                "description": "System-level readings, keyed by metric id. Each value is an \
                                observation: {\"value\":...}, {\"stale\":...}, {\"unknown\":...}, \
                                {\"unsupported\":...}, or {\"permission_denied\":...}."
            },
            "entities": {
                "type": "array",
                "description": "Readings about individual CPUs, devices, links, and pressure \
                                resources."
            },
            "processes": {
                "type": "array",
                "description": "A bounded list of the busiest processes. This is not a complete \
                                process list and was never intended to be."
            },
            "collectors": {
                "type": "array",
                "description": "Each collector's health at this moment."
            }
        }
    });
    let incident = serde_json::json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "title": "Better Monitor incident",
        "description":
            "A moment a person marked, with the state captured at that moment and how far \
             the readings had moved from the preceding baseline.",
        "type": "object",
        "required": ["id", "marked_at_unix_ms", "monotonic_ns", "window", "snapshot"],
        "properties": {
            "window": {
                "type": "object",
                "description": "How much history before and after the marker belongs to this \
                                incident, in seconds, as it was set when the marker was made."
            },
            "baseline": {
                "type": "object",
                "description": "Per-metric comparison with the mean over the baseline window. \
                                Only metrics with a real reading on both sides appear."
            },
            "snapshot": { "$ref": "sample.schema.json" }
        }
    });
    let inventory = serde_json::json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "title": "Better Monitor inventory",
        "description":
            "What the machine is. Each entry carries a sensitivity: public values are \
             exported as they stand, personal and identifier values have been replaced.",
        "type": "object",
        "required": ["schema_version", "captured_at_unix_ms", "entries"]
    });
    let coverage = serde_json::json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "title": "Better Monitor coverage",
        "description":
            "How much of the range each metric was actually observed over, and the \
             intervals where nothing was recorded at all. Absence of data is data; do not \
             interpolate across a gap and do not read a missing value as zero.",
        "type": "object",
        "required": ["range", "metrics", "gaps"]
    });
    vec![
        ("sample.schema.json", pretty(&sample)),
        ("incident.schema.json", pretty(&incident)),
        ("inventory.schema.json", pretty(&inventory)),
        ("coverage.schema.json", pretty(&coverage)),
    ]
}

fn pretty(value: &serde_json::Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
}

fn readme(built: &Built) -> String {
    let counts = built.manifest.counts;
    format!(
        "Better Monitor diagnostics export\n\
         =================================\n\
         \n\
         Format version {format}. Produced by Better Monitor {version}.\n\
         \n\
         What this is\n\
         ------------\n\
         A snapshot of what one machine was doing over a chosen interval, exported by its\n\
         owner on purpose. Nothing here was uploaded anywhere. Better Monitor has no\n\
         network code in its export path.\n\
         \n\
         Files\n\
         -----\n\
         manifest.json            what this package is, and what is in it\n\
         schema/                  the shape of every other file, with what its fields mean\n\
         inventory.json           what the machine is, redacted\n\
         inventory-diff.json      what changed since the previous capture\n\
         samples.jsonl            one recorded moment per line, oldest first\n\
         incidents.json           moments a person marked, with the state at each\n\
         collector-health.json    what each collector was doing across the interval\n\
         coverage.json            per-metric observation coverage and the gaps\n\
         redaction-report.json    what was removed before this package was written\n\
         \n\
         Reading it honestly\n\
         -------------------\n\
         A reading is not a number. It is one of five states: measured, stale, unknown,\n\
         unsupported, or permission denied. Only the first is a value. A metric this\n\
         machine cannot observe is not a zero, and charting it as one will tell you the\n\
         machine was idle when in fact nobody was watching.\n\
         \n\
         coverage.json lists the intervals with no samples at all. Do not draw a line\n\
         across them.\n\
         \n\
         The process list in each sample is bounded. It holds the busiest few processes,\n\
         not every process that was running.\n\
         \n\
         What was removed\n\
         ----------------\n\
         Command-line arguments were dropped whole. Home paths, usernames, hostnames,\n\
         hardware and network addresses, machine identifiers, and credential-shaped text\n\
         were replaced with placeholders. redaction-report.json counts every replacement\n\
         by rule.\n\
         \n\
         Redaction cannot know that an ordinary-looking word is sensitive. Read this\n\
         package before sending it to anyone.\n\
         \n\
         This package\n\
         ------------\n\
         Interval        {from} to {to} (unix milliseconds)\n\
         Samples         {samples}\n\
         Gaps            {gaps}\n\
         Incidents       {incidents}\n\
         Metrics         {metrics}\n\
         Replacements    {replacements}\n",
        format = built.manifest.format_version,
        version = built.manifest.produced_by_version,
        from = built.manifest.range.from_unix_ms,
        to = built.manifest.range.to_unix_ms,
        samples = counts.samples,
        gaps = counts.gaps,
        incidents = counts.incidents,
        metrics = counts.metrics_with_coverage,
        replacements = built.report.replacements,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use monitor_core::{CollectorId, EntityKind, MetricId, MetricSet, Observation};
    use monitor_store::{CollectorState, EntitySample, InventoryEntry, RetentionPolicy};

    fn metric(raw: &str) -> MetricId {
        MetricId::new(raw).unwrap()
    }

    fn sample(at_ms: u64) -> Sample {
        let mut metrics = MetricSet::new();
        metrics.insert(metric("cpu.utilization.busy"), Observation::float(0.4));
        metrics.insert(
            metric("cpu.temperature"),
            Observation::Unsupported(monitor_core::UnsupportedReason::NotReported {
                detail: "no sensor".into(),
            }),
        );
        let mut pressure = MetricSet::new();
        pressure.insert(metric("pressure.some.avg10"), Observation::float(1.0));
        Sample {
            wall_unix_ms: at_ms,
            monotonic_ns: at_ms * 1_000_000,
            rounds: 5,
            metrics,
            entities: vec![EntitySample {
                kind: EntityKind::PressureResource,
                key: "cpu".into(),
                metrics: pressure,
            }],
            processes: vec![ProcessSample {
                pid: 4242,
                name: "curl".into(),
                command_line: Some(
                    "/usr/bin/curl --header Authorization:ghp_S3cretT0kenValue000001".into(),
                ),
                user: Some("tim".into()),
                cpu_utilization: Some(0.9),
                memory_resident: Some(1_048_576),
            }],
            collectors: vec![CollectorState {
                collector: CollectorId::new("linux.cpu").unwrap(),
                health: CollectorHealth::Healthy,
            }],
        }
    }

    fn inventory() -> Inventory {
        let mut inventory = Inventory::new(1_000);
        inventory
            .insert("os.name", InventoryEntry::public("Zorin OS 18"))
            .insert("session.user", InventoryEntry::personal("tim"))
            .insert("session.home", InventoryEntry::personal("/home/tim"))
            .insert("host.name", InventoryEntry::personal("workshop"))
            .insert(
                "network.eth0.mac",
                InventoryEntry::identifier("aa:bb:cc:dd:ee:ff"),
            );
        inventory
    }

    fn store_with_history() -> (tempfile::TempDir, HistoryStore) {
        let directory = tempfile::tempdir().unwrap();
        let mut store =
            HistoryStore::open(directory.path().join("store"), RetentionPolicy::default()).unwrap();
        store.record_inventory(inventory()).unwrap();
        let mut second = inventory();
        second.captured_at_unix_ms = 2_000;
        second.insert("os.name", InventoryEntry::public("Zorin OS 19"));
        store.record_inventory(second).unwrap();
        for index in 0..12u64 {
            store
                .record_sample(sample(100_000 + index * 5_000))
                .unwrap();
        }
        store
            .record_incident(monitor_store::Incident {
                id: 1,
                marked_at_unix_ms: 130_000,
                monotonic_ns: 130_000_000_000,
                note: Some(
                    "froze while /home/tim/build ran with ghp_S3cretT0kenValue000001".into(),
                ),
                window: monitor_store::IncidentWindow::default(),
                baseline: Default::default(),
                snapshot: Box::new(sample(130_000)),
                about_pid: Some(4242),
            })
            .unwrap();
        (directory, store)
    }

    fn export(request: ExportRequest) -> (tempfile::TempDir, ExportOutcome) {
        let (directory, store) = store_with_history();
        let outcome = write_package(&store, &request, 1_700_000_000_000).unwrap();
        (directory, outcome)
    }

    #[test]
    fn a_package_contains_every_file_the_specification_names() {
        let target = tempfile::tempdir().unwrap();
        let destination = target.path().join("better-monitor-export");
        let (_store, outcome) = export(ExportRequest::new(TimeRange::all(), &destination));

        for name in [
            MANIFEST_FILE,
            INVENTORY_FILE,
            INVENTORY_DIFF_FILE,
            SAMPLES_FILE,
            INCIDENTS_FILE,
            COLLECTOR_HEALTH_FILE,
            COVERAGE_FILE,
            REDACTION_REPORT_FILE,
            README_FILE,
        ] {
            assert!(destination.join(name).exists(), "missing {name}");
        }
        for schema in [
            "sample.schema.json",
            "incident.schema.json",
            "inventory.schema.json",
            "coverage.schema.json",
        ] {
            assert!(
                destination.join(SCHEMA_DIRECTORY).join(schema).exists(),
                "missing schema {schema}"
            );
        }
        assert_eq!(outcome.samples, 12);
        assert_eq!(outcome.incidents, 1);
        assert!(outcome.files.iter().any(|file| file.starts_with("schema/")));
    }

    #[test]
    fn a_seeded_secret_never_appears_anywhere_in_the_package() {
        let target = tempfile::tempdir().unwrap();
        let destination = target.path().join("better-monitor-export");
        export(ExportRequest::new(TimeRange::all(), &destination));

        // The secret was planted in a process command line and in an incident
        // note. Every byte of the package is searched, not only the files that
        // were expected to carry it.
        let secret = "ghp_S3cretT0kenValue000001";
        let mut checked = 0usize;
        for entry in walk(&destination) {
            let bytes = std::fs::read(&entry).unwrap();
            let text = String::from_utf8_lossy(&bytes);
            assert!(
                !text.contains(secret),
                "the seeded secret survived into {}",
                entry.display()
            );
            assert!(
                !text.contains("/home/tim"),
                "a home path survived into {}",
                entry.display()
            );
            assert!(
                !text.contains("aa:bb:cc:dd:ee:ff"),
                "a hardware address survived into {}",
                entry.display()
            );
            checked += 1;
        }
        assert!(checked >= 13, "only {checked} files were searched");
    }

    #[test]
    fn no_command_line_argument_survives_into_the_package() {
        let target = tempfile::tempdir().unwrap();
        let destination = target.path().join("export");
        export(ExportRequest::new(TimeRange::all(), &destination));
        let samples = std::fs::read_to_string(destination.join(SAMPLES_FILE)).unwrap();
        assert!(samples.contains("/usr/bin/curl [arguments withheld]"));
        assert!(!samples.contains("--header"));
        assert!(!samples.contains("Authorization"));
    }

    #[test]
    fn the_redaction_report_counts_what_it_removed() {
        let target = tempfile::tempdir().unwrap();
        let destination = target.path().join("export");
        let (_store, outcome) = export(ExportRequest::new(TimeRange::all(), &destination));
        let report = &outcome.report;
        assert!(report.replacements > 0);
        assert!(report.fields_scanned >= report.replacements);
        assert!(report.replacements_for(Rule::CommandArguments) >= 12);
        assert!(report.replacements_for(Rule::Token) >= 1);
        assert!(report.replacements_for(Rule::HomePath) >= 1);
        assert_eq!(report.rules.len(), Rule::ALL.len());

        let written: RedactionReport = serde_json::from_slice(
            &std::fs::read(destination.join(REDACTION_REPORT_FILE)).unwrap(),
        )
        .unwrap();
        assert_eq!(&written, report);
    }

    #[test]
    fn observation_gaps_are_in_the_package_so_missing_data_is_not_read_as_zero() {
        let directory = tempfile::tempdir().unwrap();
        let mut store =
            HistoryStore::open(directory.path().join("store"), RetentionPolicy::default()).unwrap();
        store.record_sample(sample(100_000)).unwrap();
        store
            .record_gap(monitor_store::Gap {
                from_unix_ms: 105_000,
                to_unix_ms: 160_000,
                reason: monitor_store::GapReason::ServiceStopped,
            })
            .unwrap();
        store.record_sample(sample(165_000)).unwrap();

        let target = tempfile::tempdir().unwrap();
        let destination = target.path().join("export");
        write_package(
            &store,
            &ExportRequest::new(TimeRange::all(), &destination),
            1,
        )
        .unwrap();

        let coverage: CoverageFile =
            serde_json::from_slice(&std::fs::read(destination.join(COVERAGE_FILE)).unwrap())
                .unwrap();
        assert_eq!(coverage.gaps.len(), 1);
        assert_eq!(coverage.gaps[0].from_unix_ms, 105_000);
        assert_eq!(coverage.samples_present, 2);
        assert!(coverage.samples_expected > coverage.samples_present);
        assert!(
            coverage
                .notes
                .iter()
                .any(|note| note.contains("interpolate"))
        );
    }

    #[test]
    fn an_unsupported_metric_is_exported_as_unsupported_rather_than_as_a_number() {
        let target = tempfile::tempdir().unwrap();
        let destination = target.path().join("export");
        export(ExportRequest::new(TimeRange::all(), &destination));

        let samples = std::fs::read_to_string(destination.join(SAMPLES_FILE)).unwrap();
        let first: serde_json::Value =
            serde_json::from_str(samples.lines().next().unwrap()).unwrap();
        assert_eq!(
            first["metrics"]["cpu.utilization.busy"]["value"]["float"],
            0.4
        );
        assert!(first["metrics"]["cpu.temperature"].get("value").is_none());
        assert!(first["metrics"]["cpu.temperature"]["unsupported"].is_object());

        let coverage: CoverageFile =
            serde_json::from_slice(&std::fs::read(destination.join(COVERAGE_FILE)).unwrap())
                .unwrap();
        assert_eq!(coverage.metrics["cpu.temperature"].value, 0);
        assert_eq!(coverage.metrics["cpu.temperature"].unsupported, 12);
    }

    #[test]
    fn a_narrower_range_exports_less() {
        let target = tempfile::tempdir().unwrap();
        let destination = target.path().join("export");
        let (_store, outcome) = export(ExportRequest::new(
            TimeRange {
                from_unix_ms: 100_000,
                to_unix_ms: 115_000,
            },
            &destination,
        ));
        assert_eq!(outcome.samples, 4);
    }

    #[test]
    fn withholding_processes_removes_them_and_says_so() {
        let target = tempfile::tempdir().unwrap();
        let destination = target.path().join("export");
        let mut request = ExportRequest::new(TimeRange::all(), &destination);
        request.include_processes = false;
        let (_store, outcome) = export(request);

        let samples = std::fs::read_to_string(destination.join(SAMPLES_FILE)).unwrap();
        assert!(!samples.contains("curl"));
        assert_eq!(
            outcome.report.withheld_data_classes,
            vec!["process_identities".to_string()]
        );
    }

    #[test]
    fn collector_health_is_summarized_across_the_range() {
        let target = tempfile::tempdir().unwrap();
        let destination = target.path().join("export");
        export(ExportRequest::new(TimeRange::all(), &destination));
        let health: CollectorHealthFile = serde_json::from_slice(
            &std::fs::read(destination.join(COLLECTOR_HEALTH_FILE)).unwrap(),
        )
        .unwrap();
        assert_eq!(health.collectors.len(), 1);
        assert_eq!(health.collectors[0].collector, "linux.cpu");
        assert_eq!(health.collectors[0].healthy_samples, 12);
    }

    #[test]
    fn the_manifest_describes_the_package_it_sits_in() {
        let target = tempfile::tempdir().unwrap();
        let destination = target.path().join("export");
        export(ExportRequest::new(TimeRange::all(), &destination));
        let manifest: Manifest =
            serde_json::from_slice(&std::fs::read(destination.join(MANIFEST_FILE)).unwrap())
                .unwrap();
        assert_eq!(manifest.format_version, PACKAGE_FORMAT_VERSION);
        assert_eq!(manifest.counts.samples, 12);
        assert!(
            manifest
                .notes
                .iter()
                .any(|note| note.contains("not uploaded"))
        );
        for file in &manifest.files {
            assert!(
                destination.join(file).exists(),
                "the manifest lists {file}, which is not there"
            );
        }
    }

    #[test]
    fn a_preview_reports_the_same_redaction_without_writing_anything() {
        let (_directory, store) = store_with_history();
        let target = tempfile::tempdir().unwrap();
        let destination = target.path().join("export");
        let request = ExportRequest::new(TimeRange::all(), &destination);

        let previewed = preview(&store, &request, 1).unwrap();
        assert!(!destination.exists(), "a preview must write nothing");

        let written = write_package(&store, &request, 1).unwrap();
        assert_eq!(previewed, written.report);
    }

    #[test]
    fn an_existing_package_is_refused_rather_than_merged_into() {
        let target = tempfile::tempdir().unwrap();
        let destination = target.path().join("export");
        let (_directory, store) = store_with_history();
        let request = ExportRequest::new(TimeRange::all(), &destination);
        write_package(&store, &request, 1).unwrap();
        assert!(matches!(
            write_package(&store, &request, 1),
            Err(ExportError::DestinationExists { .. })
        ));
    }

    #[test]
    fn a_relative_destination_is_refused() {
        let (_directory, store) = store_with_history();
        let request = ExportRequest::new(TimeRange::all(), "relative/export");
        assert!(matches!(
            write_package(&store, &request, 1),
            Err(ExportError::DestinationNotAbsolute { .. })
        ));
    }

    #[test]
    fn a_backwards_range_is_refused() {
        let (_directory, store) = store_with_history();
        let target = tempfile::tempdir().unwrap();
        let request = ExportRequest::new(
            TimeRange {
                from_unix_ms: 900,
                to_unix_ms: 100,
            },
            target.path().join("export"),
        );
        assert!(matches!(
            write_package(&store, &request, 1),
            Err(ExportError::InvalidRange)
        ));
        assert!(matches!(
            preview(&store, &request, 1),
            Err(ExportError::InvalidRange)
        ));
    }

    #[test]
    fn the_readme_tells_a_stranger_how_to_read_the_numbers() {
        let target = tempfile::tempdir().unwrap();
        let destination = target.path().join("export");
        export(ExportRequest::new(TimeRange::all(), &destination));
        let readme = std::fs::read_to_string(destination.join(README_FILE)).unwrap();
        assert!(readme.contains("not a zero"));
        assert!(readme.contains("uploaded"));
        assert!(readme.contains("redaction-report.json"));
    }

    fn walk(directory: &Path) -> Vec<PathBuf> {
        let mut files = Vec::new();
        let mut stack = vec![directory.to_path_buf()];
        while let Some(current) = stack.pop() {
            for entry in std::fs::read_dir(&current).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    stack.push(path);
                } else {
                    files.push(path);
                }
            }
        }
        files
    }
}
