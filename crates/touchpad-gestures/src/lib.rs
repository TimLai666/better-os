//! Better Touchpad's gestures: what they are, what they collide with, how they
//! are recognized, and what applying them would do.
//!
//! Nothing in this crate knows that GNOME Shell, Mutter, libinput, or a
//! compositor exists. It answers five questions and no others:
//!
//! - What is a gesture, and what values may it hold? ([`definition`])
//! - What is configured, and how does a stored file become the current shape?
//!   ([`config`], [`store`])
//! - What does the shipped Mac-style preset map? ([`preset`])
//! - Given a stream of contact-point frames, what happened? ([`recognizer`])
//! - Given a compositor's gesture events instead of frames, what happened?
//!   ([`ingest`], which feeds the same recognizer rather than being a second
//!   one)
//! - What would applying a preset change, what does it collide with, and what
//!   did applying it actually do? ([`conflict`], [`plan`])
//!
//! The recognizer is the part worth reading. It is deliberately business logic
//! rather than integration: frames in, events out, no clock, no device, no
//! session. That is what makes activation, cancellation, cooldown, thumb
//! detection, and every preset gesture testable by replaying a stream, and it
//! is why whichever backend [ADR
//! 0012](../../../docs/decisions/0012-touchpad-gesture-backend.md) eventually
//! chooses can be dropped in front of it without moving any of the decisions
//! into it.
//!
//! Invoking an action is `touchpad-session`'s job, and the actions themselves
//! are `better-actions`'. A gesture here carries a typed action and no string
//! a shell could run.

pub mod config;
pub mod conflict;
pub mod definition;
pub mod ingest;
pub mod plan;
pub mod preset;
pub mod profiles;
pub mod recognizer;
pub mod shortcut;
pub mod store;
pub mod suppression;

pub use config::{ConfigError, GESTURE_SCHEMA_VERSION, GestureConfig, PresetId};
pub use conflict::{
    BuiltInGesture, Conflict, ConflictResolution, GNOME_46_GESTURES, detect as detect_conflicts,
};
pub use definition::{
    AnimationProgress, ConflictState, ContactCount, Cooldown, Direction, GestureBackend,
    GestureDefinition, GestureError, GestureId, GestureShape, Threshold, VerificationRecord,
};
pub use ingest::{
    CompositorGesture, CompositorGestureKind, CompositorPhase, EventRecognizer, EventScale,
};
pub use plan::{
    AdapterFailures, ApplyReport, ApprovedGesturePlan, BindingOutcome, BuiltInOutcome, ChangeKind,
    PlanError, PlannedChange, PresetPlan, RestorePlan, RunState,
};
pub use preset::mac_style;
pub use profiles::{
    GestureProfiles, MAX_DEVICE_PROFILES, MAX_GESTURES_PER_PROFILE, PROFILE_SCHEMA_VERSION,
};
pub use recognizer::{
    ContactPoint, ContactRole, FrameHealth, GestureEvent, GestureEventKind, Recognizer,
    RecognizerScale, TouchFrame, synthetic,
};
pub use shortcut::{KnownShortcuts, ShortcutCheck};
pub use store::{GestureStore, GestureStoreError};
pub use suppression::{SuppressionEvent, SuppressionState};
