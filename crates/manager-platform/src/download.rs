//! Fetching component artifacts and proving what was fetched.
//!
//! Files are named by their checksum, so the cache cannot hold two different
//! things under one name and a cached file that hashes correctly is by
//! definition the file that was asked for. A download is only ever moved into
//! place after its digest matched; a partial or wrong download is deleted
//! rather than left somewhere a later step could pick it up.

use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use better_core::ComponentId;
use sha2::{Digest, Sha256};

use crate::{DownloadBackend, DownloadReceipt, DownloadRequest, PlatformError};

/// How many times a transfer is retried before giving up. Only network
/// failures are retried: a checksum mismatch means the bytes are wrong, and
/// asking again for the same wrong bytes is not a fix.
const MAX_ATTEMPTS: u32 = 3;

const READ_TIMEOUT: Duration = Duration::from_secs(60);

/// Progress within one artifact transfer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DownloadProgress {
    pub component: ComponentId,
    pub received_bytes: u64,
    pub total_bytes: Option<u64>,
}

/// Where downloaded artifacts are kept between planning and installing.
#[derive(Clone, Debug)]
pub struct ArtifactCache {
    root: PathBuf,
}

impl ArtifactCache {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// The per-user cache, following the XDG base directory specification.
    pub fn from_default_path() -> Self {
        let base = std::env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
            .unwrap_or_else(std::env::temp_dir);
        Self::new(base.join("better-os").join("artifacts"))
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Where a given artifact lives. The checksum is the name, so nothing has
    /// to trust a file name chosen elsewhere.
    pub fn path_for(&self, sha256: &str) -> PathBuf {
        self.root.join(format!("{sha256}.deb"))
    }

    /// Whether the cache already holds this artifact, verified rather than
    /// assumed.
    pub fn holds(&self, sha256: &str) -> bool {
        let path = self.path_for(sha256);
        path.is_file()
            && digest_of(&path)
                .map(|found| found == sha256)
                .unwrap_or(false)
    }
}

fn digest_of(path: &Path) -> Result<String, std::io::Error> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex(&hasher.finalize()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Downloads over HTTPS and verifies while streaming.
pub struct HttpDownloader {
    cache: ArtifactCache,
    agent: ureq::Agent,
}

impl HttpDownloader {
    pub fn new(cache: ArtifactCache) -> Self {
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(READ_TIMEOUT))
            .build();
        Self {
            cache,
            agent: config.into(),
        }
    }

    pub fn cache(&self) -> &ArtifactCache {
        &self.cache
    }

    /// Fetches an artifact, reporting bytes as they arrive.
    pub fn fetch(
        &self,
        request: &DownloadRequest,
        progress: &mut dyn FnMut(DownloadProgress),
    ) -> Result<DownloadReceipt, PlatformError> {
        // Only HTTPS. A manifest is untrusted input, and the scheme is part of
        // what makes the checksum meaningful.
        if !request.url.starts_with("https://") {
            return Err(PlatformError::DownloadFailed {
                component: request.component.clone(),
            });
        }

        let destination = self.cache.path_for(&request.sha256);
        if self.cache.holds(&request.sha256) {
            let bytes = fs::metadata(&destination)
                .map(|metadata| metadata.len())
                .unwrap_or_default();
            progress(DownloadProgress {
                component: request.component.clone(),
                received_bytes: bytes,
                total_bytes: Some(bytes),
            });
            return Ok(DownloadReceipt {
                component: request.component.clone(),
                verified_sha256: request.sha256.clone(),
                bytes,
                artifact_path: destination,
            });
        }

        fs::create_dir_all(&self.cache.root).map_err(|_| PlatformError::DownloadFailed {
            component: request.component.clone(),
        })?;

        let mut last_error = PlatformError::DownloadFailed {
            component: request.component.clone(),
        };
        for _ in 0..MAX_ATTEMPTS {
            match self.attempt(request, &destination, progress) {
                Ok(receipt) => return Ok(receipt),
                // The bytes were wrong, not the connection. Fetching the same
                // artifact again will produce the same wrong bytes.
                Err(error @ PlatformError::ChecksumMismatch { .. }) => return Err(error),
                Err(error) => last_error = error,
            }
        }
        Err(last_error)
    }

    fn attempt(
        &self,
        request: &DownloadRequest,
        destination: &Path,
        progress: &mut dyn FnMut(DownloadProgress),
    ) -> Result<DownloadReceipt, PlatformError> {
        let failed = || PlatformError::DownloadFailed {
            component: request.component.clone(),
        };

        let response = self.agent.get(&request.url).call().map_err(|_| failed())?;
        let total_bytes = response
            .headers()
            .get("content-length")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<u64>().ok())
            .or(request.expected_bytes);

        let partial = destination.with_extension("part");
        let mut reader = response.into_body().into_reader();
        let mut hasher = Sha256::new();
        let mut received: u64 = 0;

        {
            let mut file = File::create(&partial).map_err(|_| failed())?;
            let mut buffer = vec![0_u8; 64 * 1024];
            loop {
                let read = reader.read(&mut buffer).map_err(|_| failed())?;
                if read == 0 {
                    break;
                }
                hasher.update(&buffer[..read]);
                file.write_all(&buffer[..read]).map_err(|_| failed())?;
                received += read as u64;
                progress(DownloadProgress {
                    component: request.component.clone(),
                    received_bytes: received,
                    total_bytes,
                });
            }
            file.sync_all().map_err(|_| failed())?;
        }

        let verified = hex(&hasher.finalize());
        if verified != request.sha256 {
            let _ = fs::remove_file(&partial);
            return Err(PlatformError::ChecksumMismatch {
                component: request.component.clone(),
            });
        }
        fs::rename(&partial, destination).map_err(|_| failed())?;

        Ok(DownloadReceipt {
            component: request.component.clone(),
            verified_sha256: verified,
            bytes: received,
            artifact_path: destination.to_path_buf(),
        })
    }
}

impl DownloadBackend for HttpDownloader {
    fn download(&self, request: &DownloadRequest) -> Result<DownloadReceipt, PlatformError> {
        self.fetch(request, &mut |_| {})
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temporary(label: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("better-os-cache-{label}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn component() -> ComponentId {
        ComponentId::new("better-monitor").unwrap()
    }

    #[test]
    fn an_artifact_is_named_by_its_checksum() {
        let cache = ArtifactCache::new("/tmp/example");
        assert_eq!(
            cache.path_for(&"a".repeat(64)),
            PathBuf::from(format!("/tmp/example/{}.deb", "a".repeat(64)))
        );
    }

    #[test]
    fn a_cached_file_counts_only_if_it_still_hashes_correctly() {
        let root = temporary("holds");
        let cache = ArtifactCache::new(&root);
        let content = b"a package";
        let mut hasher = Sha256::new();
        hasher.update(content);
        let checksum = hex(&hasher.finalize());

        fs::write(cache.path_for(&checksum), content).unwrap();
        assert!(cache.holds(&checksum));

        // Something replaced the file. The name still claims one thing and the
        // bytes are another, so it does not count as cached.
        fs::write(cache.path_for(&checksum), b"something else").unwrap();
        assert!(!cache.holds(&checksum));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_missing_artifact_is_not_reported_as_cached() {
        let root = temporary("missing");
        let cache = ArtifactCache::new(&root);
        assert!(!cache.holds(&"b".repeat(64)));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_url_that_is_not_https_is_refused_before_any_request() {
        let root = temporary("scheme");
        let downloader = HttpDownloader::new(ArtifactCache::new(&root));
        let error = downloader
            .fetch(
                &DownloadRequest {
                    component: component(),
                    url: "http://example.com/better-monitor.deb".to_string(),
                    sha256: "c".repeat(64),
                    expected_bytes: None,
                },
                &mut |_| {},
            )
            .unwrap_err();

        assert!(matches!(error, PlatformError::DownloadFailed { .. }));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn an_already_cached_artifact_is_not_fetched_again() {
        let root = temporary("reuse");
        let cache = ArtifactCache::new(&root);
        let content = b"a package";
        let mut hasher = Sha256::new();
        hasher.update(content);
        let checksum = hex(&hasher.finalize());
        fs::write(cache.path_for(&checksum), content).unwrap();

        let downloader = HttpDownloader::new(cache);
        // The URL is unreachable on purpose: a cache hit must not need it.
        let receipt = downloader
            .fetch(
                &DownloadRequest {
                    component: component(),
                    url: "https://198.51.100.1/never-reached.deb".to_string(),
                    sha256: checksum.clone(),
                    expected_bytes: None,
                },
                &mut |_| {},
            )
            .unwrap();

        assert_eq!(receipt.verified_sha256, checksum);
        assert_eq!(receipt.bytes, content.len() as u64);
        fs::remove_dir_all(root).unwrap();
    }
}
