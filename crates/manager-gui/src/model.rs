use better_core::{ComponentId, ComponentType};
use manager_core::{ComponentStatus, HealthState, RestartRequirement};

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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DetailTab {
    Overview,
    Versions,
    Permissions,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ActivityFilter {
    All,
    Success,
    Warning,
    Failure,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ComponentKind {
    Replacement,
    Enhancement,
    Diagnostic,
}

impl From<ComponentType> for ComponentKind {
    fn from(value: ComponentType) -> Self {
        match value {
            ComponentType::Replacement => Self::Replacement,
            ComponentType::Enhancement => Self::Enhancement,
            ComponentType::Diagnostic => Self::Diagnostic,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ComponentInfo {
    pub(crate) ui_id: &'static str,
    pub(crate) core_id: ComponentId,
    pub(crate) installed_version: Option<String>,
    pub(crate) enabled: bool,
    pub(crate) available_version: String,
    pub(crate) state: ComponentStatus,
    pub(crate) health: HealthState,
    pub(crate) restart_requirement: RestartRequirement,
    pub(crate) restore_available: bool,
    pub(crate) kind: ComponentKind,
    pub(crate) paths: Vec<String>,
    pub(crate) release_notes: Vec<String>,
}

impl ComponentInfo {
    pub(crate) fn version_label(&self) -> String {
        match self.installed_version.as_deref() {
            Some(current) if current != self.available_version => {
                format!("{current} → {}", self.available_version)
            }
            Some(current) => current.to_string(),
            None => self.available_version.clone(),
        }
    }
}

pub(crate) fn ui_id_for_component(id: &ComponentId) -> Option<&'static str> {
    match id.as_str() {
        "better-manager" => Some("manager"),
        "better-monitor" => Some("monitor"),
        "better-files-example" => Some("files"),
        _ => None,
    }
}
