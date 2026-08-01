//! Everything the daemon keeps on disk: staged artifacts, transaction
//! journals, and rollback records.
//!
//! Artifacts arrive as file descriptors and are hashed while being copied, so a
//! file only ever appears under its final name once its checksum matched. That
//! removes the window where a path could be swapped between the check and the
//! install.

use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use manager_ipc::{MAX_ARTIFACT_BYTES, RollbackRecord, TransactionOutcome};
use sha2::{Digest, Sha256};

use crate::DaemonError;

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or_default()
}

fn storage(error: impl ToString) -> DaemonError {
    DaemonError::Storage(error.to_string())
}

/// Writes a file so a reader never sees a partial one.
fn write_atomically(path: &Path, bytes: &[u8]) -> Result<(), DaemonError> {
    let parent = path.parent().ok_or_else(|| storage("path has no parent"))?;
    fs::create_dir_all(parent).map_err(storage)?;
    let temporary = parent.join(format!(
        ".{}.tmp",
        path.file_name().unwrap_or_default().to_string_lossy()
    ));
    {
        let mut file = File::create(&temporary).map_err(storage)?;
        file.write_all(bytes).map_err(storage)?;
        file.sync_all().map_err(storage)?;
    }
    fs::rename(&temporary, path).map_err(storage)
}

/// The daemon's private artifact cache.
pub struct ArtifactStore {
    root: PathBuf,
}

impl ArtifactStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Resolves a name inside the cache, refusing anything that could point
    /// outside it.
    pub fn path_for(&self, filename: &str) -> Result<PathBuf, DaemonError> {
        if filename.is_empty()
            || filename.contains('/')
            || filename.contains('\\')
            || filename.contains("..")
            || filename.starts_with('.')
        {
            return Err(DaemonError::PlanRejected(format!(
                "unsafe artifact name {filename}"
            )));
        }
        Ok(self.root.join(filename))
    }

    pub fn contains(&self, filename: &str) -> bool {
        self.path_for(filename)
            .map(|path| path.is_file())
            .unwrap_or(false)
    }

    /// Copies bytes into the cache, hashing as it goes, and keeps the result
    /// only if the digest matches what was promised.
    pub fn stage(
        &self,
        filename: &str,
        expected_sha256: &str,
        source: &mut dyn Read,
    ) -> Result<String, DaemonError> {
        let destination = self.path_for(filename)?;
        fs::create_dir_all(&self.root).map_err(storage)?;
        let partial = self.root.join(format!(".{filename}.partial"));

        let digest = {
            let mut file = File::create(&partial).map_err(storage)?;
            let mut hasher = Sha256::new();
            let mut buffer = vec![0_u8; 64 * 1024];
            let mut total: u64 = 0;
            loop {
                let read = source.read(&mut buffer).map_err(storage)?;
                if read == 0 {
                    break;
                }
                total += read as u64;
                if total > MAX_ARTIFACT_BYTES {
                    let _ = fs::remove_file(&partial);
                    return Err(DaemonError::PlanRejected("artifact too large".to_string()));
                }
                hasher.update(&buffer[..read]);
                file.write_all(&buffer[..read]).map_err(storage)?;
            }
            file.sync_all().map_err(storage)?;
            hex(&hasher.finalize())
        };

        if digest != expected_sha256 {
            // Nothing that failed its checksum is allowed to persist under a
            // name a plan could later refer to.
            let _ = fs::remove_file(&partial);
            return Err(DaemonError::ChecksumMismatch {
                component: filename.to_string(),
            });
        }
        fs::rename(&partial, &destination).map_err(storage)?;
        Ok(digest)
    }

    /// Re-hashes a cached file. Run immediately before installing, so a file
    /// tampered with after staging is still caught.
    pub fn verify(&self, filename: &str, expected_sha256: &str) -> Result<(), DaemonError> {
        let path = self.path_for(filename)?;
        let mut file = File::open(&path).map_err(|_| DaemonError::ArtifactMissing {
            component: filename.to_string(),
        })?;
        let mut hasher = Sha256::new();
        let mut buffer = vec![0_u8; 64 * 1024];
        loop {
            let read = file.read(&mut buffer).map_err(storage)?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
        }
        if hex(&hasher.finalize()) != expected_sha256 {
            return Err(DaemonError::ChecksumMismatch {
                component: filename.to_string(),
            });
        }
        Ok(())
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>()
}

/// How far a transaction got, as recorded on disk.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case", tag = "state")]
pub enum JournalState {
    Validated,
    Executing { step_index: u32 },
    Completed,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct JournalEntry {
    pub transaction_id: String,
    pub state: JournalState,
    pub updated_at_unix: u64,
    /// Present once the transaction reached an end.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<TransactionOutcome>,
}

/// The daemon's durable memory of what it has been asked to do.
pub struct Journal {
    root: PathBuf,
}

impl Journal {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn transactions(&self) -> PathBuf {
        self.root.join("transactions")
    }

    fn rollbacks(&self) -> PathBuf {
        self.root.join("rollback")
    }

    fn entry_path(&self, transaction_id: &str) -> Result<PathBuf, DaemonError> {
        if transaction_id.is_empty()
            || !transaction_id
                .chars()
                .all(|character| character.is_ascii_hexdigit() || character == '-')
        {
            return Err(DaemonError::PlanRejected(
                "unsafe transaction id".to_string(),
            ));
        }
        Ok(self.transactions().join(format!("{transaction_id}.json")))
    }

    pub fn read(&self, transaction_id: &str) -> Result<Option<JournalEntry>, DaemonError> {
        let path = self.entry_path(transaction_id)?;
        match fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map(Some).map_err(storage),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(storage(error)),
        }
    }

    pub fn write(&self, entry: &JournalEntry) -> Result<(), DaemonError> {
        let path = self.entry_path(&entry.transaction_id)?;
        let bytes = serde_json::to_vec(entry).map_err(storage)?;
        write_atomically(&path, &bytes)
    }

    pub fn set_state(&self, transaction_id: &str, state: JournalState) -> Result<(), DaemonError> {
        self.write(&JournalEntry {
            transaction_id: transaction_id.to_string(),
            state,
            updated_at_unix: now_unix(),
            outcome: None,
        })
    }

    pub fn complete(&self, outcome: &TransactionOutcome) -> Result<(), DaemonError> {
        self.write(&JournalEntry {
            transaction_id: outcome.transaction_id.clone(),
            state: JournalState::Completed,
            updated_at_unix: now_unix(),
            outcome: Some(outcome.clone()),
        })
    }

    /// Every transaction left mid-flight by a daemon that died.
    ///
    /// These are never resumed: an interrupted APT run is not something to
    /// silently continue, so they are reported as needing a person.
    pub fn interrupted(&self) -> Result<Vec<JournalEntry>, DaemonError> {
        let directory = self.transactions();
        if !directory.is_dir() {
            return Ok(Vec::new());
        }
        let mut interrupted = Vec::new();
        for entry in fs::read_dir(&directory).map_err(storage)? {
            let entry = entry.map_err(storage)?;
            let Ok(bytes) = fs::read(entry.path()) else {
                continue;
            };
            let Ok(journal) = serde_json::from_slice::<JournalEntry>(&bytes) else {
                continue;
            };
            if matches!(journal.state, JournalState::Executing { .. }) {
                interrupted.push(journal);
            }
        }
        interrupted.sort_by(|left, right| left.transaction_id.cmp(&right.transaction_id));
        Ok(interrupted)
    }

    pub fn rollback_path(&self, component: &str) -> Result<PathBuf, DaemonError> {
        if !crate::is_first_party_component(component) {
            return Err(DaemonError::PlanRejected(format!(
                "unknown component {component}"
            )));
        }
        Ok(self.rollbacks().join(format!("{component}.json")))
    }

    pub fn write_rollback(&self, record: &RollbackRecord) -> Result<(), DaemonError> {
        let path = self.rollback_path(&record.component)?;
        let bytes = serde_json::to_vec(record).map_err(storage)?;
        write_atomically(&path, &bytes)
    }

    pub fn read_rollback(&self, component: &str) -> Result<Option<RollbackRecord>, DaemonError> {
        let path = self.rollback_path(component)?;
        match fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map(Some).map_err(storage),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(storage(error)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn temporary(label: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "better-os-daemon-{label}-{}-{}",
            std::process::id(),
            now_unix()
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    const CONTENT: &[u8] = b"a real package would go here";
    // sha256 of CONTENT
    const DIGEST: &str = "3d8b1a4a56e9d0d0a2e6d2c8a4e0e9df3ba9a0b0f5f0a0e9c9d0b8f0a1c2d3e4";

    fn digest_of(bytes: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        hex(&hasher.finalize())
    }

    #[test]
    fn a_staged_artifact_appears_only_when_its_checksum_matches() {
        let root = temporary("stage");
        let store = ArtifactStore::new(&root);
        let digest = digest_of(CONTENT);

        store
            .stage("better-monitor.deb", &digest, &mut Cursor::new(CONTENT))
            .unwrap();
        assert!(store.contains("better-monitor.deb"));
        store.verify("better-monitor.deb", &digest).unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn an_artifact_whose_checksum_is_wrong_never_lands() {
        let root = temporary("mismatch");
        let store = ArtifactStore::new(&root);

        let error = store
            .stage("better-monitor.deb", DIGEST, &mut Cursor::new(CONTENT))
            .unwrap_err();

        assert!(matches!(error, DaemonError::ChecksumMismatch { .. }));
        assert!(!store.contains("better-monitor.deb"));
        // Not even the partial file survives.
        assert!(
            fs::read_dir(&root).unwrap().next().is_none(),
            "the cache should be empty"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn tampering_after_staging_is_caught_before_an_install() {
        let root = temporary("tamper");
        let store = ArtifactStore::new(&root);
        let digest = digest_of(CONTENT);
        store
            .stage("better-monitor.deb", &digest, &mut Cursor::new(CONTENT))
            .unwrap();

        fs::write(root.join("better-monitor.deb"), b"something else").unwrap();

        assert!(matches!(
            store.verify("better-monitor.deb", &digest),
            Err(DaemonError::ChecksumMismatch { .. })
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn an_artifact_name_cannot_escape_the_cache() {
        let store = ArtifactStore::new("/var/cache/better-os/archives");
        for name in [
            "../../etc/shadow",
            "/etc/shadow",
            "sub/dir.deb",
            ".hidden.deb",
            "",
        ] {
            assert!(store.path_for(name).is_err(), "{name} should be refused");
        }
    }

    #[test]
    fn an_interrupted_transaction_is_reported_rather_than_resumed() {
        let root = temporary("journal");
        let journal = Journal::new(&root);
        journal
            .set_state(
                "3f2504e0-4f89-41d3-9a0c-0305e82c3301",
                JournalState::Validated,
            )
            .unwrap();
        journal
            .set_state(
                "4f2504e0-4f89-41d3-9a0c-0305e82c3302",
                JournalState::Executing { step_index: 1 },
            )
            .unwrap();

        let interrupted = journal.interrupted().unwrap();
        assert_eq!(interrupted.len(), 1);
        assert_eq!(
            interrupted[0].transaction_id,
            "4f2504e0-4f89-41d3-9a0c-0305e82c3302"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_rollback_record_is_only_written_for_a_component_we_own() {
        let root = temporary("rollback");
        let journal = Journal::new(&root);
        assert!(journal.rollback_path("bash").is_err());
        assert!(journal.rollback_path("better-monitor").is_ok());
        fs::remove_dir_all(root).unwrap();
    }
}
