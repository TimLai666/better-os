//! One device's state machine.
//!
//! Everything that changes what may be said about a device arrives here as an
//! event: mount, unmount, a signal observation, a file operation starting or
//! finishing, a flush completing or failing, a policy change, the service
//! restarting, and the cable being pulled. The machine holds no clock and does
//! no I/O, so a whole plug-write-flush-unplug sequence replays in microseconds
//! and every failure case is reachable in a test.

use crate::evidence::{
    EvidencePolicy, FlushOutcome, FlushVerification, OpenWriters, PendingWriteback, ReadinessProof,
    SafetyEvidence, SignalStatus, TrackedOperations, WritebackScope,
};
use crate::identity::{DeviceIdentity, IdentityKey};
use crate::policy::RemovalPolicy;
use crate::state::{
    DeviceState, DeviceStateKind, Disconnected, PerformanceMode, ReadyToUnplug, UnknownState,
    UnknownStateReason, UnsafeRemovalRecord, Writing, WritingReason,
};
use crate::time::Timestamp;
use serde::Serialize;

/// What the platform saw when it last looked at a device.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObservedSignals {
    pub at: Timestamp,
    pub mounted: bool,
    pub writeback: SignalStatus<PendingWriteback>,
    pub open_writers: SignalStatus<OpenWriters>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DeviceEvent {
    Mounted {
        mount_point: String,
    },
    Unmounted,
    SignalsObserved(ObservedSignals),
    OperationStarted {
        operation: String,
    },
    OperationCompleted {
        operation: String,
        flush: FlushOutcome,
    },
    FlushStarted,
    FlushCompleted(FlushVerification),
    FlushFailed {
        detail: String,
    },
    /// This filesystem or transport exposes no flush this code can verify.
    FlushUnsupported {
        detail: String,
    },
    FilesystemError {
        detail: String,
    },
    PolicyChanged(RemovalPolicy),
    /// The coordinating service came back and does not know what happened while
    /// it was gone.
    ServiceRestarted,
    Disconnected,
}

/// Work the machine needs the service to do. The machine never performs it.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Effect {
    /// Flush this volume only. Never a machine-wide `sync`.
    RequestFilesystemFlush,
    /// Re-read the writeback and open-writer signals.
    RequestSignalRefresh,
    /// Drop mount-derived state: navigation, sidebar rows, cached listings.
    ReleaseMountState,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticKind {
    /// A device left while writes were outstanding or unverifiable.
    UnsafeRemoval,
    FlushFailed,
    FilesystemError,
    ServiceRestartedMidState,
    AmbiguousIdentity,
}

/// A record worth keeping after the moment has passed.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Diagnostic {
    pub kind: DiagnosticKind,
    pub at: Timestamp,
    pub identity: IdentityKey,
    pub detail: String,
    pub unsafe_removal: Option<UnsafeRemovalRecord>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Transition {
    pub previous: DeviceStateKind,
    pub state: DeviceState,
    pub changed: bool,
    pub effects: Vec<Effect>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Debug)]
pub struct DeviceMachine {
    identity: DeviceIdentity,
    policy: RemovalPolicy,
    evidence_policy: EvidencePolicy,
    state: DeviceState,
    mounted: bool,
    mount_point: Option<String>,
    operations: TrackedOperations,
    flush: SignalStatus<FlushVerification>,
    writeback: SignalStatus<PendingWriteback>,
    open_writers: SignalStatus<OpenWriters>,
    flush_in_progress: bool,
    /// Whether anything has reported whether this volume is mounted. Before
    /// that, "not mounted" is an assumption, and the unmounted readiness path
    /// must not run on an assumption.
    mount_observed: bool,
    /// A condition that keeps the device unknown until something positive
    /// clears it, rather than until the next observation happens to look fine.
    sticky: Option<UnknownStateReason>,
    ambiguous: bool,
    connected: bool,
}

fn unavailable<T>(detail: &str) -> SignalStatus<T> {
    SignalStatus::Unavailable {
        detail: detail.to_string(),
    }
}

impl DeviceMachine {
    /// A newly detected device. It starts in Direct Removal — the default is
    /// not conditional on having seen the device before — and in Unknown,
    /// because nothing has been observed about it yet.
    pub fn connect(identity: DeviceIdentity, policy: RemovalPolicy, at: Timestamp) -> Self {
        let state = match policy {
            RemovalPolicy::Performance => DeviceState::PerformanceMode(PerformanceMode::new(false)),
            RemovalPolicy::DirectRemoval => DeviceState::Unknown(UnknownState {
                reason: UnknownStateReason::NotYetObserved,
                since: at,
            }),
        };
        Self {
            identity,
            policy,
            evidence_policy: EvidencePolicy::default(),
            state,
            mounted: false,
            mount_point: None,
            operations: TrackedOperations::new(),
            flush: unavailable("not observed yet"),
            writeback: unavailable("not observed yet"),
            open_writers: unavailable("not observed yet"),
            flush_in_progress: false,
            mount_observed: false,
            sticky: None,
            ambiguous: false,
            connected: true,
        }
    }

    pub fn with_evidence_policy(mut self, policy: EvidencePolicy) -> Self {
        self.evidence_policy = policy;
        self
    }

    pub fn identity(&self) -> &DeviceIdentity {
        &self.identity
    }

    pub fn policy(&self) -> RemovalPolicy {
        self.policy
    }

    pub fn state(&self) -> &DeviceState {
        &self.state
    }

    pub fn mount_point(&self) -> Option<&str> {
        self.mount_point.as_deref()
    }

    pub fn is_mounted(&self) -> bool {
        self.mounted
    }

    pub fn is_connected(&self) -> bool {
        self.connected
    }

    /// Marks the device as sharing its identity with another connected device.
    /// Set by the registry, because one machine cannot see the other.
    pub fn set_ambiguous(&mut self, ambiguous: bool, at: Timestamp) -> Transition {
        self.ambiguous = ambiguous;
        let mut diagnostics = Vec::new();
        if ambiguous {
            diagnostics.push(self.diagnostic(
                DiagnosticKind::AmbiguousIdentity,
                at,
                "another connected device reports the same identity",
            ));
        }
        self.settle(at, Vec::new(), diagnostics)
    }

    pub fn apply(&mut self, event: DeviceEvent, at: Timestamp) -> Transition {
        let mut effects = Vec::new();
        let mut diagnostics = Vec::new();

        match event {
            DeviceEvent::Mounted { mount_point } => {
                self.mounted = true;
                self.mount_observed = true;
                self.mount_point = Some(mount_point);
                // A fresh mount has nothing verified about it yet. Carrying the
                // previous volume's flush across would be the exact kind of
                // inherited reassurance this model exists to prevent.
                self.flush = unavailable("no flush verified since this mount");
                self.writeback = unavailable("not observed since this mount");
                self.open_writers = unavailable("not observed since this mount");
                effects.push(Effect::RequestSignalRefresh);
            }
            DeviceEvent::Unmounted => {
                self.mounted = false;
                self.mount_observed = true;
                self.mount_point = None;
                self.flush = unavailable("volume is not mounted");
                self.writeback = unavailable("volume is not mounted");
                self.open_writers = unavailable("volume is not mounted");
                self.flush_in_progress = false;
                self.sticky = None;
                effects.push(Effect::ReleaseMountState);
            }
            DeviceEvent::SignalsObserved(observed) => {
                self.mounted = observed.mounted;
                self.mount_observed = true;
                if !observed.mounted {
                    self.mount_point = None;
                }
                // Bytes still owed to the device mean a write landed after the
                // last flush, so that flush no longer proves anything.
                if let SignalStatus::Observed(pending) = &observed.writeback
                    && pending.bytes > 0
                    && pending.scope == WritebackScope::Device
                {
                    self.flush = unavailable("a write was observed after the last verified flush");
                }
                self.writeback = observed.writeback;
                self.open_writers = observed.open_writers;
                if self.sticky == Some(UnknownStateReason::ServiceRestarted) {
                    self.sticky = None;
                }
            }
            DeviceEvent::OperationStarted { operation } => {
                self.operations.start(operation);
                self.flush = unavailable("a write started after the last verified flush");
            }
            DeviceEvent::OperationCompleted { operation, flush } => {
                self.operations.complete(&operation);
                self.apply_flush_outcome(flush, at, &mut diagnostics);
                effects.push(Effect::RequestSignalRefresh);
            }
            DeviceEvent::FlushStarted => {
                self.flush_in_progress = true;
            }
            DeviceEvent::FlushCompleted(verification) => {
                self.apply_flush_outcome(
                    FlushOutcome::Completed(verification),
                    at,
                    &mut diagnostics,
                );
                effects.push(Effect::RequestSignalRefresh);
            }
            DeviceEvent::FlushFailed { detail } => {
                self.apply_flush_outcome(FlushOutcome::Failed { detail }, at, &mut diagnostics);
            }
            DeviceEvent::FlushUnsupported { detail } => {
                self.apply_flush_outcome(
                    FlushOutcome::Unsupported { detail },
                    at,
                    &mut diagnostics,
                );
            }
            DeviceEvent::FilesystemError { detail } => {
                self.sticky = Some(UnknownStateReason::FilesystemError {
                    detail: detail.clone(),
                });
                diagnostics.push(self.diagnostic(DiagnosticKind::FilesystemError, at, &detail));
            }
            DeviceEvent::PolicyChanged(policy) => {
                self.policy = policy;
            }
            DeviceEvent::ServiceRestarted => {
                // Whatever was true before the restart, this process did not
                // watch the gap. Everything observed goes back to unknown.
                self.flush = unavailable("the storage service restarted");
                self.writeback = unavailable("the storage service restarted");
                self.open_writers = unavailable("the storage service restarted");
                self.flush_in_progress = false;
                let previous = self.state.kind();
                self.sticky = Some(UnknownStateReason::ServiceRestarted);
                effects.push(Effect::RequestSignalRefresh);
                diagnostics.push(self.diagnostic(
                    DiagnosticKind::ServiceRestartedMidState,
                    at,
                    &format!("state was {} before the restart", previous.as_str()),
                ));
            }
            DeviceEvent::Disconnected => return self.disconnect(at),
        }

        self.settle(at, effects, diagnostics)
    }

    fn apply_flush_outcome(
        &mut self,
        outcome: FlushOutcome,
        at: Timestamp,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        self.flush_in_progress = false;
        match outcome {
            FlushOutcome::Completed(verification) => {
                self.flush = SignalStatus::Observed(verification);
                // A flush that succeeded supersedes an earlier failure and
                // closes a restart gap: the filesystem answered just now.
                if matches!(
                    self.sticky,
                    Some(
                        UnknownStateReason::FlushFailed { .. }
                            | UnknownStateReason::ServiceRestarted
                            | UnknownStateReason::FilesystemError { .. }
                    )
                ) {
                    self.sticky = None;
                }
            }
            FlushOutcome::Failed { detail } => {
                self.flush = unavailable("the last flush failed");
                self.sticky = Some(UnknownStateReason::FlushFailed {
                    detail: detail.clone(),
                });
                diagnostics.push(self.diagnostic(DiagnosticKind::FlushFailed, at, &detail));
            }
            FlushOutcome::Unsupported { detail } => {
                self.flush = SignalStatus::Unsupported { detail };
            }
        }
    }

    fn diagnostic(&self, kind: DiagnosticKind, at: Timestamp, detail: &str) -> Diagnostic {
        Diagnostic {
            kind,
            at,
            identity: self.identity.key().clone(),
            detail: detail.to_string(),
            unsafe_removal: None,
        }
    }

    fn disconnect(&mut self, at: Timestamp) -> Transition {
        let previous = self.state.kind();
        let unfinished: Vec<String> = self.operations.iter().map(str::to_string).collect();
        self.connected = false;

        // A device that left while it was provably idle, or while it was not
        // mounted at all, took nothing with it.
        let clean = matches!(
            previous,
            DeviceStateKind::ReadyToUnplug | DeviceStateKind::Disconnected
        ) && unfinished.is_empty();

        let record = if clean {
            None
        } else {
            let writes_outstanding = previous == DeviceStateKind::Writing || !unfinished.is_empty();
            Some(UnsafeRemovalRecord {
                at,
                previous_state: previous,
                unfinished_operations: unfinished.clone(),
                detail: if writes_outstanding {
                    "the device was removed while writes were still outstanding".to_string()
                } else {
                    format!(
                        "the device was removed while its state was {}, so completion could not be verified",
                        previous.as_str()
                    )
                },
                recommend_filesystem_check: writes_outstanding,
            })
        };

        let mut diagnostics = Vec::new();
        if let Some(record) = &record {
            diagnostics.push(Diagnostic {
                kind: DiagnosticKind::UnsafeRemoval,
                at,
                identity: self.identity.key().clone(),
                detail: record.detail.clone(),
                unsafe_removal: Some(record.clone()),
            });
        }

        self.mounted = false;
        self.mount_point = None;
        self.state = DeviceState::Disconnected(Disconnected {
            at,
            unsafe_removal: record,
        });

        Transition {
            previous,
            state: self.state.clone(),
            changed: previous != DeviceStateKind::Disconnected,
            effects: vec![Effect::ReleaseMountState],
            diagnostics,
        }
    }

    /// Recomputes the state from everything currently held, and reports what
    /// changed.
    fn settle(
        &mut self,
        at: Timestamp,
        mut effects: Vec<Effect>,
        diagnostics: Vec<Diagnostic>,
    ) -> Transition {
        let previous = self.state.kind();
        let next = self.evaluate(at, &mut effects);
        let changed = next.kind() != previous || next != self.state;
        self.state = next;
        effects.sort();
        effects.dedup();
        Transition {
            previous,
            state: self.state.clone(),
            changed,
            effects,
            diagnostics,
        }
    }

    fn evaluate(&self, at: Timestamp, effects: &mut Vec<Effect>) -> DeviceState {
        if !self.connected {
            return self.state.clone();
        }

        // Checked before the policy: if two connected devices claim the same
        // identity, the stored preference cannot be attributed to either of
        // them, so neither the Performance promise nor a readiness claim is
        // honest here.
        if self.ambiguous {
            return DeviceState::Unknown(UnknownState {
                reason: UnknownStateReason::AmbiguousIdentity,
                since: at,
            });
        }

        if self.policy == RemovalPolicy::Performance {
            // Performance mode never becomes ready, but a visible write is
            // still worth reporting.
            let active_write = !self.operations.is_empty()
                || self.flush_in_progress
                || matches!(
                    &self.writeback,
                    SignalStatus::Observed(PendingWriteback {
                        bytes,
                        scope: WritebackScope::Device
                    }) if *bytes > 0
                );
            return DeviceState::PerformanceMode(PerformanceMode::new(active_write));
        }

        // Writes in flight outrank a sticky failure: "data is moving" is more
        // useful and more urgent than "the last flush failed".
        if !self.operations.is_empty() {
            return DeviceState::Writing(Writing {
                reason: WritingReason::TrackedOperation {
                    operations: self.operations.iter().map(str::to_string).collect(),
                },
                since: at,
            });
        }
        if self.flush_in_progress {
            return DeviceState::Writing(Writing {
                reason: WritingReason::FlushInProgress,
                since: at,
            });
        }

        if !self.mount_observed {
            return DeviceState::Unknown(UnknownState {
                reason: UnknownStateReason::NotYetObserved,
                since: at,
            });
        }

        if let Some(reason) = &self.sticky {
            return DeviceState::Unknown(UnknownState {
                reason: reason.clone(),
                since: at,
            });
        }

        let evidence = SafetyEvidence {
            observed_at: at,
            mounted: self.mounted,
            flush: self.flush.clone(),
            writeback: self.writeback.clone(),
            open_writers: self.open_writers.clone(),
            tracked_operations: self.operations.clone(),
        };

        // The device is idle but unproven: ask for the narrow flush that would
        // let it be proven. This is the only place a flush is requested outside
        // a file operation, which is what keeps flushing off a per-write path.
        let device_writeback_pending = matches!(
            &self.writeback,
            SignalStatus::Observed(PendingWriteback { bytes, scope: WritebackScope::Device })
                if *bytes > 0
        );
        let flush_worth_asking_for = matches!(self.flush, SignalStatus::Unavailable { .. });
        if self.mounted && flush_worth_asking_for && !device_writeback_pending {
            effects.push(Effect::RequestFilesystemFlush);
        }

        match ReadinessProof::from_evidence(&evidence, &self.evidence_policy) {
            Ok(proof) => DeviceState::ReadyToUnplug(ReadyToUnplug::from_proof(proof)),
            Err(refusal) => DeviceState::from_refusal(refusal, at),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::evidence::{FlushScope, ScanCoverage};
    use crate::identity::{IdentityEvidence, Transport};

    fn identity() -> DeviceIdentity {
        DeviceIdentity::from_evidence(IdentityEvidence {
            filesystem_uuid: Some("A1B2-C3D4".to_string()),
            device_path: "/dev/sdb1".to_string(),
            transport: Transport::Usb,
            ..IdentityEvidence::default()
        })
    }

    fn machine() -> DeviceMachine {
        DeviceMachine::connect(identity(), RemovalPolicy::DirectRemoval, Timestamp::START)
    }

    fn idle_signals(at: u64) -> DeviceEvent {
        DeviceEvent::SignalsObserved(ObservedSignals {
            at: Timestamp::from_millis(at),
            mounted: true,
            writeback: SignalStatus::Observed(PendingWriteback {
                bytes: 0,
                scope: WritebackScope::Device,
            }),
            open_writers: SignalStatus::Observed(OpenWriters {
                writers: Vec::new(),
                coverage: ScanCoverage::Complete,
            }),
        })
    }

    fn flush_at(at: u64) -> DeviceEvent {
        DeviceEvent::FlushCompleted(FlushVerification {
            scope: FlushScope::Filesystem,
            completed_at: Timestamp::from_millis(at),
        })
    }

    #[test]
    fn a_device_nobody_has_looked_at_yet_is_unknown_and_not_ready() {
        let machine = machine();
        assert_eq!(machine.state().kind(), DeviceStateKind::Unknown);
        assert_eq!(machine.policy(), RemovalPolicy::DirectRemoval);
    }

    #[test]
    fn an_idle_mounted_volume_is_asked_for_a_flush_before_it_is_called_ready() {
        let mut machine = machine();
        machine.apply(
            DeviceEvent::Mounted {
                mount_point: "/run/media/user/DATA".to_string(),
            },
            Timestamp::from_millis(10),
        );
        let transition = machine.apply(idle_signals(20), Timestamp::from_millis(20));
        assert_eq!(transition.state.kind(), DeviceStateKind::Unknown);
        assert!(transition.effects.contains(&Effect::RequestFilesystemFlush));

        let transition = machine.apply(flush_at(30), Timestamp::from_millis(30));
        assert_eq!(transition.state.kind(), DeviceStateKind::ReadyToUnplug);
        assert!(
            transition
                .state
                .readiness_proof()
                .unwrap()
                .fully_corroborated()
        );
    }

    #[test]
    fn a_write_after_a_verified_flush_takes_the_readiness_claim_away() {
        let mut machine = machine();
        machine.apply(
            DeviceEvent::Mounted {
                mount_point: "/run/media/user/DATA".to_string(),
            },
            Timestamp::from_millis(10),
        );
        machine.apply(idle_signals(20), Timestamp::from_millis(20));
        machine.apply(flush_at(30), Timestamp::from_millis(30));
        assert_eq!(machine.state().kind(), DeviceStateKind::ReadyToUnplug);

        // A write from any process, seen only as bytes owed to the device.
        let transition = machine.apply(
            DeviceEvent::SignalsObserved(ObservedSignals {
                at: Timestamp::from_millis(40),
                mounted: true,
                writeback: SignalStatus::Observed(PendingWriteback {
                    bytes: 1 << 20,
                    scope: WritebackScope::Device,
                }),
                open_writers: SignalStatus::Observed(OpenWriters {
                    writers: Vec::new(),
                    coverage: ScanCoverage::Complete,
                }),
            }),
            Timestamp::from_millis(40),
        );
        assert_eq!(transition.state.kind(), DeviceStateKind::Writing);

        // Draining is not enough on its own; the flush has to be redone.
        let transition = machine.apply(idle_signals(50), Timestamp::from_millis(50));
        assert_eq!(transition.state.kind(), DeviceStateKind::Unknown);
        assert!(transition.effects.contains(&Effect::RequestFilesystemFlush));
    }

    #[test]
    fn performance_mode_never_reaches_ready_however_clean_the_evidence_is() {
        let mut machine = machine();
        machine.apply(
            DeviceEvent::Mounted {
                mount_point: "/run/media/user/DATA".to_string(),
            },
            Timestamp::from_millis(10),
        );
        machine.apply(
            DeviceEvent::PolicyChanged(RemovalPolicy::Performance),
            Timestamp::from_millis(11),
        );
        machine.apply(idle_signals(20), Timestamp::from_millis(20));
        let transition = machine.apply(flush_at(30), Timestamp::from_millis(30));
        assert_eq!(transition.state.kind(), DeviceStateKind::PerformanceMode);
        assert!(!transition.state.permits_direct_removal());
    }

    #[test]
    fn an_unmounted_device_is_ready_without_ever_being_flushed() {
        let mut machine = machine();
        machine.apply(
            DeviceEvent::Mounted {
                mount_point: "/run/media/user/DATA".to_string(),
            },
            Timestamp::from_millis(10),
        );
        // Mounted and unproven, so not ready.
        assert_eq!(
            machine
                .apply(idle_signals(20), Timestamp::from_millis(20))
                .state
                .kind(),
            DeviceStateKind::Unknown
        );

        let transition = machine.apply(DeviceEvent::Unmounted, Timestamp::from_millis(25));
        assert_eq!(transition.state.kind(), DeviceStateKind::ReadyToUnplug);
        assert!(transition.effects.contains(&Effect::ReleaseMountState));
        assert!(!transition.state.readiness_proof().unwrap().mounted());
    }
}
