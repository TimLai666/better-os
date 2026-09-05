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
        let release = parse_ubuntu_release(&os_release).ok_or_else(|| {
            DaemonError::HostUnreadable(
                "neither UBUNTU_CODENAME nor VERSION_ID names a supported Ubuntu base".to_string(),
            )
        })?;

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

/// Resolves the Ubuntu base release from an os-release file.
///
/// Derivatives report their own `VERSION_ID` — Zorin OS 18 says `18`, which
/// names nothing in the release matrix — so the base comes from
/// `UBUNTU_CODENAME` first, exactly the way `install.sh` decides which package
/// to download. `VERSION_ID` stays as the fallback for hosts that carry no
/// codename field. The two must keep agreeing: a daemon that reads the badge
/// while the installer reads the base refuses every plan on a derivative,
/// which is the field failure this function replaced.
fn parse_ubuntu_release(content: &str) -> Option<String> {
    if let Some(codename) = parse_field(content, "UBUNTU_CODENAME=") {
        return match codename.as_str() {
            "jammy" => Some("22.04".to_string()),
            "noble" => Some("24.04".to_string()),
            _ => None,
        };
    }
    parse_field(content, "VERSION_ID=")
}

/// Pulls one `KEY=` value out of an os-release file, unquoting it.
fn parse_field(content: &str, prefix: &str) -> Option<String> {
    content.lines().find_map(|line| {
        let value = line.strip_prefix(prefix)?;
        let value = value.trim_matches('"').trim();
        if value.is_empty() {
            None
        } else {
            Some(value.to_string())
        }
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
        assert_eq!(parse_ubuntu_release(content).as_deref(), Some("24.04"));
    }

    #[test]
    fn an_os_release_without_a_version_reports_nothing_rather_than_a_guess() {
        assert_eq!(parse_ubuntu_release("NAME=\"Zorin OS\"\n"), None);
    }

    #[test]
    fn zorin_18_resolves_to_its_noble_base_not_its_own_version_id() {
        // The exact shape Zorin OS 18.1 ships: VERSION_ID names the badge,
        // UBUNTU_CODENAME names the base the packages are built for. Reading
        // the badge made the daemon refuse every plan on the project's primary
        // target ("plan targets release 24.04 but this host is 18").
        let content = "NAME=\"Zorin OS\"\nID=zorin\nID_LIKE=\"ubuntu debian\"\n\
             VERSION_ID=\"18\"\nUBUNTU_CODENAME=noble\n";
        assert_eq!(parse_ubuntu_release(content).as_deref(), Some("24.04"));
    }

    #[test]
    fn zorin_17_resolves_to_its_jammy_base() {
        let content = "ID=zorin\nVERSION_ID=\"17\"\nUBUNTU_CODENAME=jammy\n";
        assert_eq!(parse_ubuntu_release(content).as_deref(), Some("22.04"));
    }

    #[test]
    fn an_unknown_codename_is_refused_rather_than_falling_back_to_the_badge() {
        // A future base the matrix does not support must not degrade into the
        // derivative's own version number, which would produce a nonsense
        // comparison instead of an honest refusal.
        let content = "ID=zorin\nVERSION_ID=\"19\"\nUBUNTU_CODENAME=plucky\n";
        assert_eq!(parse_ubuntu_release(content), None);
    }

    #[test]
    fn plain_ubuntu_without_a_codename_still_uses_version_id() {
        let content = "ID=ubuntu\nVERSION_ID=\"22.04\"\n";
        assert_eq!(parse_ubuntu_release(content).as_deref(), Some("22.04"));
    }
}
