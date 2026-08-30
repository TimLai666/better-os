//! The service's own logic: state machine plus inhibitor plus store.
//!
//! Everything here is transport-free. The D-Bus surface in `service.rs` does
//! nothing but hand documents in and signals out, which is why a session can be
//! proved to survive a client disconnecting without a bus being involved.

use std::sync::Arc;

use awake_core::{
    AwakeState, BackendCapabilities, BackendState, Command, Effect, EndCause, Session, SessionId,
    TransitionError,
};
use awake_ipc::{
    AwakeRequest, AwakeResponse, RequestBody, StatusDocument, WireBackend, WireInterrupted,
    WireReason, WireSession,
};
use awake_store::{JsonStore, PersistedSession, ServiceState};
use tokio::sync::Mutex;

use crate::backend::{Clock, InhibitorBackend, LeaseHealth, LeaseRequest};

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
}

impl<B: InhibitorBackend> AwakeEngine<B> {
    /// Loads the previous state, probes the backend, and reports anything the
    /// previous run left open.
    ///
    /// A previous session is never resumed. Its inhibitor died with the process
    /// that held it, so bringing it back would claim protection that does not
    /// exist; it is explained instead.
    pub async fn start(backend: B, store: JsonStore, clock: Arc<dyn Clock>) -> Self {
        let now = clock.now_unix_seconds();
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

        Self {
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
        }
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
        let now = self.now();
        let command = match self.command_for(&request) {
            Ok(None) => return AwakeResponse::status(self.status().await),
            Ok(Some(command)) => command,
            Err(error_key) => return AwakeResponse::rejected(error_key),
        };

        match self.apply(command, now).await {
            Ok(()) => AwakeResponse::status(self.status().await),
            Err(error) => AwakeResponse::rejected(error.to_string()),
        }
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
        }
    }

    /// Applies a command and carries out every effect it produced.
    pub async fn apply(&self, command: Command, now: u64) -> Result<(), TransitionError> {
        let effects = {
            let mut inner = self.inner.lock().await;
            let effects = inner.state.apply(command, now)?;
            record(&mut inner, &effects, now);
            effects
        };
        self.reconcile(&effects).await;
        self.persist().await;
        Ok(())
    }

    /// Ends every session whose end condition has arrived, then checks that the
    /// inhibitor the service believes it holds is really still held.
    pub async fn tick(&self) {
        let now = self.now();
        let _ = self.apply(Command::Expire, now).await;
        self.verify_lease().await;
        self.persist().await;
    }

    /// Reports a battery reading, which ends sessions that watch for it.
    ///
    /// Phase 1 ships no battery provider — that arrives with the trigger
    /// providers in ticket 26 — so this is the seam a provider calls, and the
    /// threshold that reaches it is already carried end to end.
    pub async fn report_battery(&self, percent: u8) {
        let now = self.now();
        let _ = self.apply(Command::BatteryLevel { percent }, now).await;
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
    pub async fn shutdown(&self) {
        let now = self.now();
        let effects = {
            let mut inner = self.inner.lock().await;
            let effects = inner
                .state
                .apply(Command::Shutdown, now)
                .unwrap_or_default();
            record(&mut inner, &effects, now);
            effects
        };
        self.reconcile(&effects).await;

        let mut inner = self.inner.lock().await;
        inner.persisted.run.last_seen_unix_seconds = now;
        inner.persisted.run.shut_down_at_unix_seconds = Some(now);
        inner.persisted.trim();
        let _ = self.store.save(&inner.persisted);
    }

    /// Whether the service is still holding an inhibitor. Used by tests and by
    /// the shutdown path's own assertion that nothing was left behind.
    pub async fn holds_inhibitor(&self) -> bool {
        self.inner.lock().await.lease.is_some()
    }

    pub async fn status(&self) -> StatusDocument {
        let now = self.now();
        let inner = self.inner.lock().await;
        let capabilities = *self.capabilities.lock().await;
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
            now_unix_seconds: now,
        }
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
            (
                LeaseRequest::from_policy(&effective.policy, INHIBITOR_WHO, &why),
                inner.lease.as_ref().map(|(request, _)| request.clone()),
            )
        };

        // Already holding exactly what is wanted, including the same reason
        // text, so there is nothing to churn.
        if wanted == existing {
            return;
        }

        if let Some((_, lease)) = self.inner.lock().await.lease.take() {
            let _ = self.backend.release(lease).await;
        }

        let Some(request) = wanted else {
            return;
        };
        let now = self.now();
        match self.backend.acquire(&request).await {
            Ok(lease) => {
                let mut inner = self.inner.lock().await;
                inner.lease = Some((request, lease));
                let capabilities = inner.state.backend().clone();
                if let BackendState::Available(capabilities) = capabilities {
                    let _ = inner
                        .state
                        .apply(Command::BackendAvailable(capabilities), now);
                }
            }
            Err(error) => {
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

    async fn persist(&self) {
        let now = self.now();
        let mut inner = self.inner.lock().await;
        inner.persisted.run.last_seen_unix_seconds = now;
        inner.persisted.reduced_security_confirmed = inner.state.reduced_security_confirmed();
        inner.persisted.trim();
        let _ = self.store.save(&inner.persisted);
    }
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
            Effect::PolicyChanged(_) | Effect::AttentionRaised(_) | Effect::AttentionCleared => {}
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
    }

    async fn fixture_with(backend: FakeInhibitorBackend) -> Fixture {
        let directory = tempfile::tempdir().unwrap();
        let store = JsonStore::at_path(directory.path().join("state.json"));
        let clock = Arc::new(FixedClock::at(NOW));
        let engine = AwakeEngine::start(backend, store.clone(), clock.clone()).await;
        Fixture {
            engine,
            clock,
            _directory: directory,
            store,
        }
    }

    async fn fixture() -> Fixture {
        fixture_with(FakeInhibitorBackend::logind_shaped()).await
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
        }
    }

    fn rejection_of(response: &AwakeResponse) -> &str {
        match &response.body {
            ResponseBody::Rejected { error_key } => error_key,
            ResponseBody::Status(_) => panic!("expected a rejection"),
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

        let clock = Arc::new(FixedClock::at(NOW + 5_000));
        let next = AwakeEngine::start(
            FakeInhibitorBackend::logind_shaped(),
            fixture.store.clone(),
            clock,
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

        let next = AwakeEngine::start(
            FakeInhibitorBackend::logind_shaped(),
            fixture.store.clone(),
            Arc::new(FixedClock::at(NOW + 100)),
        )
        .await;
        assert!(next.status().await.reduced_security_confirmed);
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
}
