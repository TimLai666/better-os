//! Package, download, and system-capability interfaces for Better OS.
//!
//! This crate owns the boundary between planning and the host. It declares
//! what a platform backend must be able to answer and provides mock
//! implementations that never read, write, or mutate host state. The
//! privileged executor is an interface only: its security design is not
//! approved, so the shipped implementation refuses every request instead of
//! pretending to perform one.

pub mod download;
pub mod dpkg;
#[cfg(feature = "dbus-client")]
pub mod privileged;

use std::path::PathBuf;

use better_core::ComponentId;
use manager_ipc::{ExecutionStage, TransactionOutcome, WirePlan};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// What the host looks like to the planner.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SystemProfile {
    pub distribution: String,
    pub release: String,
    pub architecture: String,
    /// Free space on the install target, when the platform can report it.
    /// `None` means unavailable, not zero.
    pub free_disk_bytes: Option<u64>,
}

impl Default for SystemProfile {
    fn default() -> Self {
        Self {
            distribution: "ubuntu".to_string(),
            release: "24.04".to_string(),
            architecture: "amd64".to_string(),
            free_disk_bytes: None,
        }
    }
}

/// Reports what the host is and what it has room for.
pub trait SystemCapabilities {
    fn profile(&self) -> Result<SystemProfile, PlatformError>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DownloadRequest {
    pub component: ComponentId,
    pub url: String,
    pub sha256: String,
    pub expected_bytes: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DownloadReceipt {
    pub component: ComponentId,
    pub verified_sha256: String,
    pub bytes: u64,
    /// Where the verified bytes ended up. The file is named by its checksum, so
    /// this path cannot refer to anything other than what was verified.
    pub artifact_path: PathBuf,
}

/// Fetches a component artifact and proves what it fetched.
pub trait DownloadBackend {
    fn download(&self, request: &DownloadRequest) -> Result<DownloadReceipt, PlatformError>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PackageAction {
    Install,
    Remove,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackageRequest {
    pub component: ComponentId,
    pub action: PackageAction,
    pub artifact_path: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackageReceipt {
    pub component: ComponentId,
    pub action: PackageAction,
    pub applied_version: Option<String>,
}

/// Applies a package change to the host. Every implementation of this trait
/// mutates the system and therefore belongs behind the privileged boundary.
pub trait PackageBackend {
    fn apply(&self, request: &PackageRequest) -> Result<PackageReceipt, PlatformError>;
}

/// The privileged boundary itself, one package at a time.
///
/// Superseded by [`PrivilegedTransactionExecutor`], which carries a whole
/// transaction as ADR 0007 and `docs/architecture.md` require. It remains so
/// that the shipped refusal keeps its shape and its error key.
pub trait PrivilegedExecutor {
    fn execute(&self, request: &PackageRequest) -> Result<PackageReceipt, PlatformError>;
}

/// The privileged boundary as a transaction.
///
/// An implementation of this trait can change the host, so constructing one
/// must involve establishing an authorized connection to the privileged
/// service. Nothing that can be built out of thin air may implement it with
/// anything other than a refusal.
pub trait PrivilegedTransactionExecutor: Send + Sync {
    /// Hands a verified artifact to the privileged service, which re-checks the
    /// bytes itself before keeping them.
    fn stage_artifact(
        &self,
        transaction_id: &str,
        filename: &str,
        sha256: &str,
        artifact_path: &std::path::Path,
    ) -> Result<(), PlatformError>;

    /// Applies a whole transaction and returns what happened to every step.
    fn execute_plan(
        &self,
        plan: &WirePlan,
        progress: &mut dyn FnMut(u32, ExecutionStage),
    ) -> Result<TransactionOutcome, PlatformError>;
}

#[derive(Debug, Error)]
pub enum PlatformError {
    #[error("platform.error.capability_unavailable:{0}")]
    CapabilityUnavailable(&'static str),
    #[error("platform.error.download_failed:{component}")]
    DownloadFailed { component: ComponentId },
    #[error("platform.error.checksum_mismatch:{component}")]
    ChecksumMismatch { component: ComponentId },
    #[error("platform.error.privileged_execution_not_approved")]
    PrivilegedExecutionNotApproved,
    /// The privileged service is not installed, not running, or not reachable.
    #[error("platform.error.daemon_unavailable:{0}")]
    DaemonUnavailable(String),
    /// polkit refused, or the person cancelled the authentication prompt.
    #[error("platform.error.polkit_denied")]
    PolkitDenied,
    /// The privileged service refused or failed. The detail is its own stable
    /// machine key.
    #[error("platform.error.daemon_refused:{0}")]
    DaemonRefused(String),
}

/// A platform backend that answers from supplied values and never touches the
/// host. It is the only backend the current manager is allowed to use.
#[derive(Clone, Debug, Default)]
pub struct MockPlatform {
    profile: SystemProfile,
}

impl MockPlatform {
    pub fn new(profile: SystemProfile) -> Self {
        Self { profile }
    }
}

impl SystemCapabilities for MockPlatform {
    fn profile(&self) -> Result<SystemProfile, PlatformError> {
        Ok(self.profile.clone())
    }
}

impl DownloadBackend for MockPlatform {
    /// Reports the artifact the manifest already declared. Nothing is fetched
    /// and nothing is written, so a mock download can never disagree with the
    /// declared checksum.
    fn download(&self, request: &DownloadRequest) -> Result<DownloadReceipt, PlatformError> {
        Ok(DownloadReceipt {
            component: request.component.clone(),
            verified_sha256: request.sha256.clone(),
            bytes: request.expected_bytes.unwrap_or_default(),
            // A mock fetches nothing, so it has nothing on disk to point at.
            artifact_path: PathBuf::new(),
        })
    }
}

impl PackageBackend for MockPlatform {
    /// Refuses instead of simulating a host mutation. Mock lifecycle progress
    /// belongs to `manager-core`; a package backend that returns success here
    /// would claim the host changed.
    fn apply(&self, _request: &PackageRequest) -> Result<PackageReceipt, PlatformError> {
        Err(PlatformError::PrivilegedExecutionNotApproved)
    }
}

/// The shipped privileged executor. It refuses every request until the
/// privileged daemon protocol and its security design are approved.
#[derive(Clone, Copy, Debug, Default)]
pub struct UnapprovedPrivilegedExecutor;

impl PrivilegedExecutor for UnapprovedPrivilegedExecutor {
    fn execute(&self, _request: &PackageRequest) -> Result<PackageReceipt, PlatformError> {
        Err(PlatformError::PrivilegedExecutionNotApproved)
    }
}

impl PrivilegedTransactionExecutor for UnapprovedPrivilegedExecutor {
    fn stage_artifact(
        &self,
        _transaction_id: &str,
        _filename: &str,
        _sha256: &str,
        _artifact_path: &std::path::Path,
    ) -> Result<(), PlatformError> {
        Err(PlatformError::PrivilegedExecutionNotApproved)
    }

    fn execute_plan(
        &self,
        _plan: &WirePlan,
        _progress: &mut dyn FnMut(u32, ExecutionStage),
    ) -> Result<TransactionOutcome, PlatformError> {
        Err(PlatformError::PrivilegedExecutionNotApproved)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> PackageRequest {
        PackageRequest {
            component: ComponentId::new("better-monitor").unwrap(),
            action: PackageAction::Install,
            artifact_path: "/nonexistent/better-monitor.deb".to_string(),
        }
    }

    #[test]
    fn the_mock_platform_reports_the_profile_it_was_given() {
        let platform = MockPlatform::new(SystemProfile {
            distribution: "zorin".to_string(),
            release: "18".to_string(),
            architecture: "arm64".to_string(),
            free_disk_bytes: Some(4096),
        });

        let profile = platform.profile().unwrap();
        assert_eq!(profile.distribution, "zorin");
        assert_eq!(profile.free_disk_bytes, Some(4096));
    }

    #[test]
    fn an_unavailable_free_disk_value_stays_unavailable() {
        assert_eq!(SystemProfile::default().free_disk_bytes, None);
    }

    #[test]
    fn the_mock_download_confirms_the_declared_checksum_without_fetching() {
        let platform = MockPlatform::default();
        let receipt = platform
            .download(&DownloadRequest {
                component: ComponentId::new("better-monitor").unwrap(),
                url: "https://example.com/better-monitor.deb".to_string(),
                sha256: "b".repeat(64),
                expected_bytes: Some(512),
            })
            .unwrap();

        assert_eq!(receipt.verified_sha256, "b".repeat(64));
        assert_eq!(receipt.bytes, 512);
    }

    /// The project's original invariant was that nothing shipped could apply a
    /// package change at all. Real installation now exists, so the invariant
    /// has to say something narrower and still true: applying requires an
    /// established, authorized connection to the privileged service. Every
    /// executor that can be built without one refuses.
    ///
    /// This is deliberately a runtime property rather than a build-time
    /// feature: the point is that the default wiring cannot change the host,
    /// not that a particular build cannot.
    #[test]
    fn no_backend_applies_a_change_without_an_authorized_privileged_connection() {
        assert!(matches!(
            MockPlatform::default().apply(&request()),
            Err(PlatformError::PrivilegedExecutionNotApproved)
        ));
        assert!(matches!(
            UnapprovedPrivilegedExecutor.execute(&request()),
            Err(PlatformError::PrivilegedExecutionNotApproved)
        ));

        // The transaction-shaped boundary refuses on the same terms, so a
        // caller cannot reach the host by using the newer trait instead.
        let plan = manager_ipc::WirePlan {
            protocol_version: manager_ipc::PROTOCOL_VERSION,
            transaction_id: "3f2504e0-4f89-41d3-9a0c-0305e82c3301".to_string(),
            target_release: "24.04".to_string(),
            target_architecture: "amd64".to_string(),
            steps: Vec::new(),
        };
        assert!(matches!(
            UnapprovedPrivilegedExecutor.execute_plan(&plan, &mut |_, _| {}),
            Err(PlatformError::PrivilegedExecutionNotApproved)
        ));
        assert!(matches!(
            UnapprovedPrivilegedExecutor.stage_artifact(
                "3f2504e0-4f89-41d3-9a0c-0305e82c3301",
                "better-monitor_0.1.0_ubuntu-24.04_amd64.deb",
                &"a".repeat(64),
                std::path::Path::new("/nonexistent.deb"),
            ),
            Err(PlatformError::PrivilegedExecutionNotApproved)
        ));
    }
}
