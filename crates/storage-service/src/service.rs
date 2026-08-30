//! The session D-Bus surface.
//!
//! Bus name `org.betteros.Storage1`, object path `/org/betteros/Storage1`, on
//! the **session** bus. Nothing here is privileged, so unlike
//! `org.betteros.Manager1` there is no polkit check on the way in: every
//! operation is one the logged-in user could perform directly, and UDisks2
//! applies its own authorization to the ones that touch a device.
//!
//! Documents cross as JSON, the same shape ADR 0007 chose for the manager, so
//! the contract lives in `protocol.rs` and both sides are generated from it.

use crate::coordinator::{ServiceError, StorageCoordinator};
use crate::protocol::{
    DeviceListDocument, EjectReport, OperationNotice, PROTOCOL_VERSION, SetPolicyRequest,
};
use std::sync::Arc;
use storage_core::DeviceHandle;
use storage_platform::traits::DeviceControl;
use tokio::sync::Mutex;
use zbus::object_server::SignalEmitter;
use zbus::{fdo, interface};

pub const BUS_NAME: &str = "org.betteros.Storage1";
pub const OBJECT_PATH: &str = "/org/betteros/Storage1";
pub const INTERFACE_NAME: &str = "org.betteros.Storage1";

fn refuse(error: ServiceError) -> fdo::Error {
    match error {
        ServiceError::UnknownDevice(path) => fdo::Error::UnknownObject(path),
        ServiceError::Policy(error) => fdo::Error::AccessDenied(error.to_string()),
        other => fdo::Error::Failed(other.to_string()),
    }
}

pub struct StorageService<C: DeviceControl + 'static> {
    coordinator: Arc<Mutex<StorageCoordinator<C>>>,
}

impl<C: DeviceControl + 'static> StorageService<C> {
    pub fn new(coordinator: Arc<Mutex<StorageCoordinator<C>>>) -> Self {
        Self { coordinator }
    }
}

#[interface(name = "org.betteros.Storage1")]
impl<C: DeviceControl + 'static> StorageService<C> {
    /// Every connected external device and its current state. A client that
    /// just started calls this once and then follows the signal.
    async fn list_devices(&self) -> Result<String, fdo::Error> {
        let coordinator = self.coordinator.lock().await;
        DeviceListDocument::new(coordinator.reports())
            .to_json()
            .map_err(|error| fdo::Error::Failed(error.to_string()))
    }

    /// Mount-on-open. Leaving the location later does not unmount it; that is
    /// the point of the direct-removal model.
    async fn mount(&self, object_path: &str) -> Result<String, fdo::Error> {
        let mut coordinator = self.coordinator.lock().await;
        coordinator
            .mount(&DeviceHandle::new(object_path))
            .await
            .map(|path| path.to_string_lossy().to_string())
            .map_err(refuse)
    }

    /// The explicit action. Still here, still required in Performance mode.
    async fn eject(&self, object_path: &str) -> Result<String, fdo::Error> {
        let mut coordinator = self.coordinator.lock().await;
        let outcome = coordinator
            .eject(&DeviceHandle::new(object_path))
            .await
            .map_err(refuse)?;
        EjectReport {
            protocol_version: PROTOCOL_VERSION,
            object_path: object_path.to_string(),
            unmounted: outcome.unmounted,
            powered_off: outcome.powered_off,
            detail: outcome.detail,
        }
        .to_json()
        .map_err(|error| fdo::Error::Failed(error.to_string()))
    }

    /// Changes a device's removal policy. Performance mode is refused unless
    /// the request lists every declared risk as acknowledged.
    async fn set_policy(&self, request_json: &str) -> Result<(), fdo::Error> {
        let request = SetPolicyRequest::from_json(request_json)
            .map_err(|error| fdo::Error::InvalidArgs(error.to_string()))?;
        let mut coordinator = self.coordinator.lock().await;
        coordinator
            .set_policy(
                &DeviceHandle::new(request.object_path),
                request.policy,
                request.acknowledged_risks,
            )
            .await
            .map_err(refuse)
    }

    /// Better Files, or a future Better Copy, telling the service a write has
    /// started. Until the matching completion arrives, the device cannot be
    /// reported ready.
    async fn notify_operation_started(&self, notice_json: &str) -> Result<(), fdo::Error> {
        let notice = OperationNotice::from_json(notice_json)
            .map_err(|error| fdo::Error::InvalidArgs(error.to_string()))?;
        let mut coordinator = self.coordinator.lock().await;
        coordinator
            .operation_started(&DeviceHandle::new(notice.object_path), notice.operation)
            .await;
        Ok(())
    }

    /// A write finished. This is where the filesystem-scoped flush happens:
    /// once per operation, not once per file.
    async fn notify_operation_completed(&self, notice_json: &str) -> Result<(), fdo::Error> {
        let notice = OperationNotice::from_json(notice_json)
            .map_err(|error| fdo::Error::InvalidArgs(error.to_string()))?;
        let mut coordinator = self.coordinator.lock().await;
        coordinator
            .operation_completed(&DeviceHandle::new(notice.object_path), notice.operation)
            .await;
        Ok(())
    }

    /// Re-reads the whole inventory. For a UDisks2 restart, not for a timer.
    async fn refresh(&self) -> Result<(), fdo::Error> {
        let mut coordinator = self.coordinator.lock().await;
        coordinator.refresh_inventory().await.map_err(refuse)
    }

    /// Returns every device to Direct Removal and reports what changed. This is
    /// the uninstall path.
    async fn restore_defaults(&self) -> Result<String, fdo::Error> {
        let mut coordinator = self.coordinator.lock().await;
        let plan = coordinator.restore_defaults().await.map_err(refuse)?;
        serde_json::to_string(&plan).map_err(|error| fdo::Error::Failed(error.to_string()))
    }

    #[zbus(signal)]
    async fn device_state_changed(
        emitter: &SignalEmitter<'_>,
        object_path: &str,
        state_json: &str,
    ) -> zbus::Result<()>;

    #[zbus(property)]
    async fn protocol_version(&self) -> u32 {
        PROTOCOL_VERSION
    }
}

/// Forwards state changes to the bus until the coordinator is dropped.
pub async fn publish_updates<C: DeviceControl + 'static>(
    connection: zbus::Connection,
    mut updates: tokio::sync::broadcast::Receiver<crate::protocol::DeviceReport>,
) -> zbus::Result<()> {
    let emitter = SignalEmitter::new(&connection, OBJECT_PATH)?;
    loop {
        match updates.recv().await {
            Ok(report) => {
                let Ok(document) = serde_json::to_string(&report) else {
                    continue;
                };
                let _ = StorageService::<C>::device_state_changed(
                    &emitter,
                    &report.object_path,
                    &document,
                )
                .await;
            }
            // A slow client missing updates is not a reason to stop
            // publishing: it can call ListDevices to resynchronize.
            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
            Err(tokio::sync::broadcast::error::RecvError::Closed) => return Ok(()),
        }
    }
}
