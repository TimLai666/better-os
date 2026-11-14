//! The client half of the privileged protocol.
//!
//! This is the only thing in an unprivileged process that can cause the host to
//! change, and it cannot do so on its own: every method is a request to the
//! daemon, which authorizes the caller through polkit and revalidates the plan
//! before acting. See ADR 0007.

use std::path::Path;

use manager_ipc::{
    ExecutionStage, PackageSystemState, RepairOutcome, TransactionOutcome, WirePlan,
};
use zbus::blocking::{Connection, Proxy};

use crate::{PackageSystemRepair, PlatformError, PrivilegedTransactionExecutor};

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
        // zbus runs on its default async-io flavor, which owns its own reactor
        // and can therefore be called from any thread. This used to enter a
        // tokio runtime first, because workspace feature unification compiled
        // zbus's `tokio` flavor into every binary and that flavor panics with
        // "there is no reactor running" off a tokio thread. The guard only ever
        // covered this crate's own calls; gpui's portal and accesskit make
        // their own connections and could not be guarded, so ticket 49 removed
        // the feature instead. `flavor.rs` keeps it removed.
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

/// How often, and how far apart, a call refused by a daemon that is leaving is
/// asked again. The next call after it has gone starts a new daemon.
const LEAVING_ATTEMPTS: u32 = 40;
const LEAVING_PAUSE: std::time::Duration = std::time::Duration::from_millis(250);

/// Whether the daemon refused because it is leaving (ticket 105).
fn daemon_is_leaving(error: &zbus::Error) -> bool {
    error.to_string().contains("daemon.error.shutting_down")
}

/// Makes a call, asking again while the daemon says it is leaving.
fn call_while_leaving<T>(mut call: impl FnMut() -> zbus::Result<T>) -> Result<T, PlatformError> {
    retry_while_leaving(&mut call, LEAVING_ATTEMPTS, &|| {
        std::thread::sleep(LEAVING_PAUSE)
    })
}

fn retry_while_leaving<T>(
    call: &mut dyn FnMut() -> zbus::Result<T>,
    attempts: u32,
    pause: &dyn Fn(),
) -> Result<T, PlatformError> {
    let mut left = attempts.max(1);
    loop {
        match call() {
            Err(error) if daemon_is_leaving(&error) && left > 1 => {
                left -= 1;
                pause();
            }
            other => return other.map_err(translate),
        }
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
    // The service vanished mid-call: the bus answers for it that it left
    // without replying, or the reply's connection closed under the call.
    if text.contains("org.freedesktop.DBus.Error.NoReply")
        || text.contains("disconnected from message bus without replying")
    {
        return PlatformError::DaemonInterrupted(text);
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
        // Handing over a descriptor rather than a path means the daemon reads
        // exactly the bytes this client opened, with no window in which
        // something else could swap the file. A call asked again opens it
        // again.
        let proxy = self.proxy()?;
        let verified: String = call_while_leaving(|| {
            let file = std::fs::File::open(artifact_path)
                .map_err(|error| zbus::Error::Failure(error.to_string()))?;
            let descriptor = zbus::zvariant::OwnedFd::from(std::os::fd::OwnedFd::from(file));
            proxy.call(
                "StageArtifact",
                &(transaction_id, filename, sha256, descriptor),
            )
        })?;

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
        let proxy = self.proxy()?;
        let outcome: String =
            call_while_leaving(|| proxy.call("ApplyTransaction", &(document.as_str(),)))?;

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

impl PackageSystemRepair for DbusPrivilegedExecutor {
    fn package_system_state(&self) -> Result<PackageSystemState, PlatformError> {
        let proxy = self.proxy()?;
        let document: String = call_while_leaving(|| proxy.call("GetPackageSystemState", &()))?;
        PackageSystemState::from_json(&document)
            .map_err(|error| PlatformError::DaemonRefused(error.to_string()))
    }

    fn repair_package_system(&self) -> Result<RepairOutcome, PlatformError> {
        // The method takes nothing. Which command runs is the daemon's decision
        // and is fixed there; ADR 0014.
        let proxy = self.proxy()?;
        let document: String = call_while_leaving(|| proxy.call("RepairPackageSystem", &()))?;
        RepairOutcome::from_json(&document)
            .map_err(|error| PlatformError::DaemonRefused(error.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_call_refused_by_a_leaving_daemon_is_asked_again_and_nothing_else_is() {
        use std::cell::Cell;
        let calls = Cell::new(0);
        let pauses = Cell::new(0);
        let pause = || pauses.set(pauses.get() + 1);
        let mut leaving_twice = || -> zbus::Result<u32> {
            calls.set(calls.get() + 1);
            if calls.get() <= 2 {
                Err(zbus::Error::Failure(
                    "org.freedesktop.DBus.Error.Failed: daemon.error.shutting_down".into(),
                ))
            } else {
                Ok(7)
            }
        };
        assert_eq!(
            retry_while_leaving(&mut leaving_twice, 10, &pause).unwrap(),
            7
        );
        assert_eq!((calls.get(), pauses.get()), (3, 2));

        calls.set(0);
        let mut busy = || -> zbus::Result<u32> {
            calls.set(calls.get() + 1);
            Err(zbus::Error::Failure("daemon.error.busy".into()))
        };
        assert!(retry_while_leaving(&mut busy, 10, &pause).is_err());
        assert_eq!(calls.get(), 1, "any other refusal is reported at once");

        calls.set(0);
        let mut always_leaving = || -> zbus::Result<u32> {
            calls.set(calls.get() + 1);
            Err(zbus::Error::Failure("daemon.error.shutting_down".into()))
        };
        let refused = retry_while_leaving(&mut always_leaving, 4, &pause).unwrap_err();
        assert_eq!(calls.get(), 4, "and gives up after the attempts it has");
        assert_eq!(
            refused.to_string(),
            "platform.error.daemon_refused:daemon.error.shutting_down"
        );
    }

    /// Ticket 105: an update of the daemon's package from outside Better
    /// Manager restarts the daemon; a call it was answering — staging an
    /// artifact — gets no reply. That is told apart from a refusal, so the
    /// window can say the update cut the step off and to try again.
    #[test]
    fn a_daemon_that_left_mid_call_is_reported_as_interrupted() {
        let cut_off = translate(zbus::Error::Failure(
            "org.freedesktop.DBus.Error.NoReply: Message recipient disconnected from message bus without replying".to_string(),
        ));
        assert!(
            matches!(cut_off, PlatformError::DaemonInterrupted(_)),
            "{cut_off:?}"
        );
        assert!(
            cut_off
                .to_string()
                .starts_with("platform.error.daemon_interrupted:")
        );
    }

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

    /// A daemon older than ADR 0014 has no repair method. That is a refusal
    /// to report, not a repair that ran or a daemon that is missing.
    #[test]
    fn a_daemon_without_the_repair_method_is_reported_as_a_refusal() {
        let refused = translate(zbus::Error::MethodError(
            "org.freedesktop.DBus.Error.UnknownMethod"
                .try_into()
                .unwrap(),
            Some("Unknown method 'RepairPackageSystem'".to_string()),
            zbus::message::Message::method_call("/", "Ping")
                .unwrap()
                .build(&())
                .unwrap(),
        ));
        assert!(
            matches!(&refused, PlatformError::DaemonRefused(detail) if detail.contains("UnknownMethod")),
            "{refused:?}"
        );
    }

    #[test]
    fn a_repair_refusal_keeps_the_daemons_key() {
        let refused = translate(zbus::Error::Failure(
            "daemon.error.nothing_to_repair".to_string(),
        ));
        assert_eq!(
            refused.to_string(),
            "platform.error.daemon_refused:daemon.error.nothing_to_repair"
        );
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
