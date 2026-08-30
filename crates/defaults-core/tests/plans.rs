//! Planning, applying, verifying, and restoring — including the round trip
//! that has to end on the exact value that was captured.

mod common;

use better_core::defaults::{ObservedValue, SessionEffect};
use common::*;
use defaults_core::{
    ComponentReadiness, Confirmations, DefaultsEngine, EntryOutcome, IntegrationState, PlanAction,
    PlanKind, PlanWarning, Selection, SkipReason, SystemContext,
};
use defaults_platform::{AdapterSet, MockBehavior};
use defaults_store::{RestoreState, SnapshotStore};

const FILES: &str = "io.betteros.Files.desktop";
const MONITOR: &str = "io.betteros.Monitor.desktop";
const NAUTILUS: &str = "org.gnome.Nautilus.desktop";
const GNOME_MONITOR: &str = "gnome-system-monitor.desktop";
const FILES_SLOT: &str = "better-files/default-file-manager";
const ARCHIVE_SLOT: &str = "better-files/archive-handler";
const MONITOR_SLOT: &str = "better-monitor/task-manager";

fn system() -> SystemContext {
    SystemContext::new("zorin", "gnome")
}

fn two_component_catalog() -> better_core::ComponentCatalog {
    catalog(vec![
        manifest(
            "better-files",
            vec![
                integration("default-file-manager", "inode/directory", FILES),
                integration("archive-handler", "application/zip", FILES),
            ],
        ),
        manifest(
            "better-monitor",
            vec![integration("task-manager", "application/x-task", MONITOR)],
        ),
    ])
}

fn ready(catalog: &better_core::ComponentCatalog) -> DefaultsEngine<'_> {
    DefaultsEngine::new(catalog, system())
        .with_readiness(component("better-files"), ComponentReadiness::ready())
        .with_readiness(component("better-monitor"), ComponentReadiness::ready())
}

#[test]
fn apply_captures_the_previous_value_before_it_changes_anything() {
    let directory = tempfile::tempdir().unwrap();
    let store = SnapshotStore::at_path(directory.path());
    let catalog = two_component_catalog();
    let engine = ready(&catalog);
    let mut adapters = adapters_with(xdg_adapter(&[
        (FILES_SLOT, set(NAUTILUS)),
        (ARCHIVE_SLOT, set(NAUTILUS)),
        (MONITOR_SLOT, set(GNOME_MONITOR)),
    ]));

    let plan = engine.plan_apply(
        &Selection::All,
        &adapters,
        &store.history().unwrap(),
        &Confirmations::none(),
    );
    assert_eq!(plan.kind, PlanKind::Apply);
    assert_eq!(plan.changes().count(), 3);

    let outcome = engine.execute(&plan, &mut adapters, &store).unwrap();

    let baseline_id = outcome.baseline_snapshot.expect("a baseline was captured");
    let history = store.history().unwrap();
    let baseline = history
        .snapshots()
        .iter()
        .find(|snapshot| snapshot.snapshot_id.as_str() == baseline_id)
        .expect("the baseline is in the history");
    // The baseline holds what the desktop said, not what Better OS wrote.
    assert_eq!(
        baseline
            .entry(
                &component("better-files"),
                &integration_id("default-file-manager")
            )
            .unwrap()
            .previous_value,
        set(NAUTILUS)
    );
    assert!(
        baseline
            .entry(
                &component("better-files"),
                &integration_id("default-file-manager")
            )
            .unwrap()
            .applied_value
            .is_none()
    );
}

#[test]
fn a_full_round_trip_restores_the_captured_value_exactly() {
    let directory = tempfile::tempdir().unwrap();
    let store = SnapshotStore::at_path(directory.path());
    let catalog = two_component_catalog();
    let engine = ready(&catalog);
    let mut adapters = adapters_with(xdg_adapter(&[
        (FILES_SLOT, set(NAUTILUS)),
        (ARCHIVE_SLOT, set(NAUTILUS)),
        (MONITOR_SLOT, set(GNOME_MONITOR)),
    ]));

    let apply = engine.plan_apply(
        &Selection::All,
        &adapters,
        &store.history().unwrap(),
        &Confirmations::none(),
    );
    let applied = engine.execute(&apply, &mut adapters, &store).unwrap();
    assert_eq!(applied.succeeded(), 3);
    assert!(!applied.has_failures());

    let after_apply = engine.inspect(&Selection::All, &adapters, &store.history().unwrap());
    assert_eq!(
        after_apply.components[0].aggregate,
        defaults_core::AggregateState::Default
    );

    let restore = engine.plan_restore(
        &Selection::All,
        &adapters,
        &store.history().unwrap(),
        &Confirmations::none(),
    );
    assert_eq!(restore.kind, PlanKind::Restore);
    assert_eq!(restore.changes().count(), 3);
    let restored = engine.execute(&restore, &mut adapters, &store).unwrap();

    assert_eq!(restored.succeeded(), 3);
    let final_report = engine.inspect(&Selection::All, &adapters, &store.history().unwrap());
    // Back to exactly the value that was captured, not to a built-in guess.
    assert_eq!(
        final_report.components[0].integrations[0].current,
        set(NAUTILUS)
    );
    assert_eq!(
        final_report.components[1].integrations[0].current,
        set(GNOME_MONITOR)
    );
}

#[test]
fn restoring_one_component_leaves_another_components_change_in_place() {
    let directory = tempfile::tempdir().unwrap();
    let store = SnapshotStore::at_path(directory.path());
    let catalog = two_component_catalog();
    let engine = ready(&catalog);
    let mut adapters = adapters_with(xdg_adapter(&[
        (FILES_SLOT, set(NAUTILUS)),
        (ARCHIVE_SLOT, set(NAUTILUS)),
        (MONITOR_SLOT, set(GNOME_MONITOR)),
    ]));

    let apply = engine.plan_apply(
        &Selection::All,
        &adapters,
        &store.history().unwrap(),
        &Confirmations::none(),
    );
    engine.execute(&apply, &mut adapters, &store).unwrap();

    let restore = engine.plan_restore(
        &Selection::one(component("better-files")),
        &adapters,
        &store.history().unwrap(),
        &Confirmations::none(),
    );
    assert!(
        restore
            .entries
            .iter()
            .all(|entry| entry.component == component("better-files"))
    );
    engine.execute(&restore, &mut adapters, &store).unwrap();

    let report = engine.inspect(&Selection::All, &adapters, &store.history().unwrap());
    assert_eq!(report.components[0].integrations[0].current, set(NAUTILUS));
    // Better Monitor was never in the plan, so its successful change stands and
    // its snapshot entry still says it is applied.
    assert_eq!(report.components[1].integrations[0].current, set(MONITOR));
    assert_eq!(
        report.components[1].integrations[0].state,
        IntegrationState::Default
    );
    let history = store.history().unwrap();
    assert_eq!(
        history
            .latest_entry(
                &component("better-monitor"),
                &integration_id("task-manager")
            )
            .unwrap()
            .restore_state,
        RestoreState::Available
    );
}

#[test]
fn a_failing_entry_produces_its_own_result_and_the_others_still_succeed() {
    let directory = tempfile::tempdir().unwrap();
    let store = SnapshotStore::at_path(directory.path());
    let catalog = two_component_catalog();
    let engine = ready(&catalog);
    let mut adapter = xdg_adapter(&[
        (FILES_SLOT, set(NAUTILUS)),
        (ARCHIVE_SLOT, set(NAUTILUS)),
        (MONITOR_SLOT, set(GNOME_MONITOR)),
    ]);
    adapter.set_behavior(
        ARCHIVE_SLOT,
        MockBehavior::Fail {
            reason: "test.write_refused_by_the_filesystem".to_string(),
            detail: Some("read-only".to_string()),
        },
    );
    let mut adapters = adapters_with(adapter);

    let plan = engine.plan_apply(
        &Selection::All,
        &adapters,
        &store.history().unwrap(),
        &Confirmations::none(),
    );
    let outcome = engine.execute(&plan, &mut adapters, &store).unwrap();

    assert!(outcome.has_failures());
    assert_eq!(outcome.succeeded(), 2);
    let failed = outcome
        .results
        .iter()
        .find(|result| result.integration == integration_id("archive-handler"))
        .unwrap();
    assert!(matches!(
        &failed.outcome,
        EntryOutcome::Failed { reason, .. } if reason == "test.write_refused_by_the_filesystem"
    ));
    // The failed entry keeps its captured baseline and gains no applied value,
    // so a later run still knows what to put back.
    let history = store.history().unwrap();
    let record = history
        .latest_entry(
            &component("better-files"),
            &integration_id("archive-handler"),
        )
        .unwrap();
    assert_eq!(record.previous_value, set(NAUTILUS));
    assert_eq!(record.applied_value, None);
}

#[test]
fn a_write_that_reports_success_and_does_not_take_is_not_verified() {
    let directory = tempfile::tempdir().unwrap();
    let store = SnapshotStore::at_path(directory.path());
    let catalog = catalog(vec![manifest(
        "better-files",
        vec![integration(
            "default-file-manager",
            "inode/directory",
            FILES,
        )],
    )]);
    let engine = DefaultsEngine::new(&catalog, system())
        .with_readiness(component("better-files"), ComponentReadiness::ready());
    let mut adapter = xdg_adapter(&[(FILES_SLOT, set(NAUTILUS))]);
    adapter.set_behavior(FILES_SLOT, MockBehavior::AcceptWithoutEffect);
    let mut adapters = adapters_with(adapter);

    let plan = engine.plan_apply(
        &Selection::All,
        &adapters,
        &store.history().unwrap(),
        &Confirmations::none(),
    );
    let outcome = engine.execute(&plan, &mut adapters, &store).unwrap();

    assert!(matches!(
        outcome.results[0].outcome,
        EntryOutcome::NotVerified { .. }
    ));
    assert_eq!(outcome.succeeded(), 0);
    // Nothing was recorded as owned, because nothing was observed to be owned.
    assert_eq!(
        store
            .history()
            .unwrap()
            .latest_entry(
                &component("better-files"),
                &integration_id("default-file-manager")
            )
            .unwrap()
            .applied_value,
        None
    );
}

#[test]
fn an_adapter_that_refuses_reports_manual_action_and_executes_nothing() {
    let directory = tempfile::tempdir().unwrap();
    let store = SnapshotStore::at_path(directory.path());
    let catalog = catalog(vec![manifest(
        "better-files",
        vec![integration(
            "default-file-manager",
            "inode/directory",
            FILES,
        )],
    )]);
    let engine = DefaultsEngine::new(&catalog, system())
        .with_readiness(component("better-files"), ComponentReadiness::ready());
    let mut adapter = xdg_adapter(&[(FILES_SLOT, set(NAUTILUS))]);
    adapter.set_behavior(
        FILES_SLOT,
        MockBehavior::Refuse {
            reason: "test.no_typed_write_path".to_string(),
            detail: Some("change it in Settings".to_string()),
        },
    );
    let mut adapters = adapters_with(adapter);

    let plan = engine.plan_apply(
        &Selection::All,
        &adapters,
        &store.history().unwrap(),
        &Confirmations::none(),
    );
    let outcome = engine.execute(&plan, &mut adapters, &store).unwrap();

    assert!(matches!(
        &outcome.results[0].outcome,
        EntryOutcome::ManualActionRequired { reason, .. } if reason == "test.no_typed_write_path"
    ));
    let report = engine.inspect(&Selection::All, &adapters, &store.history().unwrap());
    assert_eq!(report.components[0].integrations[0].current, set(NAUTILUS));
}

#[test]
fn an_integration_kind_with_no_adapter_at_all_is_skipped_and_executes_nothing() {
    let directory = tempfile::tempdir().unwrap();
    let store = SnapshotStore::at_path(directory.path());
    let catalog = catalog(vec![manifest(
        "better-files",
        vec![integration(
            "default-file-manager",
            "inode/directory",
            FILES,
        )],
    )]);
    let engine = DefaultsEngine::new(&catalog, system())
        .with_readiness(component("better-files"), ComponentReadiness::ready());
    let mut adapters = AdapterSet::new();

    let plan = engine.plan_apply(
        &Selection::All,
        &adapters,
        &store.history().unwrap(),
        &Confirmations::none(),
    );
    assert!(matches!(
        plan.entries[0].action,
        PlanAction::Skip {
            reason: SkipReason::NoProductionAdapter { .. }
        }
    ));

    let outcome = engine.execute(&plan, &mut adapters, &store).unwrap();
    assert!(matches!(
        outcome.results[0].outcome,
        EntryOutcome::Skipped {
            reason: SkipReason::NoProductionAdapter { .. }
        }
    ));
    assert!(store.history().unwrap().snapshots().is_empty());
}

#[test]
fn planning_one_component_never_touches_another() {
    let directory = tempfile::tempdir().unwrap();
    let store = SnapshotStore::at_path(directory.path());
    let catalog = two_component_catalog();
    let engine = ready(&catalog);
    let adapters = adapters_with(xdg_adapter(&[
        (FILES_SLOT, set(NAUTILUS)),
        (ARCHIVE_SLOT, set(NAUTILUS)),
        (MONITOR_SLOT, set(GNOME_MONITOR)),
    ]));

    let plan = engine.plan_apply(
        &Selection::one(component("better-monitor")),
        &adapters,
        &store.history().unwrap(),
        &Confirmations::none(),
    );

    assert_eq!(plan.entries.len(), 1);
    assert_eq!(plan.entries[0].component, component("better-monitor"));
}

#[test]
fn a_plan_serializes_with_everything_a_diagnostic_needs() {
    let directory = tempfile::tempdir().unwrap();
    let store = SnapshotStore::at_path(directory.path());
    let catalog = two_component_catalog();
    let engine = ready(&catalog);
    let adapters = adapters_with(xdg_adapter(&[
        (FILES_SLOT, set(NAUTILUS)),
        (ARCHIVE_SLOT, set(NAUTILUS)),
        (MONITOR_SLOT, set(GNOME_MONITOR)),
    ]));

    let plan = engine.plan_apply(
        &Selection::All,
        &adapters,
        &store.history().unwrap(),
        &Confirmations::none(),
    );
    let json = serde_json::to_string(&plan).unwrap();
    let restored: defaults_core::DefaultsPlan = serde_json::from_str(&json).unwrap();

    assert_eq!(restored, plan);
    assert!(json.contains("\"schema_version\":1"));
}

#[test]
fn a_change_that_needs_sign_out_says_so_in_the_plan_and_in_the_result() {
    let directory = tempfile::tempdir().unwrap();
    let store = SnapshotStore::at_path(directory.path());
    let mut declaration = integration("default-file-manager", "inode/directory", FILES);
    declaration.session_effect = SessionEffect::SignOut;
    let catalog = catalog(vec![manifest("better-files", vec![declaration])]);
    let engine = DefaultsEngine::new(&catalog, system())
        .with_readiness(component("better-files"), ComponentReadiness::ready());
    let mut adapter = xdg_adapter(&[(FILES_SLOT, set(NAUTILUS))]);
    adapter.set_behavior(FILES_SLOT, MockBehavior::AcceptWithoutEffect);
    let mut adapters = adapters_with(adapter);

    let plan = engine.plan_apply(
        &Selection::All,
        &adapters,
        &store.history().unwrap(),
        &Confirmations::none(),
    );
    assert!(
        plan.entries[0]
            .warnings
            .contains(&PlanWarning::NeedsSignOut)
    );

    let outcome = engine.execute(&plan, &mut adapters, &store).unwrap();
    assert!(matches!(
        outcome.results[0].outcome,
        EntryOutcome::AppliedNeedsSignOut { .. }
    ));
}

#[test]
fn verifying_records_what_it_saw_so_the_next_comparison_is_against_an_observation() {
    let directory = tempfile::tempdir().unwrap();
    let store = SnapshotStore::at_path(directory.path());
    let catalog = catalog(vec![manifest(
        "better-files",
        vec![integration(
            "default-file-manager",
            "inode/directory",
            FILES,
        )],
    )]);
    let engine = DefaultsEngine::new(&catalog, system())
        .with_readiness(component("better-files"), ComponentReadiness::ready());
    let mut adapters = adapters_with(xdg_adapter(&[(FILES_SLOT, set(NAUTILUS))]));

    let plan = engine.plan_apply(
        &Selection::All,
        &adapters,
        &store.history().unwrap(),
        &Confirmations::none(),
    );
    engine.execute(&plan, &mut adapters, &store).unwrap();

    let report = engine.verify(&Selection::All, &adapters, &store).unwrap();
    assert_eq!(
        report.components[0].integrations[0].state,
        IntegrationState::Default
    );
    assert_eq!(
        store
            .history()
            .unwrap()
            .latest_entry(
                &component("better-files"),
                &integration_id("default-file-manager")
            )
            .unwrap()
            .last_verified_value,
        Some(desktop(FILES))
    );
}

#[test]
fn applying_a_second_time_changes_nothing_and_says_so() {
    let directory = tempfile::tempdir().unwrap();
    let store = SnapshotStore::at_path(directory.path());
    let catalog = catalog(vec![manifest(
        "better-files",
        vec![integration(
            "default-file-manager",
            "inode/directory",
            FILES,
        )],
    )]);
    let engine = DefaultsEngine::new(&catalog, system())
        .with_readiness(component("better-files"), ComponentReadiness::ready());
    let mut adapters = adapters_with(xdg_adapter(&[(FILES_SLOT, set(NAUTILUS))]));

    let first = engine.plan_apply(
        &Selection::All,
        &adapters,
        &store.history().unwrap(),
        &Confirmations::none(),
    );
    engine.execute(&first, &mut adapters, &store).unwrap();

    let second = engine.plan_apply(
        &Selection::All,
        &adapters,
        &store.history().unwrap(),
        &Confirmations::none(),
    );
    assert!(second.is_empty());
    assert!(matches!(
        second.entries[0].action,
        PlanAction::Skip {
            reason: SkipReason::AlreadyDefault
        }
    ));
}

#[test]
fn a_setting_with_nothing_in_it_is_captured_as_unset_and_restored_to_unset() {
    let directory = tempfile::tempdir().unwrap();
    let store = SnapshotStore::at_path(directory.path());
    let catalog = catalog(vec![manifest(
        "better-files",
        vec![integration(
            "default-file-manager",
            "inode/directory",
            FILES,
        )],
    )]);
    let engine = DefaultsEngine::new(&catalog, system())
        .with_readiness(component("better-files"), ComponentReadiness::ready());
    // Nothing is preset, so the adapter reports the setting holds nothing.
    let mut adapters = adapters_with(xdg_adapter(&[]));

    let apply = engine.plan_apply(
        &Selection::All,
        &adapters,
        &store.history().unwrap(),
        &Confirmations::none(),
    );
    engine.execute(&apply, &mut adapters, &store).unwrap();

    let restore = engine.plan_restore(
        &Selection::All,
        &adapters,
        &store.history().unwrap(),
        &Confirmations::none(),
    );
    assert!(matches!(
        restore.entries[0].action,
        PlanAction::Restore {
            to: ObservedValue::Unset
        }
    ));
    let outcome = engine.execute(&restore, &mut adapters, &store).unwrap();
    assert_eq!(outcome.succeeded(), 1);

    let report = engine.inspect(&Selection::All, &adapters, &store.history().unwrap());
    assert_eq!(
        report.components[0].integrations[0].current,
        ObservedValue::Unset
    );
}
