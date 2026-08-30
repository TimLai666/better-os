//! `better-storage-service` — the session-long owner of external-device state.
//!
//! Started by the user session, not by root. It connects to UDisks2 on the
//! system bus to watch and control devices, and owns `org.betteros.Storage1` on
//! the session bus for its own clients.

use std::sync::Arc;
use storage_platform::{LinuxFlush, LinuxWriteback, ProcOpenUse, Roots, UDisks2};
use storage_service::coordinator::Clock;
use storage_service::service::{BUS_NAME, OBJECT_PATH, publish_updates};
use storage_service::{PreferenceStore, StorageCoordinator, StorageService};
use tokio::sync::Mutex;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
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

    let connection = zbus::connection::Builder::session()?
        .name(BUS_NAME)?
        .serve_at(OBJECT_PATH, StorageService::new(coordinator.clone()))?
        .build()
        .await?;

    tokio::spawn(publish_updates::<UDisks2>(connection.clone(), updates));

    let pump = coordinator.clone();
    tokio::spawn(async move {
        while let Some(event) = receiver.recv().await {
            pump.lock().await.handle_event(event).await;
        }
    });

    tokio::signal::ctrl_c().await?;
    Ok(())
}
