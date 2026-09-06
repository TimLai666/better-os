//! What the unprivileged client knows about the machine it is planning for.
//!
//! The planner picks an artifact by release and architecture. Until this
//! module existed the only backend a client could build was [`MockPlatform`],
//! which reports Ubuntu 24.04 amd64 whatever the host is, so every plan on a
//! 22.04 or arm64 machine named the wrong package. It looked correct on the
//! project's own Zorin 18 host for one reason only: the daemon independently
//! resolves that host to 24.04, so the wrong answer and the right one happened
//! to agree.
//!
//! The release rules are `better_core::host`'s, shared with `manager-daemon`
//! and matched by `install.sh`, so the client cannot drift from the service
//! that will refuse its plan.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use better_core::host::{describe_os_release, resolve_distribution_id, resolve_ubuntu_release};

use crate::{MockPlatform, PlatformError, SystemCapabilities, SystemProfile};

/// Where the os-release text comes from.
enum OsReleaseSource {
    Path(PathBuf),
    /// Fixture content, so a test can exercise a release this machine is not
    /// running without touching the filesystem.
    Content(String),
}

/// Where the architecture comes from.
enum ArchitectureSource {
    /// `dpkg --print-architecture`, which is what `install.sh` asks and what
    /// names the published package. Running it reads dpkg state and changes
    /// nothing, so it stays on the unprivileged side of the boundary.
    Dpkg,
    Fixed(String),
}

/// Reads the real host.
pub struct HostPlatform {
    os_release: OsReleaseSource,
    architecture: ArchitectureSource,
}

impl HostPlatform {
    /// The machine this process is running on.
    pub fn new() -> Self {
        Self {
            os_release: OsReleaseSource::Path(PathBuf::from("/etc/os-release")),
            architecture: ArchitectureSource::Dpkg,
        }
    }

    /// A probe answering from supplied values instead of the host.
    ///
    /// This is a test seam, not a privilege boundary: both inputs are things a
    /// caller can already read, and choosing different ones only changes which
    /// published artifact a plan names — which the daemon then checks against
    /// the real machine and refuses on a mismatch.
    pub fn from_fixture(os_release: impl Into<String>, architecture: impl Into<String>) -> Self {
        Self {
            os_release: OsReleaseSource::Content(os_release.into()),
            architecture: ArchitectureSource::Fixed(architecture.into()),
        }
    }

    /// The file this probe reads, when it reads one. `None` means it was built
    /// from a fixture.
    pub fn os_release_path(&self) -> Option<&Path> {
        match &self.os_release {
            OsReleaseSource::Path(path) => Some(path.as_path()),
            OsReleaseSource::Content(_) => None,
        }
    }

    fn os_release_text(&self) -> Result<String, PlatformError> {
        match &self.os_release {
            OsReleaseSource::Path(path) => fs::read_to_string(path).map_err(|error| {
                PlatformError::UnsupportedHost(format!("cannot read {}: {error}", path.display()))
            }),
            OsReleaseSource::Content(content) => Ok(content.clone()),
        }
    }

    fn architecture(&self) -> Result<String, PlatformError> {
        match &self.architecture {
            ArchitectureSource::Fixed(architecture) => Ok(architecture.clone()),
            ArchitectureSource::Dpkg => {
                let output = Command::new("dpkg")
                    .arg("--print-architecture")
                    .output()
                    .map_err(|error| {
                        PlatformError::UnsupportedHost(format!(
                            "dpkg --print-architecture could not be run: {error}"
                        ))
                    })?;
                if !output.status.success() {
                    return Err(PlatformError::UnsupportedHost(
                        "dpkg --print-architecture failed".to_string(),
                    ));
                }
                let architecture = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if architecture.is_empty() {
                    return Err(PlatformError::UnsupportedHost(
                        "dpkg reported no architecture".to_string(),
                    ));
                }
                Ok(architecture)
            }
        }
    }

    /// Resolves a profile from os-release text and an architecture.
    ///
    /// An unsupported host is an error and never a default. Falling back to
    /// 24.04 amd64 is exactly the defect this replaces: it produces a plan
    /// that names packages built for another release, which the daemon then
    /// refuses with a message about a mismatch rather than about the host.
    pub fn profile_from(content: &str, architecture: &str) -> Result<SystemProfile, PlatformError> {
        let release = resolve_ubuntu_release(content)
            .ok_or_else(|| PlatformError::UnsupportedHost(describe_os_release(content)))?;
        Ok(SystemProfile {
            distribution: resolve_distribution_id(content),
            release,
            architecture: architecture.to_string(),
            // Free space is a separate question from what the host is, and
            // nothing here measures it. `None` means unavailable, not zero.
            free_disk_bytes: None,
        })
    }
}

impl Default for HostPlatform {
    fn default() -> Self {
        Self::new()
    }
}

impl SystemCapabilities for HostPlatform {
    fn profile(&self) -> Result<SystemProfile, PlatformError> {
        Self::profile_from(&self.os_release_text()?, &self.architecture()?)
    }
}

/// Which platform a client plans from.
///
/// Real mode reads the machine. Demo mode keeps the fixed mock profile, which
/// is the whole point of a demo: it must produce the same screens on any host,
/// and it never changes one.
pub enum ClientPlatform {
    Host(HostPlatform),
    Mock(MockPlatform),
}

impl ClientPlatform {
    /// The machine this process is running on.
    pub fn host() -> Self {
        Self::Host(HostPlatform::new())
    }

    /// The fixed profile the demo and mock execution modes exist to show.
    pub fn demo() -> Self {
        Self::Mock(MockPlatform::default())
    }
}

impl SystemCapabilities for ClientPlatform {
    fn profile(&self) -> Result<SystemProfile, PlatformError> {
        match self {
            Self::Host(host) => host.profile(),
            Self::Mock(mock) => mock.profile(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ZORIN_18: &str = "NAME=\"Zorin OS\"\nID=zorin\nID_LIKE=\"ubuntu debian\"\n\
         PRETTY_NAME=\"Zorin OS 18\"\nVERSION_ID=\"18\"\nUBUNTU_CODENAME=noble\n";
    const ZORIN_17: &str =
        "NAME=\"Zorin OS\"\nID=zorin\nVERSION_ID=\"17\"\nUBUNTU_CODENAME=jammy\n";
    const UBUNTU_2204: &str = "NAME=\"Ubuntu\"\nID=ubuntu\nVERSION_ID=\"22.04\"\n";

    #[test]
    fn a_zorin_18_host_plans_for_its_noble_base_and_says_it_is_zorin() {
        let profile = HostPlatform::from_fixture(ZORIN_18, "amd64")
            .profile()
            .expect("a noble-based host is supported");
        assert_eq!(profile.release, "24.04");
        assert_eq!(profile.distribution, "zorin");
        assert_eq!(profile.architecture, "amd64");
        assert_eq!(profile.free_disk_bytes, None);
    }

    /// The two combinations the mock could never have produced. Before this
    /// probe existed both of these planned as 24.04 amd64.
    #[test]
    fn a_jammy_host_and_an_arm64_host_are_reported_as_themselves() {
        let jammy = HostPlatform::from_fixture(ZORIN_17, "arm64")
            .profile()
            .expect("a jammy-based host is supported");
        assert_eq!(jammy.release, "22.04");
        assert_eq!(jammy.architecture, "arm64");

        let ubuntu = HostPlatform::from_fixture(UBUNTU_2204, "amd64")
            .profile()
            .expect("plain ubuntu 22.04 is supported");
        assert_eq!(ubuntu.release, "22.04");
        assert_eq!(ubuntu.distribution, "ubuntu");
    }

    #[test]
    fn an_unsupported_host_is_an_error_naming_the_host_and_never_a_guess() {
        let error = HostPlatform::from_fixture(
            "NAME=\"Fedora Linux\"\nID=fedora\nVERSION_ID=41\n",
            "amd64",
        )
        .profile()
        .expect_err("a host outside the matrix must not resolve");
        let message = error.to_string();
        assert!(
            message.starts_with("platform.error.unsupported_host:"),
            "{message}"
        );
        assert!(message.contains("ID=fedora"), "{message}");
        assert!(!message.contains("24.04"), "{message}");
    }

    #[test]
    fn an_unreadable_os_release_is_reported_rather_than_defaulted() {
        let probe = HostPlatform {
            os_release: OsReleaseSource::Path(PathBuf::from("/nonexistent/os-release")),
            architecture: ArchitectureSource::Fixed("amd64".to_string()),
        };
        assert!(matches!(
            probe.profile(),
            Err(PlatformError::UnsupportedHost(_))
        ));
    }

    #[test]
    fn the_real_probe_reads_the_system_os_release_file() {
        assert_eq!(
            HostPlatform::new().os_release_path(),
            Some(Path::new("/etc/os-release"))
        );
    }

    #[test]
    fn the_demo_platform_keeps_the_fixed_profile_and_the_host_one_does_not() {
        assert!(matches!(ClientPlatform::demo(), ClientPlatform::Mock(_)));
        assert!(matches!(ClientPlatform::host(), ClientPlatform::Host(_)));

        let demo = ClientPlatform::demo()
            .profile()
            .expect("the mock platform always reports a profile");
        assert_eq!(demo, SystemProfile::default());
    }
}
