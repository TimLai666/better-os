#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Page {
    FirstRun,
    Overview,
    Components,
    ComponentDetail(&'static str),
    Updates,
    ReviewChanges,
    Installing,
    Finished,
    Restore,
    Restored,
    Health,
    DoctorResults,
    Activity,
    Settings,
    EdgeStates,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Modal {
    None,
    ConfirmDisable(&'static str),
    ConfirmCancelInstall,
    ManualRecovery,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ComponentState {
    Healthy,
    UpdateAvailable,
    Available,
    Planned,
    Disabled,
    Incompatible,
    Degraded,
    Failed,
    RestoreAvailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ComponentKind {
    Replacement,
    Enhancement,
    Diagnostic,
    Core,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RestartRequirement {
    None,
    Logout,
    Restart,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DetailTab {
    Overview,
    Versions,
    Permissions,
    Benchmarks,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum InstallStep {
    Download,
    InstallFiles,
    ApplySettings,
    Verify,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ActivityKind {
    Success,
    Warning,
    Failure,
    Information,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ActivityFilter {
    All,
    Success,
    Warning,
    Failure,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ComponentInfo {
    pub(crate) id: &'static str,
    pub(crate) name: &'static str,
    pub(crate) installed_version: Option<&'static str>,
    pub(crate) available_version: Option<&'static str>,
    pub(crate) state: ComponentState,
    pub(crate) kind: ComponentKind,
    pub(crate) restart: RestartRequirement,
    pub(crate) download_size: &'static str,
}

impl ComponentInfo {
    pub(crate) fn version_label(self) -> String {
        match (self.installed_version, self.available_version) {
            (Some(current), Some(next)) if current != next => format!("{current} → {next}"),
            (Some(current), _) => current.to_string(),
            (None, Some(next)) => next.to_string(),
            (None, None) => "—".to_string(),
        }
    }
}

pub(crate) const COMPONENTS: [ComponentInfo; 6] = [
    ComponentInfo {
        id: "manager",
        name: "Better Manager",
        installed_version: Some("0.1.0"),
        available_version: Some("0.1.1"),
        state: ComponentState::UpdateAvailable,
        kind: ComponentKind::Core,
        restart: RestartRequirement::None,
        download_size: "4.8 MB",
    },
    ComponentInfo {
        id: "touchpad",
        name: "Better Touchpad",
        installed_version: Some("0.1.0"),
        available_version: Some("0.1.1"),
        state: ComponentState::UpdateAvailable,
        kind: ComponentKind::Enhancement,
        restart: RestartRequirement::None,
        download_size: "2.1 MB",
    },
    ComponentInfo {
        id: "monitor",
        name: "Better Monitor",
        installed_version: Some("0.1.0"),
        available_version: Some("0.2.0"),
        state: ComponentState::UpdateAvailable,
        kind: ComponentKind::Diagnostic,
        restart: RestartRequirement::Logout,
        download_size: "18.4 MB",
    },
    ComponentInfo {
        id: "launcher",
        name: "Better Launcher",
        installed_version: None,
        available_version: Some("0.1.0"),
        state: ComponentState::Available,
        kind: ComponentKind::Replacement,
        restart: RestartRequirement::Logout,
        download_size: "7.2 MB",
    },
    ComponentInfo {
        id: "files",
        name: "Better Files",
        installed_version: None,
        available_version: None,
        state: ComponentState::Planned,
        kind: ComponentKind::Replacement,
        restart: RestartRequirement::None,
        download_size: "—",
    },
    ComponentInfo {
        id: "input",
        name: "Better Input",
        installed_version: None,
        available_version: None,
        state: ComponentState::Planned,
        kind: ComponentKind::Enhancement,
        restart: RestartRequirement::Logout,
        download_size: "—",
    },
];

pub(crate) fn component_by_id(id: &str) -> Option<ComponentInfo> {
    COMPONENTS
        .iter()
        .copied()
        .find(|component| component.id == id)
}
