//! Collection, and everything that answers a question about it.
//!
//! This is where Better Monitor stops being a window. The collectors live
//! here, the sampling loop lives here, and the store is written here, so
//! closing the GUI closes a client and nothing else. Issue #16 states the
//! ownership rule directly — the service, not the GUI window, owns historical
//! collection — and this module is that sentence made true rather than
//! intended.
//!
//! # Layered observation
//!
//! Two layers run, at deliberately different costs.
//!
//! The continuous layer samples every collector once a second and feeds the
//! rounds to a downsampler. One stored sample per resolution period comes out
//! the other side, so the disk sees a fifth of the rounds the CPU does.
//!
//! The audit layer runs every few minutes and asks what the machine *is*: OS,
//! kernel, session, hardware identities, mounts, component versions, and what
//! this build cannot observe. It writes a record only when something changed,
//! so an idle week costs one inventory record rather than two thousand.
//!
//! # Bounded memory
//!
//! The engine holds one raw round, one downsample bucket, and the store's
//! retained window. None of the three grows with uptime.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use monitor_collectors_linux::{LinuxCollectors, ProcessPrivacy, Roots};
use monitor_core::{CollectorId, CollectorReport, MetricDescriptor, SupportState, Timestamp};
use monitor_export::ExportRequest;
use monitor_ipc::{
    CoverageDocument, ExportDocument, ExportState, HistoryDocument, IncidentWindowDocument,
    IncidentsDocument, InventoryDiffDocument, InventoryDocument, IpcError, MonitorRequest,
    MonitorResponse, RequestBody, ResponseBody, StatusDocument, WireCollector,
};
use monitor_store::{
    Downsampler, Gap, GapReason, HistoryStore, Incident, IncidentWindow, RetentionPolicy, Sample,
    StoreError, TimeRange, baseline_shifts, sanitize_note,
};
use tokio::sync::Mutex;

use crate::inventory::{AuditSources, collect as collect_inventory};

/// How often a raw round is taken. One second is above every collector's
/// minimum delta interval, so every counter metric can produce a value.
pub const DEFAULT_SAMPLE_INTERVAL_MS: u64 = 1_000;

/// How often the inventory audit runs.
pub const DEFAULT_AUDIT_INTERVAL_SECONDS: u64 = 300;

/// How many baseline samples an incident is compared against.
pub const INCIDENT_BASELINE_SAMPLES: usize = 24;

/// How far the clock may drift past the resolution before the hole is recorded
/// as a gap. Two periods absorbs ordinary jitter; three does not absorb a
/// suspend.
const MISSED_CADENCE_MULTIPLE: u64 = 3;

/// Everything the service was configured with.
#[derive(Clone, Debug)]
pub struct ServiceConfig {
    pub store_root: PathBuf,
    pub retention: RetentionPolicy,
    pub sample_interval: Duration,
    pub audit_interval: Duration,
    pub tracked_processes: usize,
    /// Command lines are off unless the user turned them on. The service
    /// inherits the same default the window has, because the privacy decision
    /// belongs to the collection, not to the display.
    pub privacy: ProcessPrivacy,
    pub roots: Roots,
    pub audit: AuditSources,
}

impl Default for ServiceConfig {
    fn default() -> Self {
        Self::system()
    }
}

impl ServiceConfig {
    pub fn system() -> Self {
        Self {
            store_root: HistoryStore::default_root(),
            retention: RetentionPolicy::default(),
            sample_interval: Duration::from_millis(DEFAULT_SAMPLE_INTERVAL_MS),
            audit_interval: Duration::from_secs(DEFAULT_AUDIT_INTERVAL_SECONDS),
            tracked_processes: monitor_store::DEFAULT_TRACKED_PROCESSES,
            privacy: ProcessPrivacy::default(),
            roots: Roots::system(),
            audit: AuditSources::system(),
        }
    }

    /// A service reading a captured machine and writing to a temporary
    /// directory. Used by the tests and by `better-monitor record`.
    pub fn at(store_root: impl Into<PathBuf>, roots: Roots, audit: AuditSources) -> Self {
        Self {
            store_root: store_root.into(),
            retention: RetentionPolicy::default(),
            sample_interval: Duration::from_millis(DEFAULT_SAMPLE_INTERVAL_MS),
            audit_interval: Duration::from_secs(DEFAULT_AUDIT_INTERVAL_SECONDS),
            tracked_processes: monitor_store::DEFAULT_TRACKED_PROCESSES,
            privacy: ProcessPrivacy::default(),
            roots,
            audit,
        }
    }
}

pub fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

/// What the engine keeps behind its lock.
struct EngineState {
    store: HistoryStore,
    downsampler: Downsampler,
    /// The most recent raw round. One round, replaced each tick.
    latest_round: Vec<CollectorReport>,
    /// When the current downsample bucket opened.
    bucket_opened_unix_ms: u64,
    last_audit_unix_ms: u64,
    exports: BTreeMap<u64, ExportDocument>,
    capabilities: Vec<(String, Vec<MetricDescriptor>, Vec<SupportState>)>,
    unavailable: BTreeMap<String, Vec<String>>,
}

/// The service's brain. Every request and every tick goes through here.
pub struct MonitorEngine {
    config: ServiceConfig,
    collectors: Mutex<LinuxCollectors>,
    state: Mutex<EngineState>,
    started_at_unix_ms: u64,
    rounds: AtomicU64,
    next_export_id: AtomicU64,
    recording: AtomicBool,
    recovered_truncated_bytes: u64,
}

impl MonitorEngine {
    /// Open the store and prepare to collect. Nothing is sampled yet.
    ///
    /// A hole between the last stored sample and now is recorded before the
    /// first new sample lands, so a chart cannot draw a straight line across
    /// the hours the service was not running.
    pub fn start(config: ServiceConfig) -> Result<Arc<Self>, StoreError> {
        let mut store = HistoryStore::open(&config.store_root, config.retention)?;
        let started_at_unix_ms = now_unix_ms();
        let recovered_truncated_bytes = store.recovery().history.truncated_bytes;

        if let Some(newest) = store.newest_sample_unix_ms()
            && started_at_unix_ms > newest + config.retention.resolution_seconds * 2_000
        {
            store.record_gap(Gap {
                from_unix_ms: newest,
                to_unix_ms: started_at_unix_ms,
                reason: GapReason::ServiceStopped,
            })?;
        }

        let collectors = LinuxCollectors::new(config.roots.clone(), config.privacy);
        let engine = Arc::new(Self {
            collectors: Mutex::new(collectors),
            state: Mutex::new(EngineState {
                store,
                downsampler: Downsampler::new(),
                latest_round: Vec::new(),
                bucket_opened_unix_ms: 0,
                // Zero forces an audit on the first tick, which is what makes
                // the Inventory page useful the moment the service starts.
                last_audit_unix_ms: 0,
                exports: BTreeMap::new(),
                capabilities: Vec::new(),
                unavailable: BTreeMap::new(),
            }),
            started_at_unix_ms,
            rounds: AtomicU64::new(0),
            next_export_id: AtomicU64::new(1),
            recording: AtomicBool::new(true),
            recovered_truncated_bytes,
            config,
        });
        Ok(engine)
    }

    pub fn config(&self) -> &ServiceConfig {
        &self.config
    }

    /// Raw rounds taken since the service started.
    ///
    /// This is the number the GUI-closed collection test watches: it has to
    /// keep climbing after every client has disconnected.
    pub fn rounds(&self) -> u64 {
        self.rounds.load(Ordering::Relaxed)
    }

    /// Turn command-line collection on or off.
    ///
    /// This rebuilds the collectors, which resets every counter delta: the
    /// honest cost of changing what is collected mid-run. It changes what is
    /// gathered, not only what is shown, which is why it belongs here and not
    /// in a presentation layer.
    pub async fn set_privacy(&self, privacy: ProcessPrivacy) {
        let mut collectors = self.collectors.lock().await;
        *collectors = LinuxCollectors::new(self.config.roots.clone(), privacy);
    }

    /// Take one round, downsample it, and write a stored sample when the
    /// bucket is full.
    pub async fn tick(&self) -> Result<(), StoreError> {
        let at = Timestamp::now();
        let wall = now_unix_ms();
        let reports = {
            let mut collectors = self.collectors.lock().await;
            collectors.sample(&self.config.roots, at)
        };
        self.rounds.fetch_add(1, Ordering::Relaxed);

        let sample = Sample::from_reports(&reports, self.config.tracked_processes);
        let mut state = self.state.lock().await;
        state.latest_round = reports;

        if state.bucket_opened_unix_ms == 0 {
            state.bucket_opened_unix_ms = wall;
        }
        state.downsampler.push(sample);

        // A resolution of zero means "store every round", which is what a
        // short-lived recording session wants. It is not a default: the
        // service's own default is five seconds.
        let resolution_ms = self.config.retention.resolution_seconds * 1_000;
        if wall.saturating_sub(state.bucket_opened_unix_ms) >= resolution_ms {
            let previous = state.store.newest_sample_unix_ms();
            if let Some(stored) = state.downsampler.take() {
                // A round that arrived far later than the cadence means
                // something stopped the machine or the process. Say so.
                if let Some(previous) = previous
                    && resolution_ms > 0
                    && wall.saturating_sub(previous) > resolution_ms * MISSED_CADENCE_MULTIPLE
                {
                    state.store.record_gap(Gap {
                        from_unix_ms: previous,
                        to_unix_ms: stored.wall_unix_ms,
                        reason: GapReason::MissedCadence,
                    })?;
                }
                state.store.record_sample(stored)?;
            }
            state.bucket_opened_unix_ms = wall;
        }

        drop(state);
        self.audit_if_due(wall).await?;
        Ok(())
    }

    /// Run the inventory audit if enough time has passed.
    pub async fn audit_if_due(&self, wall_unix_ms: u64) -> Result<bool, StoreError> {
        let due = {
            let state = self.state.lock().await;
            state.last_audit_unix_ms == 0
                || wall_unix_ms.saturating_sub(state.last_audit_unix_ms)
                    >= self.config.audit_interval.as_millis() as u64
        };
        if !due {
            return Ok(false);
        }
        self.audit_now(wall_unix_ms).await
    }

    /// Run the audit unconditionally. Returns whether the machine had changed.
    pub async fn audit_now(&self, wall_unix_ms: u64) -> Result<bool, StoreError> {
        let capabilities = self.capabilities().await;
        let inventory = collect_inventory(&self.config.audit, &capabilities, wall_unix_ms);
        let mut state = self.state.lock().await;
        state.last_audit_unix_ms = wall_unix_ms;
        state.capabilities = capabilities;
        state.unavailable = unavailable_metrics(&state.capabilities);
        state.store.record_inventory(inventory)
    }

    /// What each collector declares, paired with what the latest round proved
    /// about it. This is what "unavailable metrics" in the inventory means.
    async fn capabilities(&self) -> Vec<(String, Vec<MetricDescriptor>, Vec<SupportState>)> {
        let latest = { self.state.lock().await.latest_round.clone() };
        let latest = if latest.is_empty() {
            // Before the first round there is nothing to prove support with, so
            // one round is taken rather than reporting every metric unknown.
            let mut collectors = self.collectors.lock().await;
            collectors.sample(&self.config.roots, Timestamp::now())
        } else {
            latest
        };
        latest
            .iter()
            .map(|report| {
                let descriptors = descriptors_for(&report.collector);
                let support = descriptors
                    .iter()
                    .map(|descriptor| report.support_of(&descriptor.id))
                    .collect();
                (report.collector.to_string(), descriptors, support)
            })
            .collect()
    }

    /// Flush everything and stop reporting as recording.
    ///
    /// The bucket in flight is written rather than dropped: a user who closes
    /// their session should not lose the last few seconds before it.
    pub async fn shutdown(&self) -> Result<(), StoreError> {
        self.recording.store(false, Ordering::Relaxed);
        let mut state = self.state.lock().await;
        if let Some(pending) = state.downsampler.take() {
            state.store.record_sample(pending)?;
        }
        state.store.flush()
    }

    /// One request in, one reply out. Never panics, never blocks on a
    /// collector, and never answers with something it did not check.
    pub async fn handle(&self, request: MonitorRequest) -> MonitorResponse {
        if let Err(error) = request.validate() {
            return MonitorResponse::rejected(error.to_string());
        }
        match self.dispatch(request).await {
            Ok(response) => response,
            Err(error) => MonitorResponse::rejected(error.to_string()),
        }
    }

    /// Handle a raw document, for a transport that has one.
    pub async fn handle_document(&self, document: &str) -> String {
        let response = match MonitorRequest::from_json(document) {
            Ok(request) => self.handle(request).await,
            Err(error) => MonitorResponse::rejected(error.to_string()),
        };
        response.to_json().unwrap_or_else(|_| {
            format!(
                r#"{{"protocol_version":{},"body":{{"response":"rejected","error_key":"monitor.ipc.error.malformed"}}}}"#,
                monitor_ipc::PROTOCOL_VERSION
            )
        })
    }

    async fn dispatch(&self, request: MonitorRequest) -> Result<MonitorResponse, StoreError> {
        match request.body {
            RequestBody::QueryStatus {
                include_latest_round,
            } => Ok(MonitorResponse::status(
                self.status(include_latest_round).await,
            )),
            RequestBody::QueryHistory {
                from_unix_ms,
                to_unix_ms,
                max_samples,
            } => {
                let state = self.state.lock().await;
                let slice = state.store.slice(
                    TimeRange {
                        from_unix_ms,
                        to_unix_ms,
                    },
                    max_samples as usize,
                );
                Ok(MonitorResponse::new(ResponseBody::History(Box::new(
                    HistoryDocument {
                        slice,
                        resolution_seconds: self.config.retention.resolution_seconds,
                    },
                ))))
            }
            RequestBody::QueryCoverage {
                from_unix_ms,
                to_unix_ms,
            } => {
                let range = TimeRange {
                    from_unix_ms,
                    to_unix_ms,
                };
                let state = self.state.lock().await;
                Ok(MonitorResponse::new(ResponseBody::Coverage(Box::new(
                    CoverageDocument {
                        range,
                        metrics: state.store.coverage(range),
                        gaps: state.store.slice(range, 0).gaps,
                    },
                ))))
            }
            RequestBody::QueryIncidents => {
                let state = self.state.lock().await;
                Ok(MonitorResponse::new(ResponseBody::Incidents(Box::new(
                    IncidentsDocument {
                        incidents: state.store.incidents().to_vec(),
                    },
                ))))
            }
            RequestBody::QueryIncidentWindow { incident_id } => {
                let state = self.state.lock().await;
                match state.store.incident_window(incident_id) {
                    Some((incident, slice)) => Ok(MonitorResponse::new(
                        ResponseBody::IncidentWindow(Box::new(IncidentWindowDocument {
                            incident: incident.clone(),
                            slice,
                        })),
                    )),
                    None => Ok(MonitorResponse::rejected(
                        IpcError::UnknownIncident { incident_id }.to_string(),
                    )),
                }
            }
            RequestBody::MarkIncident {
                note,
                window_before_seconds,
                window_after_seconds,
                about_pid,
            } => {
                let incident = self
                    .mark(
                        note.as_deref(),
                        IncidentWindow {
                            before_seconds: window_before_seconds,
                            after_seconds: window_after_seconds,
                        },
                        about_pid,
                    )
                    .await?;
                let state = self.state.lock().await;
                let (incident, slice) = state
                    .store
                    .incident_window(incident)
                    .expect("the incident was just recorded");
                Ok(MonitorResponse::new(ResponseBody::IncidentWindow(
                    Box::new(IncidentWindowDocument {
                        incident: incident.clone(),
                        slice,
                    }),
                )))
            }
            RequestBody::QueryInventory => {
                let state = self.state.lock().await;
                Ok(MonitorResponse::new(ResponseBody::Inventory(Box::new(
                    InventoryDocument {
                        inventory: state.store.latest_inventory().cloned(),
                        captures: state.store.inventory_records().len() as u32,
                    },
                ))))
            }
            RequestBody::QueryInventoryDiff => {
                let state = self.state.lock().await;
                Ok(MonitorResponse::new(ResponseBody::InventoryDiff(Box::new(
                    InventoryDiffDocument {
                        diff: state.store.latest_inventory_diff(),
                    },
                ))))
            }
            RequestBody::RequestExport {
                from_unix_ms,
                to_unix_ms,
                destination,
                include_processes,
                preview_only,
            } => {
                let document = self
                    .export(
                        TimeRange {
                            from_unix_ms,
                            to_unix_ms,
                        },
                        PathBuf::from(destination),
                        include_processes,
                        preview_only,
                    )
                    .await;
                Ok(MonitorResponse::new(ResponseBody::Export(Box::new(
                    document,
                ))))
            }
            RequestBody::QueryExportProgress { export_id } => {
                let state = self.state.lock().await;
                match state.exports.get(&export_id) {
                    Some(document) => Ok(MonitorResponse::new(ResponseBody::Export(Box::new(
                        document.clone(),
                    )))),
                    None => Ok(MonitorResponse::rejected(
                        IpcError::UnknownExport { export_id }.to_string(),
                    )),
                }
            }
        }
    }

    /// The current status document.
    pub async fn status(&self, include_latest_round: bool) -> StatusDocument {
        let state = self.state.lock().await;
        let collectors = state
            .latest_round
            .iter()
            .map(|report| WireCollector {
                collector: report.collector.clone(),
                health: report.health.clone(),
                unavailable_metrics: state
                    .unavailable
                    .get(report.collector.as_str())
                    .cloned()
                    .unwrap_or_default(),
            })
            .collect();
        StatusDocument {
            recording: self.recording.load(Ordering::Relaxed),
            service_started_at_unix_ms: self.started_at_unix_ms,
            now_unix_ms: now_unix_ms(),
            rounds_collected: self.rounds(),
            sample_interval_ms: self.config.sample_interval.as_millis() as u64,
            retention: self.config.retention,
            store: state.store.stats(),
            recovered_truncated_bytes: self.recovered_truncated_bytes,
            collectors,
            latest_round: include_latest_round.then(|| state.latest_round.clone()),
        }
    }

    /// Mark this moment. Returns the new incident's identifier.
    ///
    /// The snapshot is the round in hand rather than a fresh one: taking a new
    /// round here would measure the moment the button reached the service, not
    /// the moment the machine was slow.
    pub async fn mark(
        &self,
        note: Option<&str>,
        window: IncidentWindow,
        about_pid: Option<u32>,
    ) -> Result<u64, StoreError> {
        let wall = now_unix_ms();
        let mut state = self.state.lock().await;
        let snapshot = if state.latest_round.is_empty() {
            // Nothing has been collected yet. An empty snapshot with the right
            // timestamp is honest; a fabricated one would not be.
            Sample::from_reports(&[], self.config.tracked_processes)
        } else {
            Sample::from_reports(&state.latest_round, self.config.tracked_processes)
        };

        let baseline: Vec<Sample> = state
            .store
            .slice(TimeRange::last(window.before_seconds, wall), usize::MAX)
            .samples
            .into_iter()
            .rev()
            .take(INCIDENT_BASELINE_SAMPLES)
            .collect();

        let id = state.store.next_incident_id();
        state.store.record_incident(Incident {
            id,
            marked_at_unix_ms: wall,
            monotonic_ns: Timestamp::now().monotonic_ns,
            note: note.and_then(sanitize_note),
            window: window.clamped(),
            baseline: baseline_shifts(&snapshot, &baseline),
            snapshot: Box::new(snapshot),
            about_pid,
        })?;
        Ok(id)
    }

    /// Build or preview an export package.
    ///
    /// It runs to completion before the reply is sent. The range is bounded by
    /// retention, so the work is bounded too, and a client that got an
    /// `export_id` back with a `Running` state it could never resolve would be
    /// worse than a caller that waited.
    pub async fn export(
        &self,
        range: TimeRange,
        destination: PathBuf,
        include_processes: bool,
        preview_only: bool,
    ) -> ExportDocument {
        let export_id = self.next_export_id.fetch_add(1, Ordering::Relaxed);
        let request = ExportRequest {
            range,
            destination,
            include_processes,
        };
        let mut state = self.state.lock().await;
        let now = now_unix_ms();
        let outcome = if preview_only {
            monitor_export::preview(&state.store, &request, now).map(|report| {
                ExportState::Previewed {
                    redactions: report.replacements,
                    rules: report.rule_keys(),
                    samples: state.store.slice(range, usize::MAX).samples.len() as u64,
                }
            })
        } else {
            monitor_export::write_package(&state.store, &request, now).map(|outcome| {
                ExportState::Completed {
                    directory: outcome.directory.display().to_string(),
                    files: outcome.files,
                    samples: outcome.samples,
                    gaps: outcome.gaps,
                    incidents: outcome.incidents,
                    redactions: outcome.report.replacements,
                }
            })
        };
        let document = ExportDocument {
            export_id,
            state: outcome.unwrap_or_else(|error| ExportState::Failed {
                error_key: error.to_string(),
            }),
        };
        state.exports.insert(export_id, document.clone());
        // The record of past exports is bookkeeping, not history, so it is
        // bounded like everything else the engine holds.
        while state.exports.len() > 32 {
            let oldest = *state.exports.keys().next().expect("a non-empty map");
            state.exports.remove(&oldest);
        }
        document
    }

    /// Read access to the store, for a caller in the same process.
    pub async fn with_store<T>(&self, read: impl FnOnce(&HistoryStore) -> T) -> T {
        let state = self.state.lock().await;
        read(&state.store)
    }
}

fn unavailable_metrics(
    capabilities: &[(String, Vec<MetricDescriptor>, Vec<SupportState>)],
) -> BTreeMap<String, Vec<String>> {
    capabilities
        .iter()
        .map(|(collector, descriptors, support)| {
            let unavailable = descriptors
                .iter()
                .zip(support.iter())
                .filter(|(_, state)| !matches!(state, SupportState::Supported))
                .map(|(descriptor, _)| descriptor.id.to_string())
                .collect();
            (collector.clone(), unavailable)
        })
        .collect()
}

/// The declared catalog for one collector.
///
/// The collectors expose their descriptors as associated functions rather than
/// through the trait, so this maps a name to the right one instead of the
/// service holding six typed handles it does not otherwise need.
fn descriptors_for(collector: &CollectorId) -> Vec<MetricDescriptor> {
    use monitor_collectors_linux::{
        CpuCollector, MemoryCollector, NetworkCollector, PressureCollector, ProcessCollector,
        StorageCollector,
    };
    match collector.as_str() {
        "linux.cpu" => CpuCollector::descriptors(),
        "linux.memory" => MemoryCollector::descriptors(),
        "linux.pressure" => PressureCollector::descriptors(),
        "linux.process" => ProcessCollector::descriptors(),
        "linux.storage" => StorageCollector::descriptors(),
        "linux.network" => NetworkCollector::descriptors(),
        _ => Vec::new(),
    }
}
