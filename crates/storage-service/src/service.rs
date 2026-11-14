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
        ServiceError::Refused(reason) => fdo::Error::AccessDenied(reason),
        other => fdo::Error::Failed(other.to_string()),
    }
}

pub struct StorageService<C: DeviceControl + Clone + 'static> {
    coordinator: Arc<Mutex<StorageCoordinator<C>>>,
}

/// Sends on the channel when dropped, however the thread holding it ends.
struct DropSend(Option<tokio::sync::oneshot::Sender<()>>);

impl Drop for DropSend {
    fn drop(&mut self) {
        if let Some(sender) = self.0.take() {
            let _ = sender.send(());
        }
    }
}

/// How often an image write's progress is signalled, at most.
const PROGRESS_INTERVAL: std::time::Duration = std::time::Duration::from_millis(250);

impl<C: DeviceControl + Clone + 'static> StorageService<C> {
    pub fn new(coordinator: Arc<Mutex<StorageCoordinator<C>>>) -> Self {
        Self { coordinator }
    }
}

#[interface(name = "org.betteros.Storage1")]
impl<C: DeviceControl + Clone + 'static> StorageService<C> {
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

    /// Erases a volume and makes a new filesystem on it. Refused unless the
    /// request says the person confirmed erasing it.
    async fn format(&self, request_json: &str) -> Result<(), fdo::Error> {
        let request = crate::protocol::FormatDeviceRequest::from_json(request_json)
            .map_err(|error| fdo::Error::InvalidArgs(error.to_string()))?;
        let format = storage_platform::FormatRequest {
            kind: request.filesystem,
            label: request.label,
        };
        let mut coordinator = self.coordinator.lock().await;
        coordinator
            .format(
                &DeviceHandle::new(&request.object_path),
                &format,
                request.erase_confirmed,
            )
            .await
            .map_err(refuse)
    }

    /// Writes a disk image to a whole drive. Returns the bytes written.
    /// Refused unless confirmed. The checks and the unmounting run under
    /// the coordinator; the copy does not, so every other device (and every
    /// other request) is served while it runs, and the drive itself refuses
    /// everything until it ends. `ImageProgress` signals say how far it is.
    async fn write_image(
        &self,
        request_json: &str,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> Result<u64, fdo::Error> {
        let request = crate::protocol::WriteImageRequest::from_json(request_json)
            .map_err(|error| fdo::Error::InvalidArgs(error.to_string()))?;
        let (write, control) = {
            let mut coordinator = self.coordinator.lock().await;
            let write = coordinator
                .begin_image_write(
                    &DeviceHandle::new(&request.object_path),
                    std::path::Path::new(&request.image_path),
                    request.erase_confirmed,
                )
                .await
                .map_err(refuse)?;
            (write, coordinator.control().clone())
        };

        // Progress arrives on the copying thread; a thread of its own turns
        // it into signals, at most every PROGRESS_INTERVAL and always the
        // last one, and ends when the copy drops its sender.
        let (sender, written) = std::sync::mpsc::channel::<u64>();
        // Told when the signalling thread is done, awaited rather than
        // joined: this runs on zbus's executor, which must not block.
        let (signalled, signalling_done) = tokio::sync::oneshot::channel::<()>();
        {
            let emitter = emitter.to_owned();
            let object_path = request.object_path.clone();
            let total = write.total;
            std::thread::spawn(move || {
                let _done = DropSend(Some(signalled));
                // A small runtime of its own to send the signals on.
                let Ok(runtime) = tokio::runtime::Builder::new_current_thread().build() else {
                    // No runtime, no progress; the copy itself goes on.
                    while written.recv().is_ok() {}
                    return;
                };
                let mut last = None::<std::time::Instant>;
                let mut pending = None;
                loop {
                    let next = written.recv_timeout(PROGRESS_INTERVAL);
                    let ended =
                        matches!(next, Err(std::sync::mpsc::RecvTimeoutError::Disconnected));
                    if let Ok(bytes) = next {
                        pending = Some(bytes);
                    }
                    let due = last.is_none_or(|at| at.elapsed() >= PROGRESS_INTERVAL);
                    if let Some(bytes) = pending
                        && (due || ended)
                    {
                        let _ = runtime.block_on(Self::image_progress(
                            &emitter,
                            &object_path,
                            bytes,
                            total,
                        ));
                        last = Some(std::time::Instant::now());
                        pending = None;
                    }
                    if ended {
                        return;
                    }
                }
            });
        };
        let progress: storage_platform::ImageProgress = {
            let sender = std::sync::Mutex::new(sender);
            Arc::new(move |bytes| {
                if let Ok(sender) = sender.lock() {
                    let _ = sender.send(bytes);
                }
            })
        };
        let result = control
            .write_image(&write.address, &write.image, progress)
            .await;
        let _ = signalling_done.await;
        // The drive takes requests again. Were this call abandoned on the
        // way, dropping `write` would do the same.
        drop(write);
        result.map_err(|error| refuse(ServiceError::Platform(error)))
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
    /// started. Until the matching completion arrives, or the caller leaves
    /// the bus, the device cannot be reported ready.
    async fn notify_operation_started(
        &self,
        notice_json: &str,
        #[zbus(header)] header: zbus::message::Header<'_>,
        #[zbus(connection)] connection: &zbus::Connection,
    ) -> Result<(), fdo::Error> {
        let notice = OperationNotice::from_json(notice_json)
            .map_err(|error| fdo::Error::InvalidArgs(error.to_string()))?;
        let handle = DeviceHandle::new(notice.object_path);
        let Some(sender) = header.sender().map(|name| name.to_owned()) else {
            // A peer-to-peer connection has no bus to say when it leaves.
            let mut coordinator = self.coordinator.lock().await;
            coordinator
                .operation_started(&handle, notice.operation)
                .await;
            return Ok(());
        };
        self.coordinator
            .lock()
            .await
            .operation_started_by(sender.as_str(), &handle, notice.operation)
            .await;

        // The caller may already have gone, and its departure may have been
        // handled before this call reached the lock. Nothing would end the
        // operation then, so ask the bus now that it is recorded. A bus that
        // cannot answer is taken to mean the caller is still there, which is
        // the answer that never claims a device ready early.
        let still_here = match fdo::DBusProxy::new(connection).await {
            Ok(bus) => bus
                .name_has_owner(sender.as_ref().into())
                .await
                .unwrap_or(true),
            Err(_) => true,
        };
        if !still_here {
            self.coordinator
                .lock()
                .await
                .client_departed(sender.as_str())
                .await;
        }
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

    /// How far an image write is: bytes written of the image's length.
    #[zbus(signal)]
    async fn image_progress(
        emitter: &SignalEmitter<'_>,
        object_path: &str,
        written: u64,
        total: u64,
    ) -> zbus::Result<()>;

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

/// Subscribes to connections leaving the bus and returns the task that ends
/// every operation a departed client started and never completed.
///
/// The subscription is in place when this returns, so a service that calls it
/// before taking its well-known name cannot miss a client's departure. The
/// returned future is spawned by the caller on its own runtime.
pub async fn watch_departures<C: DeviceControl + Clone + 'static>(
    connection: &zbus::Connection,
    coordinator: Arc<Mutex<StorageCoordinator<C>>>,
) -> zbus::Result<impl std::future::Future<Output = ()> + Send + 'static> {
    let bus = fdo::DBusProxy::new(connection).await?;
    // Only a name losing its owner, which is how a connection closing looks.
    let mut departures = bus.receive_name_owner_changed_with_args(&[(2, "")]).await?;
    Ok(async move {
        use zbus::export::futures_core::Stream;
        while let Some(signal) =
            std::future::poll_fn(|context| std::pin::Pin::new(&mut departures).poll_next(context))
                .await
        {
            let Ok(args) = signal.args() else {
                continue;
            };
            // A unique name is the connection itself; a well-known name
            // changing hands says nothing about who started what.
            let zbus::names::BusName::Unique(name) = args.name() else {
                continue;
            };
            if args.new_owner().is_some() {
                continue;
            }
            coordinator
                .lock()
                .await
                .client_departed(name.as_str())
                .await;
        }
    })
}

/// Forwards state changes to the bus until the coordinator is dropped.
pub async fn publish_updates<C: DeviceControl + Clone + 'static>(
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
