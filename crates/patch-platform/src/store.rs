//! The record of applied fixes, on disk.

use std::io::Write as _;
use std::path::{Path, PathBuf};

use patch_core::Records;
use patch_core::record::RECORDS_SCHEMA_VERSION;

/// What an interrupted save can leave beside the record.
pub const RECORDS_TEMPORARY: &str = ".fixes.json.tmp";

/// `$XDG_STATE_HOME/better-os/fixes.json`.
#[derive(Clone, Debug)]
pub struct RecordStore {
    path: PathBuf,
}

/// Why the records could not be read or kept.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StoreError {
    /// The file is there and cannot be read or understood. It is never
    /// overwritten, so the record it holds is not lost.
    Unreadable(String),
    /// A newer Better Manager wrote it.
    Newer(u32),
    Unwritable(String),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StoreError::Unreadable(reason) | StoreError::Unwritable(reason) => f.write_str(reason),
            StoreError::Newer(version) => {
                write!(
                    f,
                    "the record was written by a newer Better Manager (version {version})"
                )
            }
        }
    }
}

impl RecordStore {
    pub fn at(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The records; none when the file is not there.
    pub fn load(&self) -> Result<Records, StoreError> {
        let text = match std::fs::read_to_string(&self.path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Records::default());
            }
            Err(error) => {
                return Err(StoreError::Unreadable(format!(
                    "{}: {error}",
                    self.path.display()
                )));
            }
        };
        let version = serde_json::from_str::<serde_json::Value>(&text)
            .ok()
            .and_then(|value| value.get("schema_version")?.as_u64());
        if let Some(version) = version
            && version > u64::from(RECORDS_SCHEMA_VERSION)
        {
            return Err(StoreError::Newer(version as u32));
        }
        serde_json::from_str(&text)
            .map_err(|error| StoreError::Unreadable(format!("{}: {error}", self.path.display())))
    }

    /// Keeps the records, replacing the file whole.
    pub fn save(&self, records: &Records) -> Result<(), StoreError> {
        let failed = |error: std::io::Error| {
            StoreError::Unwritable(format!("{}: {error}", self.path.display()))
        };
        let directory = self
            .path
            .parent()
            .expect("the records file has a directory");
        std::fs::create_dir_all(directory).map_err(failed)?;
        let text = serde_json::to_string_pretty(records).expect("records serialize");
        // A fixed name, so removal knows what an interrupted save can leave.
        let temporary = self.path.with_file_name(RECORDS_TEMPORARY);
        let written = (|| {
            let mut file = std::fs::File::create(&temporary)?;
            file.write_all(text.as_bytes())?;
            file.sync_all()?;
            std::fs::rename(&temporary, &self.path)
        })();
        if let Err(error) = written {
            let _ = std::fs::remove_file(&temporary);
            return Err(failed(error));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use patch_core::record::{AppliedRecord, Recorded};
    use patch_core::{EntryId, Subject};

    #[test]
    fn records_survive_a_save_and_a_load() {
        let root = tempfile::tempdir().unwrap();
        let store = RecordStore::at(root.path().join("state/better-os/fixes.json"));
        assert_eq!(store.load().unwrap(), Records::default());
        let app = patch_core::flatpak::AppId::parse("org.example.App").unwrap();
        let records = Records {
            applied: vec![AppliedRecord {
                entry: EntryId::ElectronCursor,
                subject: Subject::FlatpakApp(app.clone()),
                applied_at: 3,
                change: Recorded::OverrideKeys {
                    app,
                    name: Default::default(),
                    created_file: true,
                    created_groups: vec![],
                    keys: vec![],
                },
            }],
            ..Records::default()
        };
        store.save(&records).unwrap();
        assert_eq!(store.load().unwrap(), records);
    }

    #[test]
    fn a_file_it_cannot_understand_is_reported_and_kept() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("fixes.json");
        std::fs::write(&path, "{ not json").unwrap();
        let store = RecordStore::at(&path);
        assert!(matches!(store.load(), Err(StoreError::Unreadable(_))));
        std::fs::write(&path, "{\"schema_version\": 99, \"applied\": []}").unwrap();
        assert_eq!(store.load(), Err(StoreError::Newer(99)));
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "{\"schema_version\": 99, \"applied\": []}"
        );
    }
}
