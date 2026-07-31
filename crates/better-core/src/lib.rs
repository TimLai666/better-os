//! Shared domain types and validation for Better OS components.

pub mod manifest;

pub use manifest::{
    Artifact, BenchmarkDefinition, ComponentCatalog, ComponentId, ComponentManifest, ComponentType,
    Dependency, Lifecycle, ManifestError, Permission, TargetMatrix,
};
