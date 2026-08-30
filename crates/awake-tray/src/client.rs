//! The tray's only way to change anything.
//!
//! Every action in the menu becomes one typed request to `awake-service`. The
//! tray never runs `systemd-inhibit`, never writes a GNOME setting, and never
//! holds a lock of its own, which is exactly why it can be restarted without
//! ending the session it started.

use awake_ipc::{
    AwakeRequest, AwakeResponse, EventBody, IpcError, RequestBody, ResponseBody, StatusDocument,
};
use thiserror::Error;

use crate::menu::QuickOptions;

#[zbus::proxy(
    interface = "org.betteros.Awake1",
    default_service = "org.betteros.Awake1",
    default_path = "/org/betteros/Awake1"
)]
pub trait Awake {
    fn request(&self, request_json: &str) -> zbus::Result<String>;

    #[zbus(signal)]
    fn status_changed(&self, event_json: &str) -> zbus::Result<()>;

    #[zbus(property)]
    fn protocol_version(&self) -> zbus::Result<u32>;
}

#[derive(Clone, Debug, Eq, PartialEq, Error)]
pub enum ClientError {
    /// The service is not on the bus. The tray says so rather than drawing a
    /// menu whose actions would silently do nothing.
    #[error("awake.tray.error.service_unavailable:{0}")]
    ServiceUnavailable(String),
    #[error("awake.tray.error.transport:{0}")]
    Transport(String),
    #[error("awake.tray.error.protocol:{0}")]
    Protocol(String),
    /// The service refused the request and said why.
    #[error("awake.tray.error.rejected:{0}")]
    Rejected(String),
}

impl From<IpcError> for ClientError {
    fn from(error: IpcError) -> Self {
        ClientError::Protocol(error.to_string())
    }
}

pub struct ServiceClient {
    proxy: AwakeProxy<'static>,
}

impl ServiceClient {
    pub async fn connect() -> Result<Self, ClientError> {
        let connection = zbus::Connection::session()
            .await
            .map_err(|error| ClientError::Transport(error.to_string()))?;
        Self::with_connection(connection).await
    }

    pub async fn with_connection(connection: zbus::Connection) -> Result<Self, ClientError> {
        let proxy = AwakeProxy::new(&connection)
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
        let proxy = AwakeProxy::builder(&connection)
            .destination(destination)
            .map_err(|error| ClientError::Transport(error.to_string()))?
            .build()
            .await
            .map_err(|error| ClientError::ServiceUnavailable(error.to_string()))?;
        Ok(Self { proxy })
    }

    pub async fn protocol_version(&self) -> Result<u32, ClientError> {
        self.proxy
            .protocol_version()
            .await
            .map_err(|error| ClientError::ServiceUnavailable(error.to_string()))
    }

    /// Sends one request and unwraps the reply into a status or a refusal.
    pub async fn send(&self, request: AwakeRequest) -> Result<StatusDocument, ClientError> {
        let document = request.to_json()?;
        let reply = self
            .proxy
            .request(&document)
            .await
            .map_err(|error| ClientError::Transport(error.to_string()))?;
        match AwakeResponse::from_json(&reply)?.body {
            ResponseBody::Status(status) => Ok(*status),
            ResponseBody::Rejected { error_key } => Err(ClientError::Rejected(error_key)),
        }
    }

    pub async fn status(&self) -> Result<StatusDocument, ClientError> {
        self.send(AwakeRequest::new(RequestBody::QueryStatus)).await
    }

    /// Every status the service pushes, so the menu follows the session rather
    /// than polling for it.
    pub async fn status_updates(&self) -> Result<StatusChangedStream, ClientError> {
        self.proxy
            .receive_status_changed()
            .await
            .map_err(|error| ClientError::Transport(error.to_string()))
    }
}

/// Turns a `StatusChanged` signal body into the status it carries.
pub fn status_from_event(document: &str) -> Result<Option<StatusDocument>, ClientError> {
    match awake_ipc::AwakeEvent::from_json(document)?.body {
        EventBody::StatusChanged(status) => Ok(Some(*status)),
        EventBody::SessionEnded { .. } | EventBody::BackendFailure { .. } => Ok(None),
    }
}

/// Builds the request one preset produces, so the menu, the tests, and the
/// window all agree about what "15 minutes" means.
pub fn start_request(
    reason: &str,
    end: awake_ipc::WireEnd,
    options: QuickOptions,
    security_confirmed: bool,
) -> AwakeRequest {
    AwakeRequest::new(RequestBody::StartSession {
        reason: reason.to_string(),
        policy: options.policy(),
        battery_stop_percent: options.battery_stop_percent(),
        end,
        security_confirmed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use awake_ipc::{WireEnd, WireIndicator};

    #[test]
    fn a_preset_becomes_a_request_carrying_the_quick_options() {
        let request = start_request(
            "保持清醒",
            WireEnd::Duration { seconds: 900 },
            QuickOptions {
                allow_display_off: false,
                stop_below_battery: false,
            },
            true,
        );

        let RequestBody::StartSession {
            policy,
            battery_stop_percent,
            security_confirmed,
            ..
        } = &request.body
        else {
            panic!("expected a start request");
        };
        assert!(policy.prevent_display_sleep);
        assert_eq!(*battery_stop_percent, None);
        assert!(*security_confirmed);
        assert!(request.validate().is_ok());
    }

    #[test]
    fn a_status_event_is_read_back_out_of_its_signal_body() {
        let status = StatusDocument {
            indicator: WireIndicator::Inactive,
            effective_policy: awake_core::SessionPolicy::default(),
            unmet_policy: Vec::new(),
            battery_stop_percent: None,
            sessions: Vec::new(),
            reasons: Vec::new(),
            backend: awake_ipc::WireBackend {
                name: "logind".to_string(),
                available: true,
                capabilities: awake_core::BackendCapabilities::NONE,
                detail: None,
            },
            attention: None,
            interrupted_previous_session: None,
            reduced_security_confirmed: false,
            now_unix_seconds: 1,
        };
        let document =
            awake_ipc::AwakeEvent::new(EventBody::StatusChanged(Box::new(status.clone())))
                .to_json()
                .unwrap();

        assert_eq!(status_from_event(&document).unwrap(), Some(status));
    }

    #[test]
    fn an_event_that_is_not_a_status_is_not_mistaken_for_one() {
        let document = awake_ipc::AwakeEvent::new(EventBody::BackendFailure {
            error_key: "awake.backend.unavailable".to_string(),
        })
        .to_json()
        .unwrap();
        assert_eq!(status_from_event(&document).unwrap(), None);
    }

    #[test]
    fn a_malformed_signal_body_is_an_error_not_a_silently_ignored_update() {
        assert!(status_from_event("not json").is_err());
    }
}
