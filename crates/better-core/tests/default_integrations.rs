//! Manifest validation for `default_integrations`.
//!
//! A manifest is untrusted input. These tests assert both halves of that: a
//! fixture declaring every representable kind parses, and each individual way a
//! declaration can be wrong is rejected with its own error rather than reaching
//! a planner.

use better_core::defaults::{AdapterId, DefaultsValue, IntegrationKind};
use better_core::{
    ComponentManifest, HealthPrerequisite, IntegrationExclusivity, ManifestError,
    RequiredPrivilege, RestorePolicy, SessionEffect,
};
use std::collections::HashSet;

const EVERY_KIND: &str = include_str!("fixtures/every-integration-kind.yaml");

fn manifest() -> ComponentManifest {
    ComponentManifest::parse_yaml(EVERY_KIND).expect("the every-kind fixture must be valid")
}

#[test]
fn a_manifest_declaring_every_integration_kind_is_accepted() {
    let manifest = manifest();
    let declared: HashSet<IntegrationKind> = manifest
        .default_integrations
        .iter()
        .map(|integration| integration.kind)
        .collect();

    assert_eq!(declared.len(), IntegrationKind::ALL.len());
    for kind in IntegrationKind::ALL {
        assert!(declared.contains(&kind), "fixture is missing {kind:?}");
    }
}

#[test]
fn every_declaration_property_survives_parsing() {
    let manifest = manifest();
    let integration = manifest
        .default_integrations
        .iter()
        .find(|integration| integration.id.as_str() == "default-file-manager")
        .expect("fixture declares the file manager integration");

    assert_eq!(integration.kind, IntegrationKind::ApplicationHandler);
    assert_eq!(integration.exclusivity, IntegrationExclusivity::Exclusive);
    assert_eq!(
        integration.target.desired,
        DefaultsValue::DesktopEntry("io.betteros.Files.desktop".to_string())
    );
    assert_eq!(integration.target.keys, vec!["inode/directory".to_string()]);
    assert_eq!(integration.platforms, vec!["ubuntu", "zorin"]);
    assert_eq!(integration.sessions, vec!["gnome"]);
    assert_eq!(integration.apply_adapter, AdapterId::XdgDefaultApp);
    assert_eq!(integration.verify_adapter, AdapterId::XdgEffectiveDefault);
    assert_eq!(integration.restore_policy, RestorePolicy::CapturedValue);
    assert_eq!(integration.privileges, RequiredPrivilege::User);
    assert_eq!(integration.session_effect, SessionEffect::Immediate);
    assert_eq!(
        integration.health_prerequisites,
        vec![
            HealthPrerequisite::Installed,
            HealthPrerequisite::Enabled,
            HealthPrerequisite::Healthy
        ]
    );
}

#[test]
fn a_manifest_without_declarations_claims_no_integrations() {
    let input = EVERY_KIND
        .split("default_integrations:")
        .next()
        .expect("fixture has a declarations block");
    let manifest = ComponentManifest::parse_yaml(input).expect("the rest of the manifest is valid");

    assert!(manifest.default_integrations.is_empty());
}

#[test]
fn a_declaration_applies_only_to_its_declared_platform_and_session() {
    let manifest = manifest();
    let integration = &manifest.default_integrations[0];

    assert!(integration.applies_to("zorin", "gnome"));
    assert!(integration.applies_to("Ubuntu", "GNOME"));
    assert!(!integration.applies_to("fedora", "gnome"));
    assert!(!integration.applies_to("ubuntu", "kde"));
}

fn rejected(from: &str, to: &str) -> ManifestError {
    let input = EVERY_KIND.replacen(from, to, 1);
    ComponentManifest::parse_yaml(&input).expect_err("this declaration must be rejected")
}

#[test]
fn rejects_an_invalid_integration_id() {
    assert!(matches!(
        rejected("id: default-file-manager", "id: Default File Manager"),
        ManifestError::InvalidIntegrationId(_)
    ));
}

#[test]
fn rejects_a_duplicate_integration_id() {
    assert!(matches!(
        rejected("id: archive-handler-group", "id: default-file-manager"),
        ManifestError::DuplicateIntegration(_)
    ));
}

#[test]
fn rejects_an_unknown_integration_kind() {
    assert!(matches!(
        rejected("kind: application-handler", "kind: wallpaper-provider"),
        ManifestError::Parse(_)
    ));
}

#[test]
fn rejects_an_unknown_adapter_id() {
    assert!(matches!(
        rejected(
            "apply_adapter: xdg-default-app",
            "apply_adapter: run-my-script"
        ),
        ManifestError::Parse(_)
    ));
}

#[test]
fn rejects_a_declaration_with_no_supported_platform() {
    assert!(matches!(
        rejected("platforms: [ubuntu, zorin]", "platforms: []"),
        ManifestError::MissingIntegrationField("platforms")
    ));
}

#[test]
fn rejects_a_declaration_with_no_supported_session() {
    assert!(matches!(
        rejected("sessions: [gnome]", "sessions: []"),
        ManifestError::MissingIntegrationField("sessions")
    ));
}

#[test]
fn rejects_a_declaration_with_no_target_key() {
    assert!(matches!(
        rejected("keys: [inode/directory]", "keys: []"),
        ManifestError::MissingIntegrationField("target.keys")
    ));
}

#[test]
fn rejects_a_repeated_target_key() {
    assert!(matches!(
        rejected(
            "keys: [application/zip, application/x-tar]",
            "keys: [application/zip, application/zip]"
        ),
        ManifestError::InvalidIntegrationTargetKey { .. }
    ));
}

#[test]
fn rejects_an_empty_target_key() {
    assert!(matches!(
        rejected("keys: [inode/directory]", "keys: ['   ']"),
        ManifestError::InvalidIntegrationTargetKey { .. }
    ));
}

#[test]
fn rejects_a_value_the_kind_cannot_carry() {
    // A global shortcut is a list of accelerators, not a desktop entry.
    assert!(matches!(
        rejected(
            "        type: text_list\n        value: [\"<Super>e\"]",
            "        type: desktop_entry\n        value: io.betteros.Files.desktop"
        ),
        ManifestError::IntegrationValueMismatch {
            kind: IntegrationKind::GlobalShortcut,
            ..
        }
    ));
}

#[test]
fn rejects_a_desktop_entry_that_is_not_a_desktop_entry() {
    assert!(matches!(
        rejected(
            "value: io.betteros.Files.desktop",
            "value: /usr/bin/rm -rf --no-preserve-root /"
        ),
        ManifestError::IntegrationValueMismatch { .. }
    ));
}

#[test]
fn rejects_an_adapter_that_cannot_serve_the_kind() {
    assert!(matches!(
        rejected(
            "apply_adapter: xdg-default-app",
            "apply_adapter: gnome-keybinding"
        ),
        ManifestError::IntegrationAdapterMismatch {
            adapter: AdapterId::GnomeKeybinding,
            kind: IntegrationKind::ApplicationHandler,
            ..
        }
    ));
}

#[test]
fn rejects_a_verify_only_adapter_named_as_the_apply_adapter() {
    assert!(matches!(
        rejected(
            "apply_adapter: xdg-default-app",
            "apply_adapter: xdg-effective-default"
        ),
        ManifestError::ReadOnlyApplyAdapter {
            adapter: AdapterId::XdgEffectiveDefault,
            ..
        }
    ));
}
