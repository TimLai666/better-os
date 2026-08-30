//! Shared domain types and validation for Better OS components.

pub mod defaults;
pub mod manifest;

pub use defaults::{
    AdapterId, DefaultIntegration, DefaultsValue, HealthPrerequisite, IntegrationExclusivity,
    IntegrationId, IntegrationKind, IntegrationTarget, MAX_INTEGRATION_ID_LENGTH,
    MAX_TARGET_KEY_LENGTH, ObservedValue, RequiredPrivilege, RestorePolicy, SessionEffect,
};
pub use manifest::{
    Artifact, BenchmarkDefinition, ComponentCatalog, ComponentIcon, ComponentId, ComponentManifest,
    ComponentType, Dependency, Lifecycle, MAX_SUMMARY_LENGTH, ManifestError, Permission,
    RestartScope, TargetMatrix,
};
