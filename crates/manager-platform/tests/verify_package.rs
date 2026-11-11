//! `verify_package` is the client-side verify: the shared health rule, read
//! without root through dpkg and the filesystem.
//!
//! The fake-driven tests pin each answer the rule gives. The read-only tests
//! ask the real dpkg database about `dpkg` itself, which is installed on every
//! host the suite runs on and is never written to.

use std::path::PathBuf;

use better_core::package_health::{
    FakeHealthProbe, HealthVerdict, InstalledFiles, SystemHealthProbe,
};
use manager_platform::dpkg::{
    DpkgProbe, FixedPackageStateProbe, PackageStateProbe, verify_package,
};

const AWAKE: &str = "better-awake";

fn path(value: &str) -> PathBuf {
    PathBuf::from(value)
}

fn listed(paths: &[&str]) -> InstalledFiles {
    InstalledFiles {
        paths: paths.iter().map(|value| path(value)).collect(),
        ..InstalledFiles::default()
    }
}

#[test]
fn an_installed_package_with_every_required_file_is_healthy() {
    let packages = FixedPackageStateProbe::new(&[(AWAKE, "0.2.8")]).with_files(
        AWAKE,
        listed(&["/usr/bin/better-awake", "/usr/share/better-awake/icon.png"]),
    );
    let health = FakeHealthProbe::with_files(vec![
        path("/usr/bin/better-awake"),
        path("/usr/share/better-awake/icon.png"),
    ]);

    assert_eq!(
        verify_package(&packages, &health, AWAKE),
        HealthVerdict::Healthy
    );
}

#[test]
fn a_missing_required_file_fails_and_names_the_file() {
    let packages = FixedPackageStateProbe::new(&[(AWAKE, "0.2.8")]).with_files(
        AWAKE,
        listed(&["/usr/bin/better-awake", "/usr/share/better-awake/icon.png"]),
    );
    // The binary is there; the icon dpkg lists is not.
    let health = FakeHealthProbe::with_files(vec![path("/usr/bin/better-awake")]);

    match verify_package(&packages, &health, AWAKE) {
        HealthVerdict::Failed(message) => assert!(
            message.contains("/usr/share/better-awake/icon.png"),
            "the failure should name the missing file, got: {message}"
        ),
        other => panic!("a missing required file must fail the verify, got {other:?}"),
    }
}

#[test]
fn a_missing_conffile_is_still_healthy() {
    // A conffile the owner deleted on purpose does not break the package.
    let packages = FixedPackageStateProbe::new(&[(AWAKE, "0.2.8")]).with_files(
        AWAKE,
        InstalledFiles {
            paths: vec![
                path("/usr/bin/better-awake"),
                path("/etc/better-awake/config.toml"),
            ],
            conffiles: vec![path("/etc/better-awake/config.toml")],
            excluded: Vec::new(),
        },
    );
    let health = FakeHealthProbe::with_files(vec![path("/usr/bin/better-awake")]);

    assert_eq!(
        verify_package(&packages, &health, AWAKE),
        HealthVerdict::Healthy
    );
}

#[test]
fn a_missing_path_dpkg_was_configured_to_exclude_is_still_healthy() {
    // dpkg lists the documentation file, but this machine was told never to
    // unpack it, so its absence is not a broken install.
    let packages = FixedPackageStateProbe::new(&[(AWAKE, "0.2.8")]).with_files(
        AWAKE,
        InstalledFiles {
            paths: vec![
                path("/usr/bin/better-awake"),
                path("/usr/share/doc/better-awake/README.md"),
            ],
            conffiles: Vec::new(),
            excluded: vec![path("/usr/share/doc/better-awake/README.md")],
        },
    );
    let health = FakeHealthProbe::with_files(vec![path("/usr/bin/better-awake")]);

    assert_eq!(
        verify_package(&packages, &health, AWAKE),
        HealthVerdict::Healthy
    );
}

#[test]
fn a_package_that_is_not_installed_fails_with_the_plain_reason() {
    let packages = FixedPackageStateProbe::new(&[]);
    let health = FakeHealthProbe::empty();

    assert_eq!(
        verify_package(&packages, &health, AWAKE),
        HealthVerdict::Failed(format!("{AWAKE} is not installed"))
    );
}

#[test]
fn an_installed_package_without_a_readable_file_list_is_undetermined() {
    // Installed, but dpkg gave no file list for it: the rule cannot say the
    // package is sound, so it must not say so.
    let packages = FixedPackageStateProbe::new(&[(AWAKE, "0.2.8")]);
    let health = FakeHealthProbe::with_files(vec![path("/usr/bin/better-awake")]);

    assert!(
        matches!(
            verify_package(&packages, &health, AWAKE),
            HealthVerdict::Undetermined(_)
        ),
        "an installed package with no file list must be undetermined, not healthy"
    );
}

#[test]
fn a_listed_directory_counts_as_present() {
    // Directories are shared between packages, so a directory dpkg lists is
    // answered as a directory, not as a missing file.
    let packages = FixedPackageStateProbe::new(&[(AWAKE, "0.2.8")]).with_files(
        AWAKE,
        listed(&["/usr/share/better-awake", "/usr/bin/better-awake"]),
    );
    let health = FakeHealthProbe::with_files(vec![path("/usr/bin/better-awake")])
        .with_directories(vec![path("/usr/share/better-awake")]);

    assert_eq!(
        verify_package(&packages, &health, AWAKE),
        HealthVerdict::Healthy
    );
}

#[test]
fn the_real_dpkg_lists_the_files_of_an_installed_package() {
    // Read-only: asks dpkg-query which files the dpkg package put on disk.
    let files = DpkgProbe
        .installed_files("dpkg")
        .expect("dpkg-query lists the files of the dpkg package");

    assert!(
        files.paths.contains(&path("/usr/bin/dpkg")),
        "the listing should include /usr/bin/dpkg, got {} paths",
        files.paths.len()
    );
}

#[test]
fn a_package_name_that_could_be_an_option_is_refused_before_dpkg_runs() {
    assert!(DpkgProbe.installed_files("--evil").is_err());
}

#[test]
fn a_package_name_with_whitespace_is_refused_before_dpkg_runs() {
    assert!(DpkgProbe.installed_files("better awake").is_err());
}

#[test]
fn the_real_dpkg_package_verifies_healthy_on_this_host() {
    // Read-only: the shared rule against the real dpkg database and this
    // host's filesystem. dpkg is installed on every host the suite runs on.
    assert_eq!(
        verify_package(&DpkgProbe, &SystemHealthProbe, "dpkg"),
        HealthVerdict::Healthy
    );
}
