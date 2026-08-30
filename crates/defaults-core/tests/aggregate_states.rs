//! All eight aggregate states, one test each, plus the detail underneath them.

mod common;

use better_core::defaults::{
    AdapterId, HealthPrerequisite, IntegrationExclusivity, IntegrationKind, ObservedValue,
    RequiredPrivilege, SessionEffect,
};
use common::*;
use defaults_core::{
    AggregateState, ComponentReadiness, DefaultsEngine, IntegrationState, Selection, SystemContext,
};
use defaults_platform::AdapterSet;
use defaults_store::SnapshotStore;

const FILES: &str = "io.betteros.Files.desktop";
const NAUTILUS: &str = "org.gnome.Nautilus.desktop";

fn system() -> SystemContext {
    SystemContext::new("zorin", "gnome")
}

/// Builds a report for one component, given what the system says and what
/// Better Manager has recorded.
fn aggregate_of(
    manifests: Vec<better_core::ComponentManifest>,
    readiness: Vec<(&str, ComponentReadiness)>,
    adapters: AdapterSet,
    snapshot_entries: Vec<defaults_store::SnapshotEntry>,
) -> AggregateState {
    let directory = tempfile::tempdir().unwrap();
    let store = store_with(directory.path(), snapshot_entries);
    let catalog = catalog(manifests);
    let mut engine = DefaultsEngine::new(&catalog, system());
    for (id, ready) in readiness {
        engine = engine.with_readiness(component(id), ready);
    }
    let history = store.history().unwrap();
    let report = engine.inspect(&Selection::All, &adapters, &history);
    report
        .component(&component("better-files"))
        .expect("the component declares integrations")
        .aggregate
        .clone()
}

#[test]
fn every_integration_pointing_at_the_component_is_default() {
    let aggregate = aggregate_of(
        vec![manifest(
            "better-files",
            vec![integration(
                "default-file-manager",
                "inode/directory",
                FILES,
            )],
        )],
        vec![("better-files", ComponentReadiness::ready())],
        adapters_with(xdg_adapter(&[(
            "better-files/default-file-manager",
            set(FILES),
        )])),
        Vec::new(),
    );
    assert_eq!(aggregate, AggregateState::Default);
}

#[test]
fn no_integration_pointing_at_the_component_is_not_default() {
    let aggregate = aggregate_of(
        vec![manifest(
            "better-files",
            vec![integration(
                "default-file-manager",
                "inode/directory",
                FILES,
            )],
        )],
        vec![("better-files", ComponentReadiness::ready())],
        adapters_with(xdg_adapter(&[(
            "better-files/default-file-manager",
            set(NAUTILUS),
        )])),
        Vec::new(),
    );
    assert_eq!(aggregate, AggregateState::NotDefault);
}

#[test]
fn some_but_not_all_integrations_is_partially_default() {
    let aggregate = aggregate_of(
        vec![manifest(
            "better-files",
            vec![
                integration("default-file-manager", "inode/directory", FILES),
                integration("archive-handler", "application/zip", FILES),
            ],
        )],
        vec![("better-files", ComponentReadiness::ready())],
        adapters_with(xdg_adapter(&[
            ("better-files/default-file-manager", set(FILES)),
            ("better-files/archive-handler", set(NAUTILUS)),
        ])),
        Vec::new(),
    );
    assert_eq!(aggregate, AggregateState::PartiallyDefault);
}

#[test]
fn a_value_that_moved_after_better_manager_wrote_it_is_changed_externally() {
    let aggregate = aggregate_of(
        vec![manifest(
            "better-files",
            vec![integration(
                "default-file-manager",
                "inode/directory",
                FILES,
            )],
        )],
        vec![("better-files", ComponentReadiness::ready())],
        adapters_with(xdg_adapter(&[(
            "better-files/default-file-manager",
            set("org.kde.dolphin.desktop"),
        )])),
        vec![applied_entry(
            "better-files",
            "default-file-manager",
            set(NAUTILUS),
            FILES,
        )],
    );
    assert_eq!(aggregate, AggregateState::ChangedExternally);
}

#[test]
fn a_component_that_is_not_installed_is_unavailable() {
    let aggregate = aggregate_of(
        vec![manifest(
            "better-files",
            vec![integration(
                "default-file-manager",
                "inode/directory",
                FILES,
            )],
        )],
        vec![("better-files", ComponentReadiness::default())],
        adapters_with(xdg_adapter(&[(
            "better-files/default-file-manager",
            set(FILES),
        )])),
        Vec::new(),
    );
    assert!(matches!(aggregate, AggregateState::Unavailable { .. }));
}

#[test]
fn an_integration_this_session_does_not_support_is_unavailable() {
    let mut declaration = integration("default-file-manager", "inode/directory", FILES);
    declaration.sessions = vec!["kde".to_string()];
    let aggregate = aggregate_of(
        vec![manifest("better-files", vec![declaration])],
        vec![("better-files", ComponentReadiness::ready())],
        AdapterSet::in_memory(),
        Vec::new(),
    );
    assert_eq!(
        aggregate,
        AggregateState::Unavailable {
            reason: "defaults.not_supported_on_this_system".to_string()
        }
    );
}

#[test]
fn an_integration_needing_administrator_access_is_unavailable_rather_than_attempted() {
    let mut declaration = integration("default-file-manager", "inode/directory", FILES);
    declaration.privileges = RequiredPrivilege::Administrator;
    let aggregate = aggregate_of(
        vec![manifest("better-files", vec![declaration])],
        vec![("better-files", ComponentReadiness::ready())],
        AdapterSet::in_memory(),
        Vec::new(),
    );
    assert_eq!(
        aggregate,
        AggregateState::Unavailable {
            reason: "defaults.requires_administrator".to_string()
        }
    );
}

#[test]
fn another_installed_component_holding_an_exclusive_integration_is_a_conflict() {
    let mut theirs = integration("default-file-manager", "inode/directory", NAUTILUS);
    theirs.exclusivity = IntegrationExclusivity::Exclusive;
    let aggregate = aggregate_of(
        vec![
            manifest(
                "better-files",
                vec![integration(
                    "default-file-manager",
                    "inode/directory",
                    FILES,
                )],
            ),
            manifest("better-legacy", vec![theirs]),
        ],
        vec![
            ("better-files", ComponentReadiness::ready()),
            ("better-legacy", ComponentReadiness::ready()),
        ],
        adapters_with(xdg_adapter(&[(
            "better-files/default-file-manager",
            set(NAUTILUS),
        )])),
        Vec::new(),
    );
    assert_eq!(
        aggregate,
        AggregateState::Conflict {
            claimant: component("better-legacy")
        }
    );
}

#[test]
fn an_integration_kind_with_no_production_adapter_is_unknown_not_guessed() {
    let mut declaration = integration("selected-input-method", "ibus/engine", FILES);
    declaration.kind = IntegrationKind::InputMethod;
    declaration.apply_adapter = AdapterId::InputMethod;
    declaration.verify_adapter = AdapterId::InputMethod;
    let aggregate = aggregate_of(
        vec![manifest("better-files", vec![declaration])],
        vec![("better-files", ComponentReadiness::ready())],
        // A production set with only the XDG adapter in it.
        AdapterSet::new().with(Box::new(defaults_platform::InMemoryAdapter::new(
            AdapterId::XdgDefaultApp,
        ))),
        Vec::new(),
    );
    assert_eq!(
        aggregate,
        AggregateState::Unknown {
            reason: "defaults.no_production_adapter:InputMethod".to_string()
        }
    );
}

#[test]
fn a_value_written_but_not_yet_effective_needs_sign_out() {
    let mut declaration = integration("default-file-manager", "inode/directory", FILES);
    declaration.session_effect = SessionEffect::SignOut;
    let aggregate = aggregate_of(
        vec![manifest("better-files", vec![declaration])],
        vec![("better-files", ComponentReadiness::ready())],
        adapters_with(xdg_adapter(&[(
            "better-files/default-file-manager",
            set(NAUTILUS),
        )])),
        vec![applied_entry(
            "better-files",
            "default-file-manager",
            set(NAUTILUS),
            FILES,
        )],
    );
    assert_eq!(aggregate, AggregateState::NeedsSignOut);
}

#[test]
fn the_aggregate_never_hides_the_individual_results() {
    let directory = tempfile::tempdir().unwrap();
    let store = SnapshotStore::at_path(directory.path());
    let catalog = catalog(vec![manifest(
        "better-files",
        vec![
            integration("default-file-manager", "inode/directory", FILES),
            integration("archive-handler", "application/zip", FILES),
        ],
    )]);
    let engine = DefaultsEngine::new(&catalog, system())
        .with_readiness(component("better-files"), ComponentReadiness::ready());
    let adapters = adapters_with(xdg_adapter(&[
        ("better-files/default-file-manager", set(FILES)),
        ("better-files/archive-handler", set(NAUTILUS)),
    ]));

    let report = engine.inspect(&Selection::All, &adapters, &store.history().unwrap());
    let component = report.component(&component("better-files")).unwrap();

    assert_eq!(component.aggregate, AggregateState::PartiallyDefault);
    assert_eq!(component.integrations.len(), 2);
    assert_eq!(component.integrations[0].state, IntegrationState::Default);
    assert_eq!(
        component.integrations[1].state,
        IntegrationState::NotDefault
    );
    assert_eq!(component.integrations[1].current, set(NAUTILUS));
}

#[test]
fn a_component_declaring_nothing_is_left_out_of_the_report() {
    let directory = tempfile::tempdir().unwrap();
    let store = SnapshotStore::at_path(directory.path());
    let catalog = catalog(vec![manifest("better-files", Vec::new())]);
    let engine = DefaultsEngine::new(&catalog, system());

    let report = engine.inspect(
        &Selection::All,
        &AdapterSet::in_memory(),
        &store.history().unwrap(),
    );

    assert!(report.components.is_empty());
}

#[test]
fn an_unmet_prerequisite_names_which_one_it_was() {
    let mut declaration = integration("default-file-manager", "inode/directory", FILES);
    declaration.health_prerequisites = vec![
        HealthPrerequisite::Installed,
        HealthPrerequisite::Enabled,
        HealthPrerequisite::Healthy,
    ];
    let aggregate = aggregate_of(
        vec![manifest("better-files", vec![declaration])],
        vec![(
            "better-files",
            ComponentReadiness {
                installed: true,
                enabled: true,
                healthy: false,
            },
        )],
        AdapterSet::in_memory(),
        Vec::new(),
    );
    assert_eq!(
        aggregate,
        AggregateState::Unavailable {
            reason: "defaults.prerequisite_not_met:Healthy".to_string()
        }
    );
}

#[test]
fn an_adapter_that_cannot_tell_reports_unknown_rather_than_not_default() {
    let mut adapter = xdg_adapter(&[]);
    adapter.preset(
        "better-files/default-file-manager",
        ObservedValue::Unknown {
            reason: "test.cannot_tell".to_string(),
        },
    );
    let aggregate = aggregate_of(
        vec![manifest(
            "better-files",
            vec![integration(
                "default-file-manager",
                "inode/directory",
                FILES,
            )],
        )],
        vec![("better-files", ComponentReadiness::ready())],
        adapters_with(adapter),
        Vec::new(),
    );
    assert_eq!(
        aggregate,
        AggregateState::Unknown {
            reason: "test.cannot_tell".to_string()
        }
    );
}
