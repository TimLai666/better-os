//! What a job was asked to do.
//!
//! A spec is data. It carries no closure, no command string, and no handle to
//! a window, which is what lets the engine own a job that outlives whoever
//! submitted it. Issue #6's "no shell-string concatenation" rule is not a
//! review item here: there is nowhere in this type to put a command line.
//!
//! Names are [`OsString`], not `String`. A file called `caf\xe9.txt` written
//! by a Latin-1 tool is a real file on a real disk, and turning it into a
//! `String` on the way in would either lose it or rename it. Nothing in the
//! operation path performs a lossy conversion; only error rendering does.

use std::ffi::{OsStr, OsString};
use std::path::PathBuf;

use files_core::location::LocalPath;

use crate::error::OperationError;
use crate::policy::CopyPolicy;

/// The proof that a user was asked before something became unrecoverable.
///
/// It has no `Deserialize`, no `Default`, and no public field, so the only way
/// to get one is to call [`DeleteConfirmation::explicit`] in response to a real
/// confirmation. A persisted job cannot resurrect one, which is the point: a
/// permanent delete recovered from disk after a crash cannot silently continue.
/// This mirrors `storage_core`'s readiness proof, which cannot be deserialized
/// for the same reason.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DeleteConfirmation(());

impl DeleteConfirmation {
    /// Records that the user confirmed this specific permanent deletion.
    pub fn explicit() -> Self {
        Self(())
    }
}

/// A reference to one item sitting in a trash directory.
///
/// The trash root travels with the item because there is more than one trash:
/// the home trash and a `.Trash-<uid>` on each mounted device. An item cannot
/// be restored or purged without knowing which one it is in.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TrashItemRef {
    pub trash_root: PathBuf,
    /// The `.trashinfo` stem, exactly as `files_platform::read_trash` reported
    /// it in `TrashedFacts::item`.
    pub item: String,
}

impl TrashItemRef {
    pub fn new(trash_root: impl Into<PathBuf>, item: impl Into<String>) -> Self {
        Self {
            trash_root: trash_root.into(),
            item: item.into(),
        }
    }
}

/// What a permanent delete is aimed at.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DeleteTarget {
    /// A path in the filesystem, deleted without passing through the trash.
    Path(LocalPath),
    /// An item already in a trash, emptied from it.
    TrashItem(TrashItemRef),
}

/// How a bulk rename builds each new name.
///
/// Both halves work on bytes. A find-and-replace over a name that is not valid
/// UTF-8 replaces what it finds and leaves the rest of the bytes untouched,
/// where a `String` round trip would have replaced the invalid bytes with
/// replacement characters and renamed the file to something else entirely.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RenamePattern {
    /// The byte sequence to replace. Empty means no find-and-replace step.
    pub find: OsString,
    pub replace: OsString,
    /// A template applied after the replacement, with `{name}` for the stem,
    /// `{ext}` for the extension including its dot, and `{n}` for the counter.
    /// `None` leaves the replaced name as it is.
    pub template: Option<String>,
    pub start: u64,
    pub step: u64,
    /// Zero-padded width of `{n}`. `3` gives `001`.
    pub width: usize,
}

impl Default for RenamePattern {
    fn default() -> Self {
        Self {
            find: OsString::new(),
            replace: OsString::new(),
            template: None,
            start: 1,
            step: 1,
            width: 1,
        }
    }
}

impl RenamePattern {
    /// A pure find-and-replace over every selected name.
    pub fn replacing(find: impl Into<OsString>, replace: impl Into<OsString>) -> Self {
        Self {
            find: find.into(),
            replace: replace.into(),
            ..Self::default()
        }
    }

    /// A numbering pattern: `template` must contain `{n}` to be worth using.
    pub fn numbering(template: impl Into<String>, start: u64, width: usize) -> Self {
        Self {
            template: Some(template.into()),
            start,
            width,
            ..Self::default()
        }
    }

    /// The new name for the item at `index` in the job's order.
    ///
    /// Returns `None` when the result would not be a usable filename — empty,
    /// `.`, `..`, or carrying a separator — so a pattern that would rename a
    /// file into another directory is refused rather than executed.
    pub fn apply(&self, original: &OsStr, index: u64) -> Option<OsString> {
        use std::os::unix::ffi::{OsStrExt, OsStringExt};

        let replaced = if self.find.is_empty() {
            original.as_bytes().to_vec()
        } else {
            replace_bytes(
                original.as_bytes(),
                self.find.as_bytes(),
                self.replace.as_bytes(),
            )
        };

        let bytes = match &self.template {
            None => replaced,
            Some(template) => {
                let split = replaced
                    .iter()
                    .rposition(|byte| *byte == b'.')
                    .filter(|index| *index > 0);
                let (stem, extension) = match split {
                    Some(at) => (&replaced[..at], &replaced[at..]),
                    None => (&replaced[..], &[][..]),
                };
                let counter = self.start + index.saturating_mul(self.step);
                let counter = format!("{counter:0width$}", width = self.width.max(1));
                let mut out = Vec::with_capacity(template.len() + replaced.len() + 8);
                let mut rest = template.as_bytes();
                while !rest.is_empty() {
                    if let Some(tail) = rest.strip_prefix(b"{name}".as_slice()) {
                        out.extend_from_slice(stem);
                        rest = tail;
                    } else if let Some(tail) = rest.strip_prefix(b"{ext}".as_slice()) {
                        out.extend_from_slice(extension);
                        rest = tail;
                    } else if let Some(tail) = rest.strip_prefix(b"{n}".as_slice()) {
                        out.extend_from_slice(counter.as_bytes());
                        rest = tail;
                    } else {
                        out.push(rest[0]);
                        rest = &rest[1..];
                    }
                }
                out
            }
        };

        let name = OsString::from_vec(bytes);
        if is_usable_name(&name) {
            Some(name)
        } else {
            None
        }
    }
}

fn replace_bytes(haystack: &[u8], needle: &[u8], replacement: &[u8]) -> Vec<u8> {
    if needle.is_empty() || needle.len() > haystack.len() {
        return haystack.to_vec();
    }
    let mut out = Vec::with_capacity(haystack.len());
    let mut index = 0;
    while index <= haystack.len() - needle.len() {
        if &haystack[index..index + needle.len()] == needle {
            out.extend_from_slice(replacement);
            index += needle.len();
        } else {
            out.push(haystack[index]);
            index += 1;
        }
    }
    out.extend_from_slice(&haystack[index..]);
    out
}

/// Whether a byte string is usable as a single filename.
pub fn is_usable_name(name: &OsStr) -> bool {
    use std::os::unix::ffi::OsStrExt;
    let bytes = name.as_bytes();
    !bytes.is_empty()
        && bytes != b"."
        && bytes != b".."
        && !bytes.contains(&b'/')
        && !bytes.contains(&0)
        // Linux caps a single component at NAME_MAX, which is 255 on every
        // filesystem Better OS supports. Refusing here gives a named error
        // instead of an ENAMETOOLONG surprise partway through a bulk rename.
        && bytes.len() <= 255
}

/// What the job does.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Operation {
    CreateFile {
        parent: LocalPath,
        name: OsString,
    },
    CreateFolder {
        parent: LocalPath,
        name: OsString,
    },
    Rename {
        path: LocalPath,
        new_name: OsString,
    },
    /// Pattern-based renaming across a selection. Out of scope for ticket 33's
    /// acceptance list, in scope for Issue #6's operation list, and cheap
    /// enough once single rename exists that leaving it out would have meant
    /// leaving the pattern engine untested until ticket 35.
    BulkRename {
        targets: Vec<LocalPath>,
        pattern: RenamePattern,
    },
    Copy {
        sources: Vec<LocalPath>,
        destination: LocalPath,
    },
    Move {
        sources: Vec<LocalPath>,
        destination: LocalPath,
    },
    /// Copy each source beside itself under a generated name.
    Duplicate {
        sources: Vec<LocalPath>,
    },
    Trash {
        sources: Vec<LocalPath>,
        /// Which trash directory to use. `None` means the home trash resolved
        /// from the environment, which is what a desktop wants. Naming one
        /// explicitly is how a caller targets a device's own trash, and how the
        /// tests avoid depending on the process environment.
        trash_root: Option<PathBuf>,
    },
    RestoreFromTrash {
        items: Vec<TrashItemRef>,
    },
    PermanentDelete {
        targets: Vec<DeleteTarget>,
        confirmation: DeleteConfirmation,
    },
    Checksum {
        targets: Vec<LocalPath>,
        algorithm: ChecksumAlgorithm,
    },
}

/// Which digest a checksum job computes.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChecksumAlgorithm {
    #[default]
    Sha256,
}

/// The operation's kind, without its arguments.
///
/// This is what a persisted record stores and what the operation centre shows.
/// It deliberately cannot be turned back into an [`Operation`]: a recovered
/// permanent delete must not become runnable again without a fresh
/// confirmation.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationKind {
    CreateFile,
    CreateFolder,
    Rename,
    BulkRename,
    Copy,
    Move,
    Duplicate,
    Trash,
    RestoreFromTrash,
    PermanentDelete,
    Checksum,
}

impl OperationKind {
    pub fn key(self) -> &'static str {
        match self {
            OperationKind::CreateFile => "files.operation.kind.create_file",
            OperationKind::CreateFolder => "files.operation.kind.create_folder",
            OperationKind::Rename => "files.operation.kind.rename",
            OperationKind::BulkRename => "files.operation.kind.bulk_rename",
            OperationKind::Copy => "files.operation.kind.copy",
            OperationKind::Move => "files.operation.kind.move",
            OperationKind::Duplicate => "files.operation.kind.duplicate",
            OperationKind::Trash => "files.operation.kind.trash",
            OperationKind::RestoreFromTrash => "files.operation.kind.restore_from_trash",
            OperationKind::PermanentDelete => "files.operation.kind.permanent_delete",
            OperationKind::Checksum => "files.operation.kind.checksum",
        }
    }

    /// Whether a paused job of this kind can be resumed usefully.
    ///
    /// Everything that walks a list of items can pause between items. A single
    /// rename cannot pause in any observable way, and saying it can would be a
    /// button that does nothing.
    pub fn supports_pause(self) -> bool {
        !matches!(
            self,
            OperationKind::CreateFile | OperationKind::CreateFolder | OperationKind::Rename
        )
    }

    /// Whether the job has a safe compensating action.
    ///
    /// A copy can be rolled back by deleting exactly the destinations it
    /// created, and a create by removing what it created. A move cannot: after
    /// the source is gone, putting it back is another move that can fail on its
    /// own, and Issue #6 explicitly puts undo without a safe compensating
    /// action out of scope. Trash is its own undo — restore-from-trash is the
    /// compensating action, and it is a job the user runs, not a rollback the
    /// engine performs silently.
    pub fn supports_rollback(self) -> bool {
        matches!(
            self,
            OperationKind::Copy
                | OperationKind::Duplicate
                | OperationKind::CreateFile
                | OperationKind::CreateFolder
        )
    }

    /// Whether the operation moves file content, and so has bytes to report.
    pub fn moves_bytes(self) -> bool {
        matches!(
            self,
            OperationKind::Copy
                | OperationKind::Move
                | OperationKind::Duplicate
                | OperationKind::Checksum
        )
    }
}

impl Operation {
    pub fn kind(&self) -> OperationKind {
        match self {
            Operation::CreateFile { .. } => OperationKind::CreateFile,
            Operation::CreateFolder { .. } => OperationKind::CreateFolder,
            Operation::Rename { .. } => OperationKind::Rename,
            Operation::BulkRename { .. } => OperationKind::BulkRename,
            Operation::Copy { .. } => OperationKind::Copy,
            Operation::Move { .. } => OperationKind::Move,
            Operation::Duplicate { .. } => OperationKind::Duplicate,
            Operation::Trash { .. } => OperationKind::Trash,
            Operation::RestoreFromTrash { .. } => OperationKind::RestoreFromTrash,
            Operation::PermanentDelete { .. } => OperationKind::PermanentDelete,
            Operation::Checksum { .. } => OperationKind::Checksum,
        }
    }
}

/// One submitted job.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JobSpec {
    pub operation: Operation,
    pub policy: CopyPolicy,
    /// Answers the job already has, so a caller that knows what it wants —
    /// "duplicate always renames" — never raises a conflict at all.
    pub conflicts: crate::conflict::ConflictPolicy,
}

impl JobSpec {
    pub fn new(operation: Operation) -> Self {
        Self {
            operation,
            policy: CopyPolicy::default(),
            conflicts: crate::conflict::ConflictPolicy::new(),
        }
    }

    pub fn with_policy(mut self, policy: CopyPolicy) -> Self {
        self.policy = policy;
        self
    }

    pub fn with_conflicts(mut self, conflicts: crate::conflict::ConflictPolicy) -> Self {
        self.conflicts = conflicts;
        self
    }

    pub fn kind(&self) -> OperationKind {
        self.operation.kind()
    }

    /// Checks everything that can be checked without touching the disk.
    ///
    /// The engine calls this before a job is queued, so a spec that could
    /// never succeed is refused at submission with a named error instead of
    /// becoming a job that fails a moment later.
    pub fn validate(&self) -> Result<(), OperationError> {
        match &self.operation {
            Operation::CreateFile { name, .. }
            | Operation::CreateFolder { name, .. }
            | Operation::Rename { new_name: name, .. } => {
                if !is_usable_name(name) {
                    return Err(OperationError::InvalidName {
                        name: PathBuf::from(name),
                    });
                }
            }
            Operation::Copy {
                sources,
                destination,
            }
            | Operation::Move {
                sources,
                destination,
            } => {
                for source in sources {
                    // Copying a directory into itself never terminates, and
                    // the walk would have discovered that only after creating
                    // an unbounded number of directories.
                    if destination.as_path().starts_with(source.as_path()) {
                        return Err(OperationError::DestinationInsideSource {
                            path: destination.as_path().to_path_buf(),
                        });
                    }
                }
            }
            Operation::BulkRename { pattern, targets } => {
                for (index, target) in targets.iter().enumerate() {
                    let Some(current) = target.as_path().file_name() else {
                        return Err(OperationError::InvalidName {
                            name: target.as_path().to_path_buf(),
                        });
                    };
                    if pattern.apply(current, index as u64).is_none() {
                        return Err(OperationError::InvalidName {
                            name: target.as_path().to_path_buf(),
                        });
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::ffi::{OsStrExt, OsStringExt};

    #[test]
    fn a_find_and_replace_over_a_non_utf8_name_keeps_the_invalid_bytes() {
        let original = OsString::from_vec(b"holiday\xff-01.jpg".to_vec());
        let pattern = RenamePattern::replacing("holiday", "trip");
        let renamed = pattern.apply(&original, 0).unwrap();
        assert_eq!(renamed.as_bytes(), b"trip\xff-01.jpg");
    }

    #[test]
    fn a_numbering_template_pads_and_steps() {
        let pattern = RenamePattern {
            template: Some("photo-{n}{ext}".to_string()),
            start: 5,
            step: 5,
            width: 3,
            ..RenamePattern::default()
        };
        assert_eq!(
            pattern.apply(OsStr::new("a.jpg"), 0).unwrap(),
            OsString::from("photo-005.jpg")
        );
        assert_eq!(
            pattern.apply(OsStr::new("b.jpg"), 2).unwrap(),
            OsString::from("photo-015.jpg")
        );
    }

    #[test]
    fn find_and_replace_runs_before_the_template_uses_the_name() {
        let pattern = RenamePattern {
            find: OsString::from("IMG"),
            replace: OsString::from("Trip"),
            template: Some("{name}-{n}{ext}".to_string()),
            start: 1,
            step: 1,
            width: 2,
        };
        assert_eq!(
            pattern.apply(OsStr::new("IMG_0042.jpg"), 0).unwrap(),
            OsString::from("Trip_0042-01.jpg")
        );
    }

    #[test]
    fn a_pattern_that_would_move_a_file_out_of_its_directory_is_refused() {
        let pattern = RenamePattern::numbering("../{n}", 1, 1);
        assert_eq!(pattern.apply(OsStr::new("a.txt"), 0), None);
        let escaping = RenamePattern::replacing("a", "sub/a");
        assert_eq!(escaping.apply(OsStr::new("a.txt"), 0), None);
    }

    #[test]
    fn a_name_longer_than_name_max_is_refused_before_the_kernel_sees_it() {
        let long = OsString::from("x".repeat(256));
        assert!(!is_usable_name(&long));
        let ok = OsString::from("x".repeat(255));
        assert!(is_usable_name(&ok));
    }

    #[test]
    fn copying_a_directory_into_itself_is_refused_at_submission() {
        let source = LocalPath::new("/data/photos").unwrap();
        let destination = LocalPath::new("/data/photos/backup").unwrap();
        let spec = JobSpec::new(Operation::Copy {
            sources: vec![source],
            destination,
        });
        assert!(matches!(
            spec.validate(),
            Err(OperationError::DestinationInsideSource { .. })
        ));
    }

    #[test]
    fn only_the_operations_with_a_safe_undo_claim_rollback() {
        assert!(OperationKind::Copy.supports_rollback());
        assert!(OperationKind::CreateFolder.supports_rollback());
        assert!(!OperationKind::Move.supports_rollback());
        assert!(!OperationKind::PermanentDelete.supports_rollback());
        assert!(!OperationKind::Trash.supports_rollback());
    }

    #[test]
    fn a_single_rename_does_not_claim_a_pause_button_that_would_do_nothing() {
        assert!(!OperationKind::Rename.supports_pause());
        assert!(OperationKind::Copy.supports_pause());
        assert!(OperationKind::BulkRename.supports_pause());
    }
}
