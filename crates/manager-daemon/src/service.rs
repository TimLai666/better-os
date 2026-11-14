//! The D-Bus surface.
//!
//! Bus name `org.betteros.Manager1`, object path `/org/betteros/Manager1`. See
//! ADR 0007 for why plans and outcomes cross as JSON documents while the values
//! the bus itself routes on stay native arguments.

use std::os::fd::AsRawFd;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use manager_ipc::{
    ExecutionStage, MAX_PLAN_BYTES, OutcomeStatus, PROTOCOL_VERSION, TransactionOutcome, WirePlan,
};
use zbus::message::Header;
use zbus::object_server::SignalEmitter;
use zbus::zvariant::OwnedFd;
use zbus::{fdo, interface};

use crate::DaemonError;
use crate::authorize::{APPLY_ACTION, Authorizer, REPAIR_ACTION};
use crate::executor::Executor;
use crate::store::{ArtifactStore, Journal};

pub const BUS_NAME: &str = "org.betteros.Manager1";
pub const OBJECT_PATH: &str = "/org/betteros/Manager1";

fn refuse(error: DaemonError) -> fdo::Error {
    match error {
        DaemonError::Unauthorized => fdo::Error::AccessDenied(error.to_string()),
        DaemonError::Busy => fdo::Error::LimitsExceeded(error.to_string()),
        DaemonError::UnknownTransaction(_) => fdo::Error::UnknownObject(error.to_string()),
        other => fdo::Error::Failed(other.to_string()),
    }
}

/// The door every call comes through, so the daemon can leave without
/// cutting one in half (ticket 105). Releasing the bus name is not that door:
/// for a `Type=dbus` unit systemd treats losing the name as the service
/// stopping and signals it at once. So the daemon first closes the gate —
/// every new call is refused with `daemon.error.shutting_down`, which a client
/// retries — waits until the calls already inside are done, and only then
/// gives the name up and exits.
#[derive(Debug, Default)]
pub struct CallGate {
    closing: AtomicBool,
    inside: AtomicUsize,
}

/// One call inside the gate.
pub struct CallGuard(Arc<CallGate>);

impl Drop for CallGuard {
    fn drop(&mut self) {
        self.0.inside.fetch_sub(1, Ordering::SeqCst);
    }
}

impl CallGate {
    /// Lets a call in, or refuses it once the gate is closing. The count goes
    /// up before the flag is read, and `close` sets the flag before it reads
    /// the count, so a call is either refused or waited for — never neither.
    pub fn enter(self: &Arc<Self>) -> Result<CallGuard, DaemonError> {
        self.inside.fetch_add(1, Ordering::SeqCst);
        let guard = CallGuard(self.clone());
        if self.closing.load(Ordering::SeqCst) {
            return Err(DaemonError::ShuttingDown);
        }
        Ok(guard)
    }

    /// Refuses every call from now on and returns once none is inside.
    pub async fn close(&self, poll: std::time::Duration) {
        self.closing.store(true, Ordering::SeqCst);
        while self.inside.load(Ordering::SeqCst) > 0 {
            tokio::time::sleep(poll).await;
        }
    }

    pub fn is_closing(&self) -> bool {
        self.closing.load(Ordering::SeqCst)
    }

    /// How many calls are inside now.
    pub fn inside(&self) -> usize {
        self.inside.load(Ordering::SeqCst)
    }
}

/// Serves `service` on a connection from `builder`, then owns `bus_name`.
///
/// The interface goes in through `Builder::serve_at`, so `build` returns only
/// once zbus's object server is reading method calls, and the name is asked
/// for after that. An object added to a connection that is already built
/// starts the server lazily, and a call that arrives before it is reading is
/// dropped with no reply (ticket 107); for this daemon that call is the
/// D-Bus activation that started it, and the caller would wait out its
/// timeout.
pub async fn serve<A: Authorizer + 'static>(
    builder: zbus::connection::Builder<'_>,
    bus_name: &str,
    service: ManagerService<A>,
) -> zbus::Result<zbus::Connection> {
    builder
        .serve_at(OBJECT_PATH, service)?
        .name(bus_name)?
        .build()
        .await
}

pub struct ManagerService<A: Authorizer + 'static> {
    authorizer: A,
    executor: Arc<Executor>,
    artifacts: Arc<ArtifactStore>,
    journal: Arc<Journal>,
    /// Only one transaction or repair may run at a time. Two concurrent dpkg
    /// runs would fight over the dpkg lock and interleave in the journal, and a
    /// repair inside a transaction would change packages it is still checking.
    running: Arc<AtomicBool>,
    /// Every call enters through this, so leaving never cuts one in half.
    gate: Arc<CallGate>,
    /// A file that exists exactly while a transaction or a repair runs
    /// (ticket 105). The daemon package's postinst reads it to decide whether
    /// a running daemon may be restarted; `None` in tests that do not ask.
    busy_marker: Option<std::path::PathBuf>,
    /// The runtime this daemon owns, captured where it is unambiguous.
    ///
    /// zbus runs on its default async-io flavor across this workspace, for the
    /// reason `manager-platform::flavor` records: its tokio flavor is a
    /// compile-time feature that cargo unifies into every binary built beside a
    /// crate asking for it, and it panics on the plain threads gpui and
    /// accesskit open their own connections from. The consequence here is that
    /// zbus polls these method bodies on *its* executor, which is not a tokio
    /// thread — so `tokio::spawn` and `tokio::task::spawn_blocking` would panic
    /// looking for a runtime. Every spawn below goes through this handle
    /// instead, which needs no ambient context. See ticket 49.
    runtime: tokio::runtime::Handle,
}

impl<A: Authorizer + 'static> ManagerService<A> {
    pub fn new(
        authorizer: A,
        executor: Arc<Executor>,
        artifacts: Arc<ArtifactStore>,
        journal: Arc<Journal>,
    ) -> Self {
        Self {
            authorizer,
            executor,
            artifacts,
            journal,
            running: Arc::new(AtomicBool::new(false)),
            gate: Arc::new(CallGate::default()),
            busy_marker: None,
            runtime: tokio::runtime::Handle::current(),
        }
    }

    async fn authorize(&self, header: &Header<'_>, action: &'static str) -> Result<(), fdo::Error> {
        match self.authorizer.check(header, action).await {
            Ok(true) => Ok(()),
            Ok(false) => Err(refuse(DaemonError::Unauthorized)),
            // A polkit that cannot be reached is not permission to proceed.
            Err(error) => Err(refuse(error)),
        }
    }

    /// Marks transactions and repairs with a file at `path` while they run.
    /// A marker left by a daemon that died is removed here, at start.
    pub fn with_busy_marker(mut self, path: impl Into<std::path::PathBuf>) -> Self {
        let path = path.into();
        let _ = std::fs::remove_file(&path);
        self.busy_marker = Some(path);
        self
    }

    /// The flag that is set while a transaction or a repair runs, for the
    /// idle timer in `main.rs` to read.
    pub fn activity(&self) -> Arc<AtomicBool> {
        self.running.clone()
    }

    /// The gate `main.rs` closes before the daemon leaves.
    pub fn gate(&self) -> Arc<CallGate> {
        self.gate.clone()
    }

    fn enter(&self) -> Result<CallGuard, fdo::Error> {
        self.gate.enter().map_err(refuse)
    }

    /// Claims the one running slot, or refuses as busy. The slot is released
    /// when the returned guard is dropped, including by a panic.
    fn begin_exclusive(&self) -> Result<RunningGuard, fdo::Error> {
        self.running
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .map_err(|_| refuse(DaemonError::Busy))?;
        if let Some(marker) = &self.busy_marker {
            if let Some(parent) = marker.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::write(marker, format!("{}\n", std::process::id()));
        }
        Ok(RunningGuard {
            running: self.running.clone(),
            marker: self.busy_marker.clone(),
        })
    }

    /// A transaction after authorization. As with a repair, the running slot
    /// and the call's place inside the gate go with the work on the blocking
    /// thread, so a client that disconnects mid-way never lets the daemon
    /// think it is idle while dpkg still runs.
    async fn run_transaction(
        &self,
        plan_json: &str,
        call: Option<CallGuard>,
        emitter: Option<SignalEmitter<'static>>,
    ) -> Result<(String, bool), fdo::Error> {
        if plan_json.len() > MAX_PLAN_BYTES {
            return Err(refuse(DaemonError::Protocol("plan too large".to_string())));
        }
        let plan = WirePlan::from_json(plan_json).map_err(|error| refuse(error.into()))?;

        // Already done? Say what happened instead of doing it twice.
        if let Some(entry) = self.journal.read(&plan.transaction_id).map_err(refuse)?
            && let Some(outcome) = entry.outcome
        {
            return outcome
                .to_json()
                .map(|document| (document, false))
                .map_err(|error| refuse(error.into()));
        }

        let guard = self.begin_exclusive()?;

        let (progress_tx, mut progress_rx) = tokio::sync::mpsc::unbounded_channel();
        let transaction_id = plan.transaction_id.clone();
        let forwarding = self.runtime.spawn(async move {
            while let Some((step_index, stage)) = progress_rx.recv().await {
                if let Some(emitter) = &emitter {
                    let _ = ManagerService::<A>::step_progress(
                        emitter,
                        &transaction_id,
                        step_index,
                        stage_name(stage),
                    )
                    .await;
                }
            }
        });

        let executor = self.executor.clone();
        let outcome = self
            .runtime
            .spawn_blocking(move || {
                let outcome = executor.execute(&plan, &mut |step_index, stage| {
                    let _ = progress_tx.send((step_index, stage));
                });
                drop(guard);
                drop(call);
                outcome
            })
            .await
            .map_err(|error| fdo::Error::Failed(error.to_string()))?;
        let _ = forwarding.await;

        // A rejection before anything ran is still a result the client needs,
        // so it is reported as a failed outcome rather than only as an error.
        let outcome = match outcome {
            Ok(outcome) => outcome,
            Err(error) => TransactionOutcome {
                protocol_version: PROTOCOL_VERSION,
                transaction_id: plan_transaction_id(plan_json),
                status: OutcomeStatus::Failed {
                    step_index: None,
                    error_key: error.to_string(),
                    recovery: None,
                },
                reports: Vec::new(),
                rollback_records: Vec::new(),
            },
        };
        outcome
            .to_json()
            .map(|document| (document, true))
            .map_err(|error| refuse(error.into()))
    }

    /// The repair after authorization: wait for nothing, take the slot or
    /// refuse, and run `dpkg --configure -a` off zbus's executor. The slot,
    /// and the call's place inside the gate, go with the dpkg run rather than
    /// with this future: a client that disconnects drops the future, never
    /// the run, and the daemon keeps counting it as running until dpkg ends.
    async fn run_repair(&self, call: Option<CallGuard>) -> Result<String, fdo::Error> {
        let guard = self.begin_exclusive()?;
        let executor = self.executor.clone();
        let outcome = self
            .runtime
            .spawn_blocking(move || {
                let outcome = executor.repair();
                drop(guard);
                drop(call);
                outcome
            })
            .await
            .map_err(|error| fdo::Error::Failed(error.to_string()))?;
        outcome
            .map_err(refuse)?
            .to_json()
            .map_err(|error| refuse(error.into()))
    }
}

#[interface(name = "org.betteros.Manager1")]
impl<A: Authorizer + 'static> ManagerService<A> {
    /// Copies an artifact into the daemon's cache from an open descriptor,
    /// hashing it on the way in. The file only appears under its final name if
    /// the digest matches, so no path handed over later can point at unverified
    /// bytes.
    async fn stage_artifact(
        &self,
        #[zbus(header)] header: Header<'_>,
        transaction_id: &str,
        filename: &str,
        expected_sha256: &str,
        artifact: OwnedFd,
    ) -> Result<String, fdo::Error> {
        let _call = self.enter()?;
        self.authorize(&header, APPLY_ACTION).await?;
        let _ = transaction_id;

        let artifacts = self.artifacts.clone();
        let filename = filename.to_string();
        let expected = expected_sha256.to_string();

        self.runtime
            .spawn_blocking(move || {
                // Take ownership of the descriptor so it is closed when this ends.
                let file = unsafe {
                    use std::os::fd::FromRawFd;
                    std::fs::File::from_raw_fd(artifact.as_raw_fd())
                };
                let mut file = std::mem::ManuallyDrop::new(file);
                artifacts.stage(&filename, &expected, &mut *file)
            })
            .await
            .map_err(|error| fdo::Error::Failed(error.to_string()))?
            .map_err(refuse)
    }

    /// Revalidates and carries out a whole transaction.
    ///
    /// Re-sending a transaction that already finished returns what it did
    /// rather than doing it again, so a client that lost the connection can ask
    /// safely.
    async fn apply_transaction(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
        plan_json: &str,
    ) -> Result<String, fdo::Error> {
        let call = self.enter()?;
        self.authorize(&header, APPLY_ACTION).await?;
        let (document, ran) = self
            .run_transaction(plan_json, Some(call), Some(emitter.to_owned()))
            .await?;
        if ran && let Ok(outcome) = TransactionOutcome::from_json(&document) {
            let _ = Self::transaction_completed(&emitter, &outcome.transaction_id, &document).await;
        }
        Ok(document)
    }

    /// What the daemon remembers about a transaction. Ungated: it reports
    /// progress, not secrets, and a client needs it after a restart.
    async fn get_status(&self, transaction_id: &str) -> Result<String, fdo::Error> {
        let _call = self.enter()?;
        let entry = self
            .journal
            .read(transaction_id)
            .map_err(refuse)?
            .ok_or_else(|| refuse(DaemonError::UnknownTransaction(transaction_id.to_string())))?;
        serde_json::to_string(&entry).map_err(|error| fdo::Error::Failed(error.to_string()))
    }

    /// Cancellation is only honest before APT has started. Once packages are
    /// being applied, stopping would leave the host in a state nothing has
    /// described, so the request is refused rather than half-honored.
    async fn cancel(
        &self,
        #[zbus(header)] header: Header<'_>,
        transaction_id: &str,
    ) -> Result<bool, fdo::Error> {
        let _call = self.enter()?;
        self.authorize(&header, APPLY_ACTION).await?;
        match self.journal.read(transaction_id).map_err(refuse)? {
            None => Ok(true),
            Some(entry) => Ok(!matches!(
                entry.state,
                crate::store::JournalState::Executing { .. }
                    | crate::store::JournalState::Completed
            )),
        }
    }

    /// What dpkg's own record says about whether its last run finished.
    /// Ungated, like `GetStatus`: it reads a world-readable record and changes
    /// nothing. ADR 0014.
    async fn get_package_system_state(&self) -> Result<String, fdo::Error> {
        let _call = self.enter()?;
        let executor = self.executor.clone();
        self.runtime
            .spawn_blocking(move || executor.apt.package_system_state())
            .await
            .map_err(|error| fdo::Error::Failed(error.to_string()))?
            .map_err(refuse)?
            .to_json()
            .map_err(|error| refuse(error.into()))
    }

    /// Runs `dpkg --configure -a` and reports what it did. Takes no argument:
    /// nothing about the command comes from the caller. Gated by its own
    /// polkit action, and refused while a transaction runs or when dpkg's
    /// record shows nothing to repair. ADR 0014.
    async fn repair_package_system(
        &self,
        #[zbus(header)] header: Header<'_>,
    ) -> Result<String, fdo::Error> {
        let call = self.enter()?;
        self.authorize(&header, REPAIR_ACTION).await?;
        self.run_repair(Some(call)).await
    }

    #[zbus(signal)]
    async fn step_progress(
        emitter: &SignalEmitter<'_>,
        transaction_id: &str,
        step_index: u32,
        stage: &str,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn transaction_completed(
        emitter: &SignalEmitter<'_>,
        transaction_id: &str,
        outcome_json: &str,
    ) -> zbus::Result<()>;

    #[zbus(property)]
    async fn protocol_version(&self) -> u32 {
        PROTOCOL_VERSION
    }
}

/// Returns once the daemon may exit: `idle` has passed with no transaction or
/// repair running.
///
/// A fixed sleep from startup used to end the process at 120 seconds whatever
/// it was doing, and a repair or a long install could take longer than that;
/// the caller then got a dropped connection instead of its outcome. While
/// something runs the timer waits, and once it finishes the idle period starts
/// again, so the caller has time to ask what happened.
pub async fn wait_until_idle(running: Arc<AtomicBool>, idle: std::time::Duration) {
    wait_until_idle_or_replaced(running, idle, idle, || false).await;
}

/// [`wait_until_idle`], and sooner once an update has replaced the daemon's
/// own program (ticket 105): `grace` after the last transaction, which is long
/// enough for its caller to ask how it ended, rather than the whole idle
/// period. The next request then starts the new program by D-Bus activation.
///
/// This is how the daemon takes part in its own update. The package's
/// maintainer scripts leave the unit alone when they run inside it — stopping
/// it there would kill the `dpkg` run that is installing the package — so the
/// running daemon finishes the transaction, records it, and leaves.
pub async fn wait_until_idle_or_replaced(
    running: Arc<AtomicBool>,
    idle: std::time::Duration,
    grace: std::time::Duration,
    replaced: impl Fn() -> bool,
) {
    let poll = idle.min(grace).min(std::time::Duration::from_secs(1));
    let mut quiet_since = tokio::time::Instant::now();
    loop {
        tokio::time::sleep(poll).await;
        if running.load(Ordering::SeqCst) {
            quiet_since = tokio::time::Instant::now();
            continue;
        }
        let quiet = quiet_since.elapsed();
        if quiet >= idle || (quiet >= grace && replaced()) {
            return;
        }
    }
}

/// Releases the single-transaction flag even if the work panicked.
struct RunningGuard {
    running: Arc<AtomicBool>,
    marker: Option<std::path::PathBuf>,
}

impl Drop for RunningGuard {
    fn drop(&mut self) {
        if let Some(marker) = &self.marker {
            let _ = std::fs::remove_file(marker);
        }
        self.running.store(false, Ordering::SeqCst);
    }
}

fn stage_name(stage: ExecutionStage) -> &'static str {
    match stage {
        ExecutionStage::Verifying => "verifying",
        ExecutionStage::Applying => "applying",
        ExecutionStage::CheckingHealth => "checking_health",
        ExecutionStage::RollingBack => "rolling_back",
    }
}

/// Recovers the transaction id from a document the executor refused, so even a
/// rejected plan reports under the id the client used.
fn plan_transaction_id(plan_json: &str) -> String {
    serde_json::from_str::<serde_json::Value>(plan_json)
        .ok()
        .and_then(|value| {
            value
                .get("transaction_id")
                .and_then(|id| id.as_str().map(str::to_string))
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stage_names_are_stable_wire_values() {
        assert_eq!(stage_name(ExecutionStage::Verifying), "verifying");
        assert_eq!(
            stage_name(ExecutionStage::CheckingHealth),
            "checking_health"
        );
    }

    #[test]
    fn a_rejected_plan_still_reports_under_the_id_the_client_used() {
        let document = r#"{"transaction_id":"3f2504e0-4f89-41d3-9a0c-0305e82c3301"}"#;
        assert_eq!(
            plan_transaction_id(document),
            "3f2504e0-4f89-41d3-9a0c-0305e82c3301"
        );
        assert_eq!(plan_transaction_id("not json"), "");
    }

    /// A repair and a transaction share one running flag. Two dpkg runs at
    /// once would fight over dpkg's lock, and a repair in the middle of a
    /// transaction would change packages the transaction is still checking.
    #[tokio::test]
    async fn a_repair_waits_its_turn_behind_a_running_transaction() {
        use crate::apt::FakeAptDriver;
        use crate::authorize::FakeAuthorizer;
        use crate::health::FakeHealthProbe;
        use crate::host::FixedHostProbe;
        use manager_ipc::{DpkgPackageStatus, InterruptedPackage, PackageSystemState};

        let root = std::env::temp_dir().join(format!(
            "better-os-service-serialized-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let artifacts = Arc::new(ArtifactStore::new(root.join("archives")));
        let journal = Arc::new(Journal::new(root.join("state")));
        let apt = Arc::new(
            FakeAptDriver::new().with_package_system(PackageSystemState {
                protocol_version: PROTOCOL_VERSION,
                journal_pending: false,
                packages: vec![InterruptedPackage {
                    package: "better-monitor".to_string(),
                    status: DpkgPackageStatus::Unpacked,
                }],
                omitted: 0,
            }),
        );
        let executor = Arc::new(Executor {
            apt: apt.clone(),
            host: Arc::new(FixedHostProbe::ubuntu_2404()),
            health: Arc::new(FakeHealthProbe::empty()),
            artifacts: artifacts.clone(),
            journal: journal.clone(),
        });
        let service = ManagerService::new(FakeAuthorizer(true), executor, artifacts, journal);

        // What `ApplyTransaction` holds while a transaction runs.
        let transaction = service.begin_exclusive().unwrap();
        let refused = service.run_repair(None).await.unwrap_err();
        assert!(
            matches!(&refused, fdo::Error::LimitsExceeded(message) if message == "daemon.error.busy"),
            "{refused:?}"
        );
        assert!(apt.calls().is_empty(), "dpkg must not have run");

        drop(transaction);
        let outcome =
            manager_ipc::RepairOutcome::from_json(&service.run_repair(None).await.unwrap())
                .unwrap();
        assert_eq!(outcome.status, manager_ipc::RepairStatus::Repaired);

        // The repair gave the flag back when it finished.
        assert!(service.begin_exclusive().is_ok());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The daemon exits after an idle period. A repair that runs past that
    /// period must still reach its caller, so the timer waits for it.
    #[tokio::test(flavor = "multi_thread")]
    async fn the_idle_timer_waits_for_a_repair_that_outlasts_it() {
        use crate::apt::FakeAptDriver;
        use crate::authorize::FakeAuthorizer;
        use crate::health::FakeHealthProbe;
        use crate::host::FixedHostProbe;
        use manager_ipc::{DpkgPackageStatus, InterruptedPackage, PackageSystemState};
        use std::time::Duration;

        let root =
            std::env::temp_dir().join(format!("better-os-service-idle-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let artifacts = Arc::new(ArtifactStore::new(root.join("archives")));
        let journal = Arc::new(Journal::new(root.join("state")));
        let apt = Arc::new(
            FakeAptDriver::new()
                .with_package_system(PackageSystemState {
                    protocol_version: PROTOCOL_VERSION,
                    journal_pending: true,
                    packages: vec![InterruptedPackage {
                        package: "linux-image-6.8.0-31-generic".to_string(),
                        status: DpkgPackageStatus::HalfConfigured,
                    }],
                    omitted: 0,
                })
                // Four times the idle period below.
                .with_configure_duration(Duration::from_millis(400)),
        );
        let executor = Arc::new(Executor {
            apt: apt.clone(),
            host: Arc::new(FixedHostProbe::ubuntu_2404()),
            health: Arc::new(FakeHealthProbe::empty()),
            artifacts: artifacts.clone(),
            journal: journal.clone(),
        });
        let service = Arc::new(ManagerService::new(
            FakeAuthorizer(true),
            executor,
            artifacts,
            journal,
        ));

        let repairing = {
            let service = service.clone();
            tokio::spawn(async move { service.run_repair(None).await })
        };
        // Let the repair claim the running slot before the timer starts.
        while !service.activity().load(Ordering::SeqCst) {
            tokio::task::yield_now().await;
        }
        let idle = tokio::spawn(wait_until_idle(
            service.activity(),
            Duration::from_millis(100),
        ));

        let first = tokio::select! {
            _ = idle => "idle",
            _ = tokio::time::sleep(Duration::from_millis(300)) => "still waiting",
        };
        assert_eq!(first, "still waiting", "the daemon would exit mid-repair");

        let outcome = repairing.await.unwrap().unwrap();
        assert_eq!(
            manager_ipc::RepairOutcome::from_json(&outcome)
                .unwrap()
                .status,
            manager_ipc::RepairStatus::Repaired
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn the_idle_timer_ends_after_the_idle_period_when_nothing_runs() {
        let running = Arc::new(AtomicBool::new(false));
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            wait_until_idle(running, std::time::Duration::from_millis(50)),
        )
        .await
        .expect("an idle daemon exits");
    }

    #[tokio::test]
    async fn a_replaced_daemon_leaves_after_the_grace_period_rather_than_the_idle_one() {
        use std::time::Duration;
        let running = Arc::new(AtomicBool::new(false));
        tokio::time::timeout(
            Duration::from_secs(5),
            wait_until_idle_or_replaced(
                running.clone(),
                Duration::from_secs(3600),
                Duration::from_millis(50),
                || true,
            ),
        )
        .await
        .expect("a replaced daemon exits once its caller has had time");

        let still = tokio::time::timeout(
            Duration::from_millis(300),
            wait_until_idle_or_replaced(
                running,
                Duration::from_secs(3600),
                Duration::from_millis(50),
                || false,
            ),
        )
        .await;
        assert!(still.is_err(), "a daemon that was not replaced waits");
    }

    #[tokio::test]
    async fn a_replaced_daemon_still_finishes_its_transaction() {
        use std::time::Duration;
        let running = Arc::new(AtomicBool::new(true));
        let waiting = tokio::spawn(wait_until_idle_or_replaced(
            running.clone(),
            Duration::from_secs(3600),
            Duration::from_millis(50),
            || true,
        ));
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(!waiting.is_finished(), "it never leaves mid-transaction");
        running.store(false, Ordering::SeqCst);
        tokio::time::timeout(Duration::from_secs(5), waiting)
            .await
            .expect("it leaves once the transaction is over")
            .unwrap();
    }

    #[tokio::test]
    async fn a_closing_gate_refuses_new_calls_and_waits_for_the_ones_inside() {
        use std::time::Duration;
        let gate = Arc::new(CallGate::default());
        let inside = gate.enter().expect("open gate lets a call in");
        let closing = {
            let gate = gate.clone();
            tokio::spawn(async move { gate.close(Duration::from_millis(10)).await })
        };
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(gate.is_closing());
        assert!(
            matches!(gate.enter(), Err(DaemonError::ShuttingDown)),
            "a call after the gate closes is refused, retryably"
        );
        assert!(!closing.is_finished(), "the call inside is waited for");
        drop(inside);
        tokio::time::timeout(Duration::from_secs(5), closing)
            .await
            .expect("closes once the call inside is done")
            .unwrap();
        assert_eq!(
            refuse(DaemonError::ShuttingDown).to_string(),
            "org.freedesktop.DBus.Error.Failed: daemon.error.shutting_down"
        );
    }

    #[test]
    fn an_unauthorized_caller_is_refused_with_access_denied() {
        assert!(matches!(
            refuse(DaemonError::Unauthorized),
            fdo::Error::AccessDenied(_)
        ));
        assert!(matches!(
            refuse(DaemonError::Busy),
            fdo::Error::LimitsExceeded(_)
        ));
    }
}
