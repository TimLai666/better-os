//! The Better Monitor user-session service.
//!
//! Runs unprivileged, one per logged-in user, for as long as the session
//! lasts. Its user unit starts it with the session, and D-Bus activation starts
//! it if a client asks before that; either way it does not exit when idle:
//! recording what the machine was doing before a slowdown is only possible if
//! something was already watching, and that is the whole job.

use std::time::Duration;

use monitor_service::{BUS_NAME, ServiceConfig, StoreWait, spawn_sampling, start_on_bus};

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let config = ServiceConfig::system();
    let store_root = config.store_root.clone();

    // A window collecting for itself may hold the store while Better Monitor
    // is being switched on; the bus name tells it to let go, and the store is
    // waited for rather than given up on.
    let connection = zbus::connection::Builder::session()?.build().await?;
    let engine = match start_on_bus(&connection, BUS_NAME, config, StoreWait::default()).await {
        Ok(engine) => engine,
        Err(error) => {
            // Without a store there is no history to record, and starting
            // anyway would leave a service that samples into nothing while
            // reporting itself as recording.
            eprintln!(
                "better-monitor-service: cannot open the history store at {}: {error}",
                store_root.display()
            );
            return Err(error);
        }
    };

    let set_aside = engine
        .with_store(|store| store.stats().set_aside_records)
        .await;
    if set_aside > 0 {
        eprintln!(
            "better-monitor-service: {set_aside} history records this version cannot read \
             are kept beside the logs in {} (*.undecodable) and left out of the history",
            store_root.display()
        );
    }

    let recovered = engine.with_store(|store| store.recovery()).await;
    if recovered.recovered_anything() {
        eprintln!(
            "better-monitor-service: the previous run was interrupted mid-write; \
             {} bytes were recovered from the end of the history log and the hole is \
             recorded as a gap",
            recovered.history.truncated_bytes
        );
    }

    let sampling = spawn_sampling(engine.clone());

    // A session ending is a clean shutdown, and a clean shutdown flushes the
    // bucket in flight. Anything less loses the last few seconds before the
    // machine the user was about to ask about went away.
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = terminate.recv() => {}
    }

    sampling.abort();
    // A bounded wait: the flush is a handful of writes, and hanging here would
    // delay the whole session's logout.
    match tokio::time::timeout(Duration::from_secs(5), engine.shutdown()).await {
        Ok(Ok(())) => {}
        Ok(Err(error)) => eprintln!("better-monitor-service: the final flush failed: {error}"),
        Err(_) => eprintln!("better-monitor-service: the final flush did not finish in time"),
    }
    Ok(())
}
