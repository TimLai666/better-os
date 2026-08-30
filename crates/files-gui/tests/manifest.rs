//! Better Files' component manifest, validated the way the manager validates
//! one.
//!
//! Beyond "it parses", three claims are checked here because they are claims
//! the manifest makes about behaviour rather than about itself: every MIME
//! association path the code can touch is declared, every declared integration
//! restores to a captured value rather than to a guessed default, and the
//! rollback plan is written down rather than implied.

use better_core::{
    ComponentCatalog, ComponentIcon, ComponentManifest, ComponentType, IntegrationKind,
    RestartScope, RestorePolicy,
};

const MANIFEST: &str = include_str!("../../../components/manifests/better-files.yaml");

fn manifest() -> ComponentManifest {
    ComponentManifest::parse_yaml(MANIFEST).expect("the Better Files manifest must be valid")
}

#[test]
fn the_manifest_parses_and_validates() {
    let manifest = manifest();
    manifest.validate().expect("validation");
    assert_eq!(manifest.id.as_str(), "better-files");
    assert_eq!(manifest.icon, ComponentIcon::Files);
    assert_eq!(manifest.restart, Some(RestartScope::Application));
    assert_eq!(manifest.component_type, ComponentType::Replacement);
}

#[test]
fn it_loads_into_a_catalog_beside_the_other_first_party_components() {
    let manifests = [
        include_str!("../../../components/manifests/better-manager.yaml"),
        include_str!("../../../components/manifests/better-monitor.yaml"),
        include_str!("../../../components/manifests/better-launcher.yaml"),
        MANIFEST,
    ]
    .into_iter()
    .map(|document| ComponentManifest::parse_yaml(document).unwrap())
    .collect::<Vec<_>>();
    let catalog = ComponentCatalog::from_manifests(manifests).expect("catalog");
    assert_eq!(catalog.manifests().count(), 4);
}

#[test]
fn every_declared_target_has_exactly_one_artifact() {
    let manifest = manifest();
    let expected = manifest.targets.releases.len() * manifest.targets.architectures.len();
    assert_eq!(manifest.artifacts.len(), expected);
    assert_eq!(expected, 4);
}

#[test]
fn the_desktop_integration_is_declared_and_is_not_implied_by_installing() {
    let manifest = manifest();
    let kinds: Vec<IntegrationKind> = manifest
        .default_integrations
        .iter()
        .map(|integration| integration.kind)
        .collect();
    assert!(kinds.contains(&IntegrationKind::ApplicationHandler));
    assert!(kinds.contains(&IntegrationKind::ToolEntryPoint));
    assert!(kinds.contains(&IntegrationKind::DesktopLauncherEntry));

    // Installing the package makes nothing the default. Every one of these is
    // a separate declaration a user applies through Better Defaults.
    for integration in &manifest.default_integrations {
        integration.validate().expect("each integration validates");
    }
}

#[test]
fn every_mime_association_path_the_code_can_touch_is_declared() {
    let manifest = manifest();
    // The MIME type Better Files claims a handler for.
    let handler = manifest
        .default_integrations
        .iter()
        .find(|integration| integration.id.as_str() == "default-file-manager")
        .expect("the file-manager handler");
    assert_eq!(handler.target.keys, ["inode/directory"]);

    // And the files it writes when Always Use is chosen. `app-chooser-core`
    // writes exactly these two: one line in `mimeapps.list`, and one rollback
    // record beside it.
    for path in [
        "~/.config/mimeapps.list",
        "~/.local/share/better-os/app-chooser/rollback",
    ] {
        assert!(
            manifest.paths.iter().any(|declared| declared == path),
            "{path} is written by the association store and must be declared"
        );
    }
}

#[test]
fn restoring_returns_to_the_captured_value_rather_than_a_guessed_default() {
    let manifest = manifest();
    for integration in &manifest.default_integrations {
        // `leave-in-place` is legitimate for the overview entry, which is
        // additive: removing Better Files from a favourites list it was added
        // to is the user's call, not the uninstaller's.
        assert!(
            matches!(
                integration.restore_policy,
                RestorePolicy::CapturedValue | RestorePolicy::LeaveInPlace
            ),
            "{} must not guess a factory default",
            integration.id.as_str()
        );
    }
    // The exclusive ones — the two that take something over — all capture.
    for id in ["default-file-manager", "file-manager-tool-entry-point"] {
        let integration = manifest
            .default_integrations
            .iter()
            .find(|integration| integration.id.as_str() == id)
            .expect(id);
        assert_eq!(integration.restore_policy, RestorePolicy::CapturedValue);
    }
}

#[test]
fn the_rollback_behavior_is_written_down_rather_than_implied() {
    let manifest = manifest();
    assert!(!manifest.lifecycle.rollback.is_empty());
    assert!(
        manifest
            .release_notes
            .iter()
            .any(|note| note.to_lowercase().contains("rollback")),
        "the rollback plan is stated in the notes a user reads"
    );
    // And the promise removal must keep: an unrelated association is not the
    // uninstaller's to delete.
    assert!(
        manifest
            .release_notes
            .iter()
            .any(|note| note.contains("one line at a time")),
        "the single-line edit is why removal cannot erase someone else's association"
    );
    assert!(!manifest.health_checks.is_empty());
}

#[test]
fn every_permission_states_why_it_is_needed() {
    let manifest = manifest();
    assert!(!manifest.permissions.is_empty());
    for permission in &manifest.permissions {
        assert!(
            permission.reason.len() > 20,
            "{} needs a real reason, not a label",
            permission.name
        );
    }
    // The one that would be most tempting to leave unexplained.
    let association = manifest
        .permissions
        .iter()
        .find(|permission| permission.name == "mime-association-write")
        .expect("the association permission");
    assert!(association.reason.contains("rollback record"));
}

#[test]
fn the_benchmarks_name_a_workload_and_a_metric_for_every_claim() {
    let manifest = manifest();
    let names: Vec<&str> = manifest
        .benchmarks
        .iter()
        .map(|benchmark| benchmark.name.as_str())
        .collect();
    // Every scenario the harness runs has a declared budget here, so a public
    // claim has somewhere to be checked against.
    for expected in [
        "large-directory-first-content",
        "current-directory-search",
        "preview-generation",
        "large-sequential-copy",
        "many-small-file-copy",
        "device-connect-open-disconnect",
    ] {
        assert!(
            names.contains(&expected),
            "{expected} has no declared budget"
        );
    }
    for benchmark in &manifest.benchmarks {
        assert!(!benchmark.workload.is_empty());
        assert!(!benchmark.metric.is_empty());
    }
}
