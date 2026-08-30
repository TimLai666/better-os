//! The default-application adapter against real `mimeapps.list` content.
//!
//! The file is the user's, so the tests that matter are the ones about what
//! survives a write: comments, unknown groups, and every association the change
//! was not about.

use app_chooser_core::AssociationStore;
use better_core::defaults::{
    AdapterId, DefaultIntegration, DefaultsValue, IntegrationExclusivity, IntegrationId,
    IntegrationKind, IntegrationTarget, ObservedValue, RequiredPrivilege, RestorePolicy,
    SessionEffect,
};
use better_core::manifest::ComponentId;
use defaults_platform::{
    AdapterRequest, DefaultsAdapter, VerifyOutcome, WriteOutcome, WriteValue, XdgDefaultAppAdapter,
};

const HAND_EDITED: &str = "# written by hand, do not tidy\n\
     \n\
     [Added Associations]\n\
     text/plain=vim.desktop;nano.desktop;\n\
     \n\
     [Default Applications]\n\
     inode/directory=org.gnome.Nautilus.desktop\n\
     text/html=firefox.desktop\n";

fn integration(keys: &[&str]) -> DefaultIntegration {
    DefaultIntegration {
        id: IntegrationId::new("default-file-manager").unwrap(),
        kind: IntegrationKind::ApplicationHandler,
        exclusivity: IntegrationExclusivity::Exclusive,
        target: IntegrationTarget {
            desired: DefaultsValue::DesktopEntry("io.betteros.Files.desktop".to_string()),
            keys: keys.iter().map(|key| key.to_string()).collect(),
        },
        platforms: vec!["zorin".to_string()],
        sessions: vec!["gnome".to_string()],
        apply_adapter: AdapterId::XdgDefaultApp,
        verify_adapter: AdapterId::XdgDefaultApp,
        restore_policy: RestorePolicy::CapturedValue,
        privileges: RequiredPrivilege::User,
        session_effect: SessionEffect::Immediate,
        health_prerequisites: Vec::new(),
    }
}

struct Fixture {
    directory: tempfile::TempDir,
}

impl Fixture {
    fn new(contents: Option<&str>) -> Self {
        let directory = tempfile::tempdir().unwrap();
        if let Some(contents) = contents {
            std::fs::write(directory.path().join("mimeapps.list"), contents).unwrap();
        }
        Self { directory }
    }

    fn adapter(&self) -> XdgDefaultAppAdapter {
        XdgDefaultAppAdapter::new(
            AdapterId::XdgDefaultApp,
            AssociationStore::new(
                self.directory.path().join("mimeapps.list"),
                self.directory.path().join("rollback"),
            ),
        )
    }

    fn contents(&self) -> String {
        std::fs::read_to_string(self.directory.path().join("mimeapps.list")).unwrap()
    }
}

fn nautilus() -> ObservedValue {
    ObservedValue::Set {
        value: DefaultsValue::DesktopEntry("org.gnome.Nautilus.desktop".to_string()),
    }
}

fn better_files() -> ObservedValue {
    ObservedValue::Set {
        value: DefaultsValue::DesktopEntry("io.betteros.Files.desktop".to_string()),
    }
}

#[test]
fn reads_the_default_the_user_file_declares() {
    let fixture = Fixture::new(Some(HAND_EDITED));
    let component = ComponentId::new("better-files").unwrap();
    let integration = integration(&["inode/directory"]);

    let observed = fixture
        .adapter()
        .read(&AdapterRequest::new(&component, &integration));

    assert_eq!(observed, nautilus());
}

#[test]
fn a_type_the_file_says_nothing_about_is_unset_rather_than_unknown() {
    let fixture = Fixture::new(Some(HAND_EDITED));
    let component = ComponentId::new("better-files").unwrap();
    let integration = integration(&["application/zip"]);

    assert_eq!(
        fixture
            .adapter()
            .read(&AdapterRequest::new(&component, &integration)),
        ObservedValue::Unset
    );
}

#[test]
fn a_missing_file_reads_as_nothing_set_rather_than_as_an_error() {
    let fixture = Fixture::new(None);
    let component = ComponentId::new("better-files").unwrap();
    let integration = integration(&["inode/directory"]);

    assert_eq!(
        fixture
            .adapter()
            .read(&AdapterRequest::new(&component, &integration)),
        ObservedValue::Unset
    );
}

#[test]
fn a_key_that_is_not_a_mime_type_is_unsupported_rather_than_guessed() {
    let fixture = Fixture::new(Some(HAND_EDITED));
    let component = ComponentId::new("better-files").unwrap();
    let integration = integration(&["not a mime type"]);

    assert!(matches!(
        fixture
            .adapter()
            .read(&AdapterRequest::new(&component, &integration)),
        ObservedValue::Unsupported { .. }
    ));
}

#[test]
fn applying_changes_one_line_and_leaves_the_rest_of_the_file_alone() {
    let fixture = Fixture::new(Some(HAND_EDITED));
    let component = ComponentId::new("better-files").unwrap();
    let integration = integration(&["inode/directory"]);
    let request = AdapterRequest::new(&component, &integration);
    let mut adapter = fixture.adapter();

    assert_eq!(adapter.apply(&request), WriteOutcome::Written);

    let after = fixture.contents();
    let before_lines: Vec<&str> = HAND_EDITED.lines().collect();
    let after_lines: Vec<&str> = after.lines().collect();
    assert_eq!(before_lines.len(), after_lines.len());
    let changed: Vec<usize> = before_lines
        .iter()
        .zip(&after_lines)
        .enumerate()
        .filter(|(_, (left, right))| left != right)
        .map(|(index, _)| index)
        .collect();
    assert_eq!(changed.len(), 1);
    assert_eq!(
        after_lines[changed[0]],
        "inode/directory=io.betteros.Files.desktop"
    );
    assert!(after.contains("# written by hand, do not tidy"));
    assert!(after.contains("text/plain=vim.desktop;nano.desktop;"));
}

#[test]
fn a_change_is_verified_by_reading_it_back() {
    let fixture = Fixture::new(Some(HAND_EDITED));
    let component = ComponentId::new("better-files").unwrap();
    let integration = integration(&["inode/directory"]);
    let request = AdapterRequest::new(&component, &integration);
    let mut adapter = fixture.adapter();

    adapter.apply(&request);

    assert!(matches!(
        adapter.verify(&request, &better_files()),
        VerifyOutcome::Matches { .. }
    ));
    assert!(matches!(
        adapter.verify(&request, &nautilus()),
        VerifyOutcome::Differs { .. }
    ));
}

#[test]
fn restoring_the_captured_value_puts_the_previous_owner_back() {
    let fixture = Fixture::new(Some(HAND_EDITED));
    let component = ComponentId::new("better-files").unwrap();
    let integration = integration(&["inode/directory"]);
    let request = AdapterRequest::new(&component, &integration);
    let mut adapter = fixture.adapter();
    let captured = adapter.read(&request);

    adapter.apply(&request);
    assert_eq!(adapter.restore(&request, &captured), WriteOutcome::Written);

    assert_eq!(adapter.read(&request), nautilus());
    assert_eq!(fixture.contents(), HAND_EDITED);
}

#[test]
fn restoring_a_setting_that_held_nothing_asks_for_manual_action_rather_than_inventing_one() {
    let fixture = Fixture::new(Some(HAND_EDITED));
    let component = ComponentId::new("better-files").unwrap();
    let integration = integration(&["application/zip"]);
    let request = AdapterRequest::new(&component, &integration);
    let mut adapter = fixture.adapter();

    adapter.apply(&request);
    let outcome = adapter.restore(&request, &ObservedValue::Unset);

    assert!(matches!(
        &outcome,
        WriteOutcome::ManualActionRequired { reason, .. }
            if reason == "xdg.clearing_a_default_is_not_supported"
    ));
    // The association Better OS wrote is still there rather than half removed.
    assert!(
        fixture
            .contents()
            .contains("application/zip=io.betteros.Files.desktop")
    );
}

#[test]
fn applying_a_group_sets_every_declared_type() {
    let fixture = Fixture::new(Some(HAND_EDITED));
    let component = ComponentId::new("better-files").unwrap();
    let integration = integration(&["application/zip", "application/x-tar"]);
    let request = AdapterRequest::new(&component, &integration);
    let mut adapter = fixture.adapter();

    assert_eq!(adapter.apply(&request), WriteOutcome::Written);

    assert_eq!(adapter.read(&request), better_files());
    let contents = fixture.contents();
    assert!(contents.contains("application/zip=io.betteros.Files.desktop"));
    assert!(contents.contains("application/x-tar=io.betteros.Files.desktop"));
}

#[test]
fn a_group_whose_types_disagree_is_unknown_rather_than_one_of_them() {
    let fixture = Fixture::new(Some(HAND_EDITED));
    let component = ComponentId::new("better-files").unwrap();
    let integration = integration(&["inode/directory", "application/zip"]);

    assert!(matches!(
        fixture
            .adapter()
            .read(&AdapterRequest::new(&component, &integration)),
        ObservedValue::Unknown { .. }
    ));
}

#[test]
fn applying_what_is_already_there_writes_nothing() {
    let fixture = Fixture::new(Some(
        "[Default Applications]\ninode/directory=io.betteros.Files.desktop\n",
    ));
    let component = ComponentId::new("better-files").unwrap();
    let integration = integration(&["inode/directory"]);
    let mut adapter = fixture.adapter();

    assert_eq!(
        adapter.apply(&AdapterRequest::new(&component, &integration)),
        WriteOutcome::AlreadyCorrect
    );
}

#[test]
fn the_read_only_adapter_refuses_to_write() {
    let fixture = Fixture::new(Some(HAND_EDITED));
    let component = ComponentId::new("better-files").unwrap();
    let integration = integration(&["inode/directory"]);
    let mut adapter = XdgDefaultAppAdapter::read_only(AssociationStore::new(
        fixture.directory.path().join("mimeapps.list"),
        fixture.directory.path().join("rollback"),
    ));

    let outcome = adapter.write(
        &AdapterRequest::new(&component, &integration),
        &WriteValue::Set {
            value: DefaultsValue::DesktopEntry("io.betteros.Files.desktop".to_string()),
        },
    );

    assert!(matches!(
        &outcome,
        WriteOutcome::ManualActionRequired { reason, .. } if reason == "xdg.read_only_adapter"
    ));
    assert_eq!(fixture.contents(), HAND_EDITED);
}
