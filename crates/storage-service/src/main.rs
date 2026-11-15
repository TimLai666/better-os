//! `better-storage-service` — the session-long owner of external-device state.
//!
//! Started by the user session, not by root. It connects to UDisks2 on the
//! system bus to watch and control devices, and owns `org.betteros.Storage1` on
//! the session bus for its own clients.

use std::sync::Arc;
use storage_platform::{LinuxFlush, LinuxWriteback, ProcOpenUse, Roots, UDisks2};
use storage_service::coordinator::Clock;
use storage_service::service::{BUS_NAME, serve};
use storage_service::{PreferenceStore, StorageCoordinator};
use tokio::sync::Mutex;

/// How long shutdown waits for work still running on the runtime's blocking
/// pool (a flush or an open-file scan) before the process leaves anyway.
const SHUTDOWN_GRACE: std::time::Duration = std::time::Duration::from_secs(2);

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    let result = runtime.block_on(run());
    // Dropping a runtime waits for every blocking task without limit, so a
    // stop request could leave the process alive; bound it instead.
    runtime.shutdown_timeout(SHUTDOWN_GRACE);
    result
}

/// Waits for SIGINT or SIGTERM. systemd stops the unit with SIGTERM, a
/// terminal with SIGINT; both end the service the same orderly way.
async fn stop_requested() -> std::io::Result<()> {
    use tokio::signal::unix::{SignalKind, signal};
    let mut interrupt = signal(SignalKind::interrupt())?;
    let mut terminate = signal(SignalKind::terminate())?;
    tokio::select! {
        _ = interrupt.recv() => {}
        _ = terminate.recv() => {}
    }
    Ok(())
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let udisks = UDisks2::connect().await?;
    let roots = Roots::system();

    let coordinator = StorageCoordinator::new(
        udisks.clone(),
        Arc::new(LinuxFlush),
        Arc::new(LinuxWriteback::new(roots.clone())),
        Arc::new(ProcOpenUse::new(roots)),
        PreferenceStore::from_default_path(),
        Clock::session(),
    )?;
    let updates = coordinator.subscribe();
    let coordinator = Arc::new(Mutex::new(coordinator));

    // Events first, then the inventory: a device that arrives during startup is
    // then queued rather than missed.
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    udisks.watch(sender).await?;
    coordinator.lock().await.refresh_inventory().await?;

    // Served, watched, and only then named; `serve` says why.
    let _connection = serve(
        zbus::connection::Builder::session()?,
        BUS_NAME,
        coordinator.clone(),
        updates,
    )
    .await?;

    let pump = coordinator.clone();
    tokio::spawn(async move {
        while let Some(event) = receiver.recv().await {
            pump.lock().await.handle_event(event).await;
        }
    });

    stop_requested().await?;
    Ok(())
}
