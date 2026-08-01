use better_core::{ComponentCatalog, ComponentId, ComponentManifest};
use gpui::*;
use gpui_component::input::{InputEvent, InputState};
use manager_core::{
    ActivityKind, ActivityRecord, ComponentFilterPreference, ComponentStatus, DesiredOperation,
    DiskSpaceCheck, DoctorCheck, HealthState, Manager, ManagerSettings, ManagerState, MockOutcome,
    MockSystemProfile, OperationProgress, OperationStage, PlanStep, ReleaseChannel,
    RestartRequirement, StoredLocale, TransactionPlan,
};
use manager_store::{JsonStore, StateStore};

use crate::{
    i18n::{Locale, copy},
    model::{ActivityFilter, ComponentInfo, DetailTab, Page, ui_id_for_component},
};

pub(crate) struct ManagerApp {
    pub(crate) page: Page,
    pub(crate) locale: Locale,
    pub(crate) search: Entity<InputState>,
    pub(crate) search_query: String,
    pub(crate) detail_tab: DetailTab,
    pub(crate) activity_filter: ActivityFilter,
    pub(crate) manager: Manager,
    pub(crate) state: ManagerState,
    pub(crate) store: JsonStore,
    pub(crate) pending_plan: Option<TransactionPlan>,
    pub(crate) planning_error: Option<AppError>,
    pub(crate) _subscriptions: Vec<Subscription>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AppError {
    Planning,
    Storage,
}

impl ManagerApp {
    pub(crate) fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let store = JsonStore::from_default_path();
        let manager = catalog_manager();
        let (state, planning_error) = match store.load() {
            Ok(outcome) if manager.validate_state(&outcome.state).is_ok() => (
                outcome.state,
                outcome.recovered_corrupt_state.map(|_| AppError::Storage),
            ),
            Ok(_) | Err(_) => (ManagerState::default(), Some(AppError::Storage)),
        };
        let locale = locale_from_stored(state.settings.locale);
        let search = cx.new(|cx| InputState::new(window, cx).placeholder(copy(locale).search));
        let subscription = Self::subscribe_to_search(&search, window, cx);
        let page = if state.active_operation.is_some() {
            Page::Installing
        } else if state.settings.onboarding_complete {
            Page::Overview
        } else {
            Page::FirstRun
        };
        let pending_plan = state
            .active_operation
            .as_ref()
            .map(|active| active.plan.clone());

        Self {
            page,
            locale,
            search,
            search_query: String::new(),
            detail_tab: DetailTab::Overview,
            activity_filter: ActivityFilter::All,
            manager,
            state,
            store,
            pending_plan,
            planning_error,
            _subscriptions: vec![subscription],
        }
    }

    fn subscribe_to_search(
        search: &Entity<InputState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Subscription {
        let search_for_callback = search.clone();
        cx.subscribe_in(search, window, move |this, _, event: &InputEvent, _, cx| {
            if matches!(event, InputEvent::Change) {
                this.search_query = search_for_callback.read(cx).value().to_string();
                cx.notify();
            }
        })
    }

    fn commit_state(&mut self, candidate: ManagerState) -> bool {
        match self.store.save(&candidate) {
            Ok(()) => {
                self.state = candidate;
                if self.planning_error == Some(AppError::Storage) {
                    self.planning_error = None;
                }
                true
            }
            Err(_) => {
                self.planning_error = Some(AppError::Storage);
                false
            }
        }
    }

    pub(crate) fn set_locale(
        &mut self,
        locale: Locale,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut candidate = self.state.clone();
        let mut settings = candidate.settings.clone();
        settings.locale = stored_from_locale(locale);
        candidate.update_settings(settings);
        if !self.commit_state(candidate) {
            cx.notify();
            return;
        }

        self.locale = locale;
        let current_value = self.search.read(cx).value().to_string();
        let search = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(copy(locale).search)
                .default_value(current_value)
        });
        let subscription = Self::subscribe_to_search(&search, window, cx);
        self.search = search;
        self._subscriptions.clear();
        self._subscriptions.push(subscription);
        cx.notify();
    }

    pub(crate) fn clear_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder(copy(self.locale).search));
        let subscription = Self::subscribe_to_search(&search, window, cx);
        self.search = search;
        self.search_query.clear();
        self._subscriptions.clear();
        self._subscriptions.push(subscription);
        cx.notify();
    }

    pub(crate) fn navigate(&mut self, page: Page, cx: &mut Context<Self>) {
        self.page = page;
        cx.notify();
    }

    pub(crate) fn complete_onboarding(&mut self, cx: &mut Context<Self>) {
        let mut candidate = self.state.clone();
        let mut settings = candidate.settings.clone();
        settings.onboarding_complete = true;
        candidate.update_settings(settings);
        if self.commit_state(candidate) {
            self.navigate(Page::Overview, cx);
        } else {
            cx.notify();
        }
    }

    pub(crate) fn prepare_update_all(&mut self, cx: &mut Context<Self>) {
        match self.manager.plan_all(&self.state) {
            Ok(plan) if !plan.is_empty() => {
                self.pending_plan = Some(plan);
                self.planning_error = None;
                self.navigate(Page::ReviewChanges, cx);
            }
            Ok(_) => {
                self.pending_plan = None;
                self.navigate(Page::Updates, cx);
            }
            Err(_) => self.show_planning_error(Page::Updates, cx),
        }
    }

    pub(crate) fn prepare_component_change(&mut self, id: &'static str, cx: &mut Context<Self>) {
        let Some(component) = self.component_by_ui_id(id) else {
            self.show_planning_error(Page::Components, cx);
            return;
        };
        let operation = match component.state {
            ComponentStatus::Available => Some(DesiredOperation::Install),
            ComponentStatus::UpdateAvailable => Some(DesiredOperation::Update),
            ComponentStatus::Disabled => Some(DesiredOperation::Enable),
            ComponentStatus::RestoreAvailable => Some(DesiredOperation::Restore),
            ComponentStatus::Degraded | ComponentStatus::Failed => Some(DesiredOperation::Verify),
            _ => None,
        };
        if let Some(operation) = operation {
            self.prepare_component_operation(id, operation, cx);
        } else {
            self.open_component(id, cx);
        }
    }

    pub(crate) fn prepare_component_operation(
        &mut self,
        id: &'static str,
        operation: DesiredOperation,
        cx: &mut Context<Self>,
    ) {
        let Some(core_id) = core_component_id(id) else {
            self.show_planning_error(Page::Components, cx);
            return;
        };
        match self.manager.plan(&self.state, &core_id, operation) {
            Ok(plan) => {
                self.pending_plan = Some(plan);
                self.planning_error = None;
                self.navigate(Page::ReviewChanges, cx);
            }
            Err(_) => self.show_planning_error(Page::Components, cx),
        }
    }

    pub(crate) fn begin_install(&mut self, cx: &mut Context<Self>) {
        let Some(plan) = self.pending_plan.clone() else {
            self.show_planning_error(Page::Updates, cx);
            return;
        };
        let mut candidate = self.state.clone();
        match self.manager.begin(&mut candidate, plan) {
            Ok(_) => {
                if self.commit_state(candidate) {
                    self.navigate(Page::Installing, cx);
                } else {
                    cx.notify();
                }
            }
            Err(_) => self.show_planning_error(Page::ReviewChanges, cx),
        }
    }

    pub(crate) fn advance_install(&mut self, cx: &mut Context<Self>) {
        let mut candidate = self.state.clone();
        match self.manager.advance(&mut candidate, MockOutcome::Succeed) {
            Ok(progress) => {
                if !self.commit_state(candidate) {
                    cx.notify();
                    return;
                }
                match progress {
                    OperationProgress::InProgress { .. } => self.navigate(Page::Installing, cx),
                    OperationProgress::Finished { operation } => self.navigate(
                        if operation == DesiredOperation::Restore {
                            Page::Restored
                        } else {
                            Page::Finished
                        },
                        cx,
                    ),
                    OperationProgress::Failed { .. } => self.navigate(Page::Restore, cx),
                }
            }
            Err(_) => self.show_planning_error(Page::Installing, cx),
        }
    }

    pub(crate) fn cancel_install(&mut self, cx: &mut Context<Self>) {
        let mut candidate = self.state.clone();
        match self.manager.cancel(&mut candidate) {
            Ok(()) => {
                if self.commit_state(candidate) {
                    self.pending_plan = None;
                    self.navigate(Page::Updates, cx);
                } else {
                    cx.notify();
                }
            }
            Err(_) => self.show_planning_error(Page::Installing, cx),
        }
    }

    pub(crate) fn prepare_recovery(
        &mut self,
        component: ComponentId,
        operation: DesiredOperation,
        cx: &mut Context<Self>,
    ) {
        match self.manager.plan(&self.state, &component, operation) {
            Ok(plan) => {
                self.pending_plan = Some(plan);
                self.planning_error = None;
                self.navigate(Page::ReviewChanges, cx);
            }
            Err(_) => self.show_planning_error(Page::Restore, cx),
        }
    }

    pub(crate) fn set_component_filter(
        &mut self,
        filter: ComponentFilterPreference,
        cx: &mut Context<Self>,
    ) {
        let mut candidate = self.state.clone();
        let mut settings = candidate.settings.clone();
        settings.component_filter = filter;
        candidate.update_settings(settings);
        self.commit_state(candidate);
        cx.notify();
    }

    pub(crate) fn set_release_channel(&mut self, channel: ReleaseChannel, cx: &mut Context<Self>) {
        let mut candidate = self.state.clone();
        let mut settings = candidate.settings.clone();
        settings.release_channel = channel;
        candidate.update_settings(settings);
        self.commit_state(candidate);
        cx.notify();
    }

    pub(crate) fn toggle_check_updates(&mut self, cx: &mut Context<Self>) {
        self.update_settings(|settings| settings.check_updates = !settings.check_updates);
        cx.notify();
    }

    pub(crate) fn toggle_auto_download(&mut self, cx: &mut Context<Self>) {
        self.update_settings(|settings| settings.auto_download = !settings.auto_download);
        cx.notify();
    }

    pub(crate) fn toggle_diagnostic_logs(&mut self, cx: &mut Context<Self>) {
        self.update_settings(|settings| settings.diagnostic_logs = !settings.diagnostic_logs);
        cx.notify();
    }

    fn update_settings(&mut self, update: impl FnOnce(&mut ManagerSettings)) {
        let mut candidate = self.state.clone();
        let mut settings = candidate.settings.clone();
        update(&mut settings);
        candidate.update_settings(settings);
        self.commit_state(candidate);
    }

    pub(crate) fn clear_activity(&mut self, cx: &mut Context<Self>) {
        let mut candidate = self.state.clone();
        candidate.clear_activity();
        self.commit_state(candidate);
        cx.notify();
    }

    pub(crate) fn components(&self) -> Vec<ComponentInfo> {
        self.manager
            .manifests()
            .filter_map(|manifest| {
                let ui_id = ui_id_for_component(&manifest.id)?;
                let record = self.state.component(&manifest.id);
                let state = self.manager.status(&self.state, &manifest.id).ok()?;
                Some(ComponentInfo {
                    ui_id,
                    core_id: manifest.id.clone(),
                    installed_version: record.and_then(|record| record.installed_version.clone()),
                    enabled: record.is_some_and(|record| record.enabled),
                    available_version: manifest.version.to_string(),
                    state,
                    health: record.map(|record| record.health).unwrap_or_default(),
                    restart_requirement: RestartRequirement::NotDeclared,
                    restore_available: record
                        .and_then(|record| record.restore_snapshot.as_ref())
                        .is_some(),
                    kind: manifest.component_type.clone().into(),
                    paths: manifest.paths.clone(),
                    release_notes: manifest.release_notes.clone(),
                })
            })
            .collect()
    }

    pub(crate) fn component_by_ui_id(&self, id: &str) -> Option<ComponentInfo> {
        self.components()
            .into_iter()
            .find(|component| component.ui_id == id)
    }

    pub(crate) fn plan_component_name(&self, id: &ComponentId) -> String {
        ui_id_for_component(id)
            .map(|ui_id| self.component_name(ui_id).to_string())
            .unwrap_or_else(|| id.to_string())
    }

    pub(crate) fn installed_count(&self) -> usize {
        self.components()
            .into_iter()
            .filter(|component| component.installed_version.is_some())
            .count()
    }

    pub(crate) fn update_plan_count(&self) -> usize {
        self.components()
            .into_iter()
            .filter(|component| component.state == ComponentStatus::UpdateAvailable)
            .count()
    }

    pub(crate) fn healthy_count(&self) -> usize {
        self.components()
            .into_iter()
            .filter(|component| {
                component.installed_version.is_some() && component.health == HealthState::Healthy
            })
            .count()
    }

    pub(crate) fn is_pending(&self, id: &ComponentId) -> bool {
        self.pending_plan
            .as_ref()
            .is_some_and(|plan| plan.steps().iter().any(|step| &step.component == id))
    }

    pub(crate) fn pending_steps(&self) -> Vec<PlanStep> {
        self.pending_plan
            .as_ref()
            .map(|plan| plan.steps().to_vec())
            .unwrap_or_default()
    }

    pub(crate) fn pending_disk_space(&self) -> DiskSpaceCheck {
        self.pending_plan
            .as_ref()
            .map(|plan| plan.disk_space())
            .unwrap_or(DiskSpaceCheck::NotRequired)
    }

    pub(crate) fn active_steps(&self) -> Vec<PlanStep> {
        self.state
            .active_operation
            .as_ref()
            .map(|active| active.plan.steps().to_vec())
            .unwrap_or_else(|| self.pending_steps())
    }

    pub(crate) fn open_component(&mut self, id: &'static str, cx: &mut Context<Self>) {
        self.page = Page::ComponentDetail(id);
        self.detail_tab = DetailTab::Overview;
        cx.notify();
    }

    pub(crate) fn selected_component(&self) -> Option<ComponentInfo> {
        match self.page {
            Page::ComponentDetail(id) => self.component_by_ui_id(id),
            _ => None,
        }
    }

    pub(crate) fn purpose(&self, id: &str) -> &'static str {
        let c = copy(self.locale);
        match id {
            "manager" => c.manager_purpose,
            "monitor" => c.monitor_purpose,
            "files" => c.files_purpose,
            _ => c.none,
        }
    }

    pub(crate) fn detail(&self, id: &str) -> &'static str {
        let c = copy(self.locale);
        match id {
            "manager" => c.manager_detail,
            "monitor" => c.monitor_detail,
            "files" => c.files_detail,
            _ => c.none,
        }
    }

    pub(crate) fn component_name(&self, id: &str) -> &'static str {
        let c = copy(self.locale);
        match id {
            "manager" => c.manager_name,
            "monitor" => c.monitor_name,
            "files" => c.files_name,
            _ => c.manager_name,
        }
    }

    pub(crate) fn component_name_for_core(&self, id: Option<&ComponentId>) -> String {
        id.map(|id| self.plan_component_name(id))
            .unwrap_or_else(|| copy(self.locale).manager.to_string())
    }

    pub(crate) fn page_is_active(&self, page: Page) -> bool {
        match page {
            Page::Components => matches!(self.page, Page::Components | Page::ComponentDetail(_)),
            Page::Updates => matches!(
                self.page,
                Page::Updates
                    | Page::ReviewChanges
                    | Page::Installing
                    | Page::Finished
                    | Page::Restore
                    | Page::Restored
            ),
            Page::Health => matches!(self.page, Page::Health | Page::DoctorResults),
            other => self.page == other,
        }
    }

    pub(crate) fn stage(&self) -> Option<OperationStage> {
        self.state
            .active_operation
            .as_ref()
            .map(|active| active.stage)
    }

    pub(crate) fn current_failure(&self) -> Option<(ComponentId, manager_core::FailureRecord)> {
        self.state
            .components
            .iter()
            .find_map(|(id, record)| record.failure.clone().map(|failure| (id.clone(), failure)))
    }

    pub(crate) fn doctor_checks(&self) -> Vec<DoctorCheck> {
        self.manager.doctor(&self.state).unwrap_or_default()
    }

    pub(crate) fn activity_matches(&self, entry: &ActivityRecord) -> bool {
        match self.activity_filter {
            ActivityFilter::All => true,
            ActivityFilter::Success => matches!(
                entry.kind,
                ActivityKind::Success | ActivityKind::RecoverySuccess
            ),
            ActivityFilter::Warning => matches!(
                entry.kind,
                ActivityKind::Warning
                    | ActivityKind::RecoveryPartial
                    | ActivityKind::ManualRecovery
            ),
            ActivityFilter::Failure => entry.kind == ActivityKind::Failure,
        }
    }

    pub(crate) fn stage_label(&self, stage: OperationStage) -> &'static str {
        let c = copy(self.locale);
        match stage {
            OperationStage::Downloading => c.downloading,
            OperationStage::Installing => c.installing_files,
            OperationStage::ApplyingSettings => c.applying_settings,
            OperationStage::CheckingHealth => c.checking_works,
        }
    }

    pub(crate) fn evidence_label(&self, evidence: Option<&str>) -> &'static str {
        let c = copy(self.locale);
        match evidence {
            Some("mock_failure_at_downloading") => c.downloading,
            Some("mock_failure_at_installing") => c.installing_files,
            Some("mock_failure_at_applying_settings") => c.applying_settings,
            Some("mock_failure_at_checking_health") => c.checking_works,
            Some("mock_restore_recheck_failed") => c.restore_recheck_failed,
            Some("mock_restore_rechecked") | Some("mock_health_check_passed") => c.passed,
            Some("mock_operation_finished") => c.finished,
            Some("mock_operation_cancelled") => c.cancel,
            _ => c.none,
        }
    }

    pub(crate) fn operation_label(&self, operation: DesiredOperation) -> &'static str {
        let c = copy(self.locale);
        match operation {
            DesiredOperation::Install => c.install,
            DesiredOperation::Update => c.update,
            DesiredOperation::Enable => c.enable,
            DesiredOperation::Disable => c.disable,
            DesiredOperation::Verify => c.health_check_label,
            DesiredOperation::Restore => c.restore_previous,
            DesiredOperation::Remove => c.remove,
        }
    }

    pub(crate) fn error_message(&self) -> &'static str {
        let c = copy(self.locale);
        match self.planning_error {
            Some(AppError::Planning) => c.planning_failed_detail,
            Some(AppError::Storage) => c.storage_error,
            None => c.none,
        }
    }

    fn show_planning_error(&mut self, page: Page, cx: &mut Context<Self>) {
        self.planning_error = Some(AppError::Planning);
        self.navigate(page, cx);
    }
}

fn core_component_id(id: &str) -> Option<ComponentId> {
    let core_id = match id {
        "manager" => "better-manager",
        "monitor" => "better-monitor",
        "files" => "better-files-example",
        _ => return None,
    };
    ComponentId::new(core_id).ok()
}

fn catalog_manager() -> Manager {
    let manifests = [
        include_str!("../../../components/manifests/better-manager.yaml"),
        include_str!("../../../components/manifests/better-monitor.yaml"),
        include_str!("../../../components/manifests/better-files-example.yaml"),
    ]
    .into_iter()
    .map(|input| ComponentManifest::parse_yaml(input).expect("example manifest must be valid"))
    .collect::<Vec<_>>();
    Manager::new(
        ComponentCatalog::from_manifests(manifests).expect("example catalog must be valid"),
        MockSystemProfile::default(),
    )
}

#[cfg(test)]
pub(crate) fn demo_manager() -> (Manager, ManagerState) {
    let manager = catalog_manager();
    let mut state = ManagerState::default();
    state.set_installed(
        ComponentId::new("better-manager").expect("example id must be valid"),
        "0.1.0",
        true,
    );
    state.set_installed(
        ComponentId::new("better-monitor").expect("example id must be valid"),
        "0.0.1",
        true,
    );
    (manager, state)
}

fn locale_from_stored(locale: StoredLocale) -> Locale {
    match locale {
        StoredLocale::System => Locale::System,
        StoredLocale::EnUs => Locale::EnUs,
        StoredLocale::ZhTw => Locale::ZhTw,
    }
}

fn stored_from_locale(locale: Locale) -> StoredLocale {
    match locale {
        Locale::System => StoredLocale::System,
        Locale::EnUs => StoredLocale::EnUs,
        Locale::ZhTw => StoredLocale::ZhTw,
    }
}
