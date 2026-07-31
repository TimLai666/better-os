use better_core::{ComponentCatalog, ComponentId, ComponentManifest};
use gpui::*;
use gpui_component::input::{InputEvent, InputState};
use manager_core::{
    DesiredOperation, InMemoryBackend, InstallationState, Manager, TransactionPlan,
};

use crate::{
    i18n::{Locale, copy},
    model::{ActivityFilter, ComponentInfo, DetailTab, InstallStep, Modal, Page, component_by_id},
};

pub(crate) struct ManagerApp {
    pub(crate) page: Page,
    pub(crate) modal: Modal,
    pub(crate) locale: Locale,
    pub(crate) search: Entity<InputState>,
    pub(crate) search_query: String,
    pub(crate) install_progress: f32,
    pub(crate) install_step: InstallStep,
    pub(crate) detail_tab: DetailTab,
    pub(crate) check_updates: bool,
    pub(crate) auto_download: bool,
    pub(crate) diagnostic_logs: bool,
    pub(crate) activity_filter: ActivityFilter,
    pub(crate) activity_cleared: bool,
    pub(crate) show_no_updates: bool,
    pub(crate) manager: Manager<InMemoryBackend>,
    pub(crate) pending_plan: Option<TransactionPlan>,
    pub(crate) planning_error: Option<PlanningError>,
    pub(crate) _subscriptions: Vec<Subscription>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PlanningError {
    PreviewOnlyComponent,
    CorePlanningFailed,
}

impl ManagerApp {
    pub(crate) fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let locale = Locale::System;
        let search = cx.new(|cx| InputState::new(window, cx).placeholder(copy(locale).search));
        let subscription = Self::subscribe_to_search(&search, window, cx);

        Self {
            page: Page::FirstRun,
            modal: Modal::None,
            locale,
            search,
            search_query: String::new(),
            install_progress: 18.0,
            install_step: InstallStep::Download,
            detail_tab: DetailTab::Overview,
            check_updates: true,
            auto_download: false,
            diagnostic_logs: true,
            activity_filter: ActivityFilter::All,
            activity_cleared: false,
            show_no_updates: false,
            manager: demo_manager(),
            pending_plan: None,
            planning_error: None,
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

    pub(crate) fn set_locale(
        &mut self,
        locale: Locale,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
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
        let locale = self.locale;
        let search = cx.new(|cx| InputState::new(window, cx).placeholder(copy(locale).search));
        let subscription = Self::subscribe_to_search(&search, window, cx);
        self.search = search;
        self.search_query.clear();
        self._subscriptions.clear();
        self._subscriptions.push(subscription);
        cx.notify();
    }

    pub(crate) fn navigate(&mut self, page: Page, cx: &mut Context<Self>) {
        self.page = page;
        self.modal = Modal::None;
        cx.notify();
    }

    pub(crate) fn prepare_update_all(&mut self, cx: &mut Context<Self>) {
        match self.manager.plan_all() {
            Ok(plan) => {
                self.pending_plan = Some(plan);
                self.planning_error = None;
            }
            Err(error) => {
                eprintln!("Update All planning failed: {error}");
                self.pending_plan = None;
                self.planning_error = Some(PlanningError::CorePlanningFailed);
            }
        }
        self.navigate(Page::ReviewChanges, cx);
    }

    pub(crate) fn prepare_component_change(&mut self, id: &'static str, cx: &mut Context<Self>) {
        let Some(core_id) = core_component_id(id) else {
            self.pending_plan = None;
            self.planning_error = Some(PlanningError::PreviewOnlyComponent);
            self.navigate(Page::ReviewChanges, cx);
            return;
        };

        let operation = match self.manager.status(&core_id) {
            Ok(InstallationState::Installed { .. }) => DesiredOperation::Update,
            Ok(InstallationState::Available) => DesiredOperation::Install,
            Err(error) => {
                eprintln!("component planning status failed: {error}");
                self.pending_plan = None;
                self.planning_error = Some(PlanningError::CorePlanningFailed);
                self.navigate(Page::ReviewChanges, cx);
                return;
            }
        };

        match self.manager.plan(&core_id, operation) {
            Ok(plan) => {
                self.pending_plan = Some(plan);
                self.planning_error = None;
            }
            Err(error) => {
                eprintln!("component planning failed: {error}");
                self.pending_plan = None;
                self.planning_error = Some(PlanningError::CorePlanningFailed);
            }
        }
        self.navigate(Page::ReviewChanges, cx);
    }

    pub(crate) fn plan_component_name(&self, id: &ComponentId) -> String {
        let ui_id = match id.as_str() {
            "better-manager" => Some("manager"),
            "better-monitor" => Some("monitor"),
            "better-files-example" => Some("files"),
            _ => None,
        };
        if let Some(ui_id) = ui_id {
            return self.component_name(ui_id).to_string();
        }

        self.manager
            .manifests()
            .find(|manifest| manifest.id.as_str() == id.as_str())
            .map(|manifest| manifest.display_name.clone())
            .unwrap_or_else(|| id.to_string())
    }

    pub(crate) fn plan_paths(&self) -> Vec<String> {
        let mut paths = Vec::new();
        if let Some(plan) = &self.pending_plan {
            for step in &plan.steps {
                if let Some(manifest) = self
                    .manager
                    .manifests()
                    .find(|manifest| manifest.id.as_str() == step.component.as_str())
                {
                    paths.extend(manifest.paths.iter().cloned());
                }
            }
        }
        paths
    }

    pub(crate) fn plan_version_label(&self, id: &ComponentId) -> String {
        let Some(manifest) = self
            .manager
            .manifests()
            .find(|manifest| manifest.id.as_str() == id.as_str())
        else {
            return id.to_string();
        };

        match self.manager.status(id) {
            Ok(InstallationState::Installed { version }) => {
                format!("{version} → {}", manifest.version)
            }
            Ok(InstallationState::Available) | Err(_) => manifest.version.to_string(),
        }
    }

    pub(crate) fn installed_count(&self) -> usize {
        self.manager
            .manifests()
            .filter(|manifest| {
                matches!(
                    self.manager.status(&manifest.id),
                    Ok(InstallationState::Installed { .. })
                )
            })
            .count()
    }

    pub(crate) fn update_plan_count(&self) -> usize {
        self.manager
            .plan_all()
            .map(|plan| plan.steps.len())
            .unwrap_or_default()
    }

    pub(crate) fn open_component(&mut self, id: &'static str, cx: &mut Context<Self>) {
        self.page = Page::ComponentDetail(id);
        self.detail_tab = DetailTab::Overview;
        self.modal = Modal::None;
        cx.notify();
    }

    pub(crate) fn selected_component(&self) -> ComponentInfo {
        match self.page {
            Page::ComponentDetail(id) => component_by_id(id),
            _ => component_by_id("monitor"),
        }
        .unwrap_or_else(|| component_by_id("manager").expect("manager component must exist"))
    }

    pub(crate) fn purpose(&self, id: &str) -> &'static str {
        let c = copy(self.locale);
        match id {
            "manager" => c.manager_purpose,
            "touchpad" => c.touchpad_purpose,
            "monitor" => c.monitor_purpose,
            "launcher" => c.launcher_purpose,
            "files" => c.files_purpose,
            "input" => c.input_purpose,
            _ => "",
        }
    }

    pub(crate) fn detail(&self, id: &str) -> &'static str {
        let c = copy(self.locale);
        match id {
            "manager" => c.manager_detail,
            "touchpad" => c.touchpad_detail,
            "monitor" => c.monitor_detail,
            "launcher" => c.launcher_detail,
            "files" => c.files_detail,
            "input" => c.input_detail,
            _ => "",
        }
    }

    pub(crate) fn component_name(&self, id: &str) -> &'static str {
        let c = copy(self.locale);
        match id {
            "manager" => c.manager_name,
            "touchpad" => c.touchpad_name,
            "monitor" => c.monitor_name,
            "launcher" => c.launcher_name,
            "files" => c.files_name,
            "input" => c.input_name,
            _ => "",
        }
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
            Page::Settings => matches!(self.page, Page::Settings | Page::EdgeStates),
            other => self.page == other,
        }
    }

    pub(crate) fn begin_install(&mut self, cx: &mut Context<Self>) {
        if self
            .pending_plan
            .as_ref()
            .map(|plan| plan.steps.is_empty())
            .unwrap_or(true)
        {
            return;
        }
        self.install_progress = 18.0;
        self.install_step = InstallStep::Download;
        self.page = Page::Installing;
        self.modal = Modal::None;
        cx.notify();
    }

    pub(crate) fn advance_install_preview(&mut self, cx: &mut Context<Self>) {
        match self.install_step {
            InstallStep::Download => {
                self.install_step = InstallStep::InstallFiles;
                self.install_progress = 46.0;
            }
            InstallStep::InstallFiles => {
                self.install_step = InstallStep::ApplySettings;
                self.install_progress = 72.0;
            }
            InstallStep::ApplySettings => {
                self.install_step = InstallStep::Verify;
                self.install_progress = 92.0;
            }
            InstallStep::Verify => {
                self.install_progress = 100.0;
                self.page = Page::Finished;
            }
        }
        cx.notify();
    }

    pub(crate) fn fail_install_preview(&mut self, cx: &mut Context<Self>) {
        self.install_step = InstallStep::Verify;
        self.install_progress = 94.0;
        self.page = Page::Restore;
        cx.notify();
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

pub(crate) fn demo_manager() -> Manager<InMemoryBackend> {
    let manifests = [
        include_str!("../../../components/manifests/better-manager.yaml"),
        include_str!("../../../components/manifests/better-monitor.yaml"),
        include_str!("../../../components/manifests/better-files-example.yaml"),
    ]
    .into_iter()
    .map(|input| ComponentManifest::parse_yaml(input).expect("example manifest must be valid"))
    .collect::<Vec<_>>();

    let backend = InMemoryBackend::default()
        .with_installed(
            ComponentId::new("better-manager").expect("example id must be valid"),
            "0.0.1",
        )
        .with_installed(
            ComponentId::new("better-monitor").expect("example id must be valid"),
            "0.0.1",
        )
        .with_installed(
            ComponentId::new("better-files-example").expect("example id must be valid"),
            "0.0.1",
        );

    Manager::new(
        ComponentCatalog::from_manifests(manifests).expect("example catalog must be valid"),
        backend,
    )
}
