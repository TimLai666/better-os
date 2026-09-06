//! Shared domain types and validation for Better OS components.

pub mod defaults;
pub mod host;
pub mod manifest;

pub use defaults::{
    AdapterId, DefaultIntegration, DefaultsValue, HealthPrerequisite, IntegrationExclusivity,
    IntegrationId, IntegrationKind, IntegrationTarget, MAX_INTEGRATION_ID_LENGTH,
    MAX_TARGET_KEY_LENGTH, ObservedValue, RequiredPrivilege, RestorePolicy, SessionEffect,
};
pub use host::{
    SUPPORTED_UBUNTU_RELEASES, describe_os_release, os_release_field, resolve_distribution_id,
    resolve_ubuntu_release,
};
pub use manifest::{
    Artifact, BenchmarkDefinition, ComponentCatalog, ComponentIcon, ComponentId, ComponentManifest,
    ComponentType, Dependency, Lifecycle, MAX_SUMMARY_LENGTH, ManifestError, Permission,
    RestartScope, TargetMatrix,
};
