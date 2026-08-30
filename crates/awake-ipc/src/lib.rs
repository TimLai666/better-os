//! The local protocol the tray and the Status window speak to `awake-service`.
//!
//! # Transport
//!
//! Requests, replies, and events cross as JSON documents carried by a session
//! D-Bus service, the same document-inside-a-typed-call shape ADR 0007 chose
//! for the privileged daemon. Issue #13 defers the final choice to an ADR; this
//! crate records what Phase 1 implements so the decision is written down rather
//! than only present in code. The session bus was picked over a unix socket
//! because the tray must talk to `org.kde.StatusNotifierWatcher` on that bus
//! anyway, so a socket would add a second transport, its own peer credentials
//! question, and its own lifecycle for nothing.
//!
//! # Trust
//!
//! Unlike `manager-ipc`, this crate does depend on its core crate. Both ends of
//! this protocol run as the same unprivileged user in the same session, so
//! there is no privilege boundary for a shared type to leak across, and one
//! definition of a session beats two that must be kept in step by hand. What
//! survives from `manager-ipc` is the input discipline: closed enums, no
//! unknown fields, and a size limit applied to the raw bytes before parsing.

use awake_core::{
    ActiveReason, BackendCapabilities, EndCondition, EndConditionError, IndicatorState, PolicyGap,
    Reason, ReasonError, Remaining, Session, SessionChange, SessionId, SessionOrigin,
    SessionPolicy, SessionRequest,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// The protocol both sides must agree on. A service rejects any other value
/// rather than guessing which fields it can still trust.
pub const PROTOCOL_VERSION: u32 = 1;

/// Largest accepted request. A session request is a handful of fields; anything
/// larger is a mistake or an attack.
pub const MAX_REQUEST_BYTES: usize = 16 * 1024;

/// Largest accepted reply, which grows with the number of active reasons.
pub const MAX_RESPONSE_BYTES: usize = 256 * 1024;

/// The smallest and largest battery thresholds worth offering. Zero would never
/// fire before the machine died; 100 would stop every session immediately.
pub const MIN_BATTERY_STOP_PERCENT: u8 = 1;
pub const MAX_BATTERY_STOP_PERCENT: u8 = 99;

/// An end condition on the wire. Mirrors [`EndCondition`] so the protocol keeps
/// its own closed shape even if the core model grows a variant.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", deny_unknown_fields)]
pub enum WireEnd {
    Indefinite,
    Duration { seconds: u64 },
    UntilUnixSeconds { unix_seconds: u64 },
}

impl From<WireEnd> for EndCondition {
    fn from(end: WireEnd) -> Self {
        match end {
            WireEnd::Indefinite => EndCondition::Indefinite,
            WireEnd::Duration { seconds } => EndCondition::Duration { seconds },
            WireEnd::UntilUnixSeconds { unix_seconds } => {
                EndCondition::UntilUnixSeconds { unix_seconds }
            }
        }
    }
}

impl From<EndCondition> for WireEnd {
    fn from(end: EndCondition) -> Self {
        match end {
            EndCondition::Indefinite => WireEnd::Indefinite,
            EndCondition::Duration { seconds } => WireEnd::Duration { seconds },
            EndCondition::UntilUnixSeconds { unix_seconds } => {
                WireEnd::UntilUnixSeconds { unix_seconds }
            }
        }
    }
}

/// What a client asks the service to do.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "request", deny_unknown_fields)]
pub enum RequestBody {
    StartSession {
        reason: String,
        policy: SessionPolicy,
        /// `null` means this session never stops itself for battery level.
        #[serde(default)]
        battery_stop_percent: Option<u8>,
        end: WireEnd,
        /// Set only by a client that has shown the security consequence of
        /// keeping the display on or stopping automatic locking.
        #[serde(default)]
        security_confirmed: bool,
    },
    ExtendSession {
        session_id: u64,
        by_seconds: u64,
    },
    /// Replaces the whole mutable part of a running session.
    ChangeSession {
        session_id: u64,
        reason: String,
        policy: SessionPolicy,
        #[serde(default)]
        battery_stop_percent: Option<u8>,
        end: WireEnd,
        #[serde(default)]
        security_confirmed: bool,
    },
    EndSession {
        session_id: u64,
    },
    QueryStatus,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AwakeRequest {
    pub protocol_version: u32,
    pub body: RequestBody,
}

impl AwakeRequest {
    pub fn new(body: RequestBody) -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            body,
        }
    }

    /// Parses and validates a request document. The size limit is applied to
    /// the raw bytes first, so an oversized payload never reaches the parser.
    pub fn from_json(document: &str) -> Result<Self, IpcError> {
        if document.len() > MAX_REQUEST_BYTES {
            return Err(IpcError::PayloadTooLarge {
                bytes: document.len(),
                limit: MAX_REQUEST_BYTES,
            });
        }
        let request: AwakeRequest =
            serde_json::from_str(document).map_err(|error| IpcError::Malformed {
                detail: error.to_string(),
            })?;
        request.validate()?;
        Ok(request)
    }

    pub fn to_json(&self) -> Result<String, IpcError> {
        serde_json::to_string(self).map_err(|error| IpcError::Malformed {
            detail: error.to_string(),
        })
    }

    /// Everything checkable without a clock or a running session.
    pub fn validate(&self) -> Result<(), IpcError> {
        if self.protocol_version != PROTOCOL_VERSION {
            return Err(IpcError::ProtocolVersion {
                found: self.protocol_version,
                expected: PROTOCOL_VERSION,
            });
        }
        match &self.body {
            RequestBody::StartSession {
                reason,
                battery_stop_percent,
                end,
                ..
            } => {
                Reason::new(reason.clone())?;
                validate_battery(*battery_stop_percent)?;
                validate_end(*end)?;
            }
            RequestBody::ChangeSession {
                session_id,
                reason,
                battery_stop_percent,
                end,
                ..
            } => {
                validate_session_id(*session_id)?;
                Reason::new(reason.clone())?;
                validate_battery(*battery_stop_percent)?;
                validate_end(*end)?;
            }
            RequestBody::ExtendSession {
                session_id,
                by_seconds,
            } => {
                validate_session_id(*session_id)?;
                if *by_seconds == 0 || *by_seconds > awake_core::MAX_SESSION_SECONDS {
                    return Err(IpcError::InvalidDuration {
                        seconds: *by_seconds,
                    });
                }
            }
            RequestBody::EndSession { session_id } => validate_session_id(*session_id)?,
            RequestBody::QueryStatus => {}
        }
        Ok(())
    }

    /// The core request a StartSession asks for, once validated.
    pub fn as_session_request(&self) -> Option<Result<SessionRequest, IpcError>> {
        let RequestBody::StartSession {
            reason,
            policy,
            battery_stop_percent,
            end,
            ..
        } = &self.body
        else {
            return None;
        };
        Some(
            Reason::new(reason.clone())
                .map_err(IpcError::from)
                .map(|reason| SessionRequest {
                    reason,
                    origin: SessionOrigin::Manual,
                    policy: *policy,
                    battery_stop_percent: *battery_stop_percent,
                    end: (*end).into(),
                }),
        )
    }

    /// The core change a ChangeSession asks for, once validated.
    pub fn as_session_change(&self) -> Option<Result<SessionChange, IpcError>> {
        let RequestBody::ChangeSession {
            reason,
            policy,
            battery_stop_percent,
            end,
            ..
        } = &self.body
        else {
            return None;
        };
        Some(
            Reason::new(reason.clone())
                .map_err(IpcError::from)
                .map(|reason| SessionChange {
                    reason,
                    policy: *policy,
                    battery_stop_percent: *battery_stop_percent,
                    end: (*end).into(),
                }),
        )
    }
}

fn validate_session_id(session_id: u64) -> Result<(), IpcError> {
    if session_id == 0 {
        Err(IpcError::InvalidSessionId { session_id })
    } else {
        Ok(())
    }
}

fn validate_battery(percent: Option<u8>) -> Result<(), IpcError> {
    match percent {
        None => Ok(()),
        Some(percent)
            if (MIN_BATTERY_STOP_PERCENT..=MAX_BATTERY_STOP_PERCENT).contains(&percent) =>
        {
            Ok(())
        }
        Some(percent) => Err(IpcError::InvalidBatteryThreshold { percent }),
    }
}

/// Only the shape-independent part: whether an `until` time is in the past
/// depends on the clock, so the service checks that against its own.
fn validate_end(end: WireEnd) -> Result<(), IpcError> {
    match end {
        WireEnd::Duration { seconds } => {
            if seconds == 0 || seconds > awake_core::MAX_SESSION_SECONDS {
                Err(IpcError::InvalidDuration { seconds })
            } else {
                Ok(())
            }
        }
        WireEnd::Indefinite | WireEnd::UntilUnixSeconds { .. } => Ok(()),
    }
}

/// How much of a session is left, as a value the tray formats.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", deny_unknown_fields)]
pub enum WireRemaining {
    UntilEnded,
    Seconds {
        seconds: u64,
    },
    /// The end passed and the service has not reaped the session yet.
    Elapsed,
}

impl From<Remaining> for WireRemaining {
    fn from(remaining: Remaining) -> Self {
        match remaining {
            Remaining::UntilEnded => WireRemaining::UntilEnded,
            Remaining::Seconds(seconds) => WireRemaining::Seconds { seconds },
            Remaining::Elapsed => WireRemaining::Elapsed,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireSession {
    pub session_id: u64,
    pub reason: String,
    pub origin: SessionOrigin,
    pub policy: SessionPolicy,
    #[serde(default)]
    pub battery_stop_percent: Option<u8>,
    pub end: WireEnd,
    pub started_at_unix_seconds: u64,
    pub remaining: WireRemaining,
}

impl WireSession {
    pub fn from_session(session: &Session, now_unix_seconds: u64) -> Self {
        Self {
            session_id: session.id.0,
            reason: session.reason.as_str().to_string(),
            origin: session.origin,
            policy: session.policy,
            battery_stop_percent: session.battery_stop_percent,
            end: session.end.into(),
            started_at_unix_seconds: session.started_at_unix_seconds,
            remaining: session.remaining(now_unix_seconds).into(),
        }
    }
}

/// The six icon states, on the wire.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WireIndicator {
    Inactive,
    ActiveManual,
    ActiveTrigger,
    PausedRules,
    AttentionRequired,
    Unavailable,
}

impl From<IndicatorState> for WireIndicator {
    fn from(state: IndicatorState) -> Self {
        match state {
            IndicatorState::Inactive => WireIndicator::Inactive,
            IndicatorState::ActiveManual => WireIndicator::ActiveManual,
            IndicatorState::ActiveTrigger => WireIndicator::ActiveTrigger,
            IndicatorState::PausedRules => WireIndicator::PausedRules,
            IndicatorState::AttentionRequired => WireIndicator::AttentionRequired,
            IndicatorState::Unavailable => WireIndicator::Unavailable,
        }
    }
}

/// What the service knows about the inhibitor backend it is using.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireBackend {
    /// A stable identifier such as `logind`. Never a localized name.
    pub name: String,
    pub available: bool,
    pub capabilities: BackendCapabilities,
    /// Present when the backend is unavailable or degraded. A stable key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// One line of "why is this machine awake".
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireReason {
    pub session_id: u64,
    pub origin: SessionOrigin,
    pub reason: String,
}

impl From<&ActiveReason> for WireReason {
    fn from(reason: &ActiveReason) -> Self {
        Self {
            session_id: reason.session.0,
            origin: reason.origin,
            reason: reason.reason.as_str().to_string(),
        }
    }
}

/// A session the previous run of the service never finished.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireInterrupted {
    pub reason: String,
    pub started_at_unix_seconds: u64,
    /// When the service last recorded that the session was still running.
    pub last_seen_unix_seconds: u64,
}

/// Everything a client needs to draw the menu or the Status window.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StatusDocument {
    pub indicator: WireIndicator,
    /// The merged policy across every active session.
    pub effective_policy: SessionPolicy,
    /// Parts of that policy the backend cannot deliver, so the menu can say so
    /// instead of showing them as in force.
    #[serde(default)]
    pub unmet_policy: Vec<PolicyGap>,
    #[serde(default)]
    pub battery_stop_percent: Option<u8>,
    #[serde(default)]
    pub sessions: Vec<WireSession>,
    #[serde(default)]
    pub reasons: Vec<WireReason>,
    pub backend: WireBackend,
    /// A stable key describing what needs attention, when anything does.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attention: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interrupted_previous_session: Option<WireInterrupted>,
    /// Whether the first-time security warning has already been accepted.
    #[serde(default)]
    pub reduced_security_confirmed: bool,
    /// The service's clock, so a client counts down against the same time the
    /// session was measured with.
    pub now_unix_seconds: u64,
}

impl StatusDocument {
    /// The manual session, if there is one. The tray's active menu describes it.
    pub fn manual_session(&self) -> Option<&WireSession> {
        self.sessions
            .iter()
            .find(|session| session.origin == SessionOrigin::Manual)
    }

    pub fn is_active(&self) -> bool {
        !self.sessions.is_empty()
    }

    pub fn session(&self, session_id: SessionId) -> Option<&WireSession> {
        self.sessions
            .iter()
            .find(|session| session.session_id == session_id.0)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "response", deny_unknown_fields)]
pub enum ResponseBody {
    /// Every accepted request answers with the state it produced, so a client
    /// never has to guess what its own command did.
    Status(Box<StatusDocument>),
    /// A stable machine key. Presentation layers own the wording.
    Rejected { error_key: String },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AwakeResponse {
    pub protocol_version: u32,
    pub body: ResponseBody,
}

impl AwakeResponse {
    pub fn status(status: StatusDocument) -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            body: ResponseBody::Status(Box::new(status)),
        }
    }

    pub fn rejected(error_key: impl Into<String>) -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            body: ResponseBody::Rejected {
                error_key: error_key.into(),
            },
        }
    }

    pub fn from_json(document: &str) -> Result<Self, IpcError> {
        if document.len() > MAX_RESPONSE_BYTES {
            return Err(IpcError::PayloadTooLarge {
                bytes: document.len(),
                limit: MAX_RESPONSE_BYTES,
            });
        }
        let response: AwakeResponse =
            serde_json::from_str(document).map_err(|error| IpcError::Malformed {
                detail: error.to_string(),
            })?;
        if response.protocol_version != PROTOCOL_VERSION {
            return Err(IpcError::ProtocolVersion {
                found: response.protocol_version,
                expected: PROTOCOL_VERSION,
            });
        }
        Ok(response)
    }

    pub fn to_json(&self) -> Result<String, IpcError> {
        serde_json::to_string(self).map_err(|error| IpcError::Malformed {
            detail: error.to_string(),
        })
    }
}

/// What the service pushes without being asked.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "event", deny_unknown_fields)]
pub enum EventBody {
    /// The full state, so a client that missed an event is still correct after
    /// the next one.
    StatusChanged(Box<StatusDocument>),
    SessionEnded {
        session_id: u64,
        cause: String,
    },
    BackendFailure {
        error_key: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AwakeEvent {
    pub protocol_version: u32,
    pub body: EventBody,
}

impl AwakeEvent {
    pub fn new(body: EventBody) -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            body,
        }
    }

    pub fn from_json(document: &str) -> Result<Self, IpcError> {
        if document.len() > MAX_RESPONSE_BYTES {
            return Err(IpcError::PayloadTooLarge {
                bytes: document.len(),
                limit: MAX_RESPONSE_BYTES,
            });
        }
        let event: AwakeEvent =
            serde_json::from_str(document).map_err(|error| IpcError::Malformed {
                detail: error.to_string(),
            })?;
        if event.protocol_version != PROTOCOL_VERSION {
            return Err(IpcError::ProtocolVersion {
                found: event.protocol_version,
                expected: PROTOCOL_VERSION,
            });
        }
        Ok(event)
    }

    pub fn to_json(&self) -> Result<String, IpcError> {
        serde_json::to_string(self).map_err(|error| IpcError::Malformed {
            detail: error.to_string(),
        })
    }
}

/// Protocol-level rejections. Every message is a stable machine key.
#[derive(Clone, Debug, Eq, PartialEq, Error)]
pub enum IpcError {
    #[error("awake.ipc.error.payload_too_large:{bytes}:{limit}")]
    PayloadTooLarge { bytes: usize, limit: usize },
    #[error("awake.ipc.error.malformed:{detail}")]
    Malformed { detail: String },
    #[error("awake.ipc.error.protocol_version:{found}:{expected}")]
    ProtocolVersion { found: u32, expected: u32 },
    #[error("awake.ipc.error.invalid_reason:{0}")]
    InvalidReason(ReasonError),
    #[error("awake.ipc.error.invalid_session_id:{session_id}")]
    InvalidSessionId { session_id: u64 },
    #[error("awake.ipc.error.invalid_battery_threshold:{percent}")]
    InvalidBatteryThreshold { percent: u8 },
    #[error("awake.ipc.error.invalid_duration:{seconds}")]
    InvalidDuration { seconds: u64 },
    #[error("awake.ipc.error.end_condition:{0}")]
    EndCondition(EndConditionError),
}

impl From<ReasonError> for IpcError {
    fn from(error: ReasonError) -> Self {
        IpcError::InvalidReason(error)
    }
}

impl From<EndConditionError> for IpcError {
    fn from(error: EndConditionError) -> Self {
        IpcError::EndCondition(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn start() -> AwakeRequest {
        AwakeRequest::new(RequestBody::StartSession {
            reason: "保持清醒".to_string(),
            policy: SessionPolicy::quick_default(),
            battery_stop_percent: Some(20),
            end: WireEnd::Duration { seconds: 7_200 },
            security_confirmed: false,
        })
    }

    fn status() -> StatusDocument {
        StatusDocument {
            indicator: WireIndicator::Inactive,
            effective_policy: SessionPolicy::default(),
            unmet_policy: Vec::new(),
            battery_stop_percent: None,
            sessions: Vec::new(),
            reasons: Vec::new(),
            backend: WireBackend {
                name: "logind".to_string(),
                available: true,
                capabilities: BackendCapabilities {
                    system_suspend: true,
                    idle: true,
                    display_sleep: false,
                    automatic_lock: false,
                },
                detail: None,
            },
            attention: None,
            interrupted_previous_session: None,
            reduced_security_confirmed: false,
            now_unix_seconds: 1_000,
        }
    }

    #[test]
    fn a_well_formed_request_survives_a_json_round_trip() {
        let document = start().to_json().unwrap();
        assert_eq!(AwakeRequest::from_json(&document).unwrap(), start());
    }

    #[test]
    fn a_status_reply_survives_a_json_round_trip() {
        let response = AwakeResponse::status(status());
        let document = response.to_json().unwrap();
        assert_eq!(AwakeResponse::from_json(&document).unwrap(), response);
    }

    #[test]
    fn an_event_survives_a_json_round_trip() {
        let event = AwakeEvent::new(EventBody::StatusChanged(Box::new(status())));
        let document = event.to_json().unwrap();
        assert_eq!(AwakeEvent::from_json(&document).unwrap(), event);
    }

    #[test]
    fn an_oversized_payload_is_refused_before_parsing() {
        let document = " ".repeat(MAX_REQUEST_BYTES + 1);
        assert!(matches!(
            AwakeRequest::from_json(&document),
            Err(IpcError::PayloadTooLarge { .. })
        ));
    }

    #[test]
    fn an_unknown_field_is_refused() {
        let document = r#"{"protocol_version":1,"body":{"request":"query_status"},"extra":1}"#;
        assert!(matches!(
            AwakeRequest::from_json(document),
            Err(IpcError::Malformed { .. })
        ));
    }

    #[test]
    fn an_unknown_request_is_refused() {
        let document = r#"{"protocol_version":1,"body":{"request":"reboot"}}"#;
        assert!(matches!(
            AwakeRequest::from_json(document),
            Err(IpcError::Malformed { .. })
        ));
    }

    #[test]
    fn another_protocol_version_is_refused_rather_than_partly_trusted() {
        let document = r#"{"protocol_version":2,"body":{"request":"query_status"}}"#;
        assert_eq!(
            AwakeRequest::from_json(document),
            Err(IpcError::ProtocolVersion {
                found: 2,
                expected: 1
            })
        );
    }

    #[test]
    fn a_reply_from_another_protocol_version_is_refused() {
        let document = r#"{"protocol_version":9,"body":{"response":"rejected","error_key":"x"}}"#;
        assert_eq!(
            AwakeResponse::from_json(document),
            Err(IpcError::ProtocolVersion {
                found: 9,
                expected: 1
            })
        );
    }

    #[test]
    fn an_empty_reason_is_refused() {
        let mut request = start();
        let RequestBody::StartSession { reason, .. } = &mut request.body else {
            unreachable!()
        };
        *reason = "   ".to_string();
        assert_eq!(
            AwakeRequest::from_json(&request.to_json().unwrap()),
            Err(IpcError::InvalidReason(ReasonError::Empty))
        );
    }

    #[test]
    fn a_reason_carrying_control_characters_is_refused() {
        let mut request = start();
        let RequestBody::StartSession { reason, .. } = &mut request.body else {
            unreachable!()
        };
        *reason = "line\u{0}break".to_string();
        assert_eq!(
            AwakeRequest::from_json(&request.to_json().unwrap()),
            Err(IpcError::InvalidReason(ReasonError::ControlCharacter))
        );
    }

    #[test]
    fn an_impossible_battery_threshold_is_refused() {
        for percent in [0u8, 100, 255] {
            let mut request = start();
            let RequestBody::StartSession {
                battery_stop_percent,
                ..
            } = &mut request.body
            else {
                unreachable!()
            };
            *battery_stop_percent = Some(percent);
            assert_eq!(
                AwakeRequest::from_json(&request.to_json().unwrap()),
                Err(IpcError::InvalidBatteryThreshold { percent })
            );
        }
    }

    #[test]
    fn opting_out_of_battery_protection_is_accepted() {
        let mut request = start();
        let RequestBody::StartSession {
            battery_stop_percent,
            ..
        } = &mut request.body
        else {
            unreachable!()
        };
        *battery_stop_percent = None;
        assert!(AwakeRequest::from_json(&request.to_json().unwrap()).is_ok());
    }

    #[test]
    fn a_zero_or_absurd_duration_is_refused() {
        for seconds in [0, awake_core::MAX_SESSION_SECONDS + 1] {
            let mut request = start();
            let RequestBody::StartSession { end, .. } = &mut request.body else {
                unreachable!()
            };
            *end = WireEnd::Duration { seconds };
            assert_eq!(
                AwakeRequest::from_json(&request.to_json().unwrap()),
                Err(IpcError::InvalidDuration { seconds })
            );
        }
    }

    #[test]
    fn extending_by_nothing_is_refused_at_the_protocol_edge() {
        let request = AwakeRequest::new(RequestBody::ExtendSession {
            session_id: 1,
            by_seconds: 0,
        });
        assert_eq!(
            AwakeRequest::from_json(&request.to_json().unwrap()),
            Err(IpcError::InvalidDuration { seconds: 0 })
        );
    }

    #[test]
    fn session_zero_is_refused_because_no_session_carries_that_id() {
        for body in [
            RequestBody::EndSession { session_id: 0 },
            RequestBody::ExtendSession {
                session_id: 0,
                by_seconds: 60,
            },
        ] {
            let request = AwakeRequest::new(body);
            assert_eq!(
                AwakeRequest::from_json(&request.to_json().unwrap()),
                Err(IpcError::InvalidSessionId { session_id: 0 })
            );
        }
    }

    #[test]
    fn a_policy_with_an_unknown_flag_is_refused() {
        let document = r#"{"protocol_version":1,"body":{"request":"start_session",
            "reason":"Build","policy":{"prevent_system_suspend":true,"prevent_idle":true,
            "prevent_display_sleep":false,"prevent_automatic_lock":false,"prevent_everything":true},
            "battery_stop_percent":20,"end":{"kind":"indefinite"}}}"#;
        assert!(matches!(
            AwakeRequest::from_json(document),
            Err(IpcError::Malformed { .. })
        ));
    }

    #[test]
    fn a_validated_start_becomes_a_core_session_request() {
        let request = start().as_session_request().unwrap().unwrap();
        assert_eq!(request.reason.as_str(), "保持清醒");
        assert_eq!(request.origin, SessionOrigin::Manual);
        assert_eq!(request.end, EndCondition::Duration { seconds: 7_200 });
    }

    #[test]
    fn a_query_is_not_a_session_request() {
        assert!(
            AwakeRequest::new(RequestBody::QueryStatus)
                .as_session_request()
                .is_none()
        );
    }

    #[test]
    fn an_until_time_in_the_past_passes_the_wire_check_and_is_left_to_the_clock() {
        // The protocol has no clock, so this is deliberately accepted here and
        // refused by the service, which does.
        let request = AwakeRequest::new(RequestBody::StartSession {
            reason: "Build".to_string(),
            policy: SessionPolicy::quick_default(),
            battery_stop_percent: None,
            end: WireEnd::UntilUnixSeconds { unix_seconds: 1 },
            security_confirmed: false,
        });
        assert!(AwakeRequest::from_json(&request.to_json().unwrap()).is_ok());
    }
}
