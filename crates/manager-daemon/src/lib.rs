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
//!
//! Besides transactions, it offers one repair: `dpkg --configure -a` for a dpkg
//! run a crash interrupted, decided in ADR 0014 and gated by its own polkit
//! action.

pub mod apt;
pub mod authorize;
pub mod boot_watch;
pub mod dpkg_state;
pub mod drives;
pub mod executor;
pub mod hardware;
pub mod health;
pub mod host;
pub mod revalidate;
pub mod service;
pub mod store;

/// Where staged artifacts live. Root-owned, and the only directory the daemon
/// will install from.
pub const ARCHIVE_DIR: &str = "/var/cache/better-os/archives";

/// Exists exactly while a transaction or a repair runs (ticket 105).
pub const BUSY_MARKER: &str = "/run/better-os/daemon-busy";

/// This service's own package. Upgrading it is the one install that runs
/// outside this service's unit (ADR 0028).
pub const SERVICE_PACKAGE: &str = "better-manager-daemon";

/// Where transaction journals and rollback records live.
pub const STATE_DIR: &str = "/var/lib/better-os";

/// The login screen watch's records and setting (ADR 0033), readable by
/// everyone.
pub const BOOT_WATCH_DIR: &str = manager_ipc::boot_watch::DIR;

/// The charge limit the person chose, for `--restore-hardware` at boot.
pub const HARDWARE_STATE: &str = "/var/lib/better-os/hardware.json";

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
    /// The daemon is leaving (idle, or replaced by an update) and takes no
    /// new call. A client asks again: the next call starts a new daemon.
    #[error("daemon.error.shutting_down")]
    ShuttingDown,
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
    /// dpkg's own record shows a run it did not finish. See ADR 0014.
    #[error("daemon.error.dpkg_interrupted")]
    DpkgInterrupted,
    /// A repair was asked for and dpkg's record shows nothing unfinished.
    #[error("daemon.error.nothing_to_repair")]
    NothingToRepair,
    /// `dpkg --configure -a` exited with an error.
    #[error("daemon.error.repair_failed")]
    RepairFailed,
    /// The person cancelled the polkit prompt (ticket 86).
    #[error("daemon.error.auth_cancelled")]
    AuthCancelled,
    /// A hardware argument outside its closed set; nothing was touched.
    #[error("daemon.error.invalid_argument:{0}")]
    InvalidArgument(String),
    /// The daemon found no such hardware on this machine.
    #[error("daemon.error.hardware_absent")]
    HardwareAbsent,
    /// The write or the read back failed.
    #[error("daemon.error.write_failed:{0}")]
    WriteFailed(String),
    /// UDisks2 knows no drive with that UUID (ticket 127).
    #[error("daemon.error.drive_absent")]
    DriveAbsent,
    /// A drive this choice is not for: `system_drive`, `system_table` (a
    /// line Better OS did not write names it), `not_a_filesystem`.
    #[error("daemon.error.drive_refused:{0}")]
    DriveRefused(String),
    /// It could not be unmounted; the entry stays.
    #[error("daemon.error.drive_busy:{0}")]
    DriveBusy(String),
    /// `/etc/fstab` or the rules file is malformed; nothing was written.
    #[error("daemon.error.mount_table_malformed:{0}")]
    MountTableMalformed(String),
    /// Another change holds the mount table's lock past the wait.
    #[error("daemon.error.mount_table_busy")]
    MountTableBusy,
    /// The entry was written and the mount failed.
    #[error("daemon.error.mount_failed:{0}")]
    MountFailed(String),
}

impl From<manager_ipc::IpcError> for DaemonError {
    fn from(error: manager_ipc::IpcError) -> Self {
        DaemonError::Protocol(error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The window reads these keys from `hardware-core`; a key renamed on one
    /// side only would turn a cancelled prompt into "That didn't change".
    #[test]
    fn the_hardware_error_keys_are_the_ones_the_window_reads() {
        use hardware_core::write::keys;
        assert_eq!(DaemonError::AuthCancelled.to_string(), keys::AUTH_CANCELLED);
        assert_eq!(DaemonError::Unauthorized.to_string(), keys::UNAUTHORIZED);
        assert_eq!(
            DaemonError::HardwareAbsent.to_string(),
            keys::HARDWARE_ABSENT
        );
        assert!(
            DaemonError::InvalidArgument("x".into())
                .to_string()
                .starts_with(keys::INVALID_ARGUMENT)
        );
        assert!(
            DaemonError::WriteFailed("x".into())
                .to_string()
                .starts_with(keys::WRITE_FAILED)
        );
    }

    /// The windows read these through `drive-mounts`.
    #[test]
    fn the_drive_error_keys_are_the_ones_the_windows_read() {
        use drive_mounts::client::{SetOutcome, classify_error};
        let name = "org.freedesktop.DBus.Error.Failed";
        for (error, outcome) in [
            (DaemonError::AuthCancelled, SetOutcome::Cancelled),
            (DaemonError::Unauthorized, SetOutcome::NotAllowed),
            (DaemonError::DriveAbsent, SetOutcome::Absent),
            (
                DaemonError::DriveRefused("system_table".into()),
                SetOutcome::Refused("system_table".into()),
            ),
            (
                DaemonError::DriveBusy("busy".into()),
                SetOutcome::Busy("busy".into()),
            ),
            (
                DaemonError::MountTableMalformed("line 2".into()),
                SetOutcome::Malformed("line 2".into()),
            ),
            (
                DaemonError::MountFailed("timeout".into()),
                SetOutcome::MountFailed("timeout".into()),
            ),
            (DaemonError::MountTableBusy, SetOutcome::TableBusy),
        ] {
            assert_eq!(classify_error(name, &error.to_string()), outcome);
        }
    }

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
