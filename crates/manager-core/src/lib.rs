//! Shared, non-privileged component lifecycle planning and execution control.
//!
//! Manifest lifecycle descriptors remain untrusted data. This crate never
//! interprets them as shell commands, package-manager invocations, or system
//! mutation. It plans transactions and advances the lifecycle state machine;
//! whether a stage was actually carried out on the host is decided by a driver
//! in [`exec`], never here.

pub mod exec;

use better_core::{
    Artifact, ComponentCatalog, ComponentId, ComponentManifest, ComponentType, ManifestError,
    RestartScope,
};
pub use manager_platform::SystemProfile;
use manager_platform::{PlatformError, SystemCapabilities};
use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

/// Version 2 adds artifact identity to plan steps, component records, and
/// snapshots, which a real execution path needs in order to know what it is
/// installing and what it would restore.
pub const STATE_SCHEMA_VERSION: u32 = 2;

/// Whether a transaction is a deterministic simulation or a real host change.
///
/// A mock transaction never leaves this crate; a real one is carried out by a
/// driver that talks to the privileged boundary. The distinction is persisted
/// so a state file cannot be reloaded with the wrong meaning.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionMode {
    #[default]
    Mock,
    Real,
}

/// The artifact a step installs, carried far enough into execution that a
/// driver never has to re-derive it from the catalog.
///
/// `url` is absent for a restore, which reinstalls a version that is already
/// cached rather than fetching it again.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PlanArtifact {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    pub sha256: String,
    pub release_asset: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_bytes: Option<u64>,
}

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

/// Better OS is dark-first: a manager that has never been configured opens
/// dark, and the other two values are explicit user choices.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StoredTheme {
    #[default]
    Dark,
    Light,
    System,
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
    /// Older state files predate the theme choice and load as dark-first.
    #[serde(default)]
    pub theme: StoredTheme,
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
            theme: StoredTheme::Dark,
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

    /// Whether the operation needs a package artifact to act on. Enabling,
    /// disabling, verifying, and removing all work from what is already
    /// installed.
    pub fn uses_artifact(self) -> bool {
        matches!(self, Self::Install | Self::Update | Self::Restore)
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

/// How far a session must be interrupted before a planned change takes effect.
/// A manifest that declares nothing stays `NotDeclared`; the manager must not
/// downgrade an undeclared component to "no restart needed".
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RestartRequirement {
    #[default]
    NotDeclared,
    NotRequired,
    RestartApplication,
    LogOut,
    Reboot,
}

impl From<Option<RestartScope>> for RestartRequirement {
    fn from(scope: Option<RestartScope>) -> Self {
        match scope {
            None => Self::NotDeclared,
            Some(RestartScope::None) => Self::NotRequired,
            Some(RestartScope::Application) => Self::RestartApplication,
            Some(RestartScope::Logout) => Self::LogOut,
            Some(RestartScope::Reboot) => Self::Reboot,
        }
    }
}

impl RestartRequirement {
    /// Whether the user has to interrupt something before the change applies.
    pub fn interrupts_session(self) -> bool {
        matches!(self, Self::RestartApplication | Self::LogOut | Self::Reboot)
    }

    /// The requirement a whole transaction inherits, which is the widest
    /// interruption any of its steps declares.
    pub fn widest(steps: impl IntoIterator<Item = Self>) -> Self {
        steps
            .into_iter()
            .max_by_key(|requirement| match requirement {
                Self::NotRequired => 0,
                Self::NotDeclared => 1,
                Self::RestartApplication => 2,
                Self::LogOut => 3,
                Self::Reboot => 4,
            })
            .unwrap_or(Self::NotDeclared)
    }
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
    /// A stable machine key. Presentation layers own the localized wording.
    pub evidence: String,
    /// Context a driver observed, such as the tail of a failed command. It is
    /// diagnostic only: nothing branches on it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    pub recovery: Option<RecoveryStatus>,
}

/// What a component looked like before a transaction touched it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ComponentSnapshot {
    pub installed_version: Option<String>,
    pub enabled: bool,
    pub health: HealthState,
    /// The artifact that produced `installed_version`. Without it a restore has
    /// a version to name but nothing to reinstall, which a real transaction
    /// must refuse rather than pretend to satisfy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact: Option<PlanArtifact>,
}

impl ComponentSnapshot {
    fn from_record(record: Option<&ComponentRecord>) -> Self {
        match record {
            Some(record) => Self {
                installed_version: record.installed_version.clone(),
                enabled: record.enabled,
                health: record.health,
                artifact: record.installed_artifact.clone(),
            },
            None => Self {
                installed_version: None,
                enabled: false,
                health: HealthState::Healthy,
                artifact: None,
            },
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ComponentRecord {
    pub installed_version: Option<String>,
    pub enabled: bool,
    pub health: HealthState,
    /// The artifact that produced `installed_version`, when the install was
    /// recorded by a version of the manager that tracked one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub installed_artifact: Option<PlanArtifact>,
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
            installed_artifact: None,
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
    /// Third-party software this component takes over from, as declared by the
    /// manifest. Reviewers need this before approving a replacement.
    #[serde(default)]
    pub replaces: Vec<String>,
    /// Third-party software this component augments without displacing.
    #[serde(default)]
    pub enhances: Vec<String>,
    pub paths: Vec<String>,
    pub restart_requirement: RestartRequirement,
    /// The artifact this step acts on, for the operations that act on one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact: Option<PlanArtifact>,
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

    /// Whether carrying this plan out would change the host.
    pub fn execution_mode(&self) -> ExecutionMode {
        if self.dry_run {
            ExecutionMode::Mock
        } else {
            ExecutionMode::Real
        }
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

    /// The interruption the whole transaction inherits from its steps.
    pub fn restart_requirement(&self) -> RestartRequirement {
        RestartRequirement::widest(self.steps.iter().map(|step| step.restart_requirement))
    }

    /// Third-party software this transaction takes over from, deduplicated
    /// across steps.
    pub fn replaces(&self) -> Vec<String> {
        Self::collect(self.steps.iter().flat_map(|step| step.replaces.iter()))
    }

    /// Third-party software this transaction augments, deduplicated across
    /// steps.
    pub fn enhances(&self) -> Vec<String> {
        Self::collect(self.steps.iter().flat_map(|step| step.enhances.iter()))
    }

    fn collect<'a>(values: impl Iterator<Item = &'a String>) -> Vec<String> {
        values
            .cloned()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ActiveOperation {
    pub plan: TransactionPlan,
    pub stage: OperationStage,
    pub snapshots: BTreeMap<ComponentId, Option<ComponentRecord>>,
}

impl ActiveOperation {
    /// Whether this transaction is putting a component back, which is the only
    /// case where a partial or failed recovery is a meaningful outcome.
    fn restores(&self) -> bool {
        self.plan
            .steps
            .last()
            .is_some_and(|step| matches!(step.operation, DesiredOperation::Restore))
    }
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
            // A real plan is now allowed, but only if it carries what a real
            // execution needs. A step that would install something without
            // naming the artifact it installs cannot be resumed or audited, so
            // it is not a valid thing to have persisted.
            if !active.plan.dry_run {
                for step in &active.plan.steps {
                    if !step.operation.uses_artifact() {
                        continue;
                    }
                    let Some(artifact) = &step.artifact else {
                        return Err(StateValidationError::InvalidActivePlan(format!(
                            "real plan step for {} has no artifact",
                            step.component
                        )));
                    };
                    if !is_sha256(&artifact.sha256) {
                        return Err(StateValidationError::InvalidActivePlan(format!(
                            "real plan step for {} has an invalid checksum",
                            step.component
                        )));
                    }
                }
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
                installed_artifact: None,
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

fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .chars()
            .all(|character| character.is_ascii_hexdigit() && !character.is_uppercase())
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

/// A scripted result used to drive the deterministic mock lifecycle.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MockOutcome {
    Succeed,
    FailAt(OperationStage),
    RestorePartially,
    RestoreRequiresManualRecovery,
}

/// Why a stage failed.
///
/// The key is stable and machine-readable so presentation layers can localize
/// it; the detail is whatever the driver saw and is never branched on.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FailureEvidence {
    pub key: String,
    pub detail: Option<String>,
}

impl FailureEvidence {
    pub fn new(key: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            detail: None,
        }
    }

    pub fn with_detail(key: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            detail: Some(detail.into()),
        }
    }
}

/// What actually happened during a stage.
///
/// This replaces the caller-supplied [`MockOutcome`] at the lifecycle boundary.
/// A driver reports what it observed; the state machine decides what that means
/// for the transaction.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StageOutcome {
    Completed,
    Failed(FailureEvidence),
    /// A restore that put some, but not all, of the component back. Only
    /// meaningful at the health check of a restore.
    RestoredPartially,
    /// A restore that could not put the component back at all.
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
    /// A real restore has a version to name but no cached artifact to reinstall.
    /// Planning refuses rather than promising a restore that cannot happen.
    #[error("manager.error.restore_artifact_missing:{0}")]
    RestoreArtifactMissing(ComponentId),
    /// A recovery outcome arrived at a stage where it has no meaning.
    #[error("manager.error.unexpected_stage_outcome:{stage:?}")]
    UnexpectedStageOutcome { stage: OperationStage },
    /// A real transaction is past the point where cancelling could put the host
    /// back the way it was.
    #[error("manager.error.not_cancelable:{stage:?}")]
    NotCancelable { stage: OperationStage },
    /// Progress could not be persisted. Continuing would leave the durable
    /// state describing a transaction that has already moved on.
    #[error("manager.error.state_not_saved:{0}")]
    StateNotSaved(String),
}

/// The shared non-privileged planning and mock lifecycle API.
#[derive(Clone)]
pub struct Manager {
    catalog: ComponentCatalog,
    profile: SystemProfile,
}

impl Manager {
    pub fn new(catalog: ComponentCatalog, profile: SystemProfile) -> Self {
        Self { catalog, profile }
    }

    /// Builds a manager from whatever the platform reports about the host.
    /// The manager never probes the system itself; capability reporting is the
    /// platform crate's boundary.
    pub fn probe(
        catalog: ComponentCatalog,
        capabilities: &dyn SystemCapabilities,
    ) -> Result<Self, PlatformError> {
        Ok(Self::new(catalog, capabilities.profile()?))
    }

    pub fn manifests(&self) -> impl Iterator<Item = &ComponentManifest> {
        let mut manifests = self.catalog.manifests().collect::<Vec<_>>();
        manifests.sort_by(|left, right| left.id.as_str().cmp(right.id.as_str()));
        manifests.into_iter()
    }

    pub fn profile(&self) -> &SystemProfile {
        &self.profile
    }

    /// The artifact built for this host's Ubuntu release and architecture. A
    /// validated manifest declares one for every supported combination, so
    /// `None` means the host is not a target this component ships for.
    fn artifact_for_profile<'a>(&self, manifest: &'a ComponentManifest) -> Option<&'a Artifact> {
        manifest.artifacts.iter().find(|artifact| {
            artifact.release == self.profile.release
                && artifact
                    .architecture
                    .eq_ignore_ascii_case(&self.profile.architecture)
        })
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

    /// Plans one operation as a simulation.
    pub fn plan(
        &self,
        state: &ManagerState,
        id: &ComponentId,
        operation: DesiredOperation,
    ) -> Result<TransactionPlan, ManagerError> {
        self.plan_in_mode(state, id, operation, ExecutionMode::Mock)
    }

    pub fn plan_in_mode(
        &self,
        state: &ManagerState,
        id: &ComponentId,
        operation: DesiredOperation,
        mode: ExecutionMode,
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
        self.finish_plan(state, steps, vec![id.clone()], mode)
    }

    /// Plans every available update as a simulation.
    pub fn plan_all(&self, state: &ManagerState) -> Result<TransactionPlan, ManagerError> {
        self.plan_all_in_mode(state, ExecutionMode::Mock)
    }

    pub fn plan_all_in_mode(
        &self,
        state: &ManagerState,
        mode: ExecutionMode,
    ) -> Result<TransactionPlan, ManagerError> {
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
        self.finish_plan(state, steps, roots, mode)
    }

    fn finish_plan(
        &self,
        state: &ManagerState,
        steps: Vec<PlanStep>,
        roots: Vec<ComponentId>,
        mode: ExecutionMode,
    ) -> Result<TransactionPlan, ManagerError> {
        // A simulation may plan a restore against a snapshot recorded before
        // artifacts were tracked. A real one may not: without the artifact
        // there is nothing to reinstall, and saying otherwise would promise a
        // restore that cannot happen.
        if mode == ExecutionMode::Real {
            for step in &steps {
                if step.operation.uses_artifact() && step.artifact.is_none() {
                    return Err(ManagerError::RestoreArtifactMissing(step.component.clone()));
                }
            }
        }
        let disk_space = self.disk_space_check(&steps)?;
        Ok(TransactionPlan {
            state_revision: state.revision,
            steps,
            dry_run: mode == ExecutionMode::Mock,
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

    /// Advances the lifecycle by one stage, given what actually happened during
    /// it.
    ///
    /// The outcome is produced by a driver — deterministic in a simulation,
    /// observed from the host in a real transaction — so the state machine
    /// never decides for itself that a stage succeeded.
    pub fn advance(
        &self,
        state: &mut ManagerState,
        outcome: StageOutcome,
    ) -> Result<OperationProgress, ManagerError> {
        self.validate_state(state)?;
        let active = state
            .active_operation
            .clone()
            .ok_or(ManagerError::NoActiveOperation)?;

        match outcome {
            StageOutcome::Failed(evidence) => return Ok(self.fail(state, active, evidence)),
            StageOutcome::RestoredPartially | StageOutcome::RestoreRequiresManualRecovery => {
                if active.stage != OperationStage::CheckingHealth || !active.restores() {
                    return Err(ManagerError::UnexpectedStageOutcome {
                        stage: active.stage,
                    });
                }
                let recovery = if outcome == StageOutcome::RestoredPartially {
                    RecoveryStatus::PartiallyRestored
                } else {
                    RecoveryStatus::ManualRecoveryRequired
                };
                return Ok(self.fail_restore(state, active, recovery));
            }
            StageOutcome::Completed => {}
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
            OperationStage::CheckingHealth => Ok(self.finish(state, active)),
        }
    }

    /// Advances using a scripted mock result.
    ///
    /// This is the deterministic path the demo and the lifecycle suite drive.
    /// It translates a scripted intent into the same [`StageOutcome`] a real
    /// driver would report, so both paths exercise one state machine.
    pub fn advance_mock(
        &self,
        state: &mut ManagerState,
        outcome: MockOutcome,
    ) -> Result<OperationProgress, ManagerError> {
        let active = state.active_operation.as_ref();
        let stage = active.map(|active| active.stage);
        let restores = active.is_some_and(ActiveOperation::restores);

        let outcome = match outcome {
            MockOutcome::FailAt(failing) if Some(failing) == stage => {
                StageOutcome::Failed(FailureEvidence::new(failure_evidence(failing)))
            }
            MockOutcome::RestorePartially
                if restores && stage == Some(OperationStage::CheckingHealth) =>
            {
                StageOutcome::RestoredPartially
            }
            MockOutcome::RestoreRequiresManualRecovery
                if restores && stage == Some(OperationStage::CheckingHealth) =>
            {
                StageOutcome::RestoreRequiresManualRecovery
            }
            // A recovery outcome outside a restore's health check says nothing
            // about this stage, which is how the mock lifecycle has always
            // treated it.
            _ => StageOutcome::Completed,
        };
        self.advance(state, outcome)
    }

    /// Abandons the active transaction and puts the component set back the way
    /// it was.
    ///
    /// A real transaction can only be cancelled while it is still downloading.
    /// Once the privileged boundary has begun applying packages, restoring the
    /// recorded snapshot would claim the host was put back without anything
    /// having put it back.
    pub fn cancel(&self, state: &mut ManagerState) -> Result<(), ManagerError> {
        self.validate_state(state)?;
        if let Some(active) = &state.active_operation
            && active.plan.execution_mode() == ExecutionMode::Real
            && active.stage != OperationStage::Downloading
        {
            return Err(ManagerError::NotCancelable {
                stage: active.stage,
            });
        }
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
        let artifact = self.artifact_for_profile(manifest);
        let plan_artifact = match operation {
            DesiredOperation::Install | DesiredOperation::Update => {
                artifact.map(|artifact| PlanArtifact {
                    url: Some(artifact.url.clone()),
                    sha256: artifact.sha256.clone(),
                    release_asset: artifact.release_asset.clone(),
                    expected_bytes: artifact.download_size_bytes,
                })
            }
            // A restore reinstalls what was already fetched once, so it names
            // the recorded artifact rather than a URL to fetch again.
            DesiredOperation::Restore => state
                .component(&manifest.id)
                .and_then(|record| record.restore_snapshot.as_ref())
                .and_then(|snapshot| snapshot.artifact.clone())
                .map(|artifact| PlanArtifact {
                    url: None,
                    ..artifact
                }),
            _ => None,
        };
        PlanStep {
            component: manifest.id.clone(),
            operation,
            before_version,
            after_version,
            dependencies,
            conflicts,
            replaces: if operation.mutates_component() {
                manifest.replaces.clone()
            } else {
                Vec::new()
            },
            enhances: if operation.mutates_component() {
                manifest.enhances.clone()
            } else {
                Vec::new()
            },
            paths: manifest.paths.clone(),
            restart_requirement: if operation.mutates_component() {
                RestartRequirement::from(manifest.restart)
            } else {
                RestartRequirement::NotRequired
            },
            artifact: plan_artifact,
            estimated_download_bytes: uses_artifact
                .then(|| artifact.and_then(|artifact| artifact.download_size_bytes))
                .flatten(),
            required_disk_bytes: uses_artifact
                .then(|| artifact.and_then(|artifact| artifact.required_disk_bytes))
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
                    record.installed_artifact = step.artifact.clone();
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
                    record.installed_artifact = snapshot.artifact;
                    record.enabled = snapshot.enabled;
                    record.health = HealthState::Degraded;
                }
                DesiredOperation::Remove => {
                    record.installed_version = None;
                    record.installed_artifact = None;
                    record.enabled = false;
                    record.health = HealthState::Healthy;
                }
            }
        }
        Ok(())
    }

    fn finish(&self, state: &mut ManagerState, active: ActiveOperation) -> OperationProgress {
        let operation = active
            .plan
            .steps
            .last()
            .map(|step| step.operation)
            .expect("active plan is non-empty");

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
            detail: None,
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

    fn fail(
        &self,
        state: &mut ManagerState,
        active: ActiveOperation,
        evidence: FailureEvidence,
    ) -> OperationProgress {
        let step = active.plan.steps.last().expect("active plan is non-empty");
        let failure = FailureRecord {
            component: step.component.clone(),
            stage: active.stage,
            evidence: evidence.key,
            detail: evidence.detail,
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
            SystemProfile {
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
        let manager = Manager::new(catalog(), SystemProfile::default());
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
        let manager = Manager::new(catalog(), SystemProfile::default());
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
