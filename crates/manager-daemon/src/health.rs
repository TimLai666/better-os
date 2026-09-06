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

use std::path::{Path, PathBuf};

use manager_ipc::{HealthResult, WireAction};

use crate::apt::AptDriver;

/// What the filesystem says about one path dpkg listed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PathState {
    /// Nothing is there.
    Missing,
    /// A directory, which several packages may share and which is not evidence
    /// about this one.
    Directory,
    /// A file, a symlink, or anything else that exists at that path.
    Present,
}

pub trait HealthProbe: Send + Sync {
    /// What is at this path, without following symlinks: a link that exists is
    /// present whether or not its target does, because the link is the file the
    /// package installed.
    fn inspect(&self, path: &Path) -> PathState;
}

pub struct SystemHealthProbe;

impl HealthProbe for SystemHealthProbe {
    fn inspect(&self, path: &Path) -> PathState {
        match std::fs::symlink_metadata(path) {
            Err(_) => PathState::Missing,
            Ok(metadata) if metadata.is_dir() => PathState::Directory,
            Ok(_) => PathState::Present,
        }
    }
}

/// A probe that answers from a fixed list, for tests. Paths in the list are
/// present, paths in `directories` are directories, and anything else is
/// missing.
pub struct FakeHealthProbe {
    pub present: Vec<PathBuf>,
    pub directories: Vec<PathBuf>,
}

impl FakeHealthProbe {
    pub fn with_files(paths: Vec<PathBuf>) -> Self {
        Self {
            present: paths,
            directories: Vec::new(),
        }
    }

    pub fn empty() -> Self {
        Self {
            present: Vec::new(),
            directories: Vec::new(),
        }
    }

    pub fn with_directories(mut self, paths: Vec<PathBuf>) -> Self {
        self.directories = paths;
        self
    }
}

impl HealthProbe for FakeHealthProbe {
    fn inspect(&self, path: &Path) -> PathState {
        if self.directories.iter().any(|known| known == path) {
            PathState::Directory
        } else if self.present.iter().any(|known| known == path) {
            PathState::Present
        } else {
            PathState::Missing
        }
    }
}

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
///   all the same — see [`crate::dpkg_config`].
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
            let files = match apt.installed_files(component) {
                Ok(files) => files,
                Err(error) => {
                    return HealthResult::Undetermined(format!(
                        "the dpkg file list for {component} could not be read: {error}"
                    ));
                }
            };
            for path in &files.paths {
                if !files.is_required(path) {
                    continue;
                }
                match probe.inspect(path) {
                    PathState::Present | PathState::Directory => {}
                    PathState::Missing => {
                        return HealthResult::Failed(format!(
                            "{component} reports version {version} but {} is missing, \
                             though dpkg lists it as installed",
                            path.display()
                        ));
                    }
                }
            }
            HealthResult::Healthy
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apt::{DebFields, FakeAptDriver};

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

    #[test]
    fn the_probe_reads_the_link_and_not_what_it_points_at() {
        let root = std::env::temp_dir().join(format!(
            "better-os-health-probe-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let file = root.join("file");
        std::fs::write(&file, b"x").unwrap();
        let dangling = root.join("dangling");
        std::os::unix::fs::symlink(root.join("no-such-target"), &dangling).unwrap();

        let probe = SystemHealthProbe;
        assert_eq!(probe.inspect(&root), PathState::Directory);
        assert_eq!(probe.inspect(&file), PathState::Present);
        assert_eq!(probe.inspect(&dangling), PathState::Present);
        assert_eq!(probe.inspect(&root.join("absent")), PathState::Missing);

        std::fs::remove_dir_all(&root).unwrap();
    }
}
