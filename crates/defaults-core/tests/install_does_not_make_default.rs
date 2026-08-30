//! Installing a component must not make it the default.
//!
//! This is asserted over the real install path rather than by reading the code:
//! a full manager transaction runs to completion against a component whose
//! manifest declares integrations, and afterwards nothing has been written to
//! any adapter and no snapshot exists.

mod common;

use better_core::defaults::AdapterId;
use common::*;
use defaults_core::{AggregateState, ComponentReadiness, DefaultsEngine, Selection, SystemContext};
use defaults_platform::InMemoryAdapter;
use defaults_store::SnapshotStore;
use manager_core::{
    DesiredOperation, Manager, ManagerState, MockOutcome, OperationProgress, SystemProfile,
};

const FILES: &str = "io.betteros.Files.desktop";
const NAUTILUS: &str = "org.gnome.Nautilus.desktop";
const SLOT: &str = "better-files/default-file-manager";

#[test]
fn installing_and_verifying_a_component_never_makes_it_the_default() {
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

    // The system default before anything happens.
    let mut adapter = InMemoryAdapter::new(AdapterId::XdgDefaultApp);
    adapter.preset(SLOT, set(NAUTILUS));

    let manager = Manager::new(catalog.clone(), SystemProfile::default());
    let mut state = ManagerState::default();
    let component_id = component("better-files");

    for operation in [DesiredOperation::Install, DesiredOperation::Verify] {
        let plan = manager.plan(&state, &component_id, operation).unwrap();
        manager.begin(&mut state, plan).unwrap();
        loop {
            match manager
                .advance_mock(&mut state, MockOutcome::Succeed)
                .unwrap()
            {
                OperationProgress::InProgress { .. } => continue,
                OperationProgress::Finished { .. } => break,
                OperationProgress::Failed { failure } => {
                    panic!("the mock lifecycle failed at {:?}", failure.stage)
                }
            }
        }
    }

    // Nothing in the install path can reach an adapter, so nothing was written.
    assert!(
        adapter.writes().is_empty(),
        "installing wrote to a defaults adapter: {:?}",
        adapter.writes()
    );
    assert!(store.history().unwrap().snapshots().is_empty());

    // And the component reports itself as not default, with the previous owner
    // still in place.
    let adapters = adapters_with(adapter);
    let engine = DefaultsEngine::new(&catalog, SystemContext::new("zorin", "gnome"))
        .with_readiness(component_id.clone(), ComponentReadiness::ready());
    let report = engine.inspect(&Selection::All, &adapters, &store.history().unwrap());

    assert_eq!(report.components[0].aggregate, AggregateState::NotDefault);
    assert_eq!(report.components[0].integrations[0].current, set(NAUTILUS));
}

#[test]
fn a_component_that_was_installed_still_needs_an_explicit_plan_to_become_default() {
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
    let mut adapters = adapters_with(xdg_adapter(&[(SLOT, set(NAUTILUS))]));
    let engine = DefaultsEngine::new(&catalog, SystemContext::new("zorin", "gnome"))
        .with_readiness(component("better-files"), ComponentReadiness::ready());

    // Building a plan reads and decides. It writes nothing until it is run.
    let plan = engine.plan_apply(
        &Selection::All,
        &adapters,
        &store.history().unwrap(),
        &defaults_core::Confirmations::none(),
    );
    assert_eq!(plan.changes().count(), 1);
    assert!(store.history().unwrap().snapshots().is_empty());
    let report = engine.inspect(&Selection::All, &adapters, &store.history().unwrap());
    assert_eq!(report.components[0].aggregate, AggregateState::NotDefault);

    // Only running it changes anything.
    engine.execute(&plan, &mut adapters, &store).unwrap();
    let report = engine.inspect(&Selection::All, &adapters, &store.history().unwrap());
    assert_eq!(report.components[0].aggregate, AggregateState::Default);
}
