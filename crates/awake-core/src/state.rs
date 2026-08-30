//! The transitions the service is allowed to make.
//!
//! Every change to what Better Awake is holding off goes through
//! [`AwakeState::apply`], which returns the effects the caller must carry out.
//! Nothing here acquires an inhibitor or writes a file: the state machine
//! decides, the service acts, and the two are tested apart.

use thiserror::Error;

use crate::policy::{BackendCapabilities, EffectivePolicy, PolicyGap, SessionPolicy};
use crate::rules::RuleId;
use crate::session::{
    EndCondition, EndConditionError, Reason, Session, SessionChange, SessionId, SessionOrigin,
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
    /// The automatic rule holding this session stopped matching.
    TriggerCleared,
    /// Automatic rules were paused or overridden, so the rule that was holding
    /// this session is no longer allowed to act. Kept apart from
    /// `TriggerCleared` because the rule still matches; only permission changed,
    /// and a history entry that conflated the two would misexplain the machine.
    RulesSuppressed,
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
            EndCause::TriggerCleared => "trigger_cleared",
            EndCause::RulesSuppressed => "rules_suppressed",
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

/// One rule that is currently satisfied, in the form the state machine needs to
/// hold a session for it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TriggerSession {
    pub rule: RuleId,
    pub reason: Reason,
    pub policy: SessionPolicy,
    pub battery_stop_percent: Option<u8>,
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
    /// Ends the manual session and leaves every trigger session running.
    ///
    /// This exists as its own command rather than as an `End` the tray aims by
    /// hand, because "End session" in a menu must never be able to take a rule's
    /// session with it by picking the wrong id.
    EndManual,
    /// Brings the trigger sessions in line with the rules that currently match.
    ///
    /// Rules that newly match gain a session; rules that stopped matching lose
    /// theirs with `clear_cause`. Manual sessions are never touched.
    SyncTriggerSessions {
        desired: Vec<TriggerSession>,
        clear_cause: EndCause,
    },
    /// Records whether automatic rules are currently suspended, which is what
    /// the paused-rules icon state is drawn from.
    RulesSuppressed {
        suppressed: bool,
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
    /// A matching rule could not be given a session, and why. Reported rather
    /// than dropped, because a rule the user wrote that silently never fires is
    /// the worst outcome available.
    TriggerRefused {
        rule: RuleId,
        error_key: String,
    },
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
    #[error("awake.error.no_manual_session")]
    NoManualSession,
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
    /// Whether automatic rules are paused or overridden right now.
    rules_suppressed: bool,
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
            rules_suppressed: false,
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

    pub fn rules_suppressed(&self) -> bool {
        self.rules_suppressed
    }

    /// The session an automatic rule is holding, if it is holding one.
    pub fn session_for_rule(&self, rule: RuleId) -> Option<&Session> {
        self.sessions
            .iter()
            .find(|session| session.rule == Some(rule))
    }

    /// The one manual session, if there is one.
    pub fn manual_session(&self) -> Option<&Session> {
        self.sessions
            .iter()
            .find(|session| session.origin == SessionOrigin::Manual)
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
        if !self.sessions.is_empty() {
            return IndicatorState::ActiveTrigger;
        }
        // Nothing is being held. Paused rules are worth saying out loud, because
        // the difference between "no rule matches" and "your rules are switched
        // off" is the difference between working and not.
        if self.rules_suppressed {
            IndicatorState::PausedRules
        } else {
            IndicatorState::Inactive
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
            Command::EndManual => {
                let session = self
                    .manual_session()
                    .map(|session| session.id)
                    .ok_or(TransitionError::NoManualSession)?;
                self.sessions.retain(|active| active.id != session);
                effects.push(Effect::SessionEnded {
                    session,
                    cause: EndCause::UserRequest,
                });
            }
            Command::SyncTriggerSessions {
                desired,
                clear_cause,
            } => effects.extend(self.sync_triggers(&desired, clear_cause, now_unix_seconds)),
            Command::RulesSuppressed { suppressed } => {
                self.rules_suppressed = suppressed;
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

    /// Brings the trigger sessions in line with the rules that currently match.
    ///
    /// Sessions are ended before any is started, so a set of rules that swapped
    /// wholesale never transiently exceeds the session cap. A rule that already
    /// holds a session keeps it, including its start time, so a rule that has
    /// matched continuously for an hour does not look like it just began.
    fn sync_triggers(
        &mut self,
        desired: &[TriggerSession],
        clear_cause: EndCause,
        now_unix_seconds: u64,
    ) -> Vec<Effect> {
        let mut effects = Vec::new();

        let stale: Vec<(SessionId, RuleId)> = self
            .sessions
            .iter()
            .filter_map(|session| session.rule.map(|rule| (session.id, rule)))
            .filter(|(_, rule)| !desired.iter().any(|wanted| wanted.rule == *rule))
            .collect();
        for (session, _) in stale {
            self.sessions.retain(|active| active.id != session);
            effects.push(Effect::SessionEnded {
                session,
                cause: clear_cause,
            });
        }

        for wanted in desired {
            if let Some(existing) = self.session_for_rule(wanted.rule) {
                // The rule is already holding one. Its policy may have been
                // edited, so the held session follows the rule rather than
                // keeping whatever it started with.
                let id = existing.id;
                if let Some(session) = self.sessions.iter_mut().find(|session| session.id == id) {
                    session.reason = wanted.reason.clone();
                    session.policy = wanted.policy;
                    session.battery_stop_percent = wanted.battery_stop_percent;
                }
                continue;
            }

            let request = SessionRequest {
                reason: wanted.reason.clone(),
                origin: SessionOrigin::Trigger,
                policy: wanted.policy,
                battery_stop_percent: wanted.battery_stop_percent,
                end: EndCondition::Indefinite,
                rule: Some(wanted.rule),
            };
            // A trigger session is never the moment to raise a first-time
            // security dialog: nobody is at the keyboard to answer it. The
            // acknowledgement must already have been given when the rule was
            // saved, and if it was not, the rule is refused with a reason rather
            // than quietly weakening the machine at three in the morning.
            match self.start(request, false, now_unix_seconds) {
                Ok(id) => effects.push(Effect::SessionStarted(id)),
                Err(error) => effects.push(Effect::TriggerRefused {
                    rule: wanted.rule,
                    error_key: error.to_string(),
                }),
            }
        }

        effects
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
            rule: request.rule,
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
            rule: None,
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
                        rule: None,
                    },
                    security_confirmed: true,
                },
                NOW,
            )
            .unwrap();

        assert_eq!(state.unmet_policy(), vec![PolicyGap::DisplaySleep]);
    }

    // ---- Trigger sessions -------------------------------------------------

    fn trigger(rule: u64, reason: &str) -> TriggerSession {
        TriggerSession {
            rule: RuleId(rule),
            reason: Reason::new(reason).unwrap(),
            policy: SessionPolicy::quick_default(),
            battery_stop_percent: Some(DEFAULT_BATTERY_STOP_PERCENT),
        }
    }

    fn sync(desired: Vec<TriggerSession>) -> Command {
        Command::SyncTriggerSessions {
            desired,
            clear_cause: EndCause::TriggerCleared,
        }
    }

    #[test]
    fn a_matching_rule_gains_a_trigger_session_and_a_stopped_one_loses_it() {
        let mut state = state();

        let effects = state
            .apply(sync(vec![trigger(1, "Build is running")]), NOW)
            .unwrap();
        assert!(matches!(effects[0], Effect::SessionStarted(_)));
        assert_eq!(state.sessions().len(), 1);
        assert_eq!(state.sessions()[0].origin, SessionOrigin::Trigger);
        assert_eq!(state.sessions()[0].rule, Some(RuleId(1)));
        assert_eq!(state.indicator(), IndicatorState::ActiveTrigger);

        let effects = state.apply(sync(Vec::new()), NOW + 60).unwrap();
        assert!(matches!(
            effects[0],
            Effect::SessionEnded {
                cause: EndCause::TriggerCleared,
                ..
            }
        ));
        assert!(state.sessions().is_empty());
    }

    #[test]
    fn a_rule_that_keeps_matching_keeps_the_session_it_already_had() {
        let mut state = state();
        state
            .apply(sync(vec![trigger(1, "Build is running")]), NOW)
            .unwrap();
        let id = state.sessions()[0].id;

        let effects = state
            .apply(sync(vec![trigger(1, "Build is running")]), NOW + 3_600)
            .unwrap();

        assert!(
            effects.is_empty(),
            "a rule that never stopped matching must not restart its session"
        );
        assert_eq!(state.sessions()[0].id, id);
        assert_eq!(
            state.sessions()[0].started_at_unix_seconds,
            NOW,
            "an hour of continuous matching must not look like it just began"
        );
    }

    #[test]
    fn editing_a_rule_updates_the_session_it_is_already_holding() {
        let mut state = state();
        state
            .apply(sync(vec![trigger(1, "Build is running")]), NOW)
            .unwrap();

        let mut edited = trigger(1, "Build is running (renamed)");
        edited.battery_stop_percent = Some(40);
        state.apply(sync(vec![edited]), NOW + 10).unwrap();

        assert_eq!(state.sessions().len(), 1);
        assert_eq!(
            state.sessions()[0].reason.as_str(),
            "Build is running (renamed)"
        );
        assert_eq!(state.effective_policy().battery_stop_percent, Some(40));
    }

    #[test]
    fn ending_a_manual_session_leaves_every_trigger_session_running() {
        let mut state = state();
        started(&mut state, EndCondition::Indefinite);
        state
            .apply(
                sync(vec![
                    trigger(1, "Build is running"),
                    trigger(2, "External display is connected"),
                ]),
                NOW,
            )
            .unwrap();
        assert_eq!(state.sessions().len(), 3);

        let effects = state.apply(Command::EndManual, NOW).unwrap();
        assert!(matches!(
            effects[0],
            Effect::SessionEnded {
                cause: EndCause::UserRequest,
                ..
            }
        ));
        assert_eq!(state.sessions().len(), 2);
        assert!(
            state
                .sessions()
                .iter()
                .all(|session| session.origin == SessionOrigin::Trigger),
            "the two rule-held sessions must survive the user ending their own"
        );
        assert_eq!(state.indicator(), IndicatorState::ActiveTrigger);
        assert_eq!(state.effective_policy().reasons.len(), 2);
    }

    #[test]
    fn ending_a_manual_session_that_is_not_there_is_refused() {
        let mut state = state();
        state.apply(sync(vec![trigger(1, "Build")]), NOW).unwrap();
        assert_eq!(
            state.apply(Command::EndManual, NOW),
            Err(TransitionError::NoManualSession)
        );
        assert_eq!(state.sessions().len(), 1);
    }

    #[test]
    fn several_active_reasons_merge_into_one_policy_with_every_reason_named() {
        let mut state = state();
        started(&mut state, EndCondition::Indefinite);

        let mut presenting = trigger(2, "External display is connected");
        presenting.policy = SessionPolicy {
            prevent_display_sleep: true,
            ..SessionPolicy::quick_default()
        };
        // The rule's reduced-security choice was accepted when it was saved.
        state.set_reduced_security_confirmed(true);
        state
            .apply(
                sync(vec![trigger(1, "Large download is running"), presenting]),
                NOW,
            )
            .unwrap();

        let effective = state.effective_policy();
        assert_eq!(effective.reasons.len(), 3);
        assert!(effective.policy.prevent_system_suspend);
        assert!(effective.policy.prevent_display_sleep);

        // One reason ending leaves the others explaining the machine.
        state
            .apply(sync(vec![trigger(1, "Large download is running")]), NOW)
            .unwrap();
        let effective = state.effective_policy();
        assert_eq!(effective.reasons.len(), 2);
        assert!(
            !effective.policy.prevent_display_sleep,
            "the display rule ended, so its part of the policy ends with it"
        );
        assert!(effective.policy.prevent_system_suspend);
    }

    #[test]
    fn a_rule_whose_policy_was_never_confirmed_is_refused_with_a_reason() {
        let mut state = state();
        let mut unlocking = trigger(1, "Presenting");
        unlocking.policy = SessionPolicy {
            prevent_automatic_lock: true,
            ..SessionPolicy::quick_default()
        };

        let effects = state.apply(sync(vec![unlocking]), NOW).unwrap();

        assert_eq!(
            effects,
            vec![Effect::TriggerRefused {
                rule: RuleId(1),
                error_key: "awake.error.security_confirmation_required".to_string(),
            }],
            "nobody is at the keyboard to answer a security prompt at trigger time"
        );
        assert!(state.sessions().is_empty());
    }

    #[test]
    fn suppressing_rules_clears_their_sessions_with_a_cause_of_its_own() {
        let mut state = state();
        state.apply(sync(vec![trigger(1, "Build")]), NOW).unwrap();

        let effects = state
            .apply(
                Command::SyncTriggerSessions {
                    desired: Vec::new(),
                    clear_cause: EndCause::RulesSuppressed,
                },
                NOW,
            )
            .unwrap();
        assert!(matches!(
            effects[0],
            Effect::SessionEnded {
                cause: EndCause::RulesSuppressed,
                ..
            }
        ));

        state
            .apply(Command::RulesSuppressed { suppressed: true }, NOW)
            .unwrap();
        assert_eq!(
            state.indicator(),
            IndicatorState::PausedRules,
            "paused rules and no rule matching are not the same state"
        );

        state
            .apply(Command::RulesSuppressed { suppressed: false }, NOW)
            .unwrap();
        assert_eq!(state.indicator(), IndicatorState::Inactive);
    }

    #[test]
    fn a_manual_session_still_names_the_icon_while_rules_are_paused() {
        let mut state = state();
        started(&mut state, EndCondition::Indefinite);
        state
            .apply(Command::RulesSuppressed { suppressed: true }, NOW)
            .unwrap();
        assert_eq!(state.indicator(), IndicatorState::ActiveManual);
    }

    #[test]
    fn a_battery_stop_ends_a_trigger_session_the_same_way_it_ends_a_manual_one() {
        let mut state = state();
        state.apply(sync(vec![trigger(1, "Build")]), NOW).unwrap();

        let effects = state
            .apply(Command::BatteryLevel { percent: 19 }, NOW)
            .unwrap();
        assert!(matches!(
            effects[0],
            Effect::SessionEnded {
                cause: EndCause::BatteryThreshold { percent: 19 },
                ..
            }
        ));
        assert!(state.sessions().is_empty());
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
