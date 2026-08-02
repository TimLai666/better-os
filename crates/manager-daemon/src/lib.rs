//! The privileged half of Better Manager.
//!
//! This process is the only thing in the project that changes the host. It
//! accepts a typed transaction plan over the D-Bus system bus, checks the
//! caller against polkit, revalidates the plan from scratch against the host it
//! is actually running on, applies it through local APT, health-checks what it
//! applied, and rolls back what it can when a step fails.
//!
//! It does not read manifests or share the planner's types beyond the wire
//! contract. Its trust anchors are a hard component-name whitelist, the
//! checksum an administrator authorized, the `.deb` control fields, and its own
//! reading of the host — never anything the client asserted.
//!
//! Everything that touches the outside world sits behind a trait so the
//! transaction logic can be tested without privileges: [`apt::AptDriver`],
//! [`host::HostProbe`], [`health::HealthProbe`], and [`authorize::Authorizer`].

pub mod apt;
pub mod authorize;
pub mod dmi;
pub mod executor;
pub mod health;
pub mod host;
pub mod monitor_service;
pub mod revalidate;
pub mod service;
pub mod store;

/// Where staged artifacts live. Root-owned, and the only directory the daemon
/// will install from.
pub const ARCHIVE_DIR: &str = "/var/cache/better-os/archives";

/// Where transaction journals and rollback records live.
pub const STATE_DIR: &str = "/var/lib/better-os";

/// The only component names this daemon will ever act on, regardless of what
/// any catalog says.
pub fn is_first_party_component(name: &str) -> bool {
    better_core::ComponentId::new(name).is_ok() && name.starts_with("better-") && name.len() > 7
}

/// Everything the daemon can refuse or fail with.
///
/// Messages are stable machine keys. The GUI and CLI own the wording a user
/// reads, exactly as they do for `manager-core` and `manager-platform` errors.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum DaemonError {
    #[error("daemon.error.unauthorized")]
    Unauthorized,
    #[error("daemon.error.busy")]
    Busy,
    #[error("daemon.error.protocol:{0}")]
    Protocol(String),
    #[error("daemon.error.plan_rejected:{0}")]
    PlanRejected(String),
    #[error("daemon.error.unknown_transaction:{0}")]
    UnknownTransaction(String),
    #[error("daemon.error.checksum_mismatch:{component}")]
    ChecksumMismatch { component: String },
    #[error("daemon.error.artifact_missing:{component}")]
    ArtifactMissing { component: String },
    #[error("daemon.error.state_drift:{component}")]
    StateDrift { component: String },
    #[error("daemon.error.apt_busy")]
    AptBusy,
    #[error("daemon.error.apt_failed:{component}")]
    AptFailed { component: String },
    #[error("daemon.error.health_failed:{component}")]
    HealthFailed { component: String },
    #[error("daemon.error.host_unreadable:{0}")]
    HostUnreadable(String),
    #[error("daemon.error.storage:{0}")]
    Storage(String),
    #[error("daemon.error.cancelled")]
    Cancelled,
}

impl From<manager_ipc::IpcError> for DaemonError {
    fn from(error: manager_ipc::IpcError) -> Self {
        DaemonError::Protocol(error.to_string())
    }
}

impl From<monitor_ipc::IpcError> for DaemonError {
    fn from(error: monitor_ipc::IpcError) -> Self {
        DaemonError::Protocol(error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_first_party_component_names_are_accepted() {
        assert!(is_first_party_component("better-monitor"));
        assert!(is_first_party_component("better-files-example"));

        // Not ours, however the plan describes it.
        assert!(!is_first_party_component("bash"));
        assert!(!is_first_party_component("better-"));
        assert!(!is_first_party_component("libbetter-monitor"));
        // Shell metacharacters cannot survive ComponentId in the first place.
        assert!(!is_first_party_component("better-monitor; rm -rf /"));
        assert!(!is_first_party_component("better-monitor/../bash"));
    }
}
