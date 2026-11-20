//! Putting back what a component took over, right before its package goes
//! (AGENTS.md: nothing a component set outlives it). Better Manager runs a
//! package's `/usr/lib/better-os/before-removal/<package>` as the person;
//! Better Files' is `better-files-before-removal`, which calls
//! [`restore_component`] for its own declarations (ticket 129, ADR 0041).
//!
//! It is the same Restore the Default apps page runs, for one component: a
//! setting someone changed since is left alone, and a setting that was never
//! changed has no record and is not touched.

use better_core::{ComponentCatalog, ComponentManifest};
use defaults_platform::AdapterSet;
use defaults_store::SnapshotStore;

use crate::engine::DefaultsEngine;
use crate::plan::{Confirmations, DefaultsOutcome, Selection};
use crate::status::{ComponentReadiness, SystemContext};

/// Restores every default `manifest` declares that Better Manager changed.
pub fn restore_component(
    manifest: ComponentManifest,
    system: SystemContext,
    adapters: &mut AdapterSet,
    store: &SnapshotStore,
) -> Result<DefaultsOutcome, String> {
    let component = manifest.id.clone();
    let catalog = ComponentCatalog::from_manifests(vec![manifest]).map_err(|e| e.to_string())?;
    let engine = DefaultsEngine::new(&catalog, system)
        // Still installed: its package is removed only after this returns.
        .with_readiness(component.clone(), ComponentReadiness::ready());
    let history = store.history().map_err(|e| e.to_string())?;
    let plan = engine.plan_restore(
        &Selection::one(component),
        adapters,
        &history,
        &Confirmations::none(),
    );
    engine
        .execute(&plan, adapters, store)
        .map_err(|e| e.to_string())
}
