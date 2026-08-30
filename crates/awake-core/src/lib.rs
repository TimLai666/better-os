//! Better Awake's session model.
//!
//! This crate is pure: it owns what a keep-awake session is, how several of
//! them combine into one effective policy, and every transition the service is
//! allowed to make. It touches no bus, no file, and no clock — the caller
//! passes the current time in, so end-condition arithmetic is testable without
//! waiting for it to happen.
//!
//! Everything a user can influence is validated on the way in. A reason string
//! reaches an inhibitor backend and a menu, so it is length-limited and free of
//! control characters before it becomes a `Reason`.

mod policy;
mod session;
mod state;

pub use policy::{
    ActiveReason, BackendCapabilities, EffectivePolicy, PolicyGap, SessionPolicy,
    merge_battery_threshold,
};
pub use session::{
    DEFAULT_BATTERY_STOP_PERCENT, EndCondition, EndConditionError, MAX_REASON_CHARS,
    MAX_SESSION_SECONDS, Reason, ReasonError, Remaining, Session, SessionChange, SessionId,
    SessionOrigin, SessionRequest,
};
pub use state::{
    AwakeState, BackendState, Command, Effect, EndCause, IndicatorState, MAX_ACTIVE_SESSIONS,
    TransitionError,
};
