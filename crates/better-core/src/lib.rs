//! Shared domain types and validation for Better OS components.

pub mod manifest;

pub use manifest::{
    Artifact, BenchmarkDefinition, ComponentCatalog, ComponentIcon, ComponentId, ComponentManifest,
    ComponentType, Dependency, Lifecycle, MAX_SUMMARY_LENGTH, ManifestError, Permission,
    RestartScope, TargetMatrix,
};
