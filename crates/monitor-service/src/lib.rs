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
use std::time::Duration;

/// How long [`start_on_bus`] keeps asking for a store a window holds.
#[derive(Clone, Copy, Debug)]
pub struct StoreWait {
    pub retry: Duration,
    pub attempts: u32,
}

impl Default for StoreWait {
    /// Every 2 seconds for a minute.
    fn default() -> Self {
        Self {
            retry: Duration::from_secs(2),
            attempts: 30,
        }
    }
}

/// Open the store, serve the interface on `connection`, and own `bus_name`.
///
/// Normally the interface is served before the name is taken, so a client
/// that D-Bus activation queued behind the name is answered at once. When a
/// Better Monitor window collecting for itself holds the store, the name is
/// taken first instead: that is what tells the window the service is here,
/// so it lets go of the store, which is asked for again every
/// `wait.retry`. The interface is served once the store is open.
pub async fn start_on_bus(
    connection: &zbus::Connection,
    bus_name: &str,
    config: ServiceConfig,
    wait: StoreWait,
) -> Result<Arc<MonitorEngine>, Box<dyn std::error::Error + Send + Sync>> {
    let store_root = config.store_root.clone();
    let mut waited = 0u32;
    let mut named = false;
    let engine = loop {
        match MonitorEngine::start(config.clone()) {
            Ok(engine) => break engine,
            Err(monitor_store::StoreError::Locked { .. }) if waited < wait.attempts => {
                if !named {
                    eprintln!(
                        "better-monitor-service: {} is open in a Better Monitor window; \
                         taking the bus name so it hands over",
                        store_root.display()
                    );
                    connection.request_name(bus_name).await?;
                    named = true;
                }
                waited += 1;
                tokio::time::sleep(wait.retry).await;
            }
            Err(error) => return Err(error.into()),
        }
    };
    connection
        .object_server()
        .at(OBJECT_PATH, MonitorDbusService::new(engine.clone()))
        .await?;
    if !named {
        connection.request_name(bus_name).await?;
    }
    Ok(engine)
}

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
