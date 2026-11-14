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

/// what, who, why, mode, uid, pid.
type InhibitorEntry = (String, String, String, String, u32, u32);

#[zbus::proxy(
    interface = "org.freedesktop.login1.Manager",
    default_service = "org.freedesktop.login1",
    default_path = "/org/freedesktop/login1"
)]
trait LogindManager {
    fn inhibit(&self, what: &str, who: &str, why: &str, mode: &str) -> zbus::Result<OwnedFd>;

    #[zbus(name = "ListInhibitors")]
    fn list_inhibitors(&self) -> zbus::Result<Vec<InhibitorEntry>>;

    #[zbus(property)]
    fn lid_closed(&self) -> zbus::Result<bool>;

    /// True with a docking station or an external display connected.
    #[zbus(property)]
    fn docked(&self) -> zbus::Result<bool>;
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
        //
        // `handle-lid-switch` is a lock any session user may take too, and it
        // is what stops logind suspending on the lid while a `sleep` lock does
        // not (`LidSwitchIgnoreInhibited=yes` by default). A machine with no
        // lid accepts it and it does nothing there.
        Ok(BackendCapabilities {
            system_suspend: true,
            idle: true,
            display_sleep: false,
            automatic_lock: false,
            lid_switch: true,
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
                    if name.as_str().contains("AccessDenied")
                        || name.as_str().contains("InteractiveAuthorizationRequired") =>
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

    async fn lid_closed(&self) -> Option<bool> {
        // Asked each time rather than read from a property cache, so the answer
        // never depends on a change signal having arrived. A machine with no
        // lid answers `false`, which never starts the closed-lid clock.
        let proxy = LogindManagerProxy::builder(&self.connection)
            .cache_properties(zbus::proxy::CacheProperties::No)
            .build()
            .await
            .ok()?;
        proxy.lid_closed().await.ok()
    }

    async fn docked(&self) -> Option<bool> {
        let proxy = LogindManagerProxy::builder(&self.connection)
            .cache_properties(zbus::proxy::CacheProperties::No)
            .build()
            .await
            .ok()?;
        proxy.docked().await.ok()
    }
}
