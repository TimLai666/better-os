//! The inhibitor seam.
//!
//! A backend is whatever can make the machine stay awake. The service holds one
//! and knows nothing about how it does its job, which is what lets every
//! session path be tested without a real logind — and what lets the Portal and
//! ScreenSaver backends land later without touching the state machine.

use awake_core::{BackendCapabilities, SessionPolicy};
use thiserror::Error;

/// One kind of thing to hold off, named the way logind names it.
///
/// Declared in the order logind writes them in `ListInhibitors`
/// (`inhibit_what_to_string`), so the joined form this service asks for is the
/// form logind lists back and [`crate::LogindBackend`]'s verify can compare the
/// two.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum InhibitWhat {
    Sleep,
    Idle,
    /// Stops logind acting on the lid switch. A `sleep` lock does not, because
    /// `LidSwitchIgnoreInhibited` defaults to `yes` (man logind.conf).
    HandleLidSwitch,
}

impl InhibitWhat {
    pub fn as_str(self) -> &'static str {
        match self {
            InhibitWhat::Sleep => "sleep",
            InhibitWhat::Idle => "idle",
            InhibitWhat::HandleLidSwitch => "handle-lid-switch",
        }
    }
}

/// What the service is asking a backend to hold.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LeaseRequest {
    /// Sorted and deduplicated, so two equivalent requests compare equal and a
    /// lease is not needlessly re-acquired.
    pub what: Vec<InhibitWhat>,
    /// Shown to anyone inspecting system inhibitors.
    pub who: String,
    /// The merged user-facing reason.
    pub why: String,
}

impl LeaseRequest {
    /// The parts of a merged policy that an inhibitor lock can express.
    ///
    /// Display blanking and automatic locking are deliberately absent: logind
    /// has no lock for either, and inventing one by writing a GNOME setting is
    /// exactly what Issue #13 forbids. They are reported as unmet capability
    /// instead. The lid lock is asked for only when the backend said it can
    /// hold one, so a backend without it still holds the rest.
    pub fn from_policy(
        policy: &SessionPolicy,
        capabilities: &BackendCapabilities,
        who: &str,
        why: &str,
    ) -> Option<Self> {
        let mut what = Vec::new();
        if policy.prevent_system_suspend {
            what.push(InhibitWhat::Sleep);
        }
        if policy.prevent_idle {
            what.push(InhibitWhat::Idle);
        }
        if policy.prevent_lid_suspend && capabilities.lid_switch {
            what.push(InhibitWhat::HandleLidSwitch);
        }
        what.sort();
        what.dedup();
        if what.is_empty() {
            return None;
        }
        Some(Self {
            what,
            who: who.to_string(),
            why: why.to_string(),
        })
    }

    /// The colon-separated form logind takes.
    pub fn what_argument(&self) -> String {
        self.what
            .iter()
            .map(|what| what.as_str())
            .collect::<Vec<_>>()
            .join(":")
    }
}

/// Whether a lease the service believes it holds is really still held.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LeaseHealth {
    Held,
    /// The backend no longer lists it. The service must say so rather than keep
    /// showing the machine as protected.
    Lost,
}

#[derive(Clone, Debug, Eq, PartialEq, Error)]
pub enum BackendError {
    #[error("awake.backend.unavailable:{0}")]
    Unavailable(String),
    #[error("awake.backend.denied:{0}")]
    Denied(String),
    #[error("awake.backend.protocol:{0}")]
    Protocol(String),
}

/// Something that can hold the machine awake.
///
/// The associated lease type keeps a real file descriptor out of the shared
/// vocabulary: the logind backend's lease owns an fd, the fake's owns a number,
/// and the service treats both as opaque.
pub trait InhibitorBackend: Send + Sync {
    type Lease: Send + Sync + std::fmt::Debug;

    /// A stable identifier such as `logind`, never a localized name.
    fn name(&self) -> &'static str;

    /// What this backend can do on this machine right now.
    fn probe(
        &self,
    ) -> impl std::future::Future<Output = Result<BackendCapabilities, BackendError>> + Send;

    fn acquire(
        &self,
        request: &LeaseRequest,
    ) -> impl std::future::Future<Output = Result<Self::Lease, BackendError>> + Send;

    /// Asks the backend whether the lease is still in force. An error means the
    /// question could not be answered, which is not the same as `Lost`.
    fn verify(
        &self,
        lease: &Self::Lease,
    ) -> impl std::future::Future<Output = Result<LeaseHealth, BackendError>> + Send;

    fn release(
        &self,
        lease: Self::Lease,
    ) -> impl std::future::Future<Output = Result<(), BackendError>> + Send;

    /// Whether the lid is closed right now, or `None` when that cannot be read
    /// (no lid, or the backend cannot say). The closed-lid battery limit is
    /// measured from this.
    fn lid_closed(&self) -> impl std::future::Future<Output = Option<bool>> + Send;

    /// Whether logind counts the machine as docked: a docking station or an
    /// external display connected. GNOME already keeps a docked machine
    /// awake with the lid closed, so the closed-lid limit does not run then.
    fn docked(&self) -> impl std::future::Future<Output = Option<bool>> + Send;
}

/// `CLOCK_BOOTTIME`, read through `/proc/uptime` so no unsafe call is needed.
/// Falls back to a process-local monotonic clock if it cannot be read.
pub fn boottime_seconds() -> u64 {
    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    std::fs::read_to_string("/proc/uptime")
        .ok()
        .and_then(|text| text.split_whitespace().next()?.parse::<f64>().ok())
        .map(|seconds| seconds as u64)
        .unwrap_or_else(|| {
            START
                .get_or_init(std::time::Instant::now)
                .elapsed()
                .as_secs()
        })
}

/// Reads the clock, so end-condition arithmetic can be driven by a test.
pub trait Clock: Send + Sync {
    fn now_unix_seconds(&self) -> u64;

    /// Seconds on a clock that only moves forward and keeps counting while the
    /// machine is suspended, for measuring how long the lid has been closed.
    /// The wall clock can be set back or forward under a running measurement.
    fn monotonic_seconds(&self) -> u64 {
        boottime_seconds()
    }
}

pub struct SystemClock;

impl Clock for SystemClock {
    fn now_unix_seconds(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_secs())
            .unwrap_or_default()
    }
}

#[cfg(any(test, feature = "test-support"))]
pub use fake::{FakeInhibitorBackend, FakeLease, FixedClock};

#[cfg(any(test, feature = "test-support"))]
mod fake {
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    #[derive(Clone, Debug, Eq, PartialEq)]
    pub struct FakeLease {
        pub id: u64,
        pub request: LeaseRequest,
    }

    /// A backend that records what it was asked for. Nothing in the shipped
    /// binary can construct one.
    #[derive(Debug, Default)]
    pub struct FakeInhibitorBackend {
        inner: Mutex<FakeInner>,
        next_id: AtomicU64,
    }

    #[derive(Debug, Default)]
    struct FakeInner {
        capabilities: Option<BackendCapabilities>,
        probe_failure: Option<BackendError>,
        acquire_failure: Option<BackendError>,
        /// Leases the backend still considers held.
        held: Vec<u64>,
        lid_closed: Option<bool>,
        docked: Option<bool>,
        /// Kinds the fake refuses as polkit would, with `Denied`.
        denied: Vec<InhibitWhat>,
        /// After every acquire and release: whether any lock still held
        /// includes `handle-lid-switch`. A swap that let go of the lid lock
        /// before taking the next one shows up here as a `false`.
        lid_held_timeline: Vec<bool>,
        pub acquired: Vec<LeaseRequest>,
        pub released: Vec<LeaseRequest>,
    }

    impl FakeInhibitorBackend {
        /// The capability shape of a working logind: it can hold sleep and idle
        /// and nothing else.
        pub fn logind_shaped() -> Self {
            Self::with_capabilities(BackendCapabilities {
                system_suspend: true,
                idle: true,
                display_sleep: false,
                automatic_lock: false,
                lid_switch: true,
            })
        }

        /// What the lid reports from now on. `None` is a machine with no lid.
        pub fn set_lid_closed(&self, closed: Option<bool>) {
            self.inner.lock().unwrap().lid_closed = closed;
        }

        /// Whether the machine reports itself docked (an external display).
        pub fn set_docked(&self, docked: Option<bool>) {
            self.inner.lock().unwrap().docked = docked;
        }

        /// Refuses every request that includes `what`, as polkit refusing
        /// one kind of lock would.
        pub fn deny(&self, what: InhibitWhat) {
            self.inner.lock().unwrap().denied.push(what);
        }

        pub fn lid_held_timeline(&self) -> Vec<bool> {
            self.inner.lock().unwrap().lid_held_timeline.clone()
        }

        /// What every lease still held asks for, so a test can say exactly
        /// what `systemd-inhibit --list` would show.
        pub fn held_requests(&self) -> Vec<LeaseRequest> {
            let inner = self.inner.lock().unwrap();
            inner
                .acquired
                .iter()
                .zip(1u64..)
                .filter(|(_, id)| inner.held.contains(id))
                .map(|(request, _)| request.clone())
                .collect()
        }

        pub fn with_capabilities(capabilities: BackendCapabilities) -> Self {
            let backend = Self::default();
            backend.inner.lock().unwrap().capabilities = Some(capabilities);
            backend
        }

        pub fn failing_probe(error: BackendError) -> Self {
            let backend = Self::default();
            backend.inner.lock().unwrap().probe_failure = Some(error);
            backend
        }

        pub fn fail_next_acquire(&self, error: BackendError) {
            self.inner.lock().unwrap().acquire_failure = Some(error);
        }

        /// Makes the backend forget a lease without being asked to release it,
        /// which is what a logind restart looks like from here.
        pub fn drop_lease_behind_our_back(&self) {
            self.inner.lock().unwrap().held.clear();
        }

        pub fn held_count(&self) -> usize {
            self.inner.lock().unwrap().held.len()
        }

        pub fn acquired(&self) -> Vec<LeaseRequest> {
            self.inner.lock().unwrap().acquired.clone()
        }

        pub fn released(&self) -> Vec<LeaseRequest> {
            self.inner.lock().unwrap().released.clone()
        }
    }

    impl FakeInhibitorBackend {
        fn note_lid(inner: &mut FakeInner) {
            let held = inner.acquired.iter().zip(1u64..).any(|(request, id)| {
                inner.held.contains(&id) && request.what.contains(&InhibitWhat::HandleLidSwitch)
            });
            inner.lid_held_timeline.push(held);
        }
    }

    impl InhibitorBackend for FakeInhibitorBackend {
        type Lease = FakeLease;

        fn name(&self) -> &'static str {
            "fake"
        }

        async fn probe(&self) -> Result<BackendCapabilities, BackendError> {
            let inner = self.inner.lock().unwrap();
            if let Some(error) = &inner.probe_failure {
                return Err(error.clone());
            }
            Ok(inner.capabilities.unwrap_or(BackendCapabilities::NONE))
        }

        async fn acquire(&self, request: &LeaseRequest) -> Result<FakeLease, BackendError> {
            let mut inner = self.inner.lock().unwrap();
            if let Some(error) = inner.acquire_failure.take() {
                return Err(error);
            }
            if let Some(what) = request.what.iter().find(|what| inner.denied.contains(what)) {
                return Err(BackendError::Denied(format!(
                    "{} refused by policy",
                    what.as_str()
                )));
            }
            let id = self.next_id.fetch_add(1, Ordering::SeqCst) + 1;
            inner.held.push(id);
            inner.acquired.push(request.clone());
            Self::note_lid(&mut inner);
            Ok(FakeLease {
                id,
                request: request.clone(),
            })
        }

        async fn verify(&self, lease: &FakeLease) -> Result<LeaseHealth, BackendError> {
            let inner = self.inner.lock().unwrap();
            if inner.held.contains(&lease.id) {
                Ok(LeaseHealth::Held)
            } else {
                Ok(LeaseHealth::Lost)
            }
        }

        async fn release(&self, lease: FakeLease) -> Result<(), BackendError> {
            let mut inner = self.inner.lock().unwrap();
            inner.held.retain(|held| *held != lease.id);
            inner.released.push(lease.request);
            Self::note_lid(&mut inner);
            Ok(())
        }

        async fn lid_closed(&self) -> Option<bool> {
            self.inner.lock().unwrap().lid_closed
        }

        async fn docked(&self) -> Option<bool> {
            self.inner.lock().unwrap().docked
        }
    }

    /// A clock a test moves by hand.
    #[derive(Debug, Default)]
    pub struct FixedClock(AtomicU64);

    impl FixedClock {
        pub fn at(now_unix_seconds: u64) -> Self {
            Self(AtomicU64::new(now_unix_seconds))
        }

        pub fn advance(&self, seconds: u64) {
            self.0.fetch_add(seconds, Ordering::SeqCst);
        }

        pub fn set(&self, now_unix_seconds: u64) {
            self.0.store(now_unix_seconds, Ordering::SeqCst);
        }
    }

    impl Clock for FixedClock {
        fn now_unix_seconds(&self) -> u64 {
            self.0.load(Ordering::SeqCst)
        }

        /// The fake moves both clocks together, which is what lets a test
        /// advance the closed-lid measurement by advancing the clock.
        fn monotonic_seconds(&self) -> u64 {
            self.0.load(Ordering::SeqCst)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOGIND: BackendCapabilities = BackendCapabilities {
        system_suspend: true,
        idle: true,
        display_sleep: false,
        automatic_lock: false,
        lid_switch: true,
    };

    #[test]
    fn the_lid_choice_adds_the_lid_switch_lock_in_the_order_logind_lists_it() {
        let request = LeaseRequest::from_policy(
            &SessionPolicy::manual_default(),
            &LOGIND,
            "Better Awake",
            "Download",
        )
        .unwrap();
        assert_eq!(request.what_argument(), "sleep:idle:handle-lid-switch");

        let lid_only = SessionPolicy {
            prevent_lid_suspend: true,
            ..SessionPolicy::default()
        };
        assert_eq!(
            LeaseRequest::from_policy(&lid_only, &LOGIND, "Better Awake", "Lid")
                .unwrap()
                .what_argument(),
            "handle-lid-switch"
        );
    }

    #[test]
    fn a_backend_without_a_lid_lock_is_not_asked_for_one() {
        let without = BackendCapabilities {
            lid_switch: false,
            ..LOGIND
        };
        let request = LeaseRequest::from_policy(
            &SessionPolicy::manual_default(),
            &without,
            "Better Awake",
            "Download",
        )
        .unwrap();
        assert_eq!(request.what_argument(), "sleep:idle");
    }

    #[test]
    fn the_default_policy_asks_logind_for_sleep_and_idle_only() {
        let request = LeaseRequest::from_policy(
            &SessionPolicy::quick_default(),
            &LOGIND,
            "Better Awake",
            "Build",
        )
        .unwrap();
        assert_eq!(request.what_argument(), "sleep:idle");
    }

    #[test]
    fn a_display_only_policy_asks_for_no_lock_because_logind_has_none() {
        let policy = SessionPolicy {
            prevent_system_suspend: false,
            prevent_idle: false,
            prevent_display_sleep: true,
            prevent_automatic_lock: true,
            prevent_lid_suspend: false,
        };
        assert_eq!(
            LeaseRequest::from_policy(&policy, &LOGIND, "Better Awake", "Presenting"),
            None
        );
    }

    #[test]
    fn an_empty_policy_needs_no_lease_at_all() {
        assert_eq!(
            LeaseRequest::from_policy(&SessionPolicy::default(), &LOGIND, "Better Awake", "None"),
            None
        );
    }

    #[tokio::test]
    async fn the_fake_backend_reports_a_lease_it_lost() {
        let backend = FakeInhibitorBackend::logind_shaped();
        let request = LeaseRequest::from_policy(
            &SessionPolicy::quick_default(),
            &LOGIND,
            "Better Awake",
            "Build",
        )
        .unwrap();
        let lease = backend.acquire(&request).await.unwrap();
        assert_eq!(backend.verify(&lease).await.unwrap(), LeaseHealth::Held);

        backend.drop_lease_behind_our_back();
        assert_eq!(backend.verify(&lease).await.unwrap(), LeaseHealth::Lost);
    }
}
