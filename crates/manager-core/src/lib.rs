//! Shared, non-privileged component lifecycle planning and mock execution.
//!
//! Manifest lifecycle descriptors remain untrusted data. This crate never
//! interprets them as shell commands, package-manager invocations, or system
//! mutation. It only produces and advances deterministic mock state.

use better_core::{ComponentCatalog, ComponentId, ComponentManifest, ComponentType, ManifestError};
use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

pub const STATE_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReleaseChannel {
    #[default]
    Stable,
    Preview,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StoredLocale {
    #[default]
    System,
    EnUs,
    ZhTw,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComponentFilterPreference {
    #[default]
    All,
    Installed,
    Updates,
    Disabled,
    NeedsAttention,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ManagerSettings {
    pub release_channel: ReleaseChannel,
    pub locale: StoredLocale,
    pub check_updates: bool,
    pub auto_download: bool,
    pub diagnostic_logs: bool,
    pub onboarding_complete: bool,
    pub component_filter: ComponentFilterPreference,
}

impl Default for ManagerSettings {
    fn default() -> Self {
        Self {
            release_channel: ReleaseChannel::Stable,
            locale: StoredLocale::System,
            check_updates: true,
            auto_download: false,
            diagnostic_logs: true,
            onboarding_complete: false,
            component_filter: ComponentFilterPreference::All,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DesiredOperation {
    Install,
    Update,
    Enable,
    Disable,
    Verify,
    Restore,
    Remove,
}

impl DesiredOperation {
    pub fn mutates_component(self) -> bool {
        !matches!(self, Self::Verify)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationStage {
    Downloading,
    Installing,
    ApplyingSettings,
    CheckingHealth,
}

impl OperationStage {
    pub const ALL: [Self; 4] = [
        Self::Downloading,
        Self::Installing,
        Self::ApplyingSettings,
        Self::CheckingHealth,
    ];

    fn next(self) -> Option<Self> {
        match self {
            Self::Downloading => Some(Self::Installing),
            Self::Installing => Some(Self::ApplyingSettings),
            Self::ApplyingSettings => Some(Self::CheckingHealth),
            Self::CheckingHealth => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComponentStatus {
    Available,
    Downloading,
    ReadyToInstall,
    Installing,
    Verifying,
    Healthy,
    UpdateAvailable,
    Disabled,
    Incompatible,
    Degraded,
    Failed,
    RestoreAvailable,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HealthState {
    #[default]
    Healthy,
    Degraded,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RestartRequirement {
    /// Manifests currently do not expose restart metadata. Do not invent it.
    NotDeclared,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiskSpaceCheck {
    NotRequired,
    #[default]
    NotDeclared,
    Sufficient {
        required_bytes: u64,
        available_bytes: u64,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryStatus {
    Restored,
    PartiallyRestored,
    ManualRecoveryRequired,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FailureRecord {
    pub component: ComponentId,
    pub stage: OperationStage,
    pub evidence: String,
    pub recovery: Option<RecoveryStatus>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ComponentSnapshot {
    pub installed_version: Option<String>,
    pub enabled: bool,
    pub health: HealthState,
}

impl ComponentSnapshot {
    fn from_record(record: Option<&ComponentRecord>) -> Self {
        match record {
            Some(record) => Self {
                installed_version: record.installed_version.clone(),
                enabled: record.enabled,
                health: record.health,
            },
            None => Self {
                installed_version: None,
                enabled: false,
                health: HealthState::Healthy,
            },
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ComponentRecord {
    pub installed_version: Option<String>,
    pub enabled: bool,
    pub health: HealthState,
    pub restore_snapshot: Option<ComponentSnapshot>,
    pub failure: Option<FailureRecord>,
    pub recovery: Option<RecoveryStatus>,
}

impl Default for ComponentRecord {
    fn default() -> Self {
        Self {
            installed_version: None,
            enabled: false,
            health: HealthState::Healthy,
            restore_snapshot: None,
            failure: None,
            recovery: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivityKind {
    Information,
    Success,
    Warning,
    Failure,
    RecoverySuccess,
    RecoveryPartial,
    ManualRecovery,
}

impl ActivityKind {
    pub fn is_recovery_success(self) -> bool {
        matches!(self, Self::RecoverySuccess)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ActivityRecord {
    pub sequence: u64,
    pub kind: ActivityKind,
    pub component: Option<ComponentId>,
    pub operation: Option<DesiredOperation>,
    pub stage: Option<OperationStage>,
    pub evidence: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PlanStep {
    pub component: ComponentId,
    pub operation: DesiredOperation,
    pub before_version: Option<String>,
    pub after_version: Option<String>,
    pub dependencies: Vec<ComponentId>,
    pub conflicts: Vec<ComponentId>,
    pub paths: Vec<String>,
    pub restart_requirement: RestartRequirement,
    pub estimated_download_bytes: Option<u64>,
    #[serde(default)]
    pub required_disk_bytes: Option<u64>,
    #[serde(default)]
    pub release_notes: Vec<String>,
    pub rollback_available: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TransactionPlan {
    /// The state revision when the user reviewed this plan. Beginning a plan
    /// rejects it if the durable state has since changed.
    state_revision: u64,
    steps: Vec<PlanStep>,
    dry_run: bool,
    roots: Vec<ComponentId>,
    #[serde(default)]
    disk_space: DiskSpaceCheck,
}

impl TransactionPlan {
    pub fn state_revision(&self) -> u64 {
        self.state_revision
    }

    pub fn steps(&self) -> &[PlanStep] {
        &self.steps
    }

    pub fn is_dry_run(&self) -> bool {
        self.dry_run
    }

    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    pub fn roots(&self) -> &[ComponentId] {
        &self.roots
    }

    pub fn disk_space(&self) -> DiskSpaceCheck {
        self.disk_space
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ActiveOperation {
    pub plan: TransactionPlan,
    pub stage: OperationStage,
    pub snapshots: BTreeMap<ComponentId, Option<ComponentRecord>>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ManagerState {
    pub schema_version: u32,
    pub revision: u64,
    #[serde(default)]
    pub components: BTreeMap<ComponentId, ComponentRecord>,
    #[serde(default)]
    pub activity: Vec<ActivityRecord>,
    #[serde(default)]
    pub settings: ManagerSettings,
    #[serde(default)]
    pub active_operation: Option<ActiveOperation>,
}

impl Default for ManagerState {
    fn default() -> Self {
        Self {
            schema_version: STATE_SCHEMA_VERSION,
            revision: 0,
            components: BTreeMap::new(),
            activity: Vec::new(),
            settings: ManagerSettings::default(),
            active_operation: None,
        }
    }
}

impl ManagerState {
    pub fn validate(&self) -> Result<(), StateValidationError> {
        if self.schema_version != STATE_SCHEMA_VERSION {
            return Err(StateValidationError::UnsupportedSchemaVersion(
                self.schema_version,
            ));
        }
        for (id, record) in &self.components {
            validate_component_id(id)?;
            validate_component_record(id, record)?;
        }
        for (offset, activity) in self.activity.iter().enumerate() {
            let expected = offset as u64 + 1;
            if activity.sequence != expected {
                return Err(StateValidationError::InvalidActivitySequence {
                    expected,
                    actual: activity.sequence,
                });
            }
            if let Some(component) = &activity.component {
                validate_component_id(component)?;
            }
        }
        if let Some(active) = &self.active_operation {
            if active.plan.steps.is_empty() {
                return Err(StateValidationError::InvalidActivePlan(
                    "active plan has no steps".to_string(),
                ));
            }
            if active.plan.state_revision > self.revision {
                return Err(StateValidationError::InvalidActivePlan(
                    "active plan revision is newer than state".to_string(),
                ));
            }
            if !active.plan.dry_run {
                return Err(StateValidationError::InvalidActivePlan(
                    "active plan is not marked as a dry run".to_string(),
                ));
            }
            if active.plan.roots.is_empty()
                || active
                    .plan
                    .roots
                    .iter()
                    .any(|root| !active.plan.steps.iter().any(|step| &step.component == root))
            {
                return Err(StateValidationError::InvalidActivePlan(
                    "active plan roots do not match its steps".to_string(),
                ));
            }
            let mut seen_roots = BTreeSet::new();
            if active
                .plan
                .roots
                .iter()
                .any(|root| !seen_roots.insert(root.clone()))
            {
                return Err(StateValidationError::InvalidActivePlan(
                    "active plan contains a duplicate root".to_string(),
                ));
            }
            let mut seen_steps = BTreeSet::new();
            for step in &active.plan.steps {
                validate_component_id(&step.component)?;
                if !seen_steps.insert(step.component.clone()) {
                    return Err(StateValidationError::InvalidActivePlan(
                        "active plan contains a duplicate component".to_string(),
                    ));
                }
                validate_optional_version(&step.component, step.before_version.as_deref())?;
                validate_optional_version(&step.component, step.after_version.as_deref())?;
                for dependency in &step.dependencies {
                    validate_component_id(dependency)?;
                }
                for conflict in &step.conflicts {
                    validate_component_id(conflict)?;
                }
                if !active.snapshots.contains_key(&step.component) {
                    return Err(StateValidationError::InvalidActivePlan(
                        "active plan is missing a component snapshot".to_string(),
                    ));
                }
            }
            for (id, snapshot) in &active.snapshots {
                validate_component_id(id)?;
                if let Some(snapshot) = snapshot {
                    validate_component_record(id, snapshot)?;
                }
            }
        }
        Ok(())
    }

    pub fn component(&self, id: &ComponentId) -> Option<&ComponentRecord> {
        self.components.get(id)
    }

    pub fn set_installed(&mut self, id: ComponentId, version: impl Into<String>, enabled: bool) {
        self.components.insert(
            id,
            ComponentRecord {
                installed_version: Some(version.into()),
                enabled,
                health: HealthState::Healthy,
                restore_snapshot: None,
                failure: None,
                recovery: None,
            },
        );
        self.bump_revision();
    }

    pub fn clear_activity(&mut self) {
        self.activity.clear();
        self.bump_revision();
    }

    pub fn update_settings(&mut self, settings: ManagerSettings) {
        self.settings = settings;
        self.bump_revision();
    }

    fn bump_revision(&mut self) {
        self.revision = self.revision.saturating_add(1);
    }

    fn record(
        &mut self,
        kind: ActivityKind,
        component: Option<ComponentId>,
        operation: Option<DesiredOperation>,
        stage: Option<OperationStage>,
        evidence: Option<String>,
    ) {
        self.activity.push(ActivityRecord {
            sequence: self.activity.len() as u64 + 1,
            kind,
            component,
            operation,
            stage,
            evidence,
        });
    }
}

fn validate_component_id(id: &ComponentId) -> Result<(), StateValidationError> {
    ComponentId::new(id.as_str())
        .map_err(|_| StateValidationError::InvalidComponentId(id.to_string()))?;
    Ok(())
}

fn validate_optional_version(
    component: &ComponentId,
    version: Option<&str>,
) -> Result<(), StateValidationError> {
    if let Some(version) = version {
        Version::parse(version).map_err(|_| StateValidationError::InvalidVersion {
            component: component.clone(),
            version: version.to_string(),
        })?;
    }
    Ok(())
}

fn validate_component_record(
    id: &ComponentId,
    record: &ComponentRecord,
) -> Result<(), StateValidationError> {
    validate_optional_version(id, record.installed_version.as_deref())?;
    if let Some(snapshot) = &record.restore_snapshot {
        validate_optional_version(id, snapshot.installed_version.as_deref())?;
    }
    if let Some(failure) = &record.failure {
        validate_component_id(&failure.component)?;
        if failure.component != *id {
            return Err(StateValidationError::InvalidActivePlan(
                "component failure is attached to a different component".to_string(),
            ));
        }
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MockSystemProfile {
    pub distribution: String,
    pub release: String,
    pub architecture: String,
    pub free_disk_bytes: Option<u64>,
}

impl Default for MockSystemProfile {
    fn default() -> Self {
        Self {
            distribution: "ubuntu".to_string(),
            release: "24.04".to_string(),
            architecture: "amd64".to_string(),
            free_disk_bytes: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MockOutcome {
    Succeed,
    FailAt(OperationStage),
    RestorePartially,
    RestoreRequiresManualRecovery,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OperationProgress {
    InProgress { stage: OperationStage },
    Finished { operation: DesiredOperation },
    Failed { failure: FailureRecord },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DoctorCheckKind {
    Catalog,
    Compatibility,
    ComponentHealth,
    RestoreData,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DoctorCheckStatus {
    Passed,
    Warning,
    Failed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DoctorCheck {
    pub kind: DoctorCheckKind,
    pub status: DoctorCheckStatus,
    pub component: Option<ComponentId>,
}

#[derive(Debug, Error)]
pub enum StateValidationError {
    #[error("manager.state.unsupported_schema:{0}")]
    UnsupportedSchemaVersion(u32),
    #[error("manager.state.invalid_component_id:{0}")]
    InvalidComponentId(String),
    #[error("manager.state.invalid_version:{component}:{version}")]
    InvalidVersion {
        component: ComponentId,
        version: String,
    },
    #[error("manager.state.invalid_activity_sequence:{expected}:{actual}")]
    InvalidActivitySequence { expected: u64, actual: u64 },
    #[error("manager.state.invalid_active_plan:{0}")]
    InvalidActivePlan(String),
}

#[derive(Debug, Error)]
pub enum ManagerError {
    #[error("manager.error.invalid_state")]
    InvalidState(#[from] StateValidationError),
    #[error("manager.error.invalid_manifest")]
    Manifest(#[from] ManifestError),
    #[error("manager.error.unknown_component:{0}")]
    UnknownComponent(ComponentId),
    #[error("manager.error.already_installed:{0}")]
    AlreadyInstalled(ComponentId),
    #[error("manager.error.not_installed:{0}")]
    NotInstalled(ComponentId),
    #[error("manager.error.no_update_available:{0}")]
    NoUpdateAvailable(ComponentId),
    #[error("manager.error.already_enabled:{0}")]
    AlreadyEnabled(ComponentId),
    #[error("manager.error.already_disabled:{0}")]
    AlreadyDisabled(ComponentId),
    #[error("manager.error.no_restore_available:{0}")]
    NoRestoreAvailable(ComponentId),
    #[error("manager.error.incompatible:{component}")]
    Incompatible { component: ComponentId },
    #[error("manager.error.conflict:{component}:{conflict}")]
    Conflict {
        component: ComponentId,
        conflict: ComponentId,
    },
    #[error("manager.error.invalid_dependency_requirement:{component}:{dependency}:{requirement}")]
    InvalidDependencyRequirement {
        component: ComponentId,
        dependency: ComponentId,
        requirement: String,
    },
    #[error("manager.error.dependency_unavailable:{component}:{dependency}")]
    DependencyUnavailable {
        component: ComponentId,
        dependency: ComponentId,
    },
    #[error("manager.error.required_by:{component}:{dependent}")]
    RequiredBy {
        component: ComponentId,
        dependent: ComponentId,
    },
    #[error("manager.error.insufficient_disk_space:{required_bytes}:{available_bytes}")]
    InsufficientDiskSpace {
        required_bytes: u64,
        available_bytes: u64,
    },
    #[error("manager.error.disk_space_requirement_overflow")]
    DiskSpaceRequirementOverflow,
    #[error("manager.error.active_operation")]
    ActiveOperation,
    #[error("manager.error.stale_plan")]
    StalePlan,
    #[error("manager.error.empty_plan")]
    EmptyPlan,
    #[error("manager.error.no_active_operation")]
    NoActiveOperation,
}

/// The shared non-privileged planning and mock lifecycle API.
#[derive(Clone)]
pub struct Manager {
    catalog: ComponentCatalog,
    profile: MockSystemProfile,
}

impl Manager {
    pub fn new(catalog: ComponentCatalog, profile: MockSystemProfile) -> Self {
        Self { catalog, profile }
    }

    pub fn manifests(&self) -> impl Iterator<Item = &ComponentManifest> {
        let mut manifests = self.catalog.manifests().collect::<Vec<_>>();
        manifests.sort_by(|left, right| left.id.as_str().cmp(right.id.as_str()));
        manifests.into_iter()
    }

    pub fn profile(&self) -> &MockSystemProfile {
        &self.profile
    }

    pub fn validate_state(&self, state: &ManagerState) -> Result<(), ManagerError> {
        state.validate()?;
        for id in state.components.keys() {
            self.manifest(id)?;
        }
        if let Some(active) = &state.active_operation {
            let snapshot_state = ManagerState {
                revision: active.plan.state_revision,
                components: active
                    .snapshots
                    .iter()
                    .filter_map(|(id, snapshot)| {
                        snapshot.clone().map(|snapshot| (id.clone(), snapshot))
                    })
                    .collect(),
                ..ManagerState::default()
            };
            self.validate_plan_against_state(&snapshot_state, &active.plan)?;

            let mut expected = snapshot_state.clone();
            if matches!(
                active.stage,
                OperationStage::ApplyingSettings | OperationStage::CheckingHealth
            ) {
                self.apply_steps(&mut expected, active)?;
            }
            for step in active.plan.steps() {
                let actual = state.components.get(&step.component);
                let expected_record = expected.components.get(&step.component);
                let matches = match (actual, expected_record) {
                    (None, None) => true,
                    (Some(actual), Some(expected)) => {
                        actual.installed_version == expected.installed_version
                            && actual.enabled == expected.enabled
                            && actual.health == expected.health
                    }
                    _ => false,
                };
                if !matches {
                    return Err(ManagerError::InvalidState(
                        StateValidationError::InvalidActivePlan(
                            "active stage does not match its saved snapshot".to_string(),
                        ),
                    ));
                }
            }
        }
        Ok(())
    }

    pub fn status(
        &self,
        state: &ManagerState,
        id: &ComponentId,
    ) -> Result<ComponentStatus, ManagerError> {
        self.validate_state(state)?;
        let manifest = self.manifest(id)?;
        if !self.is_compatible(manifest) {
            return Ok(ComponentStatus::Incompatible);
        }

        if let Some(active) = &state.active_operation {
            if active.plan.steps.iter().any(|step| &step.component == id) {
                return Ok(match active.stage {
                    OperationStage::Downloading => ComponentStatus::Downloading,
                    OperationStage::Installing | OperationStage::ApplyingSettings => {
                        ComponentStatus::Installing
                    }
                    OperationStage::CheckingHealth => ComponentStatus::Verifying,
                });
            }
        }

        let Some(record) = state.component(id) else {
            return Ok(ComponentStatus::Available);
        };
        if record.failure.is_some() {
            return Ok(if record.restore_snapshot.is_some() {
                ComponentStatus::RestoreAvailable
            } else {
                ComponentStatus::Failed
            });
        }
        if record.installed_version.is_none() {
            return Ok(ComponentStatus::Available);
        }
        if !record.enabled {
            return Ok(ComponentStatus::Disabled);
        }
        match record.health {
            HealthState::Failed => Ok(ComponentStatus::Failed),
            HealthState::Degraded => Ok(ComponentStatus::Degraded),
            HealthState::Healthy if self.update_available(record, manifest) => {
                Ok(ComponentStatus::UpdateAvailable)
            }
            HealthState::Healthy => Ok(ComponentStatus::Healthy),
        }
    }

    pub fn plan(
        &self,
        state: &ManagerState,
        id: &ComponentId,
        operation: DesiredOperation,
    ) -> Result<TransactionPlan, ManagerError> {
        self.validate_state(state)?;
        self.ensure_idle(state)?;
        let mut steps = Vec::new();
        let mut scheduled = BTreeSet::new();
        self.resolve(
            state,
            id,
            operation,
            &mut BTreeSet::new(),
            &mut scheduled,
            &mut steps,
        )?;
        let disk_space = self.disk_space_check(&steps)?;
        Ok(TransactionPlan {
            state_revision: state.revision,
            steps,
            dry_run: true,
            roots: vec![id.clone()],
            disk_space,
        })
    }

    pub fn plan_all(&self, state: &ManagerState) -> Result<TransactionPlan, ManagerError> {
        self.validate_state(state)?;
        self.ensure_idle(state)?;
        let mut steps = Vec::new();
        let mut scheduled = BTreeSet::new();
        let mut roots = Vec::new();
        for manifest in self.manifests() {
            if self.status(state, &manifest.id)? != ComponentStatus::UpdateAvailable {
                continue;
            }
            roots.push(manifest.id.clone());
            self.resolve(
                state,
                &manifest.id,
                DesiredOperation::Update,
                &mut BTreeSet::new(),
                &mut scheduled,
                &mut steps,
            )?;
        }
        let disk_space = self.disk_space_check(&steps)?;
        Ok(TransactionPlan {
            state_revision: state.revision,
            steps,
            dry_run: true,
            roots,
            disk_space,
        })
    }

    pub fn begin(
        &self,
        state: &mut ManagerState,
        plan: TransactionPlan,
    ) -> Result<OperationProgress, ManagerError> {
        self.validate_state(state)?;
        self.ensure_idle(state)?;
        if plan.state_revision != state.revision {
            return Err(ManagerError::StalePlan);
        }
        self.validate_plan_against_state(state, &plan)?;

        // Capture the complete pre-operation state, not only changed records.
        // A plan can rely on an already installed dependency that is not one of
        // its steps, and cancellation must restore the exact pre-operation
        // component set rather than leave a partial mutation behind.
        let mut snapshots = state
            .components
            .iter()
            .map(|(id, record)| (id.clone(), Some(record.clone())))
            .collect::<BTreeMap<_, _>>();
        for step in &plan.steps {
            snapshots
                .entry(step.component.clone())
                .or_insert_with(|| state.components.get(&step.component).cloned());
        }
        state.active_operation = Some(ActiveOperation {
            plan: plan.clone(),
            stage: OperationStage::Downloading,
            snapshots,
        });
        state.record(
            ActivityKind::Information,
            plan.steps.first().map(|step| step.component.clone()),
            plan.steps.first().map(|step| step.operation),
            Some(OperationStage::Downloading),
            None,
        );
        state.bump_revision();
        Ok(OperationProgress::InProgress {
            stage: OperationStage::Downloading,
        })
    }

    pub fn advance(
        &self,
        state: &mut ManagerState,
        outcome: MockOutcome,
    ) -> Result<OperationProgress, ManagerError> {
        self.validate_state(state)?;
        let active = state
            .active_operation
            .clone()
            .ok_or(ManagerError::NoActiveOperation)?;

        if matches!(outcome, MockOutcome::FailAt(stage) if stage == active.stage) {
            return Ok(self.fail(state, active));
        }

        match active.stage {
            OperationStage::Downloading | OperationStage::ApplyingSettings => {
                let next = active.stage.next().expect("these stages have a successor");
                state
                    .active_operation
                    .as_mut()
                    .expect("active operation checked above")
                    .stage = next;
                state.record(
                    ActivityKind::Information,
                    active.plan.steps.first().map(|step| step.component.clone()),
                    active.plan.steps.first().map(|step| step.operation),
                    Some(next),
                    None,
                );
                state.bump_revision();
                Ok(OperationProgress::InProgress { stage: next })
            }
            OperationStage::Installing => {
                self.apply_steps(state, &active)?;
                let next = OperationStage::ApplyingSettings;
                state
                    .active_operation
                    .as_mut()
                    .expect("active operation checked above")
                    .stage = next;
                state.record(
                    ActivityKind::Information,
                    active.plan.steps.first().map(|step| step.component.clone()),
                    active.plan.steps.first().map(|step| step.operation),
                    Some(next),
                    None,
                );
                state.bump_revision();
                Ok(OperationProgress::InProgress { stage: next })
            }
            OperationStage::CheckingHealth => Ok(self.finish(state, active, outcome)),
        }
    }

    pub fn cancel(&self, state: &mut ManagerState) -> Result<(), ManagerError> {
        self.validate_state(state)?;
        let active = state
            .active_operation
            .take()
            .ok_or(ManagerError::NoActiveOperation)?;
        self.restore_active_snapshots(state, &active);
        state.record(
            ActivityKind::Warning,
            active.plan.steps.first().map(|step| step.component.clone()),
            active.plan.steps.first().map(|step| step.operation),
            Some(active.stage),
            Some("mock_operation_cancelled".to_string()),
        );
        state.bump_revision();
        Ok(())
    }

    pub fn doctor(&self, state: &ManagerState) -> Result<Vec<DoctorCheck>, ManagerError> {
        self.validate_state(state)?;
        let mut checks = vec![DoctorCheck {
            kind: DoctorCheckKind::Catalog,
            status: DoctorCheckStatus::Passed,
            component: None,
        }];
        for manifest in self.manifests() {
            let compatibility = if self.is_compatible(manifest) {
                DoctorCheckStatus::Passed
            } else {
                DoctorCheckStatus::Warning
            };
            checks.push(DoctorCheck {
                kind: DoctorCheckKind::Compatibility,
                status: compatibility,
                component: Some(manifest.id.clone()),
            });
            let health = match self.status(state, &manifest.id) {
                Ok(ComponentStatus::Failed) | Ok(ComponentStatus::RestoreAvailable) => {
                    DoctorCheckStatus::Failed
                }
                Ok(ComponentStatus::Degraded)
                | Ok(ComponentStatus::UpdateAvailable)
                | Ok(ComponentStatus::Incompatible) => DoctorCheckStatus::Warning,
                Ok(_) | Err(_) => DoctorCheckStatus::Passed,
            };
            checks.push(DoctorCheck {
                kind: DoctorCheckKind::ComponentHealth,
                status: health,
                component: Some(manifest.id.clone()),
            });
            if state
                .component(&manifest.id)
                .and_then(|record| record.restore_snapshot.as_ref())
                .is_some()
            {
                checks.push(DoctorCheck {
                    kind: DoctorCheckKind::RestoreData,
                    status: DoctorCheckStatus::Passed,
                    component: Some(manifest.id.clone()),
                });
            }
        }
        Ok(checks)
    }

    fn validate_plan_against_state(
        &self,
        state: &ManagerState,
        plan: &TransactionPlan,
    ) -> Result<(), ManagerError> {
        if plan.is_empty() {
            return Err(ManagerError::EmptyPlan);
        }
        if plan.state_revision != state.revision {
            return Err(ManagerError::StalePlan);
        }
        if plan.roots.is_empty() {
            return Err(ManagerError::InvalidState(
                StateValidationError::InvalidActivePlan(
                    "reviewed plan has no root operation".to_string(),
                ),
            ));
        }
        let mut expected_steps = Vec::new();
        let mut scheduled = BTreeSet::new();
        for root in &plan.roots {
            let root_step = plan
                .steps()
                .iter()
                .find(|step| step.component == *root)
                .ok_or_else(|| {
                    ManagerError::InvalidState(StateValidationError::InvalidActivePlan(
                        "reviewed plan root is not a step".to_string(),
                    ))
                })?;
            self.resolve(
                state,
                root,
                root_step.operation,
                &mut BTreeSet::new(),
                &mut scheduled,
                &mut expected_steps,
            )?;
        }
        if expected_steps != plan.steps {
            return Err(ManagerError::InvalidState(
                StateValidationError::InvalidActivePlan(
                    "reviewed plan no longer matches catalog resolution".to_string(),
                ),
            ));
        }
        Ok(())
    }

    fn resolve(
        &self,
        state: &ManagerState,
        id: &ComponentId,
        operation: DesiredOperation,
        visiting: &mut BTreeSet<ComponentId>,
        scheduled: &mut BTreeSet<ComponentId>,
        steps: &mut Vec<PlanStep>,
    ) -> Result<(), ManagerError> {
        if scheduled.contains(id) {
            return Ok(());
        }
        let manifest = self.manifest(id)?;
        if !self.is_compatible(manifest) {
            return Err(ManagerError::Incompatible {
                component: id.clone(),
            });
        }
        if !visiting.insert(id.clone()) {
            return Err(ManagerError::DependencyUnavailable {
                component: id.clone(),
                dependency: id.clone(),
            });
        }

        let record = state.component(id);
        match operation {
            DesiredOperation::Install => {
                if record
                    .and_then(|record| record.installed_version.as_ref())
                    .is_some()
                {
                    return Err(ManagerError::AlreadyInstalled(id.clone()));
                }
                self.resolve_dependencies(state, manifest, visiting, scheduled, steps)?;
                self.ensure_no_conflict(state, manifest, steps)?;
            }
            DesiredOperation::Update => {
                let Some(record) = record else {
                    return Err(ManagerError::NotInstalled(id.clone()));
                };
                if !self.update_available(record, manifest) {
                    return Err(ManagerError::NoUpdateAvailable(id.clone()));
                }
                self.resolve_dependencies(state, manifest, visiting, scheduled, steps)?;
                self.ensure_no_conflict(state, manifest, steps)?;
            }
            DesiredOperation::Enable => {
                let Some(record) = record else {
                    return Err(ManagerError::NotInstalled(id.clone()));
                };
                if record.enabled {
                    return Err(ManagerError::AlreadyEnabled(id.clone()));
                }
                self.ensure_no_conflict(state, manifest, steps)?;
            }
            DesiredOperation::Disable => {
                let Some(record) = record else {
                    return Err(ManagerError::NotInstalled(id.clone()));
                };
                if !record.enabled {
                    return Err(ManagerError::AlreadyDisabled(id.clone()));
                }
            }
            DesiredOperation::Verify => {
                if record
                    .and_then(|record| record.installed_version.as_ref())
                    .is_none()
                {
                    return Err(ManagerError::NotInstalled(id.clone()));
                }
            }
            DesiredOperation::Remove => {
                if record
                    .and_then(|record| record.installed_version.as_ref())
                    .is_none()
                {
                    return Err(ManagerError::NotInstalled(id.clone()));
                }
                self.ensure_no_installed_dependents(state, id, steps)?;
            }
            DesiredOperation::Restore => {
                if record
                    .and_then(|record| record.restore_snapshot.as_ref())
                    .is_none()
                {
                    return Err(ManagerError::NoRestoreAvailable(id.clone()));
                }
            }
        }

        steps.push(self.plan_step(state, manifest, operation));
        scheduled.insert(id.clone());
        visiting.remove(id);
        Ok(())
    }

    fn resolve_dependencies(
        &self,
        state: &ManagerState,
        manifest: &ComponentManifest,
        visiting: &mut BTreeSet<ComponentId>,
        scheduled: &mut BTreeSet<ComponentId>,
        steps: &mut Vec<PlanStep>,
    ) -> Result<(), ManagerError> {
        let mut dependencies = manifest.dependencies.clone();
        dependencies.sort_by(|left, right| left.id.cmp(&right.id));
        for dependency in dependencies {
            let requirement = VersionReq::parse(&dependency.version).map_err(|_| {
                ManagerError::InvalidDependencyRequirement {
                    component: manifest.id.clone(),
                    dependency: dependency.id.clone(),
                    requirement: dependency.version.clone(),
                }
            })?;
            let dependency_manifest = self.manifest(&dependency.id)?;
            if !requirement.matches(&dependency_manifest.version) {
                return Err(ManagerError::DependencyUnavailable {
                    component: manifest.id.clone(),
                    dependency: dependency.id.clone(),
                });
            }
            let operation = match self.planned_version(state, &dependency.id, steps) {
                Some(version)
                    if Version::parse(version)
                        .is_ok_and(|version| requirement.matches(&version)) =>
                {
                    continue;
                }
                Some(_) => DesiredOperation::Update,
                None => DesiredOperation::Install,
            };
            self.resolve(state, &dependency.id, operation, visiting, scheduled, steps)
                .map_err(|error| match error {
                    ManagerError::NoUpdateAvailable(_) | ManagerError::NotInstalled(_) => {
                        ManagerError::DependencyUnavailable {
                            component: manifest.id.clone(),
                            dependency: dependency.id.clone(),
                        }
                    }
                    error => error,
                })?;
        }
        Ok(())
    }

    fn ensure_no_conflict(
        &self,
        state: &ManagerState,
        manifest: &ComponentManifest,
        steps: &[PlanStep],
    ) -> Result<(), ManagerError> {
        let mut conflicts = manifest.conflicts.clone();
        conflicts.extend(
            self.manifests()
                .filter(|other| {
                    other
                        .conflicts
                        .iter()
                        .any(|conflict| conflict == &manifest.id)
                })
                .map(|other| other.id.clone()),
        );
        conflicts.sort();
        conflicts.dedup();

        for conflict in conflicts {
            if self.planned_version(state, &conflict, steps).is_some() {
                return Err(ManagerError::Conflict {
                    component: manifest.id.clone(),
                    conflict,
                });
            }
        }
        Ok(())
    }

    fn ensure_no_installed_dependents(
        &self,
        state: &ManagerState,
        component: &ComponentId,
        steps: &[PlanStep],
    ) -> Result<(), ManagerError> {
        for manifest in self.manifests() {
            if manifest.id != *component
                && manifest
                    .dependencies
                    .iter()
                    .any(|dependency| dependency.id == *component)
                && self.planned_version(state, &manifest.id, steps).is_some()
            {
                return Err(ManagerError::RequiredBy {
                    component: component.clone(),
                    dependent: manifest.id.clone(),
                });
            }
        }
        Ok(())
    }

    fn planned_version<'a>(
        &self,
        state: &'a ManagerState,
        component: &ComponentId,
        steps: &'a [PlanStep],
    ) -> Option<&'a str> {
        if let Some(step) = steps.iter().rev().find(|step| step.component == *component) {
            return step.after_version.as_deref();
        }
        state
            .component(component)
            .and_then(|record| record.installed_version.as_deref())
    }

    fn plan_step(
        &self,
        state: &ManagerState,
        manifest: &ComponentManifest,
        operation: DesiredOperation,
    ) -> PlanStep {
        let uses_artifact = matches!(
            operation,
            DesiredOperation::Install | DesiredOperation::Update
        );
        let before_version = state
            .component(&manifest.id)
            .and_then(|record| record.installed_version.clone());
        let after_version = match operation {
            DesiredOperation::Install | DesiredOperation::Update => {
                Some(manifest.version.to_string())
            }
            DesiredOperation::Restore => state
                .component(&manifest.id)
                .and_then(|record| record.restore_snapshot.as_ref())
                .and_then(|snapshot| snapshot.installed_version.clone()),
            DesiredOperation::Remove => None,
            DesiredOperation::Enable | DesiredOperation::Disable | DesiredOperation::Verify => {
                before_version.clone()
            }
        };
        let mut dependencies = manifest
            .dependencies
            .iter()
            .map(|dependency| dependency.id.clone())
            .collect::<Vec<_>>();
        dependencies.sort();
        let mut conflicts = manifest.conflicts.clone();
        conflicts.sort();
        PlanStep {
            component: manifest.id.clone(),
            operation,
            before_version,
            after_version,
            dependencies,
            conflicts,
            paths: manifest.paths.clone(),
            restart_requirement: RestartRequirement::NotDeclared,
            estimated_download_bytes: uses_artifact
                .then_some(manifest.artifact.download_size_bytes)
                .flatten(),
            required_disk_bytes: uses_artifact
                .then_some(manifest.artifact.required_disk_bytes)
                .flatten(),
            release_notes: if uses_artifact {
                manifest.release_notes.clone()
            } else {
                Vec::new()
            },
            rollback_available: matches!(
                operation,
                DesiredOperation::Install
                    | DesiredOperation::Update
                    | DesiredOperation::Enable
                    | DesiredOperation::Disable
                    | DesiredOperation::Remove
            ),
        }
    }

    fn disk_space_check(&self, steps: &[PlanStep]) -> Result<DiskSpaceCheck, ManagerError> {
        let mut required_bytes = 0_u64;
        let mut requires_artifact = false;
        for step in steps {
            if !matches!(
                step.operation,
                DesiredOperation::Install | DesiredOperation::Update
            ) {
                continue;
            }
            requires_artifact = true;
            let Some(step_required_bytes) = step.required_disk_bytes else {
                return Ok(DiskSpaceCheck::NotDeclared);
            };
            required_bytes = required_bytes
                .checked_add(step_required_bytes)
                .ok_or(ManagerError::DiskSpaceRequirementOverflow)?;
        }
        if !requires_artifact {
            return Ok(DiskSpaceCheck::NotRequired);
        }
        let Some(available_bytes) = self.profile.free_disk_bytes else {
            return Ok(DiskSpaceCheck::NotDeclared);
        };
        if available_bytes < required_bytes {
            return Err(ManagerError::InsufficientDiskSpace {
                required_bytes,
                available_bytes,
            });
        }
        Ok(DiskSpaceCheck::Sufficient {
            required_bytes,
            available_bytes,
        })
    }

    fn apply_steps(
        &self,
        state: &mut ManagerState,
        active: &ActiveOperation,
    ) -> Result<(), ManagerError> {
        for step in &active.plan.steps {
            let record = state.components.entry(step.component.clone()).or_default();
            match step.operation {
                DesiredOperation::Install | DesiredOperation::Update => {
                    record.installed_version = step.after_version.clone();
                    record.enabled = true;
                    record.health = HealthState::Degraded;
                }
                DesiredOperation::Enable => {
                    record.enabled = true;
                    record.health = HealthState::Degraded;
                }
                DesiredOperation::Disable => {
                    record.enabled = false;
                    record.health = HealthState::Healthy;
                }
                DesiredOperation::Verify => {
                    record.health = HealthState::Degraded;
                }
                DesiredOperation::Restore => {
                    let snapshot = record
                        .restore_snapshot
                        .clone()
                        .ok_or_else(|| ManagerError::NoRestoreAvailable(step.component.clone()))?;
                    record.installed_version = snapshot.installed_version;
                    record.enabled = snapshot.enabled;
                    record.health = HealthState::Degraded;
                }
                DesiredOperation::Remove => {
                    record.installed_version = None;
                    record.enabled = false;
                    record.health = HealthState::Healthy;
                }
            }
        }
        Ok(())
    }

    fn finish(
        &self,
        state: &mut ManagerState,
        active: ActiveOperation,
        outcome: MockOutcome,
    ) -> OperationProgress {
        let operation = active
            .plan
            .steps
            .last()
            .map(|step| step.operation)
            .expect("active plan is non-empty");

        if matches!(operation, DesiredOperation::Restore)
            && matches!(
                outcome,
                MockOutcome::RestorePartially | MockOutcome::RestoreRequiresManualRecovery
            )
        {
            let recovery = if outcome == MockOutcome::RestorePartially {
                RecoveryStatus::PartiallyRestored
            } else {
                RecoveryStatus::ManualRecoveryRequired
            };
            return self.fail_restore(state, active, recovery);
        }

        for step in &active.plan.steps {
            let snapshot = active
                .snapshots
                .get(&step.component)
                .cloned()
                .unwrap_or(None);
            let record = state.components.entry(step.component.clone()).or_default();
            if record.installed_version.is_some() {
                record.health = HealthState::Healthy;
            }
            match step.operation {
                DesiredOperation::Restore => {
                    record.restore_snapshot = None;
                    record.failure = None;
                    record.recovery = Some(RecoveryStatus::Restored);
                    state.record(
                        ActivityKind::RecoverySuccess,
                        Some(step.component.clone()),
                        Some(step.operation),
                        Some(OperationStage::CheckingHealth),
                        Some("mock_restore_rechecked".to_string()),
                    );
                }
                DesiredOperation::Verify => {
                    record.failure = None;
                    record.recovery = None;
                    state.record(
                        ActivityKind::Success,
                        Some(step.component.clone()),
                        Some(step.operation),
                        Some(OperationStage::CheckingHealth),
                        Some("mock_health_check_passed".to_string()),
                    );
                }
                _ => {
                    record.restore_snapshot =
                        Some(ComponentSnapshot::from_record(snapshot.as_ref()));
                    record.failure = None;
                    record.recovery = None;
                    state.record(
                        ActivityKind::Success,
                        Some(step.component.clone()),
                        Some(step.operation),
                        Some(OperationStage::CheckingHealth),
                        Some("mock_operation_finished".to_string()),
                    );
                }
            }
        }
        state.active_operation = None;
        state.bump_revision();
        OperationProgress::Finished { operation }
    }

    fn fail_restore(
        &self,
        state: &mut ManagerState,
        active: ActiveOperation,
        recovery: RecoveryStatus,
    ) -> OperationProgress {
        let step = active.plan.steps.last().expect("active plan is non-empty");
        let failure = FailureRecord {
            component: step.component.clone(),
            stage: OperationStage::CheckingHealth,
            evidence: "mock_restore_recheck_failed".to_string(),
            recovery: Some(recovery),
        };
        let record = state.components.entry(step.component.clone()).or_default();
        record.failure = Some(failure.clone());
        record.recovery = Some(recovery);
        record.health = HealthState::Failed;
        state.active_operation = None;
        state.record(
            match recovery {
                RecoveryStatus::Restored => ActivityKind::RecoverySuccess,
                RecoveryStatus::PartiallyRestored => ActivityKind::RecoveryPartial,
                RecoveryStatus::ManualRecoveryRequired => ActivityKind::ManualRecovery,
            },
            Some(step.component.clone()),
            Some(DesiredOperation::Restore),
            Some(OperationStage::CheckingHealth),
            Some(failure.evidence.clone()),
        );
        state.bump_revision();
        OperationProgress::Failed { failure }
    }

    fn fail(&self, state: &mut ManagerState, active: ActiveOperation) -> OperationProgress {
        let step = active.plan.steps.last().expect("active plan is non-empty");
        let failure = FailureRecord {
            component: step.component.clone(),
            stage: active.stage,
            evidence: failure_evidence(active.stage).to_string(),
            recovery: None,
        };
        let changes_applied = matches!(
            active.stage,
            OperationStage::ApplyingSettings | OperationStage::CheckingHealth
        );
        for planned_step in &active.plan.steps {
            let record = state
                .components
                .entry(planned_step.component.clone())
                .or_default();
            if changes_applied
                && !matches!(
                    planned_step.operation,
                    DesiredOperation::Verify | DesiredOperation::Restore
                )
            {
                let snapshot = active
                    .snapshots
                    .get(&planned_step.component)
                    .cloned()
                    .unwrap_or(None);
                record.restore_snapshot = Some(ComponentSnapshot::from_record(snapshot.as_ref()));
            }
            record.health = HealthState::Failed;
            record.failure = Some(FailureRecord {
                component: planned_step.component.clone(),
                ..failure.clone()
            });
        }
        state.active_operation = None;
        state.record(
            ActivityKind::Failure,
            Some(step.component.clone()),
            Some(step.operation),
            Some(active.stage),
            Some(failure.evidence.clone()),
        );
        state.bump_revision();
        OperationProgress::Failed { failure }
    }

    fn restore_active_snapshots(&self, state: &mut ManagerState, active: &ActiveOperation) {
        state.components = active
            .snapshots
            .iter()
            .filter_map(|(id, snapshot)| snapshot.clone().map(|snapshot| (id.clone(), snapshot)))
            .collect();
    }

    fn ensure_idle(&self, state: &ManagerState) -> Result<(), ManagerError> {
        if state.active_operation.is_some() {
            Err(ManagerError::ActiveOperation)
        } else {
            Ok(())
        }
    }

    fn manifest(&self, id: &ComponentId) -> Result<&ComponentManifest, ManagerError> {
        self.catalog
            .get(id)
            .ok_or_else(|| ManagerError::UnknownComponent(id.clone()))
    }

    fn is_compatible(&self, manifest: &ComponentManifest) -> bool {
        let profile = &self.profile;
        manifest
            .targets
            .distributions
            .iter()
            .any(|value| value.eq_ignore_ascii_case(&profile.distribution))
            && manifest
                .targets
                .releases
                .iter()
                .any(|value| value == &profile.release)
            && manifest
                .targets
                .architectures
                .iter()
                .any(|value| value.eq_ignore_ascii_case(&profile.architecture))
    }

    fn update_available(&self, record: &ComponentRecord, manifest: &ComponentManifest) -> bool {
        let Some(installed) = record.installed_version.as_deref() else {
            return false;
        };
        match Version::parse(installed) {
            Ok(installed) => installed < manifest.version,
            Err(_) => installed != manifest.version.to_string(),
        }
    }

    pub fn component_type(&self, id: &ComponentId) -> Result<ComponentType, ManagerError> {
        Ok(self.manifest(id)?.component_type.clone())
    }
}

fn failure_evidence(stage: OperationStage) -> &'static str {
    match stage {
        OperationStage::Downloading => "mock_failure_at_downloading",
        OperationStage::Installing => "mock_failure_at_installing",
        OperationStage::ApplyingSettings => "mock_failure_at_applying_settings",
        OperationStage::CheckingHealth => "mock_failure_at_checking_health",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalog() -> ComponentCatalog {
        let manifests = [
            include_str!("../../../components/manifests/better-manager.yaml"),
            include_str!("../../../components/manifests/better-monitor.yaml"),
            include_str!("../../../components/manifests/better-files-example.yaml"),
        ]
        .into_iter()
        .map(|manifest| ComponentManifest::parse_yaml(manifest).unwrap())
        .collect::<Vec<_>>();
        ComponentCatalog::from_manifests(manifests).unwrap()
    }

    fn id(value: &str) -> ComponentId {
        ComponentId::new(value).unwrap()
    }

    #[test]
    fn incompatible_profile_is_reported_before_planning() {
        let manager = Manager::new(
            catalog(),
            MockSystemProfile {
                distribution: "ubuntu".to_string(),
                release: "22.04".to_string(),
                architecture: "arm64".to_string(),
                free_disk_bytes: None,
            },
        );
        let state = ManagerState::default();
        assert_eq!(
            manager.status(&state, &id("better-files-example")).unwrap(),
            ComponentStatus::Incompatible
        );
        assert!(matches!(
            manager.plan(
                &state,
                &id("better-files-example"),
                DesiredOperation::Install
            ),
            Err(ManagerError::Incompatible { .. })
        ));
    }

    #[test]
    fn stale_reviewed_plan_cannot_start() {
        let manager = Manager::new(catalog(), MockSystemProfile::default());
        let mut state = ManagerState::default();
        let component = id("better-monitor");
        let plan = manager
            .plan(&state, &component, DesiredOperation::Install)
            .unwrap();
        state.set_installed(id("better-manager"), "0.0.1", true);
        assert!(matches!(
            manager.begin(&mut state, plan),
            Err(ManagerError::StalePlan)
        ));
    }

    #[test]
    fn lifecycle_descriptors_are_never_part_of_steps_or_activity() {
        let manager = Manager::new(catalog(), MockSystemProfile::default());
        let state = ManagerState::default();
        let plan = manager
            .plan(&state, &id("better-monitor"), DesiredOperation::Install)
            .unwrap();
        let serialized = format!("{plan:?}");
        assert!(!serialized.contains("plan-local-apt-install"));
    }

    #[test]
    fn manager_errors_expose_stable_keys_instead_of_english_prose() {
        assert_eq!(
            ManagerError::StalePlan.to_string(),
            "manager.error.stale_plan"
        );
        assert_eq!(
            StateValidationError::UnsupportedSchemaVersion(2).to_string(),
            "manager.state.unsupported_schema:2"
        );
    }
}
