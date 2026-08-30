//! The Better Awake user-session service.
//!
//! It owns the inhibitor. The tray and the Status window are clients that can
//! be closed, restarted, or crash without the session they started ending,
//! because none of them ever held the lock in the first place.

pub mod backend;
pub mod engine;
pub mod logind;
pub mod rules;
pub mod service;

pub use backend::{
    BackendError, Clock, InhibitWhat, InhibitorBackend, LeaseHealth, LeaseRequest, SystemClock,
};
pub use engine::{AwakeEngine, BatteryStop, INHIBITOR_WHO};
pub use logind::LogindBackend;
pub use rules::{RuleDriver, RuleEdit};
pub use service::{AwakeDbusService, BUS_NAME, INTERFACE_NAME, OBJECT_PATH};

/// How often the service reaps expired sessions and re-checks that the
/// inhibitor it believes it holds is still held.
///
/// A countdown is not driven by this: clients compute remaining time from the
/// session's own start and end, so nothing busy-loops to make a menu tick.
pub const TICK_INTERVAL_SECONDS: u64 = 5;
