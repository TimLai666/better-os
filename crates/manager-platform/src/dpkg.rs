//! Reading what dpkg believes is installed.
//!
//! This is a read-only query and therefore allowed outside the privileged
//! boundary: `AGENTS.md` keeps privileged *mutation* out of the GUI and CLI,
//! not the ability to look. Asking the host directly is what lets the manager
//! notice that its own records have drifted instead of trusting them forever.

use std::process::Command;

use crate::PlatformError;

pub trait PackageStateProbe: Send + Sync {
    /// The installed version of a package, or `None` when it is not installed.
    fn installed_version(&self, package: &str) -> Result<Option<String>, PlatformError>;
}

/// Queries the real dpkg database.
pub struct DpkgProbe;

impl PackageStateProbe for DpkgProbe {
    fn installed_version(&self, package: &str) -> Result<Option<String>, PlatformError> {
        // A package name that could carry an option or a shell metacharacter
        // never reaches the command line.
        if !is_safe_package_name(package) {
            return Err(PlatformError::CapabilityUnavailable("dpkg.package_name"));
        }
        let output = Command::new("dpkg-query")
            .args(["-W", "-f=${db:Status-Status} ${Version}", "--", package])
            .env("LC_ALL", "C")
            .output()
            .map_err(|_| PlatformError::CapabilityUnavailable("dpkg.query"))?;

        if !output.status.success() {
            // dpkg-query exits non-zero for a package it has never heard of,
            // which here means "not installed" rather than a failure.
            return Ok(None);
        }
        Ok(parse_status(&String::from_utf8_lossy(&output.stdout)))
    }
}

fn is_safe_package_name(package: &str) -> bool {
    !package.is_empty()
        && package.len() <= 64
        && package.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '.' | '+')
        })
        && !package.starts_with('-')
}

fn parse_status(output: &str) -> Option<String> {
    let mut parts = output.split_whitespace();
    match (parts.next(), parts.next()) {
        (Some("installed"), Some(version)) => Some(version.to_string()),
        _ => None,
    }
}

/// Drops a leading `epoch:` so a dpkg version can be compared with a manifest
/// version, which never carries one.
pub fn strip_epoch(version: &str) -> &str {
    match version.split_once(':') {
        Some((epoch, rest)) if !epoch.is_empty() && epoch.chars().all(|c| c.is_ascii_digit()) => {
            rest
        }
        _ => version,
    }
}

/// Drops a Debian revision suffix, so `0.1.0-1~ubuntu24.04` compares equal to
/// the `0.1.0` a manifest declares.
pub fn strip_revision(version: &str) -> &str {
    match version.rsplit_once('-') {
        Some((upstream, _)) if !upstream.is_empty() => upstream,
        _ => version,
    }
}

/// The upstream version a manifest would recognise.
pub fn upstream_version(version: &str) -> &str {
    strip_revision(strip_epoch(version))
}

/// A probe that answers from a fixed list, for tests.
pub struct FixedPackageStateProbe(pub Vec<(String, String)>);

impl FixedPackageStateProbe {
    pub fn new(entries: &[(&str, &str)]) -> Self {
        Self(
            entries
                .iter()
                .map(|(package, version)| (package.to_string(), version.to_string()))
                .collect(),
        )
    }
}

impl PackageStateProbe for FixedPackageStateProbe {
    fn installed_version(&self, package: &str) -> Result<Option<String>, PlatformError> {
        Ok(self
            .0
            .iter()
            .find(|(known, _)| known == package)
            .map(|(_, version)| version.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_installed_package_reports_its_version() {
        assert_eq!(parse_status("installed 0.1.0\n").as_deref(), Some("0.1.0"));
    }

    #[test]
    fn a_package_that_is_only_configured_or_removed_is_not_installed() {
        assert_eq!(parse_status("config-files 0.1.0\n"), None);
        assert_eq!(parse_status("not-installed \n"), None);
        assert_eq!(parse_status(""), None);
    }

    #[test]
    fn a_package_name_that_could_become_an_option_is_refused() {
        for name in ["--force", "", "better monitor", "better;rm", "better/../x"] {
            assert!(!is_safe_package_name(name), "{name} should be refused");
        }
        assert!(is_safe_package_name("better-monitor"));
        assert!(is_safe_package_name("libc6"));
    }

    #[test]
    fn debian_version_decoration_is_stripped_before_comparing() {
        // The table a manifest version has to survive.
        for (dpkg_version, upstream) in [
            ("0.1.0", "0.1.0"),
            ("0.1.0-1", "0.1.0"),
            ("1:0.1.0", "0.1.0"),
            ("1:0.1.0-1~ubuntu24.04", "0.1.0"),
            ("2:1.2.3-4ubuntu5", "1.2.3"),
            // Not an epoch: a colon with a non-numeric prefix stays put.
            ("a:0.1.0", "a:0.1.0"),
        ] {
            assert_eq!(
                upstream_version(dpkg_version),
                upstream,
                "{dpkg_version} should reduce to {upstream}"
            );
        }
    }
}
