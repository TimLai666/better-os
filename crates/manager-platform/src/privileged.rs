//! The client half of the privileged protocol.
//!
//! This is the only thing in an unprivileged process that can cause the host to
//! change, and it cannot do so on its own: every method is a request to the
//! daemon, which authorizes the caller through polkit and revalidates the plan
//! before acting. See ADR 0007.

use std::path::Path;

use manager_ipc::{ExecutionStage, TransactionOutcome, WirePlan};
use zbus::blocking::{Connection, Proxy};

use crate::{PlatformError, PrivilegedTransactionExecutor};

const BUS_NAME: &str = "org.betteros.Manager1";
const OBJECT_PATH: &str = "/org/betteros/Manager1";
const INTERFACE: &str = "org.betteros.Manager1";

/// Talks to the privileged service over the D-Bus system bus.
///
/// There is no `Default` and no way to build one without a system bus
/// connection: an executor that can change the host should not be something a
/// caller can produce by accident.
pub struct DbusPrivilegedExecutor {
    connection: Connection,
}

impl DbusPrivilegedExecutor {
    /// Connects to the privileged service, failing if it is not there.
    ///
    /// A missing daemon is reported as such rather than silently degrading to a
    /// simulation: a user who asked to install something needs to know it did
    /// not happen.
    pub fn connect() -> Result<Self, PlatformError> {
        let connection = Connection::system()
            .map_err(|error| PlatformError::DaemonUnavailable(error.to_string()))?;
        let executor = Self { connection };
        // Reading the protocol version both proves the service is reachable and
        // that it speaks a version this client understands.
        let version = executor.protocol_version()?;
        if version != manager_ipc::PROTOCOL_VERSION {
            return Err(PlatformError::DaemonRefused(format!(
                "daemon.error.protocol:{version}"
            )));
        }
        Ok(executor)
    }

    fn proxy(&self) -> Result<Proxy<'_>, PlatformError> {
        Proxy::new(&self.connection, BUS_NAME, OBJECT_PATH, INTERFACE)
            .map_err(|error| PlatformError::DaemonUnavailable(error.to_string()))
    }

    pub fn protocol_version(&self) -> Result<u32, PlatformError> {
        self.proxy()?
            .get_property("ProtocolVersion")
            .map_err(|error| PlatformError::DaemonUnavailable(error.to_string()))
    }
}

/// Turns a bus error back into something the presentation layer can localize.
///
/// The daemon's own stable keys are carried through rather than reworded, so a
/// user-visible message can name the actual reason.
fn translate(error: zbus::Error) -> PlatformError {
    let text = error.to_string();
    if text.contains("daemon.error.unauthorized") {
        return PlatformError::PolkitDenied;
    }
    if text.contains("org.freedesktop.DBus.Error.ServiceUnknown")
        || text.contains("org.freedesktop.DBus.Error.NameHasNoOwner")
        || text.contains("was not provided by any .service files")
    {
        return PlatformError::DaemonUnavailable(text);
    }
    match text.split_once("daemon.error.") {
        Some((_, key)) => PlatformError::DaemonRefused(format!("daemon.error.{key}")),
        None => PlatformError::DaemonRefused(text),
    }
}

impl PrivilegedTransactionExecutor for DbusPrivilegedExecutor {
    fn stage_artifact(
        &self,
        transaction_id: &str,
        filename: &str,
        sha256: &str,
        artifact_path: &Path,
    ) -> Result<(), PlatformError> {
        let file = std::fs::File::open(artifact_path)
            .map_err(|error| PlatformError::DaemonRefused(error.to_string()))?;
        let descriptor = zbus::zvariant::OwnedFd::from(std::os::fd::OwnedFd::from(file));

        // Handing over a descriptor rather than a path means the daemon reads
        // exactly the bytes this client opened, with no window in which
        // something else could swap the file.
        let verified: String = self
            .proxy()?
            .call(
                "StageArtifact",
                &(transaction_id, filename, sha256, descriptor),
            )
            .map_err(translate)?;

        if verified != sha256 {
            return Err(PlatformError::ChecksumMismatch {
                component: better_core::ComponentId::new("better-os")
                    .expect("a literal component id"),
            });
        }
        Ok(())
    }

    fn execute_plan(
        &self,
        plan: &WirePlan,
        progress: &mut dyn FnMut(u32, ExecutionStage),
    ) -> Result<TransactionOutcome, PlatformError> {
        let document = plan
            .to_json()
            .map_err(|error| PlatformError::DaemonRefused(error.to_string()))?;

        // The call carries the result, so a client that never sees a progress
        // signal still learns everything that happened. Streaming per-step
        // progress would mean reading signals while this call is outstanding,
        // which a blocking connection cannot do on one thread — and watching
        // them on another deadlocks, because the signal iterator does not end
        // when the call returns. The long part of a transaction is the
        // download, and that reports progress from this side already.
        let outcome: String = self
            .proxy()?
            .call("ApplyTransaction", &(document,))
            .map_err(translate)?;

        let outcome = TransactionOutcome::from_json(&outcome)
            .map_err(|error| PlatformError::DaemonRefused(error.to_string()))?;

        // Replay what the outcome recorded, so a caller that could not watch
        // the signals still sees every step it covered.
        for (index, _report) in outcome.reports.iter().enumerate() {
            progress(index as u32, ExecutionStage::CheckingHealth);
        }
        Ok(outcome)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refusal_from_polkit_is_told_apart_from_the_service_being_absent() {
        let denied = translate(zbus::Error::Failure(
            "daemon.error.unauthorized".to_string(),
        ));
        assert!(matches!(denied, PlatformError::PolkitDenied));

        let absent = translate(zbus::Error::Failure(
            "org.freedesktop.DBus.Error.ServiceUnknown: no such service".to_string(),
        ));
        assert!(matches!(absent, PlatformError::DaemonUnavailable(_)));
    }

    #[test]
    fn a_daemon_error_key_survives_the_trip_back() {
        let refused = translate(zbus::Error::Failure(
            "daemon.error.apt_failed:better-monitor".to_string(),
        ));
        assert_eq!(
            refused.to_string(),
            "platform.error.daemon_refused:daemon.error.apt_failed:better-monitor"
        );
    }
}
