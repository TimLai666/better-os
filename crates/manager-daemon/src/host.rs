//! What the daemon knows about the machine it is running on.
//!
//! These values are read from the host, never taken from the plan. A plan
//! states the release and architecture it was built for; the daemon compares
//! that against what it finds here and refuses a mismatch.

use std::fs;
use std::process::Command;

use crate::DaemonError;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HostFacts {
    pub release: String,
    pub architecture: String,
}

pub trait HostProbe: Send + Sync {
    fn facts(&self) -> Result<HostFacts, DaemonError>;
}

/// Reads the real host.
pub struct SystemHostProbe;

impl HostProbe for SystemHostProbe {
    fn facts(&self) -> Result<HostFacts, DaemonError> {
        let os_release = fs::read_to_string("/etc/os-release")
            .map_err(|error| DaemonError::HostUnreadable(error.to_string()))?;
        let release = parse_version_id(&os_release)
            .ok_or_else(|| DaemonError::HostUnreadable("VERSION_ID is missing".to_string()))?;

        let output = Command::new("dpkg")
            .arg("--print-architecture")
            .output()
            .map_err(|error| DaemonError::HostUnreadable(error.to_string()))?;
        if !output.status.success() {
            return Err(DaemonError::HostUnreadable(
                "dpkg --print-architecture failed".to_string(),
            ));
        }
        let architecture = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if architecture.is_empty() {
            return Err(DaemonError::HostUnreadable(
                "dpkg reported no architecture".to_string(),
            ));
        }
        Ok(HostFacts {
            release,
            architecture,
        })
    }
}

/// Pulls `VERSION_ID` out of an os-release file, unquoting it.
fn parse_version_id(content: &str) -> Option<String> {
    content.lines().find_map(|line| {
        let value = line.strip_prefix("VERSION_ID=")?;
        Some(value.trim_matches('"').trim().to_string())
    })
}

/// A fixed host, for tests.
#[derive(Clone, Debug)]
pub struct FixedHostProbe(pub HostFacts);

impl FixedHostProbe {
    pub fn ubuntu_2404() -> Self {
        Self(HostFacts {
            release: "24.04".to_string(),
            architecture: "amd64".to_string(),
        })
    }
}

impl HostProbe for FixedHostProbe {
    fn facts(&self) -> Result<HostFacts, DaemonError> {
        Ok(self.0.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_release_is_read_from_os_release_and_unquoted() {
        let content = "NAME=\"Ubuntu\"\nVERSION_ID=\"24.04\"\nID=ubuntu\n";
        assert_eq!(parse_version_id(content).as_deref(), Some("24.04"));
    }

    #[test]
    fn an_os_release_without_a_version_reports_nothing_rather_than_a_guess() {
        assert_eq!(parse_version_id("NAME=\"Zorin OS\"\n"), None);
    }
}
