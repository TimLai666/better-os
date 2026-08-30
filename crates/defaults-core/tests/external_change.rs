//! External-change detection, across every combination of what Better Manager
//! knows and what the system now says.
//!
//! The matrix is the point. "Changed externally" only means something if the
//! two cases beside it — never touched, and applied by us and still ours —
//! reliably do not trip it.

mod common;

use common::*;
use defaults_core::{
    ComponentReadiness, Confirmations, DefaultsEngine, IntegrationState, PlanAction, Selection,
    SkipReason, SystemContext,
};
use defaults_platform::AdapterSet;
use defaults_store::{RestoreState, SnapshotEntry, SnapshotStore};

const FILES: &str = "io.betteros.Files.desktop";
const NAUTILUS: &str = "org.gnome.Nautilus.desktop";
const DOLPHIN: &str = "org.kde.dolphin.desktop";
const SLOT: &str = "better-files/default-file-manager";

struct Fixture {
    _directory: tempfile::TempDir,
    store: SnapshotStore,
    catalog: better_core::ComponentCatalog,
    adapters: AdapterSet,
}

fn fixture(now: &str, history: Vec<SnapshotEntry>) -> Fixture {
    let directory = tempfile::tempdir().unwrap();
    let store = store_with(directory.path(), history);
    Fixture {
        _directory: directory,
        store,
        catalog: catalog(vec![manifest(
            "better-files",
            vec![integration(
                "default-file-manager",
                "inode/directory",
                FILES,
            )],
        )]),
        adapters: adapters_with(xdg_adapter(&[(SLOT, set(now))])),
    }
}

impl Fixture {
    fn engine(&self) -> DefaultsEngine<'_> {
        DefaultsEngine::new(&self.catalog, SystemContext::new("zorin", "gnome"))
            .with_readiness(component("better-files"), ComponentReadiness::ready())
    }

    fn state(&self) -> IntegrationState {
        let history = self.store.history().unwrap();
        self.engine()
            .inspect(&Selection::All, &self.adapters, &history)
            .component(&component("better-files"))
            .unwrap()
            .integrations[0]
            .state
            .clone()
    }
}

#[test]
fn a_setting_better_manager_never_touched_is_not_an_external_change() {
    // Nobody recorded anything, so there is nothing this value could have
    // drifted away from. It is simply not the default yet.
    assert_eq!(
        fixture(NAUTILUS, Vec::new()).state(),
        IntegrationState::NotDefault
    );
}

#[test]
fn a_setting_better_manager_applied_and_still_owns_is_default() {
    let fixture = fixture(
        FILES,
        vec![applied_entry(
            "better-files",
            "default-file-manager",
            set(NAUTILUS),
            FILES,
        )],
    );
    assert_eq!(fixture.state(), IntegrationState::Default);
}

#[test]
fn a_setting_that_moved_after_better_manager_applied_it_is_changed_externally() {
    let fixture = fixture(
        DOLPHIN,
        vec![applied_entry(
            "better-files",
            "default-file-manager",
            set(NAUTILUS),
            FILES,
        )],
    );
    assert_eq!(
        fixture.state(),
        IntegrationState::ChangedExternally {
            last_known: Some(desktop(FILES))
        }
    );
}

#[test]
fn a_setting_that_moved_back_to_the_captured_value_is_still_an_external_change() {
    // Better Manager last wrote Better Files. Something put the old value back.
    // That is somebody's choice, not a restore Better Manager performed, and it
    // must not be silently overwritten either.
    let fixture = fixture(
        NAUTILUS,
        vec![applied_entry(
            "better-files",
            "default-file-manager",
            set(NAUTILUS),
            FILES,
        )],
    );
    assert!(matches!(
        fixture.state(),
        IntegrationState::ChangedExternally { .. }
    ));
}

#[test]
fn an_unreadable_value_is_never_treated_as_an_external_change() {
    let directory = tempfile::tempdir().unwrap();
    let store = store_with(
        directory.path(),
        vec![applied_entry(
            "better-files",
            "default-file-manager",
            set(NAUTILUS),
            FILES,
        )],
    );
    let catalog = catalog(vec![manifest(
        "better-files",
        vec![integration(
            "default-file-manager",
            "inode/directory",
            FILES,
        )],
    )]);
    let mut adapter = xdg_adapter(&[]);
    adapter.preset(
        SLOT,
        better_core::ObservedValue::Unknown {
            reason: "test.cannot_tell".to_string(),
        },
    );
    let adapters = adapters_with(adapter);
    let engine = DefaultsEngine::new(&catalog, SystemContext::new("zorin", "gnome"))
        .with_readiness(component("better-files"), ComponentReadiness::ready());

    let report = engine.inspect(&Selection::All, &adapters, &store.history().unwrap());

    assert_eq!(
        report.components[0].integrations[0].state,
        IntegrationState::Unknown {
            reason: "test.cannot_tell".to_string()
        }
    );
}

#[test]
fn an_external_change_is_skipped_by_apply_until_that_entry_is_confirmed() {
    let mut fixture = fixture(
        DOLPHIN,
        vec![applied_entry(
            "better-files",
            "default-file-manager",
            set(NAUTILUS),
            FILES,
        )],
    );
    let history = fixture.store.history().unwrap();
    let plan = fixture.engine().plan_apply(
        &Selection::All,
        &fixture.adapters,
        &history,
        &Confirmations::none(),
    );

    assert!(plan.is_empty());
    assert!(matches!(
        plan.entries[0].action,
        PlanAction::Skip {
            reason: SkipReason::ChangedExternallyWithoutConfirmation { .. }
        }
    ));
    assert!(plan.entries[0].requires_confirmation);
    assert_eq!(plan.awaiting_confirmation().count(), 1);

    // Running the unconfirmed plan writes nothing.
    let engine = DefaultsEngine::new(&fixture.catalog, SystemContext::new("zorin", "gnome"))
        .with_readiness(component("better-files"), ComponentReadiness::ready());
    let outcome = engine
        .execute(&plan, &mut fixture.adapters, &fixture.store)
        .unwrap();
    assert!(outcome.baseline_snapshot.is_none());
    assert!(outcome.recorded_snapshot.is_none());
}

#[test]
fn an_external_change_is_skipped_by_restore_until_that_entry_is_confirmed() {
    let fixture = fixture(
        DOLPHIN,
        vec![applied_entry(
            "better-files",
            "default-file-manager",
            set(NAUTILUS),
            FILES,
        )],
    );
    let history = fixture.store.history().unwrap();
    let plan = fixture.engine().plan_restore(
        &Selection::All,
        &fixture.adapters,
        &history,
        &Confirmations::none(),
    );

    assert!(plan.is_empty());
    assert!(matches!(
        plan.entries[0].action,
        PlanAction::Skip {
            reason: SkipReason::ChangedExternallyWithoutConfirmation { .. }
        }
    ));
}

#[test]
fn confirming_that_one_entry_lets_it_through_and_says_so_in_the_plan() {
    let fixture = fixture(
        DOLPHIN,
        vec![applied_entry(
            "better-files",
            "default-file-manager",
            set(NAUTILUS),
            FILES,
        )],
    );
    let history = fixture.store.history().unwrap();
    let confirmations = Confirmations::none().with(
        component("better-files"),
        integration_id("default-file-manager"),
    );
    let plan =
        fixture
            .engine()
            .plan_apply(&Selection::All, &fixture.adapters, &history, &confirmations);

    assert!(matches!(plan.entries[0].action, PlanAction::Apply { .. }));
    assert!(plan.entries[0].confirmed);
    assert!(plan.entries[0].warnings.iter().any(|warning| matches!(
        warning,
        defaults_core::PlanWarning::OverwritesExternalChange { .. }
    )));
}

#[test]
fn confirming_one_entry_does_not_confirm_another() {
    let directory = tempfile::tempdir().unwrap();
    let store = store_with(
        directory.path(),
        vec![
            applied_entry("better-files", "default-file-manager", set(NAUTILUS), FILES),
            applied_entry("better-files", "archive-handler", set(NAUTILUS), FILES),
        ],
    );
    let catalog = catalog(vec![manifest(
        "better-files",
        vec![
            integration("default-file-manager", "inode/directory", FILES),
            integration("archive-handler", "application/zip", FILES),
        ],
    )]);
    let adapters = adapters_with(xdg_adapter(&[
        (SLOT, set(DOLPHIN)),
        ("better-files/archive-handler", set(DOLPHIN)),
    ]));
    let engine = DefaultsEngine::new(&catalog, SystemContext::new("zorin", "gnome"))
        .with_readiness(component("better-files"), ComponentReadiness::ready());

    let plan = engine.plan_apply(
        &Selection::All,
        &adapters,
        &store.history().unwrap(),
        &Confirmations::none().with(
            component("better-files"),
            integration_id("default-file-manager"),
        ),
    );

    assert!(matches!(plan.entries[0].action, PlanAction::Apply { .. }));
    assert!(matches!(
        plan.entries[1].action,
        PlanAction::Skip {
            reason: SkipReason::ChangedExternallyWithoutConfirmation { .. }
        }
    ));
}

#[test]
fn a_restore_that_cannot_reproduce_the_captured_value_says_so_instead_of_guessing() {
    let directory = tempfile::tempdir().unwrap();
    let store = store_with(
        directory.path(),
        vec![SnapshotEntry {
            previous_value: better_core::ObservedValue::Unknown {
                reason: "test.never_read_definitely".to_string(),
            },
            restore_state: RestoreState::NotCaptured,
            ..applied_entry("better-files", "default-file-manager", set(NAUTILUS), FILES)
        }],
    );
    let catalog = catalog(vec![manifest(
        "better-files",
        vec![integration(
            "default-file-manager",
            "inode/directory",
            FILES,
        )],
    )]);
    let adapters = adapters_with(xdg_adapter(&[(SLOT, set(FILES))]));
    let engine = DefaultsEngine::new(&catalog, SystemContext::new("zorin", "gnome"))
        .with_readiness(component("better-files"), ComponentReadiness::ready());

    let plan = engine.plan_restore(
        &Selection::All,
        &adapters,
        &store.history().unwrap(),
        &Confirmations::none(),
    );

    assert!(matches!(
        plan.entries[0].action,
        PlanAction::Skip {
            reason: SkipReason::NothingCaptured
        }
    ));
}
