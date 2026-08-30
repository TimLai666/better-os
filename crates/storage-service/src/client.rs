//! The one way a client reaches the storage service.
//!
//! Ticket 31 built the service and its protocol documents but no client: its
//! own tests spoke to it through a hand-built proxy over a private bus. Better
//! Files is the first real consumer, and a file manager that hand-rolls a proxy
//! is a second place that knows the bus name and a second opinion about what
//! "the service is not running" means. So the client lives here, beside the
//! service, the way `monitor-service` keeps `MonitorClient` beside its own.
//!
//! What this deliberately does not do is soften an absent service. A client
//! that cannot reach `org.betteros.Storage1` gets
//! [`ClientError::ServiceUnavailable`] and has to decide what to do about it.
//! For Better Files that decision is running the same state machine in-process
//! and saying so; it is not showing a green light with nothing behind it.

use thiserror::Error;

use crate::protocol::{
    DeviceListDocument, DeviceReport, EjectReport, OperationNotice, ProtocolError, SetPolicyRequest,
};
use storage_core::{RemovalPolicy, RestoreDefaultPlan};

#[zbus::proxy(
    interface = "org.betteros.Storage1",
    default_service = "org.betteros.Storage1",
    default_path = "/org/betteros/Storage1"
)]
pub trait Storage {
    fn list_devices(&self) -> zbus::Result<String>;
    fn mount(&self, object_path: &str) -> zbus::Result<String>;
    fn eject(&self, object_path: &str) -> zbus::Result<String>;
    fn set_policy(&self, request_json: &str) -> zbus::Result<()>;
    fn notify_operation_started(&self, notice_json: &str) -> zbus::Result<()>;
    fn notify_operation_completed(&self, notice_json: &str) -> zbus::Result<()>;
    fn refresh(&self) -> zbus::Result<()>;
    fn restore_defaults(&self) -> zbus::Result<String>;

    #[zbus(signal)]
    fn device_state_changed(&self, object_path: String, state_json: String) -> zbus::Result<()>;

    #[zbus(property)]
    fn protocol_version(&self) -> zbus::Result<u32>;
}

#[derive(Clone, Debug, Eq, PartialEq, Error)]
pub enum ClientError {
    /// Not on the bus, or on the bus and not answering. Either way the caller
    /// has no device states and must not invent any.
    #[error("storage.client.error.service_unavailable:{0}")]
    ServiceUnavailable(String),
    #[error("storage.client.error.transport:{0}")]
    Transport(String),
    #[error("storage.client.error.protocol:{0}")]
    Protocol(String),
    /// The service answered with a protocol version this build does not speak.
    /// Not softened into "unavailable": a version mismatch is a different
    /// problem with a different fix.
    #[error("storage.client.error.protocol_version:{found}:{expected}")]
    ProtocolVersion { found: u32, expected: u32 },
    /// The service refused and said why.
    #[error("storage.client.error.rejected:{0}")]
    Rejected(String),
}

impl From<ProtocolError> for ClientError {
    fn from(error: ProtocolError) -> Self {
        ClientError::Protocol(error.to_string())
    }
}

/// A connected client.
pub struct StorageClient {
    proxy: StorageProxy<'static>,
}

impl StorageClient {
    /// Connects over the session bus.
    pub async fn connect() -> Result<Self, ClientError> {
        let connection = zbus::Connection::session()
            .await
            .map_err(|error| ClientError::Transport(error.to_string()))?;
        Self::with_connection(connection).await
    }

    pub async fn with_connection(connection: zbus::Connection) -> Result<Self, ClientError> {
        let proxy = StorageProxy::new(&connection)
            .await
            .map_err(|error| ClientError::ServiceUnavailable(error.to_string()))?;
        Ok(Self { proxy })
    }

    /// Points at a service published under another bus name, for the
    /// private-bus tests where the well-known name belongs to the developer's
    /// real session.
    pub async fn with_destination(
        connection: zbus::Connection,
        destination: String,
    ) -> Result<Self, ClientError> {
        let proxy = StorageProxy::builder(&connection)
            .destination(destination)
            .map_err(|error| ClientError::Transport(error.to_string()))?
            .build()
            .await
            .map_err(|error| ClientError::ServiceUnavailable(error.to_string()))?;
        Ok(Self { proxy })
    }

    /// Reads the version property, which is also the proof that the service is
    /// answering rather than merely having had a proxy built for it. Building a
    /// proxy for an absent name succeeds; this is the call that does not.
    pub async fn protocol_version(&self) -> Result<u32, ClientError> {
        self.proxy
            .protocol_version()
            .await
            .map_err(|error| ClientError::ServiceUnavailable(error.to_string()))
    }

    /// Connects and proves the service speaks this build's protocol.
    ///
    /// This is what a consumer that has a fallback should call, because the two
    /// failure modes it separates need different answers: an absent service is
    /// something to run in-process for, and a mismatched one is something to
    /// report.
    pub async fn connect_verified() -> Result<Self, ClientError> {
        let client = Self::connect().await?;
        client.verify().await?;
        Ok(client)
    }

    pub async fn verify(&self) -> Result<(), ClientError> {
        let found = self.protocol_version().await?;
        if found != crate::protocol::PROTOCOL_VERSION {
            return Err(ClientError::ProtocolVersion {
                found,
                expected: crate::protocol::PROTOCOL_VERSION,
            });
        }
        Ok(())
    }

    /// Every device the service knows, with its current state.
    pub async fn list_devices(&self) -> Result<Vec<DeviceReport>, ClientError> {
        let json = self.proxy.list_devices().await.map_err(map_call_error)?;
        Ok(DeviceListDocument::from_json(&json)?.devices)
    }

    /// Mounts a device and returns its mount point. This is what clicking an
    /// unmounted device in the sidebar does.
    pub async fn mount(&self, object_path: &str) -> Result<String, ClientError> {
        self.proxy.mount(object_path).await.map_err(map_call_error)
    }

    /// Ejects. The report says what actually happened — an unmount that
    /// succeeded with a power-off that was unavailable is not a clean eject and
    /// does not claim to be.
    pub async fn eject(&self, object_path: &str) -> Result<EjectReport, ClientError> {
        let json = self
            .proxy
            .eject(object_path)
            .await
            .map_err(map_call_error)?;
        Ok(EjectReport::from_json(&json)?)
    }

    pub async fn set_policy(
        &self,
        object_path: &str,
        policy: RemovalPolicy,
        acknowledged_risks: Vec<String>,
    ) -> Result<(), ClientError> {
        let request = match policy {
            RemovalPolicy::DirectRemoval => SetPolicyRequest::direct_removal(object_path),
            RemovalPolicy::Performance => {
                SetPolicyRequest::performance(object_path, acknowledged_risks)
            }
        };
        let json = request.to_json()?;
        self.proxy.set_policy(&json).await.map_err(map_call_error)
    }

    /// Tells the service a write is starting, so readiness is refused while it
    /// runs. Issue #6 requires the file-operation engine to do this; the client
    /// is how it reaches the service.
    pub async fn notify_operation_started(
        &self,
        object_path: &str,
        operation: &str,
    ) -> Result<(), ClientError> {
        let json = OperationNotice::new(object_path, operation).to_json()?;
        self.proxy
            .notify_operation_started(&json)
            .await
            .map_err(map_call_error)
    }

    /// Tells the service a write finished, which is what triggers the
    /// filesystem-scoped flush that readiness rests on.
    pub async fn notify_operation_completed(
        &self,
        object_path: &str,
        operation: &str,
    ) -> Result<(), ClientError> {
        let json = OperationNotice::new(object_path, operation).to_json()?;
        self.proxy
            .notify_operation_completed(&json)
            .await
            .map_err(map_call_error)
    }

    pub async fn refresh(&self) -> Result<(), ClientError> {
        self.proxy.refresh().await.map_err(map_call_error)
    }

    pub async fn restore_defaults(&self) -> Result<RestoreDefaultPlan, ClientError> {
        let json = self
            .proxy
            .restore_defaults()
            .await
            .map_err(map_call_error)?;
        serde_json::from_str(&json).map_err(|error| ClientError::Protocol(error.to_string()))
    }

    /// The state-change signal, as a stream. A consumer that subscribes does
    /// not poll, which is the whole reason the service emits it.
    pub async fn state_changes(&self) -> Result<DeviceStateChangedStream, ClientError> {
        self.proxy
            .receive_device_state_changed()
            .await
            .map_err(|error| ClientError::Transport(error.to_string()))
    }
}

pub use DeviceStateChangedStream as StateChanges;

/// Separates "the name is gone" from "the call was refused".
///
/// Both arrive as a `zbus::Error`, and treating them the same would make a
/// refused eject look like a service that had died.
fn map_call_error(error: zbus::Error) -> ClientError {
    match &error {
        zbus::Error::MethodError(name, detail, _) => {
            let name = name.as_str();
            if name.ends_with("ServiceUnknown") || name.ends_with("NoReply") {
                ClientError::ServiceUnavailable(error.to_string())
            } else {
                ClientError::Rejected(detail.clone().unwrap_or_else(|| name.to_string()))
            }
        }
        zbus::Error::InterfaceNotFound | zbus::Error::NameTaken => {
            ClientError::ServiceUnavailable(error.to_string())
        }
        _ => ClientError::Transport(error.to_string()),
    }
}
