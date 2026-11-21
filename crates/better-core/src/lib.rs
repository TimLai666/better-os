//! Shared domain types and validation for Better OS components.

pub mod app_unit;
pub mod defaults;
pub mod desktop;
pub mod host;
pub mod local_time;
pub mod manifest;
pub mod package_health;
pub mod private_dir;
pub mod self_update;
pub mod test_guard;

pub use defaults::{
    AdapterId, DconfDatabase, DefaultIntegration, DefaultsValue, HealthPrerequisite,
    IntegrationExclusivity, IntegrationId, IntegrationKind, IntegrationTarget, KeyObservation,
    MAX_INTEGRATION_ID_LENGTH, MAX_TARGET_KEY_LENGTH, ObservedValue, RequiredPrivilege,
    RestorePolicy, SessionEffect,
};
pub use desktop::{CurrentDesktop, UNKNOWN_DESKTOP};
pub use host::{
    SUPPORTED_UBUNTU_RELEASES, describe_os_release, os_release_field, resolve_distribution_id,
    resolve_ubuntu_release,
};
pub use manifest::{
    Artifact, BenchmarkDefinition, ComponentCatalog, ComponentIcon, ComponentId, ComponentManifest,
    ComponentType, Dependency, HardwareSupport, Lifecycle, MAX_SUMMARY_LENGTH, ManifestError,
    Permission, RestartScope, TargetMatrix,
};
