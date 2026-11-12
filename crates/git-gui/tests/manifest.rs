//! Better Git's component manifest, validated the way the manager validates
//! one.
//!
//! Beyond "it parses", the manifest is checked against the things it makes
//! claims about: the type and icon it takes from the closed sets, the version
//! it shares with the workspace, the state file the window actually writes, and
//! the desktop entry the package installs, whose file name and `StartupWMClass`
//! have to be the `app_id` the window opens with or the shell cannot match the
//! two.

use std::path::PathBuf;

use better_core::{
    ComponentCatalog, ComponentIcon, ComponentManifest, ComponentType, RestartScope,
};

const MANIFEST: &str = include_str!("../../../components/manifests/better-git.yaml");
const DESKTOP_ENTRY: &str = include_str!("../../../packaging/git/io.betteros.Git.desktop");
const WINDOW_SOURCE: &str = include_str!("../src/app.rs");

fn manifest() -> ComponentManifest {
    ComponentManifest::parse_yaml(MANIFEST).expect("the Better Git manifest must be valid")
}

fn desktop_value<'a>(key: &str) -> Option<&'a str> {
    let prefix = format!("{key}=");
    DESKTOP_ENTRY
        .lines()
        .find_map(|line| line.strip_prefix(prefix.as_str()))
}

#[test]
fn the_manifest_parses_and_validates() {
    let manifest = manifest();
    manifest.validate().expect("validation");
    assert_eq!(manifest.id.as_str(), "better-git");
    assert_eq!(manifest.display_name, "Better Git");
    assert_eq!(manifest.restart, Some(RestartScope::Application));
}

#[test]
fn it_takes_existing_values_from_the_closed_sets_and_claims_nothing_it_does_not_touch() {
    // A released Better Manager refuses a type or icon it does not know, so
    // Better Git uses values that already exist. Enhancement is the nearest
    // type; it replaces and enhances nothing, which is the honest reading of a
    // standalone application.
    let manifest = manifest();
    assert_eq!(manifest.component_type, ComponentType::Enhancement);
    assert!(manifest.replaces.is_empty(), "{:?}", manifest.replaces);
    assert!(manifest.enhances.is_empty(), "{:?}", manifest.enhances);
    assert_eq!(manifest.icon, ComponentIcon::Generic);
    assert!(manifest.dependencies.is_empty());
    assert!(manifest.default_integrations.is_empty());
}

#[test]
fn the_manifest_is_at_the_workspace_version() {
    let manifest = manifest();
    assert_eq!(manifest.version.to_string(), env!("CARGO_PKG_VERSION"));
}

#[test]
fn it_loads_into_a_catalog_beside_every_other_first_party_component() {
    let manifests = [
        include_str!("../../../components/manifests/better-manager.yaml"),
        include_str!("../../../components/manifests/better-monitor.yaml"),
        include_str!("../../../components/manifests/better-launcher.yaml"),
        include_str!("../../../components/manifests/better-files.yaml"),
        include_str!("../../../components/manifests/better-touchpad.yaml"),
        include_str!("../../../components/manifests/better-awake.yaml"),
        include_str!("../../../components/manifests/better-storage.yaml"),
        MANIFEST,
    ]
    .into_iter()
    .map(|document| ComponentManifest::parse_yaml(document).unwrap())
    .collect::<Vec<_>>();
    let catalog = ComponentCatalog::from_manifests(manifests).expect("catalog");
    assert_eq!(catalog.manifests().count(), 8);
}

#[test]
fn every_declared_target_has_exactly_one_artifact_named_by_the_release_rule() {
    let manifest = manifest();
    let expected = manifest.targets.releases.len() * manifest.targets.architectures.len();
    assert_eq!(manifest.artifacts.len(), expected);
    assert_eq!(expected, 4);
    for artifact in &manifest.artifacts {
        assert_eq!(
            artifact.release_asset,
            format!(
                "better-git_{}_ubuntu-{}_{}.deb",
                manifest.version, artifact.release, artifact.architecture
            )
        );
    }
}

#[test]
fn the_manifest_declares_what_the_package_installs_and_the_state_the_window_writes() {
    let manifest = manifest();
    let recent = git_gui::recent::state_path(None, Some(PathBuf::from("/home/someone")))
        .expect("an absolute HOME gives a path");
    let recent = recent
        .strip_prefix("/home/someone")
        .expect("the list lives under HOME");
    let recent = format!("~/{}", recent.display());
    for path in [
        "/usr/bin/better-git",
        "/usr/share/applications/io.betteros.Git.desktop",
        "/usr/share/icons/hicolor/scalable/apps/better-git.svg",
        recent.as_str(),
        "~/.local/state/better-os/git-repositories.json",
        "~/.local/state/better-os/git-trusted",
        "~/.config/better-os/git.json",
    ] {
        assert!(
            manifest.paths.iter().any(|declared| declared == path),
            "the manifest does not declare {path}"
        );
    }
}

#[test]
fn the_declared_state_files_are_the_ones_the_window_writes() {
    // SAFETY: this test is the only one in this file touching the
    // environment, and it only reads defaults computed from it.
    unsafe {
        std::env::set_var("HOME", "/home/someone");
        std::env::remove_var("XDG_STATE_HOME");
        std::env::remove_var("XDG_CONFIG_HOME");
    }
    let files = git_gui::session::Files::defaults();
    for (path, declared) in [
        (
            files.repositories,
            "~/.local/state/better-os/git-repositories.json",
        ),
        (files.trust, "~/.local/state/better-os/git-trusted"),
        (files.settings, "~/.config/better-os/git.json"),
    ] {
        assert_eq!(
            path.unwrap(),
            PathBuf::from(declared.replacen('~', "/home/someone", 1))
        );
    }
}

#[test]
fn the_desktop_entry_runs_the_window_and_matches_its_app_id() {
    assert_eq!(desktop_value("Exec"), Some("better-git"));
    assert_eq!(desktop_value("Icon"), Some("better-git"));
    assert_eq!(
        desktop_value("Categories"),
        Some("Development;RevisionControl;")
    );
    let app_id = desktop_value("StartupWMClass").expect("the entry names the window");
    assert!(
        WINDOW_SOURCE.contains(&format!("window_options(\"{app_id}\")")),
        "the window does not open with app_id {app_id}"
    );
    for key in ["Name[zh_TW]", "GenericName[zh_TW]", "Comment[zh_TW]"] {
        assert!(desktop_value(key).is_some(), "the entry has no {key}");
    }
}
