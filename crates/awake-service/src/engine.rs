//! The service's own logic: state machine plus inhibitor plus store.
//!
//! Everything here is transport-free. The D-Bus surface in `service.rs` does
//! nothing but hand documents in and signals out, which is why a session can be
//! proved to survive a client disconnecting without a bus being involved.

use std::sync::Arc;

use awake_core::{
    AwakeState, BackendCapabilities, BackendState, Command, Effect, EndCause, Evaluation,
    LidBatteryLimit, RuleId, Session, SessionId, TransitionError,
};
use awake_ipc::{
    AwakeEvent, AwakeRequest, AwakeResponse, EventBody, HistoryDocument, LEGACY_PROTOCOL_VERSION,
    MAX_HISTORY_PAGE, RequestBody, RuleTestDocument, RulesDocument, StatusDocument, WireActiveRule,
    WireBackend, WireBatteryProtection, WireConflict, WireHistoryEntry, WireInterrupted,
    WireLidProtection, WireProvider, WireReason, WireRuleSummary, WireSession, WireSuppression,
};
use awake_store::history::{HistoryEntry, MAX_HISTORY_ENTRIES, StartedSession};
use awake_store::{JsonStore, PersistedSession, ServiceState};
use tokio::sync::Mutex;

use crate::backend::{
    BackendError, Clock, InhibitWhat, InhibitorBackend, LeaseHealth, LeaseRequest,
};
use crate::rules::{RuleDriver, RuleEdit};
use crate::settings::{ServiceSettings, SettingsStore};

/// The `who` string every inhibitor is taken out under, so a person reading
/// `systemd-inhibit --list` can see who is holding the machine awake.
pub const INHIBITOR_WHO: &str = "Better Awake";

struct Inner<L> {
    state: AwakeState,
    lease: Option<(LeaseRequest, L)>,
    persisted: ServiceState,
    interrupted: Option<WireInterrupted>,
    backend_detail: Option<String>,
}

pub struct AwakeEngine<B: InhibitorBackend> {
    backend: B,
    clock: Arc<dyn Clock>,
    store: JsonStore,
    capabilities: Mutex<BackendCapabilities>,
    inner: Mutex<Inner<B::Lease>>,
    /// The rules, the providers, and the history. Behind its own lock, because
    /// sampling a provider reads files and must not be holding the session lock
    /// while it does.
    rules: Mutex<RuleDriver>,
    settings: SettingsStore,
    /// The closed-lid limit and the clock it is measured with. Its own lock,
    /// taken alone and never across a backend call.
    lid: Mutex<LidWatch>,
}

/// What the service knows about the lid, for the closed-lid battery limit.
#[derive(Clone, Copy, Debug, Default)]
struct LidWatch {
    limit: LidBatteryLimit,
    /// False when the settings file belongs to a newer Better Awake.
    writable: bool,
    /// The last reading, taken only while it can matter.
    closed: Option<bool>,
    /// Since when the lid has been closed while on battery and undocked, on
    /// the monotonic clock ([`Clock::monotonic_seconds`]), so setting the wall
    /// clock cannot shorten or stretch the measurement.
    closed_on_battery_since: Option<u64>,
    /// The limit ended the sessions holding the lid, and the lid has stayed
    /// closed on battery since. A rule that holds the lid is held back until
    /// the lid opens or the charger returns, so it is not started only to be
    /// ended again on the next tick.
    tripped: bool,
}

/// The refusal a rule holding the lid carries while the closed-lid limit is
/// holding it back.
pub const LID_LIMIT_HELD_BACK: &str = "awake.lid.closed_on_battery_limit";

/// A low-battery stop, reported so the service can raise a notification and the
/// history can record the percentage it happened at.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BatteryStop {
    pub session: SessionId,
    pub percent: u8,
}

/// A session that ended, with what a client needs to explain it.
///
/// Each one comes out of exactly one state transition, so a caller that
/// announces what it is handed announces every end once.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EndedSession {
    pub session: SessionId,
    pub cause: EndCause,
    /// The battery stop threshold the session carried. For a low-battery stop
    /// this is the line that was crossed.
    pub battery_stop_percent: Option<u8>,
}

/// A session the closed-lid battery limit ended, reported so a notification
/// can say why.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LidStop {
    pub session: SessionId,
    pub limit_minutes: u32,
}

/// What one tick did that the clients have to be told about.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TickOutcome {
    /// Every session the tick ended, each once.
    pub ended: Vec<EndedSession>,
    /// Whether the tick changed the sessions the service holds or the rules it
    /// refused. A rule that took hold is a change with nothing ended. A request
    /// answered while the tick ran can also read as a change here, which costs
    /// one extra status push and nothing else.
    pub changed: bool,
}

/// The part of the service's state a tick can change and a client shows: the
/// sessions held and the rules refused.
#[derive(Eq, PartialEq)]
struct TickSnapshot {
    sessions: Vec<SessionId>,
    refused: Vec<RuleId>,
}

impl EndedSession {
    /// The `SessionEnded` event that tells the clients.
    pub fn event(&self) -> AwakeEvent {
        AwakeEvent::new(EventBody::SessionEnded {
            session_id: self.session.0,
            cause: self.cause.as_key().to_string(),
            battery_stop_percent: self.battery_stop_percent,
            battery_percent: self.battery_stop().map(|stop| stop.percent),
            lid_limit_minutes: self.lid_stop().map(|stop| stop.limit_minutes),
        })
    }

    pub fn lid_stop(&self) -> Option<LidStop> {
        match self.cause {
            EndCause::LidClosedOnBattery { limit_minutes } => Some(LidStop {
                session: self.session,
                limit_minutes,
            }),
            _ => None,
        }
    }

    pub fn battery_stop(&self) -> Option<BatteryStop> {
        match self.cause {
            EndCause::BatteryThreshold { percent } => Some(BatteryStop {
                session: self.session,
                percent,
            }),
            _ => None,
        }
    }
}

impl<B: InhibitorBackend> AwakeEngine<B> {
    /// Loads the previous state, probes the backend, and reports anything the
    /// previous run left open.
    ///
    /// A previous session is never resumed. Its inhibitor died with the process
    /// that held it, so bringing it back would claim protection that does not
    /// exist; it is explained instead.
    /// Starts with the real rules, history, and providers.
    pub async fn start(backend: B, store: JsonStore, clock: Arc<dyn Clock>) -> Self {
        let driver = RuleDriver::load(
            awake_store::rules::RulesStore::from_default_path(),
            awake_store::history::HistoryStore::from_default_path(),
            awake_platform::Roots::system(),
        );
        Self::start_with_rules(backend, store, clock, driver).await
    }

    /// Starts with every file the service keeps — state, rules, history — in
    /// `directory`, under the names the service gives them, and with the
    /// providers reading a `/proc` and `/sys` tree under `directory` rather than
    /// this machine's. An empty directory is a machine with no battery, no
    /// charger, and no rules.
    ///
    /// For tests only: [`Self::start`] writes to the person's real History.
    #[cfg(any(test, feature = "test-support"))]
    pub async fn start_in(backend: B, directory: &std::path::Path, clock: Arc<dyn Clock>) -> Self {
        let driver = RuleDriver::load(
            awake_store::rules::RulesStore::at_path(
                directory.join(awake_store::rules::RULES_FILE_NAME),
            ),
            awake_store::history::HistoryStore::at_path(
                directory.join(awake_store::history::HISTORY_FILE_NAME),
            ),
            awake_platform::Roots::at(directory),
        );
        let store = JsonStore::at_path(directory.join(awake_store::STATE_FILE_NAME));
        Self::start_with_rules(backend, store, clock, driver).await
    }

    /// Starts with a rule driver the caller built, which is how a test drives
    /// the whole path — providers included — against a captured `/proc` and
    /// `/sys` tree instead of this machine.
    pub async fn start_with_rules(
        backend: B,
        store: JsonStore,
        clock: Arc<dyn Clock>,
        rules: RuleDriver,
    ) -> Self {
        let now = clock.now_unix_seconds();
        let settings = SettingsStore::beside(store.path());
        let settings_load = settings.load();
        let (mut persisted, interrupted, load_detail) = match store.load(now) {
            Ok(outcome) => {
                let interrupted = outcome.interrupted.first().map(|session| WireInterrupted {
                    reason: session.reason.clone(),
                    started_at_unix_seconds: session.started_at_unix_seconds,
                    last_seen_unix_seconds: session.last_seen_unix_seconds,
                });
                let detail = outcome
                    .recovered_corrupt_state
                    .map(|_| "awake.store.recovered_corrupt_state".to_string());
                (outcome.state, interrupted, detail)
            }
            // An unreadable or newer state file must not stop the service from
            // keeping the machine awake, but it must be said out loud.
            Err(error) => (ServiceState::new(now), None, Some(error.to_string())),
        };

        let mut state = AwakeState::new();
        state.set_reduced_security_confirmed(persisted.reduced_security_confirmed);

        let (capabilities, backend_detail) = match backend.probe().await {
            Ok(capabilities) => {
                let _ = state.apply(Command::BackendAvailable(capabilities), now);
                (capabilities, load_detail)
            }
            Err(error) => {
                let _ = state.apply(
                    Command::BackendUnavailable {
                        detail: error.to_string(),
                    },
                    now,
                );
                (BackendCapabilities::NONE, Some(error.to_string()))
            }
        };

        persisted.run.last_seen_unix_seconds = now;
        let _ = store.save(&persisted);

        // A rules file that could not be read is worth saying out loud, but it
        // must not displace a backend problem, which is the more serious of the
        // two: a service with rules and no backend can do nothing at all.
        let backend_detail = backend_detail
            .or_else(|| rules.load_detail().map(str::to_string))
            .or(settings_load.detail.clone());

        let engine = Self {
            backend,
            clock,
            store,
            capabilities: Mutex::new(capabilities),
            inner: Mutex::new(Inner {
                state,
                lease: None,
                persisted,
                interrupted,
                backend_detail,
            }),
            rules: Mutex::new(rules),
            settings,
            lid: Mutex::new(LidWatch {
                limit: settings_load.settings.lid_battery_limit,
                writable: settings_load.writable,
                ..LidWatch::default()
            }),
        };

        // Evaluated once before the service accepts its first request. A rule
        // that already matches at login must be holding the machine by the time
        // anyone looks, not five seconds later — and the status a client reads
        // immediately after startup must be a real reading rather than a set of
        // empty fields that happen to look like "nothing is happening".
        engine.reconcile_rules(now, true).await;
        engine
    }

    /// The backend this engine is driving. Exposed so a caller can ask it what
    /// it can do, and so a test can ask it what it was told to hold.
    pub fn backend(&self) -> &B {
        &self.backend
    }

    pub fn now(&self) -> u64 {
        self.clock.now_unix_seconds()
    }

    /// Answers one client request.
    ///
    /// A request that changes nothing still answers with the full state, so a
    /// client is never left guessing what its own command did.
    pub async fn handle(&self, request: AwakeRequest) -> AwakeResponse {
        self.handle_reporting(request).await.0
    }

    /// Answers one client request, and hands back any session the service
    /// ended on its own while answering it.
    ///
    /// Only a low-battery stop counts. Every other end on this path is what the
    /// request asked for — ending a session, pausing the rules, editing a rule
    /// so it no longer matches — and the client that asked already has its
    /// answer. Protection is never asked for: a rule edit re-reads the battery,
    /// and a stop found that way is the service's own decision.
    pub async fn handle_reporting(
        &self,
        request: AwakeRequest,
    ) -> (AwakeResponse, Vec<EndedSession>) {
        let now = self.now();
        let request = self.carry_over_for_legacy(request).await;

        // The rule surface answers with its own document shapes, so it is
        // dispatched before the session path rather than folded into it.
        match &request.body {
            RequestBody::SetLidBatteryLimit { limit } => {
                return (self.set_lid_battery_limit(*limit).await, Vec::new());
            }
            RequestBody::QueryRules => {
                return (
                    AwakeResponse::rules(self.rules_document(now).await),
                    Vec::new(),
                );
            }
            RequestBody::TestRule { rule_id } => {
                return (self.test_rule(RuleId(*rule_id), now).await, Vec::new());
            }
            RequestBody::QueryHistory { limit } => {
                return (
                    AwakeResponse::history(self.history_document(*limit, now).await),
                    Vec::new(),
                );
            }
            body => {
                if let Some(edit) = rule_edit_for(body, now) {
                    return self.apply_rule_edit(edit, now).await;
                }
            }
        }

        let command = match self.command_for(&request) {
            Ok(None) => return (AwakeResponse::status(self.status().await), Vec::new()),
            Ok(Some(command)) => command,
            Err(error_key) => return (AwakeResponse::rejected(error_key), Vec::new()),
        };

        let response = match self.apply(command, now).await {
            Ok(()) => AwakeResponse::status(self.status().await),
            Err(error) => AwakeResponse::rejected(error.to_string()),
        };
        (response, Vec::new())
    }

    /// A legacy client knows nothing of the lid choice, so the policy it sends
    /// says "off" only because it cannot say anything else. A session it
    /// starts gets the manual default (on), and when it changes a running
    /// session or edits a rule the choice already in force is kept, so
    /// changing a session's length from an older tray does not quietly let
    /// the lid suspend the machine again. A rule it creates keeps its rule's
    /// default (off).
    async fn carry_over_for_legacy(&self, mut request: AwakeRequest) -> AwakeRequest {
        if request.protocol_version != LEGACY_PROTOCOL_VERSION {
            return request;
        }
        match &mut request.body {
            // A session a person starts by hand keeps awake with the lid closed
            // unless they said otherwise, and a legacy client cannot say
            // otherwise, so it gets the same default a current one does.
            RequestBody::StartSession { policy, .. } => {
                policy.prevent_lid_suspend =
                    awake_core::SessionPolicy::manual_default().prevent_lid_suspend;
            }
            RequestBody::ChangeSession {
                session_id, policy, ..
            } => {
                let inner = self.inner.lock().await;
                if let Some(session) = inner.state.session(SessionId(*session_id)) {
                    policy.prevent_lid_suspend = session.policy.prevent_lid_suspend;
                }
            }
            RequestBody::UpdateRule { rule_id, rule } => {
                let rules = self.rules.lock().await;
                if let Some(existing) = rules.rules().rule(RuleId(*rule_id)) {
                    rule.policy.prevent_lid_suspend = existing.policy.prevent_lid_suspend;
                }
            }
            _ => {}
        }
        request
    }

    /// Stores a new closed-lid battery limit and answers with the status.
    ///
    /// The setting changes when a session holding the lid ends, not what is
    /// held, so no lock is re-taken; the next tick measures against it.
    async fn set_lid_battery_limit(&self, limit: LidBatteryLimit) -> AwakeResponse {
        {
            let mut lid = self.lid.lock().await;
            if !lid.writable {
                return AwakeResponse::rejected("awake.settings.not_writable");
            }
            let settings = ServiceSettings {
                lid_battery_limit: limit,
                ..ServiceSettings::default()
            };
            if let Err(error) = self.settings.save(&settings) {
                return AwakeResponse::rejected(format!("awake.settings.write_failed:{error}"));
            }
            lid.limit = limit;
        }
        AwakeResponse::status(self.status().await)
    }

    /// Applies one rule edit, then re-evaluates immediately.
    ///
    /// Immediately, not at the next tick: a person who has just switched a rule
    /// on and watched nothing happen for five seconds has been told the feature
    /// is broken.
    async fn apply_rule_edit(
        &self,
        edit: RuleEdit,
        now: u64,
    ) -> (AwakeResponse, Vec<EndedSession>) {
        let answers_with_status = edit.answers_with_status();
        {
            let mut rules = self.rules.lock().await;
            if let Err(error) = rules.edit(edit) {
                return (AwakeResponse::rejected(error.to_string()), Vec::new());
            }
        }
        let mut ended = self.reconcile_rules(now, true).await;
        ended.retain(|ended| ended.battery_stop().is_some());

        let response = if answers_with_status {
            AwakeResponse::status(self.status().await)
        } else {
            AwakeResponse::rules(self.rules_document(now).await)
        };
        (response, ended)
    }

    /// Samples the providers, evaluates the rules, and brings the trigger
    /// sessions in line with the answer.
    ///
    /// `force` skips the provider cadences, which is right after an edit and
    /// wrong on a tick — honouring the cadence is what keeps the idle cost
    /// bounded.
    ///
    /// Returns every session the pass ended.
    async fn reconcile_rules(&self, now: u64, force: bool) -> Vec<EndedSession> {
        let (mut desired, suppression, battery_percent) = {
            let mut rules = self.rules.lock().await;
            let evaluation = if force {
                rules.evaluate_now(now)
            } else {
                rules.evaluate(now)
            };
            (
                rules.desired_sessions(&evaluation),
                evaluation.suppression,
                // The battery reading comes from the same sample the rules were
                // evaluated against, so protection acts on the number the rules
                // saw rather than one read a moment later.
                rules.observations().battery_percent,
            )
        };

        // A suppressed rule set clears its sessions with a cause of its own, so
        // history can tell "the rule stopped matching" from "the rule was not
        // allowed to act".
        let clear_cause = if suppression.is_some() {
            EndCause::RulesSuppressed
        } else {
            EndCause::TriggerCleared
        };

        let lid_tripped = self.lid.lock().await.tripped;
        let (effects, mut ended, held_back) = {
            let mut inner = self.inner.lock().await;

            // The closed-lid limit ended the sessions holding the lid, and the
            // lid is still closed on battery: a rule that holds the lid would
            // only be ended again, so it waits for the lid to open or the
            // charger to return.
            let mut held_back: Vec<(RuleId, String)> = Vec::new();
            if lid_tripped {
                desired.retain(|wanted| {
                    let held = wanted.policy.prevent_lid_suspend
                        && inner.state.session_for_rule(wanted.rule).is_none();
                    if held {
                        held_back.push((wanted.rule, LID_LIMIT_HELD_BACK.to_string()));
                    }
                    !held
                });
            }

            // A rule the battery would stop the moment it started is not given
            // a session. Starting it only for protection to end it again would
            // be a fresh stop — a history entry and a notification — on every
            // pass for as long as the rule matches and the battery stays low.
            // It is held back as a refusal instead, so it still reads as
            // matching-but-not-holding. A rule already holding a session keeps
            // it here, so protection below ends it as the battery stop it is.
            if let Some(percent) = battery_percent {
                desired.retain(|wanted| {
                    let held = inner.state.session_for_rule(wanted.rule).is_none()
                        && wanted
                            .battery_stop_percent
                            .is_some_and(|threshold| percent < threshold);
                    if held {
                        held_back.push((
                            wanted.rule,
                            format!("awake.battery.below_stop_threshold:{percent}"),
                        ));
                    }
                    !held
                });
            }

            let (mut effects, ended) = transition(
                &mut inner,
                Command::SyncTriggerSessions {
                    desired,
                    clear_cause,
                },
                now,
            )
            .unwrap_or_default();
            let (suppressed, _) = transition(
                &mut inner,
                Command::RulesSuppressed {
                    suppressed: suppression.is_some(),
                },
                now,
            )
            .unwrap_or_default();
            effects.extend(suppressed);
            (effects, ended, held_back)
        };

        // A rule that matched and could not be given a session is recorded, so
        // the tray and the editor can show it rather than leaving the user to
        // wonder why nothing happened.
        let mut refused: Vec<(RuleId, String)> = effects
            .iter()
            .filter_map(|effect| match effect {
                Effect::TriggerRefused { rule, error_key } => Some((*rule, error_key.clone())),
                _ => None,
            })
            .collect();
        refused.extend(held_back);
        self.rules.lock().await.set_refused(refused);

        self.record_history(&effects, now).await;
        self.reconcile(&effects).await;
        self.persist().await;

        // Protection runs after the rules, not before, so a rule whose edited
        // threshold is now above the reading is stopped on the same pass as a
        // battery stop rather than being allowed to hold the machine awake
        // until the next one.
        if let Some(percent) = battery_percent {
            ended.extend(self.battery_level(percent, now).await);
        }
        ended
    }

    /// Translates a validated request into a state-machine command. `None`
    /// means the request only asked a question.
    fn command_for(&self, request: &AwakeRequest) -> Result<Option<Command>, String> {
        match &request.body {
            RequestBody::QueryStatus => Ok(None),
            RequestBody::StartSession {
                security_confirmed, ..
            } => {
                let session_request = request
                    .as_session_request()
                    .expect("a start request yields a session request")
                    .map_err(|error| error.to_string())?;
                Ok(Some(Command::Start {
                    request: session_request,
                    security_confirmed: *security_confirmed,
                }))
            }
            RequestBody::ChangeSession {
                session_id,
                security_confirmed,
                ..
            } => {
                let change = request
                    .as_session_change()
                    .expect("a change request yields a session change")
                    .map_err(|error| error.to_string())?;
                Ok(Some(Command::Change {
                    session: SessionId(*session_id),
                    change,
                    security_confirmed: *security_confirmed,
                }))
            }
            RequestBody::ExtendSession {
                session_id,
                by_seconds,
            } => Ok(Some(Command::Extend {
                session: SessionId(*session_id),
                by_seconds: *by_seconds,
            })),
            RequestBody::EndSession { session_id } => Ok(Some(Command::End {
                session: SessionId(*session_id),
                cause: EndCause::UserRequest,
            })),
            RequestBody::EndManualSession => Ok(Some(Command::EndManual)),
            // Handled before this point, by the rule dispatch in `handle`.
            RequestBody::QueryRules
            | RequestBody::CreateRule { .. }
            | RequestBody::UpdateRule { .. }
            | RequestBody::DeleteRule { .. }
            | RequestBody::SetRuleEnabled { .. }
            | RequestBody::DuplicateRule { .. }
            | RequestBody::ReorderRule { .. }
            | RequestBody::SetRulePriority { .. }
            | RequestBody::TestRule { .. }
            | RequestBody::PauseRules { .. }
            | RequestBody::ResumeRules
            | RequestBody::OverrideAllRules { .. }
            | RequestBody::QueryHistory { .. }
            | RequestBody::SetLidBatteryLimit { .. } => Ok(None),
        }
    }

    /// Every rule, plus which of them currently match.
    async fn rules_document(&self, now: u64) -> RulesDocument {
        let mut rules = self.rules.lock().await;
        let evaluation = rules.evaluate_now(now);
        RulesDocument {
            rules: rules.rules().rules().to_vec(),
            suppression: evaluation.suppression.map(WireSuppression::from),
            matching_rule_ids: evaluation
                .outcomes
                .iter()
                .filter(|outcome| outcome.truth.is_true())
                .map(|outcome| outcome.rule.0)
                .collect(),
            now_unix_seconds: now,
        }
    }

    /// Tests one rule. Nothing is acquired, nothing is started, and a disabled
    /// rule is tested exactly as a live one is.
    async fn test_rule(&self, id: RuleId, now: u64) -> AwakeResponse {
        let mut rules = self.rules.lock().await;
        let test = match rules.test_rule(id, now) {
            Ok(test) => test,
            Err(error) => return AwakeResponse::rejected(error.to_string()),
        };
        let unavailable_providers = test
            .outcome
            .unavailable_providers
            .iter()
            .map(|(kind, explanation)| WireProvider {
                kind: *kind,
                available: false,
                poll_seconds: rules.cadence(*kind).poll_seconds(),
                explanation: Some(explanation.clone()),
            })
            .collect();

        AwakeResponse::rule_test(RuleTestDocument {
            rule_id: id.0,
            truth: test.outcome.truth.into(),
            group_truths: test
                .outcome
                .group_truths
                .iter()
                .map(|truth| (*truth).into())
                .collect(),
            unavailable_providers,
            would_be_active: test.would_be_active,
            suppression: test.suppression.map(WireSuppression::from),
            rule_disabled: test.rule_disabled,
            now_unix_seconds: now,
        })
    }

    /// The most recent sessions, newest first.
    async fn history_document(&self, limit: u32, now: u64) -> HistoryDocument {
        let limit = limit.min(MAX_HISTORY_PAGE) as usize;
        let rules = self.rules.lock().await;
        let all = rules.history().entries();
        let entries: Vec<WireHistoryEntry> = all
            .iter()
            .rev()
            .take(limit)
            .map(|entry| WireHistoryEntry {
                session_id: entry.session_id,
                started_at_unix_seconds: entry.started_at_unix_seconds,
                ended_at_unix_seconds: entry.ended_at_unix_seconds,
                origin: entry.origin,
                rule_id: entry.rule_id,
                // Already redacted by the store on the way in. Nothing here
                // re-derives a reason from process data.
                reasons: entry.reasons.clone(),
                effective_policy: entry.effective_policy,
                battery_stop_percent: entry.battery_stop_percent,
                end_cause: entry.end_cause.clone(),
                backend_failure: entry.backend_failure.clone(),
                battery_stop_percent_at_stop: entry.battery_stop_percent_at_stop,
            })
            .collect();

        HistoryDocument {
            entries,
            total: all.len() as u32,
            retention_limit: MAX_HISTORY_ENTRIES as u32,
            now_unix_seconds: now,
        }
    }

    /// Mirrors the effects into the history.
    ///
    /// The history is the record a person reads afterwards to find out why the
    /// machine stayed up, so a session that started and a session that ended
    /// both have to reach it — including the ones nobody was watching.
    async fn record_history(&self, effects: &[Effect], now: u64) {
        if effects.is_empty() {
            return;
        }
        let started: Vec<StartedSession> = {
            let inner = self.inner.lock().await;
            effects
                .iter()
                .filter_map(|effect| match effect {
                    Effect::SessionStarted(id) => inner.state.session(*id).map(|session| {
                        StartedSession {
                            session_id: session.id.0,
                            started_at_unix_seconds: session.started_at_unix_seconds,
                            origin: session.origin,
                            rule_id: session.rule.map(|rule| rule.0),
                            // Raw here on purpose: the store redacts, so a
                            // caller cannot forget to.
                            reasons: vec![session.reason.as_str().to_string()],
                            effective_policy: session.policy,
                            battery_stop_percent: session.battery_stop_percent,
                        }
                    }),
                    _ => None,
                })
                .collect()
        };

        let mut rules = self.rules.lock().await;
        for start in started {
            rules
                .history_mut()
                .record_start(HistoryEntry::record(start));
        }
        for effect in effects {
            match effect {
                Effect::SessionEnded { session, cause } => {
                    if let EndCause::BatteryThreshold { percent } = cause {
                        rules.history_mut().record_battery_stop(session.0, *percent);
                    }
                    rules
                        .history_mut()
                        .record_end(session.0, now, cause.as_key());
                }
                Effect::AttentionRaised(detail) => {
                    // A backend failure belongs to whatever was running when it
                    // happened, which is every session that is still open.
                    let open: Vec<u64> = rules
                        .history()
                        .entries()
                        .iter()
                        .filter(|entry| entry.is_running())
                        .map(|entry| entry.session_id)
                        .collect();
                    for session_id in open {
                        rules
                            .history_mut()
                            .record_backend_failure(session_id, detail);
                    }
                }
                _ => {}
            }
        }
        rules.save_history();
    }

    /// Applies a command and carries out every effect it produced.
    pub async fn apply(&self, command: Command, now: u64) -> Result<(), TransitionError> {
        self.apply_reporting(command, now).await.map(|_| ())
    }

    /// Applies a command and hands back the effects, so a caller that must react
    /// to one can see it rather than re-deriving it from the state afterwards.
    pub async fn apply_reporting(
        &self,
        command: Command,
        now: u64,
    ) -> Result<Vec<Effect>, TransitionError> {
        self.apply_transition(command, now)
            .await
            .map(|(effects, _)| effects)
    }

    /// Applies a command, carries out its effects, and hands back both the
    /// effects and the sessions it ended.
    async fn apply_transition(
        &self,
        command: Command,
        now: u64,
    ) -> Result<(Vec<Effect>, Vec<EndedSession>), TransitionError> {
        let (effects, ended) = {
            let mut inner = self.inner.lock().await;
            transition(&mut inner, command, now)?
        };
        self.record_history(&effects, now).await;
        self.reconcile(&effects).await;
        self.persist().await;
        Ok((effects, ended))
    }

    /// One service tick: reap expired sessions, re-evaluate the rules against
    /// fresh readings, and check the inhibitor is still held.
    ///
    /// The rule evaluation honours each provider's own cadence, so a five-second
    /// tick does not mean five-second polling of everything: a provider is only
    /// re-read when its own interval has elapsed, and a provider no enabled rule
    /// needs is never read at all.
    ///
    /// Returns every session this tick ended — expired, stopped by low battery,
    /// or released by a rule that stopped matching — and whether it changed the
    /// sessions or the refused rules at all, which a rule taking hold does
    /// without ending anything. None of it was asked for by a client, so the
    /// caller tells the clients; the history entry is already written.
    pub async fn tick(&self) -> TickOutcome {
        let before = self.tick_snapshot().await;
        let now = self.now();
        let mut ended = self
            .apply_transition(Command::Expire, now)
            .await
            .map(|(_, ended)| ended)
            .unwrap_or_default();
        ended.extend(self.reconcile_rules(now, false).await);
        ended.extend(self.watch_lid(now).await);
        // A lock that could not be swapped is tried again on every tick until
        // what is held is what the sessions want; when it already is, this
        // asks the backend nothing.
        self.reacquire().await;
        self.verify_lease().await;
        self.persist().await;
        let changed = !ended.is_empty() || self.tick_snapshot().await != before;
        TickOutcome { ended, changed }
    }

    /// Measures how long the lid has been closed on battery, and ends every
    /// session holding the lid once that reaches the limit.
    ///
    /// The lid is read only while it can matter (a session holds the lid, or
    /// the clock is already running), so a machine with nothing held asks
    /// logind nothing. The battery stop threshold is not this: it ends a
    /// session at its own percentage, lid or no lid, through the rule pass.
    ///
    /// Returns the sessions it ended.
    async fn watch_lid(&self, now: u64) -> Vec<EndedSession> {
        let held = self
            .inner
            .lock()
            .await
            .state
            .effective_policy()
            .policy
            .prevent_lid_suspend;
        let on_battery = self.rules.lock().await.observations().ac_power_connected == Some(false);
        let needed = {
            let lid = self.lid.lock().await;
            held || lid.closed_on_battery_since.is_some() || lid.tripped
        };
        let closed = if needed {
            self.backend.lid_closed().await
        } else {
            None
        };
        // Docked, or an external display connected: logind then acts on the
        // lid with `HandleLidSwitchDocked` (`ignore` by default) and GNOME
        // keeps the machine up itself, so a closed lid there is a desk setup,
        // not a laptop in a bag, and the limit does not run.
        let docked = if closed == Some(true) {
            self.backend.docked().await == Some(true)
        } else {
            false
        };
        let mono = self.clock.monotonic_seconds();

        let reached = {
            let mut lid = self.lid.lock().await;
            lid.closed = closed;
            if closed == Some(true) && on_battery && !docked {
                lid.closed_on_battery_since.get_or_insert(mono);
            } else {
                lid.closed_on_battery_since = None;
                lid.tripped = false;
            }
            let limit = lid.limit;
            match (held, lid.closed_on_battery_since, limit.minutes()) {
                (true, Some(since), Some(minutes)) if limit.reached(since, mono) => {
                    lid.tripped = true;
                    Some(minutes)
                }
                _ => None,
            }
        };

        let Some(limit_minutes) = reached else {
            return Vec::new();
        };
        self.apply_transition(Command::LidClosedOnBatteryLimit { limit_minutes }, now)
            .await
            .map(|(_, ended)| ended)
            .unwrap_or_default()
    }

    /// Takes each lock on its own, as `status` does, so it never holds both.
    async fn tick_snapshot(&self) -> TickSnapshot {
        let mut refused: Vec<RuleId> = self
            .rules
            .lock()
            .await
            .refused()
            .iter()
            .map(|(rule, _)| *rule)
            .collect();
        refused.sort_unstable();
        let mut sessions: Vec<SessionId> = self
            .inner
            .lock()
            .await
            .state
            .sessions()
            .iter()
            .map(|session| session.id)
            .collect();
        sessions.sort_unstable();
        TickSnapshot { sessions, refused }
    }

    /// Reports a battery reading, which ends sessions that watch for it.
    ///
    /// Returns the sessions it stopped. The history entry is already written.
    pub async fn report_battery(&self, percent: u8) -> Vec<BatteryStop> {
        let now = self.now();
        self.battery_level(percent, now)
            .await
            .iter()
            .filter_map(EndedSession::battery_stop)
            .collect()
    }

    async fn battery_level(&self, percent: u8, now: u64) -> Vec<EndedSession> {
        self.apply_transition(Command::BatteryLevel { percent }, now)
            .await
            .map(|(_, ended)| ended)
            .unwrap_or_default()
    }

    /// Whether this machine runs on a battery.
    ///
    /// This is what "the battery stop threshold is enabled by default on
    /// battery-powered devices" is decided from. The threshold itself is a
    /// default value — `SessionRequest::quick` and `Rule::new` both set it — and
    /// this is how a client knows whether to offer the control at all. A desktop
    /// is shown "not applicable" rather than a threshold that can never fire.
    pub async fn has_battery(&self) -> bool {
        self.rules.lock().await.has_battery()
    }

    /// Forces one full rule evaluation. Used by tests and at startup, where the
    /// point is to have an answer now rather than at the next tick.
    pub async fn evaluate_rules_now(&self) -> Vec<BatteryStop> {
        let now = self.now();
        self.reconcile_rules(now, true)
            .await
            .iter()
            .filter_map(EndedSession::battery_stop)
            .collect()
    }

    /// Asks the backend whether the lease is still in force and raises
    /// attention if it is not.
    pub async fn verify_lease(&self) {
        let lease_present = {
            let inner = self.inner.lock().await;
            inner.lease.is_some()
        };
        if !lease_present {
            return;
        }

        let health = {
            let inner = self.inner.lock().await;
            let Some((_, lease)) = &inner.lease else {
                return;
            };
            self.backend.verify(lease).await
        };

        match health {
            Ok(LeaseHealth::Held) => {}
            Ok(LeaseHealth::Lost) => {
                let now = self.now();
                {
                    let mut inner = self.inner.lock().await;
                    inner.lease = None;
                    let _ = inner.state.apply(
                        Command::InhibitorLost {
                            detail: "awake.backend.lease_lost".to_string(),
                        },
                        now,
                    );
                }
                // The user still wants the session, so try to take the lock
                // again rather than silently dropping what they asked for.
                self.reacquire().await;
            }
            Err(error) => {
                let now = self.now();
                let mut inner = self.inner.lock().await;
                let _ = inner.state.apply(
                    Command::InhibitorLost {
                        detail: error.to_string(),
                    },
                    now,
                );
            }
        }
    }

    /// Releases everything and records a clean shutdown, so the next run has
    /// nothing to explain.
    ///
    /// Returns the sessions it ended, so the clients can be told before the
    /// service leaves the bus.
    pub async fn shutdown(&self) -> Vec<EndedSession> {
        let now = self.now();
        let (effects, ended) = {
            let mut inner = self.inner.lock().await;
            transition(&mut inner, Command::Shutdown, now).unwrap_or_default()
        };
        self.reconcile(&effects).await;

        let mut inner = self.inner.lock().await;
        inner.persisted.run.last_seen_unix_seconds = now;
        inner.persisted.run.shut_down_at_unix_seconds = Some(now);
        inner.persisted.trim();
        let _ = self.store.save(&inner.persisted);
        ended
    }

    /// Whether the service is still holding an inhibitor. Used by tests and by
    /// the shutdown path's own assertion that nothing was left behind.
    pub async fn holds_inhibitor(&self) -> bool {
        self.inner.lock().await.lease.is_some()
    }

    pub async fn status(&self) -> StatusDocument {
        let now = self.now();

        // The rule lock is taken and released before the session lock, so a
        // status query never holds both at once and can never be the thing that
        // deadlocks against a tick.
        let (
            active_rule_names,
            rule_summary,
            suppression,
            conflicts,
            providers,
            battery_protection,
        ) = self.rule_surface(now).await;

        let lid_watch = *self.lid.lock().await;
        let inner = self.inner.lock().await;
        let capabilities = *self.capabilities.lock().await;
        let active_rules: Vec<WireActiveRule> = inner
            .state
            .sessions()
            .iter()
            .filter_map(|session| {
                let rule = session.rule?;
                let (name, priority) = active_rule_names
                    .iter()
                    .find(|(id, _, _)| *id == rule)
                    .map(|(_, name, priority)| (name.clone(), *priority))?;
                Some(WireActiveRule {
                    rule_id: rule.0,
                    name,
                    session_id: session.id.0,
                    priority,
                })
            })
            .collect();
        let effective = inner.state.effective_policy();
        let available = matches!(inner.state.backend(), BackendState::Available(_));

        StatusDocument {
            indicator: inner.state.indicator().into(),
            effective_policy: effective.policy,
            unmet_policy: inner.state.unmet_policy(),
            battery_stop_percent: effective.battery_stop_percent,
            sessions: inner
                .state
                .sessions()
                .iter()
                .map(|session| WireSession::from_session(session, now))
                .collect(),
            reasons: effective.reasons.iter().map(WireReason::from).collect(),
            backend: WireBackend {
                name: self.backend.name().to_string(),
                available,
                capabilities,
                detail: match inner.state.backend() {
                    BackendState::Unavailable(detail) => Some(detail.clone()),
                    BackendState::Available(_) => inner.backend_detail.clone(),
                },
            },
            attention: inner.state.attention().map(str::to_string),
            interrupted_previous_session: inner.interrupted.clone(),
            reduced_security_confirmed: inner.state.reduced_security_confirmed(),
            active_rules,
            rule_summary,
            rules_suppression: suppression,
            conflicts,
            providers,
            battery_protection: WireBatteryProtection {
                stop_below_percent: effective.battery_stop_percent,
                ..battery_protection
            },
            lid: WireLidProtection {
                battery_limit: lid_watch.limit,
                closed: lid_watch.closed,
                // Measured on the monotonic clock, reported on the wall clock
                // the client counts down against.
                stops_at_unix_seconds: if effective.policy.prevent_lid_suspend {
                    lid_watch
                        .closed_on_battery_since
                        .and_then(|since| lid_watch.limit.ends_at(since))
                        .map(|ends| {
                            now.saturating_add(ends.saturating_sub(self.clock.monotonic_seconds()))
                        })
                } else {
                    None
                },
            },
            now_unix_seconds: now,
        }
    }

    /// Everything the status needs from the rule side, gathered under the rule
    /// lock alone.
    #[allow(clippy::type_complexity)]
    async fn rule_surface(
        &self,
        now: u64,
    ) -> (
        Vec<(RuleId, String, u8)>,
        WireRuleSummary,
        Option<WireSuppression>,
        Vec<WireConflict>,
        Vec<WireProvider>,
        WireBatteryProtection,
    ) {
        let mut rules = self.rules.lock().await;
        // Evaluated against the readings already in hand rather than fresh ones:
        // a status query must not be a reason to walk `/proc`, or a client that
        // polls the status would defeat every cadence in the provider set.
        let observations = rules.observations().clone();
        let evaluation: Evaluation = rules.rules().evaluate(&observations, now);

        let names: Vec<(RuleId, String, u8)> = rules
            .rules()
            .rules()
            .iter()
            .map(|rule| (rule.id, rule.name.as_str().to_string(), rule.priority))
            .collect();

        let summary = WireRuleSummary {
            total: rules.rules().rules().len() as u32,
            enabled: rules.rules().enabled_rules().count() as u32,
            refused: rules.refused().len() as u32,
        };

        let conflicts = evaluation
            .conflicts
            .iter()
            .map(|conflict| WireConflict {
                field: conflict.field.as_key().to_string(),
                winner_rule_id: conflict.winner.0,
                winner_name: names
                    .iter()
                    .find(|(id, _, _)| *id == conflict.winner)
                    .map(|(_, name, _)| name.clone())
                    .unwrap_or_default(),
                overridden_rule_ids: conflict.overridden.iter().map(|rule| rule.0).collect(),
                resolution_key: conflict.resolution_key.to_string(),
            })
            .collect();

        let providers = rules
            .provider_reports(now)
            .into_iter()
            .map(|report| WireProvider {
                kind: report.kind,
                available: report.available,
                poll_seconds: report.cadence.poll_seconds(),
                explanation: report.explanation,
            })
            .collect();

        let battery = WireBatteryProtection {
            has_battery: rules.has_battery(),
            percent: rules.observations().battery_percent,
            on_ac_power: rules.observations().ac_power_connected,
            stop_below_percent: None,
        };

        (
            names,
            summary,
            evaluation.suppression.map(WireSuppression::from),
            conflicts,
            providers,
            battery,
        )
    }

    /// Brings the held inhibitor in line with what the sessions now need.
    async fn reconcile(&self, effects: &[Effect]) {
        if !effects
            .iter()
            .any(|effect| matches!(effect, Effect::PolicyChanged(_)))
        {
            return;
        }
        self.reacquire().await;
    }

    async fn reacquire(&self) {
        let (wanted, existing) = {
            let inner = self.inner.lock().await;
            let effective = inner.state.effective_policy();
            let why = merged_why(&inner.state);
            let capabilities = match inner.state.backend() {
                BackendState::Available(capabilities) => *capabilities,
                BackendState::Unavailable(_) => BackendCapabilities::NONE,
            };
            (
                LeaseRequest::from_policy(&effective.policy, &capabilities, INHIBITOR_WHO, &why),
                inner.lease.as_ref().map(|(request, _)| request.clone()),
            )
        };

        // Already holding exactly what is wanted, including the same reason
        // text, so there is nothing to churn.
        if wanted == existing {
            return;
        }

        // Nothing wanted: let go of what is held.
        let Some(request) = wanted else {
            if let Some((_, lease)) = self.inner.lock().await.lease.take() {
                let _ = self.backend.release(lease).await;
            }
            return;
        };

        // The new lock is taken before the old one is let go. Releasing first
        // leaves a moment with no `handle-lid-switch` lock held, and logind
        // rechecks a closed lid on every turn of its loop, so with the lid shut
        // that moment is enough to suspend the machine (a rule starting or
        // stopping beside a running session changes the lock's reason, which
        // is such a swap).
        let now = self.now();
        let (request, outcome) = match self.backend.acquire(&request).await {
            // polkit may allow `sleep` and `idle` and refuse the lid lock. The
            // same lock without the lid tells which: if that is granted, the
            // lid was what was refused, so the rest is held and the lid choice
            // is reported as unavailable rather than losing every lock over
            // it. If that is refused too, the refusal was not about the lid,
            // and it is reported as the lock failing, with the lid capability
            // left as it is.
            Err(BackendError::Denied(detail))
                if request.what.contains(&InhibitWhat::HandleLidSwitch) =>
            {
                let mut without = request.clone();
                without
                    .what
                    .retain(|what| *what != InhibitWhat::HandleLidSwitch);
                if without.what.is_empty() {
                    // The lid lock was all that was asked for.
                    eprintln!("better-awake-service: the lid lock was refused ({detail})");
                    self.disable_lid_capability(now).await;
                    if let Some((_, lease)) = self.inner.lock().await.lease.take() {
                        let _ = self.backend.release(lease).await;
                    }
                    return;
                }
                match self.backend.acquire(&without).await {
                    Ok(lease) => {
                        eprintln!(
                            "better-awake-service: the lid lock was refused ({detail}), so \
                             sessions hold sleep and idle without it"
                        );
                        self.disable_lid_capability(now).await;
                        (without, Ok(lease))
                    }
                    Err(_) => (request, Err(BackendError::Denied(detail))),
                }
            }
            outcome => (request, outcome),
        };

        match outcome {
            Ok(lease) => {
                let previous = {
                    let mut inner = self.inner.lock().await;
                    let previous = inner.lease.replace((request, lease));
                    let capabilities = inner.state.backend().clone();
                    if let BackendState::Available(capabilities) = capabilities {
                        let _ = inner
                            .state
                            .apply(Command::BackendAvailable(capabilities), now);
                    }
                    previous
                };
                if let Some((_, lease)) = previous {
                    let _ = self.backend.release(lease).await;
                }
            }
            Err(error) => {
                // The new lock could not be taken. The old one no longer says
                // what is wanted, but it still holds the machine as it did a
                // moment ago, which is better than nothing while attention is
                // raised and every tick tries again. Except the lid: an old
                // lock holding the lid that is no longer wanted would keep the
                // machine awake with the lid shut against the person's choice,
                // so that one is let go.
                let unwanted_lid = {
                    let mut inner = self.inner.lock().await;
                    let _ = inner.state.apply(
                        Command::InhibitorLost {
                            detail: error.to_string(),
                        },
                        now,
                    );
                    let holds_lid = inner
                        .lease
                        .as_ref()
                        .is_some_and(|(held, _)| held.what.contains(&InhibitWhat::HandleLidSwitch));
                    if holds_lid && !request.what.contains(&InhibitWhat::HandleLidSwitch) {
                        inner.lease.take()
                    } else {
                        None
                    }
                };
                if let Some((_, lease)) = unwanted_lid {
                    let _ = self.backend.release(lease).await;
                }
            }
        }
    }

    /// The backend refused the lid lock: from now until the service restarts
    /// it is reported as a capability the backend does not have.
    async fn disable_lid_capability(&self, now: u64) {
        let mut capabilities = self.capabilities.lock().await;
        capabilities.lid_switch = false;
        let mut inner = self.inner.lock().await;
        if let BackendState::Available(_) = inner.state.backend() {
            let _ = inner
                .state
                .apply(Command::BackendAvailable(*capabilities), now);
        }
    }

    async fn persist(&self) {
        let now = self.now();
        let mut inner = self.inner.lock().await;
        inner.persisted.run.last_seen_unix_seconds = now;
        inner.persisted.reduced_security_confirmed = inner.state.reduced_security_confirmed();
        inner.persisted.trim();
        let _ = self.store.save(&inner.persisted);
    }
}

/// Applies one command under the session lock, mirrors it into the persisted
/// record, and names the sessions it ended.
///
/// The thresholds are read before the command runs, because by the time the
/// state machine reports a session ended it no longer holds it.
fn transition<L>(
    inner: &mut Inner<L>,
    command: Command,
    now: u64,
) -> Result<(Vec<Effect>, Vec<EndedSession>), TransitionError> {
    let thresholds: Vec<(SessionId, Option<u8>)> = inner
        .state
        .sessions()
        .iter()
        .map(|session| (session.id, session.battery_stop_percent))
        .collect();
    let effects = inner.state.apply(command, now)?;
    record(inner, &effects, now);
    let ended = effects
        .iter()
        .filter_map(|effect| match effect {
            Effect::SessionEnded { session, cause } => Some(EndedSession {
                session: *session,
                cause: *cause,
                battery_stop_percent: thresholds
                    .iter()
                    .find(|(id, _)| id == session)
                    .and_then(|(_, threshold)| *threshold),
            }),
            _ => None,
        })
        .collect();
    Ok((effects, ended))
}

/// Mirrors the effects into the persisted record, so a crash right after a
/// change still leaves a file that explains what was running.
fn record<L>(inner: &mut Inner<L>, effects: &[Effect], now: u64) {
    for effect in effects {
        match effect {
            Effect::SessionStarted(id) => {
                if let Some(session) = inner.state.session(*id) {
                    inner.persisted.sessions.push(persist(session));
                }
            }
            Effect::SessionEnded { session, cause } => {
                if let Some(record) = inner
                    .persisted
                    .sessions
                    .iter_mut()
                    .find(|record| record.session_id == session.0 && record.is_running())
                {
                    record.ended_at_unix_seconds = Some(now);
                    record.end_cause = Some(cause.as_key().to_string());
                }
            }
            // A refused trigger never became a session, so there is nothing in
            // the persisted record to update. It reaches the client through the
            // status instead.
            Effect::PolicyChanged(_)
            | Effect::AttentionRaised(_)
            | Effect::AttentionCleared
            | Effect::TriggerRefused { .. } => {}
        }
    }
    // A Change rewrites a running session, so the record follows it.
    for session in inner.state.sessions() {
        if let Some(record) = inner
            .persisted
            .sessions
            .iter_mut()
            .find(|record| record.session_id == session.id.0 && record.is_running())
        {
            record.reason = session.reason.as_str().to_string();
            record.policy = session.policy;
            record.battery_stop_percent = session.battery_stop_percent;
            record.end = session.end;
        }
    }
}

/// Translates a validated request into a rule edit, or `None` when the request
/// is not one.
///
/// Kept as a free function so the mapping from wire to edit is one readable
/// table rather than a long arm inside `handle`.
fn rule_edit_for(body: &RequestBody, now: u64) -> Option<RuleEdit> {
    Some(match body {
        RequestBody::CreateRule { rule } => RuleEdit::Create(rule.clone()),
        RequestBody::UpdateRule { rule_id, rule } => RuleEdit::Update {
            id: RuleId(*rule_id),
            rule: rule.clone(),
        },
        RequestBody::DeleteRule { rule_id } => RuleEdit::Delete(RuleId(*rule_id)),
        RequestBody::SetRuleEnabled { rule_id, enabled } => RuleEdit::SetEnabled {
            id: RuleId(*rule_id),
            enabled: *enabled,
        },
        RequestBody::DuplicateRule { rule_id } => RuleEdit::Duplicate(RuleId(*rule_id)),
        RequestBody::ReorderRule { rule_id, to_index } => RuleEdit::Reorder {
            id: RuleId(*rule_id),
            to_index: *to_index as usize,
        },
        RequestBody::SetRulePriority { rule_id, priority } => RuleEdit::SetPriority {
            id: RuleId(*rule_id),
            priority: *priority,
        },
        RequestBody::PauseRules { seconds } => RuleEdit::Pause {
            seconds: *seconds,
            now,
        },
        RequestBody::ResumeRules => RuleEdit::Resume,
        RequestBody::OverrideAllRules { confirmed } => RuleEdit::OverrideAll {
            confirmed: *confirmed,
        },
        _ => return None,
    })
}

fn persist(session: &Session) -> PersistedSession {
    PersistedSession {
        session_id: session.id.0,
        reason: session.reason.as_str().to_string(),
        origin: session.origin,
        policy: session.policy,
        battery_stop_percent: session.battery_stop_percent,
        end: session.end,
        started_at_unix_seconds: session.started_at_unix_seconds,
        ended_at_unix_seconds: None,
        end_cause: None,
    }
}

/// The reason handed to the inhibitor backend. One session's reason is used as
/// written; several are counted, because a logind inhibitor line is not the
/// place to paste an unbounded list.
fn merged_why(state: &AwakeState) -> String {
    let sessions = state.sessions();
    match sessions {
        [] => String::new(),
        [only] => only.reason.as_str().to_string(),
        many => format!("{} active reasons", many.len()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::{BackendError, FakeInhibitorBackend, FixedClock, InhibitWhat};
    use awake_ipc::{ResponseBody, WireEnd, WireIndicator};

    const NOW: u64 = 1_700_000_000;

    struct Fixture {
        engine: AwakeEngine<FakeInhibitorBackend>,
        clock: Arc<FixedClock>,
        _directory: tempfile::TempDir,
        store: JsonStore,
        /// The captured `/proc` and `/sys` tree the providers read, so a test
        /// can unplug a charger or flatten a battery by writing a file.
        roots: awake_platform::Roots,
    }

    impl Fixture {
        /// Sets the battery percentage the provider will report.
        fn set_battery(&self, percent: u8) {
            std::fs::write(
                self.roots.sys_path("class/power_supply/BAT1/capacity"),
                format!("{percent}\n"),
            )
            .unwrap();
        }

        fn set_on_ac(&self, online: bool) {
            std::fs::write(
                self.roots.sys_path("class/power_supply/ACAD/online"),
                if online { "1\n" } else { "0\n" },
            )
            .unwrap();
        }

        async fn create_rule(&self, rule: awake_core::Rule) -> u64 {
            let response = self
                .engine
                .handle(AwakeRequest::new(RequestBody::CreateRule {
                    rule: Box::new(rule),
                }))
                .await;
            match &response.body {
                ResponseBody::Rules(rules) => rules.rules.last().unwrap().id.0,
                other => panic!("expected a rules reply, got {other:?}"),
            }
        }
    }

    /// A machine on the charger with a healthy battery and nothing running.
    fn write_machine(root: &std::path::Path) {
        let sys = root.join("sys");
        let proc = root.join("proc");
        std::fs::create_dir_all(&proc).unwrap();
        awake_platform::power::write_supply(&sys, "ACAD", &[("type", "Mains"), ("online", "1")]);
        awake_platform::power::write_supply(
            &sys,
            "BAT1",
            &[("type", "Battery"), ("capacity", "80")],
        );
        awake_platform::display::write_connector(&sys, "card1-eDP-1", "connected");
        awake_platform::display::write_connector(&sys, "card1-HDMI-A-1", "disconnected");
        awake_platform::process::write_process(&proc, 1, "systemd", None);
    }

    async fn fixture_with(backend: FakeInhibitorBackend) -> Fixture {
        let directory = tempfile::tempdir().unwrap();
        write_machine(directory.path());
        let roots = awake_platform::Roots::at(directory.path());
        let store = JsonStore::at_path(directory.path().join("state.json"));
        let clock = Arc::new(FixedClock::at(NOW));
        let driver = crate::rules::RuleDriver::load(
            awake_store::rules::RulesStore::at_path(directory.path().join("rules.json")),
            awake_store::history::HistoryStore::at_path(directory.path().join("history.json")),
            roots.clone(),
        );
        let engine =
            AwakeEngine::start_with_rules(backend, store.clone(), clock.clone(), driver).await;
        Fixture {
            engine,
            clock,
            _directory: directory,
            store,
            roots,
        }
    }

    async fn fixture() -> Fixture {
        fixture_with(FakeInhibitorBackend::logind_shaped()).await
    }

    /// A second run of the service over the files a fixture's run left in
    /// `directory`, the way the next login finds them.
    async fn restart(
        directory: &std::path::Path,
        store: &JsonStore,
        clock: Arc<FixedClock>,
    ) -> AwakeEngine<FakeInhibitorBackend> {
        let driver = crate::rules::RuleDriver::load(
            awake_store::rules::RulesStore::at_path(directory.join("rules.json")),
            awake_store::history::HistoryStore::at_path(directory.join("history.json")),
            awake_platform::Roots::at(directory),
        );
        AwakeEngine::start_with_rules(
            FakeInhibitorBackend::logind_shaped(),
            store.clone(),
            clock,
            driver,
        )
        .await
    }

    /// A rule that matches while the charger is plugged in, which the fixture
    /// machine's `/sys` tree says it is.
    fn on_ac_rule(name: &str) -> awake_core::Rule {
        use awake_core::{Combine, Condition, ConditionGroup, Reason, Rule, RuleId};
        Rule::new(
            RuleId(0),
            Reason::new(name).unwrap(),
            Combine::All,
            [ConditionGroup::one(Condition::AcPower { connected: true }).unwrap()],
        )
        .unwrap()
    }

    fn start(end: WireEnd) -> AwakeRequest {
        AwakeRequest::new(RequestBody::StartSession {
            reason: "Android Studio build is running".to_string(),
            policy: awake_core::SessionPolicy::quick_default(),
            battery_stop_percent: Some(20),
            end,
            security_confirmed: false,
        })
    }

    fn status_of(response: &AwakeResponse) -> &StatusDocument {
        match &response.body {
            ResponseBody::Status(status) => status,
            ResponseBody::Rejected { error_key } => panic!("unexpected rejection: {error_key}"),
            other => panic!("expected a status, got {other:?}"),
        }
    }

    fn rejection_of(response: &AwakeResponse) -> &str {
        match &response.body {
            ResponseBody::Rejected { error_key } => error_key,
            other => panic!("expected a rejection, got {other:?}"),
        }
    }

    fn rules_of(response: &AwakeResponse) -> &RulesDocument {
        match &response.body {
            ResponseBody::Rules(rules) => rules,
            other => panic!("expected a rules reply, got {other:?}"),
        }
    }

    fn test_of(response: &AwakeResponse) -> &RuleTestDocument {
        match &response.body {
            ResponseBody::RuleTest(test) => test,
            other => panic!("expected a rule test, got {other:?}"),
        }
    }

    fn history_of(response: &AwakeResponse) -> &HistoryDocument {
        match &response.body {
            ResponseBody::History(history) => history,
            other => panic!("expected a history reply, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_fresh_service_holds_nothing_and_reports_its_backend() {
        let fixture = fixture().await;
        let status = fixture.engine.status().await;

        assert_eq!(status.indicator, WireIndicator::Inactive);
        assert!(status.sessions.is_empty());
        assert!(status.backend.available);
        assert_eq!(status.backend.name, "fake");
        assert!(!fixture.engine.holds_inhibitor().await);
    }

    #[tokio::test]
    async fn starting_a_quick_session_takes_a_sleep_and_idle_inhibitor() {
        let backend = FakeInhibitorBackend::logind_shaped();
        let fixture = fixture_with(backend).await;

        let response = fixture.engine.handle(start(WireEnd::Indefinite)).await;
        let status = status_of(&response);

        assert_eq!(status.indicator, WireIndicator::ActiveManual);
        assert_eq!(status.sessions.len(), 1);
        assert!(status.effective_policy.prevent_system_suspend);
        assert!(!status.effective_policy.prevent_display_sleep);
        assert_eq!(status.battery_stop_percent, Some(20));
        assert!(fixture.engine.holds_inhibitor().await);
    }

    #[tokio::test]
    async fn the_reason_the_user_typed_is_what_reaches_the_backend() {
        let fixture = fixture().await;
        fixture.engine.handle(start(WireEnd::Indefinite)).await;

        let acquired = fixture.engine.backend.acquired();
        assert_eq!(acquired.len(), 1);
        assert_eq!(acquired[0].who, INHIBITOR_WHO);
        assert_eq!(acquired[0].why, "Android Studio build is running");
        assert_eq!(
            acquired[0].what,
            vec![InhibitWhat::Sleep, InhibitWhat::Idle]
        );
    }

    #[tokio::test]
    async fn a_second_manual_session_is_refused_and_the_first_keeps_its_inhibitor() {
        let fixture = fixture().await;
        fixture.engine.handle(start(WireEnd::Indefinite)).await;

        let response = fixture
            .engine
            .handle(start(WireEnd::Duration { seconds: 900 }))
            .await;

        assert!(
            rejection_of(&response).starts_with("awake.error.manual_session_already_active"),
            "unexpected rejection: {}",
            rejection_of(&response)
        );
        let status = fixture.engine.status().await;
        assert_eq!(status.sessions.len(), 1);
        assert_eq!(status.sessions[0].end, WireEnd::Indefinite);
        assert_eq!(fixture.engine.backend.held_count(), 1);
    }

    #[tokio::test]
    async fn a_session_that_would_stop_the_screen_locking_needs_confirmation_first() {
        let fixture = fixture().await;
        let request = |confirmed| {
            AwakeRequest::new(RequestBody::StartSession {
                reason: "Presenting".to_string(),
                policy: awake_core::SessionPolicy {
                    prevent_automatic_lock: true,
                    ..awake_core::SessionPolicy::quick_default()
                },
                battery_stop_percent: Some(20),
                end: WireEnd::Indefinite,
                security_confirmed: confirmed,
            })
        };

        let refused = fixture.engine.handle(request(false)).await;
        assert_eq!(
            rejection_of(&refused),
            "awake.error.security_confirmation_required"
        );
        assert!(!fixture.engine.holds_inhibitor().await);

        let accepted = fixture.engine.handle(request(true)).await;
        assert!(status_of(&accepted).reduced_security_confirmed);
        // logind cannot hold the lock off, so the menu is told rather than
        // shown a policy that is not in force.
        assert_eq!(
            status_of(&accepted).unmet_policy,
            vec![awake_core::PolicyGap::AutomaticLock]
        );
    }

    #[tokio::test]
    async fn a_timed_session_expires_on_a_tick_and_releases_the_inhibitor() {
        let fixture = fixture().await;
        fixture
            .engine
            .handle(start(WireEnd::Duration { seconds: 900 }))
            .await;

        fixture.clock.advance(899);
        fixture.engine.tick().await;
        assert!(fixture.engine.holds_inhibitor().await);

        fixture.clock.advance(1);
        fixture.engine.tick().await;
        assert!(!fixture.engine.holds_inhibitor().await);
        assert_eq!(fixture.engine.backend.held_count(), 0);
        assert_eq!(
            fixture.engine.status().await.indicator,
            WireIndicator::Inactive
        );
    }

    #[tokio::test]
    async fn extending_a_session_pushes_its_expiry_past_the_tick_that_would_have_ended_it() {
        let fixture = fixture().await;
        let response = fixture
            .engine
            .handle(start(WireEnd::Duration { seconds: 900 }))
            .await;
        let session_id = status_of(&response).sessions[0].session_id;

        fixture.clock.advance(800);
        fixture
            .engine
            .handle(AwakeRequest::new(RequestBody::ExtendSession {
                session_id,
                by_seconds: 900,
            }))
            .await;

        fixture.clock.set(NOW + 1_700);
        fixture.engine.tick().await;
        assert!(fixture.engine.holds_inhibitor().await);

        fixture.clock.set(NOW + 1_800);
        fixture.engine.tick().await;
        assert!(!fixture.engine.holds_inhibitor().await);
    }

    #[tokio::test]
    async fn extending_an_indefinite_session_is_refused_rather_than_ignored() {
        let fixture = fixture().await;
        let response = fixture.engine.handle(start(WireEnd::Indefinite)).await;
        let session_id = status_of(&response).sessions[0].session_id;

        let refused = fixture
            .engine
            .handle(AwakeRequest::new(RequestBody::ExtendSession {
                session_id,
                by_seconds: 900,
            }))
            .await;
        assert_eq!(
            rejection_of(&refused),
            "awake.error.end_condition:awake.error.cannot_extend_indefinite"
        );
    }

    #[tokio::test]
    async fn a_battery_reading_below_the_threshold_ends_the_session_and_releases_the_lock() {
        let fixture = fixture().await;
        fixture.engine.handle(start(WireEnd::Indefinite)).await;

        fixture.engine.report_battery(20).await;
        assert!(fixture.engine.holds_inhibitor().await);

        fixture.engine.report_battery(19).await;
        assert!(!fixture.engine.holds_inhibitor().await);
        assert_eq!(fixture.engine.backend.released().len(), 1);
    }

    #[tokio::test]
    async fn a_session_that_opts_out_of_battery_protection_survives_a_flat_battery() {
        let fixture = fixture().await;
        fixture
            .engine
            .handle(AwakeRequest::new(RequestBody::StartSession {
                reason: "Long export".to_string(),
                policy: awake_core::SessionPolicy::quick_default(),
                battery_stop_percent: None,
                end: WireEnd::Indefinite,
                security_confirmed: false,
            }))
            .await;

        fixture.engine.report_battery(1).await;
        assert!(fixture.engine.holds_inhibitor().await);
        assert_eq!(fixture.engine.status().await.battery_stop_percent, None);
    }

    #[tokio::test]
    async fn an_inhibitor_lost_behind_our_back_is_noticed_and_retaken() {
        let fixture = fixture().await;
        fixture.engine.handle(start(WireEnd::Indefinite)).await;

        fixture.engine.backend.drop_lease_behind_our_back();
        fixture.engine.tick().await;

        // The session is still what the user asked for, so the service takes
        // the lock again rather than quietly leaving the machine unprotected.
        assert!(fixture.engine.holds_inhibitor().await);
        assert_eq!(fixture.engine.backend.acquired().len(), 2);
        assert_eq!(fixture.engine.status().await.sessions.len(), 1);
    }

    #[tokio::test]
    async fn a_backend_that_refuses_the_lock_leaves_the_tray_asking_for_attention() {
        let fixture = fixture().await;
        fixture
            .engine
            .backend
            .fail_next_acquire(BackendError::Denied("no".to_string()));

        let response = fixture.engine.handle(start(WireEnd::Indefinite)).await;

        assert_eq!(
            status_of(&response).indicator,
            WireIndicator::AttentionRequired
        );
        assert!(!fixture.engine.holds_inhibitor().await);
    }

    #[tokio::test]
    async fn a_service_with_no_backend_refuses_to_start_a_session_it_cannot_enforce() {
        let fixture = fixture_with(FakeInhibitorBackend::failing_probe(
            BackendError::Unavailable("no logind".to_string()),
        ))
        .await;

        let response = fixture.engine.handle(start(WireEnd::Indefinite)).await;
        assert_eq!(rejection_of(&response), "awake.error.backend_unavailable");

        let status = fixture.engine.status().await;
        assert_eq!(status.indicator, WireIndicator::Unavailable);
        assert!(!status.backend.available);
        assert_eq!(
            status.backend.detail.as_deref(),
            Some("awake.backend.unavailable:no logind")
        );
    }

    #[tokio::test]
    async fn ending_a_session_that_is_not_there_changes_nothing() {
        let fixture = fixture().await;
        fixture.engine.handle(start(WireEnd::Indefinite)).await;

        let refused = fixture
            .engine
            .handle(AwakeRequest::new(RequestBody::EndSession {
                session_id: 4_242,
            }))
            .await;

        assert_eq!(rejection_of(&refused), "awake.error.unknown_session:4242");
        assert!(fixture.engine.holds_inhibitor().await);
    }

    #[tokio::test]
    async fn changing_a_session_replaces_its_reason_end_and_battery_rule() {
        let fixture = fixture().await;
        let response = fixture.engine.handle(start(WireEnd::Indefinite)).await;
        let session_id = status_of(&response).sessions[0].session_id;

        fixture.clock.advance(3_600);
        let changed = fixture
            .engine
            .handle(AwakeRequest::new(RequestBody::ChangeSession {
                session_id,
                reason: "Rendering".to_string(),
                policy: awake_core::SessionPolicy::quick_default(),
                battery_stop_percent: Some(30),
                end: WireEnd::Duration { seconds: 600 },
                security_confirmed: false,
            }))
            .await;

        let status = status_of(&changed);
        assert_eq!(status.sessions[0].reason, "Rendering");
        assert_eq!(status.battery_stop_percent, Some(30));
        assert_eq!(
            status.sessions[0].end,
            WireEnd::UntilUnixSeconds {
                unix_seconds: NOW + 3_600 + 600
            },
            "a new duration is measured from now, not from a start an hour ago"
        );
        // The backend was told the new reason rather than left with the old.
        assert_eq!(
            fixture.engine.backend.acquired().last().unwrap().why,
            "Rendering"
        );
    }

    #[tokio::test]
    async fn a_shutdown_releases_every_inhibitor_and_leaves_nothing_to_explain() {
        let fixture = fixture().await;
        fixture.engine.handle(start(WireEnd::Indefinite)).await;

        fixture.engine.shutdown().await;

        assert!(!fixture.engine.holds_inhibitor().await);
        assert_eq!(fixture.engine.backend.held_count(), 0);
        assert_eq!(fixture.engine.backend.released().len(), 1);

        // The next run has nothing to report, because this one said goodbye.
        let outcome = fixture.store.load(NOW + 10).unwrap();
        assert!(outcome.interrupted.is_empty());
    }

    #[tokio::test]
    async fn a_service_that_died_mid_session_is_explained_by_the_next_one() {
        let fixture = fixture().await;
        fixture.engine.handle(start(WireEnd::Indefinite)).await;
        // No shutdown: this is what a crash leaves behind.
        drop(fixture.engine);

        let next = restart(
            fixture._directory.path(),
            &fixture.store,
            Arc::new(FixedClock::at(NOW + 5_000)),
        )
        .await;

        let status = next.status().await;
        let interrupted = status
            .interrupted_previous_session
            .expect("the crashed run must be explained");
        assert_eq!(interrupted.reason, "Android Studio build is running");
        assert_eq!(interrupted.started_at_unix_seconds, NOW);
        // And it is not resurrected: nothing is holding the machine awake now.
        assert!(status.sessions.is_empty());
        assert!(!next.holds_inhibitor().await);
    }

    #[tokio::test]
    async fn the_first_time_security_acknowledgement_survives_a_restart() {
        let fixture = fixture().await;
        fixture
            .engine
            .handle(AwakeRequest::new(RequestBody::StartSession {
                reason: "Presenting".to_string(),
                policy: awake_core::SessionPolicy {
                    prevent_automatic_lock: true,
                    ..awake_core::SessionPolicy::quick_default()
                },
                battery_stop_percent: Some(20),
                end: WireEnd::Indefinite,
                security_confirmed: true,
            }))
            .await;
        fixture.engine.shutdown().await;

        let next = restart(
            fixture._directory.path(),
            &fixture.store,
            Arc::new(FixedClock::at(NOW + 100)),
        )
        .await;
        assert!(next.status().await.reduced_security_confirmed);
    }

    // ---- Automatic rules, end to end --------------------------------------

    #[tokio::test]
    async fn a_matching_rule_takes_the_inhibitor_with_no_manual_session_involved() {
        let fixture = fixture().await;
        let rule_id = fixture.create_rule(on_ac_rule("Charging")).await;

        let status = fixture.engine.status().await;
        assert_eq!(status.indicator, WireIndicator::ActiveTrigger);
        assert_eq!(status.active_rules.len(), 1);
        assert_eq!(status.active_rules[0].rule_id, rule_id);
        assert_eq!(status.active_rules[0].name, "Charging");
        assert!(fixture.engine.holds_inhibitor().await);
        assert_eq!(
            fixture.engine.backend.acquired()[0].why,
            "Charging",
            "the rule's own name is what reaches the inhibitor backend"
        );
    }

    #[tokio::test]
    async fn a_rule_that_stops_matching_releases_what_it_was_holding() {
        let fixture = fixture().await;
        fixture.create_rule(on_ac_rule("Charging")).await;
        assert!(fixture.engine.holds_inhibitor().await);

        fixture.set_on_ac(false);
        fixture.clock.advance(60);
        fixture.engine.tick().await;

        assert!(!fixture.engine.holds_inhibitor().await);
        assert_eq!(
            fixture.engine.status().await.indicator,
            WireIndicator::Inactive
        );
    }

    #[tokio::test]
    async fn ending_a_manual_session_leaves_the_rules_session_holding_the_machine() {
        let fixture = fixture().await;
        fixture.create_rule(on_ac_rule("Charging")).await;
        fixture.engine.handle(start(WireEnd::Indefinite)).await;
        assert_eq!(fixture.engine.status().await.sessions.len(), 2);

        let response = fixture
            .engine
            .handle(AwakeRequest::new(RequestBody::EndManualSession))
            .await;

        let status = status_of(&response);
        assert_eq!(status.sessions.len(), 1);
        assert_eq!(status.active_rules.len(), 1);
        assert!(
            fixture.engine.holds_inhibitor().await,
            "the rule still says the machine must stay awake"
        );
        assert_eq!(status.indicator, WireIndicator::ActiveTrigger);
    }

    #[tokio::test]
    async fn two_active_reasons_merge_and_ending_one_leaves_the_other_explaining_the_machine() {
        let fixture = fixture().await;
        fixture.create_rule(on_ac_rule("Charging")).await;
        fixture.engine.handle(start(WireEnd::Indefinite)).await;

        let status = fixture.engine.status().await;
        assert_eq!(status.reasons.len(), 2);
        assert_eq!(
            fixture.engine.backend.acquired().last().unwrap().why,
            "2 active reasons"
        );

        fixture
            .engine
            .handle(AwakeRequest::new(RequestBody::EndManualSession))
            .await;
        let status = fixture.engine.status().await;
        assert_eq!(status.reasons.len(), 1);
        assert_eq!(status.reasons[0].reason, "Charging");
    }

    #[tokio::test]
    async fn pausing_the_rules_releases_their_sessions_and_resuming_takes_them_back() {
        let fixture = fixture().await;
        fixture.create_rule(on_ac_rule("Charging")).await;
        assert!(fixture.engine.holds_inhibitor().await);

        let paused = fixture
            .engine
            .handle(AwakeRequest::new(RequestBody::PauseRules {
                seconds: Some(awake_core::PAUSE_SHORT_SECONDS),
            }))
            .await;
        let status = status_of(&paused);
        assert_eq!(
            status.rules_suppression,
            Some(awake_ipc::WireSuppression::PausedUntil {
                unix_seconds: NOW + awake_core::PAUSE_SHORT_SECONDS
            })
        );
        assert_eq!(status.indicator, WireIndicator::PausedRules);
        assert!(!fixture.engine.holds_inhibitor().await);

        let resumed = fixture
            .engine
            .handle(AwakeRequest::new(RequestBody::ResumeRules))
            .await;
        assert_eq!(status_of(&resumed).rules_suppression, None);
        assert!(fixture.engine.holds_inhibitor().await);
    }

    #[tokio::test]
    async fn a_pause_ends_by_itself_and_the_rules_take_hold_again() {
        let fixture = fixture().await;
        fixture.create_rule(on_ac_rule("Charging")).await;
        fixture
            .engine
            .handle(AwakeRequest::new(RequestBody::PauseRules {
                seconds: Some(awake_core::PAUSE_SHORT_SECONDS),
            }))
            .await;
        assert!(!fixture.engine.holds_inhibitor().await);

        fixture.clock.advance(awake_core::PAUSE_SHORT_SECONDS);
        fixture.engine.tick().await;

        assert!(fixture.engine.holds_inhibitor().await);
        assert_eq!(fixture.engine.status().await.rules_suppression, None);
    }

    #[tokio::test]
    async fn overriding_every_rule_is_refused_without_a_confirmation() {
        let fixture = fixture().await;
        fixture.create_rule(on_ac_rule("Charging")).await;

        let refused = fixture
            .engine
            .handle(AwakeRequest::new(RequestBody::OverrideAllRules {
                confirmed: false,
            }))
            .await;
        assert_eq!(
            rejection_of(&refused),
            "awake.rule.error.override_confirmation_required"
        );
        assert!(
            fixture.engine.holds_inhibitor().await,
            "a refused override must change nothing"
        );

        let accepted = fixture
            .engine
            .handle(AwakeRequest::new(RequestBody::OverrideAllRules {
                confirmed: true,
            }))
            .await;
        assert_eq!(
            status_of(&accepted).rules_suppression,
            Some(awake_ipc::WireSuppression::Overridden)
        );
        assert!(!fixture.engine.holds_inhibitor().await);
    }

    #[tokio::test]
    async fn a_disabled_rule_holds_nothing_and_enabling_it_takes_hold_at_once() {
        let fixture = fixture().await;
        let rule_id = fixture.create_rule(on_ac_rule("Charging")).await;

        fixture
            .engine
            .handle(AwakeRequest::new(RequestBody::SetRuleEnabled {
                rule_id,
                enabled: false,
            }))
            .await;
        assert!(!fixture.engine.holds_inhibitor().await);

        fixture
            .engine
            .handle(AwakeRequest::new(RequestBody::SetRuleEnabled {
                rule_id,
                enabled: true,
            }))
            .await;
        assert!(
            fixture.engine.holds_inhibitor().await,
            "a rule switched on must act now, not at the next tick"
        );
    }

    #[tokio::test]
    async fn a_rule_can_be_duplicated_reordered_and_deleted() {
        let fixture = fixture().await;
        let first = fixture.create_rule(on_ac_rule("First")).await;
        let second = fixture.create_rule(on_ac_rule("Second")).await;

        let duplicated = fixture
            .engine
            .handle(AwakeRequest::new(RequestBody::DuplicateRule {
                rule_id: first,
            }))
            .await;
        let rules = rules_of(&duplicated);
        assert_eq!(rules.rules.len(), 3);
        assert!(
            !rules.rules[1].enabled,
            "a duplicate starts off so it can be edited before it acts"
        );

        let reordered = fixture
            .engine
            .handle(AwakeRequest::new(RequestBody::ReorderRule {
                rule_id: second,
                to_index: 0,
            }))
            .await;
        assert_eq!(rules_of(&reordered).rules[0].id.0, second);

        let deleted = fixture
            .engine
            .handle(AwakeRequest::new(RequestBody::DeleteRule {
                rule_id: second,
            }))
            .await;
        assert_eq!(rules_of(&deleted).rules.len(), 2);
    }

    #[tokio::test]
    async fn testing_a_rule_reports_what_it_would_do_and_acquires_nothing() {
        let fixture = fixture().await;
        let rule_id = fixture.create_rule(on_ac_rule("Charging")).await;
        // Switch it off, so nothing this test does could be holding a lock for
        // any other reason.
        fixture
            .engine
            .handle(AwakeRequest::new(RequestBody::SetRuleEnabled {
                rule_id,
                enabled: false,
            }))
            .await;
        assert!(!fixture.engine.holds_inhibitor().await);
        let acquisitions_before = fixture.engine.backend.acquired().len();

        let response = fixture
            .engine
            .handle(AwakeRequest::new(RequestBody::TestRule { rule_id }))
            .await;

        let test = test_of(&response);
        assert_eq!(test.truth, awake_ipc::WireTruth::True);
        assert!(test.would_be_active);
        assert!(test.rule_disabled);
        assert!(
            !fixture.engine.holds_inhibitor().await,
            "testing a rule must never acquire an inhibitor"
        );
        assert_eq!(
            fixture.engine.backend.acquired().len(),
            acquisitions_before,
            "and it must not even ask the backend"
        );
    }

    #[tokio::test]
    async fn testing_a_rule_whose_provider_is_unavailable_explains_which_one() {
        use awake_core::{Combine, Condition, ConditionGroup, Reason, Rule, RuleId};
        let fixture = fixture().await;
        let rule_id = fixture
            .create_rule(
                Rule::new(
                    RuleId(0),
                    Reason::new("Presenting").unwrap(),
                    Combine::All,
                    [ConditionGroup::one(Condition::Fullscreen { active: true }).unwrap()],
                )
                .unwrap(),
            )
            .await;

        let response = fixture
            .engine
            .handle(AwakeRequest::new(RequestBody::TestRule { rule_id }))
            .await;
        let test = test_of(&response);

        assert_eq!(test.truth, awake_ipc::WireTruth::Unknown);
        assert!(!test.would_be_active);
        assert_eq!(test.unavailable_providers.len(), 1);
        assert_eq!(
            test.unavailable_providers[0].kind,
            awake_core::ProviderKind::Fullscreen
        );
        assert_eq!(
            test.unavailable_providers[0].explanation.as_deref(),
            Some(awake_platform::FULLSCREEN_UNAVAILABLE),
            "an undetectable provider must explain itself rather than read as no"
        );
        assert!(!fixture.engine.holds_inhibitor().await);
    }

    #[tokio::test]
    async fn a_rule_asking_for_something_unconfirmed_is_refused_and_counted_rather_than_dropped() {
        use awake_core::SessionPolicy;
        let fixture = fixture().await;
        let mut unlocking = on_ac_rule("Presenting");
        unlocking.policy = SessionPolicy {
            prevent_automatic_lock: true,
            ..SessionPolicy::quick_default()
        };
        fixture.create_rule(unlocking).await;

        let status = fixture.engine.status().await;
        assert!(status.active_rules.is_empty());
        assert_eq!(
            status.rule_summary.refused, 1,
            "a rule that silently never fires is the worst outcome available"
        );
        assert!(!fixture.engine.holds_inhibitor().await);
    }

    #[tokio::test]
    async fn the_status_reports_every_provider_with_its_cadence_or_its_reason() {
        let fixture = fixture().await;
        let status = fixture.engine.status().await;

        assert_eq!(status.providers.len(), awake_core::ProviderKind::ALL.len());
        for provider in &status.providers {
            assert_eq!(
                provider.available,
                provider.explanation.is_none(),
                "{:?} must either work or say why not",
                provider.kind
            );
        }
        let power = status
            .providers
            .iter()
            .find(|provider| provider.kind == awake_core::ProviderKind::AcPower)
            .unwrap();
        assert_eq!(power.poll_seconds, Some(awake_platform::POWER_POLL_SECONDS));

        let schedule = status
            .providers
            .iter()
            .find(|provider| provider.kind == awake_core::ProviderKind::TimeSchedule)
            .unwrap();
        assert_eq!(
            schedule.poll_seconds, None,
            "a provider that needs no I/O must not look like one that polls"
        );
    }

    #[tokio::test]
    async fn a_conflict_between_two_active_rules_is_explained_with_its_winner() {
        use awake_core::SessionPolicy;
        let fixture = fixture().await;
        fixture.create_rule(on_ac_rule("Quiet")).await;

        // Accept the reduced-security choice, which is what saving such a rule
        // in the GUI does.
        fixture
            .engine
            .handle(AwakeRequest::new(RequestBody::StartSession {
                reason: "Presenting".to_string(),
                policy: SessionPolicy {
                    prevent_display_sleep: true,
                    ..SessionPolicy::quick_default()
                },
                battery_stop_percent: Some(20),
                end: WireEnd::Indefinite,
                security_confirmed: true,
            }))
            .await;
        fixture
            .engine
            .handle(AwakeRequest::new(RequestBody::EndManualSession))
            .await;

        let mut presenting = on_ac_rule("Presenting");
        presenting.policy = SessionPolicy {
            prevent_display_sleep: true,
            ..SessionPolicy::quick_default()
        };
        presenting.priority = 90;
        let presenting_id = fixture.create_rule(presenting).await;

        let status = fixture.engine.status().await;
        assert_eq!(status.active_rules.len(), 2);
        let conflict = status
            .conflicts
            .iter()
            .find(|conflict| conflict.field == "prevent_display_sleep")
            .expect("the disagreement must be explained");
        assert_eq!(conflict.winner_rule_id, presenting_id);
        assert_eq!(conflict.winner_name, "Presenting");
        assert_eq!(
            conflict.resolution_key,
            awake_core::RESOLUTION_STRONGEST_WINS
        );
        assert!(status.effective_policy.prevent_display_sleep);
    }

    #[tokio::test]
    async fn rules_survive_a_restart_of_the_service() {
        let fixture = fixture().await;
        fixture.create_rule(on_ac_rule("Charging")).await;
        fixture.engine.shutdown().await;

        let next = restart(
            fixture._directory.path(),
            &fixture.store,
            Arc::new(FixedClock::at(NOW + 100)),
        )
        .await;
        next.evaluate_rules_now().await;

        let status = next.status().await;
        assert_eq!(status.rule_summary.total, 1);
        assert_eq!(status.active_rules.len(), 1);
        assert!(
            next.holds_inhibitor().await,
            "a rule that still matches must take hold again after a restart"
        );
    }

    // ---- Battery safety ----------------------------------------------------

    #[tokio::test]
    async fn a_battery_powered_machine_is_recognized_from_its_own_hardware() {
        let fixture = fixture().await;
        assert!(fixture.engine.has_battery().await);
        assert!(fixture.engine.status().await.battery_protection.has_battery);
        assert_eq!(
            fixture.engine.status().await.battery_protection.percent,
            Some(80)
        );
    }

    #[tokio::test]
    async fn a_flat_battery_ends_the_session_and_reports_the_stop_for_a_notification() {
        let fixture = fixture().await;
        fixture.engine.handle(start(WireEnd::Indefinite)).await;
        let session_id = fixture.engine.status().await.sessions[0].session_id;

        assert!(
            fixture.engine.report_battery(20).await.is_empty(),
            "twenty percent is the threshold, not below it"
        );
        assert!(fixture.engine.holds_inhibitor().await);

        let stops = fixture.engine.report_battery(19).await;

        assert_eq!(
            stops,
            vec![BatteryStop {
                session: SessionId(session_id),
                percent: 19,
            }],
            "the stop must be reported so a notification can be raised for it"
        );
        assert!(!fixture.engine.holds_inhibitor().await);
    }

    #[tokio::test]
    async fn a_low_battery_stop_leaves_a_history_entry_saying_what_happened() {
        let fixture = fixture().await;
        fixture.engine.handle(start(WireEnd::Indefinite)).await;
        fixture.engine.report_battery(15).await;

        let response = fixture
            .engine
            .handle(AwakeRequest::new(RequestBody::QueryHistory { limit: 10 }))
            .await;
        let history = history_of(&response);

        assert_eq!(history.entries.len(), 1);
        let entry = &history.entries[0];
        assert_eq!(entry.end_cause.as_deref(), Some("battery_threshold"));
        assert_eq!(entry.battery_stop_percent_at_stop, Some(15));
        assert_eq!(entry.battery_stop_percent, Some(20));
        assert_eq!(entry.origin, awake_core::SessionOrigin::Manual);
        assert!(entry.ended_at_unix_seconds.is_some());
    }

    #[tokio::test]
    async fn the_battery_provider_drives_the_stop_through_a_tick_with_nobody_pushing_a_number() {
        let fixture = fixture().await;
        fixture.create_rule(on_ac_rule("Charging")).await;
        assert!(fixture.engine.holds_inhibitor().await);

        // Flatten the real battery in the captured tree. Nothing calls
        // `report_battery` — the provider reads it and the tick acts on it,
        // which is the path that runs on a real machine.
        fixture.set_battery(9);
        fixture.clock.advance(60);
        fixture.engine.tick().await;

        assert!(
            !fixture.engine.holds_inhibitor().await,
            "protection must act on what the provider read, not on a pushed number"
        );
        let response = fixture
            .engine
            .handle(AwakeRequest::new(RequestBody::QueryHistory { limit: 10 }))
            .await;
        let entry = &history_of(&response).entries[0];
        assert_eq!(entry.end_cause.as_deref(), Some("battery_threshold"));
        assert_eq!(entry.battery_stop_percent_at_stop, Some(9));
        assert_eq!(entry.origin, awake_core::SessionOrigin::Trigger);
    }

    #[tokio::test]
    async fn a_rule_stopped_by_a_flat_battery_is_not_restarted_and_stopped_again_every_tick() {
        let fixture = fixture().await;
        fixture.create_rule(on_ac_rule("Charging")).await;
        fixture.set_battery(9);

        // The charger is still plugged in, so the rule still matches on every
        // tick after the stop. Restarting its session only for protection to
        // end it again would be a new stop, and a new notification, every tick.
        for _ in 0..3 {
            fixture.clock.advance(60);
            fixture.engine.tick().await;
        }

        assert!(!fixture.engine.holds_inhibitor().await);
        assert!(fixture.engine.status().await.sessions.is_empty());
        let response = fixture
            .engine
            .handle(AwakeRequest::new(RequestBody::QueryHistory { limit: 10 }))
            .await;
        assert_eq!(history_of(&response).total, 1, "one stop, not one per tick");
        assert_eq!(
            fixture.engine.status().await.rule_summary.refused,
            1,
            "a rule that matches and is held back by the battery says so"
        );
    }

    // ---- Sessions the service ends on its own ------------------------------

    #[tokio::test]
    async fn a_session_that_expires_on_a_tick_is_reported_as_ended_once() {
        let fixture = fixture().await;
        fixture
            .engine
            .handle(start(WireEnd::Duration { seconds: 900 }))
            .await;
        let session_id = fixture.engine.status().await.sessions[0].session_id;

        fixture.clock.advance(900);
        assert_eq!(
            fixture.engine.tick().await.ended,
            vec![EndedSession {
                session: SessionId(session_id),
                cause: EndCause::Expired,
                battery_stop_percent: Some(20),
            }]
        );

        fixture.clock.advance(60);
        assert!(
            fixture.engine.tick().await.ended.is_empty(),
            "a session is reported ended once, not on every tick after it"
        );
    }

    #[tokio::test]
    async fn a_low_battery_stop_on_a_tick_is_reported_once_with_the_threshold_it_crossed() {
        let fixture = fixture().await;
        fixture.engine.handle(start(WireEnd::Indefinite)).await;
        let session_id = fixture.engine.status().await.sessions[0].session_id;

        fixture.set_battery(9);
        fixture.clock.advance(60);
        let ended = fixture.engine.tick().await.ended;

        assert_eq!(
            ended,
            vec![EndedSession {
                session: SessionId(session_id),
                cause: EndCause::BatteryThreshold { percent: 9 },
                battery_stop_percent: Some(20),
            }]
        );
        assert_eq!(
            ended[0].event().body,
            awake_ipc::EventBody::SessionEnded {
                session_id,
                cause: "battery_threshold".to_string(),
                battery_stop_percent: Some(20),
                battery_percent: Some(9),
                lid_limit_minutes: None,
            }
        );

        fixture.clock.advance(60);
        assert!(fixture.engine.tick().await.ended.is_empty());
    }

    #[tokio::test]
    async fn a_rule_that_stops_matching_on_a_tick_is_reported_as_ended() {
        let fixture = fixture().await;
        fixture.create_rule(on_ac_rule("Charging")).await;
        let session_id = fixture.engine.status().await.sessions[0].session_id;

        fixture.set_on_ac(false);
        fixture.clock.advance(60);
        let ended = fixture.engine.tick().await.ended;

        assert_eq!(
            ended
                .iter()
                .map(|ended| (ended.session, ended.cause))
                .collect::<Vec<_>>(),
            vec![(SessionId(session_id), EndCause::TriggerCleared)]
        );
    }

    #[tokio::test]
    async fn a_rule_that_starts_on_a_tick_is_reported_as_a_change_and_nothing_after_it_is() {
        let fixture = fixture().await;
        fixture.set_on_ac(false);
        fixture.create_rule(on_ac_rule("Charging")).await;
        assert!(fixture.engine.status().await.sessions.is_empty());

        fixture.set_on_ac(true);
        fixture.clock.advance(60);
        let outcome = fixture.engine.tick().await;
        assert!(outcome.changed, "a session the rule started is a change");
        assert!(outcome.ended.is_empty());
        assert_eq!(fixture.engine.status().await.sessions.len(), 1);

        fixture.clock.advance(60);
        assert_eq!(
            fixture.engine.tick().await,
            TickOutcome::default(),
            "a tick that leaves everything as it was changes nothing"
        );
    }

    #[tokio::test]
    async fn a_rule_the_battery_holds_back_on_a_tick_is_reported_as_a_change() {
        let fixture = fixture().await;
        fixture.set_on_ac(false);
        fixture.set_battery(9);
        fixture.create_rule(on_ac_rule("Charging")).await;

        fixture.set_on_ac(true);
        fixture.clock.advance(60);
        let outcome = fixture.engine.tick().await;
        assert!(outcome.changed, "a newly refused rule is a change");
        assert!(outcome.ended.is_empty());
        assert_eq!(fixture.engine.status().await.rule_summary.refused, 1);

        fixture.clock.advance(60);
        assert!(!fixture.engine.tick().await.changed);
    }

    #[tokio::test]
    async fn a_tick_that_ends_a_session_is_a_change() {
        let fixture = fixture().await;
        fixture
            .engine
            .handle(start(WireEnd::Duration { seconds: 900 }))
            .await;

        fixture.clock.advance(900);
        let outcome = fixture.engine.tick().await;
        assert_eq!(outcome.ended.len(), 1);
        assert!(outcome.changed);
    }

    #[tokio::test]
    async fn a_session_the_client_ended_is_not_reported_as_one_the_service_ended() {
        let fixture = fixture().await;
        fixture.engine.handle(start(WireEnd::Indefinite)).await;

        let (response, ended) = fixture
            .engine
            .handle_reporting(AwakeRequest::new(RequestBody::EndManualSession))
            .await;

        assert!(status_of(&response).sessions.is_empty());
        assert!(
            ended.is_empty(),
            "the client that asked already has its answer"
        );
    }

    #[tokio::test]
    async fn a_battery_stop_made_while_answering_a_rule_edit_is_still_reported() {
        let fixture = fixture().await;
        fixture.engine.handle(start(WireEnd::Indefinite)).await;
        let session_id = fixture.engine.status().await.sessions[0].session_id;
        fixture.set_battery(9);

        // Editing a rule re-reads every provider, so the flat battery is seen
        // while answering a request that had nothing to do with the session.
        let (_, ended) = fixture
            .engine
            .handle_reporting(AwakeRequest::new(RequestBody::CreateRule {
                rule: Box::new(on_ac_rule("Charging")),
            }))
            .await;

        assert_eq!(
            ended,
            vec![EndedSession {
                session: SessionId(session_id),
                cause: EndCause::BatteryThreshold { percent: 9 },
                battery_stop_percent: Some(20),
            }]
        );
    }

    #[tokio::test]
    async fn raising_a_running_rules_threshold_above_the_reading_is_a_battery_stop() {
        let fixture = fixture().await;
        let rule_id = fixture.create_rule(on_ac_rule("Charging")).await;
        let session_id = fixture.engine.status().await.sessions[0].session_id;

        let mut edited = on_ac_rule("Charging");
        edited.battery_stop_percent = Some(90);
        let (_, ended) = fixture
            .engine
            .handle_reporting(AwakeRequest::new(RequestBody::UpdateRule {
                rule_id,
                rule: Box::new(edited),
            }))
            .await;

        assert_eq!(
            ended,
            vec![EndedSession {
                session: SessionId(session_id),
                cause: EndCause::BatteryThreshold { percent: 80 },
                battery_stop_percent: Some(90),
            }],
            "the session the rule held is stopped by protection, not cleared as if the rule \
             had stopped matching"
        );
    }

    #[tokio::test]
    async fn a_shutdown_reports_every_session_it_ended() {
        let fixture = fixture().await;
        fixture.engine.handle(start(WireEnd::Indefinite)).await;
        let session_id = fixture.engine.status().await.sessions[0].session_id;

        let ended = fixture.engine.shutdown().await;

        assert_eq!(
            ended
                .iter()
                .map(|ended| (ended.session, ended.cause))
                .collect::<Vec<_>>(),
            vec![(SessionId(session_id), EndCause::ServiceShutdown)]
        );
    }

    // ---- History -----------------------------------------------------------

    #[tokio::test]
    async fn history_records_a_trigger_session_with_the_rule_that_held_it() {
        let fixture = fixture().await;
        let rule_id = fixture.create_rule(on_ac_rule("Charging")).await;
        fixture.set_on_ac(false);
        fixture.clock.advance(60);
        fixture.engine.tick().await;

        let response = fixture
            .engine
            .handle(AwakeRequest::new(RequestBody::QueryHistory { limit: 10 }))
            .await;
        let entry = &history_of(&response).entries[0];

        assert_eq!(entry.origin, awake_core::SessionOrigin::Trigger);
        assert_eq!(entry.rule_id, Some(rule_id));
        assert_eq!(entry.reasons, vec!["Charging".to_string()]);
        assert_eq!(entry.end_cause.as_deref(), Some("trigger_cleared"));
    }

    #[tokio::test]
    async fn a_rule_suspended_by_a_pause_is_recorded_apart_from_one_that_stopped_matching() {
        let fixture = fixture().await;
        fixture.create_rule(on_ac_rule("Charging")).await;
        fixture
            .engine
            .handle(AwakeRequest::new(RequestBody::PauseRules { seconds: None }))
            .await;

        let response = fixture
            .engine
            .handle(AwakeRequest::new(RequestBody::QueryHistory { limit: 10 }))
            .await;
        assert_eq!(
            history_of(&response).entries[0].end_cause.as_deref(),
            Some("rules_suppressed"),
            "the rule still matched; only permission changed, and the record must say so"
        );
    }

    #[tokio::test]
    async fn history_never_records_a_command_line_even_when_a_reason_carries_one() {
        let fixture = fixture().await;
        fixture
            .engine
            .handle(AwakeRequest::new(RequestBody::StartSession {
                reason: "deploy --token hunter2".to_string(),
                policy: awake_core::SessionPolicy::quick_default(),
                battery_stop_percent: Some(20),
                end: WireEnd::Indefinite,
                security_confirmed: false,
            }))
            .await;

        let response = fixture
            .engine
            .handle(AwakeRequest::new(RequestBody::QueryHistory { limit: 10 }))
            .await;
        let reasons = &history_of(&response).entries[0].reasons;
        assert!(
            reasons.iter().all(|reason| !reason.contains("hunter2")),
            "a token typed into a reason must not survive into the history: {reasons:?}"
        );
    }

    #[tokio::test]
    async fn history_reports_its_own_retention_limit_so_a_missing_session_is_explainable() {
        let fixture = fixture().await;
        fixture.engine.handle(start(WireEnd::Indefinite)).await;

        let response = fixture
            .engine
            .handle(AwakeRequest::new(RequestBody::QueryHistory { limit: 1 }))
            .await;
        let history = history_of(&response);
        assert_eq!(history.total, 1);
        assert_eq!(
            history.retention_limit,
            awake_store::history::MAX_HISTORY_ENTRIES as u32
        );
    }

    #[tokio::test]
    async fn an_until_time_in_the_past_is_refused_by_the_service_that_owns_the_clock() {
        let fixture = fixture().await;
        let response = fixture
            .engine
            .handle(start(WireEnd::UntilUnixSeconds {
                unix_seconds: NOW - 1,
            }))
            .await;
        assert_eq!(
            rejection_of(&response),
            "awake.error.end_condition:awake.error.end_time_in_the_past"
        );
    }

    // ---- Also when the lid is closed (ticket 109) --------------------------

    fn start_with(policy: awake_core::SessionPolicy, battery: Option<u8>) -> AwakeRequest {
        AwakeRequest::new(RequestBody::StartSession {
            reason: "Download is running".to_string(),
            policy,
            battery_stop_percent: battery,
            end: WireEnd::Indefinite,
            security_confirmed: false,
        })
    }

    fn held_what(fixture: &Fixture) -> Vec<String> {
        fixture
            .engine
            .backend()
            .held_requests()
            .iter()
            .map(LeaseRequest::what_argument)
            .collect()
    }

    /// The lid closed and the charger out, as the next tick reads them.
    fn close_lid_on_battery(fixture: &Fixture) {
        fixture.engine.backend().set_lid_closed(Some(true));
        fixture.set_on_ac(false);
    }

    fn on_battery_rule(name: &str, lid: bool) -> awake_core::Rule {
        use awake_core::{Combine, Condition, ConditionGroup, Reason, Rule, RuleId};
        let mut rule = Rule::new(
            RuleId(0),
            Reason::new(name).unwrap(),
            Combine::All,
            [ConditionGroup::one(Condition::AcPower { connected: false }).unwrap()],
        )
        .unwrap();
        rule.policy.prevent_lid_suspend = lid;
        rule
    }

    #[tokio::test]
    async fn a_manual_session_holds_the_lid_switch_and_ending_it_releases_the_lock() {
        let fixture = fixture().await;
        let status = status_of(
            &fixture
                .engine
                .handle(start_with(
                    awake_core::SessionPolicy::manual_default(),
                    Some(20),
                ))
                .await,
        )
        .clone();
        assert!(status.effective_policy.prevent_lid_suspend);
        assert!(status.unmet_policy.is_empty());
        // What `systemd-inhibit --list` would show: one block lock, all three.
        assert_eq!(held_what(&fixture), vec!["sleep:idle:handle-lid-switch"]);

        fixture
            .engine
            .handle(AwakeRequest::new(RequestBody::EndManualSession))
            .await;
        assert!(held_what(&fixture).is_empty());
        assert!(!fixture.engine.holds_inhibitor().await);
        assert_eq!(
            fixture.engine.backend().released().last().unwrap().what,
            vec![
                InhibitWhat::Sleep,
                InhibitWhat::Idle,
                InhibitWhat::HandleLidSwitch
            ]
        );
    }

    #[tokio::test]
    async fn a_session_with_the_lid_choice_off_takes_no_lid_lock() {
        let fixture = fixture().await;
        fixture
            .engine
            .handle(start_with(
                awake_core::SessionPolicy::quick_default(),
                Some(20),
            ))
            .await;
        assert_eq!(held_what(&fixture), vec!["sleep:idle"]);
        assert!(
            !fixture
                .engine
                .status()
                .await
                .effective_policy
                .prevent_lid_suspend
        );
    }

    #[tokio::test]
    async fn turning_the_lid_choice_off_in_a_running_session_drops_only_the_lid_lock() {
        let fixture = fixture().await;
        let status = status_of(
            &fixture
                .engine
                .handle(start_with(
                    awake_core::SessionPolicy::manual_default(),
                    Some(20),
                ))
                .await,
        )
        .clone();
        let session_id = status.sessions[0].session_id;
        fixture
            .engine
            .handle(AwakeRequest::new(RequestBody::ChangeSession {
                session_id,
                reason: "Download is running".to_string(),
                policy: awake_core::SessionPolicy::quick_default(),
                battery_stop_percent: Some(20),
                end: WireEnd::Indefinite,
                security_confirmed: false,
            }))
            .await;
        assert_eq!(held_what(&fixture), vec!["sleep:idle"]);
    }

    #[tokio::test]
    async fn a_backend_without_a_lid_lock_reports_the_choice_unmet_and_holds_the_rest() {
        let fixture = fixture_with(FakeInhibitorBackend::with_capabilities(
            BackendCapabilities {
                system_suspend: true,
                idle: true,
                display_sleep: false,
                automatic_lock: false,
                lid_switch: false,
            },
        ))
        .await;
        let status = status_of(
            &fixture
                .engine
                .handle(start_with(
                    awake_core::SessionPolicy::manual_default(),
                    Some(20),
                ))
                .await,
        )
        .clone();
        assert_eq!(status.unmet_policy, vec![awake_core::PolicyGap::LidSwitch]);
        assert_eq!(held_what(&fixture), vec!["sleep:idle"]);
    }

    #[tokio::test]
    async fn the_lid_closed_on_ac_never_ends_the_session() {
        let fixture = fixture().await;
        fixture
            .engine
            .handle(start_with(
                awake_core::SessionPolicy::manual_default(),
                Some(20),
            ))
            .await;
        fixture.engine.backend().set_lid_closed(Some(true));
        for _ in 0..5 {
            fixture.clock.advance(3_600);
            assert!(fixture.engine.tick().await.ended.is_empty());
        }
        let status = fixture.engine.status().await;
        assert_eq!(status.sessions.len(), 1);
        assert_eq!(status.lid.closed, Some(true));
        assert_eq!(status.lid.stops_at_unix_seconds, None);
    }

    #[tokio::test]
    async fn the_lid_closed_on_battery_past_the_limit_ends_the_session_and_releases_the_lid() {
        let fixture = fixture().await;
        let session_id = status_of(
            &fixture
                .engine
                .handle(start_with(
                    awake_core::SessionPolicy::manual_default(),
                    Some(20),
                ))
                .await,
        )
        .sessions[0]
            .session_id;

        close_lid_on_battery(&fixture);
        fixture.clock.advance(60);
        let closed_at = fixture.clock.now_unix_seconds();
        assert!(fixture.engine.tick().await.ended.is_empty());
        let status = fixture.engine.status().await;
        assert_eq!(status.lid.closed, Some(true));
        assert_eq!(status.lid.battery_limit.minutes(), Some(120));
        assert_eq!(status.lid.stops_at_unix_seconds, Some(closed_at + 7_200));

        fixture.clock.set(closed_at + 7_199);
        assert!(fixture.engine.tick().await.ended.is_empty());
        assert_eq!(held_what(&fixture), vec!["sleep:idle:handle-lid-switch"]);

        fixture.clock.set(closed_at + 7_200);
        let ended = fixture.engine.tick().await.ended;
        assert_eq!(
            ended,
            vec![EndedSession {
                session: SessionId(session_id),
                cause: EndCause::LidClosedOnBattery { limit_minutes: 120 },
                battery_stop_percent: Some(20),
            }]
        );
        assert_eq!(
            ended[0].event().body,
            EventBody::SessionEnded {
                session_id,
                cause: "lid_closed_on_battery".to_string(),
                battery_stop_percent: Some(20),
                battery_percent: None,
                lid_limit_minutes: Some(120),
            }
        );
        assert!(held_what(&fixture).is_empty(), "the machine may now sleep");

        let response = fixture
            .engine
            .handle(AwakeRequest::new(RequestBody::QueryHistory { limit: 1 }))
            .await;
        assert_eq!(
            history_of(&response).entries[0].end_cause.as_deref(),
            Some("lid_closed_on_battery")
        );
    }

    #[tokio::test]
    async fn opening_the_lid_or_plugging_in_starts_the_closed_lid_clock_again() {
        let fixture = fixture().await;
        fixture
            .engine
            .handle(start_with(
                awake_core::SessionPolicy::manual_default(),
                Some(20),
            ))
            .await;
        close_lid_on_battery(&fixture);
        // Past the power provider's cadence, so the tick reads the charger.
        fixture.clock.advance(60);
        fixture.engine.tick().await;

        fixture.clock.advance(7_000);
        fixture.engine.backend().set_lid_closed(Some(false));
        assert!(fixture.engine.tick().await.ended.is_empty());
        assert_eq!(
            fixture.engine.status().await.lid.stops_at_unix_seconds,
            None
        );
        fixture.clock.advance(600);
        assert!(fixture.engine.tick().await.ended.is_empty());

        fixture.engine.backend().set_lid_closed(Some(true));
        fixture.engine.tick().await;
        assert!(
            fixture
                .engine
                .status()
                .await
                .lid
                .stops_at_unix_seconds
                .is_some()
        );
        fixture.clock.advance(7_000);
        fixture.set_on_ac(true);
        assert!(fixture.engine.tick().await.ended.is_empty());
        // Past the limit since the lid closed, but on the charger.
        fixture.clock.advance(600);
        assert!(fixture.engine.tick().await.ended.is_empty());

        fixture.set_on_ac(false);
        fixture.clock.advance(60);
        fixture.engine.tick().await;
        assert!(
            fixture
                .engine
                .status()
                .await
                .lid
                .stops_at_unix_seconds
                .is_some()
        );
        fixture.clock.advance(7_000);
        assert!(
            fixture.engine.tick().await.ended.is_empty(),
            "each closed stretch is measured on its own"
        );
        assert_eq!(fixture.engine.status().await.sessions.len(), 1);
    }

    #[tokio::test]
    async fn a_limit_of_never_keeps_the_session_and_survives_a_restart() {
        let fixture = fixture().await;
        let response = fixture
            .engine
            .handle(AwakeRequest::new(RequestBody::SetLidBatteryLimit {
                limit: LidBatteryLimit::Never,
            }))
            .await;
        assert_eq!(
            status_of(&response).lid.battery_limit,
            LidBatteryLimit::Never
        );

        fixture
            .engine
            .handle(start_with(
                awake_core::SessionPolicy::manual_default(),
                None,
            ))
            .await;
        close_lid_on_battery(&fixture);
        for _ in 0..10 {
            fixture.clock.advance(3_600);
            assert!(fixture.engine.tick().await.ended.is_empty());
        }

        let again = restart(
            fixture._directory.path(),
            &fixture.store,
            fixture.clock.clone(),
        )
        .await;
        assert_eq!(
            again.status().await.lid.battery_limit,
            LidBatteryLimit::Never
        );
    }

    #[tokio::test]
    async fn an_edited_limit_applies_to_the_session_already_running() {
        let fixture = fixture().await;
        fixture
            .engine
            .handle(start_with(
                awake_core::SessionPolicy::manual_default(),
                Some(20),
            ))
            .await;
        close_lid_on_battery(&fixture);
        // Past the power provider's cadence, so the tick reads the charger.
        fixture.clock.advance(60);
        fixture.engine.tick().await;
        fixture
            .engine
            .handle(AwakeRequest::new(RequestBody::SetLidBatteryLimit {
                limit: LidBatteryLimit::Minutes { minutes: 30 },
            }))
            .await;
        fixture.clock.advance(30 * 60);
        assert_eq!(
            fixture.engine.tick().await.ended[0].cause,
            EndCause::LidClosedOnBattery { limit_minutes: 30 }
        );
    }

    #[tokio::test]
    async fn the_battery_threshold_still_ends_a_session_while_the_lid_is_closed() {
        let fixture = fixture().await;
        fixture
            .engine
            .handle(start_with(
                awake_core::SessionPolicy::manual_default(),
                Some(20),
            ))
            .await;
        close_lid_on_battery(&fixture);
        // Past the power provider's cadence, so the tick reads the charger.
        fixture.clock.advance(60);
        fixture.engine.tick().await;
        fixture.set_battery(15);
        fixture.clock.advance(60);
        let ended = fixture.engine.tick().await.ended;
        assert_eq!(ended[0].cause, EndCause::BatteryThreshold { percent: 15 });
        assert!(held_what(&fixture).is_empty());
    }

    #[tokio::test]
    async fn a_rule_holds_the_lid_only_when_it_says_so() {
        let fixture = fixture().await;
        fixture
            .create_rule(on_battery_rule("Unplugged", false))
            .await;
        fixture.set_on_ac(false);
        fixture.clock.advance(60);
        fixture.engine.tick().await;
        assert_eq!(held_what(&fixture), vec!["sleep:idle"]);

        let response = fixture
            .engine
            .handle(AwakeRequest::new(RequestBody::QueryRules))
            .await;
        let mut rule = rules_of(&response).rules[0].clone();
        rule.policy.prevent_lid_suspend = true;
        fixture
            .engine
            .handle(AwakeRequest::new(RequestBody::UpdateRule {
                rule_id: rule.id.0,
                rule: Box::new(rule),
            }))
            .await;
        assert_eq!(held_what(&fixture), vec!["sleep:idle:handle-lid-switch"]);
    }

    #[tokio::test]
    async fn the_closed_lid_limit_ends_a_rules_session_and_holds_it_back_until_the_lid_opens() {
        let fixture = fixture().await;
        fixture
            .create_rule(on_battery_rule("Unplugged", true))
            .await;
        close_lid_on_battery(&fixture);
        fixture.clock.advance(60);
        fixture.engine.tick().await;
        assert_eq!(held_what(&fixture), vec!["sleep:idle:handle-lid-switch"]);

        fixture.clock.advance(7_200);
        let ended = fixture.engine.tick().await.ended;
        assert_eq!(
            ended[0].cause,
            EndCause::LidClosedOnBattery { limit_minutes: 120 }
        );
        for _ in 0..3 {
            fixture.clock.advance(60);
            assert!(
                fixture.engine.tick().await.ended.is_empty(),
                "not restarted only to be ended again"
            );
        }
        assert!(held_what(&fixture).is_empty());
        assert_eq!(fixture.engine.status().await.rule_summary.refused, 1);

        fixture.engine.backend().set_lid_closed(Some(false));
        fixture.clock.advance(60);
        fixture.engine.tick().await;
        fixture.clock.advance(60);
        fixture.engine.tick().await;
        assert_eq!(held_what(&fixture), vec!["sleep:idle:handle-lid-switch"]);
    }

    #[tokio::test]
    async fn the_closed_lid_limit_leaves_a_rules_session_that_does_not_hold_the_lid() {
        let fixture = fixture().await;
        fixture
            .create_rule(on_battery_rule("Unplugged", false))
            .await;
        fixture
            .engine
            .handle(start_with(
                awake_core::SessionPolicy::manual_default(),
                Some(20),
            ))
            .await;
        close_lid_on_battery(&fixture);
        fixture.clock.advance(60);
        fixture.engine.tick().await;
        fixture.clock.advance(7_200);
        let ended = fixture.engine.tick().await.ended;
        assert_eq!(ended.len(), 1, "only the manual session held the lid");
        let status = fixture.engine.status().await;
        assert_eq!(status.sessions.len(), 1);
        assert_eq!(
            status.sessions[0].origin,
            awake_core::SessionOrigin::Trigger
        );
        assert_eq!(held_what(&fixture), vec!["sleep:idle"]);
    }

    #[tokio::test]
    async fn a_legacy_client_changing_a_session_keeps_the_lid_choice_in_force() {
        let fixture = fixture().await;
        let session_id = status_of(
            &fixture
                .engine
                .handle(start_with(
                    awake_core::SessionPolicy::manual_default(),
                    Some(20),
                ))
                .await,
        )
        .sessions[0]
            .session_id;
        let mut change = AwakeRequest::new(RequestBody::ChangeSession {
            session_id,
            reason: "Download is running".to_string(),
            policy: awake_core::SessionPolicy::quick_default(),
            battery_stop_percent: Some(20),
            end: WireEnd::Duration { seconds: 900 },
            security_confirmed: false,
        });
        change.protocol_version = LEGACY_PROTOCOL_VERSION;
        let status = status_of(&fixture.engine.handle(change).await).clone();
        assert!(status.sessions[0].policy.prevent_lid_suspend);
        assert_eq!(held_what(&fixture), vec!["sleep:idle:handle-lid-switch"]);
    }

    #[tokio::test]
    async fn a_closed_lid_limit_out_of_range_is_refused_and_changes_nothing() {
        let fixture = fixture().await;
        let response = fixture
            .engine
            .handle(AwakeRequest::new(RequestBody::SetLidBatteryLimit {
                limit: LidBatteryLimit::Minutes { minutes: 120 },
            }))
            .await;
        status_of(&response);
        // The engine is reached only by validated requests; the edge refuses
        // the rest, which awake-ipc's tests prove.
        assert!(
            AwakeRequest::new(RequestBody::SetLidBatteryLimit {
                limit: LidBatteryLimit::Minutes { minutes: 2 },
            })
            .validate()
            .is_err()
        );
        assert_eq!(
            fixture.engine.status().await.lid.battery_limit.minutes(),
            Some(120)
        );
    }

    #[tokio::test]
    async fn a_legacy_client_starting_a_session_gets_the_lid_choice_on() {
        let fixture = fixture().await;
        let mut request = start_with(awake_core::SessionPolicy::quick_default(), Some(20));
        request.protocol_version = LEGACY_PROTOCOL_VERSION;
        let status = status_of(&fixture.engine.handle(request).await).clone();
        assert!(status.sessions[0].policy.prevent_lid_suspend);
        assert_eq!(held_what(&fixture), vec!["sleep:idle:handle-lid-switch"]);
    }

    #[tokio::test]
    async fn a_legacy_client_creating_a_rule_leaves_the_lid_off() {
        let fixture = fixture().await;
        let mut request = AwakeRequest::new(RequestBody::CreateRule {
            rule: Box::new(on_battery_rule("Unplugged", false)),
        });
        request.protocol_version = LEGACY_PROTOCOL_VERSION;
        let rules = rules_of(&fixture.engine.handle(request).await).clone();
        assert!(!rules.rules[0].policy.prevent_lid_suspend);
    }

    #[tokio::test]
    async fn swapping_the_lock_never_leaves_a_moment_without_the_lid_lock() {
        let fixture = fixture().await;
        fixture
            .engine
            .handle(start_with(
                awake_core::SessionPolicy::manual_default(),
                Some(20),
            ))
            .await;
        // A rule taking hold beside the session, then letting go, each change
        // the lock's reason and so swap it.
        fixture.create_rule(on_ac_rule("Charging")).await;
        assert_eq!(fixture.engine.status().await.sessions.len(), 2);
        fixture.set_on_ac(false);
        fixture.clock.advance(60);
        fixture.engine.tick().await;
        assert_eq!(fixture.engine.status().await.sessions.len(), 1);

        let timeline = fixture.engine.backend().lid_held_timeline();
        let first = timeline
            .iter()
            .position(|held| *held)
            .expect("the lid lock was taken");
        assert!(
            timeline.len() - first >= 5,
            "two swaps happened: {timeline:?}"
        );
        assert!(
            timeline[first..].iter().all(|held| *held),
            "a swap let go of the lid lock before taking the next: {timeline:?}"
        );

        fixture
            .engine
            .handle(AwakeRequest::new(RequestBody::EndManualSession))
            .await;
        assert_eq!(
            fixture.engine.backend().lid_held_timeline().last(),
            Some(&false)
        );
        assert!(held_what(&fixture).is_empty());
    }

    #[tokio::test]
    async fn a_refused_lid_lock_still_holds_sleep_and_idle_and_says_the_lid_is_unavailable() {
        let backend = FakeInhibitorBackend::logind_shaped();
        backend.deny(InhibitWhat::HandleLidSwitch);
        let fixture = fixture_with(backend).await;
        let status = status_of(
            &fixture
                .engine
                .handle(start_with(
                    awake_core::SessionPolicy::manual_default(),
                    Some(20),
                ))
                .await,
        )
        .clone();
        assert_eq!(held_what(&fixture), vec!["sleep:idle"]);
        assert!(!status.backend.capabilities.lid_switch);
        assert_eq!(status.unmet_policy, vec![awake_core::PolicyGap::LidSwitch]);
        assert_eq!(status.attention, None, "the rest is held, nothing is lost");
        assert_eq!(status.indicator, awake_ipc::WireIndicator::ActiveManual);

        // A later change does not ask for the lid again.
        fixture
            .engine
            .handle(AwakeRequest::new(RequestBody::EndManualSession))
            .await;
        fixture
            .engine
            .handle(start_with(
                awake_core::SessionPolicy::manual_default(),
                Some(20),
            ))
            .await;
        assert_eq!(held_what(&fixture), vec!["sleep:idle"]);
    }

    #[tokio::test]
    async fn a_docked_machine_with_the_lid_closed_on_battery_is_not_stopped() {
        let fixture = fixture().await;
        fixture
            .engine
            .handle(start_with(
                awake_core::SessionPolicy::manual_default(),
                Some(20),
            ))
            .await;
        fixture.engine.backend().set_docked(Some(true));
        close_lid_on_battery(&fixture);
        for _ in 0..4 {
            fixture.clock.advance(3_600);
            assert!(fixture.engine.tick().await.ended.is_empty());
        }
        assert_eq!(
            fixture.engine.status().await.lid.stops_at_unix_seconds,
            None
        );

        // Undocked with the lid still closed: the clock starts then.
        fixture.engine.backend().set_docked(Some(false));
        fixture.clock.advance(60);
        fixture.engine.tick().await;
        assert!(
            fixture
                .engine
                .status()
                .await
                .lid
                .stops_at_unix_seconds
                .is_some()
        );
    }

    /// A clock whose wall time and monotonic time move apart, as they do when
    /// someone sets the date.
    #[derive(Default)]
    struct SteppedClock {
        wall: std::sync::atomic::AtomicU64,
        monotonic: std::sync::atomic::AtomicU64,
    }

    impl Clock for SteppedClock {
        fn now_unix_seconds(&self) -> u64 {
            self.wall.load(std::sync::atomic::Ordering::SeqCst)
        }
        fn monotonic_seconds(&self) -> u64 {
            self.monotonic.load(std::sync::atomic::Ordering::SeqCst)
        }
    }

    #[tokio::test]
    async fn the_closed_lid_time_is_measured_on_the_monotonic_clock() {
        use std::sync::atomic::Ordering;
        let directory = tempfile::tempdir().unwrap();
        write_machine(directory.path());
        let clock = Arc::new(SteppedClock::default());
        clock.wall.store(NOW, Ordering::SeqCst);
        clock.monotonic.store(500, Ordering::SeqCst);
        let engine = AwakeEngine::start_in(
            FakeInhibitorBackend::logind_shaped(),
            directory.path(),
            clock.clone(),
        )
        .await;
        engine
            .handle(start_with(
                awake_core::SessionPolicy::manual_default(),
                Some(20),
            ))
            .await;
        engine.backend().set_lid_closed(Some(true));
        awake_platform::power::write_supply(
            &directory.path().join("sys"),
            "ACAD",
            &[("type", "Mains"), ("online", "0")],
        );
        let step = |wall: u64, monotonic: u64| {
            clock.wall.fetch_add(wall, Ordering::SeqCst);
            clock.monotonic.fetch_add(monotonic, Ordering::SeqCst);
        };
        step(60, 60);
        engine.tick().await;
        assert!(engine.status().await.lid.stops_at_unix_seconds.is_some());

        // Someone sets the date three hours ahead: no time has passed.
        step(3 * 3_600, 1);
        assert!(engine.tick().await.ended.is_empty());
        assert_eq!(engine.status().await.sessions.len(), 1);

        // Two real hours later the limit is reached, whatever the wall says.
        step(0, 2 * 3_600);
        assert_eq!(
            engine.tick().await.ended[0].cause,
            EndCause::LidClosedOnBattery { limit_minutes: 120 }
        );
    }

    #[tokio::test]
    async fn a_failed_swap_lets_go_of_a_lid_lock_that_is_no_longer_wanted_and_retries() {
        let fixture = fixture().await;
        let session_id = status_of(
            &fixture
                .engine
                .handle(start_with(
                    awake_core::SessionPolicy::manual_default(),
                    Some(20),
                ))
                .await,
        )
        .sessions[0]
            .session_id;
        assert_eq!(held_what(&fixture), vec!["sleep:idle:handle-lid-switch"]);

        // The person turns the lid choice off, and taking the new lock fails.
        fixture
            .engine
            .backend()
            .fail_next_acquire(BackendError::Protocol("logind busy".to_string()));
        fixture
            .engine
            .handle(AwakeRequest::new(RequestBody::ChangeSession {
                session_id,
                reason: "Download is running".to_string(),
                policy: awake_core::SessionPolicy::quick_default(),
                battery_stop_percent: Some(20),
                end: WireEnd::Indefinite,
                security_confirmed: false,
            }))
            .await;
        assert!(
            held_what(&fixture).is_empty(),
            "the lid lock nobody wants any more is let go"
        );
        assert!(fixture.engine.status().await.attention.is_some());

        // The next tick takes what is wanted.
        fixture.clock.advance(5);
        fixture.engine.tick().await;
        assert_eq!(held_what(&fixture), vec!["sleep:idle"]);
        assert_eq!(fixture.engine.status().await.attention, None);
    }

    #[tokio::test]
    async fn a_failed_swap_keeps_the_old_lock_when_it_holds_nothing_unwanted() {
        let fixture = fixture().await;
        let session_id = status_of(
            &fixture
                .engine
                .handle(start_with(
                    awake_core::SessionPolicy::quick_default(),
                    Some(20),
                ))
                .await,
        )
        .sessions[0]
            .session_id;
        fixture
            .engine
            .backend()
            .fail_next_acquire(BackendError::Protocol("logind busy".to_string()));
        fixture
            .engine
            .handle(AwakeRequest::new(RequestBody::ChangeSession {
                session_id,
                reason: "Another reason".to_string(),
                policy: awake_core::SessionPolicy::quick_default(),
                battery_stop_percent: Some(20),
                end: WireEnd::Indefinite,
                security_confirmed: false,
            }))
            .await;
        assert_eq!(held_what(&fixture), vec!["sleep:idle"]);
    }

    #[tokio::test]
    async fn a_refusal_that_is_not_about_the_lid_leaves_the_lid_capability_alone() {
        let backend = FakeInhibitorBackend::logind_shaped();
        backend.deny(InhibitWhat::Sleep);
        let fixture = fixture_with(backend).await;
        fixture
            .engine
            .handle(start_with(
                awake_core::SessionPolicy::manual_default(),
                Some(20),
            ))
            .await;
        let status = fixture.engine.status().await;
        assert!(held_what(&fixture).is_empty());
        assert!(
            status.backend.capabilities.lid_switch,
            "sleep was refused, not the lid"
        );
        assert!(status.attention.unwrap().contains("awake.backend.denied"));
        assert!(
            !status
                .unmet_policy
                .contains(&awake_core::PolicyGap::LidSwitch)
        );
    }
}
