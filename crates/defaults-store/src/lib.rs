//! Snapshots of what the desktop said before Better OS changed it.
//!
//! A snapshot is the only thing that makes restore mean anything. Without one,
//! "restore the previous default" degrades into guessing that the built-in
//! application was probably selected, which is exactly what Issue #10 forbids.
//!
//! Three properties are load-bearing:
//!
//! - **Nothing is overwritten.** Every write creates a new snapshot file.
//!   Recording that an entry was applied, verified, or restored produces a new
//!   snapshot carrying the other entries forward untouched, so restoring one
//!   component cannot invalidate another component's record and the only known
//!   good baseline is never blindly replaced.
//! - **Damage is reported, not skipped.** A snapshot that will not parse, that
//!   claims a schema this build does not know, or that is missing a required
//!   field comes back in [`SnapshotHistory::damaged`]. A history that quietly
//!   dropped the file it could not read would look like a history with no
//!   baseline, and the difference matters when the next step is a mutation.
//! - **Values are typed.** An entry holds [`better_core::ObservedValue`], not a
//!   command or a rendered string, so putting one back is a write and never an
//!   execution.

use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use better_core::defaults::{DefaultsValue, IntegrationId, ObservedValue};
use better_core::manifest::ComponentId;
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const SNAPSHOT_SCHEMA_VERSION: u32 = 1;

/// A stable identifier for one snapshot.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SnapshotId(String);

impl SnapshotId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// A name that sorts by creation, cannot collide with a snapshot written by
    /// another process in the same second, and cannot collide with the next one
    /// this process writes however fast it writes it.
    pub fn generate(created_at: u64) -> Self {
        static SEQUENCE: AtomicU64 = AtomicU64::new(0);
        let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
        Self(format!("{created_at:020}-{}-{sequence:09}", process::id()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The machine a snapshot was taken on. A snapshot restored onto a different
/// session is not obviously safe, and the reader can only notice that if the
/// snapshot says where it came from.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct SystemIdentity {
    pub distribution: String,
    pub desktop_session: String,
}

/// Whether the value in this entry can still be put back.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RestoreState {
    /// The captured value is definite and can be written back.
    Available,
    /// The setting already holds the captured value.
    AlreadyRestored,
    /// The setting changed after Better Manager last wrote or verified it.
    ChangedExternally,
    /// Nothing definite was captured, so there is nothing to write back.
    NotCaptured,
    /// Putting this value back cannot be automated.
    ManualActionRequired,
}

/// What one integration looked like, and what Better OS did to it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SnapshotEntry {
    pub component_id: ComponentId,
    pub integration_id: IntegrationId,
    /// The effective value read before the first Better OS change.
    pub previous_value: ObservedValue,
    /// The value Better OS wants this setting to hold.
    pub better_value: DefaultsValue,
    /// What Better OS actually wrote, if it wrote anything.
    pub applied_value: Option<DefaultsValue>,
    /// What a verifying read last saw.
    pub last_verified_value: Option<DefaultsValue>,
    pub restore_state: RestoreState,
}

impl SnapshotEntry {
    /// The last value Better Manager knows it is responsible for. Anything else
    /// on the system now came from somewhere else.
    pub fn last_known_value(&self) -> Option<&DefaultsValue> {
        self.last_verified_value
            .as_ref()
            .or(self.applied_value.as_ref())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub schema_version: u32,
    pub snapshot_id: SnapshotId,
    /// Seconds since the Unix epoch. A wall-clock string would need a date
    /// library to write and to read back without ambiguity.
    pub created_at: u64,
    pub system_identity: SystemIdentity,
    pub entries: Vec<SnapshotEntry>,
}

impl Snapshot {
    pub fn new(system_identity: SystemIdentity, entries: Vec<SnapshotEntry>) -> Self {
        let created_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| elapsed.as_secs())
            .unwrap_or_default();
        Self {
            schema_version: SNAPSHOT_SCHEMA_VERSION,
            snapshot_id: SnapshotId::generate(created_at),
            created_at,
            system_identity,
            entries,
        }
    }

    /// The next snapshot in the history: same entries, with the ones named in
    /// `updates` replaced and any new ones appended. Entries this operation did
    /// not touch are carried forward exactly, which is what keeps restoring one
    /// component from disturbing another's record.
    pub fn evolve(&self, updates: Vec<SnapshotEntry>) -> Self {
        let mut entries = self.entries.clone();
        for update in updates {
            match entries.iter_mut().find(|entry| {
                entry.component_id == update.component_id
                    && entry.integration_id == update.integration_id
            }) {
                Some(existing) => *existing = update,
                None => entries.push(update),
            }
        }
        Self::new(self.system_identity.clone(), entries)
    }

    pub fn entry(
        &self,
        component: &ComponentId,
        integration: &IntegrationId,
    ) -> Option<&SnapshotEntry> {
        self.entries
            .iter()
            .find(|entry| &entry.component_id == component && &entry.integration_id == integration)
    }

    fn validate(&self) -> Result<(), Damage> {
        if self.snapshot_id.as_str().trim().is_empty() {
            return Err(Damage::Incomplete {
                field: "snapshot_id",
            });
        }
        if self.created_at == 0 {
            return Err(Damage::Incomplete {
                field: "created_at",
            });
        }
        if self.system_identity.distribution.trim().is_empty()
            || self.system_identity.desktop_session.trim().is_empty()
        {
            return Err(Damage::Incomplete {
                field: "system_identity",
            });
        }
        let mut seen = HashSet::new();
        for entry in &self.entries {
            if !seen.insert((&entry.component_id, &entry.integration_id)) {
                return Err(Damage::Incomplete {
                    field: "entries.duplicate",
                });
            }
        }
        Ok(())
    }
}

/// Why a snapshot on disk could not be used. Every one of these is reported to
/// the caller rather than skipped.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "damage", rename_all = "snake_case")]
pub enum Damage {
    /// The bytes are not a snapshot this build can parse.
    Unreadable { reason: String },
    /// A newer Better OS wrote it. It is preserved, never rewritten.
    UnsupportedSchema { version: u32 },
    /// It parses but does not carry what a snapshot must carry.
    Incomplete { field: &'static str },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DamagedSnapshot {
    pub path: PathBuf,
    pub damage: Damage,
}

/// Every snapshot on disk, and every one that could not be read.
#[derive(Clone, Debug, Default)]
pub struct SnapshotHistory {
    snapshots: Vec<Snapshot>,
    damaged: Vec<DamagedSnapshot>,
}

impl SnapshotHistory {
    /// Oldest first.
    pub fn snapshots(&self) -> &[Snapshot] {
        &self.snapshots
    }

    pub fn damaged(&self) -> &[DamagedSnapshot] {
        &self.damaged
    }

    pub fn latest(&self) -> Option<&Snapshot> {
        self.snapshots.last()
    }

    /// The oldest readable snapshot, which is the baseline the desktop was in
    /// before Better OS touched anything.
    pub fn baseline(&self) -> Option<&Snapshot> {
        self.snapshots.first()
    }

    /// The most recent record for one integration.
    pub fn latest_entry(
        &self,
        component: &ComponentId,
        integration: &IntegrationId,
    ) -> Option<&SnapshotEntry> {
        self.snapshots
            .iter()
            .rev()
            .find_map(|snapshot| snapshot.entry(component, integration))
    }
}

#[derive(Debug, Error)]
pub enum SnapshotError {
    #[error("could not access defaults snapshots at {path}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("snapshot {id} already exists and would be overwritten")]
    WouldOverwrite { id: String },
    #[error("snapshot is not valid: {0:?}")]
    Invalid(Damage),
    #[error("could not serialize a defaults snapshot")]
    Serialize(#[source] serde_json::Error),
}

/// A directory of snapshot files, newest last.
#[derive(Clone, Debug)]
pub struct SnapshotStore {
    directory: PathBuf,
}

impl SnapshotStore {
    pub fn at_path(directory: impl Into<PathBuf>) -> Self {
        Self {
            directory: directory.into(),
        }
    }

    pub fn from_default_path() -> Self {
        let data = match std::env::var("XDG_DATA_HOME") {
            Ok(value) if !value.trim().is_empty() => PathBuf::from(value),
            _ => PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".local/share"),
        };
        Self::at_path(data.join("better-os/defaults/snapshots"))
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }

    /// Writes a new snapshot. An existing file is never replaced: a snapshot id
    /// that is already on disk is refused rather than overwritten.
    pub fn write(&self, snapshot: &Snapshot) -> Result<PathBuf, SnapshotError> {
        snapshot.validate().map_err(SnapshotError::Invalid)?;
        fs::create_dir_all(&self.directory).map_err(|source| SnapshotError::Io {
            path: self.directory.clone(),
            source,
        })?;
        let path = self
            .directory
            .join(format!("{}.json", snapshot.snapshot_id.as_str()));
        if path.exists() {
            return Err(SnapshotError::WouldOverwrite {
                id: snapshot.snapshot_id.as_str().to_string(),
            });
        }
        let body = serde_json::to_vec_pretty(snapshot).map_err(SnapshotError::Serialize)?;
        write_atomically(&path, &body)?;
        Ok(path)
    }

    /// Every snapshot in the directory, oldest first, plus everything that
    /// could not be read.
    pub fn history(&self) -> Result<SnapshotHistory, SnapshotError> {
        let entries = match fs::read_dir(&self.directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(SnapshotHistory::default());
            }
            Err(source) => {
                return Err(SnapshotError::Io {
                    path: self.directory.clone(),
                    source,
                });
            }
        };
        let mut paths: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "json")
            })
            .collect();
        paths.sort();

        let mut history = SnapshotHistory::default();
        for path in paths {
            let bytes = match fs::read(&path) {
                Ok(bytes) => bytes,
                Err(source) => {
                    return Err(SnapshotError::Io { path, source });
                }
            };
            match decode(&bytes) {
                Ok(snapshot) => history.snapshots.push(snapshot),
                Err(damage) => history.damaged.push(DamagedSnapshot { path, damage }),
            }
        }
        history
            .snapshots
            .sort_by_key(|snapshot| snapshot.created_at);
        Ok(history)
    }
}

/// Reads one snapshot. The schema stamp is checked before the body is
/// deserialized, so a file written by a newer Better OS is preserved and
/// reported rather than treated as corruption.
fn decode(bytes: &[u8]) -> Result<Snapshot, Damage> {
    let value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|error| Damage::Unreadable {
            reason: error.to_string(),
        })?;
    let version = value
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
        .ok_or(Damage::Incomplete {
            field: "schema_version",
        })?;
    let version = u32::try_from(version).map_err(|_| Damage::Incomplete {
        field: "schema_version",
    })?;
    if version > SNAPSHOT_SCHEMA_VERSION {
        return Err(Damage::UnsupportedSchema { version });
    }
    // Only one schema version has ever been written. Older versions get their
    // migration arm here, next to the check that refuses newer ones, so a
    // migration can never be added without also deciding what a newer file
    // means.
    let value = match version {
        SNAPSHOT_SCHEMA_VERSION => value,
        other => return Err(Damage::UnsupportedSchema { version: other }),
    };
    let snapshot: Snapshot = serde_json::from_value(value).map_err(|error| Damage::Unreadable {
        reason: error.to_string(),
    })?;
    snapshot.validate()?;
    Ok(snapshot)
}

fn write_atomically(path: &Path, bytes: &[u8]) -> Result<(), SnapshotError> {
    use std::io::Write;

    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or_default();
    let temporary = parent.join(format!(".snapshot.{}-{nonce}.tmp", process::id()));
    let result = (|| -> Result<(), SnapshotError> {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|source| SnapshotError::Io {
                path: temporary.clone(),
                source,
            })?;
        file.write_all(bytes).map_err(|source| SnapshotError::Io {
            path: temporary.clone(),
            source,
        })?;
        file.write_all(b"\n").map_err(|source| SnapshotError::Io {
            path: temporary.clone(),
            source,
        })?;
        file.sync_all().map_err(|source| SnapshotError::Io {
            path: temporary.clone(),
            source,
        })?;
        fs::rename(&temporary, path).map_err(|source| SnapshotError::Io {
            path: path.to_path_buf(),
            source,
        })
    })();
    if result.is_err() && temporary.exists() {
        let _ = fs::remove_file(&temporary);
    }
    result
}
