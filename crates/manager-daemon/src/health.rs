//! Checking that what was installed is actually there.
//!
//! The paths checked come from dpkg's own file database for the package the
//! daemon just verified and installed, never from anything the plan supplied.
//! Manifest-declared paths are untrusted data, and the daemon still neither
//! executes nor trusts a string a manifest chose.
//!
//! Deriving one path from the package name — `/usr/bin/<component>` — was the
//! earlier rule and it was wrong about real packages. `better-awake` ships
//! `better-awake-service`, `awake-tray`, and `awake-gui` and no binary of its
//! own name, so every install of it passed apt, failed this check, and was
//! rolled back. `better-storage` has the same shape. dpkg knows what each
//! package put on disk; this asks it.

use better_core::package_health::{HealthVerdict, check_installed};
use manager_ipc::{HealthResult, WireAction};

use crate::apt::AptDriver;

pub use better_core::package_health::{FakeHealthProbe, HealthProbe, PathState, SystemHealthProbe};

/// Confirms a step did what it said.
///
/// For an install, update, or restore that means dpkg reports the package
/// installed and every path dpkg lists for it is on disk. Three kinds of listed
/// path are skipped, each because its absence is not evidence of a broken
/// package:
///
/// - directories, which are shared between packages and say nothing about this
///   one;
/// - conffiles, because a machine's owner is allowed to delete one and dpkg
///   does not consider the package broken when they have;
/// - paths this machine's dpkg was configured never to unpack, which it lists
///   all the same — see [`better_core::package_health::PathFilter`].
///
/// A package that lists no files of its own is healthy on the same rule — every
/// file it listed is there — which is the honest answer for a metapackage.
///
/// For a removal it means dpkg no longer reports the package. A check the
/// daemon could not run reports `Undetermined` rather than passing.
pub fn check(
    component: &str,
    action: WireAction,
    apt: &dyn AptDriver,
    probe: &dyn HealthProbe,
) -> HealthResult {
    let installed = apt
        .installed_version(component)
        .map_err(|error| error.to_string());

    match action {
        WireAction::Remove => match installed {
            Err(error) => HealthResult::Undetermined(format!("dpkg query failed: {error}")),
            Ok(None) => HealthResult::Healthy,
            Ok(Some(version)) => {
                HealthResult::Failed(format!("{component} is still installed at {version}"))
            }
        },
        WireAction::Install | WireAction::Update | WireAction::Restore => {
            match check_installed(
                component,
                installed,
                || {
                    apt.installed_files(component)
                        .map_err(|error| error.to_string())
                },
                probe,
            ) {
                HealthVerdict::Healthy => HealthResult::Healthy,
                HealthVerdict::Failed(reason) => HealthResult::Failed(reason),
                HealthVerdict::Undetermined(reason) => HealthResult::Undetermined(reason),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apt::{DebFields, FakeAptDriver};
    use std::path::PathBuf;

    /// The real shape of `better-awake`: three binaries, none of them named
    /// after the package, plus data files. Under the old rule every install of
    /// this package failed and was rolled back.
    const AWAKE_FILES: [&str; 6] = [
        "/.",
        "/usr/bin",
        "/usr/bin/better-awake-service",
        "/usr/bin/awake-tray",
        "/usr/bin/awake-gui",
        "/usr/share/applications/better-awake.desktop",
    ];

    fn fields() -> DebFields {
        DebFields {
            package: "better-monitor".to_string(),
            version: "0.1.0".to_string(),
            architecture: "amd64".to_string(),
        }
    }

    fn awake_apt() -> FakeAptDriver {
        FakeAptDriver::new()
            .with_installed("better-awake", "0.2.5")
            .with_files("better-awake", &AWAKE_FILES, &[])
    }

    /// A filesystem that has everything dpkg listed except `missing`. The two
    /// directories dpkg lists are reported as directories, the way a real one
    /// would be.
    fn awake_probe(missing: &[&str]) -> FakeHealthProbe {
        let directories = ["/.", "/usr/bin"];
        FakeHealthProbe::with_files(
            AWAKE_FILES
                .iter()
                .filter(|path| !directories.contains(path) && !missing.contains(path))
                .map(PathBuf::from)
                .collect(),
        )
        .with_directories(directories.iter().map(PathBuf::from).collect())
    }

    #[test]
    fn an_install_is_healthy_when_dpkg_and_the_binary_agree() {
        let apt = FakeAptDriver::new()
            .with_installed("better-monitor", "0.1.0")
            .with_deb("better-monitor.deb", fields());
        let probe = FakeHealthProbe::with_files(vec![PathBuf::from("/usr/bin/better-monitor")]);

        assert_eq!(
            check("better-monitor", WireAction::Install, &apt, &probe),
            HealthResult::Healthy
        );
    }

    #[test]
    fn a_package_whose_binaries_are_not_named_after_it_is_healthy() {
        assert_eq!(
            check(
                "better-awake",
                WireAction::Install,
                &awake_apt(),
                &awake_probe(&[])
            ),
            HealthResult::Healthy
        );
    }

    #[test]
    fn a_missing_file_fails_the_check_and_is_named() {
        let health = check(
            "better-awake",
            WireAction::Install,
            &awake_apt(),
            &awake_probe(&["/usr/bin/awake-tray"]),
        );
        match health {
            HealthResult::Failed(reason) => {
                assert!(reason.contains("/usr/bin/awake-tray"), "{reason}");
                assert!(reason.contains("0.2.5"), "{reason}");
            }
            other => panic!("expected a failure naming the file, got {other:?}"),
        }
    }

    #[test]
    fn an_install_that_left_no_binary_fails_the_check() {
        let apt = FakeAptDriver::new().with_installed("better-monitor", "0.1.0");
        let probe = FakeHealthProbe::empty();

        assert!(matches!(
            check("better-monitor", WireAction::Install, &apt, &probe),
            HealthResult::Failed(_)
        ));
    }

    #[test]
    fn a_conffile_the_machines_owner_deleted_is_not_a_broken_package() {
        let apt = FakeAptDriver::new()
            .with_installed("better-awake", "0.2.5")
            .with_files(
                "better-awake",
                &["/usr/bin/better-awake-service", "/etc/better-os/awake.conf"],
                &["/etc/better-os/awake.conf"],
            );
        let probe =
            FakeHealthProbe::with_files(vec![PathBuf::from("/usr/bin/better-awake-service")]);

        assert_eq!(
            check("better-awake", WireAction::Install, &apt, &probe),
            HealthResult::Healthy
        );
    }

    /// A minimized image — the container the end-to-end check runs in is one —
    /// configures dpkg to drop `/usr/share/doc/*`, and dpkg lists those files
    /// anyway. Requiring them would fail every package on such a machine.
    #[test]
    fn a_file_dpkg_was_configured_never_to_unpack_is_not_a_missing_file() {
        let apt = FakeAptDriver::new()
            .with_installed("better-monitor", "0.2.5")
            .with_files(
                "better-monitor",
                &[
                    "/usr/bin/better-monitor",
                    "/usr/share/doc/better-monitor/THIRD-PARTY-LICENSES.md",
                ],
                &[],
            )
            .with_excluded_files(
                "better-monitor",
                &["/usr/share/doc/better-monitor/THIRD-PARTY-LICENSES.md"],
            );
        let probe = FakeHealthProbe::with_files(vec![PathBuf::from("/usr/bin/better-monitor")]);

        assert_eq!(
            check("better-monitor", WireAction::Install, &apt, &probe),
            HealthResult::Healthy
        );
    }

    #[test]
    fn a_file_list_that_cannot_be_read_is_undetermined_rather_than_a_verdict() {
        let apt = FakeAptDriver::new()
            .with_installed("better-awake", "0.2.5")
            .with_unreadable_files("better-awake");

        assert!(matches!(
            check(
                "better-awake",
                WireAction::Install,
                &apt,
                &FakeHealthProbe::empty()
            ),
            HealthResult::Undetermined(_)
        ));
    }

    #[test]
    fn a_removal_is_healthy_only_once_dpkg_forgets_the_package() {
        let probe = FakeHealthProbe::empty();

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
}
