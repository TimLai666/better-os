use crate::{
    app::{demo_manager, translated_component},
    i18n::{Locale, copy},
    layout::{
        ActionLayout, MIN_CONTENT_WIDTH, MIN_READABLE_CHARACTERS, action_layout,
        characters_per_line,
    },
    model::{CatalogLine, ComponentInfo, host_line},
};
use better_core::{ComponentIcon, ComponentId};
use manager_core::catalog::{
    CatalogDegradation, CatalogSource, CatalogStatus, ManifestRejection, RejectionReason,
};
use manager_core::{
    ComponentStatus, DesiredOperation, ManagerSettings, ManagerState, RestartRequirement,
    StoredLocale, StoredTheme, SystemProfile,
};
use manager_platform::{SystemCapabilities, host::HostPlatform};

#[test]
fn required_visible_copy_exists_in_both_locales() {
    for locale in [Locale::EnUs, Locale::ZhTw] {
        let c = copy(locale);
        for value in [
            c.components,
            c.updates,
            c.health,
            c.activity,
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
fn an_unconfigured_manager_follows_the_desktop() {
    assert_eq!(ManagerSettings::default().theme, StoredTheme::System);
}

#[test]
fn state_saved_before_the_theme_setting_existed_follows_the_desktop() {
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
    assert_eq!(state.settings.theme, StoredTheme::System);
}

#[test]
fn a_theme_this_version_cannot_read_keeps_the_rest_of_the_state() {
    let state: ManagerState = serde_json::from_value(serde_json::json!({
        "schema_version": 2,
        "revision": 3,
        "settings": {
            "release_channel": "stable",
            "locale": "zh_tw",
            "theme": "sepia",
            "check_updates": true,
            "auto_download": false,
            "diagnostic_logs": true,
            "onboarding_complete": true,
            "component_filter": "all"
        }
    }))
    .expect("an unknown theme must not make the state unreadable");
    assert_eq!(state.settings.theme, StoredTheme::System);
    assert_eq!(state.settings.locale, StoredLocale::ZhTw);
    assert_eq!(state.revision, 3);
}

#[test]
fn a_stored_theme_choice_is_kept() {
    for theme in StoredTheme::ALL {
        let mut state = ManagerState::default();
        state.settings.theme = theme;
        let text = serde_json::to_string(&state).unwrap();
        let loaded: ManagerState = serde_json::from_str(&text).unwrap();
        assert_eq!(loaded.settings.theme, theme);
    }
}

#[test]
fn the_appearance_card_says_the_desktop_is_the_default() {
    assert_eq!(
        crate::i18n::shell(Locale::EnUs).appearance_description,
        "Follows your desktop's light or dark mode unless you pick one."
    );
    assert_eq!(
        crate::i18n::shell(Locale::ZhTw).appearance_description,
        "預設跟著桌面切換淺色或深色，也可以固定用其中一種。"
    );
}

#[test]
fn every_theme_choice_has_copy_in_both_locales() {
    for locale in [Locale::EnUs, Locale::ZhTw] {
        let s = crate::i18n::shell(locale);
        for label in [s.appearance, s.appearance_description]
            .into_iter()
            .chain(crate::settings_page::appearance_labels(locale))
        {
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
            // The control center's narrowest content column (960 less the
            // compact pane and margins) holds them on one line until the
            // person scales up to 150%, where the row wraps.
            if scale >= 1.5 {
                assert_eq!(
                    action_layout(MIN_CONTENT_WIDTH, scale, longest),
                    ActionLayout::Wrapped
                );
            }
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
                MIN_CONTENT_WIDTH,
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
            c.evidence_daemon_interrupted,
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
            c.evidence_user_units_failed,
            c.evidence_user_units_not_running,
            c.evidence_user_units_started,
            c.evidence_user_units_masked,
            c.evidence_user_units_no_session,
        ] {
            assert!(!text.is_empty(), "{locale:?} is missing a failure reason");
        }
    }
}

/// What the component page says about a component, and whether it is true.
///
/// A Zorin 18 machine with nothing installed showed "已安裝版本 0.2.4" — the
/// catalog's version under an installed heading — a red 異常 tag with no reason
/// anywhere on the page, and no way to remove anything. These lock the rows
/// that page is built from.
mod component_page {
    use super::*;
    use manager_core::{
        ComponentRecord, FailureRecord, HealthState, InstallProvenance, OperationStage,
    };

    fn manifest() -> better_core::ComponentManifest {
        let (manager, _) = demo_manager();
        manager
            .manifests()
            .find(|manifest| manifest.id.as_str() == "better-monitor")
            .expect("the demo catalog must carry the monitor")
            .clone()
    }

    fn present(record: Option<&ComponentRecord>, status: ComponentStatus) -> ComponentInfo {
        let manifest = manifest();
        ComponentInfo::present(
            &manifest,
            record,
            status,
            translated_component(Locale::ZhTw, &manifest.id),
        )
    }

    #[test]
    fn a_component_that_is_not_installed_never_reports_the_catalog_version_as_installed() {
        let info = present(None, ComponentStatus::Available);

        for locale in [Locale::EnUs, Locale::ZhTw] {
            let c = copy(locale);
            assert_eq!(info.installed_label(c.not_installed), c.not_installed);
            // The catalog version still has a home — under its own heading.
            assert_ne!(info.available_version, c.not_installed);
            assert_ne!(
                info.installed_label(c.not_installed),
                info.available_version
            );
        }
        assert!(!info.installed_outside_manager());
    }

    #[test]
    fn an_installed_component_reports_the_version_that_is_installed() {
        let record = ComponentRecord {
            installed_version: Some("0.1.0".to_string()),
            enabled: true,
            ..ComponentRecord::default()
        };
        let info = present(Some(&record), ComponentStatus::UpdateAvailable);

        assert_eq!(
            info.installed_label(copy(Locale::ZhTw).not_installed),
            "0.1.0"
        );
        assert_eq!(info.available_version, manifest().version.to_string());
        assert!(!info.installed_outside_manager());
    }

    #[test]
    fn a_component_apt_installed_is_shown_as_installed_and_says_who_installed_it() {
        let record = ComponentRecord {
            installed_version: Some("0.2.3".to_string()),
            provenance: InstallProvenance::Dpkg,
            enabled: true,
            ..ComponentRecord::default()
        };
        let info = present(Some(&record), ComponentStatus::UpdateAvailable);

        assert_eq!(
            info.installed_label(copy(Locale::ZhTw).not_installed),
            "0.2.3"
        );
        assert!(info.installed_outside_manager());
        // Nothing was captured for it, so the page must not offer a restore.
        assert!(!info.restore_available);
    }

    /// The record the field machine actually wrote. The service's reason lives
    /// inside the evidence key rather than in `detail`, so a page that only
    /// prefix-matched the key showed a generic sentence and dropped the words
    /// that said what was wrong.
    #[test]
    fn the_reason_the_service_gave_survives_even_when_it_was_recorded_inside_the_key() {
        let failure = FailureRecord {
            component: ComponentId::new("better-awake").unwrap(),
            stage: OperationStage::Installing,
            evidence: "daemon.error.plan_rejected:plan targets release 24.04 but this host is 18"
                .to_string(),
            detail: None,
            recovery: None,
        };

        let (key, detail) = failure.evidence_parts();
        assert_eq!(key, "daemon.error.plan_rejected");
        assert_eq!(
            detail,
            Some("plan targets release 24.04 but this host is 18")
        );
        for locale in [Locale::EnUs, Locale::ZhTw] {
            assert!(!copy(locale).evidence_plan_rejected.trim().is_empty());
        }

        // A key with nothing after it stays whole rather than gaining an empty
        // detail row.
        let bare = FailureRecord {
            evidence: "download.network".to_string(),
            ..failure
        };
        assert_eq!(bare.evidence_parts(), ("download.network", None));
    }

    #[test]
    fn a_recorded_failure_travels_to_the_page_whole_rather_than_as_a_tag() {
        let record = ComponentRecord {
            health: HealthState::Failed,
            failure: Some(FailureRecord {
                component: ComponentId::new("better-monitor").unwrap(),
                stage: OperationStage::Installing,
                evidence: "daemon.error.plan_rejected".to_string(),
                detail: Some("plan targets release 24.04 but this host is 18".to_string()),
                recovery: None,
            }),
            ..ComponentRecord::default()
        };
        let info = present(Some(&record), ComponentStatus::Failed);

        let failure = info.failure.expect("the page needs the failure itself");
        assert_eq!(failure.stage, OperationStage::Installing);
        assert_eq!(
            failure.detail.as_deref(),
            Some("plan targets release 24.04 but this host is 18")
        );
        // The field case exactly: a failure with nothing installed behind it.
        assert!(info.installed_version.is_none());
    }

    #[test]
    fn only_a_plan_that_touches_the_manager_warns_about_restarting_the_application() {
        let (manager, mut state) = demo_manager();
        let monitor = ComponentId::new("better-monitor").unwrap();
        let self_id = ComponentId::new("better-manager").unwrap();
        assert!(manager_core::is_self_component(&self_id));

        let monitor_plan = manager
            .plan(&state, &monitor, DesiredOperation::Update)
            .expect("the monitor must be updatable in the demo state");
        assert!(!crate::app::ManagerApp::updates_the_manager(
            monitor_plan.steps()
        ));

        // Put the manager one release behind so its own update can be planned.
        state.set_installed(self_id.clone(), "0.0.1", true);
        let self_plan = manager
            .plan(&state, &self_id, DesiredOperation::Update)
            .expect("the manager must be updatable");
        assert!(crate::app::ManagerApp::updates_the_manager(
            self_plan.steps()
        ));
    }

    #[test]
    fn the_manager_offers_no_removal_of_itself_and_says_why_in_both_locales() {
        let (manager, mut state) = demo_manager();
        let self_id = ComponentId::new("better-manager").unwrap();
        state.set_installed(self_id.clone(), "0.2.3", true);

        assert!(matches!(
            manager.plan(&state, &self_id, DesiredOperation::Remove),
            Err(manager_core::ManagerError::CannotRemoveSelf(_))
        ));
        for locale in [Locale::EnUs, Locale::ZhTw] {
            assert!(!copy(locale).cannot_remove_self.trim().is_empty());
        }
    }

    #[test]
    fn every_string_this_page_added_exists_in_both_locales_and_is_translated() {
        for locale in [Locale::EnUs, Locale::ZhTw] {
            let c = copy(locale);
            for value in [
                c.not_installed,
                c.installed_outside_manager,
                c.last_install_failed,
                c.try_install_again,
                c.remove_component,
                c.cannot_remove_self,
                c.self_update_title,
                c.self_update_detail,
            ] {
                assert!(!value.trim().is_empty(), "{locale:?} is missing a string");
            }
        }
        // Traditional Chinese must not fall back to the English wording.
        let en = copy(Locale::EnUs);
        let zh = copy(Locale::ZhTw);
        for (english, chinese) in [
            (en.not_installed, zh.not_installed),
            (en.last_install_failed, zh.last_install_failed),
            (en.try_install_again, zh.try_install_again),
            (en.remove_component, zh.remove_component),
            (en.cannot_remove_self, zh.cannot_remove_self),
            (en.self_update_detail, zh.self_update_detail),
        ] {
            assert_ne!(english, chinese);
        }
    }

    /// The new action labels never make the row worse than the ones already
    /// shipped: they wrap where the existing longest labels wrap and sit inline
    /// where those sit inline, at every supported window size and scale.
    #[test]
    fn the_new_action_labels_lay_out_no_worse_than_the_ones_already_shipped() {
        for locale in [Locale::EnUs, Locale::ZhTw] {
            let c = copy(locale);
            let new_longest = [c.try_install_again, c.remove_component, c.not_installed]
                .iter()
                .map(|label| label.chars().count())
                .max()
                .unwrap();
            let shipped_longest = [c.install_updates, c.restore_previous, c.checking_works]
                .iter()
                .map(|label| label.chars().count())
                .max()
                .unwrap();
            assert!(
                new_longest <= shipped_longest,
                "{locale:?} added a label longer than anything already on screen"
            );
            for width in [MIN_CONTENT_WIDTH, 1280.0, 1920.0] {
                for scale in [1.0, 1.25, 1.5] {
                    let shipped = action_layout(width, scale, shipped_longest);
                    let added = action_layout(width, scale, new_longest);
                    assert!(
                        added == ActionLayout::Inline || shipped == ActionLayout::Wrapped,
                        "{locale:?} at {width}x{scale} wraps a new label where nothing else wraps"
                    );
                }
            }
            // The explanations are prose in a text block, not buttons, so they
            // are only required to actually explain something.
            assert!(c.cannot_remove_self.chars().count() > 20);
            assert!(c.installed_outside_manager.chars().count() > 20);
        }
    }
}

/// The catalog states a user can actually land in, and what each one says.
///
/// These are the whole reason the refresh exists: a catalog that is behind has
/// to look different from one that is current, in both languages, without the
/// window having to be open to check.
mod catalog_state {
    use super::*;

    fn status(source: CatalogSource, degraded: Option<CatalogDegradation>) -> CatalogStatus {
        CatalogStatus {
            source,
            source_url: Some("https://example.com/manifests".to_string()),
            fetched_at_unix_seconds: Some(1_000_000),
            degraded,
            rejections: Vec::new(),
        }
    }

    #[test]
    fn a_binary_that_has_never_refreshed_is_shown_as_possibly_outdated() {
        for locale in [Locale::EnUs, Locale::ZhTw] {
            let line = CatalogLine::present(locale, &CatalogStatus::built_in(), 1_000_000);
            assert_eq!(line.age, copy(locale).catalog_never_updated);
            assert!(line.is_degraded());
            assert_eq!(line.warning, Some(copy(locale).catalog_degraded_never));
        }
    }

    #[test]
    fn a_catalog_fetched_in_this_session_carries_no_warning() {
        for locale in [Locale::EnUs, Locale::ZhTw] {
            let line =
                CatalogLine::present(locale, &status(CatalogSource::Remote, None), 1_000_000);
            assert_eq!(line.age, copy(locale).catalog_updated_just_now);
            assert!(!line.is_degraded());
            assert_eq!(line.warning, None);
        }
    }

    #[test]
    fn each_degraded_state_has_its_own_sentence_in_both_locales() {
        for locale in [Locale::EnUs, Locale::ZhTw] {
            let c = copy(locale);
            let cases = [
                (
                    CatalogSource::Cache,
                    CatalogDegradation::RefreshFailedUsingCache,
                    c.catalog_degraded_failed_cache,
                ),
                (
                    CatalogSource::BuiltIn,
                    CatalogDegradation::RefreshFailedUsingBuiltIn,
                    c.catalog_degraded_failed_built_in,
                ),
                (
                    CatalogSource::Remote,
                    CatalogDegradation::PartiallyRefreshed,
                    c.catalog_degraded_partial,
                ),
            ];
            let mut seen = Vec::new();
            for (source, degradation, expected) in cases {
                let line =
                    CatalogLine::present(locale, &status(source, Some(degradation)), 1_000_000);
                assert_eq!(line.warning, Some(expected));
                assert!(line.is_degraded());
                seen.push(expected);
            }
            // Four states including "never refreshed", four distinct sentences.
            seen.push(c.catalog_degraded_never);
            seen.sort_unstable();
            seen.dedup();
            assert_eq!(seen.len(), 4);
        }
    }

    #[test]
    fn the_age_of_a_catalog_is_reported_in_the_coarsest_useful_unit() {
        let base = status(CatalogSource::Cache, None);
        let at = |now: u64| CatalogLine::present(Locale::EnUs, &base, now).age;

        assert_eq!(at(1_000_030), "Checked for new versions moments ago");
        assert_eq!(at(1_000_000 + 60), "Checked for new versions a minute ago");
        assert_eq!(
            at(1_000_000 + 5 * 60),
            "Checked for new versions 5 minutes ago"
        );
        assert_eq!(at(1_000_000 + 3600), "Checked for new versions an hour ago");
        assert_eq!(
            at(1_000_000 + 3 * 3600),
            "Checked for new versions 3 hours ago"
        );
        assert_eq!(at(1_000_000 + 86_400), "Checked for new versions a day ago");
        assert_eq!(
            at(1_000_000 + 4 * 86_400),
            "Checked for new versions 4 days ago"
        );
        // A clock that moved backwards reads as recent, never as negative.
        assert_eq!(at(999_000), "Checked for new versions moments ago");

        let zh = |now: u64| CatalogLine::present(Locale::ZhTw, &base, now).age;
        assert_eq!(zh(1_000_030), "剛剛檢查過新版本");
        assert_eq!(zh(1_000_000 + 60), "1 分鐘前檢查過新版本");
        assert_eq!(zh(1_000_000 + 5 * 60), "5 分鐘前檢查過新版本");
        assert_eq!(zh(1_000_000 + 3 * 3600), "3 小時前檢查過新版本");
        assert_eq!(zh(1_000_000 + 4 * 86_400), "4 天前檢查過新版本");
    }

    /// Ticket 111: the line says when the list was checked, once. It used to
    /// read "Catalog downloaded just now · Updated moments ago".
    #[test]
    fn the_catalog_line_says_when_it_was_checked_once() {
        for locale in [Locale::EnUs, Locale::ZhTw] {
            for source in [CatalogSource::Remote, CatalogSource::Cache] {
                let line = CatalogLine::present(locale, &status(source, None), 1_000_000);
                assert_eq!(line.age, copy(locale).catalog_updated_just_now);
            }
        }
        assert_eq!(
            CatalogLine::present(Locale::ZhTw, &CatalogStatus::built_in(), 1).age,
            "還沒上網檢查過新版本"
        );
        assert_eq!(
            CatalogLine::present(Locale::EnUs, &CatalogStatus::built_in(), 1).age,
            "Not checked for new versions yet"
        );
    }

    #[test]
    fn refused_manifests_are_counted_on_screen_rather_than_dropped() {
        let mut degraded = status(
            CatalogSource::Remote,
            Some(CatalogDegradation::PartiallyRefreshed),
        );
        degraded.rejections = vec![
            ManifestRejection {
                file: "better-monitor.yaml".to_string(),
                reason: RejectionReason::Unreachable,
            },
            ManifestRejection {
                file: "better-files.yaml".to_string(),
                reason: RejectionReason::Invalid("bad".to_string()),
            },
        ];

        for locale in [Locale::EnUs, Locale::ZhTw] {
            let line = CatalogLine::present(locale, &degraded, 1_000_000);
            assert_eq!(line.rejected, 2);
            let sentence = line.rejected_line(locale).expect("the count is shown");
            assert!(sentence.contains('2'), "{sentence}");
            assert!(!sentence.contains("{n}"), "{sentence}");
        }
    }

    #[test]
    fn a_catalog_with_nothing_refused_shows_no_rejection_line() {
        let line = CatalogLine::present(Locale::EnUs, &status(CatalogSource::Remote, None), 1);
        assert_eq!(line.rejected, 0);
        assert_eq!(line.rejected_line(Locale::EnUs), None);
    }

    #[test]
    fn the_catalog_copy_exists_in_both_locales_and_never_falls_back_to_english() {
        for locale in [Locale::EnUs, Locale::ZhTw] {
            let c = copy(locale);
            for value in [
                c.catalog_never_updated,
                c.catalog_updated_just_now,
                c.catalog_updated_minute,
                c.catalog_updated_minutes,
                c.catalog_updated_hour,
                c.catalog_updated_hours,
                c.catalog_updated_day,
                c.catalog_updated_days,
                c.catalog_degraded_never,
                c.catalog_degraded_failed_cache,
                c.catalog_degraded_failed_built_in,
                c.catalog_degraded_partial,
                c.catalog_rejected,
                c.catalog_refresh,
                c.catalog_refreshing,
            ] {
                assert!(!value.trim().is_empty());
            }
            if locale == Locale::ZhTw {
                assert_ne!(c.catalog_refresh, copy(Locale::EnUs).catalog_refresh);
                assert_ne!(
                    c.catalog_degraded_never,
                    copy(Locale::EnUs).catalog_degraded_never
                );
            }
        }
    }
}

/// The seam that replaced `MockPlatform::default()` in the window.
///
/// The mock reports Ubuntu 24.04 amd64 on every machine, so before this a plan
/// built on a 22.04 or arm64 host named packages that host cannot use. This
/// asserts which probe each mode gets without reading the machine the tests
/// happen to run on.
#[test]
fn a_real_window_plans_from_the_host_and_a_demo_keeps_the_fixed_profile() {
    use manager_platform::host::ClientPlatform;

    assert!(matches!(
        crate::app::platform_for(manager_core::ExecutionMode::Real),
        ClientPlatform::Host(_)
    ));
    assert!(matches!(
        crate::app::platform_for(manager_core::ExecutionMode::Mock),
        ClientPlatform::Mock(_)
    ));

    let ClientPlatform::Host(host) = crate::app::platform_for(manager_core::ExecutionMode::Real)
    else {
        panic!("a real window must plan from the host");
    };
    assert_eq!(
        host.os_release_path(),
        Some(std::path::Path::new("/etc/os-release"))
    );
}

#[test]
fn the_window_plans_for_the_release_and_architecture_the_host_reports() {
    // A jammy arm64 derivative: both halves differ from the mock's answer, so
    // either one still being 24.04 or amd64 fails this.
    let platform = manager_platform::host::HostPlatform::from_fixture(
        "NAME=\"Zorin OS\"\nID=zorin\nVERSION_ID=\"17\"\nUBUNTU_CODENAME=jammy\n",
        "arm64",
    );
    let (manager, error) =
        crate::app::probe_manager(manager_core::catalog::built_in_catalog(), &platform);

    assert_eq!(error, None);
    assert_eq!(manager.profile().release, "22.04");
    assert_eq!(manager.profile().architecture, "arm64");
    assert_eq!(manager.profile().distribution, "zorin");
}

#[test]
fn an_unidentifiable_host_becomes_a_stated_window_state_and_never_a_guess() {
    let platform = manager_platform::host::HostPlatform::from_fixture(
        "NAME=\"Fedora Linux\"\nID=fedora\nVERSION_ID=41\n",
        "amd64",
    );
    let (manager, error) =
        crate::app::probe_manager(manager_core::catalog::built_in_catalog(), &platform);

    assert_eq!(error, Some(crate::app::AppError::UnsupportedHost));
    // Not a fallback to the mock's release: a plan aimed at 24.04 on a machine
    // that is not running it is the defect this state exists to prevent.
    assert_ne!(manager.profile().release, "24.04");
    assert_eq!(manager.profile().release, "unknown");

    // And nothing is plannable, rather than plannable against the wrong target.
    assert!(
        manager
            .plan(
                &ManagerState::default(),
                &ComponentId::new("better-monitor").expect("id must be valid"),
                DesiredOperation::Install,
            )
            .is_err()
    );
}

/// The footer used to print the distribution ID beside the Ubuntu release a
/// plan is built for, so the project's own primary target read as "zorin
/// 24.04" — a Zorin version that does not exist. These are the profiles the
/// real host probe produces, not hand-written labels.
#[test]
fn the_footer_tells_a_host_apart_from_the_ubuntu_base_it_is_built_on() {
    let c = copy(Locale::EnUs);
    let zorin = HostPlatform::from_fixture(
        "NAME=\"Zorin OS\"\nID=zorin\nPRETTY_NAME=\"Zorin OS 18\"\nVERSION_ID=\"18\"\n\
         UBUNTU_CODENAME=noble\n",
        "amd64",
    )
    .profile()
    .expect("a noble-based host is supported");
    assert_eq!(
        host_line(&zorin, c.ubuntu_base),
        "Zorin OS 18 · Ubuntu 24.04 base"
    );
    assert_eq!(
        host_line(&zorin, copy(Locale::ZhTw).ubuntu_base),
        "Zorin OS 18 · Ubuntu 24.04 基礎"
    );

    // Plain Ubuntu is one fact and shows one: no base note, and no
    // "Ubuntu 24.04 · Ubuntu 24.04 base" either.
    let ubuntu = HostPlatform::from_fixture(
        "NAME=\"Ubuntu\"\nID=ubuntu\nVERSION_ID=\"24.04\"\nUBUNTU_CODENAME=noble\n",
        "amd64",
    )
    .profile()
    .expect("plain ubuntu 24.04 is supported");
    assert_eq!(host_line(&ubuntu, c.ubuntu_base), "Ubuntu 24.04");
}

#[test]
fn a_host_the_window_could_not_identify_names_no_release_at_all() {
    let line = host_line(
        &SystemProfile::unidentified(),
        copy(Locale::EnUs).ubuntu_base,
    );
    assert_eq!(line, "unknown");
    assert!(!line.contains("24.04"), "{line}");
}

#[test]
fn the_unsupported_host_state_has_copy_in_both_locales() {
    for locale in [Locale::EnUs, Locale::ZhTw] {
        let message = copy(locale).unsupported_host;
        assert!(!message.trim().is_empty());
        // It has to say what is supported, or the person is told only that
        // something is wrong.
        assert!(message.contains("22.04") && message.contains("24.04"));
    }
}

/// What the window says about itself, and what a manual update check reports.
///
/// Both are ticket 46's whole point: before it, nothing on screen or on the
/// command line named the version this manager is, and the only way to ask for
/// a fresh catalog was a button on another screen that said nothing about
/// updates afterwards.
mod version_and_update_check {
    use super::*;
    use crate::model::{MANAGER_VERSION, PROJECT_REPOSITORY, UpdateCheck};

    fn status(
        source: CatalogSource,
        degraded: Option<CatalogDegradation>,
        fetched_at: Option<u64>,
    ) -> CatalogStatus {
        CatalogStatus {
            source,
            source_url: Some("https://example.com/manifests".to_string()),
            fetched_at_unix_seconds: fetched_at,
            degraded,
            rejections: Vec::new(),
        }
    }

    #[test]
    fn the_about_section_states_the_built_version_and_the_host_it_planned_for() {
        let platform = manager_platform::host::HostPlatform::from_fixture(
            "NAME=\"Zorin OS\"\nID=zorin\nVERSION_ID=\"18\"\nUBUNTU_CODENAME=noble\n",
            "amd64",
        );
        let (manager, error) =
            crate::app::probe_manager(manager_core::catalog::built_in_catalog(), &platform);
        assert_eq!(error, None);

        // The version is the package's own, so a release bump moves it without
        // anything else being edited; Settings › About says it.
        let version = crate::i18n::shell(Locale::EnUs)
            .about_version
            .replace("{version}", MANAGER_VERSION);
        assert_eq!(
            version,
            format!("Better Manager {}", env!("CARGO_PKG_VERSION"))
        );
        assert_eq!(MANAGER_VERSION.split('.').count(), 3, "{MANAGER_VERSION}");
        // The host line Home's banner carries is what ticket 45's probe read,
        // not an assumption.
        let profile = manager.profile();
        assert_eq!(profile.architecture, "amd64");
        assert_eq!(profile.release, "24.04");
        assert_eq!(crate::settings_page::SOURCE_URL, PROJECT_REPOSITORY);
        assert!(PROJECT_REPOSITORY.starts_with("https://"));
    }

    #[test]
    fn a_host_the_client_could_not_identify_is_said_so_rather_than_guessed_at() {
        let platform = manager_platform::host::HostPlatform::from_fixture(
            "NAME=\"Fedora Linux\"\nID=fedora\nVERSION_ID=41\n",
            "amd64",
        );
        let (manager, error) =
            crate::app::probe_manager(manager_core::catalog::built_in_catalog(), &platform);
        assert_eq!(error, Some(crate::app::AppError::UnsupportedHost));

        let profile = manager.profile();
        let line = host_line(profile, copy(Locale::ZhTw).ubuntu_base);
        assert!(!line.contains("24.04"), "{line}");
        assert_eq!(profile.architecture, "unknown");
    }

    #[test]
    fn the_about_section_is_named_in_the_users_own_language() {
        for locale in [Locale::EnUs, Locale::ZhTw] {
            let s = crate::i18n::shell(locale);
            for value in [s.about, s.about_version, s.about_source, s.about_licenses] {
                assert!(!value.trim().is_empty(), "{locale:?} is missing a string");
            }
        }
        let en = crate::i18n::shell(Locale::EnUs);
        let zh = crate::i18n::shell(Locale::ZhTw);
        for (english, chinese) in [
            (en.about, zh.about),
            (en.about_source, zh.about_source),
            (en.about_licenses, zh.about_licenses),
            (en.privacy, zh.privacy),
        ] {
            assert_ne!(english, chinese);
        }
    }

    #[test]
    fn a_check_that_found_updates_says_how_many() {
        for locale in [Locale::EnUs, Locale::ZhTw] {
            let check = UpdateCheck::present(
                locale,
                true,
                false,
                &status(CatalogSource::Remote, None, Some(1_000_000)),
                3,
            );
            let UpdateCheck::Found(sentence) = check else {
                panic!("{locale:?} must report the count: {check:?}");
            };
            assert!(sentence.contains('3'), "{sentence}");
            assert!(!sentence.contains("{n}"), "{sentence}");
        }
    }

    /// Ticket 111: a count says what it counts, in both languages and in the
    /// singular too.
    #[test]
    fn the_count_of_updates_names_components_and_new_versions() {
        let found = |locale, n| match UpdateCheck::present(
            locale,
            false,
            false,
            &status(CatalogSource::Remote, None, Some(1_000_000)),
            n,
        ) {
            UpdateCheck::Found(sentence) => sentence,
            other => panic!("{other:?}"),
        };
        assert_eq!(found(Locale::ZhTw, 8), "8 個元件有新版本");
        assert_eq!(found(Locale::EnUs, 8), "8 components have new versions");
        assert_eq!(found(Locale::ZhTw, 1), "1 個元件有新版本");
        assert_eq!(found(Locale::EnUs, 1), "1 component has a new version");
    }

    #[test]
    fn a_check_that_found_nothing_says_so_with_the_age_of_the_list_it_checked() {
        let check = UpdateCheck::present(
            Locale::EnUs,
            true,
            false,
            &status(CatalogSource::Remote, None, Some(1_000_000)),
            0,
        );
        assert_eq!(check, UpdateCheck::UpToDate);
        // The age goes on the line under it, from the same status.
        assert_eq!(
            CatalogLine::present(
                Locale::EnUs,
                &status(CatalogSource::Remote, None, Some(1_000_000)),
                1_000_000 + 5 * 60
            )
            .age,
            "Checked for new versions 5 minutes ago"
        );
    }

    #[test]
    fn a_refresh_that_failed_explains_which_list_is_on_screen_instead() {
        for locale in [Locale::EnUs, Locale::ZhTw] {
            let c = copy(locale);
            let cases = [
                (
                    CatalogSource::Cache,
                    CatalogDegradation::RefreshFailedUsingCache,
                    c.catalog_degraded_failed_cache,
                ),
                (
                    CatalogSource::BuiltIn,
                    CatalogDegradation::RefreshFailedUsingBuiltIn,
                    c.catalog_degraded_failed_built_in,
                ),
                (
                    CatalogSource::BuiltIn,
                    CatalogDegradation::NeverRefreshed,
                    c.catalog_degraded_never,
                ),
            ];
            for (source, degradation, expected) in cases {
                // A count exists, and is still not reported as the answer: the
                // check never reached the published catalog, so the number it
                // would print describes the old list.
                let check = UpdateCheck::present(
                    locale,
                    true,
                    false,
                    &status(source, Some(degradation), Some(1_000_000)),
                    2,
                );
                assert_eq!(check, UpdateCheck::Failed(expected));
            }
        }
    }

    #[test]
    fn a_partly_refused_refresh_still_reports_the_count_it_did_adopt() {
        // The newer manifests were adopted, and the catalog line beside this
        // one already carries the refusal, so calling this a failed check would
        // be the inaccurate answer.
        let check = UpdateCheck::present(
            Locale::EnUs,
            true,
            false,
            &status(
                CatalogSource::Remote,
                Some(CatalogDegradation::PartiallyRefreshed),
                Some(1_000_000),
            ),
            1,
        );
        assert!(matches!(check, UpdateCheck::Found(_)), "{check:?}");
    }

    #[test]
    fn the_launch_check_reports_its_count_and_a_running_one_claims_nothing() {
        // The refresh at launch is a check too: its count is the answer, not
        // the page's title repeated (ticket 111).
        let current = status(CatalogSource::Remote, None, Some(1_000_000));
        assert!(matches!(
            UpdateCheck::present(Locale::EnUs, false, false, &current, 4),
            UpdateCheck::Found(_)
        ));
        assert_eq!(
            UpdateCheck::present(Locale::EnUs, false, false, &current, 0),
            UpdateCheck::UpToDate
        );
        // A launch refresh that failed keeps the count, with the list's
        // warning under it; only a check the person asked for is "failed".
        let failed = status(
            CatalogSource::Cache,
            Some(CatalogDegradation::RefreshFailedUsingCache),
            Some(1_000_000),
        );
        assert!(matches!(
            UpdateCheck::present(Locale::EnUs, false, false, &failed, 4),
            UpdateCheck::Found(_)
        ));
        // Running outranks the previous result: a stale count under a spinner
        // reads as this check's answer.
        assert_eq!(
            UpdateCheck::present(Locale::EnUs, true, true, &current, 4),
            UpdateCheck::Running
        );
    }

    #[test]
    fn the_update_check_copy_exists_in_both_locales_and_never_falls_back_to_english() {
        for locale in [Locale::EnUs, Locale::ZhTw] {
            let c = copy(locale);
            for value in [
                c.check_updates,
                c.checking_updates,
                c.check_again,
                c.update_check_found_one,
                c.update_check_found,
                c.update_check_up_to_date,
                c.update_check_failed,
                c.updates_subtitle,
                c.review_and_update_all,
            ] {
                assert!(!value.trim().is_empty(), "{locale:?} is missing a string");
            }
            assert!(c.update_check_found.contains("{n}"));
        }
        let en = copy(Locale::EnUs);
        let zh = copy(Locale::ZhTw);
        for (english, chinese) in [
            (en.check_updates, zh.check_updates),
            (en.checking_updates, zh.checking_updates),
            (en.check_again, zh.check_again),
            (en.update_check_found_one, zh.update_check_found_one),
            (en.update_check_found, zh.update_check_found),
            (en.update_check_up_to_date, zh.update_check_up_to_date),
            (en.update_check_failed, zh.update_check_failed),
            (en.updates_subtitle, zh.updates_subtitle),
            (en.review_and_update_all, zh.review_and_update_all),
        ] {
            assert_ne!(english, chinese);
        }
    }

    /// Ticket 111: every string the Updates page and its neighbours changed,
    /// word for word in both languages, so a regression to the old wording
    /// ("等待確認", "確認變更", "重新啟動需求") fails here.
    #[test]
    fn the_updates_page_says_what_each_part_means() {
        let en = copy(Locale::EnUs);
        let zh = copy(Locale::ZhTw);
        let cases = [
            (
                en.updates_subtitle,
                "New versions of the Better OS components on this computer.",
            ),
            (zh.updates_subtitle, "這台電腦上 Better OS 元件的新版本。"),
            (en.review_and_update_all, "Review and update all"),
            (zh.review_and_update_all, "檢視並全部更新"),
            (en.check_again, "Check again"),
            (zh.check_again, "再檢查一次"),
            (en.checking_updates, "Checking for new versions…"),
            (zh.checking_updates, "正在檢查有沒有新版本…"),
            (en.update_check_up_to_date, "All components are up to date"),
            (zh.update_check_up_to_date, "所有元件都是最新版本"),
            (en.update_check_failed, "Couldn't check for new versions"),
            (zh.update_check_failed, "無法檢查有沒有新版本"),
            (en.catalog_refresh, "Check for updates"),
            (zh.catalog_refresh, "檢查更新"),
            (en.catalog_refreshing, "Checking…"),
            (zh.catalog_refreshing, "正在檢查…"),
            (en.restart_requirement, "Afterwards"),
            (zh.restart_requirement, "完成後"),
            (en.restart_not_required, "Nothing needs restarting"),
            (zh.restart_not_required, "不需要重新啟動"),
            (en.restart_application, "Reopens by itself if it is open"),
            (zh.restart_application, "開著的話會自動重新開啟"),
            (en.restart_next_open, "Takes effect the next time it opens"),
            (zh.restart_next_open, "下次開啟時生效"),
            (
                en.update_check_none_in_list,
                "No new versions in the current version list",
            ),
            (zh.update_check_none_in_list, "目前的版本清單裡沒有新版本"),
            (
                en.catalog_rejected_one,
                "1 component entry failed its check and was left as it was.",
            ),
            (
                zh.catalog_rejected_one,
                "有 1 個元件的資訊沒有通過檢查，先維持原本的內容。",
            ),
            (en.restart_log_out, "Log out and back in"),
            (zh.restart_log_out, "需要登出再登入"),
            (en.restart_reboot, "Restart the computer"),
            (zh.restart_reboot, "需要重新開機"),
            (
                en.restart_note_application,
                "Reopens by itself after the update if it is open",
            ),
            (
                zh.restart_note_application,
                "更新後，開著的話會自動重新開啟",
            ),
            (
                en.restart_note_log_out,
                "Log out and back in after the update",
            ),
            (zh.restart_note_log_out, "更新後需要登出再登入"),
            (
                en.restart_note_reboot,
                "Restart the computer after the update",
            ),
            (zh.restart_note_reboot, "更新後需要重新開機"),
            (en.restore_none_title, "Restore a component"),
            (zh.restore_none_title, "還原元件"),
            (
                en.restore_nothing_failed,
                "No component has a failed check on record, so there is nothing to restore.",
            ),
            (
                zh.restore_nothing_failed,
                "目前沒有元件檢查失敗，不需要還原。",
            ),
            (
                en.catalog_degraded_never,
                "This version list may be outdated. It ships with the app and cannot know about versions released since.",
            ),
            (
                zh.catalog_degraded_never,
                "這份版本清單可能不是最新的。它是跟著應用程式一起發佈的，不會知道之後才推出的版本。",
            ),
            (
                en.catalog_degraded_failed_cache,
                "This version list may be outdated. The latest one could not be downloaded, so the one from the last check is shown.",
            ),
            (
                zh.catalog_degraded_failed_cache,
                "這份版本清單可能不是最新的。下載不到最新的清單，所以顯示上次檢查時的版本。",
            ),
            (
                en.catalog_degraded_failed_built_in,
                "This version list may be outdated. The latest one could not be downloaded, so the one built into the app is shown.",
            ),
            (
                zh.catalog_degraded_failed_built_in,
                "這份版本清單可能不是最新的。下載不到最新的清單，所以顯示應用程式內建的版本。",
            ),
            (
                en.catalog_degraded_partial,
                "This version list may be partly outdated. Some components' entries could not be downloaded and are shown as they were.",
            ),
            (
                zh.catalog_degraded_partial,
                "這份版本清單可能有部分不是最新的。有些元件的資訊沒有下載成功，先維持原本的內容。",
            ),
            (
                en.catalog_rejected,
                "{n} component entries failed their checks and were left as they were.",
            ),
            (
                zh.catalog_rejected,
                "有 {n} 個元件的資訊沒有通過檢查，先維持原本的內容。",
            ),
        ];
        for (actual, expected) in cases {
            assert_eq!(actual, expected);
        }
        let es = crate::i18n::shell(Locale::EnUs);
        let zs = crate::i18n::shell(Locale::ZhTw);
        let shell_cases = [
            (
                es.attention_updates_header,
                "{n} components have new versions",
            ),
            (zs.attention_updates_header, "{n} 個元件有新版本"),
            (
                es.attention_update_header_one,
                "1 component has a new version",
            ),
            (zs.attention_update_header_one, "1 個元件有新版本"),
            (es.apps_updates_count, "{n} components have new versions"),
            (zs.apps_updates_count, "{n} 個元件有新版本"),
            (es.apps_update_count_one, "1 component has a new version"),
            (zs.apps_update_count_one, "1 個元件有新版本"),
            (
                es.attention_catalog_header,
                "The version list may be out of date",
            ),
            (zs.attention_catalog_header, "版本清單可能不是最新的"),
            (es.attention_catalog_action, "Check for updates"),
            (zs.attention_catalog_action, "檢查更新"),
        ];
        for (actual, expected) in shell_cases {
            assert_eq!(actual, expected);
        }
    }

    /// Ticket 110 review: the manifest's restart requirement is attached to
    /// every operation, but only an update reopens a running app by itself.
    #[test]
    fn only_an_update_says_it_reopens_by_itself() {
        use crate::components::restart_effect;
        use manager_core::DesiredOperation as Op;
        let app = RestartRequirement::RestartApplication;
        for (locale, update, next) in [
            (Locale::ZhTw, "開著的話會自動重新開啟", "下次開啟時生效"),
            (
                Locale::EnUs,
                "Reopens by itself if it is open",
                "Takes effect the next time it opens",
            ),
        ] {
            assert_eq!(restart_effect(locale, Op::Update, app), Some(update));
            for op in [Op::Install, Op::Enable, Op::Restore] {
                assert_eq!(restart_effect(locale, op, app), Some(next), "{op:?}");
            }
            for op in [Op::Disable, Op::Remove, Op::Verify] {
                assert_eq!(restart_effect(locale, op, app), None, "{op:?}");
            }
            // Logging out or restarting is the person's to do, whatever runs.
            for op in [Op::Install, Op::Update, Op::Remove] {
                assert_eq!(
                    restart_effect(locale, op, RestartRequirement::LogOut),
                    Some(copy(locale).restart_log_out)
                );
                assert_eq!(
                    restart_effect(locale, op, RestartRequirement::Reboot),
                    Some(copy(locale).restart_reboot)
                );
            }
        }
        // The review's summary row: an update only when every step is one.
        use crate::app::plan_operation;
        assert_eq!(plan_operation(&[Op::Update, Op::Update]), Op::Update);
        assert_eq!(plan_operation(&[Op::Update, Op::Install]), Op::Install);
        assert_eq!(plan_operation(&[Op::Remove, Op::Disable]), Op::Remove);
        assert_eq!(plan_operation(&[]), Op::Install);
    }

    /// Ticket 110 review: an offline first launch has only the built-in list;
    /// "up to date" would claim more than it can know.
    #[test]
    fn a_list_that_is_not_the_published_one_never_says_up_to_date() {
        let never = CatalogStatus::built_in();
        assert_ne!(
            UpdateCheck::present(Locale::EnUs, false, false, &never, 0),
            UpdateCheck::UpToDate
        );
        assert_eq!(
            UpdateCheck::present(Locale::EnUs, false, false, &never, 0),
            UpdateCheck::NoneInCurrentList
        );
        let failed = status(
            CatalogSource::Cache,
            Some(CatalogDegradation::RefreshFailedUsingCache),
            Some(1_000_000),
        );
        assert_eq!(
            UpdateCheck::present(Locale::ZhTw, false, false, &failed, 0),
            UpdateCheck::NoneInCurrentList
        );
        // A refresh that succeeded, in full or in part, may say it.
        let partial = status(
            CatalogSource::Remote,
            Some(CatalogDegradation::PartiallyRefreshed),
            Some(1_000_000),
        );
        assert_eq!(
            UpdateCheck::present(Locale::EnUs, false, false, &partial, 0),
            UpdateCheck::UpToDate
        );
        let fetched = status(CatalogSource::Remote, None, Some(1_000_000));
        assert_eq!(
            UpdateCheck::present(Locale::EnUs, false, false, &fetched, 0),
            UpdateCheck::UpToDate
        );
    }

    /// Ticket 110 review: one refused entry is said in the singular.
    #[test]
    fn one_refused_entry_is_said_in_the_singular() {
        let mut one = status(
            CatalogSource::Remote,
            Some(CatalogDegradation::PartiallyRefreshed),
            Some(1_000_000),
        );
        one.rejections = vec![ManifestRejection {
            file: "better-monitor.yaml".to_string(),
            reason: RejectionReason::Unreachable,
        }];
        assert_eq!(
            CatalogLine::present(Locale::EnUs, &one, 1_000_000).rejected_line(Locale::EnUs),
            Some("1 component entry failed its check and was left as it was.".to_string())
        );
        assert_eq!(
            CatalogLine::present(Locale::ZhTw, &one, 1_000_000).rejected_line(Locale::ZhTw),
            Some("有 1 個元件的資訊沒有通過檢查，先維持原本的內容。".to_string())
        );
    }

    /// Ticket 110 review: the age line is redrawn once a minute where it is
    /// shown, so "moments ago" does not stay on screen for an hour.
    #[test]
    fn the_pages_with_the_age_line_redraw_every_minute() {
        use crate::app::{AGE_REFRESH, shows_catalog_age};
        use crate::model::Page;
        assert_eq!(AGE_REFRESH, std::time::Duration::from_secs(60));
        assert!(shows_catalog_age(&Page::Updates));
        assert!(shows_catalog_age(&Page::Components));
        assert!(!shows_catalog_age(&Page::Home));
    }

    /// Ticket 111: a card says what happens after its update only when an
    /// update is waiting and something happens.
    #[test]
    fn a_card_says_what_its_update_will_do_only_when_it_matters() {
        use crate::components::restart_note;
        use manager_core::ComponentStatus;
        let zh = Locale::ZhTw;
        assert_eq!(
            restart_note(
                zh,
                ComponentStatus::UpdateAvailable,
                RestartRequirement::RestartApplication
            ),
            Some("更新後，開著的話會自動重新開啟")
        );
        assert_eq!(
            restart_note(
                zh,
                ComponentStatus::UpdateAvailable,
                RestartRequirement::LogOut
            ),
            Some("更新後需要登出再登入")
        );
        assert_eq!(
            restart_note(
                zh,
                ComponentStatus::UpdateAvailable,
                RestartRequirement::Reboot
            ),
            Some("更新後需要重新開機")
        );
        for quiet in [
            RestartRequirement::NotRequired,
            RestartRequirement::NotDeclared,
        ] {
            assert_eq!(
                restart_note(zh, ComponentStatus::UpdateAvailable, quiet),
                None
            );
        }
        // Nothing to update, nothing to say.
        assert_eq!(
            restart_note(
                zh,
                ComponentStatus::Healthy,
                RestartRequirement::RestartApplication
            ),
            None
        );
    }

    /// The check button and the About rows never make a row worse than what is
    /// already shipped, at every supported window size and scale.
    #[test]
    fn the_new_labels_lay_out_no_worse_than_the_ones_already_shipped() {
        for locale in [Locale::EnUs, Locale::ZhTw] {
            let c = copy(locale);
            // Ticket 111's buttons: the check, its running state, and the
            // review that updates everything.
            let new_longest = [
                c.check_updates,
                c.check_again,
                c.catalog_refreshing,
                c.review_and_update_all,
            ]
            .iter()
            .map(|label| label.chars().count())
            .max()
            .unwrap();
            let shipped_longest = [c.install_updates, c.restore_previous]
                .iter()
                .map(|label| label.chars().count())
                .max()
                .unwrap();
            assert!(
                new_longest <= shipped_longest,
                "{locale:?} added a label longer than anything already on screen"
            );
            for width in [MIN_CONTENT_WIDTH, 1280.0, 1920.0] {
                for scale in [1.0, 1.25, 1.5] {
                    let shipped = action_layout(width, scale, shipped_longest);
                    let added = action_layout(width, scale, new_longest);
                    assert!(
                        added == ActionLayout::Inline || shipped == ActionLayout::Wrapped,
                        "{locale:?} at {width}x{scale} wraps a new label where nothing else wraps"
                    );
                }
            }
        }
    }

    /// The repository line is the longest fixed string the About section shows,
    /// and it is a URL that must not be truncated into something misleading.
    #[test]
    fn the_repository_line_fits_the_narrowest_supported_window() {
        let characters = characters_per_line(MIN_CONTENT_WIDTH, 1.0);
        assert!(
            PROJECT_REPOSITORY.chars().count() <= characters,
            "{PROJECT_REPOSITORY} needs {} of {characters} characters",
            PROJECT_REPOSITORY.chars().count()
        );
        assert!(characters >= MIN_READABLE_CHARACTERS);
    }
}

/// The affordance rule, asserted where it can be: on the view models the
/// screens draw from.
///
/// A Zorin 18 machine reported the manager's failure surfaces as a wall of
/// pills that all looked pressable and mostly were not. The rule the project
/// now holds itself to is that anything that looks clickable must be
/// clickable, and anything that is not clickable must not look like a button.
/// `gpui_component::Tag` could not satisfy it — it paints the same theme
/// tokens a `Button` of the same variant paints, and adds an unconditional
/// hover — so every status in this window is a `better_ui::StatusPill` now.
/// These tests are what stops one drifting back.
mod affordance {
    use super::*;
    use crate::app::ManagerApp;
    use crate::defaults_model::PrimaryAction;
    use crate::model::ComponentKind;
    use better_ui::{Affordance, StatusPill, StatusTone};
    use defaults_core::AggregateState;
    use manager_core::{ActivityKind, DoctorCheckStatus, HealthState};

    /// Every status indicator this window can draw, in one place. A screen
    /// that grows a new one and does not add it here is the gap this test is
    /// meant to make visible, so the list is by state rather than by screen.
    fn every_status_indicator(locale: Locale) -> Vec<StatusPill> {
        let c = copy(locale);
        let mut pills = Vec::new();
        for status in [
            ComponentStatus::Available,
            ComponentStatus::Downloading,
            ComponentStatus::ReadyToInstall,
            ComponentStatus::Installing,
            ComponentStatus::Verifying,
            ComponentStatus::Healthy,
            ComponentStatus::UpdateAvailable,
            ComponentStatus::Disabled,
            ComponentStatus::Incompatible,
            ComponentStatus::Degraded,
            ComponentStatus::Failed,
            ComponentStatus::RestoreAvailable,
        ] {
            pills.push(ManagerApp::status_pill(locale, status, false));
            pills.push(ManagerApp::status_pill(locale, status, true));
        }
        for kind in [
            ComponentKind::Replacement,
            ComponentKind::Enhancement,
            ComponentKind::Diagnostic,
        ] {
            pills.push(ManagerApp::kind_pill(locale, kind));
        }
        for health in [
            HealthState::Healthy,
            HealthState::Degraded,
            HealthState::Failed,
        ] {
            pills.push(ManagerApp::health_pill(locale, health));
        }
        for status in [
            DoctorCheckStatus::Passed,
            DoctorCheckStatus::Warning,
            DoctorCheckStatus::Failed,
        ] {
            pills.push(ManagerApp::doctor_pill(c, status));
        }
        for kind in [
            ActivityKind::Success,
            ActivityKind::RecoverySuccess,
            ActivityKind::Failure,
            ActivityKind::Warning,
            ActivityKind::RecoveryPartial,
            ActivityKind::ManualRecovery,
            ActivityKind::Information,
        ] {
            pills.push(ManagerApp::activity_pill(c, kind));
        }
        for aggregate in [
            AggregateState::Default,
            AggregateState::NotDefault,
            AggregateState::PartiallyDefault,
            AggregateState::ChangedExternally,
            AggregateState::NeedsSignOut,
        ] {
            pills.push(ManagerApp::defaults_state_pill(locale, &aggregate));
        }
        pills
    }

    #[test]
    fn every_status_indicator_is_a_status_and_never_an_action() {
        for locale in [Locale::EnUs, Locale::ZhTw] {
            let pills = every_status_indicator(locale);
            assert!(
                pills.len() >= 40,
                "the enumeration lost indicators: {}",
                pills.len()
            );
            for pill in pills {
                assert_eq!(
                    StatusPill::AFFORDANCE,
                    Affordance::Status,
                    "{pill:?} must never read as something to press"
                );
                assert!(
                    !pill.label.trim().is_empty(),
                    "{locale:?} left a status indicator with no words"
                );
            }
        }
    }

    /// A status carries its tone, and the window draws a tone as a tint rather
    /// than as the fill a button of the same tone wears. What this locks is
    /// that a failure is still recognisably a failure after the change: the
    /// point was never to make every status grey.
    #[test]
    fn a_failure_still_reads_as_a_failure() {
        for locale in [Locale::EnUs, Locale::ZhTw] {
            assert_eq!(
                ManagerApp::status_pill(locale, ComponentStatus::Failed, false).tone,
                StatusTone::Danger
            );
            assert_eq!(
                ManagerApp::status_pill(locale, ComponentStatus::RestoreAvailable, false).tone,
                StatusTone::Danger
            );
            assert_eq!(
                ManagerApp::health_pill(locale, HealthState::Healthy).tone,
                StatusTone::Success
            );
            assert_eq!(
                ManagerApp::doctor_pill(copy(locale), DoctorCheckStatus::Failed).tone,
                StatusTone::Danger
            );
            assert_eq!(
                ManagerApp::activity_pill(copy(locale), ActivityKind::Failure).tone,
                StatusTone::Danger
            );
        }
    }

    /// A pending change reads the same whatever the component's own state is,
    /// because what the person needs to know is that they already asked for
    /// something.
    #[test]
    fn a_pending_component_reads_as_pending_whatever_its_state() {
        for locale in [Locale::EnUs, Locale::ZhTw] {
            let pending = ManagerApp::status_pill(locale, ComponentStatus::Failed, true);
            assert_eq!(pending.label.as_ref(), copy(locale).ready_to_install);
            assert_eq!(pending.tone, StatusTone::Info);
        }
    }

    /// The Defaults row's leading control. A component that is already the
    /// default has nothing to carry out, so it gets no button at all — not a
    /// permanently disabled one repeating the pill beside it.
    #[test]
    fn a_component_already_the_default_leads_with_a_status_not_a_button() {
        assert_eq!(
            PrimaryAction::AlreadyDefault.affordance(),
            Affordance::Status
        );
        assert_eq!(PrimaryAction::MakeDefault.affordance(), Affordance::Action);
        assert_eq!(PrimaryAction::Verify.affordance(), Affordance::Action);
        assert_eq!(
            PrimaryAction::of(&AggregateState::Default),
            PrimaryAction::AlreadyDefault
        );
    }

    /// The two screens the field report named, locked against the source
    /// that draws them.
    ///
    /// A view model cannot prove what a render function does with it, and a
    /// headless test cannot open a window. What it can do is read the render
    /// code: no screen in this window may build a status out of
    /// `gpui_component::Tag`, whose filled variants paint the same tokens a
    /// `Button` paints and which adds a hover a caller cannot switch off.
    #[test]
    fn no_screen_draws_a_status_with_the_toolkit_tag() {
        for (name, source) in [
            ("components.rs", include_str!("components.rs")),
            ("pages_main.rs", include_str!("pages_main.rs")),
            ("pages_flow.rs", include_str!("pages_flow.rs")),
            ("pages_settings.rs", include_str!("pages_settings.rs")),
            ("pages_defaults.rs", include_str!("pages_defaults.rs")),
        ] {
            assert!(
                !source.contains("Tag::"),
                "{name} builds a status out of gpui_component::Tag again"
            );
            assert!(
                !source.contains("tag::Tag"),
                "{name} imports gpui_component::tag::Tag again"
            );
        }
    }

    /// The failure card's recovery row, locked at the call site. Asserting the
    /// two strings differ is not enough on its own: the defect was the render
    /// passing the *value's* key as the label, which no comparison of copy
    /// constants can see.
    #[test]
    fn the_failure_card_labels_its_recovery_row_as_a_heading() {
        let source = include_str!("components.rs");
        assert!(
            source.contains("self.key_value_row(c.recovery_status, recovery, cx)"),
            "the failure card no longer labels its recovery row with recovery_status"
        );
        assert!(
            !source.contains("self.key_value_row(c.restore_available, recovery, cx)"),
            "the failure card prints its own label as its value again"
        );
    }

    /// The Defaults row, locked at the call site, for the same reason.
    #[test]
    fn the_defaults_row_draws_no_button_for_a_component_already_the_default() {
        let source = include_str!("pages_defaults.rs");
        assert!(
            !source.contains(".disabled(true)"),
            "a Defaults control is a permanently disabled button again"
        );
        assert!(
            source.contains("PrimaryAction::AlreadyDefault => {"),
            "the AlreadyDefault arm no longer decides on its own"
        );
    }

    /// The failure card's recovery row. It used to label the row with the same
    /// sentence it printed as the value — "a previous version can be restored"
    /// on both sides of the colon — which told a person nothing. The label is
    /// now a heading and the value is what actually happened.
    #[test]
    fn the_recovery_row_never_prints_its_own_label_as_its_value() {
        for locale in [Locale::EnUs, Locale::ZhTw] {
            let c = copy(locale);
            assert!(!c.recovery_status.trim().is_empty());
            for value in [
                c.restore_available,
                c.recovery_partial,
                c.manual_recovery_required,
            ] {
                assert_ne!(
                    c.recovery_status, value,
                    "{locale:?} prints the recovery label as its own value again"
                );
            }
        }
    }
}

/// Ticket 49: a component whose record and machine disagree now says so, and
/// offers the one action that resolves it.
///
/// Before this, drift was invisible in the window. The record carried it, the
/// planner refused every operation because of it, and no screen mentioned it —
/// the field report was a manager that offered no update for itself and gave no
/// reason.
mod drift_surface {
    use super::*;
    use crate::model::DriftNotice;
    use manager_core::{ComponentRecord, DriftKind, HealthState, InstallProvenance};

    fn manifest() -> better_core::ComponentManifest {
        let (manager, _) = demo_manager();
        manager
            .manifests()
            .find(|manifest| manifest.id.as_str() == "better-monitor")
            .expect("the demo catalog must carry the monitor")
            .clone()
    }

    fn record(drift: Option<DriftKind>) -> ComponentRecord {
        ComponentRecord {
            installed_version: Some("0.2.3".to_string()),
            enabled: true,
            health: HealthState::Healthy,
            installed_artifact: None,
            provenance: InstallProvenance::Manager,
            restore_snapshot: None,
            failure: None,
            recovery: None,
            drift,
        }
    }

    fn present(drift: Option<DriftKind>) -> ComponentInfo {
        let manifest = manifest();
        ComponentInfo::present(
            &manifest,
            Some(&record(drift)),
            ComponentStatus::Failed,
            translated_component(Locale::ZhTw, &manifest.id),
        )
    }

    #[test]
    fn a_component_without_drift_shows_no_drift_card() {
        for locale in [Locale::EnUs, Locale::ZhTw] {
            assert_eq!(present(None).drift_notice(locale), None);
        }
    }

    #[test]
    fn a_version_disagreement_names_both_versions_in_both_locales() {
        for locale in [Locale::EnUs, Locale::ZhTw] {
            let notice: DriftNotice = present(Some(DriftKind::VersionMismatch {
                host: "0.2.1".to_string(),
            }))
            .drift_notice(locale)
            .expect("a drifted component must have something to say");

            assert!(notice.detail.contains("0.2.1"), "{:?}", notice.detail);
            assert!(notice.detail.contains("0.2.3"), "{:?}", notice.detail);
            // No template placeholder survives into what a person reads.
            assert!(!notice.detail.contains('{'), "{:?}", notice.detail);
            assert!(!notice.title.trim().is_empty());
            assert!(!notice.consequence.trim().is_empty());
        }
    }

    #[test]
    fn a_package_the_host_does_not_have_says_so_rather_than_naming_a_second_version() {
        for locale in [Locale::EnUs, Locale::ZhTw] {
            let notice = present(Some(DriftKind::MissingOnHost))
                .drift_notice(locale)
                .expect("a drifted component must have something to say");

            assert!(notice.detail.contains("0.2.3"), "{:?}", notice.detail);
            assert!(!notice.detail.contains('{'), "{:?}", notice.detail);
        }
    }

    #[test]
    fn the_adoption_copy_exists_in_both_locales() {
        for locale in [Locale::EnUs, Locale::ZhTw] {
            let c = copy(locale);
            for value in [
                c.drift_title,
                c.drift_blocks_planning,
                c.adopt_host_state,
                c.adopt_host_state_confirm,
                c.adopt_host_state_confirm_action,
                c.evidence_externally_upgraded,
                c.evidence_state_adopted,
            ] {
                assert!(!value.trim().is_empty());
            }
            // The two locales must actually differ, or one of them was copied.
            assert_ne!(
                copy(Locale::EnUs).adopt_host_state,
                copy(Locale::ZhTw).adopt_host_state
            );
        }
    }

    /// A view model cannot prove what a render function does with it. The card
    /// is what makes the drift visible and the action reachable, so its absence
    /// from the component page is the regression to catch.
    #[test]
    fn the_component_page_draws_the_drift_card_and_asks_before_adopting() {
        let page = include_str!("pages_main.rs");
        assert!(
            page.contains("self.drift_card(&component, &notice, cx)"),
            "the component detail page must draw the drift card"
        );

        let card = include_str!("components.rs");
        // Adoption throws away the restore point, so the first click may only
        // ask. `adopt_host_state` is reached from the confirming branch alone.
        assert!(card.contains("this.ask_to_adopt_host_state(Some(ask_id.clone()), cx)"));
        assert!(card.contains("this.adopt_host_state(&adopt_id, cx)"));
        let confirm_at = card
            .find("Button::new(\"drift-adopt-confirm\")")
            .expect("the confirming branch must exist");
        let act_at = card
            .find("this.adopt_host_state(&adopt_id, cx)")
            .expect("the adopting handler must exist");
        assert!(
            confirm_at < act_at,
            "adoption must only be reachable from the confirmation"
        );
    }
}

/// Ticket 68: an interrupted dpkg run, shown on the Health screen with the one
/// control that repairs it.
mod package_system_repair {
    use super::*;
    use crate::model::{package_system_notice, repair_result_line};
    use manager_core::FailureEvidence;
    use manager_core::repair::{
        DpkgPackageStatus, InterruptedPackage, PackageSystemState, RepairResult,
    };

    fn interrupted(omitted: u32) -> PackageSystemState {
        PackageSystemState {
            protocol_version: manager_core::repair::PROTOCOL_VERSION,
            journal_pending: true,
            packages: vec![InterruptedPackage {
                package: "better-monitor".to_string(),
                status: DpkgPackageStatus::HalfConfigured,
            }],
            omitted,
        }
    }

    #[test]
    fn a_clean_package_system_draws_no_card() {
        for locale in [Locale::EnUs, Locale::ZhTw] {
            assert_eq!(
                package_system_notice(locale, &PackageSystemState::clean()),
                None
            );
        }
    }

    #[test]
    fn an_interrupted_package_system_names_each_package_in_dpkgs_words() {
        for locale in [Locale::EnUs, Locale::ZhTw] {
            let notice = package_system_notice(locale, &interrupted(3))
                .expect("an interrupted package system must have something to say");
            assert!(!notice.title.trim().is_empty());
            assert!(!notice.consequence.trim().is_empty());
            let lines = notice.lines.join("\n");
            assert!(lines.contains("better-monitor"), "{lines}");
            assert!(lines.contains("half-configured"), "{lines}");
            assert!(lines.contains('3'), "the omitted count is said: {lines}");
            assert!(!lines.contains('{'), "no placeholder survives: {lines}");
        }
    }

    #[test]
    fn every_repair_result_has_its_own_sentence_in_both_locales() {
        let results = [
            RepairResult::Repaired,
            RepairResult::StillInterrupted,
            RepairResult::NothingToRepair,
            RepairResult::Failed {
                evidence: FailureEvidence::new("daemon.error.repair_failed"),
            },
        ];
        for locale in [Locale::EnUs, Locale::ZhTw] {
            let lines = results
                .iter()
                .map(|result| repair_result_line(locale, result))
                .collect::<Vec<_>>();
            for line in &lines {
                assert!(!line.trim().is_empty());
            }
            for (index, line) in lines.iter().enumerate() {
                assert!(
                    !lines[index + 1..].contains(line),
                    "two results read the same: {line}"
                );
            }
        }
        assert_ne!(
            repair_result_line(Locale::EnUs, &RepairResult::Repaired),
            repair_result_line(Locale::ZhTw, &RepairResult::Repaired)
        );
    }

    /// The daemon's sandbox can be why dpkg failed (ADR 0014). A failed repair
    /// says how to run the same command from a terminal instead.
    #[test]
    fn a_failed_repair_names_the_terminal_command_in_both_locales() {
        let failed = RepairResult::Failed {
            evidence: FailureEvidence::new("daemon.error.repair_failed"),
        };
        for locale in [Locale::EnUs, Locale::ZhTw] {
            assert!(
                repair_result_line(locale, &failed).contains("sudo dpkg --configure -a"),
                "{locale:?}"
            );
        }
    }

    #[test]
    fn the_repair_copy_exists_in_both_locales() {
        for locale in [Locale::EnUs, Locale::ZhTw] {
            let c = copy(locale);
            for value in [
                c.package_system_title,
                c.package_system_journal,
                c.package_system_blocks,
                c.repair_package_system,
                c.repair_needs_password,
                c.repairing,
                c.evidence_dpkg_interrupted,
                c.evidence_package_system_repaired,
                c.evidence_package_system_still_interrupted,
                c.evidence_repair_failed,
            ] {
                assert!(!value.trim().is_empty());
            }
        }
    }

    /// The window keys these evidence strings to sentences; a key that falls
    /// through to the generic refusal would hide what happened.
    #[test]
    fn the_repair_evidence_keys_are_worded_rather_than_falling_through() {
        let app = include_str!("app.rs");
        for (key, field) in [
            (
                "daemon.error.dpkg_interrupted",
                "c.evidence_dpkg_interrupted",
            ),
            ("daemon.error.repair_failed", "c.evidence_repair_failed"),
            (
                "package_system.repaired",
                "c.evidence_package_system_repaired",
            ),
            (
                "package_system.still_interrupted",
                "c.evidence_package_system_still_interrupted",
            ),
        ] {
            let at = app
                .find(&format!("\"{key}\""))
                .unwrap_or_else(|| panic!("{key} has no wording"));
            // From the key to the start of the next match arm.
            let arm = app[at..].split("Some(").next().unwrap();
            assert!(arm.contains(field), "{key} must map to {field}: {arm}");
        }
    }

    /// A stopped background service is the defect this ticket exists for, so
    /// the window has to say that rather than fall through to a generic
    /// refusal. Both keys carry a unit and systemctl's words in the detail.
    #[test]
    fn the_user_unit_evidence_keys_are_worded_rather_than_falling_through() {
        let app = include_str!("app.rs");
        for (key, field) in [
            ("user_units.failed", "c.evidence_user_units_failed"),
            (
                "user_units.not_running",
                "c.evidence_user_units_not_running",
            ),
            ("user_units.started", "c.evidence_user_units_started"),
            ("user_units.masked", "c.evidence_user_units_masked"),
            ("user_units.no_session", "c.evidence_user_units_no_session"),
        ] {
            let at = app
                .find(&format!("\"{key}\""))
                .unwrap_or_else(|| panic!("{key} has no wording"));
            let arm = app[at..].split("Some(").next().unwrap();
            assert!(arm.contains(field), "{key} must map to {field}: {arm}");
        }
    }

    /// An activity row shows no detail, so a unit entry has to name its unit
    /// in the sentence, and a start the record caused has to say so.
    #[test]
    fn a_unit_activity_row_names_the_unit_and_why_it_was_started() {
        use crate::app::activity_evidence_text;

        let zh = copy(Locale::ZhTw);
        assert!(
            zh.evidence_user_units_started
                .contains("依 Better Manager 的紀錄重新啟用"),
            "{}",
            zh.evidence_user_units_started
        );
        let en = copy(Locale::EnUs);
        assert!(
            en.evidence_user_units_started
                .contains("Better Manager's record"),
            "{}",
            en.evidence_user_units_started
        );

        assert_eq!(
            activity_evidence_text(
                en.evidence_user_units_failed,
                Some("user_units.failed:better-awake.service")
            ),
            format!("{}: better-awake.service", en.evidence_user_units_failed)
        );
        assert_eq!(
            activity_evidence_text(
                zh.evidence_user_units_started,
                Some("user_units.started:a.service,b.service")
            ),
            format!("{}: a.service, b.service", zh.evidence_user_units_started)
        );
        assert_eq!(
            activity_evidence_text(en.evidence_health_failed, Some("health.failed")),
            en.evidence_health_failed
        );
        let rows = include_str!("pages_main.rs");
        let row = rows
            .split("fn activity_row(")
            .nth(1)
            .unwrap()
            .split("\n    }\n")
            .next()
            .unwrap();
        assert!(row.contains("activity_evidence_text("), "{row}");
    }

    /// The record saying a component is enabled is not true of the machine
    /// unless something makes it true, and startup is that something. A demo
    /// window, which returns before any of this, still touches nothing.
    #[test]
    fn the_window_heals_user_units_when_it_reconciles() {
        let app = include_str!("app.rs");
        let reconcile = app
            .split("fn reconcile_with_host(&mut self, cx: &mut Context<Self>)")
            .nth(1)
            .expect("reconciliation exists");
        let reconcile = reconcile.split("\n    }\n").next().unwrap();
        assert!(
            reconcile.contains("self.heal_user_units(cx)"),
            "{reconcile}"
        );
        assert!(reconcile.contains("ExecutionMode::Mock"), "{reconcile}");
    }

    /// Starting a unit waits on systemd for seconds at a time, so it must not
    /// run on the thread that draws the window: the pass that talks to
    /// systemd runs inside `background_spawn`, and only the bookkeeping comes
    /// back to the window. Nothing starts while a transaction owns the units.
    #[test]
    fn the_window_starts_user_units_off_the_ui_thread() {
        let app = include_str!("app.rs");
        let reconcile = app
            .split("fn reconcile_with_host(&mut self, cx: &mut Context<Self>)")
            .nth(1)
            .unwrap()
            .split("\n    }\n")
            .next()
            .unwrap();
        assert!(
            !reconcile.contains("heal_user_units(&mut"),
            "the reconciliation must not start units itself: {reconcile}"
        );
        let heal = app
            .split("fn heal_user_units(&mut self, cx: &mut Context<Self>)")
            .nth(1)
            .expect("the background pass exists")
            .split("\n    }\n")
            .next()
            .unwrap();
        let background = heal
            .split(".background_spawn(")
            .nth(1)
            .expect("the pass runs in background_spawn");
        assert!(
            background.contains(".run_user_unit_healing("),
            "{background}"
        );
        assert!(
            background.contains("SystemdUserUnitController::new()"),
            "{background}"
        );
        assert!(heal.contains("self.running.is_some()"), "{heal}");
        assert!(
            heal.contains("self.state.active_operation.is_some()"),
            "{heal}"
        );
        let finish = app
            .split("fn finish_healing(")
            .nth(1)
            .expect("the result is applied on the window's thread")
            .split("\n    }\n")
            .next()
            .unwrap();
        assert!(finish.contains(".record_user_unit_healing("), "{finish}");
        assert!(!finish.contains("SystemdUserUnitController"), "{finish}");
    }

    /// A real transaction enables and starts the component's own units, so the
    /// driver it builds has to be given the controller that does it.
    #[test]
    fn the_real_driver_is_given_the_user_unit_controller() {
        let app = include_str!("app.rs");
        let run = app
            .split("fn run_real_transaction(&mut self")
            .nth(1)
            .expect("the real transaction exists");
        let run = run.split("\n    }\n").next().unwrap();
        assert!(run.contains("SystemdUserUnitController::new()"), "{run}");
        assert!(run.contains(".with_user_units(&user_units)"), "{run}");
    }

    /// A view model cannot prove what a render function does with it. The card
    /// and its button are what make the repair reachable, and the button must
    /// go through `manager-core` rather than talking to the service itself.
    #[test]
    fn the_health_screen_draws_the_card_and_repairs_through_manager_core() {
        let page = include_str!("pages_settings.rs");
        assert!(
            page.contains("self.package_system_card(cx)"),
            "the Health screen must draw the package system card"
        );
        assert!(page.contains("this.repair_package_system(cx)"));

        let app = include_str!("app.rs");
        assert!(app.contains("repair::request_repair(&service)"));
        assert!(app.contains("repair::record_repair(&mut candidate, &report)"));
        assert!(app.contains("repair::package_system_state(&service)"));
        // Nothing in the window runs dpkg or reads its record itself.
        for source in [app, page, include_str!("components.rs")] {
            assert!(!source.contains("\"dpkg\""));
            assert!(!source.contains("/var/lib/dpkg"));
        }
    }
}

/// Ticket 69: a package dpkg holds at a version the manager cannot order is a
/// doctor finding on the Health screen too.
mod unorderable_host_versions {
    use super::*;
    use crate::model::unorderable_detail;
    use manager_core::{DoctorCheck, DoctorCheckKind, DoctorCheckStatus};

    fn check() -> DoctorCheck {
        DoctorCheck {
            kind: DoctorCheckKind::UnorderableHostVersion,
            status: DoctorCheckStatus::Warning,
            component: Some(ComponentId::new("better-monitor").unwrap()),
            host_version: Some("1:0.2.8~rc1-1".to_string()),
        }
    }

    #[test]
    fn the_finding_names_dpkgs_version_and_what_to_do_in_both_locales() {
        for locale in [Locale::EnUs, Locale::ZhTw] {
            let detail = unorderable_detail(locale, &check())
                .expect("an unorderable package has something to say");
            assert!(detail.contains("1:0.2.8~rc1-1"), "{detail}");
            assert!(detail.contains("apt remove better-monitor"), "{detail}");
            assert!(!detail.contains('{'), "{detail}");
            assert!(!copy(locale).check_unorderable_version.trim().is_empty());
        }
        assert_ne!(
            unorderable_detail(Locale::EnUs, &check()),
            unorderable_detail(Locale::ZhTw, &check())
        );
    }

    #[test]
    fn other_checks_carry_no_such_detail() {
        let mut other = check();
        other.kind = DoctorCheckKind::HostReconciliation;
        assert_eq!(unorderable_detail(Locale::EnUs, &other), None);
    }

    /// dpkg is asked once, when the window reconciles, not on every frame the
    /// Health screen draws; and a demo window, which skips reconciling, never
    /// asks at all.
    #[test]
    fn the_window_asks_dpkg_when_it_reconciles_and_lists_the_findings() {
        let app = include_str!("app.rs");
        let reconcile = app
            .split("fn reconcile_with_host(&mut self, cx: &mut Context<Self>)")
            .nth(1)
            .expect("reconciliation exists");
        let reconcile = reconcile.split("\n    }\n").next().unwrap();
        assert!(
            reconcile.contains(".unorderable_host_checks(&self.state, &DpkgProbe)"),
            "{reconcile}"
        );
        let doctor = app
            .split("fn doctor_checks(&self)")
            .nth(1)
            .unwrap()
            .split("\n    }\n")
            .next()
            .unwrap();
        assert!(
            doctor.contains("self.unorderable.iter().cloned()"),
            "{doctor}"
        );
        assert!(!doctor.contains("DpkgProbe"), "{doctor}");

        let page = include_str!("pages_settings.rs");
        assert!(
            page.contains("DoctorCheckKind::UnorderableHostVersion => c.check_unorderable_version")
        );
        assert!(page.contains("unorderable_detail(self.locale, check)"));
    }

    /// Nothing is planned for such a package: the window plans a real
    /// operation through `plan_on_host`, which refuses it, and no screen
    /// offers Install for it. The component page says why instead.
    #[test]
    fn the_window_neither_plans_nor_offers_an_unorderable_package() {
        let app = include_str!("app.rs");
        assert!(
            !app.contains(".plan_in_mode(&self.state, id, operation, self.execution)")
                && !app
                    .contains(".plan_in_mode(&self.state, &component, operation, self.execution)"),
            "a component operation is planned without asking dpkg"
        );
        let plan = app
            .split("fn plan_operation(")
            .nth(1)
            .expect("one planning path for component operations")
            .split("\n    }\n")
            .next()
            .unwrap();
        assert!(plan.contains(".plan_on_host("), "{plan}");
        assert!(plan.contains("&DpkgProbe"), "{plan}");

        let card = include_str!("components.rs");
        let button = card
            .split("fn component_action_button(")
            .nth(1)
            .unwrap()
            .split("match component.state")
            .next()
            .unwrap();
        assert!(
            button.contains("self.unorderable_check(&component.core_id)"),
            "{button}"
        );

        let detail = include_str!("pages_main.rs");
        assert!(detail.contains(".unorderable_check(&component.core_id)"));
        assert!(detail.contains("unorderable.is_none()"));
        assert!(detail.contains("unorderable_detail(self.locale, check)"));
    }
}

/// `better-manager --page` with a window already open (spec section 1), over
/// a private bus this test starts and kills. Nothing here touches the
/// developer's own session bus.
mod single_instance {
    use std::io::{BufRead, BufReader};
    use std::process::{Child, Command, Stdio};
    use std::time::{Duration, Instant};

    use crate::deep_link::{BUS_NAME, INTERFACE, OBJECT_PATH, Request, bus};
    use crate::model::Page;

    fn page(page: Page) -> Request {
        Request::Page {
            page,
            until_dialog: false,
        }
    }

    /// A private session bus, killed when the test ends however it ends.
    struct PrivateBus {
        child: Child,
        address: String,
    }

    impl PrivateBus {
        fn start() -> Option<Self> {
            let mut child = Command::new("dbus-daemon")
                .args(["--session", "--nofork", "--print-address"])
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .ok()?;
            let mut address = String::new();
            let read = child
                .stdout
                .take()
                .map(|out| BufReader::new(out).read_line(&mut address));
            let bus = Self {
                child,
                address: address.trim().to_string(),
            };
            (matches!(read, Some(Ok(_))) && !bus.address.is_empty()).then_some(bus)
        }

        fn connect(&self, timeout: Duration) -> zbus::blocking::Connection {
            bus::connect(Some(&self.address), timeout).expect("the private bus answers")
        }
    }

    impl Drop for PrivateBus {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }

    macro_rules! bus_or_skip {
        () => {
            match PrivateBus::start() {
                Some(bus) => bus,
                None => {
                    eprintln!("skipping: dbus-daemon is not available in this environment");
                    return;
                }
            }
        };
    }

    /// The single-instance rule over a real, private bus: the first window
    /// owns the name, a second is refused it and its request arrives at the
    /// first, and a bad page is refused across the bus.
    #[test]
    fn a_second_window_hands_its_page_to_the_first() {
        let private = bus_or_skip!();
        let first = bus::claim(private.connect(bus::FORWARD_TIMEOUT)).unwrap();
        let bus::Claim::Owner(owner) = first else {
            panic!("the first window owns the name");
        };
        let second = bus::claim(private.connect(bus::FORWARD_TIMEOUT)).unwrap();
        let bus::Claim::Running(connection) = second else {
            panic!("the second window finds the name taken");
        };
        bus::forward(&connection, &page(Page::System)).unwrap();
        assert_eq!(
            smol::block_on(owner.requests.recv()).unwrap(),
            page(Page::System)
        );
        bus::forward(&connection, &Request::Raise).unwrap();
        assert_eq!(
            smol::block_on(owner.requests.recv()).unwrap(),
            Request::Raise
        );
        let refused = connection.call_method(
            Some(BUS_NAME),
            OBJECT_PATH,
            Some(INTERFACE),
            "ShowPage",
            &("bogus",),
        );
        assert!(refused.is_err());
        assert!(owner.requests.try_recv().is_err());
    }

    /// Something that is not Better Manager holds the name: the second
    /// process gets an error rather than a window it thinks it raised, and
    /// does not hang.
    #[test]
    fn a_name_held_by_something_else_is_an_error_not_a_hang() {
        let private = bus_or_skip!();
        let squatter = private.connect(bus::FORWARD_TIMEOUT);
        squatter.request_name(BUS_NAME).unwrap();
        let bus::Claim::Running(connection) =
            bus::claim(private.connect(Duration::from_millis(500))).unwrap()
        else {
            panic!("the name is taken");
        };
        // The holder may refuse the call or never answer it; either way the
        // second process gets an error within its timeout.
        let started = Instant::now();
        assert!(bus::forward(&connection, &page(Page::System)).is_err());
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "{:?}",
            started.elapsed()
        );
    }

    struct Silent;

    #[zbus::interface(name = "io.betteros.Manager")]
    impl Silent {
        /// Holds the call far longer than any caller waits.
        async fn show_page(&self, _page: String) {
            smol::Timer::after(Duration::from_secs(30)).await;
        }
    }

    /// A window that owns the name and never answers: the forward gives up
    /// after its timeout.
    #[test]
    fn a_window_that_never_answers_is_given_up_on() {
        let private = bus_or_skip!();
        let silent = private.connect(bus::FORWARD_TIMEOUT);
        silent.object_server().at(OBJECT_PATH, Silent).unwrap();
        silent.request_name(BUS_NAME).unwrap();
        let bus::Claim::Running(connection) =
            bus::claim(private.connect(Duration::from_millis(500))).unwrap()
        else {
            panic!("the name is taken");
        };
        let started = Instant::now();
        assert!(bus::forward(&connection, &Request::Raise).is_err());
        let waited = started.elapsed();
        assert!(waited >= Duration::from_millis(400), "{waited:?}");
        assert!(waited < Duration::from_secs(5), "{waited:?}");
    }
}
