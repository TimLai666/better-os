//! Better Defaults: what the desktop currently says, what Better OS wants it
//! to say, and how to move between the two without losing the difference.
//!
//! Everything a user-facing surface needs is here, and nothing that touches the
//! system is. The GUI and the CLI both build a [`DefaultsEngine`], hand it an
//! adapter set and a snapshot store, and review the same plan. There is no
//! second path for "just do it": the global action and a single component's
//! action are the same call with a different [`Selection`].
//!
//! The invariants worth naming:
//!
//! - Installing a component changes nothing here. Nothing in this crate runs
//!   unless a plan was built and executed on purpose.
//! - The effective value is read again immediately before a change. A value
//!   that no longer matches what Better Manager last wrote or verified is
//!   [`IntegrationState::ChangedExternally`] and is skipped until that exact
//!   entry is confirmed.
//! - Every change is verified by a second read, and an unverified write is its
//!   own outcome rather than a success.
//! - The previous value is captured before the first change, and restore writes
//!   that captured value — never a built-in application this crate guessed at.

pub mod engine;
pub mod plan;
pub mod status;

pub use engine::DefaultsEngine;
pub use plan::{
    Confirmations, DefaultsOutcome, DefaultsPlan, EntryOutcome, EntryResult, PLAN_SCHEMA_VERSION,
    PlanAction, PlanEntry, PlanKind, PlanWarning, Selection, SkipReason,
};
pub use status::{
    AggregateState, ComponentDefaults, ComponentReadiness, DefaultsReport, IntegrationState,
    IntegrationStatus, SystemContext,
};
