//! The Better Monitor user-session service.
//!
//! It owns collection. The window and the CLI are clients that can be closed,
//! restarted, or killed without the machine stopping being observed, because
//! none of them ever held a collector in the first place.

pub mod client;
pub mod engine;
pub mod inventory;
pub mod service;

pub use client::{ClientError, MonitorClient};
pub use engine::{
    DEFAULT_AUDIT_INTERVAL_SECONDS, DEFAULT_SAMPLE_INTERVAL_MS, MonitorEngine, ServiceConfig,
    now_unix_ms,
};
pub use inventory::{AuditSources, ComponentVersions, SessionFacts};
pub use service::{BUS_NAME, INTERFACE_NAME, MonitorDbusService, OBJECT_PATH};

use std::sync::Arc;

/// Run the sampling loop until the returned handle is dropped or aborted.
///
/// A steady interval, not a busy loop: the task is parked on a timer between
/// rounds. A tick that fails to write is reported and the loop continues,
/// because a full disk must not end collection — the next compaction may well
/// make room, and a service that exited would leave the user with nothing.
pub fn spawn_sampling(engine: Arc<MonitorEngine>) -> tokio::task::JoinHandle<()> {
    let interval = engine.config().sample_interval;
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            ticker.tick().await;
            if let Err(error) = engine.tick().await {
                eprintln!("better-monitor-service: a sample could not be recorded: {error}");
            }
        }
    })
}
