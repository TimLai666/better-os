use crate::{
    app::{demo_manager, translated_component},
    i18n::{Locale, copy},
    layout::{
        ActionLayout, ColumnLayout, MIN_READABLE_CHARACTERS, MIN_WINDOW_WIDTH,
        STEP_LABEL_MIN_WIDTH, action_layout, character_advance, characters_per_line,
        first_run_column, step_label_width,
    },
    model::{CatalogLine, ComponentInfo, host_line},
};
use better_core::{ComponentIcon, ComponentId};
use manager_core::catalog::{
    CatalogDegradation, CatalogSource, CatalogStatus, ManifestRejection, RejectionReason,
};
use manager_core::{
    ComponentStatus, DesiredOperation, ManagerSettings, ManagerState, RestartRequirement,
    StoredTheme, SystemProfile,
};
use manager_platform::{SystemCapabilities, host::HostPlatform};

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

/// The first-run step list, and the width its labels are owed.
///
/// A field report from a Chinese desktop showed the step column rendered one
/// character per line beside a compatibility card that had taken the width.
/// The column had no flex grow factor, so it never claimed the space the row
/// had already given it. These lock the geometry the fix depends on.
mod step_list {
    use super::*;

    /// Every window size a person can actually produce.
    fn sizes() -> impl Iterator<Item = (f32, f32)> {
        [MIN_WINDOW_WIDTH, 1024.0, 1280.0, 1920.0, 2560.0]
            .into_iter()
            .flat_map(|width| {
                [1.0, 1.25, 1.5]
                    .into_iter()
                    .map(move |scale| (width, scale))
            })
    }

    #[test]
    fn a_step_label_is_never_narrower_than_the_readable_minimum() {
        for (width, scale) in sizes() {
            let label = step_label_width(width, scale);
            assert!(
                label >= STEP_LABEL_MIN_WIDTH,
                "{width}x{scale} gives a step label {label} wide"
            );
        }
    }

    #[test]
    fn a_step_label_holds_a_readable_run_of_characters_in_both_locales() {
        for locale in [Locale::EnUs, Locale::ZhTw] {
            let advance = character_advance(locale);
            for (width, scale) in sizes() {
                let characters = characters_per_line(step_label_width(width, scale), advance);
                assert!(
                    characters >= MIN_READABLE_CHARACTERS,
                    "{locale:?} at {width}x{scale} fits {characters} characters on a line"
                );
            }
        }
    }

    /// The regression itself, stated as the number it produced.
    #[test]
    fn a_column_that_collapses_to_its_min_content_width_is_one_character_wide() {
        let collapsed = characters_per_line(
            character_advance(Locale::ZhTw),
            character_advance(Locale::ZhTw),
        );
        assert_eq!(collapsed, 1);
        assert!(collapsed < MIN_READABLE_CHARACTERS);
    }

    #[test]
    fn the_two_first_run_columns_stack_rather_than_squeeze() {
        // Side by side while both fit at their declared minimum...
        assert_eq!(first_run_column(1920.0, 1.0).0, ColumnLayout::SideBySide);
        assert_eq!(
            first_run_column(MIN_WINDOW_WIDTH, 1.0).0,
            ColumnLayout::SideBySide
        );
        // ...and stacked once they do not, which is what keeps the label wide.
        assert_eq!(
            first_run_column(MIN_WINDOW_WIDTH, 1.5).0,
            ColumnLayout::Stacked
        );
        let (_, stacked) = first_run_column(MIN_WINDOW_WIDTH, 1.5);
        let (_, side_by_side) = first_run_column(MIN_WINDOW_WIDTH, 1.0);
        assert!(stacked > side_by_side);
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
            for width in [MIN_WINDOW_WIDTH, 1280.0, 1920.0] {
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
            assert_eq!(line.source, copy(locale).catalog_source_built_in);
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
            assert_eq!(line.source, copy(locale).catalog_source_remote);
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

        assert_eq!(at(1_000_030), "Updated moments ago");
        assert_eq!(at(1_000_000 + 5 * 60), "Updated 5 minutes ago");
        assert_eq!(at(1_000_000 + 3 * 3600), "Updated 3 hours ago");
        assert_eq!(at(1_000_000 + 4 * 86_400), "Updated 4 days ago");
        // A clock that moved backwards reads as recent, never as negative.
        assert_eq!(at(999_000), "Updated moments ago");
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
                c.catalog_source_built_in,
                c.catalog_source_cache,
                c.catalog_source_remote,
                c.catalog_never_updated,
                c.catalog_updated_just_now,
                c.catalog_updated_minutes,
                c.catalog_updated_hours,
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
    use crate::model::{AboutInfo, MANAGER_VERSION, PROJECT_REPOSITORY, UpdateCheck};

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

        let about = AboutInfo::present(Locale::EnUs, manager.profile());

        // The version is the package's own, so a release bump moves it without
        // anything else being edited.
        assert_eq!(about.version, env!("CARGO_PKG_VERSION"));
        assert_eq!(about.version, MANAGER_VERSION);
        assert_eq!(about.version.split('.').count(), 3, "{}", about.version);
        // The platform line is what ticket 45's probe read, not an assumption.
        assert_eq!(about.platform, "zorin 24.04 · amd64");
        assert_eq!(about.repository, PROJECT_REPOSITORY);
        assert!(about.repository.starts_with("https://"));
        assert!(about.name.contains(copy(Locale::EnUs).manager));
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

        let about = AboutInfo::present(Locale::ZhTw, manager.profile());
        assert_eq!(about.platform, "unknown unknown · unknown");
        assert!(!about.platform.contains("24.04"));
    }

    #[test]
    fn the_about_section_is_named_in_the_users_own_language() {
        for locale in [Locale::EnUs, Locale::ZhTw] {
            let c = copy(locale);
            for value in [
                c.about_section,
                c.about_version,
                c.about_platform,
                c.about_repository,
            ] {
                assert!(!value.trim().is_empty(), "{locale:?} is missing a string");
            }
        }
        let en = copy(Locale::EnUs);
        let zh = copy(Locale::ZhTw);
        for (english, chinese) in [
            (en.about_section, zh.about_section),
            (en.about_version, zh.about_version),
            (en.about_platform, zh.about_platform),
            (en.about_repository, zh.about_repository),
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
                1_000_000,
            );
            let UpdateCheck::Found(sentence) = check else {
                panic!("{locale:?} must report the count: {check:?}");
            };
            assert!(sentence.contains('3'), "{sentence}");
            assert!(!sentence.contains("{n}"), "{sentence}");
        }
    }

    #[test]
    fn a_check_that_found_nothing_says_so_with_the_age_of_the_list_it_checked() {
        let check = UpdateCheck::present(
            Locale::EnUs,
            true,
            false,
            &status(CatalogSource::Remote, None, Some(1_000_000)),
            0,
            1_000_000 + 5 * 60,
        );
        assert_eq!(check, UpdateCheck::UpToDate("Updated 5 minutes ago".into()));

        // A catalog nothing has ever fetched says that instead of a fake age.
        let never = UpdateCheck::present(
            Locale::EnUs,
            true,
            false,
            &status(CatalogSource::BuiltIn, None, None),
            0,
            1_000_000,
        );
        assert_eq!(
            never,
            UpdateCheck::UpToDate(copy(Locale::EnUs).catalog_never_updated.to_string())
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
                    1_000_000,
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
            1_000_000,
        );
        assert!(matches!(check, UpdateCheck::Found(_)), "{check:?}");
    }

    #[test]
    fn nothing_is_claimed_before_a_check_is_asked_for_or_while_one_runs() {
        let current = status(CatalogSource::Remote, None, Some(1_000_000));
        assert_eq!(
            UpdateCheck::present(Locale::EnUs, false, false, &current, 4, 1_000_000),
            UpdateCheck::NotRun
        );
        // Running outranks the previous result: a stale count under a spinner
        // reads as this check's answer.
        assert_eq!(
            UpdateCheck::present(Locale::EnUs, true, true, &current, 4, 1_000_000),
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
                c.update_check_found,
                c.update_check_up_to_date,
                c.update_check_failed,
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
            (en.update_check_found, zh.update_check_found),
            (en.update_check_up_to_date, zh.update_check_up_to_date),
            (en.update_check_failed, zh.update_check_failed),
        ] {
            assert_ne!(english, chinese);
        }
    }

    /// The check button and the About rows never make a row worse than what is
    /// already shipped, at every supported window size and scale.
    #[test]
    fn the_new_labels_lay_out_no_worse_than_the_ones_already_shipped() {
        for locale in [Locale::EnUs, Locale::ZhTw] {
            let c = copy(locale);
            let new_longest = [
                c.check_updates,
                c.checking_updates,
                c.about_version,
                c.about_platform,
                c.about_repository,
            ]
            .iter()
            .map(|label| label.chars().count())
            .max()
            .unwrap();
            let shipped_longest = [c.install_updates, c.restore_previous, c.catalog_refreshing]
                .iter()
                .map(|label| label.chars().count())
                .max()
                .unwrap();
            assert!(
                new_longest <= shipped_longest,
                "{locale:?} added a label longer than anything already on screen"
            );
            for width in [MIN_WINDOW_WIDTH, 1280.0, 1920.0] {
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
        let characters = characters_per_line(MIN_WINDOW_WIDTH, 1.0);
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
