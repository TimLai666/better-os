//! Better Launcher's component manifest, validated the way the manager
//! validates one.
//!
//! Two things are checked. The manifest parses and passes `better-core`'s own
//! validation, which is what "a valid Better OS manifest" means. And the
//! settings it says the component touches are the settings the code names, so
//! the declaration cannot drift away from `launcher-platform::shortcut` while
//! both still look correct on their own.

use better_core::{
    ComponentCatalog, ComponentIcon, ComponentManifest, ComponentType, RestartScope,
};
use launcher_platform::GnomeCustomKeybinding;
use launcher_platform::activation::SingleInstance;

const MANIFEST: &str = include_str!("../../../components/manifests/better-launcher.yaml");

fn manifest() -> ComponentManifest {
    ComponentManifest::parse_yaml(MANIFEST).expect("the launcher manifest must be valid")
}

#[test]
fn the_manifest_parses_and_validates() {
    let manifest = manifest();
    manifest.validate().expect("validation");
    assert_eq!(manifest.id.as_str(), "better-launcher");
    assert_eq!(manifest.icon, ComponentIcon::Launcher);
    assert_eq!(manifest.restart, Some(RestartScope::Application));
}

#[test]
fn it_loads_into_a_catalog_beside_the_other_first_party_components() {
    let manifests = [
        include_str!("../../../components/manifests/better-manager.yaml"),
        include_str!("../../../components/manifests/better-monitor.yaml"),
        MANIFEST,
    ]
    .into_iter()
    .map(|document| ComponentManifest::parse_yaml(document).unwrap())
    .collect::<Vec<_>>();

    let catalog = ComponentCatalog::from_manifests(manifests).expect("catalog");
    assert_eq!(catalog.manifests().count(), 3);
}

#[test]
fn every_declared_target_has_exactly_one_artifact() {
    let manifest = manifest();
    let expected = manifest.targets.releases.len() * manifest.targets.architectures.len();
    assert_eq!(manifest.artifacts.len(), expected);
    // Validation already refuses a missing or duplicated variant; this is the
    // arithmetic behind it, stated so a widened target matrix fails here too.
    assert_eq!(expected, 4);
}

#[test]
fn the_launcher_enhances_rather_than_replaces_so_the_overview_survives() {
    let manifest = manifest();
    assert_eq!(manifest.component_type, ComponentType::Enhancement);
    assert!(
        manifest.replaces.is_empty(),
        "installing the launcher must not declare the GNOME overview replaced"
    );
    assert!(!manifest.enhances.is_empty());
}

#[test]
fn the_manifest_declares_the_settings_the_shortcut_code_names() {
    let manifest = manifest();
    for setting in GnomeCustomKeybinding::for_launcher().declared_settings() {
        assert!(
            manifest.paths.contains(&setting),
            "the manifest does not declare {setting}, which the shortcut integration touches"
        );
    }
    assert!(
        manifest
            .paths
            .iter()
            .any(|path| path == "/usr/bin/better-launcher"),
        "the manifest must declare the binary it installs"
    );
}

#[test]
fn the_optional_gesture_adapter_and_the_session_bus_name_are_both_declared() {
    let manifest = manifest();
    let names: Vec<&str> = manifest
        .permissions
        .iter()
        .map(|permission| permission.name.as_str())
        .collect();
    for required in [
        "desktop-session",
        "application-metadata-read",
        "session-bus-name",
        "gnome-keyboard-shortcut",
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
    assert!(
        manifest
            .permissions
            .iter()
            .any(|permission| permission.reason.contains(SingleInstance::DEFAULT_NAME)),
        "the bus name the launcher owns must be named in the manifest, not just in code"
    );
}

#[test]
fn the_benchmarks_issue_2_asks_for_are_defined_with_a_regression_budget() {
    let manifest = manifest();
    let names: Vec<&str> = manifest
        .benchmarks
        .iter()
        .map(|benchmark| benchmark.name.as_str())
        .collect();
    for required in [
        "warm-search-update",
        "warm-overlay-open",
        "application-list-update",
        "idle-overhead",
    ] {
        assert!(names.contains(&required), "{required} has no definition");
    }
    for benchmark in &manifest.benchmarks {
        assert!(!benchmark.workload.trim().is_empty(), "{}", benchmark.name);
        assert!(!benchmark.metric.trim().is_empty(), "{}", benchmark.name);
        assert!(
            benchmark.maximum_regression_percent >= 0.0,
            "{} has no regression budget",
            benchmark.name
        );
    }
}

#[test]
fn the_rollback_behavior_is_written_down_rather_than_implied() {
    let manifest = manifest();
    assert!(!manifest.lifecycle.rollback.trim().is_empty());
    assert!(
        manifest
            .release_notes
            .iter()
            .any(|note| note.to_lowercase().contains("rollback")),
        "a rollback descriptor with no written plan beside it explains nothing"
    );
    assert!(
        !manifest.health_checks.is_empty(),
        "rollback is only meaningful next to a health check that can fail"
    );
}
