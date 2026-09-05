//! Versioned on-disk persistence for the refreshed component catalog.
//!
//! The same adapter rules the manager state file follows: this crate decides
//! nothing about catalogs, writes through a temporary file and an atomic
//! rename, and refuses to reset a file a newer version wrote. It differs in one
//! way, deliberately. A missing, corrupt, or unreadable cache is not an error
//! here — it is an absence, and an absence means the built-in catalog is used.
//! Losing a cache costs a refresh; refusing to start because of one would cost
//! the whole window.

use std::{
    env,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    process,
    time::{SystemTime, UNIX_EPOCH},
};

use better_core::ComponentCatalog;
use manager_core::catalog::{
    CATALOG_CACHE_SCHEMA_VERSION, CachedCatalog, CatalogStatus, RefreshOutcome, catalog_at_start,
};
use thiserror::Error;

pub const CATALOG_FILE_NAME: &str = "manager-catalog.json";

#[derive(Debug, Error)]
pub enum CatalogStoreError {
    #[error("could not access the catalog cache at {path}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not serialize the catalog cache")]
    Serialize(#[source] serde_json::Error),
}

/// What a load found, and why it found nothing when it found nothing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CacheAbsence {
    /// No cache has been written yet.
    Missing,
    /// The file is unreadable, unparseable, or does not validate. It is left
    /// where it is rather than deleted, so a person can look at it.
    Unusable,
    /// A newer version of the manager wrote it. Not read and not replaced.
    FutureSchema,
}

#[derive(Clone, Debug)]
pub struct CatalogLoad {
    pub cache: Option<CachedCatalog>,
    /// Set when there is no usable cache, saying which kind of nothing it is.
    pub absence: Option<CacheAbsence>,
}

pub trait CatalogCacheStore {
    fn load(&self) -> CatalogLoad;
    fn save(&self, cache: &CachedCatalog) -> Result<(), CatalogStoreError>;
}

#[derive(Clone, Debug)]
pub struct JsonCatalogStore {
    path: PathBuf,
}

impl JsonCatalogStore {
    pub fn from_default_path() -> Self {
        Self::at_path(default_catalog_path())
    }

    pub fn at_path(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn temporary_path(&self) -> PathBuf {
        let parent = self.path.parent().unwrap_or_else(|| Path::new("."));
        let name = self
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(CATALOG_FILE_NAME);
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or_default();
        parent.join(format!(".{name}.{}-{nonce}.tmp", process::id()))
    }
}

impl CatalogCacheStore for JsonCatalogStore {
    fn load(&self) -> CatalogLoad {
        let absent = |absence| CatalogLoad {
            cache: None,
            absence: Some(absence),
        };
        let Ok(bytes) = fs::read(&self.path) else {
            return absent(CacheAbsence::Missing);
        };
        let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
            return absent(CacheAbsence::Unusable);
        };
        // The schema stamp is read before the body, so a file a newer manager
        // wrote is recognized rather than reported as corrupt.
        match value
            .get("schema_version")
            .and_then(serde_json::Value::as_u64)
        {
            Some(version) if version > u64::from(CATALOG_CACHE_SCHEMA_VERSION) => {
                return absent(CacheAbsence::FutureSchema);
            }
            Some(_) => {}
            None => return absent(CacheAbsence::Unusable),
        }
        let Ok(cache) = serde_json::from_value::<CachedCatalog>(value) else {
            return absent(CacheAbsence::Unusable);
        };
        // A cache file is a file on disk and is no more trusted than a fetched
        // document: it only counts if the manifests in it still validate and
        // still form a catalog.
        if cache.catalog().is_err() {
            return absent(CacheAbsence::Unusable);
        }
        CatalogLoad {
            cache: Some(cache),
            absence: None,
        }
    }

    fn save(&self, cache: &CachedCatalog) -> Result<(), CatalogStoreError> {
        let parent = self.path.parent().unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent).map_err(|source| CatalogStoreError::Io {
            path: parent.to_path_buf(),
            source,
        })?;
        let temporary = self.temporary_path();
        let result = (|| -> Result<(), CatalogStoreError> {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)
                .map_err(|source| CatalogStoreError::Io {
                    path: temporary.clone(),
                    source,
                })?;
            serde_json::to_writer_pretty(&mut file, cache).map_err(CatalogStoreError::Serialize)?;
            file.write_all(b"\n")
                .map_err(|source| CatalogStoreError::Io {
                    path: temporary.clone(),
                    source,
                })?;
            file.sync_all().map_err(|source| CatalogStoreError::Io {
                path: temporary.clone(),
                source,
            })?;
            fs::rename(&temporary, &self.path).map_err(|source| CatalogStoreError::Io {
                path: self.path.clone(),
                source,
            })
        })();
        if result.is_err() && temporary.exists() {
            let _ = fs::remove_file(&temporary);
        }
        result
    }
}

/// The catalog and state a session starts with, read from this store.
///
/// Both the command line and the window call this rather than each deciding
/// what an unusable cache means.
pub fn start(store: &dyn CatalogCacheStore) -> (ComponentCatalog, CatalogStatus) {
    catalog_at_start(store.load().cache)
}

/// Persists a refresh that landed. A refresh that adopted nothing carries no
/// cache and writes nothing, so a failure never restamps a good file.
///
/// A write failure is returned rather than swallowed, but it does not
/// invalidate the refresh: the catalog in memory is still the fetched one, and
/// the only cost is that the next start has to fetch again.
pub fn cache_refresh(
    store: &dyn CatalogCacheStore,
    outcome: &RefreshOutcome,
) -> Result<(), CatalogStoreError> {
    match &outcome.cache {
        Some(cache) => store.save(cache),
        None => Ok(()),
    }
}

fn default_catalog_path() -> PathBuf {
    if let Some(path) = env::var_os("XDG_STATE_HOME") {
        return PathBuf::from(path)
            .join("better-os")
            .join(CATALOG_FILE_NAME);
    }
    if let Some(home) = env::var_os("HOME") {
        return PathBuf::from(home)
            .join(".local")
            .join("state")
            .join("better-os")
            .join(CATALOG_FILE_NAME);
    }
    PathBuf::from(".better-os").join(CATALOG_FILE_NAME)
}

#[cfg(test)]
mod tests {
    use super::*;
    use manager_core::catalog::{CatalogSource, built_in_manifests, catalog_at_start};

    fn temporary_directory(name: &str) -> PathBuf {
        let path = env::temp_dir().join(format!(
            "better-os-catalog-store-{name}-{}-{}",
            process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn cache() -> CachedCatalog {
        CachedCatalog {
            schema_version: CATALOG_CACHE_SCHEMA_VERSION,
            source_url: "https://example.com/manifests".to_string(),
            fetched_at_unix_seconds: 1_700_000_000,
            manifests: built_in_manifests(),
        }
    }

    #[test]
    fn a_written_catalog_comes_back_whole() {
        let directory = temporary_directory("round-trip");
        let store = JsonCatalogStore::at_path(directory.join(CATALOG_FILE_NAME));
        let written = cache();
        store.save(&written).unwrap();

        let loaded = store.load();
        assert_eq!(loaded.absence, None);
        let read_back = loaded.cache.expect("the cache is there");
        assert_eq!(read_back, written);
        assert_eq!(read_back.catalog().unwrap().manifests().count(), 7);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn an_offline_restart_uses_the_cached_catalog_rather_than_the_built_in_one() {
        let directory = temporary_directory("offline-restart");
        let store = JsonCatalogStore::at_path(directory.join(CATALOG_FILE_NAME));
        store.save(&cache()).unwrap();

        // A fresh session, with nothing but the disk to go on.
        let (catalog, status) =
            catalog_at_start(JsonCatalogStore::at_path(store.path()).load().cache);
        assert_eq!(status.source, CatalogSource::Cache);
        assert_eq!(status.fetched_at_unix_seconds, Some(1_700_000_000));
        assert_eq!(catalog.manifests().count(), 7);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn a_missing_cache_is_an_absence_and_not_a_failure() {
        let directory = temporary_directory("missing");
        let load = JsonCatalogStore::at_path(directory.join(CATALOG_FILE_NAME)).load();
        assert!(load.cache.is_none());
        assert_eq!(load.absence, Some(CacheAbsence::Missing));
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn a_corrupt_cache_is_ignored_and_left_on_disk() {
        let directory = temporary_directory("corrupt");
        let path = directory.join(CATALOG_FILE_NAME);
        fs::write(&path, b"{not json").unwrap();
        let load = JsonCatalogStore::at_path(&path).load();

        assert!(load.cache.is_none());
        assert_eq!(load.absence, Some(CacheAbsence::Unusable));
        assert!(path.exists(), "the unusable file is kept for inspection");
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn a_cache_holding_an_invalid_manifest_is_refused_rather_than_partly_used() {
        let directory = temporary_directory("tampered");
        let path = directory.join(CATALOG_FILE_NAME);
        let store = JsonCatalogStore::at_path(&path);
        store.save(&cache()).unwrap();

        // Someone edited a checksum into something that is not one.
        let mut value: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        value["manifests"][0]["artifacts"][0]["sha256"] = serde_json::Value::from("nope");
        fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();

        let load = store.load();
        assert!(load.cache.is_none());
        assert_eq!(load.absence, Some(CacheAbsence::Unusable));
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn a_cache_written_by_a_newer_manager_is_not_read_and_not_replaced() {
        let directory = temporary_directory("future");
        let path = directory.join(CATALOG_FILE_NAME);
        let store = JsonCatalogStore::at_path(&path);
        store.save(&cache()).unwrap();
        let mut value: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        value["schema_version"] = serde_json::Value::from(CATALOG_CACHE_SCHEMA_VERSION + 1);
        fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();

        let load = store.load();
        assert!(load.cache.is_none());
        assert_eq!(load.absence, Some(CacheAbsence::FutureSchema));
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn a_save_leaves_no_temporary_file_behind() {
        let directory = temporary_directory("atomic");
        let store = JsonCatalogStore::at_path(directory.join(CATALOG_FILE_NAME));
        store.save(&cache()).unwrap();
        store.save(&cache()).unwrap();

        let leftovers = fs::read_dir(&directory)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().ends_with(".tmp"))
            .count();
        assert_eq!(leftovers, 0);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn the_cache_lives_beside_the_state_file_in_the_state_directory() {
        // Both are per-user state, not cache: a stale catalog is still the
        // catalog a machine plans from, and losing it silently changes what the
        // manager offers.
        assert_eq!(
            default_catalog_path().parent(),
            crate::JsonStore::from_default_path().path().parent()
        );
    }
}
