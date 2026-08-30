//! The systemd-logind inhibitor backend.
//!
//! `org.freedesktop.login1.Manager.Inhibit` hands back a file descriptor. The
//! lock lasts exactly as long as that descriptor is open, which is why the
//! service holds it and no shell command is ever run: there is nothing to run,
//! and a `systemd-inhibit` child process would tie the lock to a process the
//! service does not control.
//!
//! Block mode is used rather than delay mode. A delay lock only postpones a
//! suspend, which is not what "keep this computer awake" means.

use awake_core::BackendCapabilities;
use zbus::zvariant::OwnedFd;

use crate::backend::{BackendError, InhibitorBackend, LeaseHealth, LeaseRequest};

/// A held lock. Dropping the descriptor releases it, so `Drop` alone is a
/// correct release path even if the service dies without asking.
#[derive(Debug)]
pub struct LogindLease {
    _descriptor: OwnedFd,
    what: String,
}

#[zbus::proxy(
    interface = "org.freedesktop.login1.Manager",
    default_service = "org.freedesktop.login1",
    default_path = "/org/freedesktop/login1"
)]
trait LogindManager {
    fn inhibit(&self, what: &str, who: &str, why: &str, mode: &str) -> zbus::Result<OwnedFd>;

    /// what, who, why, mode, uid, pid.
    #[zbus(name = "ListInhibitors")]
    fn list_inhibitors(&self) -> zbus::Result<Vec<(String, String, String, String, u32, u32)>>;
}

pub struct LogindBackend {
    connection: zbus::Connection,
}

impl LogindBackend {
    /// logind lives on the system bus, but taking an inhibitor there needs no
    /// privileges and no polkit action: any session user may hold one.
    pub async fn connect() -> Result<Self, BackendError> {
        let connection = zbus::Connection::system()
            .await
            .map_err(|error| BackendError::Unavailable(error.to_string()))?;
        Ok(Self { connection })
    }

    pub fn with_connection(connection: zbus::Connection) -> Self {
        Self { connection }
    }

    async fn proxy(&self) -> Result<LogindManagerProxy<'_>, BackendError> {
        LogindManagerProxy::new(&self.connection)
            .await
            .map_err(|error| BackendError::Unavailable(error.to_string()))
    }
}

impl InhibitorBackend for LogindBackend {
    type Lease = LogindLease;

    fn name(&self) -> &'static str {
        "logind"
    }

    async fn probe(&self) -> Result<BackendCapabilities, BackendError> {
        // Listing is the cheapest call that proves logind is really answering.
        self.proxy()
            .await?
            .list_inhibitors()
            .await
            .map_err(|error| BackendError::Unavailable(error.to_string()))?;

        // logind can hold off sleep and idle handling. It has no lock for
        // display blanking or for the screen lock; those need the Portal or
        // ScreenSaver backends, which are Phase 4. Reporting them as
        // unsupported is the honest answer, and the service surfaces it rather
        // than pretending the session covers them.
        Ok(BackendCapabilities {
            system_suspend: true,
            idle: true,
            display_sleep: false,
            automatic_lock: false,
        })
    }

    async fn acquire(&self, request: &LeaseRequest) -> Result<LogindLease, BackendError> {
        let what = request.what_argument();
        let descriptor = self
            .proxy()
            .await?
            .inhibit(&what, &request.who, &request.why, "block")
            .await
            .map_err(|error| match error {
                zbus::Error::MethodError(ref name, _, _)
                    if name.as_str().contains("AccessDenied") =>
                {
                    BackendError::Denied(error.to_string())
                }
                error => BackendError::Protocol(error.to_string()),
            })?;
        Ok(LogindLease {
            _descriptor: descriptor,
            what,
        })
    }

    async fn verify(&self, lease: &LogindLease) -> Result<LeaseHealth, BackendError> {
        // Holding the descriptor is not proof on its own: logind can restart
        // and forget the lock while the fd stays open on this side. Asking it
        // what it currently lists is the only answer worth reporting.
        let inhibitors = self
            .proxy()
            .await?
            .list_inhibitors()
            .await
            .map_err(|error| BackendError::Protocol(error.to_string()))?;

        let mine = std::process::id();
        let held = inhibitors
            .iter()
            .any(|(what, _who, _why, mode, _uid, pid)| {
                *pid == mine && mode == "block" && what == &lease.what
            });
        Ok(if held {
            LeaseHealth::Held
        } else {
            LeaseHealth::Lost
        })
    }

    async fn release(&self, lease: LogindLease) -> Result<(), BackendError> {
        // Closing the descriptor is the release. There is no call to make.
        drop(lease);
        Ok(())
    }
}
