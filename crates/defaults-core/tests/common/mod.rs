//! Shared fixtures: manifests that declare integrations, and adapter sets whose
//! readings the test controls.

#![allow(dead_code)]

use better_core::defaults::{
    AdapterId, DefaultIntegration, DefaultsValue, HealthPrerequisite, IntegrationExclusivity,
    IntegrationId, IntegrationKind, IntegrationTarget, ObservedValue, RequiredPrivilege,
    RestorePolicy, SessionEffect,
};
use better_core::manifest::{
    Artifact, ComponentCatalog, ComponentIcon, ComponentId, ComponentManifest, ComponentType,
    Lifecycle, TargetMatrix,
};
use defaults_platform::{AdapterSet, InMemoryAdapter};
use defaults_store::{RestoreState, Snapshot, SnapshotEntry, SnapshotStore, SystemIdentity};

pub fn component(value: &str) -> ComponentId {
    ComponentId::new(value).unwrap()
}

pub fn integration_id(value: &str) -> IntegrationId {
    IntegrationId::new(value).unwrap()
}

pub fn desktop(value: &str) -> DefaultsValue {
    DefaultsValue::DesktopEntry(value.to_string())
}

pub fn set(value: &str) -> ObservedValue {
    ObservedValue::Set {
        value: desktop(value),
    }
}

/// An application-handler integration, which is the shape most of these tests
/// need. Anything a test varies is a parameter; everything else is the same
/// every time so a failure points at what the test changed.
pub fn integration(id: &str, key: &str, desired: &str) -> DefaultIntegration {
    DefaultIntegration {
        id: integration_id(id),
        kind: IntegrationKind::ApplicationHandler,
        exclusivity: IntegrationExclusivity::Exclusive,
        target: IntegrationTarget {
            desired: desktop(desired),
            keys: vec![key.to_string()],
        },
        platforms: vec!["zorin".to_string()],
        sessions: vec!["gnome".to_string()],
        apply_adapter: AdapterId::XdgDefaultApp,
        verify_adapter: AdapterId::XdgDefaultApp,
        restore_policy: RestorePolicy::CapturedValue,
        privileges: RequiredPrivilege::User,
        session_effect: SessionEffect::Immediate,
        health_prerequisites: vec![HealthPrerequisite::Installed],
    }
}

pub fn manifest(id: &str, integrations: Vec<DefaultIntegration>) -> ComponentManifest {
    let asset = format!("{id}_1.0.0_ubuntu-24.04_amd64.deb");
    ComponentManifest {
        schema_version: 2,
        id: component(id),
        display_name: id.to_string(),
        component_type: ComponentType::Replacement,
        version: "1.0.0".parse().unwrap(),
        summary: None,
        icon: ComponentIcon::Generic,
        restart: None,
        targets: TargetMatrix {
            distributions: vec!["ubuntu".to_string(), "zorin".to_string()],
            releases: vec!["24.04".to_string()],
            architectures: vec!["amd64".to_string()],
        },
        replaces: Vec::new(),
        enhances: Vec::new(),
        dependencies: Vec::new(),
        conflicts: Vec::new(),
        artifacts: vec![Artifact {
            release: "24.04".to_string(),
            architecture: "amd64".to_string(),
            url: format!("https://example.com/{asset}"),
            sha256: "a".repeat(64),
            release_asset: asset,
            download_size_bytes: None,
            required_disk_bytes: None,
            signature: None,
        }],
        lifecycle: Lifecycle {
            install: "mock-install".to_string(),
            enable: "mock-enable".to_string(),
            disable: "mock-disable".to_string(),
            remove: "mock-remove".to_string(),
            rollback: "mock-rollback".to_string(),
        },
        health_checks: Vec::new(),
        benchmarks: Vec::new(),
        permissions: Vec::new(),
        paths: Vec::new(),
        release_notes: Vec::new(),
        default_integrations: integrations,
    }
}

pub fn catalog(manifests: Vec<ComponentManifest>) -> ComponentCatalog {
    ComponentCatalog::from_manifests(manifests).unwrap()
}

/// An adapter set where one adapter has been configured by the test and the
/// rest are the untouched in-memory ones.
pub fn adapters_with(adapter: InMemoryAdapter) -> AdapterSet {
    let mut set = AdapterSet::in_memory();
    set.insert(Box::new(adapter));
    set
}

/// The in-memory adapter every application-handler integration in these tests
/// declares, seeded with what the system currently says.
pub fn xdg_adapter(readings: &[(&str, ObservedValue)]) -> InMemoryAdapter {
    let mut adapter = InMemoryAdapter::new(AdapterId::XdgDefaultApp);
    for (slot, value) in readings {
        adapter.preset(*slot, value.clone());
    }
    adapter
}

pub fn identity() -> SystemIdentity {
    SystemIdentity {
        distribution: "zorin".to_string(),
        desktop_session: "gnome".to_string(),
    }
}

/// A snapshot entry describing something Better Manager applied and verified.
pub fn applied_entry(
    component_id: &str,
    integration: &str,
    previous: ObservedValue,
    applied: &str,
) -> SnapshotEntry {
    SnapshotEntry {
        component_id: component(component_id),
        integration_id: integration_id(integration),
        previous_value: previous,
        better_value: desktop(applied),
        applied_value: Some(desktop(applied)),
        last_verified_value: Some(desktop(applied)),
        restore_state: RestoreState::Available,
    }
}

/// A snapshot store with a history already written into it.
pub fn store_with(directory: &std::path::Path, entries: Vec<SnapshotEntry>) -> SnapshotStore {
    let store = SnapshotStore::at_path(directory);
    if !entries.is_empty() {
        store.write(&Snapshot::new(identity(), entries)).unwrap();
    }
    store
}
