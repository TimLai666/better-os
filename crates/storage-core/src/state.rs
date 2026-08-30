//! The five user-visible states, plus disconnected, as distinct types.
//!
//! They are separate structs rather than variants carrying a shared payload
//! because they promise different things. Only [`ReadyToUnplug`] can be built
//! from a [`ReadinessProof`], only [`Writing`] carries a reason data is moving,
//! and [`UnknownState`] exists so an unverifiable device has somewhere to be
//! that is not a reassuring green row.

use crate::evidence::{PendingWriteback, ReadinessProof, ReadinessRefusal, Signal, WriterIdentity};
use crate::time::Timestamp;
use serde::Serialize;

/// A flat discriminant for assertions, logging, and coverage reporting.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceStateKind {
    ReadyToUnplug,
    Writing,
    Busy,
    PerformanceMode,
    Unknown,
    Disconnected,
}

impl DeviceStateKind {
    /// Whether this state permits telling the user they may pull the cable.
    /// Exactly one kind does.
    pub fn permits_direct_removal(self) -> bool {
        matches!(self, DeviceStateKind::ReadyToUnplug)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            DeviceStateKind::ReadyToUnplug => "ready_to_unplug",
            DeviceStateKind::Writing => "writing",
            DeviceStateKind::Busy => "busy",
            DeviceStateKind::PerformanceMode => "performance_mode",
            DeviceStateKind::Unknown => "unknown",
            DeviceStateKind::Disconnected => "disconnected",
        }
    }
}

/// No known filesystem write is pending, no tracked operation is active, and a
/// flush was verified. It does not claim physical persistence; see
/// `docs/storage-safety-signals.md`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ReadyToUnplug {
    proof: ReadinessProof,
}

impl ReadyToUnplug {
    /// The only constructor, and it needs a proof. There is no other way in.
    pub fn from_proof(proof: ReadinessProof) -> Self {
        Self { proof }
    }

    pub fn proof(&self) -> &ReadinessProof {
        &self.proof
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WritingReason {
    /// A copy or write this system started and is tracking.
    TrackedOperation { operations: Vec<String> },
    /// Bytes the kernel still owes the device, whoever wrote them.
    PendingWriteback { pending: PendingWriteback },
    /// A flush was requested and has not reported back yet.
    FlushInProgress,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Writing {
    pub reason: WritingReason,
    pub since: Timestamp,
}

/// Something holds the volume in a way that makes removal unsafe or uncertain,
/// even though no write is visible.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Blocker {
    Process(WriterIdentity),
    /// A blocker that exists but could not be named — the honest answer when
    /// the scan saw a held mount it has no permission to attribute.
    Unidentified {
        detail: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Busy {
    pub blockers: Vec<Blocker>,
    pub since: Timestamp,
}

/// The user chose throughput over direct removal for this device.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct PerformanceMode {
    /// Always true. Present as a field so a surface reads the promise from the
    /// state rather than remembering the rule.
    pub eject_required: bool,
    /// Whether a write is currently visible. A device in Performance mode is
    /// still worth showing as busy, but it never becomes ready.
    pub active_write: bool,
}

impl PerformanceMode {
    pub fn new(active_write: bool) -> Self {
        Self {
            eject_required: true,
            active_write,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UnknownStateReason {
    /// Nothing has been observed about this device yet.
    NotYetObserved,
    /// A signal the readiness rule needs is missing on this host.
    SignalUnsupported {
        signal: Signal,
        detail: String,
    },
    SignalUnavailable {
        signal: Signal,
        detail: String,
    },
    SignalPermissionDenied {
        signal: Signal,
        detail: String,
    },
    /// A flush was attempted and failed. Nothing about the device is trustworthy
    /// until a later flush succeeds.
    FlushFailed {
        detail: String,
    },
    /// The coordinating service restarted and cannot vouch for what happened
    /// while it was gone.
    ServiceRestarted,
    /// The last proof is older than the policy allows.
    EvidenceStale,
    /// Two connected devices report the same identity.
    AmbiguousIdentity,
    /// The filesystem reported an error.
    FilesystemError {
        detail: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct UnknownState {
    pub reason: UnknownStateReason,
    pub since: Timestamp,
}

/// What was outstanding when a device vanished.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct UnsafeRemovalRecord {
    pub at: Timestamp,
    /// The state the device was in when it disappeared.
    pub previous_state: DeviceStateKind,
    /// Operations this system had not seen complete.
    pub unfinished_operations: Vec<String>,
    pub detail: String,
    /// Whether a filesystem check is worth recommending. True whenever the
    /// device left while writes were known or believed to be outstanding.
    pub recommend_filesystem_check: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Disconnected {
    pub at: Timestamp,
    /// `Some` only when the device left in a state that could not promise its
    /// data was written. A clean unplug records nothing.
    pub unsafe_removal: Option<UnsafeRemovalRecord>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "state")]
pub enum DeviceState {
    ReadyToUnplug(ReadyToUnplug),
    Writing(Writing),
    Busy(Busy),
    PerformanceMode(PerformanceMode),
    Unknown(UnknownState),
    Disconnected(Disconnected),
}

impl DeviceState {
    pub fn kind(&self) -> DeviceStateKind {
        match self {
            DeviceState::ReadyToUnplug(_) => DeviceStateKind::ReadyToUnplug,
            DeviceState::Writing(_) => DeviceStateKind::Writing,
            DeviceState::Busy(_) => DeviceStateKind::Busy,
            DeviceState::PerformanceMode(_) => DeviceStateKind::PerformanceMode,
            DeviceState::Unknown(_) => DeviceStateKind::Unknown,
            DeviceState::Disconnected(_) => DeviceStateKind::Disconnected,
        }
    }

    pub fn permits_direct_removal(&self) -> bool {
        self.kind().permits_direct_removal()
    }

    pub fn readiness_proof(&self) -> Option<&ReadinessProof> {
        match self {
            DeviceState::ReadyToUnplug(ready) => Some(ready.proof()),
            _ => None,
        }
    }

    /// Turns a refused readiness claim into the state that refusal implies.
    ///
    /// Pending bytes and this system's own unfinished copies mean Writing. A
    /// process holding the volume means Busy. Everything else — an unsupported
    /// interface, a signal that could not be read, a stale proof — means
    /// Unknown, because there is no honest way to narrow it further.
    pub fn from_refusal(refusal: ReadinessRefusal, at: Timestamp) -> Self {
        match refusal {
            ReadinessRefusal::OperationsInFlight { operations } => DeviceState::Writing(Writing {
                reason: WritingReason::TrackedOperation { operations },
                since: at,
            }),
            ReadinessRefusal::WritebackPending { bytes, scope } => DeviceState::Writing(Writing {
                reason: WritingReason::PendingWriteback {
                    pending: PendingWriteback { bytes, scope },
                },
                since: at,
            }),
            ReadinessRefusal::WritersOpen { writers } => DeviceState::Busy(Busy {
                blockers: writers.into_iter().map(Blocker::Process).collect(),
                since: at,
            }),
            ReadinessRefusal::WriterScanIncomplete {
                unreadable_processes,
            } => DeviceState::Busy(Busy {
                blockers: vec![Blocker::Unidentified {
                    detail: format!(
                        "{unreadable_processes} processes could not be inspected for open writers"
                    ),
                }],
                since: at,
            }),
            ReadinessRefusal::FlushNotVerified => DeviceState::Unknown(UnknownState {
                reason: UnknownStateReason::SignalUnavailable {
                    signal: Signal::Flush,
                    detail: "no flush has been verified since the last write".to_string(),
                },
                since: at,
            }),
            ReadinessRefusal::EvidenceStale { .. } => DeviceState::Unknown(UnknownState {
                reason: UnknownStateReason::EvidenceStale,
                since: at,
            }),
            ReadinessRefusal::SignalUnsupported { signal, detail } => {
                DeviceState::Unknown(UnknownState {
                    reason: UnknownStateReason::SignalUnsupported { signal, detail },
                    since: at,
                })
            }
            ReadinessRefusal::SignalUnavailable { signal, detail } => {
                DeviceState::Unknown(UnknownState {
                    reason: UnknownStateReason::SignalUnavailable { signal, detail },
                    since: at,
                })
            }
            ReadinessRefusal::SignalPermissionDenied { signal, detail } => {
                DeviceState::Unknown(UnknownState {
                    reason: UnknownStateReason::SignalPermissionDenied { signal, detail },
                    since: at,
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::evidence::WritebackScope;
    use std::collections::BTreeSet;
    use std::time::Duration;

    #[test]
    fn every_state_kind_is_distinct_and_only_one_permits_direct_removal() {
        let kinds = [
            DeviceStateKind::ReadyToUnplug,
            DeviceStateKind::Writing,
            DeviceStateKind::Busy,
            DeviceStateKind::PerformanceMode,
            DeviceStateKind::Unknown,
            DeviceStateKind::Disconnected,
        ];
        let unique: BTreeSet<_> = kinds.iter().copied().collect();
        assert_eq!(unique.len(), 6);
        assert_eq!(
            kinds
                .iter()
                .filter(|kind| kind.permits_direct_removal())
                .count(),
            1
        );
    }

    #[test]
    fn pending_bytes_read_as_writing_and_a_held_file_reads_as_busy() {
        let at = Timestamp::from_millis(1);
        assert_eq!(
            DeviceState::from_refusal(
                ReadinessRefusal::WritebackPending {
                    bytes: 8192,
                    scope: WritebackScope::Device
                },
                at
            )
            .kind(),
            DeviceStateKind::Writing
        );
        assert_eq!(
            DeviceState::from_refusal(
                ReadinessRefusal::WritersOpen {
                    writers: vec![WriterIdentity {
                        pid: 12,
                        name: None
                    }]
                },
                at
            )
            .kind(),
            DeviceStateKind::Busy
        );
    }

    #[test]
    fn an_unverifiable_signal_degrades_to_unknown_rather_than_to_a_softer_claim() {
        let at = Timestamp::from_millis(1);
        for refusal in [
            ReadinessRefusal::FlushNotVerified,
            ReadinessRefusal::EvidenceStale {
                age: Duration::from_secs(9000),
            },
            ReadinessRefusal::SignalUnsupported {
                signal: Signal::OpenWriters,
                detail: "no procfs".to_string(),
            },
            ReadinessRefusal::SignalPermissionDenied {
                signal: Signal::OpenWriters,
                detail: "/proc/1/fd".to_string(),
            },
        ] {
            let state = DeviceState::from_refusal(refusal, at);
            assert_eq!(state.kind(), DeviceStateKind::Unknown);
            assert!(!state.permits_direct_removal());
        }
    }

    #[test]
    fn an_unnameable_blocker_is_still_reported_as_a_blocker() {
        let state = DeviceState::from_refusal(
            ReadinessRefusal::WriterScanIncomplete {
                unreadable_processes: 3,
            },
            Timestamp::from_millis(1),
        );
        let DeviceState::Busy(busy) = &state else {
            panic!("expected busy, got {state:?}");
        };
        assert!(matches!(
            busy.blockers.as_slice(),
            [Blocker::Unidentified { .. }]
        ));
    }

    #[test]
    fn performance_mode_always_asks_for_eject() {
        assert!(PerformanceMode::new(false).eject_required);
        assert!(PerformanceMode::new(true).eject_required);
        assert!(
            !DeviceState::PerformanceMode(PerformanceMode::new(false)).permits_direct_removal()
        );
    }
}
