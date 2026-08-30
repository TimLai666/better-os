//! Better Touchpad's decisions, with no display server, shell, or compositor
//! anywhere near them.
//!
//! This crate answers five questions and nothing else:
//!
//! - What settings exist, and what values are possible? ([`settings`],
//!   [`value`])
//! - What does the user's configuration say, and how does an older file become
//!   the current one? ([`config`], [`store`])
//! - What is current, pending, effective, and previous for each setting?
//!   ([`state`])
//! - What would applying or restoring do, and what did it actually do?
//!   ([`plan`])
//! - Is any of this working? ([`health`])
//!
//! It has no idea that GNOME, dconf, or libinput exist. A backend maps these
//! identities onto whatever it stores, and `docs/touchpad-sensitivity-mapping.md`
//! records the one mapping that ships and what it cannot promise.

pub mod backup;
pub mod config;
pub mod health;
pub mod plan;
pub mod settings;
pub mod state;
pub mod store;
pub mod value;

pub use backup::{BACKUP_SCHEMA_VERSION, Backup};
pub use config::{
    BackendSelection, CONFIG_SCHEMA_VERSION, ClickingConfig, ConfigError, DeviceSelection,
    PointerConfig, ScrollingConfig, TouchpadConfig,
};
pub use health::{HealthCheck, HealthFacts, HealthReport, HealthState};
pub use plan::{
    ApplyPlan, ApplyStep, RestorePlan, RestoreScope, RestoreStep, RunReport, RunState, SkipReason,
    SkippedStep, StepOutcome,
};
pub use settings::{
    Capabilities, Reading, Section, SessionEffect, SettingId, SettingValue, Support, ValueKind,
};
pub use state::{Inhibited, SettingState, TouchpadState};
pub use store::{StoreError, TouchpadStore};
pub use value::{AccelerationProfile, ClickMethod, ScrollFactor, Sensitivity, ValueError};
