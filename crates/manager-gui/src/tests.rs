use crate::{
    app::{demo_manager, translated_component},
    i18n::{Locale, copy},
    layout::{
        ActionLayout, ColumnLayout, MIN_READABLE_CHARACTERS, MIN_WINDOW_WIDTH,
        STEP_LABEL_MIN_WIDTH, action_layout, character_advance, characters_per_line,
        first_run_column, step_label_width,
    },
    model::{CatalogLine, ComponentInfo},
};
use better_core::{ComponentIcon, ComponentId};
use manager_core::catalog::{
    CatalogDegradation, CatalogSource, CatalogStatus, ManifestRejection, RejectionReason,
};
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
