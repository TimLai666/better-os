use better_core::{ComponentCatalog, ComponentId, ComponentManifest};
use gpui::*;
use gpui_component::input::{InputEvent, InputState};
use gpui_component::{Theme, ThemeMode};
use manager_core::ExecutionMode;
use manager_core::exec::{CancelToken, RealDriver, RunnerEvent, StageProgress, TransactionRunner};
use manager_core::{
    ActivityKind, ActivityRecord, ComponentFilterPreference, ComponentStatus, DesiredOperation,
    DiskSpaceCheck, DoctorCheck, HealthState, Manager, ManagerSettings, ManagerState, MockOutcome,
    OperationProgress, OperationStage, PlanStep, ReleaseChannel, RestartRequirement, StoredLocale,
    StoredTheme, TransactionPlan,
};
use manager_platform::MockPlatform;
use manager_platform::download::{ArtifactCache, HttpDownloader};
use manager_platform::privileged::DbusPrivilegedExecutor;
use manager_store::{JsonStore, StateStore};

use crate::{
    defaults_app::DefaultsState,
    i18n::{Locale, copy},
    model::{ActivityFilter, ComponentInfo, DetailTab, Page},
};

/// How this window runs transactions.
///
/// Real is the default: a manager that quietly simulated would tell a user
/// their machine changed when it did not. The demo mode stays available for
/// screenshots and for trying the flow without a privileged service, and says
/// so on screen.
fn default_execution_mode() -> ExecutionMode {
    match std::env::var("BETTER_MANAGER_EXECUTION").as_deref() {
        Ok("mock") | Ok("demo") => ExecutionMode::Mock,
        _ => ExecutionMode::Real,
    }
}

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
    /// Whether this window simulates transactions or actually performs them.
    pub(crate) execution: ExecutionMode,
    /// The Defaults section's own state, read and changed through
    /// `defaults-core` and nothing else.
    pub(crate) defaults: DefaultsState,
    /// Live progress from a running real transaction.
    pub(crate) transfer: Option<Transfer>,
    /// Set while a real transaction is running. Dropping it stops the work.
    pub(crate) running: Option<Task<()>>,
    pub(crate) cancel: Option<CancelToken>,
    pub(crate) _subscriptions: Vec<Subscription>,
}

/// What a real transaction is currently moving.
#[derive(Clone, Debug)]
pub(crate) struct Transfer {
    pub(crate) component: String,
    pub(crate) received_bytes: u64,
    pub(crate) total_bytes: Option<u64>,
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
        apply_theme(state.settings.theme, window, cx);
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
            execution: default_execution_mode(),
            defaults: DefaultsState::default(),
            transfer: None,
            running: None,
            cancel: None,
            _subscriptions: vec![subscription],
        }
    }

    /// Whether this window can actually change the machine.
    pub(crate) fn is_demo(&self) -> bool {
        self.execution == ExecutionMode::Mock
    }

    /// Whether the user may still abandon the running transaction.
    ///
    /// A simulation can always be abandoned. A real one only until it has been
    /// handed to the privileged service: after that the host may already have
    /// changed, and offering a cancel would promise a restoration nothing
    /// performed.
    pub(crate) fn can_cancel_now(&self) -> bool {
        if self.is_demo() {
            return true;
        }
        self.state
            .active_operation
            .as_ref()
            .map(|active| active.stage == OperationStage::Downloading)
            .unwrap_or(true)
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

    pub(crate) fn set_theme(
        &mut self,
        theme: StoredTheme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut candidate = self.state.clone();
        let mut settings = candidate.settings.clone();
        settings.theme = theme;
        candidate.update_settings(settings);
        if !self.commit_state(candidate) {
            cx.notify();
            return;
        }
        apply_theme(theme, window, cx);
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
        match self.manager.plan_all_in_mode(&self.state, self.execution) {
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

    pub(crate) fn prepare_component_change(&mut self, id: &ComponentId, cx: &mut Context<Self>) {
        let Some(component) = self.component_by_id(id) else {
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
        id: &ComponentId,
        operation: DesiredOperation,
        cx: &mut Context<Self>,
    ) {
        match self
            .manager
            .plan_in_mode(&self.state, id, operation, self.execution)
        {
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
                    if !self.is_demo() {
                        self.run_real_transaction(cx);
                    }
                } else {
                    cx.notify();
                }
            }
            Err(_) => self.show_planning_error(Page::ReviewChanges, cx),
        }
    }

    /// Runs the whole transaction off the UI thread.
    ///
    /// The runner owns the state and every save for the duration; the window
    /// adopts what it reports rather than keeping a second copy in step. All
    /// network and IPC happens on the background thread, so the interface keeps
    /// drawing while packages are being fetched and applied.
    fn run_real_transaction(&mut self, cx: &mut Context<Self>) {
        let manager = self.manager.clone();
        let store = self.store.clone();
        let mut state = self.state.clone();
        let profile = self.manager.profile().clone();
        let cancel = CancelToken::new();
        self.cancel = Some(cancel.clone());
        self.transfer = None;

        let (sender, receiver) = smol::channel::unbounded::<RunnerEvent>();
        let worker = cx.background_spawn(async move {
            let downloader = HttpDownloader::new(ArtifactCache::from_default_path());
            let executor = match DbusPrivilegedExecutor::connect() {
                Ok(executor) => executor,
                Err(_) => {
                    // Nothing was applied, so abandoning is honest here.
                    let _ = manager.cancel(&mut state);
                    let _ = store.save(&state);
                    let _ = sender.send(RunnerEvent::StateSaved(Box::new(state))).await;
                    return;
                }
            };
            let driver = RealDriver::new(
                &downloader,
                &executor,
                uuid::Uuid::new_v4().to_string(),
                profile,
            );
            let mut runner = TransactionRunner::new(&manager, Box::new(driver), &store)
                .with_cancel_token(cancel);
            let _ = runner.run(&mut state, &mut |event| {
                let _ = sender.send_blocking(event);
            });
        });

        let pump = cx.spawn(async move |this, cx| {
            while let Ok(event) = receiver.recv().await {
                if this
                    .update(cx, |app, cx| app.apply_runner_event(event, cx))
                    .is_err()
                {
                    break;
                }
            }
        });
        // Keeping the worker alive is the pump's job; the window only has to
        // hold one handle.
        self.running = Some(cx.background_spawn(async move {
            worker.await;
            pump.await;
        }));
    }

    /// Adopts what the background transaction reported.
    fn apply_runner_event(&mut self, event: RunnerEvent, cx: &mut Context<Self>) {
        match event {
            RunnerEvent::Progress(StageProgress::Downloading {
                component,
                received_bytes,
                total_bytes,
            }) => {
                self.transfer = Some(Transfer {
                    component,
                    received_bytes,
                    total_bytes,
                });
                cx.notify();
            }
            RunnerEvent::Progress(StageProgress::Applying { .. })
            | RunnerEvent::StageEntered(_) => {
                self.transfer = None;
                cx.notify();
            }
            RunnerEvent::StateSaved(state) => {
                self.state = *state;
                self.pending_plan = self
                    .state
                    .active_operation
                    .as_ref()
                    .map(|active| active.plan.clone());
                cx.notify();
            }
            RunnerEvent::Finished(progress) => {
                self.transfer = None;
                self.cancel = None;
                match progress {
                    OperationProgress::Finished { operation } => self.navigate(
                        if operation == DesiredOperation::Restore {
                            Page::Restored
                        } else {
                            Page::Finished
                        },
                        cx,
                    ),
                    OperationProgress::Failed { .. } => self.navigate(Page::Restore, cx),
                    OperationProgress::InProgress { .. } => cx.notify(),
                }
            }
        }
    }

    pub(crate) fn advance_install(&mut self, cx: &mut Context<Self>) {
        let mut candidate = self.state.clone();
        match self
            .manager
            .advance_mock(&mut candidate, MockOutcome::Succeed)
        {
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
        // A real transaction is cancelled by asking the running work to stop,
        // which it only honors while stopping still leaves the host as it was.
        if let Some(cancel) = &self.cancel {
            cancel.cancel();
            cx.notify();
            return;
        }
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
        match self
            .manager
            .plan_in_mode(&self.state, &component, operation, self.execution)
        {
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
                let record = self.state.component(&manifest.id);
                let state = self.manager.status(&self.state, &manifest.id).ok()?;
                Some(ComponentInfo::present(
                    manifest,
                    record,
                    state,
                    translated_component(self.locale, &manifest.id),
                ))
            })
            .collect()
    }

    pub(crate) fn component_by_id(&self, id: &ComponentId) -> Option<ComponentInfo> {
        self.components()
            .into_iter()
            .find(|component| &component.core_id == id)
    }

    pub(crate) fn plan_component_name(&self, id: &ComponentId) -> String {
        translated_component(self.locale, id)
            .map(|translation| translation.name.to_string())
            .or_else(|| {
                self.manager
                    .manifests()
                    .find(|manifest| &manifest.id == id)
                    .map(|manifest| manifest.display_name.clone())
            })
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

    /// The interruption the pending transaction as a whole would require.
    pub(crate) fn pending_restart_requirement(&self) -> RestartRequirement {
        self.pending_plan
            .as_ref()
            .map(|plan| plan.restart_requirement())
            .unwrap_or(RestartRequirement::NotDeclared)
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

    pub(crate) fn open_component(&mut self, id: &ComponentId, cx: &mut Context<Self>) {
        self.page = Page::ComponentDetail(id.clone());
        self.detail_tab = DetailTab::Overview;
        cx.notify();
    }

    pub(crate) fn selected_component(&self) -> Option<ComponentInfo> {
        match &self.page {
            Page::ComponentDetail(id) => self.component_by_id(id),
            _ => None,
        }
    }

    /// Presentation text for a value a component did not declare.
    pub(crate) fn declared_or_not(&self, value: &str) -> String {
        if value.trim().is_empty() {
            copy(self.locale).not_declared.to_string()
        } else {
            value.to_string()
        }
    }

    pub(crate) fn component_name_for_core(&self, id: Option<&ComponentId>) -> String {
        id.map(|id| self.plan_component_name(id))
            .unwrap_or_else(|| copy(self.locale).manager.to_string())
    }

    pub(crate) fn page_is_active(&self, page: &Page) -> bool {
        match page {
            Page::Components => matches!(self.page, Page::Components | Page::ComponentDetail(_)),
            Page::Defaults => matches!(
                self.page,
                Page::Defaults
                    | Page::DefaultsComponent(_)
                    | Page::DefaultsReview
                    | Page::DefaultsResults
            ),
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
            other => &self.page == other,
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
            Some("mock_operation_cancelled") | Some("operation.cancelled") => c.cancel,
            // Real execution reports what actually went wrong.
            Some("download.network") => c.evidence_download_network,
            Some("download.checksum_mismatch") => c.evidence_checksum_mismatch,
            Some("daemon.unavailable") | Some("daemon.not_approved") => {
                c.evidence_daemon_unavailable
            }
            Some("daemon.polkit_denied") => c.evidence_polkit_denied,
            Some("restore.artifact_missing") => c.evidence_restore_artifact_missing,
            Some(other) if other.starts_with("daemon.error.apt_busy") => c.evidence_apt_busy,
            Some(other) if other.starts_with("daemon.error.apt_failed") => c.evidence_apt_failed,
            Some(other) if other.starts_with("daemon.error.health_failed") => {
                c.evidence_health_failed
            }
            Some(other) if other.starts_with("daemon.error.state_drift") => c.evidence_state_drift,
            Some(other) if other.starts_with("daemon.") => c.evidence_daemon_refused,
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

/// Applies a stored theme choice to the running window. `System` follows the
/// desktop appearance; the other two are explicit and override it.
pub(crate) fn apply_theme(theme: StoredTheme, window: &mut Window, cx: &mut App) {
    match theme {
        StoredTheme::Dark => Theme::change(ThemeMode::Dark, Some(window), cx),
        StoredTheme::Light => Theme::change(ThemeMode::Light, Some(window), cx),
        StoredTheme::System => Theme::sync_system_appearance(Some(window), cx),
    }
}

/// The copy this build ships for a first-party component.
#[derive(Clone, Copy)]
pub(crate) struct ComponentTranslation {
    pub(crate) name: &'static str,
    pub(crate) summary: &'static str,
    pub(crate) detail: &'static str,
}

/// The copy this build ships for a first-party component. A component without
/// a translation is presented from its own manifest instead of being dropped
/// from the catalog.
pub(crate) fn translated_component(
    locale: Locale,
    id: &ComponentId,
) -> Option<ComponentTranslation> {
    let c = copy(locale);
    match id.as_str() {
        "better-manager" => Some(ComponentTranslation {
            name: c.manager_name,
            summary: c.manager_purpose,
            detail: c.manager_detail,
        }),
        "better-monitor" => Some(ComponentTranslation {
            name: c.monitor_name,
            summary: c.monitor_purpose,
            detail: c.monitor_detail,
        }),
        "better-files-example" => Some(ComponentTranslation {
            name: c.files_name,
            summary: c.files_purpose,
            detail: c.files_detail,
        }),
        _ => None,
    }
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
    Manager::probe(
        ComponentCatalog::from_manifests(manifests).expect("example catalog must be valid"),
        &MockPlatform::default(),
    )
    .expect("the mock platform always reports a profile")
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
