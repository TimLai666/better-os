//! The Better Storage component manifest, validated the way the manager does.
//!
//! Better Manager treats a manifest as untrusted input. If this one does not
//! pass the same validation the catalog applies, the component is not
//! installable, so it is checked here rather than discovered at release time.

use better_core::{ComponentManifest, ComponentType, RestartScope};

const MANIFEST: &str = include_str!("../../../components/manifests/better-storage.yaml");

#[test]
fn the_manifest_passes_the_same_validation_the_catalog_applies() {
    let manifest = ComponentManifest::parse_yaml(MANIFEST).expect("a valid manifest");
    assert_eq!(manifest.id.to_string(), "better-storage");
    assert_eq!(manifest.component_type, ComponentType::Enhancement);
    // Installing a session service only takes full effect at the next login.
    assert_eq!(manifest.restart, Some(RestartScope::Logout));
}

#[test]
fn the_manifest_declares_its_services_integration_paths_and_rollback() {
    let manifest = ComponentManifest::parse_yaml(MANIFEST).unwrap();

    let paths = manifest.paths.join(" ");
    assert!(
        paths.contains("better-storage-service"),
        "no service binary"
    );
    assert!(paths.contains("systemd/user"), "no session service unit");
    assert!(paths.contains("dbus-1/services"), "no D-Bus service file");
    assert!(
        paths.contains("storage-preferences.json"),
        "no configuration path"
    );

    let permissions: Vec<&str> = manifest
        .permissions
        .iter()
        .map(|permission| permission.name.as_str())
        .collect();
    assert!(permissions.contains(&"system-dbus-udisks2-client"));
    assert!(permissions.contains(&"session-dbus-own-name"));
    assert!(
        manifest
            .permissions
            .iter()
            .all(|permission| !permission.reason.trim().is_empty()),
        "every declared permission needs a reason"
    );

    assert!(!manifest.health_checks.is_empty());
    assert!(!manifest.lifecycle.rollback.trim().is_empty());
}

#[test]
fn the_declared_benchmarks_match_the_ones_that_actually_run() {
    let manifest = ComponentManifest::parse_yaml(MANIFEST).unwrap();
    let names: Vec<&str> = manifest
        .benchmarks
        .iter()
        .map(|benchmark| benchmark.name.as_str())
        .collect();
    // These three are measured by `storage-core/tests/throughput.rs` and
    // `storage-service/tests/latency.rs`. Device-level throughput needs
    // hardware and is a recorded follow-up, not a declared benchmark here.
    assert!(names.contains(&"state-machine-throughput"));
    assert!(names.contains(&"event-to-state-update-latency"));
    assert!(names.contains(&"service-idle-overhead"));
}

#[test]
fn every_supported_target_has_an_artifact() {
    let manifest = ComponentManifest::parse_yaml(MANIFEST).unwrap();
    let expected = manifest.targets.releases.len() * manifest.targets.architectures.len();
    assert_eq!(manifest.artifacts.len(), expected);
}
