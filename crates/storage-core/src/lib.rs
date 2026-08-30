//! The external-storage safety model: identity, evidence, policy, and state.
//!
//! Nothing here knows about D-Bus, UDisks2, `/proc`, GPUI, or a shell command.
//! It is given events and evidence and decides what may honestly be said about
//! a device, which is the only reason the whole decision path can be tested
//! without unplugging a real disk mid-write.
//!
//! The invariant the rest of the system leans on is in [`ReadinessProof`]: a
//! [`ReadyToUnplug`](state::ReadyToUnplug) state cannot be built without one,
//! and a proof cannot be built without positive evidence. Missing, unsupported,
//! or unreadable signals do not average out into a reassuring answer; they
//! produce [`DeviceState::Unknown`](state::DeviceState::Unknown).

pub mod evidence;
pub mod identity;
pub mod machine;
pub mod policy;
pub mod preferences;
pub mod registry;
pub mod state;
pub mod time;

pub use evidence::{
    EvidencePolicy, FlushOutcome, FlushScope, FlushVerification, OpenWriters, PendingWriteback,
    ReadinessProof, ReadinessRefusal, SafetyEvidence, ScanCoverage, Signal, SignalStatus,
    TrackedOperations, WritebackScope, WriterIdentity,
};
pub use identity::{DeviceIdentity, IdentityConfidence, IdentityEvidence, IdentityKey, Transport};
pub use machine::{DeviceEvent, DeviceMachine, Diagnostic, DiagnosticKind, Effect, Transition};
pub use policy::{PERFORMANCE_RISK_KEYS, PerformanceOptIn, PolicyError, RemovalPolicy};
pub use preferences::{
    PREFERENCES_SCHEMA_VERSION, PreferenceRecord, PreferenceSet, RestoreDefaultPlan, RestoreEntry,
};
pub use registry::{DeviceHandle, DeviceRegistry};
pub use state::{
    Blocker, Busy, DeviceState, DeviceStateKind, Disconnected, PerformanceMode, ReadyToUnplug,
    UnknownState, UnknownStateReason, UnsafeRemovalRecord, Writing, WritingReason,
};
pub use time::Timestamp;
