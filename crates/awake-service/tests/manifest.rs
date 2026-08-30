//! Better Awake's component manifest, validated the way the manager validates
//! one.
//!
//! Two things are checked. The manifest parses and passes `better-core`'s own
//! validation, which is what "a valid Better OS manifest" means. And the things
//! it says the component owns are the things the code names, so the declaration
//! cannot drift away from `awake_service::BUS_NAME` while both still look
//! correct on their own.
//!
//! Ticket 26 also puts uninstall behaviour in the manifest rather than in a
//! maintainer script nobody reads, so the release notes are asserted here too.

use awake_service::BUS_NAME;
use better_core::{
    ComponentCatalog, ComponentIcon, ComponentManifest, ComponentType, RestartScope,
};

const MANIFEST: &str = include_str!("../../../components/manifests/better-awake.yaml");

fn manifest() -> ComponentManifest {
    ComponentManifest::parse_yaml(MANIFEST).expect("the awake manifest must be valid")
}

#[test]
fn the_manifest_parses_and_validates() {
    let manifest = manifest();
    manifest.validate().expect("validation");
    assert_eq!(manifest.id.as_str(), "better-awake");
    assert_eq!(manifest.component_type, ComponentType::Enhancement);
    // No closed-set icon fits a keep-awake utility, and guessing one from the
    // component id is exactly what the enum exists to prevent.
    assert_eq!(manifest.icon, ComponentIcon::Generic);
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
    // Two releases times two architectures is four.
    assert_eq!(expected, 4);
}

#[test]
fn every_capability_the_component_needs_is_declared_with_a_reason() {
    let manifest = manifest();
    let names: Vec<&str> = manifest
        .permissions
        .iter()
        .map(|permission| permission.name.as_str())
        .collect();
    for required in [
        "session-bus-name",
        "logind-inhibitor",
        "status-notifier-item",
        "systemd-user-unit",
        "procfs-read",
        "sysfs-read",
        "filesystem-watch",
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
fn the_bus_name_the_service_owns_is_named_in_the_manifest() {
    let manifest = manifest();
    assert!(
        manifest
            .permissions
            .iter()
            .any(|permission| permission.reason.contains(BUS_NAME)),
        "the bus name the service owns must be named in the manifest, not just in code"
    );
}

#[test]
fn the_unmet_inhibitor_capabilities_are_declared_rather_than_implied() {
    let manifest = manifest();
    let inhibitor = manifest
        .permissions
        .iter()
        .find(|permission| permission.name == "logind-inhibitor")
        .expect("the inhibitor capability must be declared");
    let reason = inhibitor.reason.to_lowercase();
    for locked in ["sleep", "idle"] {
        assert!(
            reason.contains(locked),
            "the {locked} lock the service actually takes is not named"
        );
    }
    for unmet in ["display-blank", "automatic-lock"] {
        assert!(
            reason.contains(unmet),
            "{unmet} has no logind lock and must be declared unmet, not left unsaid"
        );
    }
}

#[test]
fn the_manifest_declares_the_binaries_and_the_state_files() {
    let manifest = manifest();
    for path in [
        "/usr/bin/better-awake-service",
        "/usr/bin/awake-tray",
        "/usr/bin/awake-gui",
        "~/.local/state/better-awake/awake-rules.json",
        "~/.local/state/better-awake/awake-history.json",
        "~/.local/state/better-awake/awake-service-state.json",
    ] {
        assert!(
            manifest.paths.iter().any(|declared| declared == path),
            "the manifest does not declare {path}"
        );
    }
}

#[test]
fn the_uninstall_behavior_ticket_26_requires_is_written_in_the_release_notes() {
    let manifest = manifest();
    let notes: Vec<String> = manifest
        .release_notes
        .iter()
        .map(|note| note.to_lowercase())
        .collect();
    let mentions = |needles: &[&str]| {
        notes
            .iter()
            .any(|note| needles.iter().all(|needle| note.contains(needle)))
    };

    assert!(
        mentions(&["uninstall", "releases every inhibitor"]),
        "uninstall must say it releases the inhibitors it holds"
    );
    assert!(
        mentions(&["uninstall", "disables", "autostart"]),
        "uninstall must say the systemd user unit and the tray autostart entry are disabled"
    );
    assert!(
        mentions(&["rules and history", "explicit"]),
        "removing user rules and history must be an explicit choice, stated as one"
    );
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

#[test]
fn the_benchmark_has_a_regression_budget() {
    let manifest = manifest();
    let idle = manifest
        .benchmarks
        .iter()
        .find(|benchmark| benchmark.name == "idle-overhead")
        .expect("idle-overhead has no definition");
    assert!(!idle.workload.trim().is_empty());
    assert!(!idle.metric.trim().is_empty());
    assert!(
        idle.maximum_regression_percent >= 0.0,
        "idle-overhead has no regression budget"
    );
}
