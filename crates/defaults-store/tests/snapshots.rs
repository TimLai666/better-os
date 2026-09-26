//! Snapshot round-trip, history, and damage reporting.

use better_core::defaults::{DefaultsValue, IntegrationId, ObservedValue};
use better_core::manifest::ComponentId;
use defaults_store::{
    Damage, RestoreState, Snapshot, SnapshotEntry, SnapshotError, SnapshotStore, SystemIdentity,
};

fn identity() -> SystemIdentity {
    SystemIdentity {
        distribution: "zorin".to_string(),
        desktop_session: "gnome".to_string(),
    }
}

fn entry(component: &str, integration: &str, previous: ObservedValue) -> SnapshotEntry {
    SnapshotEntry {
        component_id: ComponentId::new(component).unwrap(),
        integration_id: IntegrationId::new(integration).unwrap(),
        previous_value: previous,
        better_value: DefaultsValue::DesktopEntry("io.betteros.Files.desktop".to_string()),
        applied_value: None,
        last_verified_value: None,
        restore_state: RestoreState::Available,
    }
}

fn nautilus() -> ObservedValue {
    ObservedValue::Set {
        value: DefaultsValue::DesktopEntry("org.gnome.Nautilus.desktop".to_string()),
    }
}

#[test]
fn a_snapshot_round_trips_through_the_store_unchanged() {
    let directory = tempfile::tempdir().unwrap();
    let store = SnapshotStore::at_path(directory.path());
    let snapshot = Snapshot::new(
        identity(),
        vec![entry("better-files", "default-file-manager", nautilus())],
    );

    store.write(&snapshot).unwrap();
    let history = store.history().unwrap();

    assert_eq!(history.snapshots(), std::slice::from_ref(&snapshot));
    assert!(history.damaged().is_empty());
    assert_eq!(
        history
            .latest_entry(
                &ComponentId::new("better-files").unwrap(),
                &IntegrationId::new("default-file-manager").unwrap()
            )
            .map(|entry| entry.previous_value.clone()),
        Some(nautilus())
    );
}

#[test]
fn writing_again_keeps_the_earlier_record_instead_of_replacing_it() {
    let directory = tempfile::tempdir().unwrap();
    let store = SnapshotStore::at_path(directory.path());
    let baseline = Snapshot::new(
        identity(),
        vec![entry("better-files", "default-file-manager", nautilus())],
    );
    store.write(&baseline).unwrap();

    let applied = baseline.evolve(vec![SnapshotEntry {
        applied_value: Some(DefaultsValue::DesktopEntry(
            "io.betteros.Files.desktop".to_string(),
        )),
        ..baseline.entries[0].clone()
    }]);
    store.write(&applied).unwrap();

    let history = store.history().unwrap();
    assert_eq!(history.snapshots().len(), 2);
    // The baseline still says what the desktop looked like before anything
    // changed, which is the value a restore has to return to.
    assert_eq!(history.baseline().unwrap().entries[0].applied_value, None);
    assert_eq!(
        history.latest().unwrap().entries[0].applied_value,
        Some(DefaultsValue::DesktopEntry(
            "io.betteros.Files.desktop".to_string()
        ))
    );
}

#[test]
fn evolving_one_entry_carries_every_other_entry_forward_untouched() {
    let baseline = Snapshot::new(
        identity(),
        vec![
            entry("better-files", "default-file-manager", nautilus()),
            entry(
                "better-monitor",
                "task-manager-shortcut",
                ObservedValue::Unset,
            ),
        ],
    );

    let restored = baseline.evolve(vec![SnapshotEntry {
        restore_state: RestoreState::AlreadyRestored,
        ..baseline.entries[0].clone()
    }]);

    assert_eq!(restored.entries[1], baseline.entries[1]);
    assert_eq!(
        restored.entries[0].restore_state,
        RestoreState::AlreadyRestored
    );
    assert_ne!(restored.snapshot_id, baseline.snapshot_id);
}

#[test]
fn evolving_appends_an_entry_the_baseline_never_had() {
    let baseline = Snapshot::new(
        identity(),
        vec![entry("better-files", "default-file-manager", nautilus())],
    );
    let extended = baseline.evolve(vec![entry(
        "better-monitor",
        "task-manager-shortcut",
        ObservedValue::Unset,
    )]);

    assert_eq!(extended.entries.len(), 2);
    assert_eq!(extended.entries[0], baseline.entries[0]);
}

#[test]
fn the_same_snapshot_id_is_refused_rather_than_overwritten() {
    let directory = tempfile::tempdir().unwrap();
    let store = SnapshotStore::at_path(directory.path());
    let snapshot = Snapshot::new(
        identity(),
        vec![entry("better-files", "default-file-manager", nautilus())],
    );

    store.write(&snapshot).unwrap();
    assert!(matches!(
        store.write(&snapshot),
        Err(SnapshotError::WouldOverwrite { .. })
    ));
}

#[test]
fn an_unreadable_snapshot_is_reported_and_the_good_ones_still_load() {
    let directory = tempfile::tempdir().unwrap();
    let store = SnapshotStore::at_path(directory.path());
    let snapshot = Snapshot::new(
        identity(),
        vec![entry("better-files", "default-file-manager", nautilus())],
    );
    store.write(&snapshot).unwrap();
    std::fs::write(directory.path().join("00-truncated.json"), b"{\"schema_ver").unwrap();

    let history = store.history().unwrap();

    assert_eq!(history.snapshots(), std::slice::from_ref(&snapshot));
    assert_eq!(history.damaged().len(), 1);
    assert!(matches!(
        history.damaged()[0].damage,
        Damage::Unreadable { .. }
    ));
}

#[test]
fn a_snapshot_from_a_newer_better_os_is_preserved_and_reported() {
    let directory = tempfile::tempdir().unwrap();
    let store = SnapshotStore::at_path(directory.path());
    std::fs::write(
        directory.path().join("00-future.json"),
        br#"{"schema_version": 99, "snapshot_id": "future", "created_at": 1,
             "system_identity": {"distribution": "zorin", "desktop_session": "gnome"},
             "entries": []}"#,
    )
    .unwrap();

    let history = store.history().unwrap();

    assert!(history.snapshots().is_empty());
    assert_eq!(
        history.damaged()[0].damage,
        Damage::UnsupportedSchema { version: 99 }
    );
    // The file is still there. A newer writer's record is never rewritten.
    assert!(directory.path().join("00-future.json").exists());
}

#[test]
fn an_incomplete_snapshot_is_reported_rather_than_used() {
    let directory = tempfile::tempdir().unwrap();
    let store = SnapshotStore::at_path(directory.path());
    std::fs::write(
        directory.path().join("00-incomplete.json"),
        br#"{"schema_version": 1, "snapshot_id": "", "created_at": 5,
             "system_identity": {"distribution": "zorin", "desktop_session": "gnome"},
             "entries": []}"#,
    )
    .unwrap();
    std::fs::write(
        directory.path().join("01-no-identity.json"),
        br#"{"schema_version": 1, "snapshot_id": "x", "created_at": 5,
             "system_identity": {"distribution": "", "desktop_session": ""},
             "entries": []}"#,
    )
    .unwrap();

    let history = store.history().unwrap();

    assert!(history.snapshots().is_empty());
    assert_eq!(history.damaged().len(), 2);
    assert!(
        history
            .damaged()
            .iter()
            .all(|damaged| matches!(damaged.damage, Damage::Incomplete { .. }))
    );
}

#[test]
fn a_snapshot_naming_one_integration_twice_is_incomplete() {
    let directory = tempfile::tempdir().unwrap();
    let store = SnapshotStore::at_path(directory.path());
    let snapshot = Snapshot {
        entries: vec![
            entry("better-files", "default-file-manager", nautilus()),
            entry("better-files", "default-file-manager", ObservedValue::Unset),
        ],
        ..Snapshot::new(identity(), Vec::new())
    };

    assert!(matches!(
        store.write(&snapshot),
        Err(SnapshotError::Invalid(Damage::Incomplete { .. }))
    ));
}

#[test]
fn a_directory_that_does_not_exist_yet_is_an_empty_history() {
    let directory = tempfile::tempdir().unwrap();
    let store = SnapshotStore::at_path(directory.path().join("never-written"));
    let history = store.history().unwrap();

    assert!(history.snapshots().is_empty());
    assert!(history.damaged().is_empty());
    assert!(history.baseline().is_none());
}

#[test]
fn the_last_value_better_manager_wrote_prefers_the_verified_one() {
    let applied = DefaultsValue::DesktopEntry("io.betteros.Files.desktop".to_string());
    let verified = DefaultsValue::DesktopEntry("org.gnome.Nautilus.desktop".to_string());
    let mut record = entry("better-files", "default-file-manager", nautilus());

    assert_eq!(record.last_known_value(), None);
    record.applied_value = Some(applied.clone());
    assert_eq!(record.last_known_value(), Some(&applied));
    record.last_verified_value = Some(verified.clone());
    assert_eq!(record.last_known_value(), Some(&verified));
}

/// Exactly what a build from before per-type capture wrote: schema version 1,
/// and a previous value that is one owner.
const VERSION_1_SNAPSHOT: &[u8] = br#"{
  "schema_version": 1,
  "snapshot_id": "00000000001790000000-4242-000000000",
  "created_at": 1790000000,
  "system_identity": { "distribution": "zorin", "desktop_session": "gnome" },
  "entries": [
    {
      "component_id": "better-files",
      "integration_id": "default-file-manager",
      "previous_value": {
        "state": "set",
        "value": { "type": "desktop_entry", "value": "org.gnome.Nautilus.desktop" }
      },
      "better_value": { "type": "desktop_entry", "value": "io.betteros.Files.desktop" },
      "applied_value": { "type": "desktop_entry", "value": "io.betteros.Files.desktop" },
      "last_verified_value": { "type": "desktop_entry", "value": "io.betteros.Files.desktop" },
      "restore_state": "available"
    }
  ]
}
"#;

#[test]
fn a_snapshot_written_before_per_type_capture_still_loads() {
    let directory = tempfile::tempdir().unwrap();
    let store = SnapshotStore::at_path(directory.path());
    std::fs::write(
        directory.path().join("00-version-1.json"),
        VERSION_1_SNAPSHOT,
    )
    .unwrap();

    let history = store.history().unwrap();

    assert!(history.damaged().is_empty(), "{:?}", history.damaged());
    assert_eq!(history.snapshots().len(), 1);
    assert_eq!(history.snapshots()[0].schema_version, 1);
    assert_eq!(
        history
            .latest_entry(
                &ComponentId::new("better-files").unwrap(),
                &IntegrationId::new("default-file-manager").unwrap()
            )
            .map(|entry| entry.previous_value.clone()),
        Some(nautilus())
    );
}

#[test]
fn a_group_captured_type_by_type_keeps_every_type_through_the_store() {
    use better_core::defaults::KeyObservation;

    let directory = tempfile::tempdir().unwrap();
    let store = SnapshotStore::at_path(directory.path());
    let per_type = ObservedValue::Mixed {
        per_key: vec![
            KeyObservation {
                key: "image/png".to_string(),
                observed: ObservedValue::Set {
                    value: DefaultsValue::DesktopEntry("org.gnome.eog.desktop".to_string()),
                },
            },
            KeyObservation {
                key: "image/jpeg".to_string(),
                observed: ObservedValue::Unset,
            },
        ],
    };
    let snapshot = Snapshot::new(
        identity(),
        vec![entry("better-files", "image-viewer", per_type.clone())],
    );
    // A per-type value is something a version 1 reader cannot parse, so the
    // file says it needs a newer reader rather than looking corrupt to one.
    assert_eq!(snapshot.schema_version, 2);
    assert_eq!(defaults_store::SNAPSHOT_SCHEMA_VERSION, 2);

    store.write(&snapshot).unwrap();
    let history = store.history().unwrap();

    assert_eq!(history.snapshots(), std::slice::from_ref(&snapshot));
    assert_eq!(history.snapshots()[0].entries[0].previous_value, per_type);
}
