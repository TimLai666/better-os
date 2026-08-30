//! The one way a client reaches the service.
//!
//! The GUI and the CLI both go through here, so there is one place that knows
//! the bus name, one place that decides what "the service is not running"
//! means, and no chance of the two disagreeing about it. A client that cannot
//! reach the service gets [`ClientError::ServiceUnavailable`] and is expected
//! to say so, not to quietly fall back to pretending it has data.

use monitor_ipc::{
    CoverageDocument, ExportDocument, HistoryDocument, IncidentWindowDocument, IncidentsDocument,
    InventoryDiffDocument, InventoryDocument, IpcError, MonitorRequest, MonitorResponse,
    RequestBody, ResponseBody, StatusDocument,
};
use thiserror::Error;

#[zbus::proxy(
    interface = "org.betteros.Monitor1",
    default_service = "org.betteros.Monitor1",
    default_path = "/org/betteros/Monitor1"
)]
pub trait Monitor {
    fn request(&self, request_json: &str) -> zbus::Result<String>;

    #[zbus(property)]
    fn protocol_version(&self) -> zbus::Result<u32>;

    #[zbus(property)]
    fn rounds_collected(&self) -> zbus::Result<u64>;
}

#[derive(Clone, Debug, Eq, PartialEq, Error)]
pub enum ClientError {
    /// The service is not on the bus. A client says so rather than drawing a
    /// History page whose emptiness would read as "nothing happened".
    #[error("monitor.client.error.service_unavailable:{0}")]
    ServiceUnavailable(String),
    #[error("monitor.client.error.transport:{0}")]
    Transport(String),
    #[error("monitor.client.error.protocol:{0}")]
    Protocol(String),
    /// The service refused the request and said why.
    #[error("monitor.client.error.rejected:{0}")]
    Rejected(String),
    /// The service answered, but with a different document than was asked for.
    #[error("monitor.client.error.unexpected_reply")]
    UnexpectedReply,
}

impl From<IpcError> for ClientError {
    fn from(error: IpcError) -> Self {
        ClientError::Protocol(error.to_string())
    }
}

pub struct MonitorClient {
    proxy: MonitorProxy<'static>,
}

impl MonitorClient {
    pub async fn connect() -> Result<Self, ClientError> {
        let connection = zbus::Connection::session()
            .await
            .map_err(|error| ClientError::Transport(error.to_string()))?;
        Self::with_connection(connection).await
    }

    pub async fn with_connection(connection: zbus::Connection) -> Result<Self, ClientError> {
        let proxy = MonitorProxy::new(&connection)
            .await
            .map_err(|error| ClientError::ServiceUnavailable(error.to_string()))?;
        Ok(Self { proxy })
    }

    /// Points at a service published under another bus name. Used by the
    /// private-bus integration test, where the well-known name belongs to the
    /// developer's real session.
    pub async fn with_destination(
        connection: zbus::Connection,
        destination: String,
    ) -> Result<Self, ClientError> {
        let proxy = MonitorProxy::builder(&connection)
            .destination(destination)
            .map_err(|error| ClientError::Transport(error.to_string()))?
            .build()
            .await
            .map_err(|error| ClientError::ServiceUnavailable(error.to_string()))?;
        Ok(Self { proxy })
    }

    /// Whether the service is actually answering, as opposed to merely having
    /// a proxy built for it.
    pub async fn protocol_version(&self) -> Result<u32, ClientError> {
        self.proxy
            .protocol_version()
            .await
            .map_err(|error| ClientError::ServiceUnavailable(error.to_string()))
    }

    pub async fn rounds_collected(&self) -> Result<u64, ClientError> {
        self.proxy
            .rounds_collected()
            .await
            .map_err(|error| ClientError::ServiceUnavailable(error.to_string()))
    }

    /// Send one request and unwrap the reply, or the refusal.
    pub async fn send(&self, request: MonitorRequest) -> Result<ResponseBody, ClientError> {
        let document = request.to_json()?;
        let reply = self
            .proxy
            .request(&document)
            .await
            .map_err(|error| ClientError::Transport(error.to_string()))?;
        match MonitorResponse::from_json(&reply)?.body {
            ResponseBody::Rejected { error_key } => Err(ClientError::Rejected(error_key)),
            body => Ok(body),
        }
    }

    pub async fn status(&self, include_latest_round: bool) -> Result<StatusDocument, ClientError> {
        match self
            .send(MonitorRequest::new(RequestBody::QueryStatus {
                include_latest_round,
            }))
            .await?
        {
            ResponseBody::Status(status) => Ok(*status),
            _ => Err(ClientError::UnexpectedReply),
        }
    }

    pub async fn history(
        &self,
        from_unix_ms: u64,
        to_unix_ms: u64,
        max_samples: u32,
    ) -> Result<HistoryDocument, ClientError> {
        match self
            .send(MonitorRequest::new(RequestBody::QueryHistory {
                from_unix_ms,
                to_unix_ms,
                max_samples,
            }))
            .await?
        {
            ResponseBody::History(history) => Ok(*history),
            _ => Err(ClientError::UnexpectedReply),
        }
    }

    pub async fn coverage(
        &self,
        from_unix_ms: u64,
        to_unix_ms: u64,
    ) -> Result<CoverageDocument, ClientError> {
        match self
            .send(MonitorRequest::new(RequestBody::QueryCoverage {
                from_unix_ms,
                to_unix_ms,
            }))
            .await?
        {
            ResponseBody::Coverage(coverage) => Ok(*coverage),
            _ => Err(ClientError::UnexpectedReply),
        }
    }

    pub async fn incidents(&self) -> Result<IncidentsDocument, ClientError> {
        match self
            .send(MonitorRequest::new(RequestBody::QueryIncidents))
            .await?
        {
            ResponseBody::Incidents(incidents) => Ok(*incidents),
            _ => Err(ClientError::UnexpectedReply),
        }
    }

    pub async fn incident_window(
        &self,
        incident_id: u64,
    ) -> Result<IncidentWindowDocument, ClientError> {
        match self
            .send(MonitorRequest::new(RequestBody::QueryIncidentWindow {
                incident_id,
            }))
            .await?
        {
            ResponseBody::IncidentWindow(window) => Ok(*window),
            _ => Err(ClientError::UnexpectedReply),
        }
    }

    pub async fn mark(
        &self,
        note: Option<String>,
        window_before_seconds: u64,
        window_after_seconds: u64,
        about_pid: Option<u32>,
    ) -> Result<IncidentWindowDocument, ClientError> {
        match self
            .send(MonitorRequest::new(RequestBody::MarkIncident {
                note,
                window_before_seconds,
                window_after_seconds,
                about_pid,
            }))
            .await?
        {
            ResponseBody::IncidentWindow(window) => Ok(*window),
            _ => Err(ClientError::UnexpectedReply),
        }
    }

    pub async fn inventory(&self) -> Result<InventoryDocument, ClientError> {
        match self
            .send(MonitorRequest::new(RequestBody::QueryInventory))
            .await?
        {
            ResponseBody::Inventory(inventory) => Ok(*inventory),
            _ => Err(ClientError::UnexpectedReply),
        }
    }

    pub async fn inventory_diff(&self) -> Result<InventoryDiffDocument, ClientError> {
        match self
            .send(MonitorRequest::new(RequestBody::QueryInventoryDiff))
            .await?
        {
            ResponseBody::InventoryDiff(diff) => Ok(*diff),
            _ => Err(ClientError::UnexpectedReply),
        }
    }

    pub async fn export(
        &self,
        from_unix_ms: u64,
        to_unix_ms: u64,
        destination: String,
        include_processes: bool,
        preview_only: bool,
    ) -> Result<ExportDocument, ClientError> {
        match self
            .send(MonitorRequest::new(RequestBody::RequestExport {
                from_unix_ms,
                to_unix_ms,
                destination,
                include_processes,
                preview_only,
            }))
            .await?
        {
            ResponseBody::Export(export) => Ok(*export),
            _ => Err(ClientError::UnexpectedReply),
        }
    }
}
