//! Always Use and its rollback, run against real `mimeapps.list` files.
//!
//! The fixtures are the shapes a user's file actually takes: hand edited with
//! comments and section notes, written by a tool that emitted its groups in an
//! order nobody expected and repeated one of them, CRLF, missing its final
//! newline, and empty. Every one of them is a file Better OS must be able to
//! change one line of and then put back exactly as it found it.

use std::path::{Path, PathBuf};

use app_catalog_core::{ApplicationRecord, DesktopFile, DesktopId, EntryScope, MimeType, NoProbe};
use app_chooser_core::{AssociationStore, DEFAULT_APPLICATIONS, MimeAppsFile};

fn fixture_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn fixture(name: &str) -> String {
    std::fs::read_to_string(fixture_path(name)).expect("fixture is readable")
}

const FIXTURES: [&str; 6] = [
    "hand-edited.list",
    "weird-ordering.list",
    "crlf.list",
    "no-final-newline.list",
    "empty.list",
    "comments-everywhere.list",
];

fn mime(value: &str) -> MimeType {
    MimeType::parse(value).expect("valid mime type")
}

fn id(value: &str) -> DesktopId {
    DesktopId::new(value).expect("valid desktop id")
}

fn record(desktop_id: &str, mime_types: &str) -> ApplicationRecord {
    let body = format!(
        "[Desktop Entry]\nType=Application\nName=App\nExec=app %U\nMimeType={mime_types}\n"
    );
    let file = DesktopFile::parse(&body).expect("valid entry");
    ApplicationRecord::from_desktop_file(
        id(desktop_id),
        PathBuf::from(format!("/usr/share/applications/{desktop_id}")),
        EntryScope::System,
        &file,
        &NoProbe,
    )
    .expect("valid record")
}

struct Sandbox {
    _dir: tempfile::TempDir,
    store: AssociationStore,
    original: String,
}

impl Sandbox {
    fn with(fixture_name: &str) -> Self {
        let dir = tempfile::tempdir().expect("temp dir");
        let original = fixture(fixture_name);
        let path = dir.path().join("mimeapps.list");
        std::fs::write(&path, &original).expect("seed the sandbox");
        let store = AssociationStore::new(path, dir.path().join("rollback"));
        Self {
            _dir: dir,
            store,
            original,
        }
    }

    fn contents(&self) -> String {
        std::fs::read_to_string(self.store.path()).expect("read back")
    }
}

/// Lines that differ between two renderings, ignoring line endings, which are
/// asserted separately.
fn differing_lines(before: &str, after: &str) -> Vec<String> {
    let before: Vec<&str> = before.lines().collect();
    let after: Vec<&str> = after.lines().collect();
    let mut differences: Vec<String> = Vec::new();
    for index in 0..before.len().max(after.len()) {
        let left = before.get(index).copied();
        let right = after.get(index).copied();
        if left != right {
            differences.push(right.unwrap_or_default().to_string());
        }
    }
    differences
}

#[test]
fn every_fixture_round_trips_byte_for_byte_when_nothing_is_changed() {
    for name in FIXTURES {
        let text = fixture(name);
        assert_eq!(MimeAppsFile::parse(&text).render(), text, "{name}");
    }
}

#[test]
fn changing_an_existing_association_changes_exactly_one_line() {
    for (name, mime_type) in [
        ("hand-edited.list", "image/png"),
        ("weird-ordering.list", "application/pdf"),
        ("crlf.list", "image/png"),
        ("no-final-newline.list", "image/png"),
        ("comments-everywhere.list", "image/png"),
    ] {
        let sandbox = Sandbox::with(name);
        let outcome = sandbox
            .store
            .set_default(&mime(mime_type), &record("chosen.desktop", mime_type))
            .expect("set default");
        assert!(outcome.changed, "{name}");
        let after = sandbox.contents();
        assert_eq!(
            differing_lines(&sandbox.original, &after),
            vec![format!("{mime_type}=chosen.desktop")],
            "{name} must change exactly one line"
        );
        assert_eq!(
            after.matches("\r\n").count(),
            sandbox.original.matches("\r\n").count(),
            "{name} must keep its line endings"
        );
    }
}

#[test]
fn every_unrelated_association_survives_a_change() {
    let sandbox = Sandbox::with("hand-edited.list");
    let before = MimeAppsFile::parse(&sandbox.original).associations();
    sandbox
        .store
        .set_default(&mime("image/png"), &record("gimp.desktop", "image/png"))
        .expect("set default");
    let after = MimeAppsFile::parse(&sandbox.contents()).associations();

    for untouched in [
        "text/html",
        "x-scheme-handler/http",
        "x-scheme-handler/https",
        "image/jpeg",
    ] {
        let mime = mime(untouched);
        assert_eq!(
            before.default_for(&mime),
            after.default_for(&mime),
            "{untouched} must be untouched"
        );
    }
    assert_eq!(
        before.added_for(&mime("text/plain")),
        after.added_for(&mime("text/plain")),
        "added associations must be untouched"
    );
    assert_eq!(
        after.default_for(&mime("image/png")),
        Some(&id("gimp.desktop"))
    );
}

#[test]
fn a_rollback_restores_every_fixture_byte_for_byte() {
    for name in FIXTURES {
        for mime_type in ["image/png", "text/x-rust"] {
            let sandbox = Sandbox::with(name);
            let outcome = sandbox
                .store
                .set_default(&mime(mime_type), &record("chosen.desktop", mime_type))
                .expect("set default");
            sandbox.store.restore(&outcome.rollback).expect("restore");
            assert_eq!(
                sandbox.contents(),
                sandbox.original,
                "{name} must be byte-identical after a rollback of {mime_type}"
            );
        }
    }
}

#[test]
fn the_first_of_two_repeated_groups_receives_the_new_key() {
    // The weird-ordering fixture opens `[Default Applications]` twice. The
    // specification's first-wins rule means the new key belongs in the first
    // one; the second stays exactly where its author put it.
    let sandbox = Sandbox::with("weird-ordering.list");
    sandbox
        .store
        .set_default(&mime("text/x-rust"), &record("zed.desktop", "text/x-rust"))
        .expect("set default");
    let after = sandbox.contents();
    assert!(
        after.contains("application/pdf=org.gnome.Evince.desktop\ntext/x-rust=zed.desktop\n"),
        "the key belongs in the first group:\n{after}"
    );
    assert!(after.contains("[Some Other Desktop Group]\nNotAnAssociation=true\n"));
    assert!(after.contains("[Default Applications]\ntext/csv=libreoffice-calc.desktop\n"));
    assert_eq!(
        MimeAppsFile::parse(&after).count_keys(DEFAULT_APPLICATIONS, "text/csv"),
        1
    );
}

#[test]
fn a_comment_bearing_group_keeps_its_comments_in_place() {
    let sandbox = Sandbox::with("comments-everywhere.list");
    sandbox
        .store
        .set_default(&mime("text/x-rust"), &record("zed.desktop", "text/x-rust"))
        .expect("set default");
    let after = sandbox.contents();
    for comment in [
        "# leading comment",
        "   # indented comment",
        "# comment before the first key",
        "# comment between keys",
        "# trailing comment inside the group",
        "# comment before the next group",
    ] {
        assert!(after.contains(comment), "lost {comment}");
    }
    assert!(after.contains("image/png=eog.desktop\ntext/x-rust=zed.desktop\n"));
}

#[test]
fn a_removed_association_in_the_fixture_is_reported_not_edited() {
    let sandbox = Sandbox::with("weird-ordering.list");
    let outcome = sandbox
        .store
        .set_default(&mime("image/png"), &record("gimp.desktop", "image/png"))
        .expect("set default");
    assert!(
        outcome
            .warnings
            .contains(&app_chooser_core::AssociationWarning::ListedInRemovedAssociations)
    );
    assert!(
        sandbox
            .contents()
            .contains("[Removed Associations]\nimage/png=gimp.desktop;\n")
    );
}

#[test]
fn clearing_better_os_rollback_state_leaves_the_users_file_alone() {
    // Removing Better OS must not take the user's associations with it. The
    // rollback records live in Better OS's own directory; deleting that
    // directory is the whole of "clearing state", and the file is untouched.
    let sandbox = Sandbox::with("hand-edited.list");
    sandbox
        .store
        .set_default(&mime("image/png"), &record("gimp.desktop", "image/png"))
        .expect("set default");
    let after_change = sandbox.contents();
    std::fs::remove_dir_all(sandbox.store.rollback_dir()).expect("clear better-os state");
    assert!(sandbox.store.rollback_records().expect("list").is_empty());
    assert_eq!(sandbox.contents(), after_change);
}
