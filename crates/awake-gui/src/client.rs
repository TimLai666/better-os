//! This window's only way to read or change anything.
//!
//! Every action becomes one typed `awake-ipc` request to `awake-service`. The
//! window never holds an inhibitor, never writes a power setting, and never
//! runs a shell command — which is exactly why closing it changes nothing and
//! why a session it started survives it.
//!
//! The tray carries a client of its own for the same protocol. Keeping this one
//! here rather than importing the tray's is deliberate: the window needs the
//! rules, test, and history replies that a menu never asks for, and a GUI that
//! depended on the tray binary's crate would not build without it.

use awake_ipc::{
    AwakeRequest, AwakeResponse, HistoryDocument, IpcError, RequestBody, ResponseBody,
    RuleTestDocument, RulesDocument, StatusDocument,
};
use thiserror::Error;

#[zbus::proxy(
    interface = "org.betteros.Awake1",
    default_service = "org.betteros.Awake1",
    default_path = "/org/betteros/Awake1"
)]
pub(crate) trait Awake {
    fn request(&self, request_json: &str) -> zbus::Result<String>;

    #[zbus(property)]
    fn protocol_version(&self) -> zbus::Result<u32>;
}

#[derive(Clone, Debug, Eq, PartialEq, Error)]
pub(crate) enum ClientError {
    /// The service is not on the bus. The window says so rather than drawing
    /// controls whose actions would silently do nothing.
    #[error("awake.gui.error.service_unavailable:{0}")]
    ServiceUnavailable(String),
    #[error("awake.gui.error.transport:{0}")]
    Transport(String),
    #[error("awake.gui.error.protocol:{0}")]
    Protocol(String),
    /// The service refused the request and said why, as a stable key.
    #[error("awake.gui.error.rejected:{0}")]
    Rejected(String),
    /// The service answered, but with a different document than the request
    /// asked for. Treated as a protocol fault rather than guessed at.
    #[error("awake.gui.error.unexpected_reply")]
    UnexpectedReply,
}

impl From<IpcError> for ClientError {
    fn from(error: IpcError) -> Self {
        ClientError::Protocol(error.to_string())
    }
}

pub(crate) struct ServiceClient {
    proxy: AwakeProxy<'static>,
}

impl ServiceClient {
    pub(crate) async fn connect() -> Result<Self, ClientError> {
        let connection = zbus::Connection::session()
            .await
            .map_err(|error| ClientError::Transport(error.to_string()))?;
        // Not cached, so `ProtocolVersion` is read from the service each time
        // rather than from whatever answered when the proxy was built.
        let proxy = AwakeProxy::builder(&connection)
            .cache_properties(zbus::proxy::CacheProperties::No)
            .build()
            .await
            .map_err(|error| ClientError::ServiceUnavailable(error.to_string()))?;
        Ok(Self { proxy })
    }

    pub(crate) async fn protocol_version(&self) -> Result<u32, ClientError> {
        self.proxy
            .protocol_version()
            .await
            .map_err(|error| ClientError::ServiceUnavailable(error.to_string()))
    }

    /// Sends one request in the version the service speaks. A service from
    /// before protocol version 3 refuses the current version; the request is
    /// then sent again in its shape, without the lid fields, so this window
    /// keeps working until an update restarts the service. A request only
    /// version 3 has is refused with a key that says the service is too old.
    async fn send(&self, body: RequestBody) -> Result<ResponseBody, ClientError> {
        let request = AwakeRequest::new(body);
        // Asked first: a service from before version 3 reports 2, and a
        // request it cannot parse is then never sent to it.
        let legacy = matches!(
            self.protocol_version().await,
            Ok(version) if version == awake_ipc::LEGACY_PROTOCOL_VERSION
        );
        let mut response = if legacy {
            AwakeResponse::from_json_any(
                &self
                    .call(
                        &request
                            .to_legacy_json()
                            .map_err(|error| ClientError::Rejected(error.to_string()))?,
                    )
                    .await?,
            )?
        } else {
            AwakeResponse::from_json_any(&self.call(&request.to_json()?).await?)?
        };
        if !legacy && response.is_legacy_version_refusal() {
            let document = request
                .to_legacy_json()
                .map_err(|error| ClientError::Rejected(error.to_string()))?;
            response = AwakeResponse::from_json_any(&self.call(&document).await?)?;
        }
        match response.body {
            ResponseBody::Rejected { error_key } => Err(ClientError::Rejected(error_key)),
            body => Ok(body),
        }
    }

    /// Sends a request that changes something, and succeeds on any reply that
    /// is not a refusal. A rule edit is answered with the rule list and a
    /// session change with the status; the window re-reads both afterwards
    /// either way, so the shape of the answer is not what decides success.
    pub(crate) async fn perform(&self, body: RequestBody) -> Result<(), ClientError> {
        self.send(body).await.map(|_| ())
    }

    async fn call(&self, document: &str) -> Result<String, ClientError> {
        self.proxy
            .request(document)
            .await
            .map_err(|error| ClientError::Transport(error.to_string()))
    }

    pub(crate) async fn status_request(
        &self,
        body: RequestBody,
    ) -> Result<StatusDocument, ClientError> {
        match self.send(body).await? {
            ResponseBody::Status(status) => Ok(*status),
            _ => Err(ClientError::UnexpectedReply),
        }
    }

    pub(crate) async fn status(&self) -> Result<StatusDocument, ClientError> {
        self.status_request(RequestBody::QueryStatus).await
    }

    pub(crate) async fn rules(&self) -> Result<RulesDocument, ClientError> {
        match self.send(RequestBody::QueryRules).await? {
            ResponseBody::Rules(rules) => Ok(*rules),
            _ => Err(ClientError::UnexpectedReply),
        }
    }

    pub(crate) async fn test_rule(&self, rule_id: u64) -> Result<RuleTestDocument, ClientError> {
        match self.send(RequestBody::TestRule { rule_id }).await? {
            ResponseBody::RuleTest(test) => Ok(*test),
            _ => Err(ClientError::UnexpectedReply),
        }
    }

    pub(crate) async fn history(&self, limit: u32) -> Result<HistoryDocument, ClientError> {
        match self.send(RequestBody::QueryHistory { limit }).await? {
            ResponseBody::History(history) => Ok(*history),
            _ => Err(ClientError::UnexpectedReply),
        }
    }
}

/// Everything one refresh reads, so the window makes one round of queries
/// rather than one per section.
pub(crate) struct Snapshot {
    pub(crate) status: Result<StatusDocument, ClientError>,
    pub(crate) rules: Result<RulesDocument, ClientError>,
    pub(crate) history: Result<HistoryDocument, ClientError>,
    pub(crate) protocol_version: Option<u32>,
}

impl Snapshot {
    /// The state of every section, or the one connection error that explains
    /// why none of them can be shown.
    pub(crate) fn read(history_limit: u32) -> Self {
        // One runtime that ends with this call. The window is a client; it
        // never holds a connection open for a session it does not own.
        let runtime = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(runtime) => runtime,
            Err(error) => return Self::unreachable(ClientError::Transport(error.to_string())),
        };

        runtime.block_on(async move {
            let client = match ServiceClient::connect().await {
                Ok(client) => client,
                Err(error) => return Self::unreachable(error),
            };
            Self {
                protocol_version: client.protocol_version().await.ok(),
                status: client.status().await,
                rules: client.rules().await,
                history: client.history(history_limit).await,
            }
        })
    }

    fn unreachable(error: ClientError) -> Self {
        Self {
            status: Err(error.clone()),
            rules: Err(error.clone()),
            history: Err(error),
            protocol_version: None,
        }
    }
}
