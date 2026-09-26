//! A handler group whose types open in different applications.
//!
//! These run the production `mimeapps.list` adapter over a file in a temporary
//! directory, because the point is what ends up in that file: each type back at
//! its own previous owner, not the whole group flattened onto one of them.

mod common;

use app_chooser_core::AssociationStore;
use better_core::defaults::{AdapterId, IntegrationKind, KeyObservation, ObservedValue};
use common::*;
use defaults_core::{
    ComponentReadiness, Confirmations, DefaultsEngine, DefaultsOutcome, EntryOutcome,
    IntegrationState, KeyOutcome, PlanAction, PlanWarning, Selection, SkipReason, SystemContext,
};
use defaults_platform::{AdapterSet, XdgDefaultAppAdapter};
use defaults_store::{RestoreState, SnapshotStore};

const FILES: &str = "io.betteros.Files.desktop";
const EOG: &str = "org.gnome.eog.desktop";
const GTHUMB: &str = "org.gnome.gThumb.desktop";
const NAUTILUS: &str = "org.gnome.Nautilus.desktop";

const TWO_VIEWERS: &str = "[Default Applications]\n\
    image/png=org.gnome.eog.desktop\n\
    image/jpeg=org.gnome.gThumb.desktop\n";

struct Desktop {
    directory: tempfile::TempDir,
    catalog: better_core::ComponentCatalog,
    store: SnapshotStore,
    adapters: AdapterSet,
}

impl Desktop {
    fn new(mimeapps: &str, types: &[&str]) -> Self {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("mimeapps.list"), mimeapps).unwrap();
        let mut group = integration("image-viewer", types[0], FILES);
        group.kind = IntegrationKind::MimeUriHandlerGroup;
        group.target.keys = types.iter().map(|value| value.to_string()).collect();
        let mut adapters = AdapterSet::in_memory();
        adapters.insert(Box::new(XdgDefaultAppAdapter::new(
            AdapterId::XdgDefaultApp,
            AssociationStore::new(
                directory.path().join("mimeapps.list"),
                directory.path().join("rollback"),
            ),
        )));
        Self {
            store: SnapshotStore::at_path(directory.path().join("snapshots")),
            catalog: catalog(vec![manifest("better-files", vec![group])]),
            adapters,
            directory,
        }
    }

    fn file(&self) -> String {
        std::fs::read_to_string(self.directory.path().join("mimeapps.list")).unwrap()
    }

    fn status(&self) -> defaults_core::IntegrationStatus {
        let history = self.store.history().unwrap();
        engine(&self.catalog)
            .inspect(&Selection::All, &self.adapters, &history)
            .component(&component("better-files"))
            .unwrap()
            .integrations[0]
            .clone()
    }

    fn plan(&self, restore: bool) -> defaults_core::DefaultsPlan {
        let history = self.store.history().unwrap();
        let engine = engine(&self.catalog);
        if restore {
            engine.plan_restore(
                &Selection::All,
                &self.adapters,
                &history,
                &Confirmations::none(),
            )
        } else {
            engine.plan_apply(
                &Selection::All,
                &self.adapters,
                &history,
                &Confirmations::none(),
            )
        }
    }

    fn run(&mut self, restore: bool) -> DefaultsOutcome {
        let plan = self.plan(restore);
        engine(&self.catalog)
            .execute(&plan, &mut self.adapters, &self.store)
            .unwrap()
    }
}

fn engine(catalog: &better_core::ComponentCatalog) -> DefaultsEngine<'_> {
    DefaultsEngine::new(catalog, SystemContext::new("zorin", "gnome"))
        .with_readiness(component("better-files"), ComponentReadiness::ready())
}

fn per_type(pairs: &[(&str, ObservedValue)]) -> ObservedValue {
    ObservedValue::Mixed {
        per_key: pairs
            .iter()
            .map(|(key, observed)| KeyObservation {
                key: key.to_string(),
                observed: observed.clone(),
            })
            .collect(),
    }
}

#[test]
fn a_mixed_group_can_be_applied_after_a_preview_that_names_each_owner() {
    let desktop = Desktop::new(TWO_VIEWERS, &["image/png", "image/jpeg"]);
    let before = per_type(&[("image/png", set(EOG)), ("image/jpeg", set(GTHUMB))]);

    let status = desktop.status();
    assert_eq!(status.state, IntegrationState::NotDefault);
    assert_eq!(status.current, before);

    let plan = desktop.plan(false);
    let entry = &plan.entries[0];
    assert_eq!(
        entry.action,
        PlanAction::Apply {
            to: desktop_value()
        }
    );
    assert_eq!(entry.current, before, "the preview lists each type's owner");
    assert!(
        !entry
            .warnings
            .contains(&PlanWarning::PreviousValueIndeterminate),
        "each type's owner is known, so the change can be undone"
    );
}

fn desktop_value() -> better_core::DefaultsValue {
    desktop(FILES)
}

#[test]
fn restore_puts_each_type_back_to_its_own_previous_owner() {
    let mut desktop = Desktop::new(TWO_VIEWERS, &["image/png", "image/jpeg"]);
    let before = per_type(&[("image/png", set(EOG)), ("image/jpeg", set(GTHUMB))]);

    let applied = desktop.run(false);
    assert_eq!(
        applied.results[0].outcome,
        EntryOutcome::Applied {
            value: desktop_value()
        }
    );
    assert!(
        desktop
            .file()
            .contains("image/png=io.betteros.Files.desktop")
    );
    assert!(
        desktop
            .file()
            .contains("image/jpeg=io.betteros.Files.desktop")
    );
    let history = desktop.store.history().unwrap();
    let record = history
        .latest_entry(&component("better-files"), &integration_id("image-viewer"))
        .unwrap();
    assert_eq!(record.previous_value, before);
    assert_eq!(record.restore_state, RestoreState::Available);
    assert!(desktop.status().restore_available);

    assert_eq!(
        desktop.plan(true).entries[0].action,
        PlanAction::Restore { to: before.clone() }
    );
    let restored = desktop.run(true);
    assert_eq!(
        restored.results[0].outcome,
        EntryOutcome::PerKey {
            keys: vec![
                KeyOutcome {
                    key: "image/png".to_string(),
                    outcome: EntryOutcome::Restored { value: set(EOG) },
                },
                KeyOutcome {
                    key: "image/jpeg".to_string(),
                    outcome: EntryOutcome::Restored { value: set(GTHUMB) },
                },
            ]
        }
    );
    assert!(restored.results[0].outcome.is_success());
    assert!(!restored.has_failures());
    assert!(desktop.file().contains("image/png=org.gnome.eog.desktop"));
    assert!(
        desktop
            .file()
            .contains("image/jpeg=org.gnome.gThumb.desktop")
    );

    // The group is back where it started, and a second restore has nothing to
    // do rather than rewriting it.
    assert_eq!(desktop.status().current, before);
    assert_eq!(
        desktop.plan(true).entries[0].action,
        PlanAction::Skip {
            reason: SkipReason::AlreadyRestored
        }
    );
}

#[test]
fn a_type_that_had_no_owner_needs_manual_action_and_the_rest_are_still_restored() {
    let mut desktop = Desktop::new(TWO_VIEWERS, &["image/png", "image/webp"]);
    assert_eq!(
        desktop.status().current,
        per_type(&[
            ("image/png", set(EOG)),
            ("image/webp", ObservedValue::Unset)
        ])
    );

    assert!(desktop.run(false).results[0].outcome.is_success());
    let restored = desktop.run(true);

    let EntryOutcome::PerKey { keys } = &restored.results[0].outcome else {
        panic!(
            "expected a per-type result, got {:?}",
            restored.results[0].outcome
        );
    };
    assert_eq!(keys.len(), 2);
    assert_eq!(keys[0].key, "image/png");
    assert_eq!(keys[0].outcome, EntryOutcome::Restored { value: set(EOG) });
    assert_eq!(keys[1].key, "image/webp");
    assert!(matches!(
        &keys[1].outcome,
        EntryOutcome::ManualActionRequired { reason, .. }
            if reason == "xdg.clearing_a_default_is_not_supported"
    ));
    assert!(!restored.results[0].outcome.is_success());
    assert!(!restored.has_failures(), "manual action is not a failure");
    assert!(desktop.file().contains("image/png=org.gnome.eog.desktop"));
    assert!(
        desktop
            .file()
            .contains("image/webp=io.betteros.Files.desktop")
    );

    // Better Manager no longer claims the group, so the type still pointing at
    // Better Files is not read as somebody else's change.
    assert_eq!(desktop.status().state, IntegrationState::NotDefault);
}

#[test]
fn a_snapshot_written_before_per_type_capture_restores_as_before() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("mimeapps.list"),
        "[Default Applications]\ninode/directory=io.betteros.Files.desktop\n",
    )
    .unwrap();
    let snapshots = directory.path().join("snapshots");
    std::fs::create_dir_all(&snapshots).unwrap();
    std::fs::write(
        snapshots.join("00000000001790000000-4242-000000000.json"),
        r#"{
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
"#,
    )
    .unwrap();
    let catalog = catalog(vec![manifest(
        "better-files",
        vec![integration(
            "default-file-manager",
            "inode/directory",
            FILES,
        )],
    )]);
    let store = SnapshotStore::at_path(&snapshots);
    let mut adapters = AdapterSet::in_memory();
    adapters.insert(Box::new(XdgDefaultAppAdapter::new(
        AdapterId::XdgDefaultApp,
        AssociationStore::new(
            directory.path().join("mimeapps.list"),
            directory.path().join("rollback"),
        ),
    )));

    let history = store.history().unwrap();
    assert!(history.damaged().is_empty());
    let plan =
        engine(&catalog).plan_restore(&Selection::All, &adapters, &history, &Confirmations::none());
    assert_eq!(
        plan.entries[0].action,
        PlanAction::Restore { to: set(NAUTILUS) }
    );
    let outcome = engine(&catalog)
        .execute(&plan, &mut adapters, &store)
        .unwrap();

    assert_eq!(
        outcome.results[0].outcome,
        EntryOutcome::Restored {
            value: set(NAUTILUS)
        }
    );
    assert!(
        std::fs::read_to_string(directory.path().join("mimeapps.list"))
            .unwrap()
            .contains("inode/directory=org.gnome.Nautilus.desktop")
    );
}
