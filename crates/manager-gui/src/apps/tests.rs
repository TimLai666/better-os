use super::*;
use crate::model::ActivityFilter;
use better_core::ComponentId;
use manager_core::{ComponentRecord, FailureRecord, PlanStep};

fn id(value: &str) -> ComponentId {
    ComponentId::new(value).unwrap()
}

fn manager() -> manager_core::Manager {
    manager_core::Manager::probe(
        manager_core::catalog::built_in_catalog(),
        &manager_platform::MockPlatform::default(),
    )
    .unwrap()
}

fn component(name: &str, record: Option<ComponentRecord>, state: ComponentStatus) -> ComponentInfo {
    let manager = manager();
    let manifest = manager
        .manifests()
        .find(|manifest| manifest.id.as_str() == name)
        .unwrap()
        .clone();
    ComponentInfo::present(&manifest, record.as_ref(), state, None)
}

fn installed(version: &str) -> ComponentRecord {
    ComponentRecord {
        installed_version: Some(version.to_string()),
        enabled: true,
        ..ComponentRecord::default()
    }
}

fn step(name: &str, operation: DesiredOperation, after: Option<&str>) -> PlanStep {
    let manager = manager();
    let state = manager_core::ManagerState::default();
    let mut step = manager
        .plan_in_mode(
            &state,
            &id(name),
            DesiredOperation::Install,
            manager_core::ExecutionMode::Mock,
        )
        .unwrap()
        .steps()
        .iter()
        .find(|step| step.component.as_str() == name)
        .unwrap()
        .clone();
    step.operation = operation;
    step.after_version = after.map(str::to_string);
    step
}

#[test]
fn the_launch_check_obeys_check_for_updates_when_opened() {
    assert!(refreshes_at_launch(true, false));
    assert!(!refreshes_at_launch(false, false), "the setting is off");
    assert!(!refreshes_at_launch(true, true), "BETTER_MANAGER_OFFLINE");
    assert!(!refreshes_at_launch(false, true));
}

#[test]
fn the_window_calls_the_launch_rule_with_the_stored_setting() {
    let source = include_str!("../app.rs");
    let start = source.find("fn refresh_catalog_at_launch").unwrap();
    let body = &source[start..start + 700];
    assert!(body.contains("crate::apps::refreshes_at_launch("));
    assert!(body.contains("self.state.settings.check_updates"));
    assert!(body.contains("BETTER_MANAGER_OFFLINE"));
}

#[test]
fn each_filter_shows_what_its_name_says() {
    let available = component("better-files", None, ComponentStatus::Available);
    let update = component(
        "better-files",
        Some(installed("0.0.1")),
        ComponentStatus::UpdateAvailable,
    );
    let off = component(
        "better-files",
        Some(installed("0.0.1")),
        ComponentStatus::Disabled,
    );
    let failed = component(
        "better-files",
        Some(installed("0.0.1")),
        ComponentStatus::Failed,
    );
    use ComponentFilterPreference as F;
    assert!(matches_filter(&available, F::All));
    assert!(!matches_filter(&available, F::Installed));
    assert!(matches_filter(&update, F::Installed));
    assert!(matches_filter(&update, F::Updates));
    assert!(!matches_filter(&off, F::Updates));
    assert!(matches_filter(&off, F::Disabled));
    assert!(matches_filter(&failed, F::NeedsAttention));
    assert!(!matches_filter(&update, F::NeedsAttention));
    for locale in [Locale::EnUs, Locale::ZhTw] {
        let labels: Vec<_> = FILTERS.iter().map(|f| filter_label(locale, *f)).collect();
        assert_eq!(labels.len(), 5);
    }
    assert_eq!(
        FILTERS.map(|f| filter_label(Locale::ZhTw, f)),
        ["全部", "已安裝", "可更新", "已停用", "需要處理"]
    );
}

#[test]
fn search_matches_name_summary_and_id_ignoring_case_and_any_chinese_substring() {
    let files = component("better-files", None, ComponentStatus::Available);
    assert!(matches_search(&files, ""));
    assert!(matches_search(&files, "  FILES "));
    assert!(matches_search(&files, "better-files"));
    assert!(!matches_search(&files, "launcher"));
    let mut translated = files.clone();
    translated.summary = "安全移除外接磁碟".to_string();
    assert!(matches_search(&translated, "外接"));
}

#[test]
fn downloads_run_in_plan_order_and_later_stages_cover_every_row() {
    let steps = vec![
        step("better-files", DesiredOperation::Update, Some("0.3.0")),
        step("better-git", DesiredOperation::Update, Some("0.3.0")),
        step("better-launcher", DesiredOperation::Update, Some("0.3.0")),
    ];
    let run = RunFacts {
        steps: &steps,
        stage: OperationStage::Downloading,
        transfer: Some(("better-git", 5_000_000, Some(10_000_000))),
    };
    assert_eq!(
        row_progress(&run, &id("better-files")),
        Some(RowProgress::Downloaded)
    );
    assert_eq!(
        row_progress(&run, &id("better-git")),
        Some(RowProgress::Downloading {
            received: 5_000_000,
            total: Some(10_000_000)
        })
    );
    assert_eq!(
        row_progress(&run, &id("better-launcher")),
        Some(RowProgress::Preparing)
    );
    assert_eq!(row_progress(&run, &id("better-monitor")), None);
    let downloading = row_progress(&run, &id("better-git")).unwrap();
    assert_eq!(downloading.fraction(), Some(0.5));
    assert_eq!(
        downloading.caption(Locale::ZhTw),
        "正在下載 5.0 MB / 10.0 MB"
    );
    assert_eq!(
        downloading.caption(Locale::EnUs),
        "Downloading 5.0 MB of 10.0 MB"
    );

    let installing = RunFacts {
        stage: OperationStage::Installing,
        ..run.clone()
    };
    let row = row_progress(&installing, &id("better-launcher")).unwrap();
    assert_eq!(row, RowProgress::Stage(OperationStage::Installing));
    assert_eq!(row.fraction(), None, "no length to measure: indeterminate");
    assert_eq!(
        row.caption(Locale::ZhTw),
        crate::i18n::copy(Locale::ZhTw).installing_files
    );
}

#[test]
fn a_download_of_unknown_size_is_indeterminate() {
    let progress = RowProgress::Downloading {
        received: 1_500_000,
        total: None,
    };
    assert_eq!(progress.fraction(), None);
    assert_eq!(progress.caption(Locale::EnUs), "Downloading 1.5 MB");
}

#[test]
fn a_row_shows_one_thing_in_order_of_what_matters() {
    let update = component(
        "better-files",
        Some(installed("0.0.1")),
        ComponentStatus::UpdateAvailable,
    );
    let facts = |component, progress, result: Option<&str>, unorderable| RowFacts {
        component,
        unorderable,
        progress,
        result: result.map(str::to_string),
        retry: DesiredOperation::Verify,
    };
    // Progress first, while the change runs.
    assert_eq!(
        trailing(
            Locale::EnUs,
            &facts(&update, Some(RowProgress::Preparing), None, false)
        ),
        Trailing::Progress(RowProgress::Preparing)
    );
    // Nothing can be planned: View says why.
    assert_eq!(
        trailing(Locale::EnUs, &facts(&update, None, None, true)),
        Trailing::View
    );
    // The one action.
    assert_eq!(
        trailing(Locale::EnUs, &facts(&update, None, None, false)),
        Trailing::Action(DesiredOperation::Update)
    );
    let available = component("better-files", None, ComponentStatus::Available);
    assert_eq!(
        trailing(Locale::EnUs, &facts(&available, None, None, false)),
        Trailing::Action(DesiredOperation::Install)
    );
    let off = component(
        "better-files",
        Some(installed("0.0.1")),
        ComponentStatus::Disabled,
    );
    assert_eq!(
        trailing(Locale::EnUs, &facts(&off, None, None, false)),
        Trailing::Action(DesiredOperation::Enable)
    );
    // Nothing to do: the state word, not a button.
    let healthy = component(
        "better-files",
        Some(installed("0.2.9")),
        ComponentStatus::Healthy,
    );
    assert_eq!(
        trailing(Locale::ZhTw, &facts(&healthy, None, None, false)),
        Trailing::State("已安裝 0.2.9".to_string())
    );
    // What just finished, until the next change.
    assert_eq!(
        trailing(
            Locale::EnUs,
            &facts(&healthy, None, Some("Updated to 0.2.9"), false)
        ),
        Trailing::Result("Updated to 0.2.9".to_string())
    );
}

#[test]
fn a_failure_offers_retry_and_restore_only_when_a_previous_version_is_kept() {
    let mut record = installed("0.0.1");
    record.failure = Some(FailureRecord {
        component: id("better-files"),
        stage: OperationStage::CheckingHealth,
        evidence: "health.failed".to_string(),
        detail: None,
        recovery: None,
    });
    let failed = component(
        "better-files",
        Some(record.clone()),
        ComponentStatus::Failed,
    );
    let facts = RowFacts {
        component: &failed,
        unorderable: false,
        progress: None,
        result: Some("Updated to 0.3.0".to_string()),
        retry: DesiredOperation::Verify,
    };
    assert_eq!(
        trailing(Locale::EnUs, &facts),
        Trailing::Failed {
            retry: DesiredOperation::Verify,
            restore: false
        },
        "a failure outranks the result of the change that caused it"
    );
    record.restore_snapshot = Some(manager_core::ComponentSnapshot {
        installed_version: Some("0.0.1".to_string()),
        enabled: true,
        health: manager_core::HealthState::Healthy,
        artifact: None,
    });
    let restorable = component(
        "better-files",
        Some(record),
        ComponentStatus::RestoreAvailable,
    );
    assert_eq!(
        trailing(
            Locale::EnUs,
            &RowFacts {
                component: &restorable,
                ..facts.clone()
            }
        ),
        Trailing::Failed {
            retry: DesiredOperation::Verify,
            restore: true
        }
    );
}

#[test]
fn only_removing_and_restoring_ask_first() {
    for operation in [
        DesiredOperation::Install,
        DesiredOperation::Update,
        DesiredOperation::Enable,
        DesiredOperation::Disable,
        DesiredOperation::Verify,
    ] {
        assert!(!asks_first(operation), "{operation:?}");
    }
    assert!(asks_first(DesiredOperation::Remove));
    assert!(asks_first(DesiredOperation::Restore));
    assert_eq!(
        Dialog::remove(id("better-git")),
        Dialog::Remove {
            id: id("better-git"),
            delete_data: true
        },
        "the box is ticked by default"
    );
}

#[test]
fn each_finished_operation_says_what_it_did_in_both_locales() {
    let cases = [
        (DesiredOperation::Install, "Installed 0.3.0", "已安裝 0.3.0"),
        (
            DesiredOperation::Update,
            "Updated to 0.3.0",
            "已更新到 0.3.0",
        ),
        (DesiredOperation::Enable, "Turned on", "已啟用"),
        (DesiredOperation::Disable, "Turned off", "已停用"),
        (
            DesiredOperation::Verify,
            "Checked: it works",
            "已檢查，可以正常使用",
        ),
        (
            DesiredOperation::Restore,
            "Restored to 0.3.0",
            "已還原成 0.3.0",
        ),
        (DesiredOperation::Remove, "Removed", "已移除"),
    ];
    for (operation, en, zh) in cases {
        let step = step("better-files", operation, Some("0.3.0"));
        assert_eq!(result_line(Locale::EnUs, &step), en);
        assert_eq!(result_line(Locale::ZhTw, &step), zh);
    }
    let unknown = step("better-files", DesiredOperation::Restore, None);
    assert_eq!(result_line(Locale::ZhTw, &unknown), "已還原上一個版本");
}

#[test]
fn the_running_card_counts_what_it_changes() {
    let one = vec![step("better-files", DesiredOperation::Update, None)];
    let two = vec![
        step("better-files", DesiredOperation::Update, None),
        step("better-git", DesiredOperation::Update, None),
    ];
    let mixed = vec![
        step("better-files", DesiredOperation::Install, None),
        step("better-git", DesiredOperation::Enable, None),
    ];
    assert_eq!(run_title(Locale::EnUs, &one), "Updating 1 component");
    assert_eq!(run_title(Locale::ZhTw, &two), "正在更新 2 個元件");
    assert_eq!(run_title(Locale::EnUs, &mixed), "Changing 2 components");
}

#[test]
fn the_cost_of_an_update_is_small_text_and_never_a_gate() {
    let mut update = step("better-files", DesiredOperation::Update, Some("0.3.0"));
    update.before_version = Some("0.2.9".to_string());
    update.estimated_download_bytes = Some(12_300_000);
    update.rollback_available = true;
    assert_eq!(
        update_facts(
            Locale::EnUs,
            &update,
            Some("Reopens by itself if it is open")
        ),
        "0.2.9 → 0.3.0 · 12.3 MB download · Reopens by itself if it is open · Previous version kept"
    );
    update.estimated_download_bytes = None;
    update.rollback_available = false;
    assert_eq!(update_facts(Locale::ZhTw, &update, None), "0.2.9 → 0.3.0");
}

#[test]
fn the_health_header_counts_what_needs_attention() {
    assert_eq!(
        health_header(Locale::EnUs, 0, 0),
        crate::i18n::copy(Locale::EnUs).all_checks_passed
    );
    assert_eq!(health_header(Locale::ZhTw, 1, 0), "有 1 項檢查需要處理");
    assert_eq!(health_header(Locale::EnUs, 1, 2), "3 checks need attention");
}

#[test]
fn the_activity_filters_sort_every_kind() {
    use manager_core::ActivityKind as K;
    assert!(activity_matches(
        ActivityFilter::Success,
        K::RecoverySuccess
    ));
    assert!(activity_matches(ActivityFilter::Warning, K::ManualRecovery));
    assert!(activity_matches(ActivityFilter::Failure, K::Failure));
    assert!(!activity_matches(ActivityFilter::Failure, K::Warning));
    assert!(activity_matches(ActivityFilter::All, K::Information));
}

/// Every first-party component reads in the current language (audit M3).
#[test]
fn every_built_in_component_has_its_line_in_both_locales() {
    for manifest in manager_core::catalog::built_in_manifests() {
        for locale in [Locale::EnUs, Locale::ZhTw] {
            assert!(
                crate::app::translated_component(locale, &manifest.id).is_some(),
                "{} has no shipped copy",
                manifest.id
            );
        }
        let en = crate::app::translated_component(Locale::EnUs, &manifest.id).unwrap();
        let zh = crate::app::translated_component(Locale::ZhTw, &manifest.id).unwrap();
        assert_ne!(en.summary, zh.summary, "{}", manifest.id);
        assert_ne!(en.detail, zh.detail, "{}", manifest.id);
    }
}

/// The copy is translated, and nothing makes a person wait on a word
/// (owner, 2026-11-15: no 等待確認).
#[test]
fn the_page_s_copy_is_translated_and_never_says_waiting_for_confirmation() {
    let en = copy(Locale::EnUs);
    let zh = copy(Locale::ZhTw);
    let pairs = [
        (en.group_components, zh.group_components),
        (en.search_placeholder, zh.search_placeholder),
        (en.refresh_list, zh.refresh_list),
        (en.update_all, zh.update_all),
        (en.action_retry, zh.action_retry),
        (en.action_restore, zh.action_restore),
        (en.remove_title, zh.remove_title),
        (en.delete_data, zh.delete_data),
        (en.restore_title, zh.restore_title),
        (en.clear_title, zh.clear_title),
        (en.self_update_detail, zh.self_update_detail),
        (en.run_full_check, zh.run_full_check),
        (en.available_updates, zh.available_updates),
    ];
    for (en, zh) in pairs {
        assert_ne!(en, zh);
        for text in [en, zh] {
            assert!(!text.contains("等待確認"), "{text}");
            assert!(!text.to_lowercase().contains("waiting for"), "{text}");
        }
    }
    assert_eq!(zh.delete_data, "同時刪除我的設定和紀錄");
    assert_eq!(en.delete_data, "Also delete my settings and history");
    assert_eq!(zh.clear_title, "清除所有活動紀錄？");
    assert_eq!(en.clear_title, "Clear all activity?");
}

#[test]
fn the_new_labels_fit_the_narrowest_window() {
    // The row's trailing edge holds two buttons at most: Retry and Restore.
    for locale in [Locale::EnUs, Locale::ZhTw] {
        let c = copy(locale);
        let width = crate::layout::characters_per_line(220.0, 7.0);
        for text in [
            c.action_retry,
            c.action_restore,
            c.update_all,
            c.action_view,
        ] {
            assert!(text.chars().count() <= width, "{text}");
        }
    }
}

#[test]
fn an_open_window_checks_again_by_itself_when_the_list_is_six_hours_old() {
    let hour = 3600;
    let now = 100 * hour;
    // Fetched an hour ago: nothing to do.
    assert!(!periodic_check_due(
        true,
        false,
        false,
        Some(now - hour),
        None,
        now
    ));
    // Six hours old: check.
    assert!(periodic_check_due(
        true,
        false,
        false,
        Some(now - 6 * hour),
        None,
        now
    ));
    // Never fetched and never tried: check.
    assert!(periodic_check_due(true, false, false, None, None, now));
    // A check that failed ten minutes ago is not repeated every tick.
    assert!(!periodic_check_due(
        true,
        false,
        false,
        Some(now - 7 * hour),
        Some(now - 600),
        now
    ));
    // The setting, the offline rule, and a running check or change all stop it.
    assert!(!periodic_check_due(false, false, false, None, None, now));
    assert!(!periodic_check_due(true, true, false, None, None, now));
    assert!(!periodic_check_due(true, false, true, None, None, now));
}

#[test]
fn before_the_first_byte_every_row_is_downloading_with_no_length_yet() {
    let steps = vec![step("better-files", DesiredOperation::Update, None)];
    let run = RunFacts {
        steps: &steps,
        stage: OperationStage::Downloading,
        transfer: None,
    };
    let row = row_progress(&run, &id("better-files")).unwrap();
    assert_eq!(row, RowProgress::Stage(OperationStage::Downloading));
    assert_eq!(row.fraction(), None);
}

/// Audit M13: no two first-party components share a glyph in the Apps list.
#[test]
fn every_built_in_component_has_its_own_glyph() {
    use crate::components::{first_party_glyph, manifest_glyph};
    let glyphs: Vec<String> = manager_core::catalog::built_in_manifests()
        .iter()
        .map(|manifest| {
            let glyph =
                first_party_glyph(manifest.id.as_str()).unwrap_or(manifest_glyph(manifest.icon));
            gpui_component::IconNamed::path(glyph).to_string()
        })
        .collect();
    let mut unique = glyphs.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), glyphs.len(), "{glyphs:?}");
}

/// Ticket 124: adding Better Manager's missing parts starts at once like
/// every other row action, names the parts on the row, and says what it did.
#[test]
fn adding_missing_parts_names_them_and_reports_its_own_result() {
    let manager = manager();
    let self_id = id(manager_core::SELF_COMPONENT_ID);
    let version = manager
        .manifests()
        .find(|manifest| manifest.id == self_id)
        .unwrap()
        .version
        .to_string();
    let mut state = manager_core::ManagerState::default();
    state.set_installed(self_id.clone(), version, true);
    let plan = manager
        .plan(&state, &self_id, DesiredOperation::AddCompanions)
        .expect("the shipped manifest names companions");
    let step = &plan.steps()[0];
    for locale in [Locale::EnUs, Locale::ZhTw] {
        let facts = update_facts(locale, step, None);
        assert!(facts.contains("better-shell-adapter"), "{facts}");
        assert!(facts.contains("better-manager-daemon"), "{facts}");
        assert_eq!(result_line(locale, step), copy(locale).result_parts_added);
        assert_eq!(
            action_label(locale, DesiredOperation::AddCompanions),
            copy(locale).action_add_parts
        );
    }
    assert_ne!(
        copy(Locale::EnUs).result_parts_added,
        copy(Locale::ZhTw).result_parts_added
    );
}

/// Ticket 124 on ticket 85's design: the service notice and the
/// window-attention Install start the change with no review page, and the
/// notice shows the running change in place of its button.
#[test]
fn the_service_and_adapter_buttons_start_at_once() {
    let app = include_str!("../app.rs");
    let service = app
        .split("pub(crate) fn start_service_update(")
        .nth(1)
        .unwrap()
        .split("\n    }\n")
        .next()
        .unwrap();
    assert!(service.contains("self.start_operation(&id, operation, cx)"));
    let attention = include_str!("../window_attention_app.rs")
        .split("pub(crate) fn install_shell_adapter(")
        .nth(1)
        .unwrap()
        .split("\n    }\n")
        .next()
        .unwrap();
    assert!(attention.contains("self.start_operation(&id, operation, cx)"));
    let card = include_str!("../render_apps.rs")
        .split("fn service_notice_card(")
        .nth(1)
        .unwrap()
        .split("\n    }\n")
        .next()
        .unwrap();
    assert!(card.contains("this.start_service_update(action, cx)"));
    let in_place = include_str!("../render_apps.rs")
        .split("pub(crate) fn service_change_card(")
        .nth(1)
        .unwrap()
        .split("\n    }\n")
        .next()
        .unwrap();
    // Progress in place, and a Retry that starts the same change again.
    assert!(in_place.contains("Trailing::Progress(_)"));
    assert!(in_place.contains("a.action_retry"));
    assert_eq!(in_place.matches("start(this, cx)").count(), 2);
    let attention = include_str!("../render_window_attention.rs");
    assert!(attention.contains("|this, cx| this.install_shell_adapter(cx)"));
    // A finished change reads the service again, so the notice goes away.
    assert!(
        app.contains("self.read_service_facts();\n        if self.page == Page::WindowAttention")
    );
}

/// The remove confirmation says up front that a setting that cannot be put
/// back never stops the removal, and where it is reported (ticket 88 review).
#[test]
fn the_remove_confirmation_says_settings_are_put_back_or_reported() {
    assert!(
        copy(Locale::EnUs)
            .remove_body
            .contains("removal still finishes")
    );
    assert!(copy(Locale::EnUs).remove_body.contains("Activity"));
    assert!(copy(Locale::ZhTw).remove_body.contains("移除仍會完成"));
    assert!(copy(Locale::ZhTw).remove_body.contains("活動紀錄"));
}
