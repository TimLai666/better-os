//! The D-Bus surface.
//!
//! Bus name `org.betteros.Manager1`, object path `/org/betteros/Manager1`. See
//! ADR 0007 for why plans and outcomes cross as JSON documents while the values
//! the bus itself routes on stay native arguments.

use std::os::fd::AsRawFd;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use manager_ipc::{
    ExecutionStage, MAX_PLAN_BYTES, OutcomeStatus, PROTOCOL_VERSION, TransactionOutcome, WirePlan,
};
use zbus::message::Header;
use zbus::object_server::SignalEmitter;
use zbus::zvariant::OwnedFd;
use zbus::{fdo, interface};

use crate::DaemonError;
use crate::authorize::Authorizer;
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

pub struct ManagerService<A: Authorizer + 'static> {
    authorizer: A,
    executor: Arc<Executor>,
    artifacts: Arc<ArtifactStore>,
    journal: Arc<Journal>,
    /// Only one transaction may run at a time. Two concurrent APT runs would
    /// fight over the dpkg lock and interleave in the journal.
    running: Arc<AtomicBool>,
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
        }
    }

    async fn authorize(&self, header: &Header<'_>) -> Result<(), fdo::Error> {
        match self.authorizer.check(header).await {
            Ok(true) => Ok(()),
            Ok(false) => Err(refuse(DaemonError::Unauthorized)),
            // A polkit that cannot be reached is not permission to proceed.
            Err(error) => Err(refuse(error)),
        }
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
        self.authorize(&header).await?;
        let _ = transaction_id;

        let artifacts = self.artifacts.clone();
        let filename = filename.to_string();
        let expected = expected_sha256.to_string();

        tokio::task::spawn_blocking(move || {
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
        self.authorize(&header).await?;

        if plan_json.len() > MAX_PLAN_BYTES {
            return Err(refuse(DaemonError::Protocol("plan too large".to_string())));
        }
        let plan = WirePlan::from_json(plan_json).map_err(|error| refuse(error.into()))?;

        // Already done? Say what happened instead of doing it twice.
        if let Some(entry) = self.journal.read(&plan.transaction_id).map_err(refuse)?
            && let Some(outcome) = entry.outcome
        {
            return outcome.to_json().map_err(|error| refuse(error.into()));
        }

        if self
            .running
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Err(refuse(DaemonError::Busy));
        }
        let guard = RunningGuard(self.running.clone());

        let (progress_tx, mut progress_rx) = tokio::sync::mpsc::unbounded_channel();
        let transaction_id = plan.transaction_id.clone();
        let signal_emitter = emitter.to_owned();
        let forwarding = tokio::spawn(async move {
            while let Some((step_index, stage)) = progress_rx.recv().await {
                let _ = ManagerService::<A>::step_progress(
                    &signal_emitter,
                    &transaction_id,
                    step_index,
                    stage_name(stage),
                )
                .await;
            }
        });

        let executor = self.executor.clone();
        let outcome = tokio::task::spawn_blocking(move || {
            executor.execute(&plan, &mut |step_index, stage| {
                let _ = progress_tx.send((step_index, stage));
            })
        })
        .await
        .map_err(|error| fdo::Error::Failed(error.to_string()))?;
        let _ = forwarding.await;
        drop(guard);

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

        let document = outcome.to_json().map_err(|error| refuse(error.into()))?;
        let _ = Self::transaction_completed(&emitter, &outcome.transaction_id, &document).await;
        Ok(document)
    }

    /// What the daemon remembers about a transaction. Ungated: it reports
    /// progress, not secrets, and a client needs it after a restart.
    async fn get_status(&self, transaction_id: &str) -> Result<String, fdo::Error> {
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
        self.authorize(&header).await?;
        match self.journal.read(transaction_id).map_err(refuse)? {
            None => Ok(true),
            Some(entry) => Ok(!matches!(
                entry.state,
                crate::store::JournalState::Executing { .. }
                    | crate::store::JournalState::Completed
            )),
        }
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

/// Releases the single-transaction flag even if the work panicked.
struct RunningGuard(Arc<AtomicBool>);

impl Drop for RunningGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
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
