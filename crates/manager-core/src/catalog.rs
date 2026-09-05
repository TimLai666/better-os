//! Where the component catalog comes from, and how honest it is.
//!
//! A published binary embeds the manifests as they stood when it was built, and
//! a manifest can only record a real artifact checksum once its own release is
//! public. So the catalog compiled into release N always describes release
//! N-1's packages. This module makes the catalog refreshable instead: the
//! manifests on the repository's `main` branch are fetched, validated as
//! untrusted input exactly as the built-in ones are, and cached on disk so an
//! offline restart keeps the last good answer.
//!
//! Three properties are load-bearing and are asserted rather than described.
//! A fetched manifest that fails validation is rejected on its own and never
//! replaces a valid one. A fetched manifest older than the one already held is
//! flagged rather than adopted. And a refresh that produced nothing leaves the
//! previous catalog in place with a state that says so, because a catalog which
//! quietly went stale is worse than one that admits it.

use std::collections::BTreeMap;
use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

use better_core::{ComponentCatalog, ComponentId, ComponentManifest, ManifestError};
use manager_platform::catalog_fetch::{FetchError, ManifestFetcher};
use semver::Version;
use serde::{Deserialize, Serialize};

/// The manifest files the catalog is made of, with the copy this binary was
/// built from. The file name is also the identity check: a document served as
/// `better-monitor.yaml` that declares another component is refused, so one
/// replaced file cannot introduce a component under a name nobody asked for.
const BUILT_IN: [(&str, &str); 7] = [
    (
        "better-manager.yaml",
        include_str!("../../../components/manifests/better-manager.yaml"),
    ),
    (
        "better-monitor.yaml",
        include_str!("../../../components/manifests/better-monitor.yaml"),
    ),
    (
        "better-launcher.yaml",
        include_str!("../../../components/manifests/better-launcher.yaml"),
    ),
    (
        "better-files.yaml",
        include_str!("../../../components/manifests/better-files.yaml"),
    ),
    (
        "better-touchpad.yaml",
        include_str!("../../../components/manifests/better-touchpad.yaml"),
    ),
    (
        "better-awake.yaml",
        include_str!("../../../components/manifests/better-awake.yaml"),
    ),
    (
        "better-storage.yaml",
        include_str!("../../../components/manifests/better-storage.yaml"),
    ),
];

/// Version 1 is the first on-disk catalog cache. A newer version is left alone
/// rather than reset, the same rule the manager state file follows.
pub const CATALOG_CACHE_SCHEMA_VERSION: u32 = 1;

/// The file names a refresh asks for, in catalog order.
pub fn catalog_files() -> Vec<&'static str> {
    BUILT_IN.iter().map(|(file, _)| *file).collect()
}

/// The manifests compiled into this binary.
///
/// This panics on an invalid document on purpose: the manifests are part of the
/// source tree and a build that embedded a broken one is broken, not degraded.
/// A test in this module holds that line.
pub fn built_in_manifests() -> Vec<ComponentManifest> {
    BUILT_IN
        .iter()
        .map(|(file, document)| {
            ComponentManifest::parse_yaml(document)
                .unwrap_or_else(|error| panic!("built-in manifest {file} is invalid: {error}"))
        })
        .collect()
}

/// The compiled-in catalog: the offline fallback, and the starting point of
/// every refresh.
pub fn built_in_catalog() -> ComponentCatalog {
    ComponentCatalog::from_manifests(built_in_manifests())
        .expect("the built-in catalog must assemble")
}

/// Where the catalog on screen came from.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CatalogSource {
    /// Compiled into this binary.
    #[default]
    BuiltIn,
    /// Read from the on-disk cache a previous refresh wrote.
    Cache,
    /// Fetched from the published catalog in this session.
    Remote,
}

impl fmt::Display for CatalogSource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::BuiltIn => "built-in",
            Self::Cache => "cache",
            Self::Remote => "remote",
        })
    }
}

/// Why the catalog may not describe the newest published release.
///
/// Every variant is a visible state, not a log line. There is no variant for
/// "probably fine": a catalog is either known current for this session or it
/// says which way it is behind.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CatalogDegradation {
    /// Nothing has been fetched yet, so this is the catalog the binary was
    /// built with and it cannot verify its own release's packages.
    NeverRefreshed,
    /// A refresh was attempted and produced nothing; an earlier cached catalog
    /// is what is on screen.
    RefreshFailedUsingCache,
    /// A refresh was attempted and produced nothing, and there is no cache, so
    /// the compiled-in catalog is what is on screen.
    RefreshFailedUsingBuiltIn,
    /// Some manifests were refreshed and some were not. The rest are whatever
    /// was already held.
    PartiallyRefreshed,
}

impl fmt::Display for CatalogDegradation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::NeverRefreshed => "catalog.degraded.never_refreshed",
            Self::RefreshFailedUsingCache => "catalog.degraded.refresh_failed_using_cache",
            Self::RefreshFailedUsingBuiltIn => "catalog.degraded.refresh_failed_using_built_in",
            Self::PartiallyRefreshed => "catalog.degraded.partially_refreshed",
        })
    }
}

/// Why one fetched manifest was not adopted.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RejectionReason {
    /// It could not be fetched at all.
    Unreachable,
    /// It was fetched and is not a valid manifest. The detail is the validator's
    /// own message.
    Invalid(String),
    /// It is a valid manifest for a different component than the file names.
    IdMismatch { declared: ComponentId },
    /// It is older than what is already held for the same component.
    Downgrade { held: Version, offered: Version },
    /// It is valid on its own and the catalog it would produce is not — a
    /// missing dependency, a cycle, or a duplicate. The whole refresh is
    /// rejected in that case, because a catalog is only meaningful whole.
    CatalogGraph(String),
}

impl fmt::Display for RejectionReason {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreachable => formatter.write_str("catalog.rejected.unreachable"),
            Self::Invalid(detail) => write!(formatter, "catalog.rejected.invalid:{detail}"),
            Self::IdMismatch { declared } => {
                write!(formatter, "catalog.rejected.id_mismatch:{declared}")
            }
            Self::Downgrade { held, offered } => {
                write!(formatter, "catalog.rejected.downgrade:{held}>{offered}")
            }
            Self::CatalogGraph(detail) => {
                write!(formatter, "catalog.rejected.catalog_graph:{detail}")
            }
        }
    }
}

/// One manifest that was not adopted, and why.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ManifestRejection {
    pub file: String,
    pub reason: RejectionReason,
}

impl fmt::Display for ManifestRejection {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.file, self.reason)
    }
}

/// What a refresh wrote to disk, and what a later start reads back.
///
/// The manifests are stored as they were validated rather than as fetched text,
/// so a reader does not need the YAML parser's behaviour to match the writer's.
/// They are validated again on read regardless: a cache file is a file on disk
/// and is no more trusted than a fetched document.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CachedCatalog {
    pub schema_version: u32,
    /// The base URL this was fetched from, so a cache written against one
    /// source is recognizable when the source changes.
    pub source_url: String,
    /// When the fetch happened, in seconds since the Unix epoch.
    pub fetched_at_unix_seconds: u64,
    pub manifests: Vec<ComponentManifest>,
}

impl CachedCatalog {
    /// Rebuilds the catalog this cache describes, validating every manifest and
    /// the graph they form. An unusable cache is an error, never a partial
    /// catalog.
    pub fn catalog(&self) -> Result<ComponentCatalog, ManifestError> {
        ComponentCatalog::from_manifests(self.manifests.clone())
    }
}

/// The catalog in force and everything a user would need to judge it.
#[derive(Clone, Debug, PartialEq)]
pub struct CatalogStatus {
    pub source: CatalogSource,
    pub source_url: Option<String>,
    pub fetched_at_unix_seconds: Option<u64>,
    /// `None` means this catalog was fetched in full during this session.
    pub degraded: Option<CatalogDegradation>,
    pub rejections: Vec<ManifestRejection>,
}

impl CatalogStatus {
    /// The status of a binary that has never refreshed and has no cache.
    pub fn built_in() -> Self {
        Self {
            source: CatalogSource::BuiltIn,
            source_url: None,
            fetched_at_unix_seconds: None,
            degraded: Some(CatalogDegradation::NeverRefreshed),
            rejections: Vec::new(),
        }
    }

    /// The status of a catalog restored from a cache a previous session wrote.
    /// It is degraded on purpose: nothing has been checked against the
    /// published catalog in this session, so how old it is, is all that can be
    /// said about it.
    pub fn from_cache(cache: &CachedCatalog) -> Self {
        Self {
            source: CatalogSource::Cache,
            source_url: Some(cache.source_url.clone()),
            fetched_at_unix_seconds: Some(cache.fetched_at_unix_seconds),
            degraded: None,
            rejections: Vec::new(),
        }
    }

    pub fn is_degraded(&self) -> bool {
        self.degraded.is_some()
    }
}

/// A refresh attempt's result: the catalog to use, what to cache, and what to
/// say about it.
#[derive(Clone, Debug)]
pub struct RefreshOutcome {
    pub catalog: ComponentCatalog,
    /// The cache to write. `None` when nothing was adopted, so a failed refresh
    /// never rewrites a good cache with a worse one or restamps its fetch time.
    pub cache: Option<CachedCatalog>,
    pub status: CatalogStatus,
    /// How many manifests came from the source in this refresh.
    pub accepted: usize,
}

/// Seconds since the Unix epoch, or zero on a clock before it.
pub fn now_unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or_default()
}

/// Assembles the catalog a session starts with: the cache if there is a usable
/// one, and the compiled-in manifests otherwise.
///
/// A cache that fails validation is not repaired and not partially used. It is
/// ignored, and the built-in catalog is reported with its own state, so a
/// tampered or truncated cache degrades to the one catalog whose contents are
/// part of the binary.
pub fn catalog_at_start(cache: Option<CachedCatalog>) -> (ComponentCatalog, CatalogStatus) {
    if let Some(cache) = cache {
        if cache.schema_version == CATALOG_CACHE_SCHEMA_VERSION {
            if let Ok(catalog) = cache.catalog() {
                let status = CatalogStatus::from_cache(&cache);
                return (catalog, status);
            }
        }
    }
    (built_in_catalog(), CatalogStatus::built_in())
}

/// Fetches, validates, and adopts the published manifests.
///
/// `base` is what is already held — the cache or the built-in manifests — and
/// is both the fallback for anything that could not be adopted and the
/// comparison the downgrade guard uses.
pub fn refresh(
    fetcher: &dyn ManifestFetcher,
    base: &ComponentCatalog,
    base_status: &CatalogStatus,
    now_unix_seconds: u64,
) -> RefreshOutcome {
    let mut held: BTreeMap<ComponentId, ComponentManifest> = base
        .manifests()
        .map(|manifest| (manifest.id.clone(), manifest.clone()))
        .collect();
    let mut rejections = Vec::new();
    let mut accepted = 0_usize;

    for file in catalog_files() {
        match evaluate(fetcher, file, &held) {
            Ok(manifest) => {
                held.insert(manifest.id.clone(), manifest);
                accepted += 1;
            }
            Err(reason) => rejections.push(ManifestRejection {
                file: file.to_string(),
                reason,
            }),
        }
    }

    let candidate = ComponentCatalog::from_manifests(held.values().cloned());
    let catalog = match candidate {
        Ok(catalog) if accepted > 0 => catalog,
        // Nothing was adopted, or the adopted set does not form a catalog. In
        // both cases what was already in force stays in force.
        Ok(_) => {
            return failed(base, base_status, rejections);
        }
        Err(error) => {
            rejections.push(ManifestRejection {
                file: "catalog".to_string(),
                reason: RejectionReason::CatalogGraph(error.to_string()),
            });
            return failed(base, base_status, rejections);
        }
    };

    let degraded = if rejections.is_empty() {
        None
    } else {
        Some(CatalogDegradation::PartiallyRefreshed)
    };
    let source_url = fetcher.source_url();
    let cache = CachedCatalog {
        schema_version: CATALOG_CACHE_SCHEMA_VERSION,
        source_url: source_url.clone(),
        fetched_at_unix_seconds: now_unix_seconds,
        manifests: catalog.manifests().cloned().collect(),
    };

    RefreshOutcome {
        catalog,
        cache: Some(cache),
        status: CatalogStatus {
            source: CatalogSource::Remote,
            source_url: Some(source_url),
            fetched_at_unix_seconds: Some(now_unix_seconds),
            degraded,
            rejections,
        },
        accepted,
    }
}

/// The result of a refresh that adopted nothing: the previous catalog, kept,
/// with a state that says a refresh was tried and did not land.
fn failed(
    base: &ComponentCatalog,
    base_status: &CatalogStatus,
    rejections: Vec<ManifestRejection>,
) -> RefreshOutcome {
    let degraded = match base_status.source {
        CatalogSource::BuiltIn => CatalogDegradation::RefreshFailedUsingBuiltIn,
        CatalogSource::Cache | CatalogSource::Remote => CatalogDegradation::RefreshFailedUsingCache,
    };
    RefreshOutcome {
        catalog: base.clone(),
        cache: None,
        status: CatalogStatus {
            source: base_status.source,
            source_url: base_status.source_url.clone(),
            fetched_at_unix_seconds: base_status.fetched_at_unix_seconds,
            degraded: Some(degraded),
            rejections,
        },
        accepted: 0,
    }
}

/// Fetches one file and decides whether it may replace what is held.
fn evaluate(
    fetcher: &dyn ManifestFetcher,
    file: &str,
    held: &BTreeMap<ComponentId, ComponentManifest>,
) -> Result<ComponentManifest, RejectionReason> {
    let document = fetcher.fetch(file).map_err(|error| match error {
        FetchError::Unreachable { .. } | FetchError::InsecureSource => RejectionReason::Unreachable,
        FetchError::Unreadable { .. } => RejectionReason::Invalid(error.to_string()),
    })?;

    // The full existing validation, not a lighter one for remote documents.
    let manifest = ComponentManifest::parse_yaml(&document)
        .map_err(|error| RejectionReason::Invalid(error.to_string()))?;

    let expected_id = file.trim_end_matches(".yaml");
    if manifest.id.as_str() != expected_id {
        return Err(RejectionReason::IdMismatch {
            declared: manifest.id.clone(),
        });
    }

    if let Some(current) = held.get(&manifest.id) {
        if manifest.version < current.version {
            return Err(RejectionReason::Downgrade {
                held: current.version.clone(),
                offered: manifest.version.clone(),
            });
        }
    }

    Ok(manifest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use manager_platform::catalog_fetch::StaticManifestFetcher;

    /// A manifest for a component that exists, at a version we choose, so the
    /// downgrade guard and the validation path can be driven without depending
    /// on what the shipped manifests happen to say today.
    fn document(id: &str, version: &str) -> String {
        format!(
            "schema_version: 2\n\
             id: {id}\n\
             display_name: {id}\n\
             component_type: enhancement\n\
             version: {version}\n\
             targets:\n  \
               distributions: [ubuntu]\n  \
               releases: ['24.04']\n  \
               architectures: [amd64]\n\
             artifacts:\n  \
               - release: '24.04'\n    \
                 architecture: amd64\n    \
                 url: https://example.com/{id}_{version}_ubuntu-24.04_amd64.deb\n    \
                 sha256: {hash}\n    \
                 release_asset: {id}_{version}_ubuntu-24.04_amd64.deb\n\
             lifecycle:\n  \
               install: plan-install\n  \
               enable: plan-enable\n  \
               disable: plan-disable\n  \
               remove: plan-remove\n  \
               rollback: plan-rollback\n",
            hash = "a".repeat(64)
        )
    }

    /// A fetcher serving a valid, newer manifest for every catalog file.
    fn newer_everything(version: &str) -> StaticManifestFetcher {
        StaticManifestFetcher::new(catalog_files().into_iter().map(|file| {
            (
                file.to_string(),
                document(file.trim_end_matches(".yaml"), version),
            )
        }))
    }

    fn built_in_status() -> (ComponentCatalog, CatalogStatus) {
        (built_in_catalog(), CatalogStatus::built_in())
    }

    #[test]
    fn every_built_in_manifest_is_valid_and_named_after_its_component() {
        for (file, document) in BUILT_IN {
            let manifest = ComponentManifest::parse_yaml(document)
                .unwrap_or_else(|error| panic!("{file}: {error}"));
            assert_eq!(manifest.id.as_str(), file.trim_end_matches(".yaml"));
        }
        assert_eq!(built_in_catalog().manifests().count(), BUILT_IN.len());
    }

    #[test]
    fn a_binary_with_no_cache_says_it_has_never_refreshed() {
        let (catalog, status) = catalog_at_start(None);
        assert_eq!(catalog.manifests().count(), BUILT_IN.len());
        assert_eq!(status.source, CatalogSource::BuiltIn);
        assert_eq!(status.degraded, Some(CatalogDegradation::NeverRefreshed));
    }

    #[test]
    fn a_full_refresh_adopts_every_manifest_and_reports_no_degradation() {
        let (base, status) = built_in_status();
        let outcome = refresh(&newer_everything("9.9.9"), &base, &status, 1_700_000_000);

        assert_eq!(outcome.accepted, BUILT_IN.len());
        assert_eq!(outcome.status.source, CatalogSource::Remote);
        assert_eq!(outcome.status.degraded, None);
        assert!(outcome.status.rejections.is_empty());
        assert_eq!(
            outcome.status.fetched_at_unix_seconds,
            Some(1_700_000_000_u64)
        );
        let monitor = ComponentId::new("better-monitor").unwrap();
        assert_eq!(
            outcome.catalog.get(&monitor).unwrap().version,
            Version::new(9, 9, 9)
        );
        let cache = outcome.cache.expect("a landed refresh is cached");
        assert_eq!(cache.schema_version, CATALOG_CACHE_SCHEMA_VERSION);
        assert_eq!(cache.manifests.len(), BUILT_IN.len());
    }

    #[test]
    fn an_invalid_manifest_is_rejected_alone_and_the_valid_one_is_kept() {
        let (base, status) = built_in_status();
        let fetcher = newer_everything("9.9.9").with("better-monitor.yaml", "not: [a manifest");
        let outcome = refresh(&fetcher, &base, &status, 1);

        assert_eq!(outcome.accepted, BUILT_IN.len() - 1);
        assert_eq!(
            outcome.status.degraded,
            Some(CatalogDegradation::PartiallyRefreshed)
        );
        let rejection = outcome
            .status
            .rejections
            .iter()
            .find(|rejection| rejection.file == "better-monitor.yaml")
            .expect("the bad file is named");
        assert!(matches!(rejection.reason, RejectionReason::Invalid(_)));
        // The one it could not replace is still the built-in one, not missing.
        let monitor = ComponentId::new("better-monitor").unwrap();
        let built_in_version = built_in_catalog().get(&monitor).unwrap().version.clone();
        assert_eq!(
            outcome.catalog.get(&monitor).unwrap().version,
            built_in_version
        );
        // A partial refresh is still worth caching: six of the seven are newer.
        assert!(outcome.cache.is_some());
    }

    #[test]
    fn a_manifest_with_an_unsupported_schema_version_is_rejected() {
        let (base, status) = built_in_status();
        let fetcher = newer_everything("9.9.9").with(
            "better-files.yaml",
            document("better-files", "9.9.9").replace("schema_version: 2", "schema_version: 99"),
        );
        let outcome = refresh(&fetcher, &base, &status, 1);

        let rejection = outcome
            .status
            .rejections
            .iter()
            .find(|rejection| rejection.file == "better-files.yaml")
            .expect("the wrong-schema file is named");
        match &rejection.reason {
            RejectionReason::Invalid(detail) => assert!(detail.contains("99")),
            other => panic!("expected an invalid-manifest rejection, got {other:?}"),
        }
    }

    #[test]
    fn a_manifest_served_under_another_components_name_is_rejected() {
        let (base, status) = built_in_status();
        let fetcher = newer_everything("9.9.9")
            .with("better-monitor.yaml", document("better-files", "9.9.9"));
        let outcome = refresh(&fetcher, &base, &status, 1);

        let rejection = outcome
            .status
            .rejections
            .iter()
            .find(|rejection| rejection.file == "better-monitor.yaml")
            .expect("the mismatched file is named");
        assert!(matches!(
            &rejection.reason,
            RejectionReason::IdMismatch { declared } if declared.as_str() == "better-files"
        ));
    }

    #[test]
    fn a_lower_version_than_the_one_held_is_flagged_and_not_adopted() {
        let (base, status) = built_in_status();
        let held = built_in_catalog()
            .get(&ComponentId::new("better-awake").unwrap())
            .unwrap()
            .version
            .clone();
        let fetcher =
            newer_everything("9.9.9").with("better-awake.yaml", document("better-awake", "0.0.1"));
        let outcome = refresh(&fetcher, &base, &status, 1);

        let rejection = outcome
            .status
            .rejections
            .iter()
            .find(|rejection| rejection.file == "better-awake.yaml")
            .expect("the downgrade is named");
        assert_eq!(
            rejection.reason,
            RejectionReason::Downgrade {
                held: held.clone(),
                offered: Version::new(0, 0, 1),
            }
        );
        assert_eq!(
            outcome
                .catalog
                .get(&ComponentId::new("better-awake").unwrap())
                .unwrap()
                .version,
            held
        );
    }

    #[test]
    fn the_same_version_is_adopted_because_a_republished_manifest_is_not_a_downgrade() {
        let (base, status) = built_in_status();
        let awake = ComponentId::new("better-awake").unwrap();
        let held = built_in_catalog().get(&awake).unwrap().version.to_string();
        let fetcher = StaticManifestFetcher::new([(
            "better-awake.yaml".to_string(),
            document("better-awake", &held),
        )]);
        let outcome = refresh(&fetcher, &base, &status, 1);

        assert_eq!(outcome.accepted, 1);
        // The adopted document is the fetched one, not the held one: its
        // artifact list is the fixture's single variant.
        assert_eq!(outcome.catalog.get(&awake).unwrap().artifacts.len(), 1);
    }

    #[test]
    fn a_refresh_that_reaches_nothing_keeps_the_built_in_catalog_and_says_so() {
        let (base, status) = built_in_status();
        let outcome = refresh(&StaticManifestFetcher::default(), &base, &status, 1);

        assert_eq!(outcome.accepted, 0);
        assert_eq!(outcome.status.source, CatalogSource::BuiltIn);
        assert_eq!(
            outcome.status.degraded,
            Some(CatalogDegradation::RefreshFailedUsingBuiltIn)
        );
        assert_eq!(outcome.status.rejections.len(), BUILT_IN.len());
        assert!(
            outcome
                .status
                .rejections
                .iter()
                .all(|rejection| rejection.reason == RejectionReason::Unreachable)
        );
        // Nothing is written, so a good cache is never restamped by a failure.
        assert!(outcome.cache.is_none());
        assert_eq!(outcome.catalog.manifests().count(), BUILT_IN.len());
    }

    #[test]
    fn a_failed_refresh_over_a_cache_keeps_the_cache_and_its_fetch_time() {
        let cache = CachedCatalog {
            schema_version: CATALOG_CACHE_SCHEMA_VERSION,
            source_url: "https://example.com/manifests".to_string(),
            fetched_at_unix_seconds: 1_700_000_000,
            manifests: built_in_manifests(),
        };
        let (base, status) = catalog_at_start(Some(cache));
        assert_eq!(status.source, CatalogSource::Cache);
        assert_eq!(status.degraded, None);

        let outcome = refresh(&StaticManifestFetcher::default(), &base, &status, 9_999);
        assert_eq!(outcome.status.source, CatalogSource::Cache);
        assert_eq!(
            outcome.status.degraded,
            Some(CatalogDegradation::RefreshFailedUsingCache)
        );
        assert_eq!(
            outcome.status.fetched_at_unix_seconds,
            Some(1_700_000_000_u64)
        );
        assert!(outcome.cache.is_none());
    }

    #[test]
    fn a_cache_whose_manifests_do_not_form_a_catalog_falls_back_to_the_built_in_one() {
        let mut manifests = built_in_manifests();
        manifests.push(manifests[0].clone());
        let (catalog, status) = catalog_at_start(Some(CachedCatalog {
            schema_version: CATALOG_CACHE_SCHEMA_VERSION,
            source_url: "https://example.com/manifests".to_string(),
            fetched_at_unix_seconds: 5,
            manifests,
        }));

        assert_eq!(status.source, CatalogSource::BuiltIn);
        assert_eq!(status.degraded, Some(CatalogDegradation::NeverRefreshed));
        assert_eq!(catalog.manifests().count(), BUILT_IN.len());
    }

    #[test]
    fn a_cache_from_a_future_schema_is_not_read() {
        let (_, status) = catalog_at_start(Some(CachedCatalog {
            schema_version: CATALOG_CACHE_SCHEMA_VERSION + 1,
            source_url: "https://example.com/manifests".to_string(),
            fetched_at_unix_seconds: 5,
            manifests: built_in_manifests(),
        }));
        assert_eq!(status.source, CatalogSource::BuiltIn);
    }

    #[test]
    fn a_refreshed_set_that_would_not_assemble_is_refused_whole() {
        let (base, status) = built_in_status();
        // Every file serves a manifest that depends on a component nobody
        // publishes. Each one is valid alone; the catalog they form is not.
        let fetcher = StaticManifestFetcher::new(catalog_files().into_iter().map(|file| {
            let id = file.trim_end_matches(".yaml");
            (
                file.to_string(),
                document(id, "9.9.9")
                    + "dependencies:\n  - id: better-nothing\n    version: '>=1.0.0'\n",
            )
        }));
        let outcome = refresh(&fetcher, &base, &status, 1);

        assert_eq!(outcome.accepted, 0);
        assert_eq!(
            outcome.status.degraded,
            Some(CatalogDegradation::RefreshFailedUsingBuiltIn)
        );
        assert!(
            outcome
                .status
                .rejections
                .iter()
                .any(|rejection| matches!(rejection.reason, RejectionReason::CatalogGraph(_)))
        );
        assert!(outcome.cache.is_none());
    }

    #[test]
    fn every_degraded_state_and_rejection_carries_a_stable_machine_key() {
        assert_eq!(
            CatalogDegradation::NeverRefreshed.to_string(),
            "catalog.degraded.never_refreshed"
        );
        assert_eq!(
            CatalogDegradation::PartiallyRefreshed.to_string(),
            "catalog.degraded.partially_refreshed"
        );
        assert_eq!(
            ManifestRejection {
                file: "better-monitor.yaml".to_string(),
                reason: RejectionReason::Downgrade {
                    held: Version::new(1, 0, 0),
                    offered: Version::new(0, 9, 0),
                },
            }
            .to_string(),
            "better-monitor.yaml: catalog.rejected.downgrade:1.0.0>0.9.0"
        );
        assert_eq!(CatalogSource::Remote.to_string(), "remote");
    }
}
