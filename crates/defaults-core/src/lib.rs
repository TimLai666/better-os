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

pub mod adapters;
pub mod engine;
pub mod plan;
pub mod status;

pub use adapters::{AdapterMode, AdapterSession, AdapterSessionError};
/// The installed application a restore offers instead of one that is gone. It
/// is named here so a surface that shows it need not depend on the adapters.
pub use defaults_platform::ApplicationChoice;
/// What a dconf refusal or note means to the person, `(en, zh_TW)`: which
/// database the session reads could not be decided, or a database it reads
/// is damaged. A surface shows this rather than "needs to be done by hand".
pub use defaults_platform::dconf_profile::sentence as dconf_sentence;
/// Why a setting reads as one this desktop does not have: no installed
/// GSettings schema defines its key.
pub use defaults_platform::gsettings::NO_SUCH_KEY as NO_SUCH_SETTING;
pub use engine::DefaultsEngine;
pub use plan::{
    Confirmations, DefaultsOutcome, DefaultsPlan, EntryOutcome, EntryResult, KeyOutcome,
    PLAN_SCHEMA_VERSION, PlanAction, PlanEntry, PlanKind, PlanWarning, RestoreChoice, Selection,
    SkipReason,
};
pub use status::{
    AggregateState, ComponentDefaults, ComponentReadiness, DefaultsReport, IntegrationState,
    IntegrationStatus, SystemContext,
};
