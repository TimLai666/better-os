//! The wire contract between the storage service and its clients.
//!
//! The transport is the **session** D-Bus, name `org.betteros.Storage1`. That
//! choice follows `manager-daemon`'s shape one level down: the manager daemon
//! is a system service because it changes the host, and this one is a session
//! service because it does not. Everything it does — enumerate through UDisks2,
//! read `/proc`, call `syncfs` on a mount the session already owns — is
//! available to the logged-in user, so the service runs unprivileged and there
//! is no polkit action to write. What would need privilege is recorded in
//! `docs/storage-safety-signals.md`; issue #5 defers that boundary to an ADR.
//!
//! Documents cross as JSON for the same reason ADR 0007 gives: one definition,
//! generated on both sides, rather than two hand-matched D-Bus signatures.
//!
//! [`StateReport`] is deliberately not [`storage_core::DeviceState`]. That type
//! carries a `ReadinessProof`, which has no `Deserialize` on purpose — a client
//! must not be able to hand the service a readiness claim it never earned. What
//! crosses the wire is a description of a proof, never a proof.

use serde::{Deserialize, Serialize};
use storage_core::state::{Blocker, DeviceState, UnknownStateReason, WritingReason};
use storage_core::{DeviceStateKind, RemovalPolicy};
use thiserror::Error;

/// Both sides must agree on this. A mismatch is refused rather than guessed at.
pub const PROTOCOL_VERSION: u32 = 1;

/// Largest request document accepted, checked before parsing.
pub const MAX_REQUEST_BYTES: usize = 64 * 1024;

#[derive(Debug, Error)]
pub enum ProtocolError {
    #[error("document is {bytes} bytes, over the {limit} byte limit")]
    PayloadTooLarge { bytes: usize, limit: usize },
    #[error("document is malformed: {detail}")]
    Malformed { detail: String },
    #[error("protocol version {found} is not {PROTOCOL_VERSION}")]
    VersionMismatch { found: u32 },
}

fn parse<T: for<'de> Deserialize<'de>>(document: &str) -> Result<T, ProtocolError> {
    if document.len() > MAX_REQUEST_BYTES {
        return Err(ProtocolError::PayloadTooLarge {
            bytes: document.len(),
            limit: MAX_REQUEST_BYTES,
        });
    }
    serde_json::from_str(document).map_err(|error| ProtocolError::Malformed {
        detail: error.to_string(),
    })
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockerReport {
    Process { pid: i32, name: Option<String> },
    Unidentified { detail: String },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UnsafeRemovalReport {
    pub at_millis: u64,
    pub previous_state: String,
    pub unfinished_operations: Vec<String>,
    pub detail: String,
    pub recommend_filesystem_check: bool,
}

/// A device state as a client sees it.
///
/// Every reason is a stable machine key. The wording a person reads is the
/// presentation layer's job, the same split `manager-core` uses for its errors.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "state")]
pub enum StateReport {
    ReadyToUnplug {
        proven_at_millis: u64,
        /// `None` when the volume is not mounted, where no flush was needed.
        flush_scope: Option<String>,
        /// Whether every signal behind the claim was authoritative for this
        /// device. False means the claim rests on a partial writer scan or on
        /// machine-wide writeback figures.
        fully_corroborated: bool,
        mounted: bool,
    },
    Writing {
        reason: String,
        detail: String,
    },
    Busy {
        blockers: Vec<BlockerReport>,
    },
    PerformanceMode {
        eject_required: bool,
        active_write: bool,
    },
    Unknown {
        reason: String,
        detail: String,
    },
    Disconnected {
        unsafe_removal: Option<UnsafeRemovalReport>,
    },
}

impl StateReport {
    pub fn kind(&self) -> DeviceStateKind {
        match self {
            StateReport::ReadyToUnplug { .. } => DeviceStateKind::ReadyToUnplug,
            StateReport::Writing { .. } => DeviceStateKind::Writing,
            StateReport::Busy { .. } => DeviceStateKind::Busy,
            StateReport::PerformanceMode { .. } => DeviceStateKind::PerformanceMode,
            StateReport::Unknown { .. } => DeviceStateKind::Unknown,
            StateReport::Disconnected { .. } => DeviceStateKind::Disconnected,
        }
    }

    pub fn permits_direct_removal(&self) -> bool {
        self.kind().permits_direct_removal()
    }

    pub fn from_state(state: &DeviceState) -> Self {
        match state {
            DeviceState::ReadyToUnplug(ready) => {
                let proof = ready.proof();
                StateReport::ReadyToUnplug {
                    proven_at_millis: proof.proven_at().as_duration().as_millis() as u64,
                    flush_scope: proof.flush().map(|flush| match flush.scope {
                        storage_core::FlushScope::Filesystem => "filesystem".to_string(),
                        storage_core::FlushScope::Device => "device".to_string(),
                    }),
                    fully_corroborated: proof.fully_corroborated(),
                    mounted: proof.mounted(),
                }
            }
            DeviceState::Writing(writing) => {
                let (reason, detail) = match &writing.reason {
                    WritingReason::TrackedOperation { operations } => {
                        ("storage.writing.tracked_operation", operations.join(", "))
                    }
                    WritingReason::PendingWriteback { pending } => (
                        "storage.writing.pending_writeback",
                        format!("{} bytes owed to the device", pending.bytes),
                    ),
                    WritingReason::FlushInProgress => {
                        ("storage.writing.flush_in_progress", String::new())
                    }
                };
                StateReport::Writing {
                    reason: reason.to_string(),
                    detail,
                }
            }
            DeviceState::Busy(busy) => StateReport::Busy {
                blockers: busy
                    .blockers
                    .iter()
                    .map(|blocker| match blocker {
                        Blocker::Process(writer) => BlockerReport::Process {
                            pid: writer.pid,
                            name: writer.name.clone(),
                        },
                        Blocker::Unidentified { detail } => BlockerReport::Unidentified {
                            detail: detail.clone(),
                        },
                    })
                    .collect(),
            },
            DeviceState::PerformanceMode(performance) => StateReport::PerformanceMode {
                eject_required: performance.eject_required,
                active_write: performance.active_write,
            },
            DeviceState::Unknown(unknown) => {
                let (reason, detail) = match &unknown.reason {
                    UnknownStateReason::NotYetObserved => {
                        ("storage.unknown.not_yet_observed", String::new())
                    }
                    UnknownStateReason::SignalUnsupported { signal, detail } => (
                        "storage.unknown.signal_unsupported",
                        format!("{}: {detail}", signal.as_str()),
                    ),
                    UnknownStateReason::SignalUnavailable { signal, detail } => (
                        "storage.unknown.signal_unavailable",
                        format!("{}: {detail}", signal.as_str()),
                    ),
                    UnknownStateReason::SignalPermissionDenied { signal, detail } => (
                        "storage.unknown.signal_permission_denied",
                        format!("{}: {detail}", signal.as_str()),
                    ),
                    UnknownStateReason::FlushFailed { detail } => {
                        ("storage.unknown.flush_failed", detail.clone())
                    }
                    UnknownStateReason::ServiceRestarted => {
                        ("storage.unknown.service_restarted", String::new())
                    }
                    UnknownStateReason::EvidenceStale => {
                        ("storage.unknown.evidence_stale", String::new())
                    }
                    UnknownStateReason::AmbiguousIdentity => {
                        ("storage.unknown.ambiguous_identity", String::new())
                    }
                    UnknownStateReason::FilesystemError { detail } => {
                        ("storage.unknown.filesystem_error", detail.clone())
                    }
                };
                StateReport::Unknown {
                    reason: reason.to_string(),
                    detail,
                }
            }
            DeviceState::Disconnected(disconnected) => StateReport::Disconnected {
                unsafe_removal: disconnected.unsafe_removal.as_ref().map(|record| {
                    UnsafeRemovalReport {
                        at_millis: record.at.as_duration().as_millis() as u64,
                        previous_state: record.previous_state.as_str().to_string(),
                        unfinished_operations: record.unfinished_operations.clone(),
                        detail: record.detail.clone(),
                        recommend_filesystem_check: record.recommend_filesystem_check,
                    }
                }),
            },
        }
    }
}

/// One device, as published to clients.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceReport {
    /// The handle a client passes back to act on this device.
    pub object_path: String,
    pub device_path: String,
    pub display_name: String,
    /// The stable identity key. Two rows with the same key are the same volume.
    pub identity: String,
    pub identity_confidence: String,
    pub filesystem: Option<String>,
    pub mount_point: Option<String>,
    pub policy: RemovalPolicy,
    pub state: StateReport,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceListDocument {
    pub protocol_version: u32,
    pub devices: Vec<DeviceReport>,
}

impl DeviceListDocument {
    pub fn new(devices: Vec<DeviceReport>) -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            devices,
        }
    }

    pub fn to_json(&self) -> Result<String, ProtocolError> {
        serde_json::to_string(self).map_err(|error| ProtocolError::Malformed {
            detail: error.to_string(),
        })
    }

    pub fn from_json(document: &str) -> Result<Self, ProtocolError> {
        let parsed: Self = parse(document)?;
        if parsed.protocol_version != PROTOCOL_VERSION {
            return Err(ProtocolError::VersionMismatch {
                found: parsed.protocol_version,
            });
        }
        Ok(parsed)
    }
}

/// A request to change a device's removal policy.
///
/// The acknowledged risk keys are part of the request because Performance mode
/// cannot be switched on without them, and the service will not accept a claim
/// that they were shown unless the client says which ones.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetPolicyRequest {
    pub protocol_version: u32,
    pub object_path: String,
    pub policy: RemovalPolicy,
    #[serde(default)]
    pub acknowledged_risks: Vec<String>,
}

impl SetPolicyRequest {
    pub fn direct_removal(object_path: impl Into<String>) -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            object_path: object_path.into(),
            policy: RemovalPolicy::DirectRemoval,
            acknowledged_risks: Vec::new(),
        }
    }

    pub fn performance(object_path: impl Into<String>, acknowledged_risks: Vec<String>) -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            object_path: object_path.into(),
            policy: RemovalPolicy::Performance,
            acknowledged_risks,
        }
    }

    pub fn to_json(&self) -> Result<String, ProtocolError> {
        serde_json::to_string(self).map_err(|error| ProtocolError::Malformed {
            detail: error.to_string(),
        })
    }

    pub fn from_json(document: &str) -> Result<Self, ProtocolError> {
        let parsed: Self = parse(document)?;
        if parsed.protocol_version != PROTOCOL_VERSION {
            return Err(ProtocolError::VersionMismatch {
                found: parsed.protocol_version,
            });
        }
        Ok(parsed)
    }
}

/// A file operation starting or finishing.
///
/// This is the surface Better Files and any future Better Copy call. It exists
/// now, typed, and is wired to the state machine; the file manager that will
/// call it is ticket 35's work.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperationNotice {
    pub protocol_version: u32,
    pub object_path: String,
    /// The client's own id for the operation. Completion must use the same one.
    pub operation: String,
}

impl OperationNotice {
    pub fn new(object_path: impl Into<String>, operation: impl Into<String>) -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            object_path: object_path.into(),
            operation: operation.into(),
        }
    }

    pub fn to_json(&self) -> Result<String, ProtocolError> {
        serde_json::to_string(self).map_err(|error| ProtocolError::Malformed {
            detail: error.to_string(),
        })
    }

    pub fn from_json(document: &str) -> Result<Self, ProtocolError> {
        let parsed: Self = parse(document)?;
        if parsed.protocol_version != PROTOCOL_VERSION {
            return Err(ProtocolError::VersionMismatch {
                found: parsed.protocol_version,
            });
        }
        Ok(parsed)
    }
}

/// What an eject attempt did.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EjectReport {
    pub protocol_version: u32,
    pub object_path: String,
    pub unmounted: bool,
    pub powered_off: bool,
    pub detail: String,
}

impl EjectReport {
    pub fn to_json(&self) -> Result<String, ProtocolError> {
        serde_json::to_string(self).map_err(|error| ProtocolError::Malformed {
            detail: error.to_string(),
        })
    }

    pub fn from_json(document: &str) -> Result<Self, ProtocolError> {
        parse(document)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use storage_core::Timestamp;
    use storage_core::evidence::{
        EvidencePolicy, FlushScope, FlushVerification, OpenWriters, PendingWriteback,
        ReadinessProof, SafetyEvidence, ScanCoverage, SignalStatus, TrackedOperations,
        WritebackScope,
    };
    use storage_core::state::ReadyToUnplug;

    fn ready_state() -> DeviceState {
        let evidence = SafetyEvidence {
            observed_at: Timestamp::from_millis(4242),
            mounted: true,
            flush: SignalStatus::Observed(FlushVerification {
                scope: FlushScope::Filesystem,
                completed_at: Timestamp::from_millis(4242),
            }),
            writeback: SignalStatus::Observed(PendingWriteback {
                bytes: 0,
                scope: WritebackScope::Device,
            }),
            open_writers: SignalStatus::Observed(OpenWriters {
                writers: Vec::new(),
                coverage: ScanCoverage::Complete,
            }),
            tracked_operations: TrackedOperations::new(),
        };
        let proof = ReadinessProof::from_evidence(&evidence, &EvidencePolicy::default()).unwrap();
        DeviceState::ReadyToUnplug(ReadyToUnplug::from_proof(proof))
    }

    #[test]
    fn a_readiness_claim_crosses_as_a_description_and_never_as_a_proof() {
        let report = StateReport::from_state(&ready_state());
        let document = serde_json::to_string(&report).unwrap();
        assert!(report.permits_direct_removal());
        assert!(document.contains("fully_corroborated"));
        // The description round-trips; the proof it describes does not exist on
        // the client side at all, which is the point.
        let decoded: StateReport = serde_json::from_str(&document).unwrap();
        assert_eq!(decoded, report);
    }

    #[test]
    fn every_state_has_a_distinct_report_and_only_one_permits_removal() {
        use std::collections::BTreeSet;
        use storage_core::evidence::ReadinessRefusal;
        use storage_core::state::{Disconnected, PerformanceMode};

        let states = [
            ready_state(),
            DeviceState::from_refusal(
                ReadinessRefusal::WritebackPending {
                    bytes: 1024,
                    scope: WritebackScope::Device,
                },
                Timestamp::START,
            ),
            DeviceState::from_refusal(
                ReadinessRefusal::WritersOpen {
                    writers: vec![storage_core::WriterIdentity { pid: 1, name: None }],
                },
                Timestamp::START,
            ),
            DeviceState::PerformanceMode(PerformanceMode::new(false)),
            DeviceState::from_refusal(ReadinessRefusal::FlushNotVerified, Timestamp::START),
            DeviceState::Disconnected(Disconnected {
                at: Timestamp::START,
                unsafe_removal: None,
            }),
        ];
        let reports: Vec<StateReport> = states.iter().map(StateReport::from_state).collect();
        let kinds: BTreeSet<_> = reports.iter().map(StateReport::kind).collect();
        assert_eq!(kinds.len(), 6);
        assert_eq!(
            reports
                .iter()
                .filter(|report| report.permits_direct_removal())
                .count(),
            1
        );
    }

    #[test]
    fn an_oversized_or_wrong_version_document_is_refused_before_it_is_trusted() {
        let oversized = format!(
            r#"{{"protocol_version":1,"object_path":"{}","policy":"direct_removal"}}"#,
            "a".repeat(MAX_REQUEST_BYTES)
        );
        assert!(matches!(
            SetPolicyRequest::from_json(&oversized),
            Err(ProtocolError::PayloadTooLarge { .. })
        ));

        let wrong_version =
            r#"{"protocol_version":99,"object_path":"/a","policy":"direct_removal"}"#;
        assert!(matches!(
            SetPolicyRequest::from_json(wrong_version),
            Err(ProtocolError::VersionMismatch { found: 99 })
        ));
    }

    #[test]
    fn an_unknown_field_is_rejected_rather_than_ignored() {
        let document =
            r#"{"protocol_version":1,"object_path":"/a","policy":"performance","surprise":true}"#;
        assert!(matches!(
            SetPolicyRequest::from_json(document),
            Err(ProtocolError::Malformed { .. })
        ));
    }

    #[test]
    fn requests_round_trip_through_their_wire_form() {
        let request = SetPolicyRequest::performance(
            "/org/freedesktop/UDisks2/block_devices/sdb1",
            vec!["storage.performance.eject_required".to_string()],
        );
        assert_eq!(
            SetPolicyRequest::from_json(&request.to_json().unwrap()).unwrap(),
            request
        );

        let notice = OperationNotice::new("/a", "copy-1");
        assert_eq!(
            OperationNotice::from_json(&notice.to_json().unwrap()).unwrap(),
            notice
        );
    }

    #[test]
    fn an_unsafe_removal_reaches_clients_with_its_recommendation_intact() {
        use storage_core::state::{Disconnected, UnsafeRemovalRecord};
        let state = DeviceState::Disconnected(Disconnected {
            at: Timestamp::from_millis(90),
            unsafe_removal: Some(UnsafeRemovalRecord {
                at: Timestamp::from_millis(90),
                previous_state: DeviceStateKind::Writing,
                unfinished_operations: vec!["copy-1".to_string()],
                detail: "the device was removed while writes were still outstanding".to_string(),
                recommend_filesystem_check: true,
            }),
        });
        let StateReport::Disconnected {
            unsafe_removal: Some(record),
        } = StateReport::from_state(&state)
        else {
            panic!("expected an unsafe removal report");
        };
        assert!(record.recommend_filesystem_check);
        assert_eq!(record.previous_state, "writing");
    }
}
