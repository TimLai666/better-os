//! The Better Manager privileged daemon.
//!
//! Started on demand by D-Bus activation, runs as root, and exits once it has
//! been idle for a while. It is never enabled as a permanently running service:
//! a package manager that is not managing packages has no reason to be alive.

use std::sync::Arc;
use std::time::Duration;

use better_core::self_update::{OnDisk, ProgramFile};
use manager_daemon::apt::AptGetDriver;
use manager_daemon::authorize::PolkitAuthorizer;
use manager_daemon::executor::Executor;
use manager_daemon::health::SystemHealthProbe;
use manager_daemon::host::SystemHostProbe;
use manager_daemon::service::{BUS_NAME, ManagerService, OBJECT_PATH, wait_until_idle_or_replaced};
use manager_daemon::store::{ArtifactStore, Journal};
use manager_daemon::{ARCHIVE_DIR, STATE_DIR};

/// How long the daemon waits with nothing running before exiting.
const IDLE_TIMEOUT: Duration = Duration::from_secs(120);

/// How long the daemon stays after its last transaction once an update has
/// replaced its program: enough for the caller to read the outcome.
const REPLACED_GRACE: Duration = Duration::from_secs(10);

/// How often a leaving daemon looks whether the calls inside have finished.
const DRAIN_POLL: Duration = Duration::from_millis(50);

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
    let service = ManagerService::new(
        PolkitAuthorizer::new(connection.clone()),
        executor,
        artifacts,
        journal,
    )
    // The daemon package's postinst restarts a running daemon after an
    // upgrade only while this file is absent (ticket 105). /run is emptied at
    // boot, and a file left by a daemon that died is removed here.
    .with_busy_marker(manager_daemon::BUSY_MARKER);

    let activity = service.activity();
    let gate = service.gate();
    connection.object_server().at(OBJECT_PATH, service).await?;
    connection.request_name(BUS_NAME).await?;

    // Nothing to do but wait to be called, and never exit in the middle of a
    // transaction or a repair. D-Bus activation starts us again on the next
    // request — as the new program, when an update replaced this one.
    let program = ProgramFile::current().ok();
    let replaced = move || {
        program
            .as_ref()
            .is_some_and(|program| program.on_disk() != OnDisk::Same)
    };
    wait_until_idle_or_replaced(activity.clone(), IDLE_TIMEOUT, REPLACED_GRACE, &replaced).await;
    // Leaving: close the door to new calls (clients retry and reach a new
    // daemon), wait for the calls already inside, and only then give the name
    // up. Releasing it first would make systemd stop this `Type=dbus` unit at
    // once, in the middle of whatever was still running.
    gate.close(DRAIN_POLL).await;
    let _ = connection.release_name(BUS_NAME).await;
    Ok(())
}
