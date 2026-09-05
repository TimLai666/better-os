//! Fetching component manifests from the published catalog.
//!
//! This is the same shape as the artifact download seam next door: a trait the
//! planner depends on, one HTTPS implementation, and one implementation that
//! answers from supplied values so nothing above it needs a network to be
//! tested. What comes back is a byte string and nothing else — this module
//! parses no YAML and validates no manifest, because a fetcher that decided
//! what a manifest means would be a second validator.

use std::collections::BTreeMap;
use std::io::Read;
use std::time::Duration;

use thiserror::Error;

/// Where the published manifests live.
///
/// Pinned to this repository's `main` branch: the manifests on `main` carry the
/// checksums of the latest published release, which is exactly the release-lag
/// a compiled-in catalog cannot describe. The host is part of the constant so a
/// configured value cannot redirect the catalog somewhere else without being
/// seen in a diff.
pub const DEFAULT_CATALOG_BASE_URL: &str =
    "https://raw.githubusercontent.com/TimLai666/better-os/main/components/manifests";

/// How long one manifest transfer may take, start to finish.
///
/// A refresh happens while a window is opening, so this is a bound on how long
/// a degraded network can keep the catalog undecided, not a generous allowance.
pub const FETCH_TIMEOUT: Duration = Duration::from_secs(10);

/// The largest manifest this will read. The shipped manifests are a few
/// kilobytes; this is a bound on what an unexpected response can cost, not a
/// schema limit.
pub const MAX_MANIFEST_BYTES: usize = 256 * 1024;

#[derive(Clone, Debug, Eq, PartialEq, Error)]
pub enum FetchError {
    /// The request did not complete: no network, DNS failure, timeout, TLS
    /// failure, or a non-success status.
    #[error("catalog.fetch.unreachable:{file}")]
    Unreachable { file: String },
    /// The response was larger than [`MAX_MANIFEST_BYTES`], or was not text.
    #[error("catalog.fetch.unreadable:{file}")]
    Unreadable { file: String },
    /// The base URL is not HTTPS. Refused before any request is made.
    #[error("catalog.fetch.insecure_source")]
    InsecureSource,
}

/// Fetches one manifest file by name.
///
/// The name is a file name from a fixed list the caller holds — never a path
/// and never anything a fetched document supplied — so an implementation joins
/// it to its own base and nothing else.
pub trait ManifestFetcher {
    fn fetch(&self, file: &str) -> Result<String, FetchError>;

    /// Where this fetcher reads from, for the record a cache keeps.
    fn source_url(&self) -> String;
}

/// Fetches manifests over HTTPS with a bounded timeout and a bounded body.
pub struct HttpManifestFetcher {
    base_url: String,
    agent: ureq::Agent,
}

impl HttpManifestFetcher {
    /// Reads from the pinned published catalog, unless
    /// `BETTER_MANAGER_CATALOG_URL` names another HTTPS base. The override
    /// exists so a proof run or a fork can point somewhere else; it cannot
    /// weaken the scheme, because a non-HTTPS base is refused on every fetch.
    pub fn from_environment() -> Self {
        Self::new(
            std::env::var("BETTER_MANAGER_CATALOG_URL")
                .unwrap_or_else(|_| DEFAULT_CATALOG_BASE_URL.to_string()),
        )
    }

    pub fn new(base_url: impl Into<String>) -> Self {
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(FETCH_TIMEOUT))
            .build();
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            agent: config.into(),
        }
    }
}

impl ManifestFetcher for HttpManifestFetcher {
    fn fetch(&self, file: &str) -> Result<String, FetchError> {
        // A manifest is untrusted input, and the scheme is part of what makes
        // fetching it meaningful at all.
        if !self.base_url.starts_with("https://") {
            return Err(FetchError::InsecureSource);
        }
        let unreachable = || FetchError::Unreachable {
            file: file.to_string(),
        };
        let unreadable = || FetchError::Unreadable {
            file: file.to_string(),
        };

        let url = format!("{}/{file}", self.base_url);
        let response = self.agent.get(&url).call().map_err(|_| unreachable())?;
        let mut body = Vec::new();
        response
            .into_body()
            .into_reader()
            // One byte over the limit is read on purpose, so a response at
            // exactly the limit is accepted and a longer one is refused rather
            // than silently truncated into a manifest that parses.
            .take(MAX_MANIFEST_BYTES as u64 + 1)
            .read_to_end(&mut body)
            .map_err(|_| unreachable())?;
        if body.len() > MAX_MANIFEST_BYTES {
            return Err(unreadable());
        }
        String::from_utf8(body).map_err(|_| unreadable())
    }

    fn source_url(&self) -> String {
        self.base_url.clone()
    }
}

/// Answers from manifests it was given, and reaches no network.
///
/// The test seam for everything above this module, in the same spirit as
/// [`crate::MockPlatform`]: a refresh path is worth nothing if the only way to
/// exercise its rejection, downgrade, and partial-fetch behaviour is to be
/// online and unlucky.
#[derive(Clone, Debug, Default)]
pub struct StaticManifestFetcher {
    documents: BTreeMap<String, String>,
    source_url: String,
}

impl StaticManifestFetcher {
    pub fn new(documents: impl IntoIterator<Item = (String, String)>) -> Self {
        Self {
            documents: documents.into_iter().collect(),
            source_url: "https://fixture.invalid/manifests".to_string(),
        }
    }

    pub fn with_source_url(mut self, url: impl Into<String>) -> Self {
        self.source_url = url.into();
        self
    }

    /// Removes a file, so a fetch for it fails the way an interrupted or
    /// partial publication does.
    pub fn without(mut self, file: &str) -> Self {
        self.documents.remove(file);
        self
    }

    pub fn with(mut self, file: impl Into<String>, document: impl Into<String>) -> Self {
        self.documents.insert(file.into(), document.into());
        self
    }
}

impl ManifestFetcher for StaticManifestFetcher {
    fn fetch(&self, file: &str) -> Result<String, FetchError> {
        self.documents
            .get(file)
            .cloned()
            .ok_or_else(|| FetchError::Unreachable {
                file: file.to_string(),
            })
    }

    fn source_url(&self) -> String {
        self.source_url.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_insecure_base_is_refused_before_any_request() {
        let fetcher = HttpManifestFetcher::new("http://example.com/manifests");
        assert_eq!(
            fetcher.fetch("better-monitor.yaml"),
            Err(FetchError::InsecureSource)
        );
    }

    #[test]
    fn a_trailing_slash_does_not_produce_a_double_slash() {
        let fetcher = HttpManifestFetcher::new("https://example.com/manifests/");
        assert_eq!(fetcher.source_url(), "https://example.com/manifests");
    }

    #[test]
    fn the_static_fetcher_reports_a_missing_file_as_unreachable() {
        let fetcher = StaticManifestFetcher::new([("a.yaml".to_string(), "body".to_string())]);
        assert_eq!(fetcher.fetch("a.yaml").as_deref(), Ok("body"));
        assert_eq!(
            fetcher.fetch("b.yaml"),
            Err(FetchError::Unreachable {
                file: "b.yaml".to_string()
            })
        );
    }

    #[test]
    fn every_fetch_failure_carries_a_stable_machine_key() {
        assert_eq!(
            FetchError::Unreachable {
                file: "better-monitor.yaml".to_string()
            }
            .to_string(),
            "catalog.fetch.unreachable:better-monitor.yaml"
        );
        assert_eq!(
            FetchError::Unreadable {
                file: "better-monitor.yaml".to_string()
            }
            .to_string(),
            "catalog.fetch.unreadable:better-monitor.yaml"
        );
        assert_eq!(
            FetchError::InsecureSource.to_string(),
            "catalog.fetch.insecure_source"
        );
    }
}
