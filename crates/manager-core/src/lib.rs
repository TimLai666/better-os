//! Non-privileged component planning and status operations.

use better_core::{ComponentCatalog, ComponentId, ComponentManifest, ManifestError};
use std::collections::HashMap;
use thiserror::Error;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DesiredOperation {
    Install,
    Update,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlanStep {
    pub component: ComponentId,
    pub operation: DesiredOperation,
    pub detail: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransactionPlan {
    pub steps: Vec<PlanStep>,
    pub dry_run: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InstallationState {
    Installed { version: String },
    Available,
}

pub trait ComponentBackend {
    fn installed_version(&self, id: &ComponentId) -> Option<String>;
}

#[derive(Clone, Debug, Default)]
pub struct InMemoryBackend {
    installed: HashMap<ComponentId, String>,
}

impl InMemoryBackend {
    pub fn with_installed(mut self, id: ComponentId, version: impl Into<String>) -> Self {
        self.installed.insert(id, version.into());
        self
    }
}

impl ComponentBackend for InMemoryBackend {
    fn installed_version(&self, id: &ComponentId) -> Option<String> {
        self.installed.get(id).cloned()
    }
}

#[derive(Debug, Error)]
pub enum ManagerError {
    #[error(transparent)]
    Manifest(#[from] ManifestError),
    #[error("component {0} is not in the catalog")]
    UnknownComponent(ComponentId),
    #[error("component {0} is already installed")]
    AlreadyInstalled(ComponentId),
}

#[derive(Clone)]
pub struct Manager<B> {
    catalog: ComponentCatalog,
    backend: B,
}

impl<B: ComponentBackend> Manager<B> {
    pub fn new(catalog: ComponentCatalog, backend: B) -> Self {
        Self { catalog, backend }
    }

    pub fn manifests(&self) -> impl Iterator<Item = &ComponentManifest> {
        self.catalog.manifests()
    }

    pub fn status(&self, id: &ComponentId) -> Result<InstallationState, ManagerError> {
        if self.catalog.get(id).is_none() {
            return Err(ManagerError::UnknownComponent(id.clone()));
        }
        Ok(match self.backend.installed_version(id) {
            Some(version) => InstallationState::Installed { version },
            None => InstallationState::Available,
        })
    }

    pub fn plan(
        &self,
        id: &ComponentId,
        operation: DesiredOperation,
    ) -> Result<TransactionPlan, ManagerError> {
        let manifest = self
            .catalog
            .get(id)
            .ok_or_else(|| ManagerError::UnknownComponent(id.clone()))?;
        if matches!(operation, DesiredOperation::Install)
            && self.backend.installed_version(id).is_some()
        {
            return Err(ManagerError::AlreadyInstalled(id.clone()));
        }
        Ok(TransactionPlan {
            steps: vec![PlanStep {
                component: id.clone(),
                operation,
                detail: format!(
                    "verify {}, then use local APT planning path",
                    manifest.artifact.url
                ),
            }],
            dry_run: true,
        })
    }

    pub fn plan_all(&self) -> Result<TransactionPlan, ManagerError> {
        let mut steps = Vec::new();
        for manifest in self.manifests() {
            let operation = if self.backend.installed_version(&manifest.id).is_some() {
                DesiredOperation::Update
            } else {
                DesiredOperation::Install
            };
            steps.extend(self.plan(&manifest.id, operation)?.steps);
        }
        Ok(TransactionPlan {
            steps,
            dry_run: true,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use better_core::ComponentManifest;

    fn manifest() -> ComponentManifest {
        ComponentManifest::parse_yaml(include_str!(
            "../../../components/manifests/better-monitor.yaml"
        ))
        .unwrap()
    }

    #[test]
    fn plans_install_without_mutating_backend() {
        let manifest = manifest();
        let id = manifest.id.clone();
        let catalog = ComponentCatalog::from_manifests([manifest]).unwrap();
        let manager = Manager::new(catalog, InMemoryBackend::default());
        let plan = manager.plan(&id, DesiredOperation::Install).unwrap();
        assert!(plan.dry_run);
        assert_eq!(manager.status(&id).unwrap(), InstallationState::Available);
    }

    #[test]
    fn rejects_install_for_installed_component() {
        let manifest = manifest();
        let id = manifest.id.clone();
        let catalog = ComponentCatalog::from_manifests([manifest]).unwrap();
        let backend = InMemoryBackend::default().with_installed(id.clone(), "0.1.0");
        let manager = Manager::new(catalog, backend);
        assert!(matches!(
            manager.plan(&id, DesiredOperation::Install),
            Err(ManagerError::AlreadyInstalled(_))
        ));
    }

    #[test]
    fn plans_update_all_through_the_same_core_api() {
        let manifest = manifest();
        let id = manifest.id.clone();
        let catalog = ComponentCatalog::from_manifests([manifest]).unwrap();
        let backend = InMemoryBackend::default().with_installed(id.clone(), "0.0.1");
        let manager = Manager::new(catalog, backend);
        let plan = manager.plan_all().unwrap();
        assert_eq!(plan.steps[0].component, id);
        assert_eq!(plan.steps[0].operation, DesiredOperation::Update);
        assert!(plan.dry_run);
    }
}
