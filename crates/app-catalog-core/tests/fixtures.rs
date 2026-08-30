//! Catalog behavior asserted over a recorded desktop-entry fixture tree.
//!
//! The tree in `tests/fixtures` stands in for a real installation, so every
//! visibility, precedence, and rejection rule is proved without depending on
//! whatever happens to be installed on the machine running the tests.

use std::fs;
use std::path::{Path, PathBuf};

use app_catalog_core::{
    CatalogBuilder, DesktopEnvironments, DesktopId, DirectoryRank, EntryScope, ExecutableStatus,
    LaunchTarget, Locale, MimeType, NoCanonicalExecutable, SourceKind,
};

fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// Walks one fixture application directory the way discovery does, folding
/// subdirectories into the desktop ID.
fn add_directory(builder: &mut CatalogBuilder<'_>, root: &Path, rank: &DirectoryRank) {
    let mut stack = vec![root.to_path_buf()];
    while let Some(current) = stack.pop() {
        for entry in fs::read_dir(&current).expect("fixture directory") {
            let path = entry.expect("fixture entry").path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension().and_then(|value| value.to_str()) != Some("desktop") {
                continue;
            }
            let relative = path.strip_prefix(root).expect("relative fixture path");
            let desktop_id = DesktopId::from_relative_path(relative).expect("fixture desktop id");
            let bytes = fs::read(&path).expect("fixture bytes");
            builder.add_entry(desktop_id, path, rank, &bytes);
        }
    }
}

fn catalog() -> app_catalog_core::Catalog {
    let root = fixture_root();
    let mut builder = CatalogBuilder::default();
    add_directory(
        &mut builder,
        &root.join("user/applications"),
        &DirectoryRank {
            rank: 0,
            scope: EntryScope::User,
        },
    );
    add_directory(
        &mut builder,
        &root.join("system/applications"),
        &DirectoryRank {
            rank: 1,
            scope: EntryScope::System,
        },
    );
    builder.build()
}

fn id(value: &str) -> DesktopId {
    DesktopId::new(value).expect("desktop id")
}

#[test]
fn the_user_entry_wins_and_the_system_entry_is_recorded_as_shadowed() {
    let catalog = catalog();
    let editor = catalog.get(&id("editor.desktop")).expect("editor record");
    assert_eq!(editor.display_name(None), "User Editor");
    assert_eq!(
        editor.display_name(Locale::parse("zh_TW.UTF-8").as_ref()),
        "使用者編輯器"
    );
    assert_eq!(editor.source.scope, EntryScope::User);
    assert!(
        catalog
            .shadowed()
            .iter()
            .any(|shadowed| shadowed.desktop_id == id("editor.desktop"))
    );
}

#[test]
fn a_hidden_user_entry_deletes_the_system_application() {
    let catalog = catalog();
    assert!(catalog.get(&id("system-only.desktop")).is_none());
    assert!(catalog.hidden().contains(&id("system-only.desktop")));
}

#[test]
fn a_subdirectory_entry_gets_a_dash_folded_desktop_id() {
    let catalog = catalog();
    let konsole = catalog
        .get(&id("kde4-konsole.desktop"))
        .expect("konsole record");
    assert_eq!(konsole.display_name(None), "Konsole");
}

#[test]
fn every_exclusion_rule_is_proved_separately() {
    let catalog = catalog();
    let gnome = DesktopEnvironments::parse("ubuntu:GNOME");
    let visible: Vec<&str> = catalog
        .visible(&gnome)
        .map(|record| record.desktop_id.as_str())
        .collect();

    // Present and shown.
    assert!(visible.contains(&"editor.desktop"));
    assert!(visible.contains(&"terminal-tool.desktop"));

    // NoDisplay: the record exists but is not shown.
    assert!(catalog.get(&id("helper.desktop")).is_some());
    assert!(!visible.contains(&"helper.desktop"));

    // OnlyShowIn names a desktop that is not this one.
    assert!(!visible.contains(&"kde-only.desktop"));
    let kde = DesktopEnvironments::parse("KDE");
    assert!(
        catalog
            .visible(&kde)
            .any(|record| record.desktop_id == id("kde-only.desktop"))
    );

    // NotShowIn names this desktop.
    assert!(!visible.contains(&"not-gnome.desktop"));
    assert!(
        catalog
            .visible(&kde)
            .any(|record| record.desktop_id == id("not-gnome.desktop"))
    );

    // TryExec names a program that is not installed.
    assert!(!visible.contains(&"missing-tool.desktop"));

    // Hidden removed the ID from the catalog entirely.
    assert!(!visible.contains(&"system-only.desktop"));
}

#[test]
fn a_terminal_entry_survives_with_its_flag_set() {
    let catalog = catalog();
    let record = catalog
        .get(&id("terminal-tool.desktop"))
        .expect("terminal record");
    assert!(record.capabilities.terminal);
}

#[test]
fn malformed_and_hostile_entries_are_rejected_with_machine_keys() {
    let catalog = catalog();
    let mut keys: Vec<String> = catalog
        .rejected()
        .iter()
        .map(|rejected| {
            format!(
                "{}={}",
                rejected.path.file_name().unwrap().to_string_lossy(),
                rejected.error
            )
        })
        .collect();
    keys.sort();
    assert_eq!(
        keys,
        vec![
            "broken-link.desktop=catalog.error.unsupported_type:Link".to_string(),
            "broken-no-name.desktop=catalog.error.missing_field:Name".to_string(),
            "broken-truncated.desktop=catalog.error.invalid_group_header:1".to_string(),
            "hostile-exec.desktop=catalog.error.exec_unknown_field_code:z".to_string(),
        ]
    );
    // A rejected neighbour never empties the catalog.
    assert!(catalog.len() >= 8);
}

#[test]
fn a_flatpak_entry_reports_that_it_has_no_canonical_executable() {
    let catalog = catalog();
    let record = catalog
        .get(&id("flatpak-app.desktop"))
        .expect("flatpak record");
    assert_eq!(record.source.kind, SourceKind::Flatpak);
    assert_eq!(
        record.executable,
        ExecutableStatus::NotApplicable {
            reason: NoCanonicalExecutable::Flatpak
        }
    );
    assert_eq!(record.executable.path(), None);
}

#[test]
fn a_dbus_activatable_entry_is_flagged_and_carries_its_action() {
    let catalog = catalog();
    let record = catalog
        .get(&id("dbus-files.desktop"))
        .expect("files record");
    assert!(record.capabilities.dbus_activatable);
    assert_eq!(record.dbus_service.as_deref(), Some("dbus-files"));
    assert_eq!(
        record.executable,
        ExecutableStatus::NotApplicable {
            reason: NoCanonicalExecutable::DBusActivated
        }
    );
    assert_eq!(record.action("new-window").unwrap().id, "new-window");
}

#[test]
fn mime_lookup_finds_the_winning_record_only() {
    let catalog = catalog();
    let gnome = DesktopEnvironments::parse("GNOME");
    let mime = MimeType::parse("text/plain").expect("mime type");
    let found: Vec<&str> = catalog
        .supporting_mime_type(&mime, &gnome)
        .map(|record| record.desktop_id.as_str())
        .collect();
    assert_eq!(found, vec!["editor.desktop"]);
}

#[test]
fn launching_the_winning_record_uses_its_own_exec_line() {
    let catalog = catalog();
    let record = catalog.get(&id("editor.desktop")).expect("editor record");
    let targets = vec![LaunchTarget::path("/tmp/notes; rm -rf ~.txt").expect("target")];
    let invocations = record
        .build_invocations(None, &targets, None)
        .expect("invocations");
    assert_eq!(invocations.len(), 1);
    assert_eq!(invocations[0].program, "user-editor");
    assert_eq!(invocations[0].arguments, vec!["/tmp/notes; rm -rf ~.txt"]);
}
