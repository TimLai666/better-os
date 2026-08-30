//! Discovery, precedence, and change watching against real directories.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use app_catalog_core::{DesktopId, EntryScope, NoProbe};
use app_catalog_platform::{
    ApplicationDirectories, ApplicationDirectory, CatalogWatcher, WatchBackend, discover,
};

fn write(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("fixture directory");
    }
    fs::write(path, contents).expect("fixture file");
}

fn entry(name: &str, extra: &str) -> String {
    format!("[Desktop Entry]\nType=Application\nName={name}\nExec=app\n{extra}")
}

fn directories(paths: &[(PathBuf, EntryScope)]) -> ApplicationDirectories {
    ApplicationDirectories::new(
        paths
            .iter()
            .enumerate()
            .map(|(rank, (path, scope))| ApplicationDirectory {
                path: path.clone(),
                rank,
                scope: *scope,
            })
            .collect(),
    )
}

#[test]
fn discovery_reads_user_and_system_directories_with_the_right_precedence() {
    let root = tempfile::tempdir().expect("temporary root");
    let user = root.path().join("user/applications");
    let system = root.path().join("system/applications");
    write(&user.join("editor.desktop"), &entry("User Editor", ""));
    write(&system.join("editor.desktop"), &entry("System Editor", ""));
    write(&system.join("other.desktop"), &entry("Other", ""));
    write(&system.join("vendor/nested.desktop"), &entry("Nested", ""));
    // Not a desktop entry, and not read.
    write(&system.join("mimeinfo.cache"), "[MIME Cache]\n");

    let directories = directories(&[(user, EntryScope::User), (system, EntryScope::System)]);
    let catalog = discover(&directories, &NoProbe);

    assert_eq!(catalog.len(), 3);
    assert_eq!(
        catalog
            .get(&DesktopId::new("editor.desktop").unwrap())
            .unwrap()
            .display_name(None),
        "User Editor"
    );
    assert_eq!(
        catalog
            .get(&DesktopId::new("editor.desktop").unwrap())
            .unwrap()
            .source
            .scope,
        EntryScope::User
    );
    assert!(
        catalog
            .get(&DesktopId::new("vendor-nested.desktop").unwrap())
            .is_some()
    );
    assert_eq!(catalog.shadowed().len(), 1);
    assert!(catalog.rejected().is_empty());
}

#[test]
fn a_missing_directory_is_skipped_rather_than_failing_discovery() {
    let root = tempfile::tempdir().expect("temporary root");
    let system = root.path().join("system/applications");
    write(&system.join("only.desktop"), &entry("Only", ""));
    let directories = directories(&[
        (root.path().join("absent/applications"), EntryScope::User),
        (system, EntryScope::System),
    ]);
    let catalog = discover(&directories, &NoProbe);
    assert_eq!(catalog.len(), 1);
}

#[test]
fn a_hidden_user_entry_removes_the_system_application_from_discovery() {
    let root = tempfile::tempdir().expect("temporary root");
    let user = root.path().join("user/applications");
    let system = root.path().join("system/applications");
    write(&user.join("app.desktop"), &entry("App", "Hidden=true\n"));
    write(&system.join("app.desktop"), &entry("App", ""));
    let directories = directories(&[(user, EntryScope::User), (system, EntryScope::System)]);
    let catalog = discover(&directories, &NoProbe);
    assert!(catalog.is_empty());
    assert_eq!(catalog.hidden().len(), 1);
}

#[test]
fn watching_uses_an_event_driven_backend_and_reports_an_added_entry() {
    let root = tempfile::tempdir().expect("temporary root");
    let system = root.path().join("system/applications");
    write(&system.join("first.desktop"), &entry("First", ""));
    let directories = directories(&[(system.clone(), EntryScope::System)]);

    let watcher = CatalogWatcher::new(&directories).expect("watcher");
    // On Linux the recommended backend is inotify, so nothing runs while the
    // directories are idle. If this ever became a poll watcher the catalog
    // would be burning CPU on an idle desktop.
    assert_eq!(watcher.backend(), WatchBackend::EventDriven);
    assert_eq!(watcher.watched(), std::slice::from_ref(&system));

    write(&system.join("second.desktop"), &entry("Second", ""));
    let change = watcher
        .next_change(Duration::from_secs(10), Duration::from_millis(100))
        .expect("a change event");
    assert!(
        change
            .paths
            .iter()
            .any(|path| path.ends_with("second.desktop"))
    );

    let catalog = discover(&directories, &NoProbe);
    assert_eq!(catalog.len(), 2);
}

#[test]
fn watching_reports_a_removed_entry_and_the_reload_drops_it() {
    let root = tempfile::tempdir().expect("temporary root");
    let system = root.path().join("system/applications");
    write(&system.join("first.desktop"), &entry("First", ""));
    write(&system.join("second.desktop"), &entry("Second", ""));
    let directories = directories(&[(system.clone(), EntryScope::System)]);
    let watcher = CatalogWatcher::new(&directories).expect("watcher");

    fs::remove_file(system.join("second.desktop")).expect("remove entry");
    let change = watcher
        .next_change(Duration::from_secs(10), Duration::from_millis(100))
        .expect("a change event");
    assert!(
        change
            .paths
            .iter()
            .any(|path| path.ends_with("second.desktop"))
    );

    let catalog = discover(&directories, &NoProbe);
    assert_eq!(catalog.len(), 1);
}

#[test]
fn watching_reports_a_changed_entry_and_the_reload_sees_the_new_name() {
    let root = tempfile::tempdir().expect("temporary root");
    let system = root.path().join("system/applications");
    write(&system.join("first.desktop"), &entry("Before", ""));
    let directories = directories(&[(system.clone(), EntryScope::System)]);
    let watcher = CatalogWatcher::new(&directories).expect("watcher");

    write(&system.join("first.desktop"), &entry("After", ""));
    let change = watcher
        .next_change(Duration::from_secs(10), Duration::from_millis(100))
        .expect("a change event");
    assert!(
        change
            .paths
            .iter()
            .any(|path| path.ends_with("first.desktop"))
    );

    let catalog = discover(&directories, &NoProbe);
    assert_eq!(
        catalog
            .get(&DesktopId::new("first.desktop").unwrap())
            .unwrap()
            .display_name(None),
        "After"
    );
}

#[test]
fn an_idle_watcher_reports_nothing() {
    let root = tempfile::tempdir().expect("temporary root");
    let system = root.path().join("system/applications");
    write(&system.join("first.desktop"), &entry("First", ""));
    let directories = directories(&[(system, EntryScope::System)]);
    let watcher = CatalogWatcher::new(&directories).expect("watcher");
    assert!(
        watcher
            .next_change(Duration::from_millis(300), Duration::from_millis(50))
            .is_none()
    );
}

#[test]
fn an_unreadable_entry_is_reported_rather_than_dropped_silently() {
    use std::os::unix::fs::PermissionsExt;

    let root = tempfile::tempdir().expect("temporary root");
    let system = root.path().join("system/applications");
    write(&system.join("readable.desktop"), &entry("Readable", ""));
    let secret = system.join("secret.desktop");
    write(&secret, &entry("Secret", ""));
    fs::set_permissions(&secret, fs::Permissions::from_mode(0o000)).expect("permissions");

    let directories = directories(&[(system, EntryScope::System)]);
    let catalog = discover(&directories, &NoProbe);

    if catalog.rejected().is_empty() {
        // Running as root defeats the permission bits; the readable entry must
        // still be there either way.
        assert_eq!(catalog.len(), 2);
    } else {
        assert_eq!(catalog.len(), 1);
        assert!(
            catalog.rejected()[0]
                .error
                .to_string()
                .starts_with("catalog.error.unreadable:")
        );
    }
}
