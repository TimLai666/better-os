//! The Better Manager privileged daemon.
//!
//! Started on demand by D-Bus activation, runs as root, and exits once it has
//! been idle for a while. It is never enabled as a permanently running service:
//! a package manager that is not managing packages has no reason to be alive.

use std::sync::Arc;
use std::time::Duration;

use manager_daemon::apt::AptGetDriver;
use manager_daemon::authorize::PolkitAuthorizer;
use manager_daemon::dmi::SystemMemoryInventory;
use manager_daemon::executor::Executor;
use manager_daemon::health::SystemHealthProbe;
use manager_daemon::host::SystemHostProbe;
use manager_daemon::monitor_service::{
    BUS_NAME as MONITOR_BUS_NAME, MonitorService, OBJECT_PATH as MONITOR_OBJECT_PATH,
};
use manager_daemon::service::{
    BUS_NAME as MANAGER_BUS_NAME, ManagerService, OBJECT_PATH as MANAGER_OBJECT_PATH,
};
use manager_daemon::store::{ArtifactStore, Journal};
use manager_daemon::{ARCHIVE_DIR, STATE_DIR};

/// How long the daemon waits with nothing to do before exiting.
const IDLE_TIMEOUT: Duration = Duration::from_secs(120);

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let artifacts = Arc::new(ArtifactStore::new(ARCHIVE_DIR));
    let journal = Arc::new(Journal::new(STATE_DIR));

    // A transaction left mid-flight by a daemon that died is never resumed. An
    // interrupted dpkg run needs a person to look at it, and continuing would
    // be guessing at what state the packages are in.
    for interrupted in journal.interrupted()? {
        eprintln!(
            "better-manager-daemon: transaction {} was interrupted and needs manual recovery",
            interrupted.transaction_id
        );
    }

    let executor = Arc::new(Executor {
        apt: Arc::new(AptGetDriver),
        host: Arc::new(SystemHostProbe),
        health: Arc::new(SystemHealthProbe),
        artifacts: artifacts.clone(),
        journal: journal.clone(),
    });

    let connection = zbus::connection::Builder::system()?.build().await?;
    let manager_service = ManagerService::new(
        PolkitAuthorizer::new(connection.clone()),
        executor,
        artifacts,
        journal,
    );
    let monitor_service = MonitorService::new(
        PolkitAuthorizer::new(connection.clone()),
        Arc::new(SystemMemoryInventory),
    );

    connection
        .object_server()
        .at(MANAGER_OBJECT_PATH, manager_service)
        .await?;
    connection
        .object_server()
        .at(MONITOR_OBJECT_PATH, monitor_service)
        .await?;
    connection.request_name(MANAGER_BUS_NAME).await?;
    connection.request_name(MONITOR_BUS_NAME).await?;

    // Nothing to do but wait to be called. D-Bus activation starts us again on
    // the next request.
    tokio::time::sleep(IDLE_TIMEOUT).await;
    Ok(())
}
