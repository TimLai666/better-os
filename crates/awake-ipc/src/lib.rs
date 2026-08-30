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
    ProviderKind, Reason, ReasonError, Remaining, Rule, RuleError, Session, SessionChange,
    SessionId, SessionOrigin, SessionPolicy, SessionRequest, Suppression, Truth,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// The protocol both sides must agree on. A service rejects any other value
/// rather than guessing which fields it can still trust.
///
/// Version 2 added the rule surface: rule editing, pause and override, the
/// provider capability report, and history. The version was raised rather than
/// the new fields simply added, because every document here is parsed with
/// `deny_unknown_fields` — a version 1 client handed a version 2 status would
/// reject the whole reply, so pretending the two are compatible would produce a
/// client that silently stopped working rather than one that says why.
pub const PROTOCOL_VERSION: u32 = 2;

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
    /// Ends the manual session and leaves every trigger session running.
    ///
    /// Its own request rather than an `EndSession` the tray aims by hand: the
    /// menu's End session must not be able to take a rule's session with it by
    /// naming the wrong id.
    EndManualSession,
    QueryStatus,

    // ---- Automatic rules ------------------------------------------------
    /// Every rule, in the user's own order.
    QueryRules,
    /// Adds a rule. Whatever id the rule carries is ignored; the service assigns
    /// one, so two clients editing at once cannot mint the same identity.
    CreateRule {
        rule: Box<Rule>,
    },
    /// Replaces everything about a rule except its identity and its position.
    UpdateRule {
        rule_id: u64,
        rule: Box<Rule>,
    },
    DeleteRule {
        rule_id: u64,
    },
    SetRuleEnabled {
        rule_id: u64,
        enabled: bool,
    },
    DuplicateRule {
        rule_id: u64,
    },
    ReorderRule {
        rule_id: u64,
        to_index: u32,
    },
    SetRulePriority {
        rule_id: u64,
        priority: u8,
    },
    /// Evaluates one rule against the current readings and reports what it would
    /// do. Acquires nothing, starts nothing, and works on a disabled rule.
    TestRule {
        rule_id: u64,
    },
    /// Pauses every rule. `None` seconds means until resumed; a duration must be
    /// one of the two lengths Issue #13 names.
    PauseRules {
        #[serde(default)]
        seconds: Option<u64>,
    },
    /// Ends a pause or an override.
    ResumeRules,
    /// Suspends every rule until resumed.
    ///
    /// `confirmed` must be set. The service refuses it otherwise, so a client
    /// that has not shown the consequence cannot turn every rule off by
    /// accident — and the flag being on the wire is what makes that refusal
    /// testable from outside the process.
    OverrideAllRules {
        confirmed: bool,
    },

    // ---- History ---------------------------------------------------------
    /// The most recent sessions, newest first.
    QueryHistory {
        limit: u32,
    },
}

/// The largest history page a client may ask for. A bigger request is refused
/// rather than answered, because a reply is size-limited and a silently
/// truncated page would look like the end of the history.
pub const MAX_HISTORY_PAGE: u32 = 200;

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
            RequestBody::CreateRule { rule } => rule.validate()?,
            RequestBody::UpdateRule { rule_id, rule } => {
                validate_rule_id(*rule_id)?;
                rule.validate()?;
            }
            RequestBody::DeleteRule { rule_id }
            | RequestBody::DuplicateRule { rule_id }
            | RequestBody::TestRule { rule_id }
            | RequestBody::SetRuleEnabled { rule_id, .. }
            | RequestBody::SetRulePriority { rule_id, .. } => validate_rule_id(*rule_id)?,
            RequestBody::ReorderRule { rule_id, to_index } => {
                validate_rule_id(*rule_id)?;
                if *to_index as usize >= awake_core::MAX_RULES {
                    return Err(IpcError::InvalidRulePosition {
                        index: *to_index as usize,
                    });
                }
            }
            RequestBody::PauseRules { seconds } => {
                // The protocol refuses a length the rule engine would refuse
                // anyway, so a client learns at the edge rather than after a
                // round trip.
                if let Some(seconds) = seconds
                    && *seconds != awake_core::PAUSE_SHORT_SECONDS
                    && *seconds != awake_core::PAUSE_LONG_SECONDS
                {
                    return Err(IpcError::InvalidPauseDuration { seconds: *seconds });
                }
            }
            RequestBody::QueryHistory { limit } => {
                if *limit == 0 || *limit > MAX_HISTORY_PAGE {
                    return Err(IpcError::InvalidHistoryLimit { limit: *limit });
                }
            }
            RequestBody::EndManualSession
            | RequestBody::QueryStatus
            | RequestBody::QueryRules
            | RequestBody::ResumeRules
            | RequestBody::OverrideAllRules { .. } => {}
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
                    // A client can only ask for a manual session. A session
                    // belonging to a rule is created by the service's own
                    // evaluation, never by anything that reaches this wire.
                    rule: None,
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

/// Rule ids start at one, so zero is always a client that lost track of which
/// rule it meant rather than a rule that might exist.
fn validate_rule_id(rule_id: u64) -> Result<(), IpcError> {
    if rule_id == 0 {
        Err(IpcError::InvalidRuleId { rule_id })
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

/// One rule that is currently holding a session, named so the tray can list
/// active reasons by rule rather than by opaque session id.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireActiveRule {
    pub rule_id: u64,
    pub name: String,
    pub session_id: u64,
    pub priority: u8,
}

/// How many rules exist and how many are switched on, which is what the tray's
/// "Automatic rules — On" line is drawn from.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireRuleSummary {
    pub total: u32,
    pub enabled: u32,
    /// Rules that match but could not be given a session, so a rule that
    /// silently never fires is visible rather than invisible.
    pub refused: u32,
}

/// Why automatic rules are suspended, on the wire.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", deny_unknown_fields)]
pub enum WireSuppression {
    PausedUntil { unix_seconds: u64 },
    PausedUntilResumed,
    Overridden,
}

impl From<Suppression> for WireSuppression {
    fn from(suppression: Suppression) -> Self {
        match suppression {
            Suppression::PausedUntil { unix_seconds } => {
                WireSuppression::PausedUntil { unix_seconds }
            }
            Suppression::PausedUntilResumed => WireSuppression::PausedUntilResumed,
            Suppression::Overridden => WireSuppression::Overridden,
        }
    }
}

/// One disagreement between active rules and how it was settled.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireConflict {
    /// A stable field key such as `prevent_display_sleep`.
    pub field: String,
    pub winner_rule_id: u64,
    pub winner_name: String,
    pub overridden_rule_ids: Vec<u64>,
    /// A stable key naming the rule that settled it.
    pub resolution_key: String,
}

/// What one provider says about itself.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireProvider {
    pub kind: ProviderKind,
    pub available: bool,
    /// How often it is read. Absent for a provider that needs no polling, which
    /// is how Diagnostics tells "free" apart from "every second".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub poll_seconds: Option<u64>,
    /// A stable key naming what is missing, present exactly when `available` is
    /// false. The pair is what lets a rule editor explain a control rather than
    /// render an inert one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub explanation: Option<String>,
}

/// The battery protection state the Status and Battery sections both show.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireBatteryProtection {
    /// Whether this machine runs on a battery at all, read from the hardware
    /// rather than guessed. A desktop shows the controls as not applicable
    /// instead of offering a threshold that can never fire.
    pub has_battery: bool,
    /// The current reading, absent when it cannot be read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub percent: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_ac_power: Option<bool>,
    /// The threshold in force across every active session.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_below_percent: Option<u8>,
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
    /// Rules currently holding a session, in priority order.
    #[serde(default)]
    pub active_rules: Vec<WireActiveRule>,
    #[serde(default)]
    pub rule_summary: WireRuleSummary,
    /// Why rules are suspended, when they are.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rules_suppression: Option<WireSuppression>,
    /// Disagreements between the active rules, so the UI can explain the merged
    /// policy rather than present it as if nobody had asked for anything else.
    #[serde(default)]
    pub conflicts: Vec<WireConflict>,
    /// What every provider can and cannot do here.
    #[serde(default)]
    pub providers: Vec<WireProvider>,
    #[serde(default)]
    pub battery_protection: WireBatteryProtection,
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

/// Every rule, plus the suspension state that applies to all of them.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RulesDocument {
    /// In the user's own order, which is the order the editor shows.
    pub rules: Vec<Rule>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suppression: Option<WireSuppression>,
    /// Which rules currently match, so the editor can mark them without a second
    /// round trip.
    #[serde(default)]
    pub matching_rule_ids: Vec<u64>,
    pub now_unix_seconds: u64,
}

/// One group's answer inside a rule test.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WireTruth {
    True,
    False,
    /// The provider that would answer this could not be read.
    Unknown,
}

impl From<Truth> for WireTruth {
    fn from(truth: Truth) -> Self {
        match truth {
            Truth::True => WireTruth::True,
            Truth::False => WireTruth::False,
            Truth::Unknown => WireTruth::Unknown,
        }
    }
}

/// What testing one rule found, with nothing having been started.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuleTestDocument {
    pub rule_id: u64,
    pub truth: WireTruth,
    /// One answer per condition group, in the rule's own order.
    pub group_truths: Vec<WireTruth>,
    /// Providers this rule needs that could not be read, each with its reason.
    #[serde(default)]
    pub unavailable_providers: Vec<WireProvider>,
    pub would_be_active: bool,
    /// Set when the rule matches but every rule is suspended.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suppression: Option<WireSuppression>,
    /// Set when the rule itself is switched off. Testing a disabled rule is
    /// allowed, because that is when someone most wants to know if it works.
    pub rule_disabled: bool,
    pub now_unix_seconds: u64,
}

/// One recorded session, on the wire.
///
/// Deliberately not `awake_store`'s own type. The store's on-disk shape and the
/// shape a client reads are allowed to move apart, and putting the file format
/// in the tray's dependency tree to render a list would tie them together for
/// nothing.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireHistoryEntry {
    pub session_id: u64,
    pub started_at_unix_seconds: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_at_unix_seconds: Option<u64>,
    pub origin: SessionOrigin,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule_id: Option<u64>,
    /// Already redacted by the store before it was written. Nothing on this wire
    /// re-derives a reason from process data.
    #[serde(default)]
    pub reasons: Vec<String>,
    pub effective_policy: SessionPolicy,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub battery_stop_percent: Option<u8>,
    /// A stable `EndCause` key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_cause: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backend_failure: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub battery_stop_percent_at_stop: Option<u8>,
}

/// A page of history, newest first.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HistoryDocument {
    pub entries: Vec<WireHistoryEntry>,
    /// How many the service holds in total, so a client can say "showing 50 of
    /// 312" rather than implying it has everything.
    pub total: u32,
    /// The cap the service enforces, so the History view can explain why an old
    /// session is not there instead of looking broken.
    pub retention_limit: u32,
    pub now_unix_seconds: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "response", deny_unknown_fields)]
pub enum ResponseBody {
    /// Every accepted request that changes state answers with the state it
    /// produced, so a client never has to guess what its own command did.
    Status(Box<StatusDocument>),
    Rules(Box<RulesDocument>),
    RuleTest(Box<RuleTestDocument>),
    History(Box<HistoryDocument>),
    /// A stable machine key. Presentation layers own the wording.
    Rejected {
        error_key: String,
    },
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

    pub fn rules(rules: RulesDocument) -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            body: ResponseBody::Rules(Box::new(rules)),
        }
    }

    pub fn rule_test(test: RuleTestDocument) -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            body: ResponseBody::RuleTest(Box::new(test)),
        }
    }

    pub fn history(history: HistoryDocument) -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            body: ResponseBody::History(Box::new(history)),
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
    #[error("awake.ipc.error.invalid_rule_id:{rule_id}")]
    InvalidRuleId { rule_id: u64 },
    #[error("awake.ipc.error.invalid_rule:{0}")]
    InvalidRule(RuleError),
    #[error("awake.ipc.error.invalid_rule_position:{index}")]
    InvalidRulePosition { index: usize },
    #[error("awake.ipc.error.invalid_pause_duration:{seconds}")]
    InvalidPauseDuration { seconds: u64 },
    #[error("awake.ipc.error.invalid_history_limit:{limit}")]
    InvalidHistoryLimit { limit: u32 },
}

impl From<RuleError> for IpcError {
    fn from(error: RuleError) -> Self {
        IpcError::InvalidRule(error)
    }
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
            active_rules: Vec::new(),
            rule_summary: WireRuleSummary::default(),
            rules_suppression: None,
            conflicts: Vec::new(),
            providers: Vec::new(),
            battery_protection: WireBatteryProtection::default(),
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
        let document = r#"{"protocol_version":7,"body":{"request":"query_status"}}"#;
        assert_eq!(
            AwakeRequest::from_json(document),
            Err(IpcError::ProtocolVersion {
                found: 7,
                expected: PROTOCOL_VERSION
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
                expected: PROTOCOL_VERSION
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

    // ---- The rule surface -------------------------------------------------

    fn rule(name: &str) -> Rule {
        use awake_core::{Combine, Condition, ConditionGroup, RuleId};
        Rule::new(
            RuleId(0),
            Reason::new(name).unwrap(),
            Combine::All,
            [ConditionGroup::one(Condition::AcPower { connected: true }).unwrap()],
        )
        .unwrap()
    }

    fn round_trips(body: RequestBody) {
        let request = AwakeRequest::new(body);
        let document = request.to_json().unwrap();
        assert_eq!(AwakeRequest::from_json(&document).unwrap(), request);
    }

    #[test]
    fn every_rule_request_survives_a_json_round_trip() {
        round_trips(RequestBody::QueryRules);
        round_trips(RequestBody::CreateRule {
            rule: Box::new(rule("Build is running")),
        });
        round_trips(RequestBody::UpdateRule {
            rule_id: 3,
            rule: Box::new(rule("Renamed")),
        });
        round_trips(RequestBody::DeleteRule { rule_id: 3 });
        round_trips(RequestBody::SetRuleEnabled {
            rule_id: 3,
            enabled: false,
        });
        round_trips(RequestBody::DuplicateRule { rule_id: 3 });
        round_trips(RequestBody::ReorderRule {
            rule_id: 3,
            to_index: 0,
        });
        round_trips(RequestBody::SetRulePriority {
            rule_id: 3,
            priority: 90,
        });
        round_trips(RequestBody::TestRule { rule_id: 3 });
        round_trips(RequestBody::PauseRules {
            seconds: Some(awake_core::PAUSE_SHORT_SECONDS),
        });
        round_trips(RequestBody::PauseRules { seconds: None });
        round_trips(RequestBody::ResumeRules);
        round_trips(RequestBody::OverrideAllRules { confirmed: true });
        round_trips(RequestBody::QueryHistory { limit: 50 });
        round_trips(RequestBody::EndManualSession);
    }

    #[test]
    fn a_rule_with_no_conditions_is_refused_at_the_protocol_edge() {
        // Hand-written rather than built, because the constructor refuses it and
        // the point is what happens when a document arrives from elsewhere.
        let document = r#"{"protocol_version":2,"body":{"request":"create_rule","rule":
            {"id":0,"name":"Nothing","enabled":true,"priority":50,"combine":"all",
             "groups":[],"policy":{"prevent_system_suspend":true,"prevent_idle":true,
             "prevent_display_sleep":false,"prevent_automatic_lock":false}}}}"#;
        assert_eq!(
            AwakeRequest::from_json(document),
            Err(IpcError::InvalidRule(awake_core::RuleError::EmptyRule)),
            "an empty AND would be vacuously true and keep the machine awake forever"
        );
    }

    #[test]
    fn a_rule_condition_that_is_not_in_the_closed_set_is_refused() {
        // There is no `run_command` condition, and no document can invent one.
        let document = r#"{"protocol_version":2,"body":{"request":"create_rule","rule":
            {"id":0,"name":"Evil","enabled":true,"priority":50,"combine":"all",
             "groups":[{"combine":"all","conditions":[{"condition":"run_command",
             "command":"rm -rf /"}]}],
             "policy":{"prevent_system_suspend":true,"prevent_idle":true,
             "prevent_display_sleep":false,"prevent_automatic_lock":false}}}}"#;
        assert!(matches!(
            AwakeRequest::from_json(document),
            Err(IpcError::Malformed { .. })
        ));
    }

    #[test]
    fn a_rule_with_an_out_of_range_operand_is_refused() {
        use awake_core::{Combine, Condition, ConditionGroup, RuleId};
        let mut broken = rule("Backwards");
        broken.groups = vec![ConditionGroup {
            combine: Combine::All,
            conditions: vec![Condition::BatteryPercent {
                at_least: 80,
                at_most: 20,
            }],
        }];
        broken.id = RuleId(0);
        let request = AwakeRequest::new(RequestBody::CreateRule {
            rule: Box::new(broken),
        });
        assert_eq!(
            AwakeRequest::from_json(&request.to_json().unwrap()),
            Err(IpcError::InvalidRule(
                awake_core::RuleError::InvalidBatteryRange
            ))
        );
    }

    #[test]
    fn rule_zero_is_refused_because_no_rule_carries_that_id() {
        for body in [
            RequestBody::DeleteRule { rule_id: 0 },
            RequestBody::DuplicateRule { rule_id: 0 },
            RequestBody::TestRule { rule_id: 0 },
            RequestBody::SetRuleEnabled {
                rule_id: 0,
                enabled: true,
            },
            RequestBody::SetRulePriority {
                rule_id: 0,
                priority: 1,
            },
        ] {
            let request = AwakeRequest::new(body);
            assert_eq!(
                AwakeRequest::from_json(&request.to_json().unwrap()),
                Err(IpcError::InvalidRuleId { rule_id: 0 })
            );
        }
    }

    #[test]
    fn a_reorder_past_the_rule_limit_is_refused_before_it_reaches_the_engine() {
        let request = AwakeRequest::new(RequestBody::ReorderRule {
            rule_id: 1,
            to_index: awake_core::MAX_RULES as u32,
        });
        assert_eq!(
            AwakeRequest::from_json(&request.to_json().unwrap()),
            Err(IpcError::InvalidRulePosition {
                index: awake_core::MAX_RULES
            })
        );
    }

    #[test]
    fn a_pause_length_nobody_offered_is_refused_at_the_edge() {
        let request = AwakeRequest::new(RequestBody::PauseRules {
            seconds: Some(86_400),
        });
        assert_eq!(
            AwakeRequest::from_json(&request.to_json().unwrap()),
            Err(IpcError::InvalidPauseDuration { seconds: 86_400 }),
            "an arbitrary length would turn Pause into an undocumented off switch"
        );

        for accepted in [
            awake_core::PAUSE_SHORT_SECONDS,
            awake_core::PAUSE_LONG_SECONDS,
        ] {
            let request = AwakeRequest::new(RequestBody::PauseRules {
                seconds: Some(accepted),
            });
            assert!(AwakeRequest::from_json(&request.to_json().unwrap()).is_ok());
        }
    }

    #[test]
    fn an_override_carries_its_confirmation_on_the_wire_so_the_refusal_is_testable() {
        // The protocol accepts both, because the refusal belongs to the rule
        // engine; what matters here is that the flag cannot be omitted and
        // default to true.
        let document = r#"{"protocol_version":2,"body":{"request":"override_all_rules"}}"#;
        assert!(
            matches!(
                AwakeRequest::from_json(document),
                Err(IpcError::Malformed { .. })
            ),
            "a missing confirmation must not be read as a given one"
        );

        let request = AwakeRequest::new(RequestBody::OverrideAllRules { confirmed: false });
        assert!(AwakeRequest::from_json(&request.to_json().unwrap()).is_ok());
    }

    #[test]
    fn a_history_page_that_is_empty_or_unbounded_is_refused() {
        for limit in [0, MAX_HISTORY_PAGE + 1, u32::MAX] {
            let request = AwakeRequest::new(RequestBody::QueryHistory { limit });
            assert_eq!(
                AwakeRequest::from_json(&request.to_json().unwrap()),
                Err(IpcError::InvalidHistoryLimit { limit })
            );
        }
        let request = AwakeRequest::new(RequestBody::QueryHistory {
            limit: MAX_HISTORY_PAGE,
        });
        assert!(AwakeRequest::from_json(&request.to_json().unwrap()).is_ok());
    }

    #[test]
    fn every_new_reply_shape_survives_a_json_round_trip() {
        let rules = AwakeResponse::rules(RulesDocument {
            rules: vec![rule("Build is running")],
            suppression: Some(WireSuppression::PausedUntilResumed),
            matching_rule_ids: vec![1],
            now_unix_seconds: 1_000,
        });
        assert_eq!(
            AwakeResponse::from_json(&rules.to_json().unwrap()).unwrap(),
            rules
        );

        let test = AwakeResponse::rule_test(RuleTestDocument {
            rule_id: 1,
            truth: WireTruth::Unknown,
            group_truths: vec![WireTruth::Unknown],
            unavailable_providers: vec![WireProvider {
                kind: ProviderKind::Fullscreen,
                available: false,
                poll_seconds: None,
                explanation: Some("awake.provider.fullscreen_needs_compositor_adapter".to_string()),
            }],
            would_be_active: false,
            suppression: None,
            rule_disabled: true,
            now_unix_seconds: 1_000,
        });
        assert_eq!(
            AwakeResponse::from_json(&test.to_json().unwrap()).unwrap(),
            test
        );

        let history = AwakeResponse::history(HistoryDocument {
            entries: vec![WireHistoryEntry {
                session_id: 1,
                started_at_unix_seconds: 900,
                ended_at_unix_seconds: Some(1_000),
                origin: SessionOrigin::Trigger,
                rule_id: Some(2),
                reasons: vec!["Build is running".to_string()],
                effective_policy: SessionPolicy::quick_default(),
                battery_stop_percent: Some(20),
                end_cause: Some("battery_threshold".to_string()),
                backend_failure: None,
                battery_stop_percent_at_stop: Some(19),
            }],
            total: 312,
            retention_limit: 500,
            now_unix_seconds: 1_000,
        });
        assert_eq!(
            AwakeResponse::from_json(&history.to_json().unwrap()).unwrap(),
            history
        );
    }

    #[test]
    fn a_status_carrying_the_rule_surface_survives_a_round_trip() {
        let mut status = status();
        status.active_rules = vec![WireActiveRule {
            rule_id: 2,
            name: "External display is connected".to_string(),
            session_id: 7,
            priority: 50,
        }];
        status.rule_summary = WireRuleSummary {
            total: 4,
            enabled: 3,
            refused: 1,
        };
        status.rules_suppression = Some(WireSuppression::PausedUntil {
            unix_seconds: 2_000,
        });
        status.conflicts = vec![WireConflict {
            field: "prevent_display_sleep".to_string(),
            winner_rule_id: 2,
            winner_name: "Presenting".to_string(),
            overridden_rule_ids: vec![1],
            resolution_key: awake_core::RESOLUTION_STRONGEST_WINS.to_string(),
        }];
        status.providers = vec![WireProvider {
            kind: ProviderKind::AcPower,
            available: true,
            poll_seconds: Some(10),
            explanation: None,
        }];
        status.battery_protection = WireBatteryProtection {
            has_battery: true,
            percent: Some(65),
            on_ac_power: Some(true),
            stop_below_percent: Some(20),
        };

        let response = AwakeResponse::status(status);
        assert_eq!(
            AwakeResponse::from_json(&response.to_json().unwrap()).unwrap(),
            response
        );
    }

    #[test]
    fn a_version_one_client_is_refused_rather_than_partly_trusted() {
        // Version 1 shipped without the rule surface. Its documents parse as
        // far as the version field and no further, which is the point.
        let document = r#"{"protocol_version":1,"body":{"request":"query_status"}}"#;
        assert_eq!(
            AwakeRequest::from_json(document),
            Err(IpcError::ProtocolVersion {
                found: 1,
                expected: PROTOCOL_VERSION
            })
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
