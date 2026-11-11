//! The Better Awake user-session service.
//!
//! Runs unprivileged, one per logged-in user, for as long as the session lasts.
//! Unlike the manager daemon it is not activated on demand and does not exit
//! when idle: it is holding a lock whose lifetime is the point.

use std::sync::Arc;
use std::time::Duration;

use awake_ipc::notification::Locale;
use awake_service::notify::LowBatteryNotifier;
use awake_service::service::{shutdown_and_announce, tick_and_announce};
use awake_service::{
    AwakeDbusService, AwakeEngine, BUS_NAME, LogindBackend, OBJECT_PATH, SystemClock,
    TICK_INTERVAL_SECONDS,
};
use awake_store::JsonStore;
use zbus::object_server::SignalEmitter;

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let backend = match LogindBackend::connect().await {
        Ok(backend) => backend,
        Err(error) => {
            // Without a backend there is no honest way to keep anything awake,
            // so this is said plainly rather than started in a state that would
            // accept sessions it cannot enforce.
            eprintln!("better-awake-service: no inhibitor backend: {error}");
            return Err(error.into());
        }
    };

    let store = JsonStore::from_default_path();
    let engine = Arc::new(AwakeEngine::start(backend, store, Arc::new(SystemClock)).await);

    let status = engine.status().await;
    if let Some(interrupted) = &status.interrupted_previous_session {
        eprintln!(
            "better-awake-service: the previous run ended without releasing its session ({}), \
             started at {} and last seen at {}",
            interrupted.reason,
            interrupted.started_at_unix_seconds,
            interrupted.last_seen_unix_seconds
        );
    }

    // Raises the low-battery notification when no tray is running to. Spawns
    // through this runtime, because a stop found inside a D-Bus handler is not
    // on a tokio task.
    let notifier = LowBatteryNotifier::new(
        tokio::runtime::Handle::current(),
        Locale::from_environment(),
    );

    let connection = zbus::connection::Builder::session()?.build().await?;
    connection
        .object_server()
        .at(
            OBJECT_PATH,
            AwakeDbusService::new(engine.clone()).with_notifier(notifier.clone()),
        )
        .await?;
    connection.request_name(BUS_NAME).await?;

    let emitter = SignalEmitter::new(&connection, OBJECT_PATH)?.into_owned();
    let ticking = engine.clone();
    let tick_emitter = emitter.clone();
    let ticker = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(TICK_INTERVAL_SECONDS));
        loop {
            interval.tick().await;

            // Every session a tick ends is pushed to the clients as
            // `SessionEnded`. A low-battery stop is raised as a desktop
            // notification by the tray, or by the service when no tray is
            // running, and is also said on stderr.
            for ended in tick_and_announce(&ticking, &tick_emitter, Some(&notifier)).await {
                if let Some(stop) = ended.battery_stop() {
                    eprintln!(
                        "better-awake-service: session {} ended because the battery reached {}%",
                        stop.session, stop.percent
                    );
                }
            }
        }
    });

    // A session ending is a clean shutdown, and a clean shutdown must release
    // every inhibitor. Anything less leaves the machine awake with nobody left
    // to explain why.
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = terminate.recv() => {}
    }

    ticker.abort();
    shutdown_and_announce(&engine, &emitter).await;
    Ok(())
}
