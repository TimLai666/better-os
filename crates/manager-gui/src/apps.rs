//! The Apps page (spec section 7), with no GPUI in it: what each component
//! row shows at its trailing edge, the progress a running change draws on
//! the rows it touches, what a finished change says, the filters and the
//! search, the confirmation dialogs, and the page's copy.
//!
//! Updates work like Windows Update (owner, 2026-11-15): there is no review
//! page. Update all, Install, Update, Turn on, Turn off, and Check again start
//! at once as one transaction; each affected row shows its own progress and
//! then its result, a failed row says why and offers Retry and Restore, and
//! only removing a component and restoring an older version ask first.

use better_core::ComponentId;
use manager_core::{
    ActivityKind, ComponentFilterPreference, ComponentStatus, DesiredOperation, OperationStage,
    PlanStep,
};

use crate::i18n::Locale;
use crate::model::ComponentInfo;

/// How long a demo window waits between the stages it steps through by
/// itself. A real transaction moves as fast as the work does.
pub(crate) const DEMO_STEP: std::time::Duration = std::time::Duration::from_millis(1100);

/// Whether the window checks for updates when it opens (ticket 85): only
/// when the stored setting asks for it, and never when the run was told to
/// stay offline.
pub(crate) fn refreshes_at_launch(check_updates: bool, offline: bool) -> bool {
    check_updates && !offline
}

/// How long a window left open waits between update checks of its own
/// (ticket 46): a window open for a week should not keep planning from a
/// week-old list until someone presses a button.
pub(crate) const PERIODIC_CHECK_EVERY: u64 = 6 * 60 * 60;

/// How often an open window asks itself whether that check is due.
pub(crate) const PERIODIC_TICK: std::time::Duration = std::time::Duration::from_secs(10 * 60);

/// Whether an open window should check for updates now, by itself: the same
/// setting and the same offline rule as the launch check, never while a
/// check or a change is running, and only once the list is
/// [`PERIODIC_CHECK_EVERY`] old, counting the last attempt so a failing
/// network is not asked again every tick.
pub(crate) fn periodic_check_due(
    check_updates: bool,
    offline: bool,
    busy: bool,
    last_fetched: Option<u64>,
    last_attempted: Option<u64>,
    now: u64,
) -> bool {
    if !refreshes_at_launch(check_updates, offline) || busy {
        return false;
    }
    match last_fetched.max(last_attempted) {
        None => true,
        Some(at) => now.saturating_sub(at) >= PERIODIC_CHECK_EVERY,
    }
}

/// The filter views, in the `SelectorBar`'s order.
pub(crate) const FILTERS: [ComponentFilterPreference; 5] = [
    ComponentFilterPreference::All,
    ComponentFilterPreference::Installed,
    ComponentFilterPreference::Updates,
    ComponentFilterPreference::Disabled,
    ComponentFilterPreference::NeedsAttention,
];

pub(crate) fn filter_label(locale: Locale, filter: ComponentFilterPreference) -> &'static str {
    let c = copy(locale);
    match filter {
        ComponentFilterPreference::All => c.filter_all,
        ComponentFilterPreference::Installed => c.filter_installed,
        ComponentFilterPreference::Updates => c.filter_updates,
        ComponentFilterPreference::Disabled => c.filter_turned_off,
        ComponentFilterPreference::NeedsAttention => c.filter_attention,
    }
}

/// Whether a component belongs in a filter's view.
pub(crate) fn matches_filter(component: &ComponentInfo, filter: ComponentFilterPreference) -> bool {
    match filter {
        ComponentFilterPreference::All => true,
        ComponentFilterPreference::Installed => component.installed_version.is_some(),
        ComponentFilterPreference::Updates => component.state == ComponentStatus::UpdateAvailable,
        ComponentFilterPreference::Disabled => component.state == ComponentStatus::Disabled,
        ComponentFilterPreference::NeedsAttention => needs_attention(component),
    }
}

/// A component something is wrong with: a failure, a check that did not
/// pass, a record that disagrees with the machine, or one this PC cannot run.
pub(crate) fn needs_attention(component: &ComponentInfo) -> bool {
    component.drift.is_some()
        || matches!(
            component.state,
            ComponentStatus::Degraded
                | ComponentStatus::Failed
                | ComponentStatus::RestoreAvailable
                | ComponentStatus::Incompatible
        )
}

/// Whether a component matches what was typed in "Search components": its
/// name, its one line, or its id, ignoring case. Chinese has no word breaks,
/// so any substring matches.
pub(crate) fn matches_search(component: &ComponentInfo, query: &str) -> bool {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return true;
    }
    [
        component.name.as_str(),
        component.summary.as_str(),
        component.core_id.as_str(),
    ]
    .iter()
    .any(|field| field.to_lowercase().contains(&query))
}

/// What the running transaction is doing, as the rows need to know it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RunFacts<'a> {
    pub(crate) steps: &'a [PlanStep],
    pub(crate) stage: OperationStage,
    /// The component whose package is being downloaded, and how far.
    pub(crate) transfer: Option<(&'a str, u64, Option<u64>)>,
}

/// Where one row is in a running change.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum RowProgress {
    /// In the change, its turn not come yet.
    Preparing,
    Downloading {
        received: u64,
        total: Option<u64>,
    },
    /// Its package is here; others are still downloading.
    Downloaded,
    /// The whole change is at this stage, which runs for every component at
    /// once.
    Stage(OperationStage),
}

impl RowProgress {
    /// The bar's fill: a known fraction while bytes are counted, nothing
    /// (an indeterminate bar) while the work has no measurable length.
    pub(crate) fn fraction(&self) -> Option<f32> {
        match self {
            RowProgress::Preparing => Some(0.0),
            RowProgress::Downloading {
                received,
                total: Some(total),
            } if *total > 0 => Some((*received as f32 / *total as f32).clamp(0.0, 1.0)),
            RowProgress::Downloading { .. } => None,
            RowProgress::Downloaded => Some(1.0),
            RowProgress::Stage(_) => None,
        }
    }

    pub(crate) fn caption(&self, locale: Locale) -> String {
        let c = copy(locale);
        let base = crate::i18n::copy(locale);
        match self {
            RowProgress::Preparing => c.progress_preparing.to_string(),
            RowProgress::Downloading {
                received,
                total: Some(total),
            } => c
                .progress_downloading
                .replace("{received}", &megabytes(*received))
                .replace("{total}", &megabytes(*total)),
            RowProgress::Downloading { received, .. } => c
                .progress_downloading_unknown
                .replace("{received}", &megabytes(*received)),
            RowProgress::Downloaded => c.progress_downloaded.to_string(),
            RowProgress::Stage(OperationStage::Downloading) => base.downloading.to_string(),
            RowProgress::Stage(OperationStage::Installing) => base.installing_files.to_string(),
            RowProgress::Stage(OperationStage::ApplyingSettings) => {
                base.applying_settings.to_string()
            }
            RowProgress::Stage(OperationStage::CheckingHealth) => base.checking_works.to_string(),
        }
    }
}

/// A row's place in the running change, or `None` when it is not in it.
///
/// Downloads run one component after another in the plan's order, so the
/// rows before the one being fetched are done and the ones after it have not
/// started. Every later stage is one step for the whole change.
pub(crate) fn row_progress(run: &RunFacts<'_>, id: &ComponentId) -> Option<RowProgress> {
    let index = run.steps.iter().position(|step| &step.component == id)?;
    if run.stage != OperationStage::Downloading {
        return Some(RowProgress::Stage(run.stage));
    }
    // Before the first byte is counted (and in a demo, which moves no
    // bytes) the download is under way with no length to show yet.
    let Some((current, received, total)) = run.transfer else {
        return Some(RowProgress::Stage(OperationStage::Downloading));
    };
    let current_index = run
        .steps
        .iter()
        .position(|step| step.component.as_str() == current);
    Some(match current_index {
        Some(at) if at == index => RowProgress::Downloading { received, total },
        Some(at) if at > index => RowProgress::Downloaded,
        _ => RowProgress::Preparing,
    })
}

/// What a finished change says on a row it touched.
pub(crate) fn result_line(locale: Locale, step: &PlanStep) -> String {
    let c = copy(locale);
    let version = step.after_version.as_deref().unwrap_or_default();
    match step.operation {
        DesiredOperation::Install => c.result_installed.replace("{version}", version),
        DesiredOperation::Update => c.result_updated.replace("{version}", version),
        DesiredOperation::Enable => c.result_turned_on.to_string(),
        DesiredOperation::Disable => c.result_turned_off.to_string(),
        DesiredOperation::Verify => c.result_checked.to_string(),
        DesiredOperation::Restore => match step.after_version.as_deref() {
            Some(version) => c.result_restored.replace("{version}", version),
            None => c.result_restored_unknown.to_string(),
        },
        DesiredOperation::Remove => c.result_removed.to_string(),
        DesiredOperation::AddCompanions => c.result_parts_added.to_string(),
    }
}

/// The one thing at a component row's trailing edge (audit M2).
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Trailing {
    /// A running change is working on it.
    Progress(RowProgress),
    /// The last change failed, or its check did not pass: the description
    /// says why, and these are the ways out.
    Failed {
        retry: DesiredOperation,
        restore: bool,
    },
    /// What the change that just finished did.
    Result(String),
    /// One standard button that starts this operation at once.
    Action(DesiredOperation),
    /// Nothing can be done from the row (the record disagrees with the
    /// machine, or dpkg holds a version nothing can order): View opens the
    /// page that says why.
    View,
    /// Nothing to do: the state word, in secondary text.
    State(String),
}

/// What decides a row's trailing edge.
#[derive(Clone, Debug)]
pub(crate) struct RowFacts<'a> {
    pub(crate) component: &'a ComponentInfo,
    /// dpkg holds it at a version nothing can order (ticket 69).
    pub(crate) unorderable: bool,
    pub(crate) progress: Option<RowProgress>,
    /// The result line of the change that just finished, when it touched
    /// this component and succeeded.
    pub(crate) result: Option<String>,
    /// What "Retry" means for it: installing again when nothing is
    /// installed, a health check otherwise.
    pub(crate) retry: DesiredOperation,
}

pub(crate) fn trailing(locale: Locale, facts: &RowFacts<'_>) -> Trailing {
    let component = facts.component;
    if let Some(progress) = &facts.progress {
        return Trailing::Progress(progress.clone());
    }
    if facts.unorderable || component.drift.is_some() {
        return Trailing::View;
    }
    if component.failure.is_some()
        || matches!(
            component.state,
            ComponentStatus::Failed | ComponentStatus::Degraded | ComponentStatus::RestoreAvailable
        )
    {
        return Trailing::Failed {
            retry: facts.retry,
            restore: component.restore_available,
        };
    }
    if let Some(result) = &facts.result {
        return Trailing::Result(result.clone());
    }
    let c = copy(locale);
    match component.state {
        ComponentStatus::Available => Trailing::Action(DesiredOperation::Install),
        ComponentStatus::UpdateAvailable => Trailing::Action(DesiredOperation::Update),
        ComponentStatus::Disabled => Trailing::Action(DesiredOperation::Enable),
        ComponentStatus::Incompatible => Trailing::State(c.state_incompatible.to_string()),
        _ => Trailing::State(
            c.state_installed.replace(
                "{version}",
                component
                    .installed_version
                    .as_deref()
                    .unwrap_or(&component.available_version),
            ),
        ),
    }
}

/// The label of a button that starts `operation`.
pub(crate) fn action_label(locale: Locale, operation: DesiredOperation) -> &'static str {
    let c = copy(locale);
    match operation {
        DesiredOperation::Install => c.action_install,
        DesiredOperation::Update => c.action_update,
        DesiredOperation::Enable => c.action_turn_on,
        DesiredOperation::Disable => c.action_turn_off,
        DesiredOperation::Verify => c.action_check,
        DesiredOperation::Restore => c.action_restore,
        DesiredOperation::Remove => c.action_remove,
        DesiredOperation::AddCompanions => c.action_add_parts,
    }
}

/// Whether an operation asks before it starts. Only two do (owner,
/// 2026-11-15): removing a component, and putting an older version back.
pub(crate) fn asks_first(operation: DesiredOperation) -> bool {
    matches!(
        operation,
        DesiredOperation::Remove | DesiredOperation::Restore
    )
}

/// The running card's title: what the change is doing, and to how many.
pub(crate) fn run_title(locale: Locale, steps: &[PlanStep]) -> String {
    let c = copy(locale);
    let count = steps.len();
    let all = |operation| steps.iter().all(|step| step.operation == operation);
    let (one, many) = if all(DesiredOperation::Update) {
        (c.run_updating_one, c.run_updating)
    } else if all(DesiredOperation::Install) {
        (c.run_installing_one, c.run_installing)
    } else {
        (c.run_working_one, c.run_working)
    };
    if count == 1 {
        one.to_string()
    } else {
        many.replace("{n}", &count.to_string())
    }
}

/// The small text under an update row: the versions, the download, what
/// happens afterwards, and whether the previous version is kept. Each part
/// is there only when the plan knows it; none of it is a gate.
pub(crate) fn update_facts(locale: Locale, step: &PlanStep, afterwards: Option<&str>) -> String {
    let c = copy(locale);
    let mut parts = Vec::new();
    match (&step.before_version, &step.after_version) {
        (Some(before), Some(after)) => parts.push(format!("{before} → {after}")),
        (None, Some(after)) => parts.push(after.clone()),
        _ => {}
    }
    // Adding missing parts names them: nothing else about the row changes
    // (ticket 124).
    if step.operation == DesiredOperation::AddCompanions
        && let Some(artifact) = &step.artifact
        && !artifact.companions.is_empty()
    {
        let packages: Vec<&str> = artifact
            .companions
            .iter()
            .map(|companion| companion.package.as_str())
            .collect();
        parts.push(c.adds_parts.replace("{parts}", &packages.join(", ")));
    }
    if let Some(bytes) = step.estimated_download_bytes {
        parts.push(c.download_size.replace("{size}", &megabytes(bytes)));
    }
    if let Some(afterwards) = afterwards {
        parts.push(afterwards.to_string());
    }
    if step.rollback_available {
        parts.push(c.previous_kept.to_string());
    }
    parts.join(" · ")
}

/// Bytes in decimal megabytes with one decimal, as the download line reads.
pub(crate) fn megabytes(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / 1_000_000.0)
}

/// What the Apps page is asking the person to confirm.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Dialog {
    /// Remove a component; `delete_data` is the check box, ticked by
    /// default, shown only when the component declares personal data.
    Remove { id: ComponentId, delete_data: bool },
    /// Put the previous version back.
    Restore { id: ComponentId },
    /// Record what the system has as the truth (drift).
    Adopt { id: ComponentId },
    /// Clear the activity log.
    ClearActivity,
}

impl Dialog {
    pub(crate) fn remove(id: ComponentId) -> Self {
        Dialog::Remove {
            id,
            delete_data: true,
        }
    }
}

/// The Health page's summary header.
pub(crate) fn health_header(locale: Locale, failed: usize, warnings: usize) -> String {
    let c = copy(locale);
    match failed + warnings {
        0 => crate::i18n::copy(locale).all_checks_passed.to_string(),
        1 => c.health_one_needs_attention.to_string(),
        n => c.health_needs_attention.replace("{n}", &n.to_string()),
    }
}

/// Which activity entries a filter shows.
pub(crate) fn activity_matches(filter: crate::model::ActivityFilter, kind: ActivityKind) -> bool {
    use crate::model::ActivityFilter;
    match filter {
        ActivityFilter::All => true,
        ActivityFilter::Success => {
            matches!(kind, ActivityKind::Success | ActivityKind::RecoverySuccess)
        }
        ActivityFilter::Warning => matches!(
            kind,
            ActivityKind::Warning | ActivityKind::RecoveryPartial | ActivityKind::ManualRecovery
        ),
        ActivityFilter::Failure => kind == ActivityKind::Failure,
    }
}

/// The Apps page's own copy. What today's screens already said stays in
/// `i18n::Copy` and is used from there.
pub(crate) struct AppsCopy {
    pub(crate) group_components: &'static str,
    pub(crate) filter_all: &'static str,
    pub(crate) filter_installed: &'static str,
    pub(crate) filter_updates: &'static str,
    pub(crate) filter_turned_off: &'static str,
    pub(crate) filter_attention: &'static str,
    pub(crate) search_placeholder: &'static str,
    pub(crate) refresh_list: &'static str,
    pub(crate) refreshing: &'static str,
    pub(crate) no_match: &'static str,
    pub(crate) empty_view: &'static str,
    pub(crate) clear_search: &'static str,
    pub(crate) show_all: &'static str,
    pub(crate) state_installed: &'static str,
    pub(crate) state_incompatible: &'static str,
    pub(crate) action_install: &'static str,
    pub(crate) action_update: &'static str,
    pub(crate) action_turn_on: &'static str,
    pub(crate) action_turn_off: &'static str,
    pub(crate) action_check: &'static str,
    pub(crate) action_retry: &'static str,
    pub(crate) action_restore: &'static str,
    pub(crate) action_remove: &'static str,
    pub(crate) action_view: &'static str,
    pub(crate) update_all: &'static str,
    pub(crate) progress_preparing: &'static str,
    pub(crate) progress_downloading: &'static str,
    pub(crate) progress_downloading_unknown: &'static str,
    pub(crate) progress_downloaded: &'static str,
    pub(crate) result_installed: &'static str,
    pub(crate) result_parts_added: &'static str,
    pub(crate) action_add_parts: &'static str,
    pub(crate) result_updated: &'static str,
    pub(crate) result_turned_on: &'static str,
    pub(crate) result_turned_off: &'static str,
    pub(crate) result_checked: &'static str,
    pub(crate) result_restored: &'static str,
    pub(crate) result_restored_unknown: &'static str,
    pub(crate) result_removed: &'static str,
    pub(crate) run_updating_one: &'static str,
    pub(crate) run_updating: &'static str,
    pub(crate) run_installing_one: &'static str,
    pub(crate) run_installing: &'static str,
    pub(crate) run_working_one: &'static str,
    pub(crate) run_working: &'static str,
    pub(crate) run_password: &'static str,
    pub(crate) run_cancel: &'static str,
    pub(crate) run_cancelling: &'static str,
    pub(crate) available_updates: &'static str,
    pub(crate) no_updates: &'static str,
    pub(crate) download_size: &'static str,
    pub(crate) previous_kept: &'static str,
    pub(crate) self_update_title: &'static str,
    pub(crate) change_failed_title: &'static str,
    pub(crate) adds_parts: &'static str,
    pub(crate) self_update_detail: &'static str,
    pub(crate) failed_health: &'static str,
    pub(crate) kind: &'static str,
    pub(crate) status: &'static str,
    pub(crate) health_checks: &'static str,
    pub(crate) health_checks_none: &'static str,
    pub(crate) default_apps: &'static str,
    pub(crate) default_apps_description: &'static str,
    pub(crate) actions_header: &'static str,
    pub(crate) remove_title: &'static str,
    pub(crate) remove_body: &'static str,
    pub(crate) delete_data: &'static str,
    pub(crate) delete_data_detail: &'static str,
    pub(crate) restore_title: &'static str,
    pub(crate) restore_title_unknown: &'static str,
    pub(crate) restore_body: &'static str,
    pub(crate) clear_title: &'static str,
    pub(crate) clear_body: &'static str,
    pub(crate) clear: &'static str,
    pub(crate) data_trashed: &'static str,
    pub(crate) data_failed: &'static str,
    pub(crate) run_full_check: &'static str,
    pub(crate) checked_ago: &'static str,
    pub(crate) health_one_needs_attention: &'static str,
    pub(crate) health_needs_attention: &'static str,
    pub(crate) results_header: &'static str,
}

static EN: AppsCopy = AppsCopy {
    group_components: "Better OS components",
    filter_all: "All",
    filter_installed: "Installed",
    filter_updates: "Updates",
    filter_turned_off: "Turned off",
    filter_attention: "Needs attention",
    search_placeholder: "Search components",
    refresh_list: "Refresh list",
    refreshing: "Checking…",
    no_match: "No components match \"{query}\".",
    empty_view: "No components here right now.",
    clear_search: "Clear search",
    show_all: "Show all",
    state_installed: "Installed {version}",
    state_incompatible: "Not available for this PC",
    action_install: "Install",
    action_update: "Update",
    action_turn_on: "Turn on",
    action_turn_off: "Turn off",
    action_check: "Check again",
    action_retry: "Retry",
    action_restore: "Restore previous version",
    action_remove: "Remove",
    action_view: "View",
    update_all: "Update all",
    progress_preparing: "Getting ready",
    progress_downloading: "Downloading {received} of {total}",
    progress_downloading_unknown: "Downloading {received}",
    progress_downloaded: "Downloaded",
    result_installed: "Installed {version}",
    result_parts_added: "Missing parts added",
    action_add_parts: "Add missing parts",
    result_updated: "Updated to {version}",
    result_turned_on: "Turned on",
    result_turned_off: "Turned off",
    result_checked: "Checked: it works",
    result_restored: "Restored to {version}",
    result_restored_unknown: "Previous version restored",
    result_removed: "Removed",
    run_updating_one: "Updating 1 component",
    run_updating: "Updating {n} components",
    run_installing_one: "Installing 1 component",
    run_installing: "Installing {n} components",
    run_working_one: "Changing 1 component",
    run_working: "Changing {n} components",
    run_password: "You may be asked for an administrator's password once.",
    run_cancel: "Cancel",
    run_cancelling: "Cancelling…",
    available_updates: "Available updates",
    no_updates: "Every component on this PC has its newest version.",
    download_size: "{size} download",
    previous_kept: "Previous version kept",
    self_update_title: "This updates Better Manager itself",
    change_failed_title: "That did not finish",
    adds_parts: "Adds {parts}",
    self_update_detail: "When the update finishes, it reopens on the new version by itself.",
    failed_health: "It didn't pass its health check.",
    kind: "Type",
    status: "Status",
    health_checks: "Health checks",
    health_checks_none: "None declared",
    default_apps: "Default apps",
    default_apps_description: "Choose what this component opens by default",
    actions_header: "Manage",
    remove_title: "Remove {component}?",
    remove_body: "Its programs, background services, and everything else it added to this PC are removed, and settings it changed are put back. If one can't be put back, removal still finishes and Activity lists it.",
    delete_data: "Also delete my settings and history",
    delete_data_detail: "They're moved to the trash.",
    restore_title: "Restore {component} to {version}?",
    restore_title_unknown: "Restore {component} to the previous version?",
    restore_body: "{current} is replaced. You can update again afterwards.",
    clear_title: "Clear all activity?",
    clear_body: "This can't be undone.",
    clear: "Clear",
    data_trashed: "{component}'s settings and history were moved to the trash.",
    data_failed: "{component} was removed, but some of its settings couldn't be moved to the trash.",
    run_full_check: "Run the full check",
    checked_ago: "Last checked: {ago}",
    health_one_needs_attention: "1 check needs attention",
    health_needs_attention: "{n} checks need attention",
    results_header: "Results",
};

static ZH_TW: AppsCopy = AppsCopy {
    group_components: "Better OS 元件",
    filter_all: "全部",
    filter_installed: "已安裝",
    filter_updates: "可更新",
    filter_turned_off: "已停用",
    filter_attention: "需要處理",
    search_placeholder: "搜尋元件",
    refresh_list: "更新清單",
    refreshing: "正在檢查…",
    no_match: "找不到符合「{query}」的元件。",
    empty_view: "目前這裡沒有元件。",
    clear_search: "清除搜尋",
    show_all: "顯示全部",
    state_installed: "已安裝 {version}",
    state_incompatible: "這台電腦無法使用",
    action_install: "安裝",
    action_update: "更新",
    action_turn_on: "啟用",
    action_turn_off: "停用",
    action_check: "重新檢查",
    action_retry: "重試",
    action_restore: "還原上一個版本",
    action_remove: "移除",
    action_view: "查看",
    update_all: "全部更新",
    progress_preparing: "準備中",
    progress_downloading: "正在下載 {received} / {total}",
    progress_downloading_unknown: "正在下載 {received}",
    progress_downloaded: "已下載",
    result_installed: "已安裝 {version}",
    result_parts_added: "已補上缺少的部分",
    action_add_parts: "補上缺少的部分",
    result_updated: "已更新到 {version}",
    result_turned_on: "已啟用",
    result_turned_off: "已停用",
    result_checked: "已檢查，可以正常使用",
    result_restored: "已還原成 {version}",
    result_restored_unknown: "已還原上一個版本",
    result_removed: "已移除",
    run_updating_one: "正在更新 1 個元件",
    run_updating: "正在更新 {n} 個元件",
    run_installing_one: "正在安裝 1 個元件",
    run_installing: "正在安裝 {n} 個元件",
    run_working_one: "正在變更 1 個元件",
    run_working: "正在變更 {n} 個元件",
    run_password: "過程中可能會請你輸入一次管理員密碼。",
    run_cancel: "取消",
    run_cancelling: "正在取消…",
    available_updates: "可用的更新",
    no_updates: "這台電腦上的元件都已是最新版本。",
    download_size: "下載 {size}",
    previous_kept: "會保留上一個版本",
    self_update_title: "這次會更新 Better Manager 本身",
    change_failed_title: "沒有完成",
    adds_parts: "補上 {parts}",
    self_update_detail: "更新完成後，它會自己以新版本重新開啟。",
    failed_health: "沒有通過健康檢查。",
    kind: "類型",
    status: "狀態",
    health_checks: "健康檢查",
    health_checks_none: "沒有宣告",
    default_apps: "預設應用程式",
    default_apps_description: "選擇這個元件預設要開啟的項目",
    actions_header: "管理",
    remove_title: "要移除 {component} 嗎？",
    remove_body: "它的程式、背景服務，以及它加到這台電腦的其他東西都會一併移除，改過的設定也會改回來。如果有設定改不回來，移除仍會完成，並記在「活動紀錄」。",
    delete_data: "同時刪除我的設定和紀錄",
    delete_data_detail: "它們會移到垃圾桶。",
    restore_title: "要把 {component} 還原成 {version} 嗎？",
    restore_title_unknown: "要把 {component} 還原成上一個版本嗎？",
    restore_body: "目前的 {current} 會被取代。之後可以再更新。",
    clear_title: "清除所有活動紀錄？",
    clear_body: "清除後無法復原。",
    clear: "清除",
    data_trashed: "{component} 的設定和紀錄已移到垃圾桶。",
    data_failed: "{component} 已移除，但有些設定沒辦法移到垃圾桶。",
    run_full_check: "執行完整檢查",
    checked_ago: "上次檢查：{ago}",
    health_one_needs_attention: "有 1 項檢查需要處理",
    health_needs_attention: "有 {n} 項檢查需要處理",
    results_header: "檢查結果",
};

/// The shipped copy for the first-party components today's translation table
/// did not cover, so every component's line reads in the current language
/// (audit M3): its product name, one line, and what it does.
pub(crate) fn first_party(
    locale: Locale,
    id: &str,
) -> Option<(&'static str, &'static str, &'static str)> {
    let zh = locale.resolved() == Locale::ZhTw;
    let pick = |en: (&'static str, &'static str, &'static str),
                zh_tw: (&'static str, &'static str, &'static str)| {
        Some(if zh { zh_tw } else { en })
    };
    match id {
        "better-launcher" => pick(
            (
                "Better Launcher",
                "Search and every app in one overlay",
                "Opens one overlay with the search row on top and the whole application library below it.",
            ),
            (
                "Better Launcher",
                "搜尋和所有應用程式都在同一個畫面",
                "開啟一個畫面，上方是搜尋列，下方是所有應用程式。",
            ),
        ),
        "better-git" => pick(
            (
                "Better Git",
                "Git and GitHub without a terminal",
                "Open a repository, see each change and its diff, commit, and sync with GitHub, the way GitHub Desktop does.",
            ),
            (
                "Better Git",
                "不用終端機也能使用 Git 和 GitHub",
                "開啟儲存庫，查看每個變更和差異、提交，並與 GitHub 同步，用起來像 GitHub Desktop。",
            ),
        ),
        "better-touchpad" => pick(
            (
                "Better Touchpad",
                "Pointer, scrolling, tapping, and gestures",
                "Pointer speed, scrolling, tapping, and gestures in one place, with every unavailable setting saying why.",
            ),
            (
                "Better Touchpad",
                "指標、捲動、輕觸和手勢",
                "在同一個地方調整指標速度、捲動、輕觸和手勢，無法使用的設定也會說明原因。",
            ),
        ),
        "better-awake" => pick(
            (
                "Better Awake",
                "Keep this PC awake, on purpose or by rule",
                "Keep-awake sessions and automatic rules, with every reason the machine is awake shown.",
            ),
            (
                "Better Awake",
                "依需要或規則讓電腦保持清醒",
                "手動或依規則讓電腦保持清醒，並列出每個讓電腦保持清醒的原因。",
            ),
        ),
        "better-storage" => pick(
            (
                "Better Storage",
                "Safe removal for external drives",
                "Direct removal for external drives, with an honest ready-to-unplug state.",
            ),
            (
                "Better Storage",
                "安全移除外接磁碟",
                "直接移除外接磁碟，並如實告訴你什麼時候可以拔除。",
            ),
        ),
        _ => None,
    }
}

pub(crate) fn copy(locale: Locale) -> &'static AppsCopy {
    match locale.resolved() {
        Locale::ZhTw => &ZH_TW,
        _ => &EN,
    }
}

#[cfg(test)]
mod tests;
