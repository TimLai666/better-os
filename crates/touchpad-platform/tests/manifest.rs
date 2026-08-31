//! Better Touchpad's component manifest, validated the way the manager
//! validates one.
//!
//! The same two jobs `launcher-platform/tests/manifest.rs` does. The manifest
//! parses and passes `better-core`'s own validation, which is what "a valid
//! Better OS manifest" means. And the things it declares are the things the
//! code actually has: the health check IDs `touchpad-core` emits, and the dconf
//! keys the GNOME backend can write. Either half can look correct on its own
//! while having drifted from the other, which is exactly the failure a manifest
//! nobody tests produces.

use better_core::{
    ComponentCatalog, ComponentIcon, ComponentManifest, ComponentType, RestartScope,
};
use touchpad_core::{Capabilities, HealthFacts, HealthReport, SettingId};
use touchpad_platform::GnomeBackend;

const MANIFEST: &str = include_str!("../../../components/manifests/better-touchpad.yaml");

fn manifest() -> ComponentManifest {
    ComponentManifest::parse_yaml(MANIFEST).expect("the touchpad manifest must be valid")
}

#[test]
fn the_manifest_parses_and_validates() {
    let manifest = manifest();
    manifest.validate().expect("validation");
    assert_eq!(manifest.id.as_str(), "better-touchpad");
    assert_eq!(manifest.icon, ComponentIcon::Touchpad);
    assert_eq!(manifest.restart, Some(RestartScope::Application));
}

#[test]
fn it_loads_into_a_catalog_beside_the_other_first_party_components() {
    let manifests = [
        include_str!("../../../components/manifests/better-manager.yaml"),
        include_str!("../../../components/manifests/better-monitor.yaml"),
        include_str!("../../../components/manifests/better-launcher.yaml"),
        MANIFEST,
    ]
    .into_iter()
    .map(|document| ComponentManifest::parse_yaml(document).unwrap())
    .collect::<Vec<_>>();

    let catalog = ComponentCatalog::from_manifests(manifests).expect("catalog");
    assert_eq!(catalog.manifests().count(), 4);
}

#[test]
fn every_declared_target_has_exactly_one_artifact() {
    let manifest = manifest();
    let expected = manifest.targets.releases.len() * manifest.targets.architectures.len();
    assert_eq!(manifest.artifacts.len(), expected);
    assert_eq!(expected, 4);
}

#[test]
fn installing_the_touchpad_replaces_no_gnome_setting() {
    let manifest = manifest();
    assert_eq!(manifest.component_type, ComponentType::Enhancement);
    assert!(
        manifest.replaces.is_empty(),
        "installing Better Touchpad must not declare a GNOME setting replaced; \
         the Mac-style preset is offered and confirmed, never applied by the install"
    );
    assert!(!manifest.enhances.is_empty());
}

/// The manifest names the checks the binary reports, not a second vocabulary
/// invented for the catalog. Five are always present; the last two are the
/// alternatives for a run in safe mode and a run with the integration switched
/// off, so a single evaluation can never produce both.
#[test]
fn the_declared_health_checks_are_the_ones_the_code_emits() {
    let manifest = manifest();
    let capabilities = Capabilities::everything_immediate();
    let facts = |safe_mode: bool, integration_enabled: bool| HealthFacts {
        configuration_readable: true,
        configuration_detail: "read".to_string(),
        backend_name: "gnome",
        backend_reachable: true,
        backend_detail: "the dconf service answered".to_string(),
        devices_found: 1,
        selected_device: Some("usb:06cb:ce67"),
        capabilities: &capabilities,
        capture_present: true,
        safe_mode,
        integration_enabled,
    };

    let mut emitted: Vec<String> = Vec::new();
    for (safe_mode, integration_enabled) in [(false, true), (false, false), (true, true)] {
        for check in HealthReport::evaluate(&facts(safe_mode, integration_enabled)).checks {
            if !emitted.contains(&check.id) {
                emitted.push(check.id);
            }
        }
    }

    for id in &emitted {
        assert!(
            manifest.health_checks.contains(id),
            "{id} is reported by the binary and not declared in the manifest"
        );
    }
    for declared in &manifest.health_checks {
        assert!(
            emitted.contains(declared),
            "{declared} is declared in the manifest and no health check emits it"
        );
    }
}

/// A declaration that has drifted away from `GnomeBackend`'s key table would
/// tell Better Manager the component touches settings it does not, or hide
/// settings it does.
#[test]
fn the_manifest_declares_every_gnome_key_the_backend_can_write() {
    let manifest = manifest();
    for setting in SettingId::ALL {
        let Some(path) = GnomeBackend::key_path(setting) else {
            // GNOME 46 has no key for the scroll factors or smooth scrolling.
            // Those are shown as unavailable and never written.
            continue;
        };
        assert!(
            manifest.paths.contains(&path),
            "the manifest does not declare {path}, which the GNOME backend writes"
        );
    }
    for path in &manifest.paths {
        if let Some(key) = path.strip_prefix(touchpad_platform::TOUCHPAD_PREFIX) {
            assert!(
                SettingId::ALL
                    .into_iter()
                    .filter_map(GnomeBackend::key_path)
                    .any(|mapped| mapped.ends_with(&format!("/{key}"))),
                "the manifest declares {path}, which no setting maps to"
            );
        }
    }
}

#[test]
fn the_binary_the_configuration_and_the_safe_mode_entry_are_all_declared() {
    let manifest = manifest();
    for required in [
        "/usr/bin/better-touchpad",
        "/usr/share/applications/better-touchpad.desktop",
        "/usr/share/applications/better-touchpad-safe-mode.desktop",
        "~/.config/better-os/touchpad/config.json",
        "~/.config/better-os/touchpad/backup.json",
        "~/.config/better-os/touchpad/safe-mode",
    ] {
        assert!(
            manifest.paths.iter().any(|path| path == required),
            "{required} is not declared"
        );
    }
}

#[test]
fn the_configuration_paths_are_the_ones_the_store_uses() {
    let manifest = manifest();
    let directory = touchpad_core::TouchpadStore::new("/home/example/.config/better-os/touchpad");
    for path in [
        directory.config_path(),
        directory.backup_path(),
        directory.safe_mode_path(),
    ] {
        let declared = path
            .to_string_lossy()
            .replace("/home/example/.config", "~/.config");
        assert!(
            manifest.paths.contains(&declared),
            "the store writes {declared} and the manifest does not declare it"
        );
    }
}

#[test]
fn every_permission_gives_a_reason_and_the_gesture_gap_is_one_of_them() {
    let manifest = manifest();
    let names: Vec<&str> = manifest
        .permissions
        .iter()
        .map(|permission| permission.name.as_str())
        .collect();
    for required in [
        "desktop-session",
        "dconf-read",
        "dconf-write",
        "procfs-read",
        "sysfs-read",
        "gesture-adapter",
    ] {
        assert!(names.contains(&required), "{required} is not declared");
    }
    for permission in &manifest.permissions {
        assert!(
            !permission.reason.trim().is_empty(),
            "{} declares no reason",
            permission.name
        );
    }
}

#[test]
fn the_benchmarks_issue_3_asks_for_are_defined_with_a_regression_budget() {
    let manifest = manifest();
    let names: Vec<&str> = manifest
        .benchmarks
        .iter()
        .map(|benchmark| benchmark.name.as_str())
        .collect();
    for required in [
        "setting-apply-and-verify",
        "setting-restore-and-verify",
        "settings-read-back",
        "window-ready",
        "idle-overhead",
    ] {
        assert!(names.contains(&required), "{required} has no definition");
    }
    for benchmark in &manifest.benchmarks {
        assert!(!benchmark.workload.trim().is_empty(), "{}", benchmark.name);
        assert!(!benchmark.metric.trim().is_empty(), "{}", benchmark.name);
        assert!(
            benchmark.maximum_regression_percent > 0.0,
            "{} has no regression budget",
            benchmark.name
        );
    }
}

#[test]
fn the_safe_mode_and_restore_behavior_is_written_down_rather_than_implied() {
    let manifest = manifest();
    assert!(!manifest.lifecycle.rollback.trim().is_empty());
    let notes = manifest.release_notes.join(" ").to_lowercase();
    for subject in ["rollback", "safe mode", "restore"] {
        assert!(
            notes.contains(subject),
            "a {subject} descriptor with no written plan beside it explains nothing"
        );
    }
    assert!(
        !manifest.health_checks.is_empty(),
        "rollback is only meaningful next to a health check that can fail"
    );
}
