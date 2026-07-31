use semver::Version;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fmt;
use thiserror::Error;

pub const CURRENT_SCHEMA_VERSION: u32 = 2;

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ComponentId(String);

impl ComponentId {
    pub fn new(value: impl Into<String>) -> Result<Self, ManifestError> {
        let value = value.into();
        if value.is_empty()
            || value.len() > 64
            || !value.chars().all(|character| {
                character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
            })
            || value.starts_with('-')
            || value.ends_with('-')
        {
            return Err(ManifestError::InvalidComponentId(value));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ComponentId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ComponentType {
    Replacement,
    Enhancement,
    Diagnostic,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ComponentManifest {
    pub schema_version: u32,
    pub id: ComponentId,
    pub display_name: String,
    pub component_type: ComponentType,
    pub version: Version,
    pub targets: TargetMatrix,
    #[serde(default)]
    pub replaces: Vec<String>,
    #[serde(default)]
    pub enhances: Vec<String>,
    #[serde(default)]
    pub dependencies: Vec<Dependency>,
    #[serde(default)]
    pub conflicts: Vec<ComponentId>,
    pub artifacts: Vec<Artifact>,
    pub lifecycle: Lifecycle,
    #[serde(default)]
    pub health_checks: Vec<String>,
    #[serde(default)]
    pub benchmarks: Vec<BenchmarkDefinition>,
    #[serde(default)]
    pub permissions: Vec<Permission>,
    #[serde(default)]
    pub paths: Vec<String>,
}

impl ComponentManifest {
    pub fn parse_yaml(input: &str) -> Result<Self, ManifestError> {
        let manifest: Self = serde_yaml::from_str(input)?;
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn validate(&self) -> Result<(), ManifestError> {
        ComponentId::new(self.id.0.clone())?;
        if self.schema_version != CURRENT_SCHEMA_VERSION {
            return Err(ManifestError::UnsupportedSchemaVersion(self.schema_version));
        }
        if self.display_name.trim().is_empty() {
            return Err(ManifestError::MissingField("display_name"));
        }
        if self.targets.distributions.is_empty()
            || self.targets.releases.is_empty()
            || self.targets.architectures.is_empty()
        {
            return Err(ManifestError::MissingField("targets"));
        }
        if self.artifacts.is_empty() {
            return Err(ManifestError::InvalidArtifact);
        }

        let mut variants = HashSet::new();
        for artifact in &self.artifacts {
            if !self.targets.releases.contains(&artifact.release)
                || !self.targets.architectures.contains(&artifact.architecture)
            {
                return Err(ManifestError::UnsupportedArtifactVariant {
                    release: artifact.release.clone(),
                    architecture: artifact.architecture.clone(),
                });
            }
            if artifact.url.trim().is_empty()
                || !artifact.url.starts_with("https://")
                || artifact.sha256.len() != 64
                || !artifact
                    .sha256
                    .chars()
                    .all(|character| character.is_ascii_hexdigit())
                || artifact.release_asset.trim().is_empty()
                || artifact
                    .release_asset
                    .chars()
                    .any(|character| character == '/' || character == '\\')
            {
                return Err(ManifestError::InvalidArtifact);
            }
            let expected_asset = format!(
                "{}_{}_ubuntu-{}_{}.deb",
                self.id, self.version, artifact.release, artifact.architecture
            );
            if artifact.release_asset != expected_asset
                || !artifact.url.ends_with(&artifact.release_asset)
            {
                return Err(ManifestError::InvalidArtifact);
            }
            if !variants.insert((artifact.release.clone(), artifact.architecture.clone())) {
                return Err(ManifestError::DuplicateArtifactVariant {
                    release: artifact.release.clone(),
                    architecture: artifact.architecture.clone(),
                });
            }
        }

        for release in &self.targets.releases {
            for architecture in &self.targets.architectures {
                if !variants.contains(&(release.clone(), architecture.clone())) {
                    return Err(ManifestError::MissingArtifactVariant {
                        release: release.clone(),
                        architecture: architecture.clone(),
                    });
                }
            }
        }
        if self.lifecycle.install.trim().is_empty()
            || self.lifecycle.enable.trim().is_empty()
            || self.lifecycle.disable.trim().is_empty()
            || self.lifecycle.remove.trim().is_empty()
            || self.lifecycle.rollback.trim().is_empty()
        {
            return Err(ManifestError::MissingField("lifecycle"));
        }
        if self.conflicts.iter().any(|id| id == &self.id) {
            return Err(ManifestError::SelfConflict(self.id.clone()));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TargetMatrix {
    pub distributions: Vec<String>,
    pub releases: Vec<String>,
    pub architectures: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Dependency {
    pub id: ComponentId,
    pub version: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Artifact {
    pub release: String,
    pub architecture: String,
    pub url: String,
    pub sha256: String,
    pub release_asset: String,
    #[serde(default)]
    pub signature: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Lifecycle {
    pub install: String,
    pub enable: String,
    pub disable: String,
    pub remove: String,
    pub rollback: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BenchmarkDefinition {
    pub name: String,
    pub workload: String,
    pub metric: String,
    pub minimum_improvement_percent: f64,
    pub maximum_regression_percent: f64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Permission {
    pub name: String,
    pub reason: String,
}

#[derive(Debug, Error)]
pub enum ManifestError {
    #[error("failed to parse manifest: {0}")]
    Parse(#[from] serde_yaml::Error),
    #[error("component id is invalid: {0}")]
    InvalidComponentId(String),
    #[error("manifest schema version {0} is unsupported")]
    UnsupportedSchemaVersion(u32),
    #[error("required field is missing or empty: {0}")]
    MissingField(&'static str),
    #[error("artifact URL or SHA-256 checksum is invalid")]
    InvalidArtifact,
    #[error("artifact variant {release}/{architecture} is not declared by targets")]
    UnsupportedArtifactVariant {
        release: String,
        architecture: String,
    },
    #[error("artifact variant {release}/{architecture} is declared more than once")]
    DuplicateArtifactVariant {
        release: String,
        architecture: String,
    },
    #[error("artifact variant {release}/{architecture} is missing")]
    MissingArtifactVariant {
        release: String,
        architecture: String,
    },
    #[error("component {0} conflicts with itself")]
    SelfConflict(ComponentId),
    #[error("duplicate component id: {0}")]
    DuplicateComponent(ComponentId),
    #[error("dependency {dependency} required by {component} is missing")]
    MissingDependency {
        component: ComponentId,
        dependency: ComponentId,
    },
    #[error("dependency cycle detected: {0:?}")]
    DependencyCycle(Vec<ComponentId>),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ComponentCatalog {
    manifests: HashMap<ComponentId, ComponentManifest>,
}

impl ComponentCatalog {
    pub fn from_manifests(
        manifests: impl IntoIterator<Item = ComponentManifest>,
    ) -> Result<Self, ManifestError> {
        let mut catalog = Self::default();
        for manifest in manifests {
            manifest.validate()?;
            if catalog.manifests.contains_key(&manifest.id) {
                return Err(ManifestError::DuplicateComponent(manifest.id.clone()));
            }
            catalog.manifests.insert(manifest.id.clone(), manifest);
        }
        catalog.validate_graph()?;
        Ok(catalog)
    }

    pub fn manifests(&self) -> impl Iterator<Item = &ComponentManifest> {
        self.manifests.values()
    }

    pub fn get(&self, id: &ComponentId) -> Option<&ComponentManifest> {
        self.manifests.get(id)
    }

    fn validate_graph(&self) -> Result<(), ManifestError> {
        for manifest in self.manifests.values() {
            for dependency in &manifest.dependencies {
                if !self.manifests.contains_key(&dependency.id) {
                    return Err(ManifestError::MissingDependency {
                        component: manifest.id.clone(),
                        dependency: dependency.id.clone(),
                    });
                }
            }
        }

        let mut visiting = HashSet::new();
        let mut visited = HashSet::new();
        for id in self.manifests.keys() {
            let mut path = Vec::new();
            self.visit(id, &mut visiting, &mut visited, &mut path)?;
        }
        Ok(())
    }

    fn visit(
        &self,
        id: &ComponentId,
        visiting: &mut HashSet<ComponentId>,
        visited: &mut HashSet<ComponentId>,
        path: &mut Vec<ComponentId>,
    ) -> Result<(), ManifestError> {
        if visited.contains(id) {
            return Ok(());
        }
        if !visiting.insert(id.clone()) {
            let cycle_start = path.iter().position(|item| item == id).unwrap_or(0);
            return Err(ManifestError::DependencyCycle(path[cycle_start..].to_vec()));
        }
        path.push(id.clone());
        if let Some(manifest) = self.manifests.get(id) {
            for dependency in &manifest.dependencies {
                self.visit(&dependency.id, visiting, visited, path)?;
            }
        }
        path.pop();
        visiting.remove(id);
        visited.insert(id.clone());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn yaml(id: &str, dependency: Option<&str>, conflict: Option<&str>) -> String {
        let dependency = dependency
            .map(|value| format!("\n  - id: {value}\n    version: '>=1.0.0'"))
            .unwrap_or_default();
        let conflict = conflict
            .map(|value| format!("\n  - {value}"))
            .unwrap_or_default();
        format!(
            "schema_version: 2\nid: {id}\ndisplay_name: {id}\ncomponent_type: diagnostic\nversion: 1.0.0\ntargets:\n  distributions: [ubuntu]\n  releases: [24.04]\n  architectures: [amd64]\nartifacts:\n  - release: \"24.04\"\n    architecture: amd64\n    url: https://example.com/{id}_1.0.0_ubuntu-24.04_amd64.deb\n    sha256: {hash}\n    release_asset: {id}_1.0.0_ubuntu-24.04_amd64.deb\nlifecycle:\n  install: plan-install\n  enable: plan-enable\n  disable: plan-disable\n  remove: plan-remove\n  rollback: plan-rollback\ndependencies:{dependency}\nconflicts:{conflict}\n",
            hash = "a".repeat(64),
        )
    }

    #[test]
    fn parses_valid_manifest() {
        let manifest = ComponentManifest::parse_yaml(&yaml("better-monitor", None, None)).unwrap();
        assert_eq!(manifest.id.as_str(), "better-monitor");
        assert_eq!(manifest.artifacts[0].release, "24.04");
        assert_eq!(manifest.artifacts[0].architecture, "amd64");
    }

    #[test]
    fn rejects_missing_required_fields() {
        let input = yaml("better-monitor", None, None)
            .replace("display_name: better-monitor", "display_name: ''");
        assert!(matches!(
            ComponentManifest::parse_yaml(&input),
            Err(ManifestError::MissingField("display_name"))
        ));
    }

    #[test]
    fn rejects_unsupported_schema_version() {
        let input =
            yaml("better-monitor", None, None).replace("schema_version: 2", "schema_version: 3");
        assert!(matches!(
            ComponentManifest::parse_yaml(&input),
            Err(ManifestError::UnsupportedSchemaVersion(3))
        ));
    }

    #[test]
    fn rejects_missing_artifact_variant() {
        let input = yaml("better-monitor", None, None)
            .replace("releases: [24.04]", "releases: [22.04, 24.04]");
        assert!(matches!(
            ComponentManifest::parse_yaml(&input),
            Err(ManifestError::MissingArtifactVariant { .. })
        ));
    }

    #[test]
    fn rejects_duplicate_artifact_variant() {
        let input = yaml("better-monitor", None, None).replace(
            "lifecycle:",
            "  - release: \"24.04\"\n    architecture: amd64\n    url: https://example.com/better-monitor_1.0.0_ubuntu-24.04_amd64.deb\n    sha256: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n    release_asset: better-monitor_1.0.0_ubuntu-24.04_amd64.deb\nlifecycle:",
        );
        assert!(matches!(
            ComponentManifest::parse_yaml(&input),
            Err(ManifestError::DuplicateArtifactVariant { .. })
        ));
    }

    #[test]
    fn rejects_artifact_variant_outside_target_matrix() {
        let input =
            yaml("better-monitor", None, None).replace("release: \"24.04\"", "release: \"22.04\"");
        assert!(matches!(
            ComponentManifest::parse_yaml(&input),
            Err(ManifestError::UnsupportedArtifactVariant { .. })
        ));
    }

    #[test]
    fn rejects_non_hex_artifact_checksums() {
        let input = yaml("better-monitor", None, None).replace(&"a".repeat(64), &"g".repeat(64));
        assert!(matches!(
            ComponentManifest::parse_yaml(&input),
            Err(ManifestError::InvalidArtifact)
        ));
    }

    #[test]
    fn rejects_mismatched_release_asset_name() {
        let input = yaml("better-monitor", None, None).replace(
            "release_asset: better-monitor_1.0.0_ubuntu-24.04_amd64.deb",
            "release_asset: wrong-name.deb",
        );
        assert!(matches!(
            ComponentManifest::parse_yaml(&input),
            Err(ManifestError::InvalidArtifact)
        ));
    }

    #[test]
    fn rejects_invalid_component_ids_from_deserialization() {
        let input = yaml("Better Monitor", None, None);
        assert!(matches!(
            ComponentManifest::parse_yaml(&input),
            Err(ManifestError::InvalidComponentId(_))
        ));
    }

    #[test]
    fn rejects_dependency_cycles() {
        let first = ComponentManifest::parse_yaml(&yaml("first", Some("second"), None)).unwrap();
        let second = ComponentManifest::parse_yaml(&yaml("second", Some("first"), None)).unwrap();
        assert!(matches!(
            ComponentCatalog::from_manifests([first, second]),
            Err(ManifestError::DependencyCycle(_))
        ));
    }

    #[test]
    fn rejects_conflicts_with_a_missing_dependency() {
        let first =
            ComponentManifest::parse_yaml(&yaml("first", Some("missing"), Some("second"))).unwrap();
        let second = ComponentManifest::parse_yaml(&yaml("second", None, None)).unwrap();
        assert!(matches!(
            ComponentCatalog::from_manifests([first, second]),
            Err(ManifestError::MissingDependency { .. })
        ));
    }
}
