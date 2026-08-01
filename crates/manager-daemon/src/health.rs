//! Checking that what was installed is actually there.
//!
//! The check deliberately derives the executable path from the package name
//! rather than reading it from anything the plan supplied. Manifest-declared
//! paths are untrusted data, and the daemon never executes or trusts a string a
//! manifest chose.

use std::path::{Path, PathBuf};

use manager_ipc::{HealthResult, WireAction};

use crate::apt::AptDriver;

pub trait HealthProbe: Send + Sync {
    /// Whether a regular, executable file exists at this path.
    fn is_executable_file(&self, path: &Path) -> bool;
}

pub struct SystemHealthProbe;

impl HealthProbe for SystemHealthProbe {
    fn is_executable_file(&self, path: &Path) -> bool {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path)
            .map(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
}

/// A probe that answers from a fixed list, for tests.
pub struct FakeHealthProbe(pub Vec<PathBuf>);

impl HealthProbe for FakeHealthProbe {
    fn is_executable_file(&self, path: &Path) -> bool {
        self.0.iter().any(|known| known == path)
    }
}

fn binary_path(component: &str) -> PathBuf {
    PathBuf::from("/usr/bin").join(component)
}

/// Confirms a step did what it said.
///
/// For an install, update, or restore that means dpkg reports the package
/// installed and its binary is present. For a removal it means dpkg no longer
/// reports it. A check the daemon could not run reports `Undetermined` rather
/// than passing.
pub fn check(
    component: &str,
    action: WireAction,
    apt: &dyn AptDriver,
    probe: &dyn HealthProbe,
) -> HealthResult {
    let installed = match apt.installed_version(component) {
        Ok(installed) => installed,
        Err(error) => {
            return HealthResult::Undetermined(format!("dpkg query failed: {error}"));
        }
    };

    match action {
        WireAction::Remove => match installed {
            None => HealthResult::Healthy,
            Some(version) => {
                HealthResult::Failed(format!("{component} is still installed at {version}"))
            }
        },
        WireAction::Install | WireAction::Update | WireAction::Restore => {
            let Some(version) = installed else {
                return HealthResult::Failed(format!("{component} is not installed"));
            };
            let path = binary_path(component);
            if probe.is_executable_file(&path) {
                HealthResult::Healthy
            } else {
                HealthResult::Failed(format!(
                    "{component} reports version {version} but {} is missing",
                    path.display()
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apt::{DebFields, FakeAptDriver};

    fn fields() -> DebFields {
        DebFields {
            package: "better-monitor".to_string(),
            version: "0.1.0".to_string(),
            architecture: "amd64".to_string(),
        }
    }

    #[test]
    fn an_install_is_healthy_when_dpkg_and_the_binary_agree() {
        let apt = FakeAptDriver::new()
            .with_installed("better-monitor", "0.1.0")
            .with_deb("better-monitor.deb", fields());
        let probe = FakeHealthProbe(vec![PathBuf::from("/usr/bin/better-monitor")]);

        assert_eq!(
            check("better-monitor", WireAction::Install, &apt, &probe),
            HealthResult::Healthy
        );
    }

    #[test]
    fn an_install_that_left_no_binary_fails_the_check() {
        let apt = FakeAptDriver::new().with_installed("better-monitor", "0.1.0");
        let probe = FakeHealthProbe(Vec::new());

        assert!(matches!(
            check("better-monitor", WireAction::Install, &apt, &probe),
            HealthResult::Failed(_)
        ));
    }

    #[test]
    fn a_removal_is_healthy_only_once_dpkg_forgets_the_package() {
        let probe = FakeHealthProbe(Vec::new());

        let gone = FakeAptDriver::new();
        assert_eq!(
            check("better-monitor", WireAction::Remove, &gone, &probe),
            HealthResult::Healthy
        );

        let still_there = FakeAptDriver::new().with_installed("better-monitor", "0.1.0");
        assert!(matches!(
            check("better-monitor", WireAction::Remove, &still_there, &probe),
            HealthResult::Failed(_)
        ));
    }

    #[test]
    fn the_checked_path_comes_from_the_package_name_not_from_the_plan() {
        assert_eq!(
            binary_path("better-monitor"),
            PathBuf::from("/usr/bin/better-monitor")
        );
    }
}
