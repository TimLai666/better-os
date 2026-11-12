//! The unit discovery is only useful if it finds the units the published
//! packages actually install. The file lists in `fixtures/deb-file-lists-0.2.9.txt`
//! are `dpkg-deb -c` output for the release assets, so this is a test against
//! the real thing rather than against a list written to make it pass.

use std::collections::BTreeMap;
use std::path::PathBuf;

use better_core::package_health::InstalledFiles;
use manager_platform::user_units::{is_plain_unit_name, service_units};

const FIXTURE: &str = include_str!("fixtures/deb-file-lists-0.2.9.txt");

/// Splits the fixture into one `InstalledFiles` per package.
///
/// A `dpkg-deb -c` line ends with the path, which is the only field this needs;
/// none of these packages has a path with whitespace in it, and the fixture says
/// so. Conffiles are left empty: which paths are configuration files does not
/// change which paths are units, and the fixture is not a substitute for
/// `dpkg-query`'s `${Conffiles}` field.
fn file_lists() -> BTreeMap<String, InstalledFiles> {
    let mut lists = BTreeMap::new();
    let mut package: Option<String> = None;
    let mut paths: Vec<PathBuf> = Vec::new();

    let mut finish = |package: &mut Option<String>, paths: &mut Vec<PathBuf>| {
        if let Some(package) = package.take() {
            lists.insert(
                package,
                InstalledFiles {
                    paths: std::mem::take(paths),
                    ..InstalledFiles::default()
                },
            );
        }
    };

    for line in FIXTURE.lines() {
        if let Some(name) = line.strip_prefix("## package: ") {
            finish(&mut package, &mut paths);
            package = Some(name.trim().to_string());
            continue;
        }
        // Permission string, owner/group, size, date, time, path.
        if !(line.starts_with('-') || line.starts_with('d') || line.starts_with('l')) {
            continue;
        }
        let Some(path) = line.split_whitespace().last() else {
            continue;
        };
        // dpkg-deb writes paths relative to the package root: "./usr/bin/x".
        let path = path.strip_prefix('.').unwrap_or(path);
        let path = if path.is_empty() { "/" } else { path };
        paths.push(PathBuf::from(path));
    }
    finish(&mut package, &mut paths);
    lists
}

#[test]
fn the_four_published_units_are_found_in_the_real_file_lists() {
    let lists = file_lists();
    for (package, unit) in [
        ("better-awake", "better-awake.service"),
        ("better-monitor", "better-monitor.service"),
        ("better-storage", "better-storage.service"),
        ("better-touchpad", "better-touchpad-gestures.service"),
    ] {
        let files = lists
            .get(package)
            .unwrap_or_else(|| panic!("the fixture has {package}"));
        assert_eq!(
            service_units(files),
            [unit],
            "{package} should own exactly {unit}"
        );
        assert!(is_plain_unit_name(unit), "{unit} must be startable");
        let expected = PathBuf::from(format!("/usr/lib/systemd/user/{unit}"));
        assert!(
            files.paths.contains(&expected),
            "{package} does not list {}",
            expected.display()
        );
    }
}

#[test]
fn a_unit_on_the_system_bus_is_not_a_user_unit() {
    let lists = file_lists();
    let files = lists
        .get("better-manager-daemon")
        .expect("the fixture has the daemon");
    // It installs /usr/lib/systemd/system/better-manager-daemon.service and a
    // D-Bus system service. Neither is something `systemctl --user` can start,
    // so neither may be returned: enabling them would fail for a component
    // whose package and health are perfectly sound.
    assert!(service_units(files).is_empty());
    assert!(
        files
            .paths
            .iter()
            .any(|path| path.ends_with("better-manager-daemon.service")),
        "the fixture must really contain the system unit"
    );
    assert!(
        files.paths.iter().any(|path| path
            .parent()
            .is_some_and(|parent| parent.ends_with("dbus-1/system-services"))),
        "the fixture must really contain the D-Bus activation file"
    );
}

#[test]
fn the_unit_directory_itself_is_not_a_unit() {
    let lists = file_lists();
    let files = &lists["better-awake"];
    // dpkg lists the directory that holds the unit. It is not one.
    assert!(
        files
            .paths
            .contains(&PathBuf::from("/usr/lib/systemd/user"))
    );
    assert_eq!(service_units(files), ["better-awake.service"]);
}
