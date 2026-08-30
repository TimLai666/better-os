//! Better Awake's session and rule model.
//!
//! This crate is pure: it owns what a keep-awake session is, what an automatic
//! rule is, how several of them combine into one effective policy, and every
//! transition the service is allowed to make. It touches no bus, no file, and no
//! clock — the caller passes the current time in, so end-condition arithmetic
//! and schedule windows are testable without waiting for them to happen.
//!
//! Everything a user can influence is validated on the way in. A reason string
//! reaches an inhibitor backend and a menu, so it is length-limited and free of
//! control characters before it becomes a `Reason`. A rule condition is a closed
//! enum variant with validated operands, so there is no shape a shell command
//! could take even if something tried to build one.

mod evaluate;
mod policy;
mod rules;
mod session;
mod state;

pub use evaluate::{
    ActiveRule, Conflict, Evaluation, Observations, PolicyField, ProviderAvailability,
    RESOLUTION_EARLIEST_BATTERY_STOP, RESOLUTION_STRONGEST_WINS, RuleOutcome, RuleTest, Truth,
    WatchActivity, evaluate_condition, evaluate_group,
};
pub use policy::{
    ActiveReason, BackendCapabilities, EffectivePolicy, PolicyGap, SessionPolicy,
    merge_battery_threshold,
};
pub use rules::{
    Combine, Condition, ConditionGroup, DEFAULT_PRIORITY, InterfaceName, LocalTime,
    MAX_CONDITIONS_PER_GROUP, MAX_GROUPS_PER_RULE, MAX_INTERFACE_CHARS, MAX_MATCHER_CHARS,
    MAX_RULES, MAX_WATCH_WINDOW_SECONDS, MAX_WATCHED_PATH_CHARS, MINUTES_PER_DAY,
    PAUSE_LONG_SECONDS, PAUSE_SHORT_SECONDS, PauseState, ProcessMatchKind, ProcessMatcher,
    ProviderKind, Rule, RuleError, RuleId, RuleSet, Schedule, Suppression, WatchedPath, Weekday,
};
pub use session::{
    DEFAULT_BATTERY_STOP_PERCENT, EndCondition, EndConditionError, MAX_REASON_CHARS,
    MAX_SESSION_SECONDS, Reason, ReasonError, Remaining, Session, SessionChange, SessionId,
    SessionOrigin, SessionRequest,
};
pub use state::{
    AwakeState, BackendState, Command, Effect, EndCause, IndicatorState, MAX_ACTIVE_SESSIONS,
    TransitionError, TriggerSession,
};
