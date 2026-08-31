use crate::{
    app::{demo_manager, translated_component},
    i18n::{Locale, copy},
    layout::{ActionLayout, MIN_WINDOW_WIDTH, action_layout},
    model::ComponentInfo,
};
use better_core::{ComponentIcon, ComponentId};
use manager_core::{
    ComponentStatus, DesiredOperation, ManagerSettings, ManagerState, RestartRequirement,
    StoredTheme,
};

#[test]
fn required_visible_copy_exists_in_both_locales() {
    for locale in [Locale::EnUs, Locale::ZhTw] {
        let c = copy(locale);
        for value in [
            c.overview,
            c.components,
            c.updates,
            c.health,
            c.activity,
            c.settings,
            c.review_changes,
            c.install_updates,
            c.applying_settings,
            c.checking_works,
            c.restore_previous,
            c.ready_to_install,
            c.storage_error,
            c.release_notes,
            c.required_disk_space,
        ] {
            assert!(!value.trim().is_empty());
        }
    }
}

#[test]
fn update_all_uses_the_same_core_plan_as_the_cli_path() {
    let (manager, state) = demo_manager();
    let plan = manager
        .plan_all(&state)
        .expect("demo catalog must be plannable");

    assert!(plan.is_dry_run());
    assert_eq!(plan.steps().len(), 1);
    assert_eq!(plan.steps()[0].operation, DesiredOperation::Update);
    assert_eq!(
        manager.status(&state, &plan.steps()[0].component).unwrap(),
        ComponentStatus::UpdateAvailable
    );
}

#[test]
fn an_untranslated_component_is_presented_from_its_own_manifest() {
    let manifest = better_core::ComponentManifest::parse_yaml(
        "schema_version: 2\nid: third-party-tool\ndisplay_name: Third Party Tool\ncomponent_type: enhancement\nversion: 2.0.0\nsummary: Speeds up an unrelated desktop workflow\nicon: launcher\nrestart: reboot\nreplaces:\n  - org.example.Old\ntargets:\n  distributions: [ubuntu]\n  releases: ['24.04']\n  architectures: [amd64]\nartifacts:\n  - release: '24.04'\n    architecture: amd64\n    url: https://example.com/third-party-tool_2.0.0_ubuntu-24.04_amd64.deb\n    sha256: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n    release_asset: third-party-tool_2.0.0_ubuntu-24.04_amd64.deb\nlifecycle:\n  install: mock-install\n  enable: mock-enable\n  disable: mock-disable\n  remove: mock-remove\n  rollback: mock-rollback\n",
    )
    .expect("the manifest must be valid");

    let info = ComponentInfo::present(
        &manifest,
        None,
        ComponentStatus::Available,
        translated_component(Locale::EnUs, &manifest.id),
    );

    assert_eq!(info.name, "Third Party Tool");
    assert_eq!(info.summary, "Speeds up an unrelated desktop workflow");
    assert_eq!(info.detail, info.summary);
    assert_eq!(info.icon, ComponentIcon::Launcher);
    assert_eq!(info.restart_requirement, RestartRequirement::Reboot);
    assert_eq!(info.replaces, vec!["org.example.Old".to_string()]);
    assert_eq!(info.element_id("install"), "install-third-party-tool");
}

#[test]
fn a_shipped_component_keeps_its_translated_name_in_both_locales() {
    let (manager, _) = demo_manager();
    let manifest = manager
        .manifests()
        .find(|manifest| manifest.id.as_str() == "better-monitor")
        .expect("the demo catalog must carry the monitor");

    for locale in [Locale::EnUs, Locale::ZhTw] {
        let info = ComponentInfo::present(
            manifest,
            None,
            ComponentStatus::Available,
            translated_component(locale, &manifest.id),
        );
        assert_eq!(info.name, copy(locale).monitor_name);
        assert_eq!(info.summary, copy(locale).monitor_purpose);
        assert_eq!(info.icon, ComponentIcon::Monitor);
        assert_eq!(
            info.restart_requirement,
            RestartRequirement::RestartApplication
        );
        assert_eq!(info.enhances, vec!["gnome-system-monitor".to_string()]);
    }
}

#[test]
fn every_catalog_component_is_presentable() {
    let (manager, state) = demo_manager();

    for manifest in manager.manifests() {
        let status = manager
            .status(&state, &manifest.id)
            .expect("every catalog component must resolve a status");
        let info = ComponentInfo::present(
            manifest,
            state.component(&manifest.id),
            status,
            translated_component(Locale::EnUs, &manifest.id),
        );
        assert!(!info.name.trim().is_empty());
        assert!(!info.summary.trim().is_empty());
    }
}

#[test]
fn every_restart_requirement_has_copy_in_both_locales() {
    for locale in [Locale::EnUs, Locale::ZhTw] {
        let c = copy(locale);
        for label in [
            c.not_declared,
            c.restart_not_required,
            c.restart_application,
            c.restart_log_out,
            c.restart_reboot,
            c.replaces_label,
            c.enhances_label,
        ] {
            assert!(!label.trim().is_empty());
        }
    }
}

#[test]
fn a_transaction_reports_the_widest_interruption_it_requires() {
    // The example manifest is a schema fixture, not a shipped component, so
    // this test builds its own catalog around it instead of finding it in the
    // built-in one.
    let manifest = better_core::manifest::ComponentManifest::parse_yaml(include_str!(
        "../../../components/manifests/better-files-example.yaml"
    ))
    .expect("the example manifest must stay valid");
    let manager = manager_core::Manager::probe(
        better_core::manifest::ComponentCatalog::from_manifests(vec![manifest])
            .expect("a one-entry catalog must build"),
        &manager_platform::MockPlatform::default(),
    )
    .expect("the mock platform always reports a profile");
    let state = ManagerState::default();
    let plan = manager
        .plan(
            &state,
            &ComponentId::new("better-files-example").expect("id must be valid"),
            DesiredOperation::Install,
        )
        .expect("the example component must be plannable");

    assert_eq!(plan.restart_requirement(), RestartRequirement::LogOut);
    assert_eq!(plan.replaces(), vec!["org.gnome.Nautilus".to_string()]);
}

#[test]
fn an_unconfigured_manager_opens_dark() {
    assert_eq!(ManagerSettings::default().theme, StoredTheme::Dark);
}

#[test]
fn state_saved_before_the_theme_setting_existed_loads_dark() {
    let legacy = serde_json::json!({
        "schema_version": 1,
        "revision": 3,
        "components": {},
        "activity": [],
        "settings": {
            "release_channel": "stable",
            "locale": "system",
            "check_updates": true,
            "auto_download": false,
            "diagnostic_logs": true,
            "onboarding_complete": true,
            "component_filter": "all"
        },
        "active_operation": null
    });

    let state: ManagerState =
        serde_json::from_value(legacy).expect("a pre-theme state file must still load");
    assert_eq!(state.settings.theme, StoredTheme::Dark);
}

#[test]
fn every_theme_choice_has_copy_in_both_locales() {
    for locale in [Locale::EnUs, Locale::ZhTw] {
        let c = copy(locale);
        for label in [
            c.appearance,
            c.appearance_description,
            c.dark_theme,
            c.light_theme,
            c.system_default,
        ] {
            assert!(!label.trim().is_empty());
        }
    }
}

#[test]
fn localized_long_actions_wrap_at_every_supported_scale() {
    for locale in [Locale::EnUs, Locale::ZhTw] {
        let c = copy(locale);
        let longest = [
            c.install_updates,
            c.restore_previous,
            c.checking_works,
            c.manual_recovery_required,
        ]
        .iter()
        .map(|label| label.chars().count())
        .max()
        .unwrap();
        for scale in [1.0, 1.25, 1.5] {
            let expected_at_minimum = match locale {
                Locale::EnUs => ActionLayout::Wrapped,
                Locale::ZhTw if scale == 1.0 => ActionLayout::Inline,
                Locale::ZhTw | Locale::System => ActionLayout::Wrapped,
            };
            assert_eq!(
                action_layout(MIN_WINDOW_WIDTH, scale, longest),
                expected_at_minimum
            );
            assert_eq!(action_layout(1920.0, scale, longest), ActionLayout::Inline);
        }
    }
}

#[test]
fn synthetic_long_translations_wrap_at_the_supported_minimum_size() {
    let synthetic_translation =
        "Install every available component update and restore the previous version if checks fail";
    for scale in [1.0, 1.25, 1.5] {
        assert_eq!(
            action_layout(
                MIN_WINDOW_WIDTH,
                scale,
                synthetic_translation.chars().count()
            ),
            ActionLayout::Wrapped
        );
    }
}

#[test]
fn every_real_failure_reason_has_copy_in_both_locales() {
    // A real transaction can fail in ways the simulation never could. Each of
    // those needs words a person can act on, in both languages, or the screen
    // falls back to saying nothing useful.
    for locale in [Locale::EnUs, Locale::ZhTw] {
        let c = copy(locale);
        for text in [
            c.evidence_download_network,
            c.evidence_checksum_mismatch,
            c.evidence_daemon_unavailable,
            c.evidence_polkit_denied,
            c.evidence_restore_artifact_missing,
            c.evidence_apt_busy,
            c.evidence_apt_failed,
            c.evidence_health_failed,
            c.evidence_state_drift,
            c.evidence_daemon_refused,
            c.demo_mode_banner,
            c.downloading_progress,
            c.check_host_reconciliation,
        ] {
            assert!(!text.is_empty(), "{locale:?} is missing a failure reason");
        }
    }
}
