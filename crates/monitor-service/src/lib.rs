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

/// How long [`await_object_server`] waits for the object server in all.
const OBJECT_SERVER_DEADLINE: Duration = Duration::from_secs(10);

/// Starts `connection`'s object server and returns once it is dispatching.
///
/// zbus starts the server lazily: `Connection::object_server()` spawns a task
/// that subscribes to incoming method calls a moment later, and a call that
/// reaches the connection before that is dropped with no reply (ticket 107).
/// A connection built with `Builder::serve_at` does not have the gap, but this
/// one is handed in already built, and in the store-wait path the interface
/// cannot exist before the name is taken.
///
/// The server's root object answers `org.freedesktop.DBus.Peer.Ping`, and only
/// a server that is reading calls can answer, so the connection pings its own
/// unique name until it is answered. A ping sent too early is simply never
/// answered; each attempt waits twice as long as the last, up to a quarter of
/// a second, so a slow machine is not mistaken for a server that never
/// started.
async fn await_object_server(
    connection: &zbus::Connection,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let _ = connection.object_server();
    let Some(own_name) = connection.unique_name().map(ToString::to_string) else {
        // A peer-to-peer connection: there is no bus to lose a call on.
        return Ok(());
    };
    let started = std::time::Instant::now();
    let mut attempt = Duration::from_millis(5);
    while started.elapsed() < OBJECT_SERVER_DEADLINE {
        let ping = connection.call_method(
            Some(own_name.as_str()),
            "/",
            Some("org.freedesktop.DBus.Peer"),
            "Ping",
            &(),
        );
        match tokio::time::timeout(attempt, ping).await {
            Ok(Ok(_)) => return Ok(()),
            Ok(Err(error)) => return Err(error.into()),
            Err(_) => attempt = (attempt * 2).min(Duration::from_millis(250)),
        }
    }
    Err("the D-Bus object server did not start answering calls".into())
}

/// Open the store, serve the interface on `connection`, and own `bus_name`.
///
/// The connection's object server is running and dispatching before any name
/// is requested (see [`await_object_server`]), so a client that D-Bus
/// activation queued behind the name, or one that called the moment it saw
/// the name appear, is answered rather than left waiting for its timeout.
///
/// Normally the interface is served before the name is taken. When a Better
/// Monitor window collecting for itself holds the store, the name is taken
/// first instead: that is what tells the window the service is here, so it
/// lets go of the store, which is asked for again every `wait.retry`. The
/// interface is then added late, once the store is open. That late add is on
/// a server that is already dispatching, which is the reason it may come
/// after the name: a call made in between is refused at once as an unknown
/// object rather than dropped and left to time out.
pub async fn start_on_bus(
    connection: &zbus::Connection,
    bus_name: &str,
    config: ServiceConfig,
    wait: StoreWait,
) -> Result<Arc<MonitorEngine>, Box<dyn std::error::Error + Send + Sync>> {
    await_object_server(connection).await?;
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
