//! Restore when the application it would put back has been uninstalled.
//!
//! These run the production `mimeapps.list` adapter over a file in a temporary
//! directory and check restores against a catalog the test controls, because
//! the point is what does and does not end up in that file.

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use app_chooser_core::AssociationStore;
use better_core::defaults::{AdapterId, IntegrationKind, KeyObservation, ObservedValue};
use common::*;
use defaults_core::{
    ComponentReadiness, Confirmations, DefaultsEngine, DefaultsOutcome, DefaultsPlan, EntryOutcome,
    KeyOutcome, PlanAction, PlanWarning, RestoreChoice, Selection, SkipReason, SystemContext,
};
use defaults_platform::{
    AdapterSet, ApplicationChoice, InstalledApplications, XdgDefaultAppAdapter,
};
use defaults_store::{RestoreState, SnapshotStore};

const FILES: &str = "io.betteros.Files.desktop";
const NAUTILUS: &str = "org.gnome.Nautilus.desktop";
const DOLPHIN: &str = "org.kde.dolphin.desktop";
const EOG: &str = "org.gnome.eog.desktop";
const GTHUMB: &str = "org.gnome.gThumb.desktop";

/// An installed application: desktop ID, name, and the types it declares.
type App = (&'static str, &'static str, Vec<&'static str>);

/// A catalog the test edits while the engine holds it.
#[derive(Clone, Default)]
struct Installed(Rc<RefCell<Vec<App>>>);

impl Installed {
    fn new(apps: &[(&'static str, &'static str, &[&'static str])]) -> Self {
        Self(Rc::new(RefCell::new(
            apps.iter()
                .map(|(id, name, types)| (*id, *name, types.to_vec()))
                .collect(),
        )))
    }

    fn uninstall(&self, id: &str) {
        self.0
            .borrow_mut()
            .retain(|(installed, _, _)| *installed != id);
    }
}

impl InstalledApplications for Installed {
    fn is_installed(&self, desktop_id: &str) -> bool {
        self.0.borrow().iter().any(|(id, _, _)| *id == desktop_id)
    }

    fn for_types(&self, types: &[String]) -> Vec<ApplicationChoice> {
        self.0
            .borrow()
            .iter()
            .filter(|(_, _, declared)| types.iter().all(|t| declared.contains(&t.as_str())))
            .map(|(id, name, _)| ApplicationChoice {
                desktop_id: id.to_string(),
                name: name.to_string(),
            })
            .collect()
    }
}

struct Desktop {
    directory: tempfile::TempDir,
    catalog: better_core::ComponentCatalog,
    store: SnapshotStore,
    adapters: AdapterSet,
}

impl Desktop {
    fn new(mimeapps: &str, types: &[&str], installed: &Installed) -> Self {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("mimeapps.list"), mimeapps).unwrap();
        let mut integration = integration("handler", types[0], FILES);
        if types.len() > 1 {
            integration.kind = IntegrationKind::MimeUriHandlerGroup;
        }
        integration.target.keys = types.iter().map(|value| value.to_string()).collect();
        let mut adapters = AdapterSet::in_memory().with_applications(Box::new(installed.clone()));
        adapters.insert(Box::new(XdgDefaultAppAdapter::new(
            AdapterId::XdgDefaultApp,
            AssociationStore::new(
                directory.path().join("mimeapps.list"),
                directory.path().join("rollback"),
            ),
        )));
        Self {
            store: SnapshotStore::at_path(directory.path().join("snapshots")),
            catalog: catalog(vec![manifest("better-files", vec![integration])]),
            adapters,
            directory,
        }
    }

    fn file(&self) -> String {
        std::fs::read_to_string(self.directory.path().join("mimeapps.list")).unwrap()
    }

    fn plan(&self, restore: bool, confirmations: &Confirmations) -> DefaultsPlan {
        let history = self.store.history().unwrap();
        let engine = engine(&self.catalog);
        if restore {
            engine.plan_restore(&Selection::All, &self.adapters, &history, confirmations)
        } else {
            engine.plan_apply(&Selection::All, &self.adapters, &history, confirmations)
        }
    }

    fn execute(&mut self, plan: &DefaultsPlan) -> DefaultsOutcome {
        engine(&self.catalog)
            .execute(plan, &mut self.adapters, &self.store)
            .unwrap()
    }

    fn run(&mut self, restore: bool, confirmations: &Confirmations) -> DefaultsOutcome {
        let plan = self.plan(restore, confirmations);
        self.execute(&plan)
    }

    fn restore_state(&self) -> RestoreState {
        self.store
            .history()
            .unwrap()
            .latest_entry(&component("better-files"), &integration_id("handler"))
            .unwrap()
            .restore_state
    }
}

fn engine(catalog: &better_core::ComponentCatalog) -> DefaultsEngine<'_> {
    DefaultsEngine::new(catalog, SystemContext::new("zorin", "gnome"))
        .with_readiness(component("better-files"), ComponentReadiness::ready())
}

fn choice(id: &str, name: &str) -> ApplicationChoice {
    ApplicationChoice {
        desktop_id: id.to_string(),
        name: name.to_string(),
    }
}

fn file_managers() -> Installed {
    Installed::new(&[
        (FILES, "Better Files", &["inode/directory"]),
        (NAUTILUS, "Files", &["inode/directory"]),
        (DOLPHIN, "Dolphin", &["inode/directory"]),
    ])
}

const ONE_MANAGER: &str = "[Default Applications]\n\
    inode/directory=org.gnome.Nautilus.desktop\n";

fn missing_nautilus() -> SkipReason {
    SkipReason::PreviousApplicationMissing {
        desktop_id: NAUTILUS.to_string(),
        alternatives: vec![choice(FILES, "Better Files"), choice(DOLPHIN, "Dolphin")],
    }
}

#[test]
fn restore_names_an_uninstalled_previous_application_and_writes_nothing() {
    let installed = file_managers();
    let mut desktop = Desktop::new(ONE_MANAGER, &["inode/directory"], &installed);
    assert!(
        desktop.run(false, &Confirmations::none()).results[0]
            .outcome
            .is_success()
    );
    installed.uninstall(NAUTILUS);

    let plan = desktop.plan(true, &Confirmations::none());
    assert_eq!(
        plan.entries[0].action,
        PlanAction::Skip {
            reason: missing_nautilus()
        }
    );
    let before = desktop.file();
    let outcome = desktop.execute(&plan);
    assert_eq!(
        outcome.results[0].outcome,
        EntryOutcome::Skipped {
            reason: missing_nautilus()
        }
    );
    assert_eq!(desktop.file(), before, "nothing is written for it");
    assert!(
        desktop
            .file()
            .contains("inode/directory=io.betteros.Files.desktop")
    );
    // The capture is still there, so the person can choose what to do with it.
    assert_eq!(desktop.restore_state(), RestoreState::Available);
}

#[test]
fn restore_checks_again_when_it_runs() {
    let installed = file_managers();
    let mut desktop = Desktop::new(ONE_MANAGER, &["inode/directory"], &installed);
    desktop.run(false, &Confirmations::none());

    let plan = desktop.plan(true, &Confirmations::none());
    assert_eq!(
        plan.entries[0].action,
        PlanAction::Restore { to: set(NAUTILUS) }
    );
    // Uninstalled between the review and the run.
    installed.uninstall(NAUTILUS);
    let outcome = desktop.execute(&plan);
    assert_eq!(
        outcome.results[0].outcome,
        EntryOutcome::Skipped {
            reason: missing_nautilus()
        }
    );
    assert!(
        desktop
            .file()
            .contains("inode/directory=io.betteros.Files.desktop")
    );
}

#[test]
fn choosing_another_application_restores_to_it() {
    let installed = file_managers();
    let mut desktop = Desktop::new(ONE_MANAGER, &["inode/directory"], &installed);
    desktop.run(false, &Confirmations::none());
    installed.uninstall(NAUTILUS);

    let chosen = Confirmations::none().with_restore_choice(
        component("better-files"),
        integration_id("handler"),
        None,
        RestoreChoice::Application(DOLPHIN.to_string()),
    );
    let plan = desktop.plan(true, &chosen);
    assert_eq!(
        plan.entries[0].action,
        PlanAction::Restore { to: set(DOLPHIN) }
    );
    // The review still shows what is missing and what was chosen instead.
    assert_eq!(
        plan.entries[0].warnings,
        vec![PlanWarning::PreviousApplicationMissing {
            key: None,
            desktop_id: NAUTILUS.to_string(),
            alternatives: vec![choice(FILES, "Better Files"), choice(DOLPHIN, "Dolphin")],
            chosen: Some(RestoreChoice::Application(DOLPHIN.to_string())),
        }]
    );
    let outcome = desktop.execute(&plan);
    assert_eq!(
        outcome.results[0].outcome,
        EntryOutcome::Restored {
            value: set(DOLPHIN)
        }
    );
    assert_eq!(
        desktop.file(),
        "[Default Applications]\ninode/directory=org.kde.dolphin.desktop\n"
    );
    assert_eq!(desktop.restore_state(), RestoreState::AlreadyRestored);
}

#[test]
fn clearing_instead_removes_the_default() {
    let installed = file_managers();
    let mut desktop = Desktop::new(ONE_MANAGER, &["inode/directory"], &installed);
    desktop.run(false, &Confirmations::none());
    installed.uninstall(NAUTILUS);

    let cleared = Confirmations::none().with_restore_choice(
        component("better-files"),
        integration_id("handler"),
        None,
        RestoreChoice::ClearDefault,
    );
    let outcome = desktop.run(true, &cleared);
    assert_eq!(
        outcome.results[0].outcome,
        EntryOutcome::Restored {
            value: ObservedValue::Unset
        }
    );
    assert_eq!(desktop.file(), "[Default Applications]\n");
}

#[test]
fn a_choice_that_is_not_installed_is_not_taken() {
    let installed = file_managers();
    let mut desktop = Desktop::new(ONE_MANAGER, &["inode/directory"], &installed);
    desktop.run(false, &Confirmations::none());
    installed.uninstall(NAUTILUS);

    let invented = Confirmations::none().with_restore_choice(
        component("better-files"),
        integration_id("handler"),
        None,
        RestoreChoice::Application("org.example.Gone.desktop".to_string()),
    );
    assert_eq!(
        desktop.plan(true, &invented).entries[0].action,
        PlanAction::Skip {
            reason: missing_nautilus()
        }
    );
}

#[test]
fn an_installed_previous_application_restores_as_before() {
    let installed = file_managers();
    let mut desktop = Desktop::new(ONE_MANAGER, &["inode/directory"], &installed);
    desktop.run(false, &Confirmations::none());
    assert_eq!(
        desktop.run(true, &Confirmations::none()).results[0].outcome,
        EntryOutcome::Restored {
            value: set(NAUTILUS)
        }
    );
    assert_eq!(desktop.file(), ONE_MANAGER);
}

const TWO_VIEWERS: &str = "[Default Applications]\n\
    image/png=org.gnome.eog.desktop\n\
    image/jpeg=org.gnome.gThumb.desktop\n";

fn viewers() -> Installed {
    Installed::new(&[
        (FILES, "Better Files", &["image/png", "image/jpeg"]),
        (EOG, "Eye of GNOME", &["image/png", "image/jpeg"]),
        (GTHUMB, "gThumb", &["image/png", "image/jpeg"]),
    ])
}

fn missing_eog(installed: &Installed) -> SkipReason {
    SkipReason::PreviousApplicationMissing {
        desktop_id: EOG.to_string(),
        alternatives: installed.for_types(&["image/png".to_string()]),
    }
}

#[test]
fn a_group_restored_type_by_type_reports_the_missing_type_and_restores_the_rest() {
    let installed = viewers();
    let mut desktop = Desktop::new(TWO_VIEWERS, &["image/png", "image/jpeg"], &installed);
    desktop.run(false, &Confirmations::none());
    installed.uninstall(EOG);

    let plan = desktop.plan(true, &Confirmations::none());
    let captured = ObservedValue::Mixed {
        per_key: vec![
            KeyObservation {
                key: "image/png".to_string(),
                observed: set(EOG),
            },
            KeyObservation {
                key: "image/jpeg".to_string(),
                observed: set(GTHUMB),
            },
        ],
    };
    assert_eq!(plan.entries[0].action, PlanAction::Restore { to: captured });
    assert!(
        plan.entries[0]
            .warnings
            .contains(&PlanWarning::PreviousApplicationMissing {
                key: Some("image/png".to_string()),
                desktop_id: EOG.to_string(),
                alternatives: installed.for_types(&["image/png".to_string()]),
                chosen: None,
            }),
        "{:?}",
        plan.entries[0].warnings
    );

    let outcome = desktop.execute(&plan);
    assert_eq!(
        outcome.results[0].outcome,
        EntryOutcome::PerKey {
            keys: vec![
                KeyOutcome {
                    key: "image/png".to_string(),
                    outcome: EntryOutcome::Skipped {
                        reason: missing_eog(&installed)
                    },
                },
                KeyOutcome {
                    key: "image/jpeg".to_string(),
                    outcome: EntryOutcome::Restored { value: set(GTHUMB) },
                },
            ]
        }
    );
    assert!(
        !outcome.has_failures(),
        "a missing application is not a failure"
    );
    assert!(
        desktop
            .file()
            .contains("image/png=io.betteros.Files.desktop")
    );
    assert!(
        desktop
            .file()
            .contains("image/jpeg=org.gnome.gThumb.desktop")
    );
    assert_eq!(desktop.restore_state(), RestoreState::Available);

    // The missing type is then cleared on its own.
    let cleared = Confirmations::none().with_restore_choice(
        component("better-files"),
        integration_id("handler"),
        Some("image/png".to_string()),
        RestoreChoice::ClearDefault,
    );
    let outcome = desktop.run(true, &cleared);
    let EntryOutcome::PerKey { keys } = &outcome.results[0].outcome else {
        panic!("expected a per-type result: {outcome:?}");
    };
    assert_eq!(
        keys[0].outcome,
        EntryOutcome::Restored {
            value: ObservedValue::Unset
        }
    );
    assert!(outcome.results[0].outcome.is_success());
    assert_eq!(
        desktop.file(),
        "[Default Applications]\nimage/jpeg=org.gnome.gThumb.desktop\n"
    );
    assert_eq!(desktop.restore_state(), RestoreState::AlreadyRestored);
}
