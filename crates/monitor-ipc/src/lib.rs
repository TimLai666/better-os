//! The local protocol the GUI and the CLI speak to `monitor-service`.
//!
//! # Transport
//!
//! JSON documents carried by a *session* D-Bus interface, `org.betteros.Monitor1`
//! at `/org/betteros/Monitor1`. That is the same shape ADR 0007 chose for the
//! privileged daemon and the same choice `awake-ipc` made for Better Awake, and
//! matching it is the point: one way of reading a Better OS local protocol
//! rather than three. A unix socket was the alternative and would have worked,
//! but it would have added a second transport to the project, its own peer
//! credential question, and its own activation and lifecycle story, in exchange
//! for nothing this protocol needs. Issue #16 defers the final answer to an
//! ADR; this module records what is implemented so the choice is written down
//! rather than only present in code.
//!
//! # Trust
//!
//! Both ends run as the same unprivileged user in the same session, so there is
//! no privilege boundary here and shared types with `monitor-core` and
//! `monitor-store` are safe. What survives from `manager-ipc` is the input
//! discipline: closed enums, `deny_unknown_fields`, a byte limit applied to the
//! raw document before it reaches the parser, and validation that runs on the
//! way in rather than at the point of use.
//!
//! # What the protocol will not do
//!
//! There is no request that uploads anything, and no request that starts an
//! export the user did not ask for. Export is a request from a client, with a
//! destination the client names, and the service refuses a destination it
//! cannot treat as an absolute local directory.

use std::collections::BTreeMap;

use monitor_core::{CollectorHealth, CollectorId, CollectorReport};
use monitor_store::{
    CoverageCounts, Gap, HistorySlice, Incident, IncidentWindow, Inventory, InventoryDiff,
    MAX_NOTE_LENGTH, MAX_WINDOW_SECONDS, MIN_WINDOW_SECONDS, RetentionPolicy, StoreStats,
    TimeRange,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// The protocol both sides must agree on exactly. A service refuses any other
/// value rather than guessing which fields it can still trust.
///
/// Version 2 (ticket 97) added entity kinds (graphics cards, NPUs,
/// batteries, power sources, sensors), units, `top_processes`, and
/// `battery_use`, none of which a version 1 client can parse. A client reads
/// the version property first and, on a mismatch, says the service has to
/// restart instead of failing on the first reply.
pub const PROTOCOL_VERSION: u32 = 2;

/// The session bus name the taskbar item owns while it runs, so the window
/// can tell whether one is running.
pub const TRAY_BUS_NAME: &str = "org.betteros.MonitorTray1";

/// The session bus name the service owns.
pub const BUS_NAME: &str = "org.betteros.Monitor1";
/// The object the service publishes.
pub const OBJECT_PATH: &str = "/org/betteros/Monitor1";
/// The interface the object implements.
pub const INTERFACE_NAME: &str = "org.betteros.Monitor1";

/// Largest accepted request. Every request is a handful of scalars and at most
/// one note and one path.
pub const MAX_REQUEST_BYTES: usize = 16 * 1024;

/// Largest accepted reply. A history range is the big one: a few thousand
/// samples, each with bounded entity and process lists.
pub const MAX_RESPONSE_BYTES: usize = 64 * 1024 * 1024;

/// The largest number of samples one reply may carry. A client that wants more
/// asks for a narrower range; the service will not build a reply it cannot
/// bound.
pub const MAX_SAMPLES_PER_REPLY: u32 = 20_000;

/// What a client asks the service to do.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "request", deny_unknown_fields)]
pub enum RequestBody {
    /// What the service is doing right now.
    QueryStatus {
        /// Whether to include the most recent raw collector round.
        ///
        /// The GUI asks for it, because that is what the live pages draw. The
        /// CLI does not, because printing a summary does not need a thousand
        /// process entities crossing the bus.
        #[serde(default)]
        include_latest_round: bool,
        /// Keep only this many of the busiest processes per resource in the
        /// round, instead of every process. The taskbar item asks for this
        /// every second; a thousand process entities a second would cost it
        /// more than everything it draws.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        top_processes: Option<u32>,
    },
    /// Stored samples and the gaps between them, over one interval.
    QueryHistory {
        from_unix_ms: u64,
        to_unix_ms: u64,
        /// Hard cap on returned samples. The reply says whether it was hit.
        max_samples: u32,
    },
    /// Per-metric observation coverage over one interval.
    QueryCoverage {
        from_unix_ms: u64,
        to_unix_ms: u64,
    },
    QueryIncidents,
    /// One incident with the history its own window covers.
    QueryIncidentWindow {
        incident_id: u64,
    },
    /// Mark this moment. The service captures the surrounding state.
    MarkIncident {
        #[serde(default)]
        note: Option<String>,
        window_before_seconds: u64,
        window_after_seconds: u64,
        /// The process the marker is about, when it was raised from a row.
        #[serde(default)]
        about_pid: Option<u32>,
    },
    QueryInventory,
    QueryInventoryDiff,
    /// Build an export package. Always explicit, never automatic, never
    /// uploaded.
    RequestExport {
        from_unix_ms: u64,
        to_unix_ms: u64,
        /// Absolute local directory to create the package in.
        destination: String,
        #[serde(default)]
        include_processes: bool,
        /// Report what redaction would do without writing anything.
        #[serde(default)]
        preview_only: bool,
    },
    QueryExportProgress {
        export_id: u64,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MonitorRequest {
    pub protocol_version: u32,
    pub body: RequestBody,
}

impl MonitorRequest {
    pub fn new(body: RequestBody) -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            body,
        }
    }

    /// Parse and validate a request. The size limit is applied to the raw
    /// bytes first, so an oversized payload never reaches the parser.
    pub fn from_json(document: &str) -> Result<Self, IpcError> {
        if document.len() > MAX_REQUEST_BYTES {
            return Err(IpcError::PayloadTooLarge {
                bytes: document.len(),
                limit: MAX_REQUEST_BYTES,
            });
        }
        let request: MonitorRequest =
            serde_json::from_str(document).map_err(|error| IpcError::Malformed {
                detail: error.to_string(),
            })?;
        request.validate()?;
        Ok(request)
    }

    pub fn to_json(&self) -> Result<String, IpcError> {
        serde_json::to_string(self).map_err(|error| IpcError::Malformed {
            detail: error.to_string(),
        })
    }

    /// Everything checkable without a clock or a store.
    pub fn validate(&self) -> Result<(), IpcError> {
        if self.protocol_version != PROTOCOL_VERSION {
            return Err(IpcError::ProtocolVersion {
                found: self.protocol_version,
                expected: PROTOCOL_VERSION,
            });
        }
        match &self.body {
            RequestBody::QueryHistory {
                from_unix_ms,
                to_unix_ms,
                max_samples,
            } => {
                validate_range(*from_unix_ms, *to_unix_ms)?;
                if *max_samples == 0 || *max_samples > MAX_SAMPLES_PER_REPLY {
                    return Err(IpcError::InvalidSampleLimit {
                        requested: *max_samples,
                        limit: MAX_SAMPLES_PER_REPLY,
                    });
                }
            }
            RequestBody::QueryCoverage {
                from_unix_ms,
                to_unix_ms,
            } => validate_range(*from_unix_ms, *to_unix_ms)?,
            RequestBody::MarkIncident {
                note,
                window_before_seconds,
                window_after_seconds,
                ..
            } => {
                if let Some(note) = note {
                    validate_note(note)?;
                }
                validate_window(IncidentWindow {
                    before_seconds: *window_before_seconds,
                    after_seconds: *window_after_seconds,
                })?;
            }
            RequestBody::RequestExport {
                from_unix_ms,
                to_unix_ms,
                destination,
                ..
            } => {
                validate_range(*from_unix_ms, *to_unix_ms)?;
                validate_destination(destination)?;
            }
            RequestBody::QueryExportProgress { export_id } => {
                if *export_id == 0 {
                    return Err(IpcError::UnknownExport {
                        export_id: *export_id,
                    });
                }
            }
            RequestBody::QueryStatus { .. }
            | RequestBody::QueryIncidents
            | RequestBody::QueryIncidentWindow { .. }
            | RequestBody::QueryInventory
            | RequestBody::QueryInventoryDiff => {}
        }
        Ok(())
    }

    /// The interval a range request asks for, once validated.
    pub fn range(&self) -> Option<TimeRange> {
        match &self.body {
            RequestBody::QueryHistory {
                from_unix_ms,
                to_unix_ms,
                ..
            }
            | RequestBody::QueryCoverage {
                from_unix_ms,
                to_unix_ms,
            }
            | RequestBody::RequestExport {
                from_unix_ms,
                to_unix_ms,
                ..
            } => Some(TimeRange {
                from_unix_ms: *from_unix_ms,
                to_unix_ms: *to_unix_ms,
            }),
            _ => None,
        }
    }
}

fn validate_range(from_unix_ms: u64, to_unix_ms: u64) -> Result<(), IpcError> {
    if from_unix_ms > to_unix_ms {
        return Err(IpcError::InvalidRange {
            from_unix_ms,
            to_unix_ms,
        });
    }
    Ok(())
}

fn validate_note(note: &str) -> Result<(), IpcError> {
    if note.len() > MAX_NOTE_LENGTH {
        return Err(IpcError::NoteTooLong {
            bytes: note.len(),
            limit: MAX_NOTE_LENGTH,
        });
    }
    // A newline is fine in a note. A NUL or an escape sequence is not: it would
    // reach a terminal through the CLI and a text file through the export.
    if note
        .chars()
        .any(|character| character.is_control() && character != '\n')
    {
        return Err(IpcError::NoteControlCharacter);
    }
    Ok(())
}

fn validate_window(window: IncidentWindow) -> Result<(), IpcError> {
    if window.is_valid() {
        Ok(())
    } else {
        Err(IpcError::InvalidWindow {
            before_seconds: window.before_seconds,
            after_seconds: window.after_seconds,
            minimum: MIN_WINDOW_SECONDS,
            maximum: MAX_WINDOW_SECONDS,
        })
    }
}

/// A destination has to be an absolute local path with no traversal in it.
///
/// The service creates a directory there and writes files into it, so a
/// relative path would land wherever the service happens to have been started,
/// and a `..` component would let a client walk out of the directory it named.
fn validate_destination(destination: &str) -> Result<(), IpcError> {
    let invalid = destination.is_empty()
        || !destination.starts_with('/')
        || destination.contains('\0')
        || std::path::Path::new(destination)
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir));
    if invalid {
        return Err(IpcError::InvalidDestination {
            destination: destination.to_string(),
        });
    }
    Ok(())
}

/// One collector's identity and current health, on the wire.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireCollector {
    pub collector: CollectorId,
    pub health: CollectorHealth,
    /// Metrics this collector declares that this host cannot produce. The
    /// Diagnostics and Inventory pages read it so an unavailable metric is
    /// visible rather than absent.
    #[serde(default)]
    pub unavailable_metrics: Vec<String>,
}

/// What the service is doing, and what it has.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StatusDocument {
    /// False only while the service is shutting down. A client that sees it
    /// false must not report the machine as unobserved; it must say the
    /// service is stopping.
    pub recording: bool,
    pub service_started_at_unix_ms: u64,
    pub now_unix_ms: u64,
    /// Raw collector rounds taken since the service started, before
    /// downsampling. The GUI-closed collection test watches this number.
    pub rounds_collected: u64,
    pub sample_interval_ms: u64,
    pub retention: RetentionPolicy,
    pub store: StoreStats,
    /// Non-zero when the previous run was interrupted mid-write.
    pub recovered_truncated_bytes: u64,
    pub collectors: Vec<WireCollector>,
    /// The most recent raw round, when the client asked for it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_round: Option<Vec<CollectorReport>>,
    /// What has used the battery since the machine was last unplugged, on a
    /// machine with a battery.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub battery_use: Option<BatteryUse>,
}

/// Samples and gaps over one interval.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HistoryDocument {
    pub slice: HistorySlice,
    pub resolution_seconds: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoverageDocument {
    pub range: TimeRange,
    pub metrics: BTreeMap<String, CoverageCounts>,
    pub gaps: Vec<Gap>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IncidentsDocument {
    pub incidents: Vec<Incident>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IncidentWindowDocument {
    pub incident: Incident,
    pub slice: HistorySlice,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InventoryDocument {
    /// Absent before the first audit has run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inventory: Option<Inventory>,
    pub captures: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InventoryDiffDocument {
    /// Absent when there has only ever been one capture, which is not the same
    /// answer as "nothing changed".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diff: Option<InventoryDiff>,
}

/// Where an export has got to.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "state", deny_unknown_fields)]
pub enum ExportState {
    /// Accepted and running. `step` is a stable key, not a sentence.
    Running {
        step: String,
        percent: u8,
    },
    Completed {
        directory: String,
        files: Vec<String>,
        samples: u64,
        gaps: u64,
        incidents: u64,
        redactions: u64,
    },
    /// A preview: what redaction would remove, with nothing written.
    Previewed {
        redactions: u64,
        rules: Vec<String>,
        samples: u64,
    },
    Failed {
        error_key: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExportDocument {
    pub export_id: u64,
    pub state: ExportState,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "response", deny_unknown_fields)]
pub enum ResponseBody {
    Status(Box<StatusDocument>),
    History(Box<HistoryDocument>),
    Coverage(Box<CoverageDocument>),
    Incidents(Box<IncidentsDocument>),
    IncidentWindow(Box<IncidentWindowDocument>),
    Inventory(Box<InventoryDocument>),
    InventoryDiff(Box<InventoryDiffDocument>),
    Export(Box<ExportDocument>),
    /// A stable machine key. Presentation layers own the wording.
    Rejected {
        error_key: String,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MonitorResponse {
    pub protocol_version: u32,
    pub body: ResponseBody,
}

impl MonitorResponse {
    pub fn new(body: ResponseBody) -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            body,
        }
    }

    pub fn status(status: StatusDocument) -> Self {
        Self::new(ResponseBody::Status(Box::new(status)))
    }

    pub fn rejected(error_key: impl Into<String>) -> Self {
        Self::new(ResponseBody::Rejected {
            error_key: error_key.into(),
        })
    }

    pub fn from_json(document: &str) -> Result<Self, IpcError> {
        if document.len() > MAX_RESPONSE_BYTES {
            return Err(IpcError::PayloadTooLarge {
                bytes: document.len(),
                limit: MAX_RESPONSE_BYTES,
            });
        }
        let response: MonitorResponse =
            serde_json::from_str(document).map_err(|error| IpcError::Malformed {
                detail: error.to_string(),
            })?;
        if response.protocol_version != PROTOCOL_VERSION {
            return Err(IpcError::ProtocolVersion {
                found: response.protocol_version,
                expected: PROTOCOL_VERSION,
            });
        }
        Ok(response)
    }

    pub fn to_json(&self) -> Result<String, IpcError> {
        serde_json::to_string(self).map_err(|error| IpcError::Malformed {
            detail: error.to_string(),
        })
    }
}

/// What the service pushes without being asked.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "event", deny_unknown_fields)]
pub enum EventBody {
    /// A new incident was marked, by this client or another one.
    IncidentMarked { incident_id: u64 },
    /// An export changed state.
    ExportProgress(Box<ExportDocument>),
    /// The inventory audit found the machine had changed.
    InventoryChanged { captured_at_unix_ms: u64 },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MonitorEvent {
    pub protocol_version: u32,
    pub body: EventBody,
}

impl MonitorEvent {
    pub fn new(body: EventBody) -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            body,
        }
    }

    pub fn from_json(document: &str) -> Result<Self, IpcError> {
        if document.len() > MAX_RESPONSE_BYTES {
            return Err(IpcError::PayloadTooLarge {
                bytes: document.len(),
                limit: MAX_RESPONSE_BYTES,
            });
        }
        let event: MonitorEvent =
            serde_json::from_str(document).map_err(|error| IpcError::Malformed {
                detail: error.to_string(),
            })?;
        if event.protocol_version != PROTOCOL_VERSION {
            return Err(IpcError::ProtocolVersion {
                found: event.protocol_version,
                expected: PROTOCOL_VERSION,
            });
        }
        Ok(event)
    }

    pub fn to_json(&self) -> Result<String, IpcError> {
        serde_json::to_string(self).map_err(|error| IpcError::Malformed {
            detail: error.to_string(),
        })
    }
}

/// Protocol-level rejections. Every message is a stable machine key.
#[derive(Clone, Debug, Eq, PartialEq, Error)]
pub enum IpcError {
    #[error("monitor.ipc.error.payload_too_large:{bytes}:{limit}")]
    PayloadTooLarge { bytes: usize, limit: usize },
    #[error("monitor.ipc.error.malformed:{detail}")]
    Malformed { detail: String },
    #[error("monitor.ipc.error.protocol_version:{found}:{expected}")]
    ProtocolVersion { found: u32, expected: u32 },
    #[error("monitor.ipc.error.invalid_range:{from_unix_ms}:{to_unix_ms}")]
    InvalidRange { from_unix_ms: u64, to_unix_ms: u64 },
    #[error("monitor.ipc.error.invalid_sample_limit:{requested}:{limit}")]
    InvalidSampleLimit { requested: u32, limit: u32 },
    #[error("monitor.ipc.error.note_too_long:{bytes}:{limit}")]
    NoteTooLong { bytes: usize, limit: usize },
    #[error("monitor.ipc.error.note_control_character")]
    NoteControlCharacter,
    #[error(
        "monitor.ipc.error.invalid_window:{before_seconds}:{after_seconds}:{minimum}:{maximum}"
    )]
    InvalidWindow {
        before_seconds: u64,
        after_seconds: u64,
        minimum: u64,
        maximum: u64,
    },
    #[error("monitor.ipc.error.invalid_destination:{destination}")]
    InvalidDestination { destination: String },
    #[error("monitor.ipc.error.unknown_incident:{incident_id}")]
    UnknownIncident { incident_id: u64 },
    #[error("monitor.ipc.error.unknown_export:{export_id}")]
    UnknownExport { export_id: u64 },
}

/// What has used the battery since the machine was last unplugged.
///
/// The service owns this because it is the one thing that is running the
/// whole time the machine is on its battery; a window opened an hour after the
/// plug came out still sees the whole hour. It is kept in memory only, and it
/// restarts when the machine is unplugged again, the "since last unplugged"
/// span a laptop's battery settings show.
pub mod battery_use {
    use std::collections::BTreeMap;

    use monitor_core::{CollectorReport, EntityKind, MetricId, MetricScalar, Observation};
    use serde::{Deserialize, Serialize};

    /// The most process names one document lists.
    pub const MAX_LISTED: usize = 50;

    /// One program's estimated share.
    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
    #[serde(deny_unknown_fields)]
    pub struct ProcessEnergy {
        pub name: String,
        pub watt_hours: f64,
    }

    /// The span and its programs, largest share first.
    #[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
    #[serde(deny_unknown_fields)]
    pub struct BatteryUse {
        /// When the machine was unplugged, or when the service first saw it
        /// on the battery.
        pub since_unix_ms: u64,
        /// Whether it is on the battery right now.
        pub on_battery: bool,
        /// Seconds spent on the battery in the span.
        pub on_battery_seconds: f64,
        /// Everything the battery gave, in watt-hours.
        pub total_watt_hours: f64,
        pub processes: Vec<ProcessEnergy>,
    }

    fn id(raw: &str) -> MetricId {
        MetricId::new(raw).expect("a battery-use metric id is well formed")
    }

    /// Adds each round's draw and per-process estimate into the span.
    #[derive(Debug, Default)]
    pub struct BatteryUseTracker {
        has_battery: bool,
        use_: BatteryUse,
        by_name: BTreeMap<String, f64>,
    }

    impl BatteryUseTracker {
        pub fn new() -> Self {
            Self::default()
        }

        /// Read one round taken `seconds` after the previous one.
        pub fn observe(&mut self, round: &[CollectorReport], seconds: f64, now_unix_ms: u64) {
            let Some(battery) = round
                .iter()
                .find(|report| report.collector.as_str() == "linux.battery")
            else {
                return;
            };
            let on_battery = match battery.metrics.get(&id("power.on_battery")) {
                Some(Observation::Value(MetricScalar::Bool(on))) => *on,
                _ => {
                    self.has_battery = false;
                    return;
                }
            };
            self.has_battery = true;
            if on_battery && !self.use_.on_battery {
                // Unplugged: a new span starts.
                self.use_ = BatteryUse {
                    since_unix_ms: now_unix_ms,
                    ..BatteryUse::default()
                };
                self.by_name.clear();
            }
            self.use_.on_battery = on_battery;
            if !on_battery || !(seconds > 0.0 && seconds < 60.0) {
                return;
            }
            let hours = seconds / 3600.0;
            if let Some(draw) = battery
                .metrics
                .get(&id("power.battery.draw"))
                .and_then(Observation::as_f64)
            {
                self.use_.total_watt_hours += draw * hours;
            }
            self.use_.on_battery_seconds += seconds;
            if let Some(processes) = round
                .iter()
                .find(|report| report.collector.as_str() == "linux.process")
            {
                for entity in processes.entities_of(EntityKind::Process) {
                    let Some(watts) = entity
                        .metrics
                        .get(&id("process.energy.power"))
                        .and_then(Observation::as_f64)
                        .filter(|w| *w > 0.0)
                    else {
                        continue;
                    };
                    let Some(name) = entity
                        .metrics
                        .get(&id("process.name"))
                        .and_then(Observation::as_text)
                    else {
                        continue;
                    };
                    *self.by_name.entry(name.to_string()).or_default() += watts * hours;
                }
            }
        }

        /// The span so far. `None` on a machine without a battery, or before
        /// the machine has ever been seen on its battery.
        pub fn current(&self) -> Option<BatteryUse> {
            if !self.has_battery || self.use_.since_unix_ms == 0 {
                return None;
            }
            let mut use_ = self.use_.clone();
            let mut processes: Vec<ProcessEnergy> = self
                .by_name
                .iter()
                .map(|(name, watt_hours)| ProcessEnergy {
                    name: name.clone(),
                    watt_hours: *watt_hours,
                })
                .collect();
            processes.sort_by(|a, b| {
                b.watt_hours
                    .total_cmp(&a.watt_hours)
                    .then_with(|| a.name.cmp(&b.name))
            });
            processes.truncate(MAX_LISTED);
            use_.processes = processes;
            Some(use_)
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use monitor_core::{CollectorId, Entity, EntityId, MetricSet, Timestamp};

        fn round(on: bool, draw: f64, processes: &[(&str, f64)]) -> Vec<CollectorReport> {
            let at = Timestamp {
                unix_ms: 1,
                monotonic_ns: 1,
            };
            let mut battery = CollectorReport::new(CollectorId::new("linux.battery").unwrap(), at);
            battery
                .metrics
                .insert(id("power.on_battery"), Observation::boolean(on));
            battery
                .metrics
                .insert(id("power.battery.draw"), Observation::float(draw));
            let mut table = CollectorReport::new(CollectorId::new("linux.process").unwrap(), at);
            for (index, (name, watts)) in processes.iter().enumerate() {
                let mut m = MetricSet::new();
                m.insert(id("process.name"), Observation::text(*name));
                m.insert(id("process.energy.power"), Observation::float(*watts));
                table.entities.push(Entity::new(
                    EntityId::new(EntityKind::Process, index.to_string()),
                    m,
                ));
            }
            vec![battery, table]
        }

        #[test]
        fn energy_adds_up_by_name_while_on_the_battery_and_restarts_when_unplugged() {
            let mut tracker = BatteryUseTracker::new();
            tracker.observe(&round(false, 0.0, &[("firefox", 5.0)]), 1.0, 1_000);
            assert_eq!(tracker.current(), None, "never on the battery yet");
            for second in 0..3600 {
                tracker.observe(
                    &round(
                        true,
                        10.0,
                        &[("firefox", 6.0), ("firefox", 1.0), ("code", 2.0)],
                    ),
                    1.0,
                    2_000 + second,
                );
            }
            let use_ = tracker.current().unwrap();
            assert_eq!(use_.since_unix_ms, 2_000);
            assert!((use_.total_watt_hours - 10.0).abs() < 1e-6);
            assert!((use_.on_battery_seconds - 3600.0).abs() < 1e-6);
            assert_eq!(use_.processes[0].name, "firefox");
            assert!((use_.processes[0].watt_hours - 7.0).abs() < 1e-6);
            assert!((use_.processes[1].watt_hours - 2.0).abs() < 1e-6);

            // Plugged in: the span stays, nothing more is added.
            tracker.observe(&round(false, 0.0, &[("firefox", 6.0)]), 1.0, 9_000);
            let plugged = tracker.current().unwrap();
            assert!(!plugged.on_battery);
            assert!((plugged.total_watt_hours - 10.0).abs() < 1e-6);

            // Unplugged again: a new span.
            tracker.observe(&round(true, 10.0, &[]), 1.0, 10_000);
            let again = tracker.current().unwrap();
            assert_eq!(again.since_unix_ms, 10_000);
            assert!(again.processes.is_empty());
        }

        #[test]
        fn a_gap_longer_than_a_minute_is_not_counted_as_drain() {
            let mut tracker = BatteryUseTracker::new();
            tracker.observe(&round(true, 10.0, &[]), 1.0, 1_000);
            tracker.observe(&round(true, 10.0, &[]), 3600.0, 2_000);
            assert!(tracker.current().unwrap().total_watt_hours < 0.01);
        }
    }
}

/// Keeping a round small enough to send every second.
pub mod trim {
    use std::collections::BTreeSet;

    use monitor_core::{CollectorReport, EntityKind, MetricId, Observation};

    /// What a process is ranked by. A process in the top of any one is kept.
    const RANKINGS: [&[&str]; 6] = [
        &["process.cpu.utilization"],
        &["process.memory.resident"],
        &["process.io.read.bytes.rate", "process.io.write.bytes.rate"],
        &["process.network.receive.rate", "process.network.send.rate"],
        &["process.gpu.utilization"],
        &["process.energy.power"],
    ];

    fn score(metrics: &monitor_core::MetricSet, ids: &[&str]) -> f64 {
        ids.iter()
            .filter_map(|raw| {
                MetricId::new(*raw)
                    .ok()
                    .and_then(|id| metrics.get(&id).and_then(Observation::as_f64))
            })
            .sum()
    }

    /// The round with only the `top` busiest processes per resource kept.
    /// Every other report is unchanged, and `process.count` still counts every
    /// process.
    pub fn trim_round(round: &[CollectorReport], top: usize) -> Vec<CollectorReport> {
        round
            .iter()
            .map(|report| {
                if report.collector.as_str() != "linux.process" {
                    return report.clone();
                }
                let processes: Vec<usize> = report
                    .entities
                    .iter()
                    .enumerate()
                    .filter(|(_, e)| e.id.kind == EntityKind::Process)
                    .map(|(index, _)| index)
                    .collect();
                // Processes of one program are ranked together, so five
                // compiler processes are one place in the list, not five,
                // and every process of a kept program is kept.
                let name_id = MetricId::new("process.name").ok();
                let name_of = |index: usize| -> String {
                    let entity = &report.entities[index];
                    name_id
                        .as_ref()
                        .and_then(|id| entity.metrics.get(id))
                        .and_then(Observation::as_text)
                        .map(str::to_string)
                        .unwrap_or_else(|| format!("[{}]", entity.id.key))
                };
                let mut keep: BTreeSet<usize> = BTreeSet::new();
                for ids in RANKINGS {
                    let mut by_name: Vec<(String, f64, Vec<usize>)> = Vec::new();
                    for index in &processes {
                        let value = score(&report.entities[*index].metrics, ids);
                        if value <= 0.0 {
                            continue;
                        }
                        let name = name_of(*index);
                        match by_name.iter_mut().find(|(n, _, _)| *n == name) {
                            Some(entry) => {
                                entry.1 += value;
                                entry.2.push(*index);
                            }
                            None => by_name.push((name, value, vec![*index])),
                        }
                    }
                    by_name.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.2[0].cmp(&b.2[0])));
                    for (_, _, members) in by_name.into_iter().take(top) {
                        keep.extend(members);
                    }
                }
                let mut trimmed = report.clone();
                trimmed.entities = report
                    .entities
                    .iter()
                    .enumerate()
                    .filter(|(index, e)| e.id.kind != EntityKind::Process || keep.contains(index))
                    .map(|(_, e)| e.clone())
                    .collect();
                trimmed
            })
            .collect()
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use monitor_core::{CollectorId, Entity, EntityId, MetricSet, Timestamp};

        #[test]
        fn only_the_busiest_processes_of_each_resource_are_kept() {
            let at = Timestamp {
                unix_ms: 1,
                monotonic_ns: 1,
            };
            let mut report = CollectorReport::new(CollectorId::new("linux.process").unwrap(), at);
            for pid in 0..20u32 {
                let mut m = MetricSet::new();
                m.insert(
                    MetricId::new("process.cpu.utilization").unwrap(),
                    Observation::float(pid as f64),
                );
                m.insert(
                    MetricId::new("process.memory.resident").unwrap(),
                    Observation::unsigned(if pid == 0 { 1 << 30 } else { 0 }),
                );
                report.entities.push(Entity::new(
                    EntityId::new(EntityKind::Process, pid.to_string()),
                    m,
                ));
            }
            let other = CollectorReport::new(CollectorId::new("linux.cpu").unwrap(), at);
            let trimmed = trim_round(&[other.clone(), report], 3);
            assert_eq!(trimmed[0], other);
            let kept: Vec<&str> = trimmed[1]
                .entities
                .iter()
                .map(|e| e.id.key.as_str())
                .collect();
            assert_eq!(kept, vec!["0", "17", "18", "19"]);
        }
    }
}

pub use battery_use::{BatteryUse, BatteryUseTracker, ProcessEnergy};
pub use trim::trim_round;

#[cfg(test)]
mod tests {
    use super::*;
    use monitor_store::{RecoverySummary, StoreRecovery};

    fn mark() -> MonitorRequest {
        MonitorRequest::new(RequestBody::MarkIncident {
            note: Some("系統剛剛卡住".to_string()),
            window_before_seconds: 300,
            window_after_seconds: 120,
            about_pid: Some(4242),
        })
    }

    fn status() -> StatusDocument {
        StatusDocument {
            recording: true,
            service_started_at_unix_ms: 1_700_000_000_000,
            now_unix_ms: 1_700_000_060_000,
            rounds_collected: 60,
            sample_interval_ms: 1_000,
            retention: RetentionPolicy::default(),
            store: StoreStats::default(),
            recovered_truncated_bytes: 0,
            collectors: vec![WireCollector {
                collector: CollectorId::new("linux.cpu").unwrap(),
                health: CollectorHealth::Healthy,
                unavailable_metrics: vec!["cpu.temperature".to_string()],
            }],
            latest_round: None,
            battery_use: None,
        }
    }

    #[test]
    fn a_well_formed_request_survives_a_json_round_trip() {
        let document = mark().to_json().unwrap();
        assert_eq!(MonitorRequest::from_json(&document).unwrap(), mark());
    }

    #[test]
    fn a_status_reply_survives_a_json_round_trip() {
        let response = MonitorResponse::status(status());
        let document = response.to_json().unwrap();
        assert_eq!(MonitorResponse::from_json(&document).unwrap(), response);
    }

    #[test]
    fn an_event_survives_a_json_round_trip() {
        let event = MonitorEvent::new(EventBody::IncidentMarked { incident_id: 7 });
        let document = event.to_json().unwrap();
        assert_eq!(MonitorEvent::from_json(&document).unwrap(), event);
    }

    #[test]
    fn an_oversized_payload_is_refused_before_parsing() {
        let document = " ".repeat(MAX_REQUEST_BYTES + 1);
        assert!(matches!(
            MonitorRequest::from_json(&document),
            Err(IpcError::PayloadTooLarge { .. })
        ));
    }

    #[test]
    fn an_unknown_field_is_refused() {
        let document =
            r#"{"protocol_version":1,"body":{"request":"query_incidents"},"extra":true}"#;
        assert!(matches!(
            MonitorRequest::from_json(document),
            Err(IpcError::Malformed { .. })
        ));
    }

    #[test]
    fn an_unknown_request_is_refused_rather_than_ignored() {
        let document = r#"{"protocol_version":1,"body":{"request":"delete_everything"}}"#;
        assert!(matches!(
            MonitorRequest::from_json(document),
            Err(IpcError::Malformed { .. })
        ));
    }

    #[test]
    fn another_protocol_version_is_refused_rather_than_partly_trusted() {
        // A version 1 client, from before ticket 97.
        let document = r#"{"protocol_version":1,"body":{"request":"query_incidents"}}"#;
        assert_eq!(
            MonitorRequest::from_json(document),
            Err(IpcError::ProtocolVersion {
                found: 1,
                expected: PROTOCOL_VERSION
            })
        );
    }

    #[test]
    fn a_reply_from_another_protocol_version_is_refused() {
        let document = r#"{"protocol_version":9,"body":{"response":"rejected","error_key":"x"}}"#;
        assert_eq!(
            MonitorResponse::from_json(document),
            Err(IpcError::ProtocolVersion {
                found: 9,
                expected: PROTOCOL_VERSION
            })
        );
    }

    #[test]
    fn a_backwards_range_is_refused() {
        let request = MonitorRequest::new(RequestBody::QueryHistory {
            from_unix_ms: 5_000,
            to_unix_ms: 1_000,
            max_samples: 100,
        });
        assert_eq!(
            MonitorRequest::from_json(&request.to_json().unwrap()),
            Err(IpcError::InvalidRange {
                from_unix_ms: 5_000,
                to_unix_ms: 1_000
            })
        );
    }

    #[test]
    fn a_sample_limit_of_zero_or_beyond_the_cap_is_refused() {
        for requested in [0, MAX_SAMPLES_PER_REPLY + 1] {
            let request = MonitorRequest::new(RequestBody::QueryHistory {
                from_unix_ms: 0,
                to_unix_ms: 1,
                max_samples: requested,
            });
            assert_eq!(
                MonitorRequest::from_json(&request.to_json().unwrap()),
                Err(IpcError::InvalidSampleLimit {
                    requested,
                    limit: MAX_SAMPLES_PER_REPLY
                })
            );
        }
    }

    #[test]
    fn a_note_longer_than_the_store_accepts_is_refused_at_the_protocol_edge() {
        let mut request = mark();
        let RequestBody::MarkIncident { note, .. } = &mut request.body else {
            unreachable!()
        };
        *note = Some("x".repeat(MAX_NOTE_LENGTH + 1));
        assert!(matches!(
            MonitorRequest::from_json(&request.to_json().unwrap()),
            Err(IpcError::NoteTooLong { .. })
        ));
    }

    #[test]
    fn a_note_carrying_a_control_character_is_refused() {
        let mut request = mark();
        let RequestBody::MarkIncident { note, .. } = &mut request.body else {
            unreachable!()
        };
        *note = Some("before\u{0}after".to_string());
        assert_eq!(
            MonitorRequest::from_json(&request.to_json().unwrap()),
            Err(IpcError::NoteControlCharacter)
        );
    }

    #[test]
    fn a_note_may_still_contain_a_line_break() {
        let mut request = mark();
        let RequestBody::MarkIncident { note, .. } = &mut request.body else {
            unreachable!()
        };
        *note = Some("froze\nwhile saving".to_string());
        assert!(MonitorRequest::from_json(&request.to_json().unwrap()).is_ok());
    }

    #[test]
    fn an_impossible_incident_window_is_refused() {
        for (before, after) in [(0, 60), (60, 0), (MAX_WINDOW_SECONDS + 1, 60)] {
            let request = MonitorRequest::new(RequestBody::MarkIncident {
                note: None,
                window_before_seconds: before,
                window_after_seconds: after,
                about_pid: None,
            });
            assert!(
                matches!(
                    MonitorRequest::from_json(&request.to_json().unwrap()),
                    Err(IpcError::InvalidWindow { .. })
                ),
                "expected {before}/{after} to be refused"
            );
        }
    }

    #[test]
    fn an_export_destination_must_be_an_absolute_path_without_traversal() {
        for destination in [
            "",
            "relative/path",
            "/home/tim/../../etc",
            "/tmp/with\u{0}nul",
        ] {
            let request = MonitorRequest::new(RequestBody::RequestExport {
                from_unix_ms: 0,
                to_unix_ms: 1,
                destination: destination.to_string(),
                include_processes: false,
                preview_only: false,
            });
            assert!(
                matches!(
                    MonitorRequest::from_json(&request.to_json().unwrap()),
                    Err(IpcError::InvalidDestination { .. })
                ),
                "expected {destination:?} to be refused"
            );
        }
    }

    #[test]
    fn an_absolute_destination_is_accepted() {
        let request = MonitorRequest::new(RequestBody::RequestExport {
            from_unix_ms: 0,
            to_unix_ms: 1,
            destination: "/home/tim/Desktop/monitor-export".to_string(),
            include_processes: true,
            preview_only: true,
        });
        assert!(MonitorRequest::from_json(&request.to_json().unwrap()).is_ok());
    }

    #[test]
    fn export_progress_zero_is_refused_because_no_export_carries_that_id() {
        let request = MonitorRequest::new(RequestBody::QueryExportProgress { export_id: 0 });
        assert_eq!(
            MonitorRequest::from_json(&request.to_json().unwrap()),
            Err(IpcError::UnknownExport { export_id: 0 })
        );
    }

    #[test]
    fn a_range_request_reports_the_interval_it_carries() {
        let request = MonitorRequest::new(RequestBody::QueryCoverage {
            from_unix_ms: 10,
            to_unix_ms: 20,
        });
        assert_eq!(
            request.range(),
            Some(TimeRange {
                from_unix_ms: 10,
                to_unix_ms: 20
            })
        );
        assert!(
            MonitorRequest::new(RequestBody::QueryIncidents)
                .range()
                .is_none()
        );
    }

    #[test]
    fn a_status_reply_can_carry_the_recovery_a_restart_had_to_perform() {
        let mut document = status();
        document.recovered_truncated_bytes = StoreRecovery {
            history: RecoverySummary {
                records: 10,
                truncated_bytes: 42,
            },
            ..StoreRecovery::default()
        }
        .history
        .truncated_bytes;
        let encoded = MonitorResponse::status(document.clone()).to_json().unwrap();
        let ResponseBody::Status(decoded) = MonitorResponse::from_json(&encoded).unwrap().body
        else {
            panic!("a status reply")
        };
        assert_eq!(decoded.recovered_truncated_bytes, 42);
    }

    #[test]
    fn the_transport_names_are_the_session_bus_ones_this_module_documents() {
        assert_eq!(BUS_NAME, "org.betteros.Monitor1");
        assert_eq!(OBJECT_PATH, "/org/betteros/Monitor1");
        assert_eq!(INTERFACE_NAME, BUS_NAME);
    }
}
