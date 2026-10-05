//! Writing "Always use for this file type", removing a type's default, and
//! taking either back.
//!
//! The order of operations is the point of this module. The rollback record is
//! written and flushed *before* `mimeapps.list` is opened for writing, so a
//! crash between the two leaves a record that describes a change that never
//! happened — recoverable — rather than a change with no record, which is not.
//!
//! Restoring puts the previous line back verbatim. When Better OS created the
//! file or the group, restoring removes exactly what it added, which is why
//! clearing Better OS state cannot destroy an association it did not create.

use std::io::Write;
use std::path::{Path, PathBuf};

use app_catalog_core::{ApplicationRecord, DesktopId, MimeType};
use serde::{Deserialize, Serialize};

use crate::mimeapps::{DEFAULT_APPLICATIONS, DefaultChange, Line, MimeAppsFile, RemovedLine};

/// The rollback record format. A future version is refused rather than
/// misread.
pub const ROLLBACK_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, thiserror::Error)]
pub enum AssociationError {
    #[error("could not read {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not write {path}: {source}")]
    Write {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("{path} is not valid UTF-8 and will not be rewritten")]
    NotUtf8 { path: PathBuf },
    #[error("rollback record {path} is not readable: {source}")]
    RollbackUnreadable {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("rollback record schema version {found} is not supported")]
    RollbackSchema { found: u32 },
    #[error("{path} no longer matches the rollback record, so nothing was changed")]
    RollbackStale { path: PathBuf },
    #[error("no home directory is set, so the per-user association file cannot be located")]
    NoHomeDirectory,
}

/// Something worth telling the user about a change that still went ahead.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AssociationWarning {
    /// The application does not declare the MIME type. The choice is honored,
    /// and the UI is expected to say so in the user's own words.
    ApplicationDoesNotDeclareType,
    /// The same type and application appear in `[Removed Associations]`, which
    /// may keep the new default from taking effect. Better OS does not quietly
    /// edit a second line to fix it.
    ListedInRemovedAssociations,
    /// The file lists this type more than once in `[Default Applications]`.
    /// Setting a default changed the first entry, because that is the one that
    /// wins, and left the rest alone. Removing a default removed every entry,
    /// because a later one left behind would become the default.
    DuplicateDefaultKey,
}

/// The previous state of the one line a change touched.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PreviousDefault {
    /// Nothing to undo.
    Unchanged,
    /// A line existed and read exactly this.
    Line {
        index: usize,
        previous: Line,
        applied: String,
    },
    /// Better OS added these lines; undoing removes them.
    Inserted {
        index: usize,
        texts: Vec<String>,
        created_group: bool,
        fixed_final_newline: bool,
    },
    /// Better OS removed these lines; undoing puts them back where they were.
    Removed { lines: Vec<RemovedLine> },
}

impl PreviousDefault {
    fn from_change(change: DefaultChange) -> Self {
        match change {
            DefaultChange::Unchanged => Self::Unchanged,
            DefaultChange::Replaced {
                index,
                previous,
                applied,
            } => Self::Line {
                index,
                previous,
                applied,
            },
            DefaultChange::Inserted {
                index,
                texts,
                created_group,
                fixed_final_newline,
            } => Self::Inserted {
                index,
                texts,
                created_group,
                fixed_final_newline,
            },
            DefaultChange::Removed { lines } => Self::Removed { lines },
        }
    }

    fn to_change(&self) -> DefaultChange {
        match self {
            Self::Unchanged => DefaultChange::Unchanged,
            Self::Line {
                index,
                previous,
                applied,
            } => DefaultChange::Replaced {
                index: *index,
                previous: previous.clone(),
                applied: applied.clone(),
            },
            Self::Inserted {
                index,
                texts,
                created_group,
                fixed_final_newline,
            } => DefaultChange::Inserted {
                index: *index,
                texts: texts.clone(),
                created_group: *created_group,
                fixed_final_newline: *fixed_final_newline,
            },
            Self::Removed { lines } => DefaultChange::Removed {
                lines: lines.clone(),
            },
        }
    }
}

/// Everything needed to undo one association change.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AssociationRollback {
    pub schema_version: u32,
    pub target: PathBuf,
    pub mime_type: String,
    /// The application the change made the default, or nothing for a change
    /// that removed the type's default.
    #[serde(default)]
    pub applied_desktop_id: Option<String>,
    /// Whether the file existed before the change. When it did not, restoring
    /// removes the file Better OS created rather than leaving an empty one.
    pub file_existed: bool,
    pub previous: PreviousDefault,
}

impl AssociationRollback {
    /// Whether applying this record would change anything.
    pub fn is_noop(&self) -> bool {
        matches!(self.previous, PreviousDefault::Unchanged)
    }
}

/// The outcome of an Always Use or a removal.
#[derive(Clone, Debug)]
pub struct AssociationOutcome {
    pub rollback: AssociationRollback,
    /// Where the rollback record was written. Always populated, including for a
    /// change that turned out to be a no-op, so a caller never has to guess.
    pub rollback_path: PathBuf,
    pub changed: bool,
    pub warnings: Vec<AssociationWarning>,
}

/// Reads and writes the user's MIME associations.
#[derive(Clone, Debug)]
pub struct AssociationStore {
    path: PathBuf,
    rollback_dir: PathBuf,
}

impl AssociationStore {
    pub fn new(path: PathBuf, rollback_dir: PathBuf) -> Self {
        Self { path, rollback_dir }
    }

    /// The per-user file the XDG mime-apps specification names, with rollback
    /// records kept in Better OS's own data directory.
    pub fn for_user() -> Result<Self, AssociationError> {
        let config = match std::env::var("XDG_CONFIG_HOME") {
            Ok(value) if !value.trim().is_empty() => PathBuf::from(value),
            _ => home()?.join(".config"),
        };
        let data = match std::env::var("XDG_DATA_HOME") {
            Ok(value) if !value.trim().is_empty() => PathBuf::from(value),
            _ => home()?.join(".local/share"),
        };
        Ok(Self::new(
            config.join("mimeapps.list"),
            data.join("better-os/app-chooser/rollback"),
        ))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn rollback_dir(&self) -> &Path {
        &self.rollback_dir
    }

    /// Reads the file. A missing file is an empty one, not an error.
    pub fn load(&self) -> Result<MimeAppsFile, AssociationError> {
        match std::fs::read(&self.path) {
            Ok(bytes) => {
                let text = String::from_utf8(bytes).map_err(|_| AssociationError::NotUtf8 {
                    path: self.path.clone(),
                })?;
                Ok(MimeAppsFile::parse(&text))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(MimeAppsFile::parse(""))
            }
            Err(source) => Err(AssociationError::Read {
                path: self.path.clone(),
                source,
            }),
        }
    }

    /// Makes `record` the user's default for `mime`, touching that one
    /// association and nothing else.
    pub fn set_default(
        &self,
        mime: &MimeType,
        record: &ApplicationRecord,
    ) -> Result<AssociationOutcome, AssociationError> {
        let mut warnings = Vec::new();
        if !record.supports_mime_type(mime) {
            warnings.push(AssociationWarning::ApplicationDoesNotDeclareType);
        }
        self.set_default_id(mime, &record.desktop_id, warnings)
    }

    /// The same change addressed by desktop ID, for callers that already hold
    /// an identity rather than a record.
    pub fn set_default_id(
        &self,
        mime: &MimeType,
        desktop_id: &DesktopId,
        mut warnings: Vec<AssociationWarning>,
    ) -> Result<AssociationOutcome, AssociationError> {
        let file_existed = self.path.exists();
        let mut file = self.load()?;
        let associations = file.associations();
        if associations.is_removed(mime, desktop_id) {
            warnings.push(AssociationWarning::ListedInRemovedAssociations);
        }
        if file.count_keys(DEFAULT_APPLICATIONS, mime.as_str()) > 1 {
            warnings.push(AssociationWarning::DuplicateDefaultKey);
        }

        let change = file.set_default(mime, desktop_id);
        self.commit(
            mime,
            Some(desktop_id),
            file_existed,
            &file,
            change,
            warnings,
        )
    }

    /// Removes the user's default for `mime`, so the type has no default
    /// application in this file. Every other line, comment, and group is left
    /// exactly as it was, and the rollback record is written first, as for
    /// [`Self::set_default`]. A file that does not exist is not created.
    pub fn remove_default(&self, mime: &MimeType) -> Result<AssociationOutcome, AssociationError> {
        let file_existed = self.path.exists();
        let mut file = self.load()?;
        let change = file.remove_default(mime);
        let mut warnings = Vec::new();
        if matches!(&change, DefaultChange::Removed { lines } if lines.len() > 1) {
            warnings.push(AssociationWarning::DuplicateDefaultKey);
        }
        self.commit(mime, None, file_existed, &file, change, warnings)
    }

    /// Records a change and then makes it. The record goes to disk first: a
    /// crash after that is recoverable, and a crash after the mutation with no
    /// record is not.
    fn commit(
        &self,
        mime: &MimeType,
        applied: Option<&DesktopId>,
        file_existed: bool,
        file: &MimeAppsFile,
        change: DefaultChange,
        warnings: Vec<AssociationWarning>,
    ) -> Result<AssociationOutcome, AssociationError> {
        let changed = !matches!(change, DefaultChange::Unchanged);
        let rollback = AssociationRollback {
            schema_version: ROLLBACK_SCHEMA_VERSION,
            target: self.path.clone(),
            mime_type: mime.as_str().to_string(),
            applied_desktop_id: applied.map(|id| id.as_str().to_string()),
            file_existed,
            previous: PreviousDefault::from_change(change),
        };
        let rollback_path = self.write_rollback(&rollback)?;
        if changed {
            write_atomically(&self.path, file.render().as_bytes())?;
        }
        Ok(AssociationOutcome {
            rollback,
            rollback_path,
            changed,
            warnings,
        })
    }

    /// Puts back what a rollback record describes.
    pub fn restore(&self, rollback: &AssociationRollback) -> Result<(), AssociationError> {
        if rollback.schema_version != ROLLBACK_SCHEMA_VERSION {
            return Err(AssociationError::RollbackSchema {
                found: rollback.schema_version,
            });
        }
        if matches!(rollback.previous, PreviousDefault::Unchanged) {
            return Ok(());
        }
        if !rollback.file_existed {
            // Better OS created the file. Removing it returns the user to
            // having no per-user associations at all, which is where they were.
            match std::fs::remove_file(&rollback.target) {
                Ok(()) => return Ok(()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
                Err(source) => {
                    return Err(AssociationError::Write {
                        path: rollback.target.clone(),
                        source,
                    });
                }
            }
        }
        let text =
            std::fs::read_to_string(&rollback.target).map_err(|source| AssociationError::Read {
                path: rollback.target.clone(),
                source,
            })?;
        let mut file = MimeAppsFile::parse(&text);
        if !file.undo(&rollback.previous.to_change()) {
            return Err(AssociationError::RollbackStale {
                path: rollback.target.clone(),
            });
        }
        write_atomically(&rollback.target, file.render().as_bytes())
    }

    /// Reads a rollback record written earlier.
    pub fn read_rollback(path: &Path) -> Result<AssociationRollback, AssociationError> {
        let text = std::fs::read_to_string(path).map_err(|source| AssociationError::Read {
            path: path.to_path_buf(),
            source,
        })?;
        let rollback: AssociationRollback =
            serde_json::from_str(&text).map_err(|source| AssociationError::RollbackUnreadable {
                path: path.to_path_buf(),
                source,
            })?;
        if rollback.schema_version != ROLLBACK_SCHEMA_VERSION {
            return Err(AssociationError::RollbackSchema {
                found: rollback.schema_version,
            });
        }
        Ok(rollback)
    }

    /// Every rollback record on disk, oldest name first.
    pub fn rollback_records(&self) -> Result<Vec<PathBuf>, AssociationError> {
        let entries = match std::fs::read_dir(&self.rollback_dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(source) => {
                return Err(AssociationError::Read {
                    path: self.rollback_dir.clone(),
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
        Ok(paths)
    }

    fn write_rollback(&self, rollback: &AssociationRollback) -> Result<PathBuf, AssociationError> {
        std::fs::create_dir_all(&self.rollback_dir).map_err(|source| AssociationError::Write {
            path: self.rollback_dir.clone(),
            source,
        })?;
        let path = self.rollback_dir.join(rollback_file_name(rollback));
        let body = serde_json::to_string_pretty(rollback)
            .expect("a rollback record is always serializable");
        write_atomically(&path, body.as_bytes())?;
        Ok(path)
    }
}

/// A stable, filesystem-safe name that keeps successive changes to the same
/// type from overwriting each other's records.
fn rollback_file_name(rollback: &AssociationRollback) -> String {
    let mime: String = rollback
        .mime_type
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '-'
            }
        })
        .collect();
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or_default();
    format!("{mime}-{stamp}.json")
}

fn home() -> Result<PathBuf, AssociationError> {
    std::env::var("HOME")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
        .ok_or(AssociationError::NoHomeDirectory)
}

/// Writes through a temporary file in the same directory and renames, so a
/// crash mid-write cannot leave a half-written association file.
fn write_atomically(path: &Path, bytes: &[u8]) -> Result<(), AssociationError> {
    let parent = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent).map_err(|source| AssociationError::Write {
        path: parent.to_path_buf(),
        source,
    })?;
    let temporary = parent.join(format!(
        ".{}.better-os.tmp",
        path.file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "mimeapps.list".to_string())
    ));
    let write = || -> std::io::Result<()> {
        let mut handle = std::fs::File::create(&temporary)?;
        handle.write_all(bytes)?;
        handle.sync_all()?;
        Ok(())
    };
    write().map_err(|source| AssociationError::Write {
        path: temporary.clone(),
        source,
    })?;
    std::fs::rename(&temporary, path).map_err(|source| {
        let _ = std::fs::remove_file(&temporary);
        AssociationError::Write {
            path: path.to_path_buf(),
            source,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use app_catalog_core::{DesktopFile, EntryScope, NoProbe};

    fn mime(value: &str) -> MimeType {
        MimeType::parse(value).expect("valid mime type")
    }

    fn id(value: &str) -> DesktopId {
        DesktopId::new(value).expect("valid desktop id")
    }

    fn record(desktop_id: &str, mime_types: &str) -> ApplicationRecord {
        let body = format!(
            "[Desktop Entry]\nType=Application\nName=App\nExec=app %U\nMimeType={mime_types}\n"
        );
        let file = DesktopFile::parse(&body).expect("valid entry");
        ApplicationRecord::from_desktop_file(
            id(desktop_id),
            PathBuf::from(format!("/usr/share/applications/{desktop_id}")),
            EntryScope::System,
            &file,
            &NoProbe,
        )
        .expect("valid record")
    }

    struct Fixture {
        _dir: tempfile::TempDir,
        store: AssociationStore,
    }

    fn fixture(contents: Option<&str>) -> Fixture {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("mimeapps.list");
        if let Some(contents) = contents {
            std::fs::write(&path, contents).expect("write fixture");
        }
        let store = AssociationStore::new(path, dir.path().join("rollback"));
        Fixture { _dir: dir, store }
    }

    const ORIGINAL: &str = "# user's own file\n\
         [Added Associations]\n\
         text/plain=vim.desktop;\n\
         \n\
         [Default Applications]\n\
         image/png=eog.desktop\n\
         text/html=firefox.desktop\n\
         application/pdf=evince.desktop\n";

    #[test]
    fn always_use_changes_one_line_and_leaves_every_other_association_alone() {
        let fixture = fixture(Some(ORIGINAL));
        let outcome = fixture
            .store
            .set_default(&mime("image/png"), &record("gimp.desktop", "image/png;"))
            .expect("set default");
        assert!(outcome.changed);
        assert!(outcome.warnings.is_empty());

        let after = std::fs::read_to_string(fixture.store.path()).expect("read back");
        let before_lines: Vec<&str> = ORIGINAL.lines().collect();
        let after_lines: Vec<&str> = after.lines().collect();
        assert_eq!(before_lines.len(), after_lines.len());
        let changed: Vec<(usize, &str)> = before_lines
            .iter()
            .zip(&after_lines)
            .enumerate()
            .filter(|(_, (left, right))| left != right)
            .map(|(index, (_, right))| (index, *right))
            .collect();
        assert_eq!(changed.len(), 1, "exactly one line may change");
        assert_eq!(changed[0].1, "image/png=gimp.desktop");

        let associations = fixture.store.load().expect("reload").associations();
        assert_eq!(
            associations.default_for(&mime("text/html")),
            Some(&id("firefox.desktop"))
        );
        assert_eq!(
            associations.default_for(&mime("application/pdf")),
            Some(&id("evince.desktop"))
        );
        assert_eq!(
            associations.added_for(&mime("text/plain")),
            &[id("vim.desktop")]
        );
    }

    #[test]
    fn the_rollback_record_is_written_before_the_file_changes() {
        let fixture = fixture(Some(ORIGINAL));
        let outcome = fixture
            .store
            .set_default(&mime("image/png"), &record("gimp.desktop", "image/png;"))
            .expect("set default");
        let stored =
            AssociationStore::read_rollback(&outcome.rollback_path).expect("read rollback record");
        assert_eq!(stored, outcome.rollback);
        assert_eq!(stored.applied_desktop_id.as_deref(), Some("gimp.desktop"));
        assert!(stored.file_existed);
        assert!(matches!(stored.previous, PreviousDefault::Line { .. }));
    }

    #[test]
    fn a_rollback_restores_the_file_byte_for_byte() {
        let fixture = fixture(Some(ORIGINAL));
        let outcome = fixture
            .store
            .set_default(&mime("image/png"), &record("gimp.desktop", "image/png;"))
            .expect("set default");
        fixture.store.restore(&outcome.rollback).expect("restore");
        assert_eq!(
            std::fs::read_to_string(fixture.store.path()).expect("read back"),
            ORIGINAL
        );
    }

    #[test]
    fn a_rollback_of_a_newly_added_type_restores_the_file_byte_for_byte() {
        let fixture = fixture(Some(ORIGINAL));
        let outcome = fixture
            .store
            .set_default(&mime("text/x-rust"), &record("zed.desktop", "text/x-rust;"))
            .expect("set default");
        assert!(
            std::fs::read_to_string(fixture.store.path())
                .expect("read back")
                .contains("text/x-rust=zed.desktop")
        );
        fixture.store.restore(&outcome.rollback).expect("restore");
        assert_eq!(
            std::fs::read_to_string(fixture.store.path()).expect("read back"),
            ORIGINAL
        );
    }

    #[test]
    fn a_file_better_os_created_is_removed_by_its_rollback() {
        let fixture = fixture(None);
        let outcome = fixture
            .store
            .set_default(&mime("text/plain"), &record("nano.desktop", "text/plain;"))
            .expect("set default");
        assert!(fixture.store.path().exists());
        assert!(!outcome.rollback.file_existed);
        fixture.store.restore(&outcome.rollback).expect("restore");
        assert!(
            !fixture.store.path().exists(),
            "a file Better OS created must not survive its own rollback"
        );
    }

    #[test]
    fn choosing_an_application_that_does_not_declare_the_type_still_works_and_warns() {
        let fixture = fixture(Some(ORIGINAL));
        let outcome = fixture
            .store
            .set_default(&mime("image/png"), &record("nano.desktop", "text/plain;"))
            .expect("set default");
        assert!(outcome.changed);
        assert_eq!(
            outcome.warnings,
            vec![AssociationWarning::ApplicationDoesNotDeclareType]
        );
    }

    #[test]
    fn a_removed_association_is_reported_rather_than_quietly_edited() {
        let original = "[Default Applications]\nimage/png=eog.desktop\n\n[Removed Associations]\nimage/png=gimp.desktop\n";
        let fixture = fixture(Some(original));
        let outcome = fixture
            .store
            .set_default(&mime("image/png"), &record("gimp.desktop", "image/png;"))
            .expect("set default");
        assert!(
            outcome
                .warnings
                .contains(&AssociationWarning::ListedInRemovedAssociations)
        );
        let after = std::fs::read_to_string(fixture.store.path()).expect("read back");
        assert!(
            after.contains("[Removed Associations]\nimage/png=gimp.desktop"),
            "the second association must not be edited behind the user's back"
        );
    }

    #[test]
    fn a_duplicate_default_key_is_reported_and_only_the_winning_line_changes() {
        let original = "[Default Applications]\nimage/png=eog.desktop\nimage/png=second.desktop\n";
        let fixture = fixture(Some(original));
        let outcome = fixture
            .store
            .set_default(&mime("image/png"), &record("gimp.desktop", "image/png;"))
            .expect("set default");
        assert!(
            outcome
                .warnings
                .contains(&AssociationWarning::DuplicateDefaultKey)
        );
        assert_eq!(
            std::fs::read_to_string(fixture.store.path()).expect("read back"),
            "[Default Applications]\nimage/png=gimp.desktop\nimage/png=second.desktop\n"
        );
    }

    #[test]
    fn setting_the_association_that_is_already_there_writes_no_change_but_still_records_one() {
        let fixture = fixture(Some(ORIGINAL));
        let outcome = fixture
            .store
            .set_default(&mime("image/png"), &record("eog.desktop", "image/png;"))
            .expect("set default");
        assert!(!outcome.changed);
        assert!(outcome.rollback.is_noop());
        assert!(outcome.rollback_path.exists());
        assert_eq!(
            std::fs::read_to_string(fixture.store.path()).expect("read back"),
            ORIGINAL
        );
    }

    #[test]
    fn a_rollback_against_a_file_someone_else_changed_refuses_rather_than_guessing() {
        let fixture = fixture(Some(ORIGINAL));
        let outcome = fixture
            .store
            .set_default(&mime("image/png"), &record("gimp.desktop", "image/png;"))
            .expect("set default");
        fixture
            .store
            .set_default(&mime("image/png"), &record("other.desktop", "image/png;"))
            .expect("second change");
        let error = fixture
            .store
            .restore(&outcome.rollback)
            .expect_err("a stale rollback must refuse");
        assert!(matches!(error, AssociationError::RollbackStale { .. }));
        assert!(
            std::fs::read_to_string(fixture.store.path())
                .expect("read back")
                .contains("image/png=other.desktop")
        );
    }

    #[test]
    fn a_rollback_record_from_a_future_schema_is_refused() {
        let fixture = fixture(Some(ORIGINAL));
        let dir = fixture.store.rollback_dir().to_path_buf();
        std::fs::create_dir_all(&dir).expect("create dir");
        let path = dir.join("future.json");
        std::fs::write(
            &path,
            r#"{"schema_version":99,"target":"/tmp/x","mime_type":"text/plain","applied_desktop_id":"a.desktop","file_existed":true,"previous":{"kind":"unchanged"}}"#,
        )
        .expect("write");
        assert!(matches!(
            AssociationStore::read_rollback(&path),
            Err(AssociationError::RollbackSchema { found: 99 })
        ));
    }

    #[test]
    fn rollback_records_are_listed_and_reread() {
        let fixture = fixture(Some(ORIGINAL));
        fixture
            .store
            .set_default(&mime("image/png"), &record("gimp.desktop", "image/png;"))
            .expect("set default");
        let records = fixture.store.rollback_records().expect("list");
        assert_eq!(records.len(), 1);
        let reread = AssociationStore::read_rollback(&records[0]).expect("reread");
        assert_eq!(reread.mime_type, "image/png");
    }

    #[test]
    fn a_file_that_is_not_utf8_is_refused_rather_than_rewritten() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("mimeapps.list");
        std::fs::write(&path, [0x66, 0x6f, 0xff, 0x6f]).expect("write");
        let store = AssociationStore::new(path, dir.path().join("rollback"));
        assert!(matches!(
            store.load(),
            Err(AssociationError::NotUtf8 { .. })
        ));
    }

    #[test]
    fn removing_a_default_takes_out_one_line_and_its_rollback_puts_it_back_byte_for_byte() {
        let fixture = fixture(Some(ORIGINAL));
        let outcome = fixture
            .store
            .remove_default(&mime("text/html"))
            .expect("remove default");
        assert!(outcome.changed);
        assert!(outcome.warnings.is_empty());
        assert_eq!(
            std::fs::read_to_string(fixture.store.path()).expect("read back"),
            ORIGINAL.replace("text/html=firefox.desktop\n", "")
        );
        let associations = fixture.store.load().expect("reload").associations();
        assert!(associations.default_for(&mime("text/html")).is_none());
        assert_eq!(
            associations.default_for(&mime("image/png")),
            Some(&id("eog.desktop"))
        );

        let stored =
            AssociationStore::read_rollback(&outcome.rollback_path).expect("read rollback record");
        assert_eq!(stored, outcome.rollback);
        assert_eq!(stored.mime_type, "text/html");
        assert_eq!(stored.applied_desktop_id, None);
        assert!(matches!(stored.previous, PreviousDefault::Removed { .. }));

        fixture.store.restore(&stored).expect("restore");
        assert_eq!(
            std::fs::read_to_string(fixture.store.path()).expect("read back"),
            ORIGINAL
        );
    }

    #[test]
    fn a_removal_writes_its_rollback_record_before_it_touches_the_file() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().expect("temp dir");
        let config = dir.path().join("config");
        std::fs::create_dir_all(&config).expect("config dir");
        let path = config.join("mimeapps.list");
        std::fs::write(&path, ORIGINAL).expect("write fixture");
        let store = AssociationStore::new(path.clone(), dir.path().join("rollback"));
        // The file's directory refuses the write, so only what happened before
        // it is left to look at.
        std::fs::set_permissions(&config, std::fs::Permissions::from_mode(0o555)).expect("chmod");
        let result = store.remove_default(&mime("text/html"));
        std::fs::set_permissions(&config, std::fs::Permissions::from_mode(0o755)).expect("chmod");

        assert!(matches!(result, Err(AssociationError::Write { .. })));
        assert_eq!(store.rollback_records().expect("list").len(), 1);
        assert_eq!(std::fs::read_to_string(&path).expect("read back"), ORIGINAL);
    }

    #[test]
    fn removing_from_a_file_that_does_not_exist_creates_nothing_and_its_rollback_deletes_nothing() {
        let fixture = fixture(None);
        let outcome = fixture
            .store
            .remove_default(&mime("text/html"))
            .expect("remove default");
        assert!(!outcome.changed);
        assert!(outcome.rollback.is_noop());
        assert!(!fixture.store.path().exists());

        // A file somebody writes afterwards is not Better OS's to delete.
        std::fs::write(fixture.store.path(), ORIGINAL).expect("write");
        fixture.store.restore(&outcome.rollback).expect("restore");
        assert_eq!(
            std::fs::read_to_string(fixture.store.path()).expect("read back"),
            ORIGINAL
        );
    }

    #[test]
    fn removing_a_repeated_default_is_reported_and_removes_every_copy() {
        let original = "[Default Applications]\nimage/png=eog.desktop\nimage/png=second.desktop\n";
        let fixture = fixture(Some(original));
        let outcome = fixture
            .store
            .remove_default(&mime("image/png"))
            .expect("remove default");
        assert_eq!(
            outcome.warnings,
            vec![AssociationWarning::DuplicateDefaultKey]
        );
        assert_eq!(
            std::fs::read_to_string(fixture.store.path()).expect("read back"),
            "[Default Applications]\n"
        );
        fixture.store.restore(&outcome.rollback).expect("restore");
        assert_eq!(
            std::fs::read_to_string(fixture.store.path()).expect("read back"),
            original
        );
    }

    #[test]
    fn a_removal_rollback_against_a_file_someone_else_changed_refuses_rather_than_guessing() {
        let fixture = fixture(Some(ORIGINAL));
        let outcome = fixture
            .store
            .remove_default(&mime("text/html"))
            .expect("remove default");
        fixture
            .store
            .set_default(&mime("text/html"), &record("other.desktop", "text/html;"))
            .expect("second change");
        let error = fixture
            .store
            .restore(&outcome.rollback)
            .expect_err("a stale rollback must refuse");
        assert!(matches!(error, AssociationError::RollbackStale { .. }));
        assert!(
            std::fs::read_to_string(fixture.store.path())
                .expect("read back")
                .contains("text/html=other.desktop")
        );
    }

    #[test]
    fn a_rollback_record_written_before_removal_existed_still_reads() {
        let fixture = fixture(Some(ORIGINAL));
        let dir = fixture.store.rollback_dir().to_path_buf();
        std::fs::create_dir_all(&dir).expect("create dir");
        let path = dir.join("older.json");
        std::fs::write(
            &path,
            r#"{"schema_version":1,"target":"/tmp/x","mime_type":"text/plain","applied_desktop_id":"a.desktop","file_existed":true,"previous":{"kind":"unchanged"}}"#,
        )
        .expect("write");
        let record = AssociationStore::read_rollback(&path).expect("an older record reads");
        assert_eq!(record.applied_desktop_id.as_deref(), Some("a.desktop"));
    }
}
