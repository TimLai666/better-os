//! The transitions the service is allowed to make.
//!
//! Every change to what Better Awake is holding off goes through
//! [`AwakeState::apply`], which returns the effects the caller must carry out.
//! Nothing here acquires an inhibitor or writes a file: the state machine
//! decides, the service acts, and the two are tested apart.

use thiserror::Error;

use crate::policy::{BackendCapabilities, EffectivePolicy, PolicyGap, SessionPolicy};
use crate::session::{
    EndCondition, EndConditionError, Session, SessionChange, SessionId, SessionOrigin,
    SessionRequest,
};

/// More than this many sessions at once is a runaway client, not a user.
pub const MAX_ACTIVE_SESSIONS: usize = 32;

/// Why a session stopped. Kept on the ended session so the tray, the store, and
/// a later history view all give the same answer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EndCause {
    /// Someone chose End session.
    UserRequest,
    /// The end condition arrived.
    Expired,
    BatteryThreshold {
        percent: u8,
    },
    /// The inhibitor could not be kept, so the session is not pretending to be
    /// in force.
    BackendFailure,
    /// The service is stopping and is releasing everything it holds.
    ServiceShutdown,
    /// Replaced by a different session the user started in its place.
    Replaced,
}

impl EndCause {
    /// A stable key; presentation layers own the wording.
    pub fn as_key(&self) -> &'static str {
        match self {
            EndCause::UserRequest => "user_request",
            EndCause::Expired => "expired",
            EndCause::BatteryThreshold { .. } => "battery_threshold",
            EndCause::BackendFailure => "backend_failure",
            EndCause::ServiceShutdown => "service_shutdown",
            EndCause::Replaced => "replaced",
        }
    }
}

/// The six states the tray icon distinguishes, per Issue #13.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IndicatorState {
    Inactive,
    ActiveManual,
    ActiveTrigger,
    /// Automatic rules are suspended. No rule engine exists in Phase 1, so
    /// nothing produces this yet; ticket 26 does.
    PausedRules,
    AttentionRequired,
    Unavailable,
}

impl IndicatorState {
    pub fn as_key(&self) -> &'static str {
        match self {
            IndicatorState::Inactive => "inactive",
            IndicatorState::ActiveManual => "active_manual",
            IndicatorState::ActiveTrigger => "active_trigger",
            IndicatorState::PausedRules => "paused_rules",
            IndicatorState::AttentionRequired => "attention_required",
            IndicatorState::Unavailable => "unavailable",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Command {
    Start {
        request: SessionRequest,
        /// Set by a client that has shown the security consequence of turning
        /// off automatic locking or keeping the display on.
        security_confirmed: bool,
    },
    Extend {
        session: SessionId,
        by_seconds: u64,
    },
    Change {
        session: SessionId,
        change: SessionChange,
        security_confirmed: bool,
    },
    End {
        session: SessionId,
        cause: EndCause,
    },
    /// Ends every session whose end condition has arrived.
    Expire,
    /// The battery reading, which may end sessions that watch it.
    BatteryLevel {
        percent: u8,
    },
    /// The service could not confirm the inhibitor is still held.
    InhibitorLost {
        detail: String,
    },
    BackendUnavailable {
        detail: String,
    },
    BackendAvailable(BackendCapabilities),
    /// Ends everything, because the service is going away.
    Shutdown,
}

/// What the service must do after a transition.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Effect {
    SessionStarted(SessionId),
    SessionEnded {
        session: SessionId,
        cause: EndCause,
    },
    /// The merged policy changed, so the held inhibitor must be re-acquired,
    /// replaced, or released.
    PolicyChanged(EffectivePolicy),
    AttentionRaised(String),
    AttentionCleared,
}

#[derive(Clone, Debug, Eq, PartialEq, Error)]
pub enum TransitionError {
    #[error("awake.error.manual_session_already_active:{session}")]
    ManualSessionAlreadyActive { session: SessionId },
    #[error("awake.error.unknown_session:{0}")]
    UnknownSession(SessionId),
    #[error("awake.error.end_condition:{0}")]
    EndCondition(#[from] EndConditionError),
    #[error("awake.error.too_many_sessions")]
    TooManySessions,
    #[error("awake.error.security_confirmation_required")]
    SecurityConfirmationRequired,
    #[error("awake.error.backend_unavailable")]
    BackendUnavailable,
}

/// What the service knows about its inhibitor backend right now.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BackendState {
    Available(BackendCapabilities),
    Unavailable(String),
}

#[derive(Clone, Debug)]
pub struct AwakeState {
    sessions: Vec<Session>,
    next_id: u64,
    backend: BackendState,
    attention: Option<String>,
    /// Whether the user has already been shown, and accepted, the consequence
    /// of a session that keeps the display on or stops the screen locking.
    reduced_security_confirmed: bool,
}

impl AwakeState {
    /// A state whose backend has not reported yet. Nothing may be started until
    /// it does, because a session no backend can enforce would be a lie.
    pub fn new() -> Self {
        Self {
            sessions: Vec::new(),
            next_id: 1,
            backend: BackendState::Unavailable("awake.backend.not_probed".to_string()),
            attention: None,
            reduced_security_confirmed: false,
        }
    }

    pub fn with_backend(capabilities: BackendCapabilities) -> Self {
        let mut state = Self::new();
        state.backend = BackendState::Available(capabilities);
        state
    }

    pub fn sessions(&self) -> &[Session] {
        &self.sessions
    }

    pub fn session(&self, id: SessionId) -> Option<&Session> {
        self.sessions.iter().find(|session| session.id == id)
    }

    pub fn backend(&self) -> &BackendState {
        &self.backend
    }

    pub fn attention(&self) -> Option<&str> {
        self.attention.as_deref()
    }

    pub fn reduced_security_confirmed(&self) -> bool {
        self.reduced_security_confirmed
    }

    /// Lets a restarting service restore the acknowledgement it recorded, so
    /// the first-time warning stays a first-time warning.
    pub fn set_reduced_security_confirmed(&mut self, confirmed: bool) {
        self.reduced_security_confirmed = confirmed;
    }

    pub fn effective_policy(&self) -> EffectivePolicy {
        EffectivePolicy::merge(&self.sessions)
    }

    /// The parts of the merged policy the current backend cannot deliver.
    pub fn unmet_policy(&self) -> Vec<PolicyGap> {
        match &self.backend {
            BackendState::Available(capabilities) => {
                capabilities.gaps(&self.effective_policy().policy)
            }
            BackendState::Unavailable(_) => {
                BackendCapabilities::NONE.gaps(&self.effective_policy().policy)
            }
        }
    }

    pub fn indicator(&self) -> IndicatorState {
        if matches!(self.backend, BackendState::Unavailable(_)) {
            return IndicatorState::Unavailable;
        }
        if self.attention.is_some() {
            return IndicatorState::AttentionRequired;
        }
        if self
            .sessions
            .iter()
            .any(|session| session.origin == SessionOrigin::Manual)
        {
            return IndicatorState::ActiveManual;
        }
        if self.sessions.is_empty() {
            IndicatorState::Inactive
        } else {
            IndicatorState::ActiveTrigger
        }
    }

    /// Applies one command, returning what the service must now do.
    ///
    /// A refused command changes nothing: every check runs before the first
    /// mutation, so a rejection never leaves half a session behind.
    pub fn apply(
        &mut self,
        command: Command,
        now_unix_seconds: u64,
    ) -> Result<Vec<Effect>, TransitionError> {
        let before = self.effective_policy();
        let mut effects = Vec::new();

        match command {
            Command::Start {
                request,
                security_confirmed,
            } => {
                let id = self.start(request, security_confirmed, now_unix_seconds)?;
                effects.push(Effect::SessionStarted(id));
            }
            Command::Extend {
                session,
                by_seconds,
            } => {
                let index = self.index_of(session)?;
                self.sessions[index].extend(by_seconds, now_unix_seconds)?;
            }
            Command::Change {
                session,
                change,
                security_confirmed,
            } => {
                let index = self.index_of(session)?;
                self.check_backend()?;
                self.check_security(&change.policy, security_confirmed)?;
                change.end.validate(now_unix_seconds)?;
                if change.policy.needs_security_confirmation() {
                    self.reduced_security_confirmed = true;
                }
                let existing = &mut self.sessions[index];
                existing.reason = change.reason;
                existing.policy = change.policy;
                existing.battery_stop_percent = change.battery_stop_percent;
                // A replacement duration is measured from now, not from a start
                // that may be hours old, which is what "change this session to
                // one hour" means to the person choosing it.
                existing.end = match change.end {
                    EndCondition::Duration { seconds } => EndCondition::UntilUnixSeconds {
                        unix_seconds: now_unix_seconds.saturating_add(seconds),
                    },
                    other => other,
                };
            }
            Command::End { session, cause } => {
                let index = self.index_of(session)?;
                self.sessions.remove(index);
                effects.push(Effect::SessionEnded { session, cause });
            }
            Command::Expire => {
                let expired: Vec<SessionId> = self
                    .sessions
                    .iter()
                    .filter(|session| session.has_expired(now_unix_seconds))
                    .map(|session| session.id)
                    .collect();
                for session in expired {
                    self.sessions.retain(|active| active.id != session);
                    effects.push(Effect::SessionEnded {
                        session,
                        cause: EndCause::Expired,
                    });
                }
            }
            Command::BatteryLevel { percent } => {
                let stopped: Vec<SessionId> = self
                    .sessions
                    .iter()
                    .filter(|session| session.should_stop_for_battery(percent))
                    .map(|session| session.id)
                    .collect();
                for session in stopped {
                    self.sessions.retain(|active| active.id != session);
                    effects.push(Effect::SessionEnded {
                        session,
                        cause: EndCause::BatteryThreshold { percent },
                    });
                }
            }
            Command::InhibitorLost { detail } => {
                // The sessions stay: the user asked for them and the service
                // will try to re-acquire. What changes is that the tray stops
                // claiming the machine is protected.
                self.attention = Some(detail.clone());
                effects.push(Effect::AttentionRaised(detail));
            }
            Command::BackendUnavailable { detail } => {
                self.backend = BackendState::Unavailable(detail.clone());
                effects.push(Effect::AttentionRaised(detail));
            }
            Command::BackendAvailable(capabilities) => {
                self.backend = BackendState::Available(capabilities);
                if self.attention.take().is_some() {
                    effects.push(Effect::AttentionCleared);
                }
            }
            Command::Shutdown => {
                for session in std::mem::take(&mut self.sessions) {
                    effects.push(Effect::SessionEnded {
                        session: session.id,
                        cause: EndCause::ServiceShutdown,
                    });
                }
            }
        }

        let after = self.effective_policy();
        if after != before {
            effects.push(Effect::PolicyChanged(after));
        }
        Ok(effects)
    }

    fn start(
        &mut self,
        request: SessionRequest,
        security_confirmed: bool,
        now_unix_seconds: u64,
    ) -> Result<SessionId, TransitionError> {
        self.check_backend()?;
        if self.sessions.len() >= MAX_ACTIVE_SESSIONS {
            return Err(TransitionError::TooManySessions);
        }
        if request.origin == SessionOrigin::Manual
            && let Some(existing) = self
                .sessions
                .iter()
                .find(|session| session.origin == SessionOrigin::Manual)
        {
            // Two manual sessions would leave the user unable to say which one
            // the menu is describing. Changing the existing one is the
            // supported way to alter a running session.
            return Err(TransitionError::ManualSessionAlreadyActive {
                session: existing.id,
            });
        }
        self.check_security(&request.policy, security_confirmed)?;
        request.end.validate(now_unix_seconds)?;

        if request.policy.needs_security_confirmation() {
            self.reduced_security_confirmed = true;
        }
        let id = SessionId(self.next_id);
        self.next_id += 1;
        self.sessions.push(Session {
            id,
            reason: request.reason,
            origin: request.origin,
            policy: request.policy,
            battery_stop_percent: request.battery_stop_percent,
            end: request.end,
            started_at_unix_seconds: now_unix_seconds,
        });
        Ok(id)
    }

    fn check_backend(&self) -> Result<(), TransitionError> {
        match self.backend {
            BackendState::Available(_) => Ok(()),
            BackendState::Unavailable(_) => Err(TransitionError::BackendUnavailable),
        }
    }

    fn check_security(
        &self,
        policy: &SessionPolicy,
        security_confirmed: bool,
    ) -> Result<(), TransitionError> {
        if policy.needs_security_confirmation()
            && !security_confirmed
            && !self.reduced_security_confirmed
        {
            return Err(TransitionError::SecurityConfirmationRequired);
        }
        Ok(())
    }

    fn index_of(&self, id: SessionId) -> Result<usize, TransitionError> {
        self.sessions
            .iter()
            .position(|session| session.id == id)
            .ok_or(TransitionError::UnknownSession(id))
    }
}

impl Default for AwakeState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::{DEFAULT_BATTERY_STOP_PERCENT, Reason};

    const NOW: u64 = 1_000;

    fn capabilities() -> BackendCapabilities {
        BackendCapabilities {
            system_suspend: true,
            idle: true,
            display_sleep: false,
            automatic_lock: false,
        }
    }

    fn state() -> AwakeState {
        AwakeState::with_backend(capabilities())
    }

    fn quick(end: EndCondition) -> Command {
        Command::Start {
            request: SessionRequest::quick(Reason::new("Build is running").unwrap(), end),
            security_confirmed: false,
        }
    }

    fn started(state: &mut AwakeState, end: EndCondition) -> SessionId {
        let effects = state.apply(quick(end), NOW).unwrap();
        match effects.first() {
            Some(Effect::SessionStarted(id)) => *id,
            other => panic!("expected a started session, got {other:?}"),
        }
    }

    #[test]
    fn a_fresh_state_holds_nothing_and_reports_no_backend() {
        let state = AwakeState::new();
        assert_eq!(state.indicator(), IndicatorState::Unavailable);
        assert!(state.effective_policy().is_idle());
    }

    #[test]
    fn nothing_can_start_before_a_backend_reports() {
        let mut state = AwakeState::new();
        assert_eq!(
            state.apply(quick(EndCondition::Indefinite), NOW),
            Err(TransitionError::BackendUnavailable)
        );
        assert!(state.sessions().is_empty());
    }

    #[test]
    fn starting_a_session_reports_the_new_effective_policy() {
        let mut state = state();
        let effects = state.apply(quick(EndCondition::Indefinite), NOW).unwrap();

        assert_eq!(effects.len(), 2);
        assert!(matches!(effects[0], Effect::SessionStarted(SessionId(1))));
        let Effect::PolicyChanged(policy) = &effects[1] else {
            panic!("expected a policy change, got {:?}", effects[1]);
        };
        assert!(policy.policy.prevent_system_suspend);
        assert!(!policy.policy.prevent_display_sleep);
        assert_eq!(
            policy.battery_stop_percent,
            Some(DEFAULT_BATTERY_STOP_PERCENT)
        );
        assert_eq!(state.indicator(), IndicatorState::ActiveManual);
    }

    #[test]
    fn a_second_manual_session_is_refused_and_the_first_is_untouched() {
        let mut state = state();
        let first = started(&mut state, EndCondition::Indefinite);

        assert_eq!(
            state.apply(quick(EndCondition::Duration { seconds: 900 }), NOW),
            Err(TransitionError::ManualSessionAlreadyActive { session: first })
        );
        assert_eq!(state.sessions().len(), 1);
        assert_eq!(state.sessions()[0].end, EndCondition::Indefinite);
    }

    #[test]
    fn a_trigger_session_may_run_alongside_a_manual_one() {
        let mut state = state();
        started(&mut state, EndCondition::Indefinite);
        let mut request = SessionRequest::quick(
            Reason::new("External display is connected").unwrap(),
            EndCondition::Indefinite,
        );
        request.origin = SessionOrigin::Trigger;

        state
            .apply(
                Command::Start {
                    request,
                    security_confirmed: false,
                },
                NOW,
            )
            .unwrap();

        assert_eq!(state.sessions().len(), 2);
        assert_eq!(state.effective_policy().reasons.len(), 2);
        assert_eq!(
            state.indicator(),
            IndicatorState::ActiveManual,
            "a manual session is what the user started, so it names the icon"
        );
    }

    #[test]
    fn ending_the_only_session_returns_to_inactive() {
        let mut state = state();
        let id = started(&mut state, EndCondition::Indefinite);

        let effects = state
            .apply(
                Command::End {
                    session: id,
                    cause: EndCause::UserRequest,
                },
                NOW,
            )
            .unwrap();

        assert!(matches!(
            effects[0],
            Effect::SessionEnded {
                cause: EndCause::UserRequest,
                ..
            }
        ));
        assert!(matches!(effects[1], Effect::PolicyChanged(_)));
        assert_eq!(state.indicator(), IndicatorState::Inactive);
    }

    #[test]
    fn ending_a_session_that_is_not_there_is_refused() {
        let mut state = state();
        assert_eq!(
            state.apply(
                Command::End {
                    session: SessionId(7),
                    cause: EndCause::UserRequest,
                },
                NOW,
            ),
            Err(TransitionError::UnknownSession(SessionId(7)))
        );
    }

    #[test]
    fn a_timed_session_expires_on_its_own_and_an_indefinite_one_does_not() {
        let mut state = state();
        let timed = started(&mut state, EndCondition::Duration { seconds: 900 });

        assert!(state.apply(Command::Expire, NOW + 899).unwrap().is_empty());

        let effects = state.apply(Command::Expire, NOW + 900).unwrap();
        assert!(matches!(
            effects[0],
            Effect::SessionEnded {
                session,
                cause: EndCause::Expired,
            } if session == timed
        ));
        assert_eq!(state.indicator(), IndicatorState::Inactive);
    }

    #[test]
    fn extending_a_timed_session_pushes_its_expiry_out() {
        let mut state = state();
        let id = started(&mut state, EndCondition::Duration { seconds: 900 });

        state
            .apply(
                Command::Extend {
                    session: id,
                    by_seconds: 900,
                },
                NOW + 100,
            )
            .unwrap();

        assert!(
            state
                .apply(Command::Expire, NOW + 1_700)
                .unwrap()
                .is_empty()
        );
        assert_eq!(state.apply(Command::Expire, NOW + 1_800).unwrap().len(), 2);
    }

    #[test]
    fn extending_an_indefinite_session_is_refused_rather_than_ignored() {
        let mut state = state();
        let id = started(&mut state, EndCondition::Indefinite);
        assert_eq!(
            state.apply(
                Command::Extend {
                    session: id,
                    by_seconds: 900
                },
                NOW,
            ),
            Err(TransitionError::EndCondition(
                EndConditionError::CannotExtendIndefinite
            ))
        );
    }

    #[test]
    fn changing_a_session_measures_a_new_duration_from_now() {
        let mut state = state();
        let id = started(&mut state, EndCondition::Indefinite);

        state
            .apply(
                Command::Change {
                    session: id,
                    change: SessionChange {
                        reason: Reason::new("Rendering").unwrap(),
                        policy: SessionPolicy::quick_default(),
                        battery_stop_percent: Some(30),
                        end: EndCondition::Duration { seconds: 600 },
                    },
                    security_confirmed: false,
                },
                NOW + 5_000,
            )
            .unwrap();

        let session = state.session(id).unwrap();
        assert_eq!(session.reason.as_str(), "Rendering");
        assert_eq!(session.ends_at_unix_seconds(), Some(NOW + 5_600));
        assert_eq!(state.effective_policy().battery_stop_percent, Some(30));
    }

    #[test]
    fn a_session_that_disables_locking_needs_confirmation_the_first_time_only() {
        let mut state = state();
        let unlocked = SessionPolicy {
            prevent_automatic_lock: true,
            ..SessionPolicy::quick_default()
        };
        let request = |policy| SessionRequest {
            reason: Reason::new("Presenting").unwrap(),
            origin: SessionOrigin::Manual,
            policy,
            battery_stop_percent: Some(DEFAULT_BATTERY_STOP_PERCENT),
            end: EndCondition::Indefinite,
        };

        assert_eq!(
            state.apply(
                Command::Start {
                    request: request(unlocked),
                    security_confirmed: false,
                },
                NOW,
            ),
            Err(TransitionError::SecurityConfirmationRequired)
        );
        assert!(state.sessions().is_empty());

        let id = match state
            .apply(
                Command::Start {
                    request: request(unlocked),
                    security_confirmed: true,
                },
                NOW,
            )
            .unwrap()[0]
        {
            Effect::SessionStarted(id) => id,
            ref other => panic!("expected a started session, got {other:?}"),
        };
        state
            .apply(
                Command::End {
                    session: id,
                    cause: EndCause::UserRequest,
                },
                NOW,
            )
            .unwrap();

        // The warning was shown and accepted once, so the same choice no longer
        // interrupts the user.
        assert!(
            state
                .apply(
                    Command::Start {
                        request: request(unlocked),
                        security_confirmed: false,
                    },
                    NOW,
                )
                .is_ok()
        );
    }

    #[test]
    fn a_battery_reading_below_the_threshold_ends_the_session() {
        let mut state = state();
        let id = started(&mut state, EndCondition::Indefinite);

        assert!(
            state
                .apply(Command::BatteryLevel { percent: 20 }, NOW)
                .unwrap()
                .is_empty()
        );

        let effects = state
            .apply(Command::BatteryLevel { percent: 19 }, NOW)
            .unwrap();
        assert!(matches!(
            effects[0],
            Effect::SessionEnded {
                session,
                cause: EndCause::BatteryThreshold { percent: 19 },
            } if session == id
        ));
        assert_eq!(state.indicator(), IndicatorState::Inactive);
    }

    #[test]
    fn a_lost_inhibitor_raises_attention_without_discarding_the_session() {
        let mut state = state();
        started(&mut state, EndCondition::Indefinite);

        let effects = state
            .apply(
                Command::InhibitorLost {
                    detail: "awake.backend.lease_missing".to_string(),
                },
                NOW,
            )
            .unwrap();

        assert_eq!(
            effects,
            vec![Effect::AttentionRaised(
                "awake.backend.lease_missing".to_string()
            )]
        );
        assert_eq!(state.indicator(), IndicatorState::AttentionRequired);
        assert_eq!(state.sessions().len(), 1);

        state
            .apply(Command::BackendAvailable(capabilities()), NOW)
            .unwrap();
        assert_eq!(state.indicator(), IndicatorState::ActiveManual);
    }

    #[test]
    fn shutdown_ends_every_session_so_nothing_is_left_held() {
        let mut state = state();
        started(&mut state, EndCondition::Indefinite);

        let effects = state.apply(Command::Shutdown, NOW).unwrap();
        assert!(matches!(
            effects[0],
            Effect::SessionEnded {
                cause: EndCause::ServiceShutdown,
                ..
            }
        ));
        assert!(state.sessions().is_empty());
        assert!(state.effective_policy().is_idle());
    }

    #[test]
    fn a_policy_the_backend_cannot_deliver_is_reported_as_unmet() {
        let mut state = state();
        state
            .apply(
                Command::Start {
                    request: SessionRequest {
                        reason: Reason::new("Presenting").unwrap(),
                        origin: SessionOrigin::Manual,
                        policy: SessionPolicy {
                            prevent_display_sleep: true,
                            ..SessionPolicy::quick_default()
                        },
                        battery_stop_percent: None,
                        end: EndCondition::Indefinite,
                    },
                    security_confirmed: true,
                },
                NOW,
            )
            .unwrap();

        assert_eq!(state.unmet_policy(), vec![PolicyGap::DisplaySleep]);
    }

    #[test]
    fn an_end_time_in_the_past_never_becomes_a_session() {
        let mut state = state();
        assert_eq!(
            state.apply(
                quick(EndCondition::UntilUnixSeconds {
                    unix_seconds: NOW - 1
                }),
                NOW,
            ),
            Err(TransitionError::EndCondition(
                EndConditionError::EndTimeInThePast
            ))
        );
        assert!(state.sessions().is_empty());
    }
}
