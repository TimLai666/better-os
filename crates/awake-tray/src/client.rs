//! The tray's only way to change anything.
//!
//! Every action in the menu becomes one typed request to `awake-service`. The
//! tray never runs `systemd-inhibit`, never writes a GNOME setting, and never
//! holds a lock of its own, which is exactly why it can be restarted without
//! ending the session it started.

use awake_ipc::{
    AwakeRequest, AwakeResponse, EventBody, IpcError, RequestBody, ResponseBody, StatusDocument,
};
use std::time::Duration;

use thiserror::Error;

use crate::menu::{MenuAction, QuickOptions};

#[zbus::proxy(
    interface = "org.betteros.Awake1",
    default_service = "org.betteros.Awake1",
    default_path = "/org/betteros/Awake1"
)]
pub trait Awake {
    fn request(&self, request_json: &str) -> zbus::Result<String>;

    /// Every event in the current protocol version.
    #[zbus(signal)]
    fn event(&self, event_json: &str) -> zbus::Result<()>;

    /// The same events in protocol version 2's shape. The only signal a
    /// service from before version 3 sends, so the tray follows it while the
    /// service it talks to is one of those, and ignores it otherwise.
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

/// How long the tray waits for the service to answer one call. A reply the
/// bus lost, or a service wedged inside a handler, must not leave a menu click
/// or the loop that follows the service waiting forever; past this the call is
/// treated as the service being unavailable, which the menu says.
///
/// It is set on the tray's connection (`method_timeout`), not around each call
/// with a runtime's timer: zbus runs the menu's handlers on its own executor,
/// where a tokio timer does not exist.
pub const CALL_TIMEOUT: Duration = Duration::from_secs(5);

/// The tray's first call at login, which may be the one that starts the
/// service through D-Bus activation: the bus's own activation limit.
pub const FIRST_CALL_TIMEOUT: Duration = Duration::from_secs(25);

/// A call the connection's `method_timeout` gave up on is the service being
/// unavailable; any other failure keeps the error it came with.
fn call_error(error: zbus::Error, otherwise: fn(String) -> ClientError) -> ClientError {
    match &error {
        zbus::Error::InputOutput(io) if io.kind() == std::io::ErrorKind::TimedOut => {
            ClientError::ServiceUnavailable(format!("no answer in time: {error}"))
        }
        _ => otherwise(error.to_string()),
    }
}

pub struct ServiceClient {
    proxy: AwakeProxy<'static>,
    /// Set once the service refused the current protocol version for its
    /// older one: until it restarts, requests go in that version's shape and
    /// the lid choice reads as unavailable.
    legacy: std::sync::atomic::AtomicBool,
    /// Whether the service's `ProtocolVersion` property has been read since it
    /// (re)appeared.
    version_read: std::sync::atomic::AtomicBool,
}

impl ServiceClient {
    pub async fn connect() -> Result<Self, ClientError> {
        let connection = zbus::connection::Builder::session()
            .map_err(|error| ClientError::Transport(error.to_string()))?
            .method_timeout(CALL_TIMEOUT)
            .build()
            .await
            .map_err(|error| ClientError::Transport(error.to_string()))?;
        Self::with_connection(connection).await
    }

    pub async fn with_connection(connection: zbus::Connection) -> Result<Self, ClientError> {
        // Properties are not cached: zbus refreshes a cached property only on
        // `PropertiesChanged`, which a service that restarts as a newer one
        // never sends, so a cached `ProtocolVersion` would keep this tray
        // speaking version 2 to a version 3 service until it restarted too.
        let proxy = AwakeProxy::builder(&connection)
            .cache_properties(zbus::proxy::CacheProperties::No)
            .build()
            .await
            .map_err(|error| ClientError::ServiceUnavailable(error.to_string()))?;
        Ok(Self::new(proxy))
    }

    fn new(proxy: AwakeProxy<'static>) -> Self {
        Self {
            proxy,
            legacy: std::sync::atomic::AtomicBool::new(false),
            version_read: std::sync::atomic::AtomicBool::new(false),
        }
    }

    /// Whether the service this tray talks to is one from before protocol
    /// version 3, which a request found out.
    pub fn speaks_legacy(&self) -> bool {
        self.legacy.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Forgets what the last service spoke. Called when a service (re)appears
    /// on the bus, because an update restarts it as the newer one.
    pub fn reset_protocol(&self) {
        self.legacy
            .store(false, std::sync::atomic::Ordering::SeqCst);
        self.version_read
            .store(false, std::sync::atomic::Ordering::SeqCst);
    }

    /// Sends one request in the version the service speaks.
    ///
    /// A service from before protocol version 3 refuses a current request
    /// with a version error; the request is then sent again in its shape,
    /// without the lid fields, and every later one goes that way until the
    /// service restarts. A request only version 3 has is refused here with a
    /// key that says the service is too old.
    async fn exchange(&self, request: &AwakeRequest) -> Result<AwakeResponse, ClientError> {
        // The service says which version it speaks; a service from before
        // version 3 says 2. Asked once per service, before the first request,
        // so a request it could not parse is never sent to it. The refusal
        // check below stays for a service that cannot be asked.
        if !self
            .version_read
            .swap(true, std::sync::atomic::Ordering::SeqCst)
            && let Ok(version) = self.protocol_version().await
            && version == awake_ipc::LEGACY_PROTOCOL_VERSION
        {
            self.legacy.store(true, std::sync::atomic::Ordering::SeqCst);
        }
        if !self.speaks_legacy() {
            let reply = self.call(&request.to_json()?).await?;
            let response = AwakeResponse::from_json_any(&reply)?;
            if !response.is_legacy_version_refusal() {
                return Ok(response);
            }
            self.legacy.store(true, std::sync::atomic::Ordering::SeqCst);
        }
        let document = request
            .to_legacy_json()
            .map_err(|error| ClientError::Rejected(error.to_string()))?;
        let reply = self.call(&document).await?;
        Ok(AwakeResponse::from_json_any(&reply)?)
    }

    async fn call(&self, document: &str) -> Result<String, ClientError> {
        self.proxy
            .request(document)
            .await
            .map_err(|error| call_error(error, ClientError::Transport))
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
            .cache_properties(zbus::proxy::CacheProperties::No)
            .build()
            .await
            .map_err(|error| ClientError::ServiceUnavailable(error.to_string()))?;
        Ok(Self::new(proxy))
    }

    pub async fn protocol_version(&self) -> Result<u32, ClientError> {
        self.proxy
            .protocol_version()
            .await
            .map_err(|error| call_error(error, ClientError::ServiceUnavailable))
    }

    /// Sends one request and unwraps the reply into a status or a refusal.
    pub async fn send(&self, request: AwakeRequest) -> Result<StatusDocument, ClientError> {
        match self.exchange(&request).await?.body {
            ResponseBody::Status(status) => Ok(*status),
            ResponseBody::Rejected { error_key } => Err(ClientError::Rejected(error_key)),
            // The tray asks for none of these. A reply shaped for the rule
            // editor or the history view arriving here means the service
            // answered a different question, so it is an error rather than a
            // status the menu could be redrawn from.
            ResponseBody::Rules(_) | ResponseBody::RuleTest(_) | ResponseBody::History(_) => Err(
                ClientError::Protocol("awake.tray.error.unexpected_reply".to_string()),
            ),
        }
    }

    pub async fn status(&self) -> Result<StatusDocument, ClientError> {
        self.send(AwakeRequest::new(RequestBody::QueryStatus)).await
    }

    /// Every change of who owns the service's name: `Some` when a service
    /// (re)appears, `None` when it leaves the bus.
    pub async fn owner_changes(
        &self,
    ) -> Result<zbus::proxy::OwnerChangedStream<'static>, ClientError> {
        self.proxy
            .inner()
            .receive_owner_changed()
            .await
            .map_err(|error| ClientError::Transport(error.to_string()))
    }

    /// Every status the service pushes, so the menu follows the session rather
    /// than polling for it.
    pub async fn status_updates(&self) -> Result<EventStream, ClientError> {
        self.proxy
            .receive_event()
            .await
            .map_err(|error| ClientError::Transport(error.to_string()))
    }

    /// The same events from a service from before protocol version 3.
    pub async fn legacy_status_updates(&self) -> Result<StatusChangedStream, ClientError> {
        self.proxy
            .receive_status_changed()
            .await
            .map_err(|error| ClientError::Transport(error.to_string()))
    }
}

/// Turns a `StatusChanged` signal body into the status it carries.
pub fn status_from_event(document: &str) -> Result<Option<StatusDocument>, ClientError> {
    Ok(match service_event(document)? {
        ServiceEvent::Status(status) => Some(*status),
        ServiceEvent::LowBatteryStop(_) | ServiceEvent::LidClosedStop(_) | ServiceEvent::Other => {
            None
        }
    })
}

/// A session the service stopped because the battery fell below the
/// threshold the session carried.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LowBatteryStop {
    pub session_id: u64,
    /// The threshold that was crossed, in percent.
    pub threshold_percent: u8,
    /// The reading that crossed it.
    pub percent: u8,
}

/// A session the service stopped because the lid stayed closed on battery for
/// longer than the limit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LidClosedStop {
    pub session_id: u64,
    pub limit_minutes: u32,
}

/// What one signal body means to the tray.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ServiceEvent {
    Status(Box<StatusDocument>),
    LowBatteryStop(LowBatteryStop),
    LidClosedStop(LidClosedStop),
    /// Anything the tray has nothing to do for. A session that ended for
    /// another reason is shown by the status that follows it.
    Other,
}

/// Reads one signal body the service pushed.
///
/// A low-battery stop that does not say which threshold it crossed is an
/// error rather than a notification with the number missing.
pub fn service_event(document: &str) -> Result<ServiceEvent, ClientError> {
    match awake_ipc::AwakeEvent::from_json_any(document)?.body {
        EventBody::StatusChanged(status) => Ok(ServiceEvent::Status(status)),
        EventBody::SessionEnded {
            session_id,
            cause,
            battery_stop_percent,
            battery_percent,
            ..
        } if cause == awake_core::EndCause::BatteryThreshold { percent: 0 }.as_key() => {
            match (battery_stop_percent, battery_percent) {
                (Some(threshold_percent), Some(percent)) => {
                    Ok(ServiceEvent::LowBatteryStop(LowBatteryStop {
                        session_id,
                        threshold_percent,
                        percent,
                    }))
                }
                _ => Err(ClientError::Protocol(format!(
                    "awake.tray.error.battery_stop_without_threshold:{session_id}"
                ))),
            }
        }
        EventBody::SessionEnded {
            session_id,
            cause,
            lid_limit_minutes,
            ..
        } if cause == awake_core::EndCause::LidClosedOnBattery { limit_minutes: 0 }.as_key() => {
            match lid_limit_minutes {
                Some(limit_minutes) => Ok(ServiceEvent::LidClosedStop(LidClosedStop {
                    session_id,
                    limit_minutes,
                })),
                None => Err(ClientError::Protocol(format!(
                    "awake.tray.error.lid_stop_without_limit:{session_id}"
                ))),
            }
        }
        EventBody::SessionEnded { .. } | EventBody::BackendFailure { .. } => {
            Ok(ServiceEvent::Other)
        }
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

/// The request a menu action becomes, for every action that needs nothing from
/// the current status.
///
/// `None` means the action is not one of these: it either needs the running
/// session (start, extend), never reaches the service (a local toggle, opening
/// the window, quitting), or — in the case of
/// [`MenuAction::ArmOverrideAllRules`] — deliberately sends nothing, because
/// arming the override is not overriding it.
pub fn menu_request(action: MenuAction) -> Option<AwakeRequest> {
    let body = match action {
        // Never `EndSession { session_id }`: the tray must not be able to end a
        // rule's session by naming an id it guessed from the status.
        MenuAction::EndSession => RequestBody::EndManualSession,
        MenuAction::PauseRules(seconds) => RequestBody::PauseRules { seconds },
        MenuAction::ResumeRules => RequestBody::ResumeRules,
        MenuAction::ConfirmOverrideAllRules => RequestBody::OverrideAllRules { confirmed: true },
        _ => return None,
    };
    Some(AwakeRequest::new(body))
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
                keep_awake_lid_closed: false,
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
        assert!(!policy.prevent_lid_suspend);
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
            active_rules: Vec::new(),
            rule_summary: awake_ipc::WireRuleSummary::default(),
            rules_suppression: None,
            conflicts: Vec::new(),
            providers: Vec::new(),
            battery_protection: awake_ipc::WireBatteryProtection::default(),
            lid: awake_ipc::WireLidProtection::default(),
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

    fn session_ended(
        cause: &str,
        battery_stop_percent: Option<u8>,
        battery_percent: Option<u8>,
    ) -> String {
        awake_ipc::AwakeEvent::new(EventBody::SessionEnded {
            session_id: 3,
            cause: cause.to_string(),
            battery_stop_percent,
            battery_percent,
            lid_limit_minutes: None,
        })
        .to_json()
        .unwrap()
    }

    #[test]
    fn a_low_battery_stop_is_read_with_the_threshold_it_crossed() {
        assert_eq!(
            service_event(&session_ended("battery_threshold", Some(20), Some(19))).unwrap(),
            ServiceEvent::LowBatteryStop(LowBatteryStop {
                session_id: 3,
                threshold_percent: 20,
                percent: 19,
            })
        );
    }

    #[test]
    fn a_session_that_ended_for_another_reason_is_not_a_low_battery_stop() {
        for cause in ["expired", "trigger_cleared", "service_shutdown"] {
            assert_eq!(
                service_event(&session_ended(cause, Some(20), None)).unwrap(),
                ServiceEvent::Other,
                "{cause}"
            );
        }
    }

    #[test]
    fn a_low_battery_stop_that_does_not_say_its_threshold_is_an_error() {
        assert!(service_event(&session_ended("battery_threshold", None, Some(19))).is_err());
        assert!(service_event(&session_ended("battery_threshold", Some(20), None)).is_err());
    }

    #[test]
    fn a_closed_lid_stop_is_read_with_the_limit_that_ended_it() {
        let event = |lid_limit_minutes| {
            awake_ipc::AwakeEvent::new(EventBody::SessionEnded {
                session_id: 4,
                cause: "lid_closed_on_battery".to_string(),
                battery_stop_percent: Some(20),
                battery_percent: None,
                lid_limit_minutes,
            })
            .to_json()
            .unwrap()
        };
        assert_eq!(
            service_event(&event(Some(120))).unwrap(),
            ServiceEvent::LidClosedStop(LidClosedStop {
                session_id: 4,
                limit_minutes: 120,
            })
        );
        assert!(service_event(&event(None)).is_err());
    }

    #[test]
    fn a_status_from_a_service_before_version_3_is_read_with_the_lid_unavailable() {
        let document = r#"{"protocol_version":2,"body":{"event":"status_changed","indicator":"inactive","effective_policy":{"prevent_system_suspend":false,"prevent_idle":false,"prevent_display_sleep":false,"prevent_automatic_lock":false},"backend":{"name":"logind","available":true,"capabilities":{"system_suspend":true,"idle":true,"display_sleep":false,"automatic_lock":false}},"now_unix_seconds":5}}"#;
        let status = status_from_event(document).unwrap().unwrap();
        assert!(!status.backend.capabilities.lid_switch);
        assert!(!status.effective_policy.prevent_lid_suspend);
    }

    #[test]
    fn a_malformed_signal_body_is_an_error_not_a_silently_ignored_update() {
        assert!(status_from_event("not json").is_err());
    }

    #[test]
    fn ending_a_session_from_the_menu_ends_the_manual_one_and_never_a_rules_session() {
        let request = menu_request(MenuAction::EndSession).expect("End session sends a request");
        assert_eq!(request.body, RequestBody::EndManualSession);
        assert!(request.validate().is_ok());
    }

    #[test]
    fn the_pause_and_resume_items_become_the_requests_the_service_accepts() {
        for (action, expected) in [
            (
                MenuAction::PauseRules(Some(awake_core::PAUSE_SHORT_SECONDS)),
                RequestBody::PauseRules {
                    seconds: Some(awake_core::PAUSE_SHORT_SECONDS),
                },
            ),
            (
                MenuAction::PauseRules(Some(awake_core::PAUSE_LONG_SECONDS)),
                RequestBody::PauseRules {
                    seconds: Some(awake_core::PAUSE_LONG_SECONDS),
                },
            ),
            (
                MenuAction::PauseRules(None),
                RequestBody::PauseRules { seconds: None },
            ),
            (MenuAction::ResumeRules, RequestBody::ResumeRules),
        ] {
            let request = menu_request(action).expect("a rule control sends a request");
            assert_eq!(request.body, expected);
            assert!(
                request.validate().is_ok(),
                "the menu must not offer a length the protocol refuses"
            );
        }
    }

    #[test]
    fn arming_the_override_sends_nothing_and_only_the_confirmation_carries_the_flag() {
        assert_eq!(
            menu_request(MenuAction::ArmOverrideAllRules),
            None,
            "arming must not reach the service"
        );
        assert_eq!(
            menu_request(MenuAction::ConfirmOverrideAllRules)
                .expect("the confirmation sends a request")
                .body,
            RequestBody::OverrideAllRules { confirmed: true }
        );
    }

    #[test]
    fn a_call_that_ran_out_of_time_is_the_service_being_unavailable() {
        let timed_out = zbus::Error::InputOutput(std::sync::Arc::new(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            "timed out",
        )));
        assert!(matches!(
            call_error(timed_out, ClientError::Transport),
            ClientError::ServiceUnavailable(_)
        ));
        assert!(matches!(
            call_error(
                zbus::Error::Failure("no".to_string()),
                ClientError::Transport
            ),
            ClientError::Transport(_)
        ));
    }
}
