//! Conflicts, and the one decision that answers a thousand of them.
//!
//! Issue #6's user story is precise: a job "asks about a name conflict once
//! instead of a thousand times". That is the whole reason [`ResolutionScope`]
//! exists. A resolution answers either this one conflict or every remaining
//! conflict of the same kind, and the engine parks on the first one it cannot
//! answer from a standing decision.
//!
//! A conflict is not always a name clash. A destination whose filesystem is
//! case-insensitive, a destination directory the user cannot write to, and a
//! destination with no room left are all decisions rather than failures,
//! because skip-the-rest and choose-somewhere-else are real answers to all
//! three.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Why the job stopped to ask.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictKind {
    /// Something is already at the destination path.
    Exists,
    /// Nothing is at the destination path, but the destination filesystem
    /// treats an existing name as the same name. Reported separately because
    /// "overwrite" means overwriting a file with a different name, which the
    /// user has to be told.
    CaseConflict,
    /// The destination directory refuses the write.
    Permission,
    /// The destination filesystem has no room for this item.
    NoSpace,
}

impl ConflictKind {
    pub fn key(self) -> &'static str {
        match self {
            ConflictKind::Exists => "files.conflict.kind.exists",
            ConflictKind::CaseConflict => "files.conflict.kind.case_conflict",
            ConflictKind::Permission => "files.conflict.kind.permission",
            ConflictKind::NoSpace => "files.conflict.kind.no_space",
        }
    }

    /// Whether overwriting is even meaningful for this kind. Answering
    /// "overwrite" to a full disk is not a resolution, and the engine refuses
    /// it rather than trying the write again and failing identically.
    pub fn accepts_overwrite(self) -> bool {
        matches!(self, ConflictKind::Exists | ConflictKind::CaseConflict)
    }
}

/// One decision the job needs.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Conflict {
    pub kind: ConflictKind,
    /// The item being worked on. `None` for a conflict raised by an operation
    /// that creates rather than transfers, such as create-folder.
    #[serde(with = "crate::store::path_bytes_option")]
    pub source: Option<PathBuf>,
    /// Where it was going.
    #[serde(with = "crate::store::path_bytes")]
    pub destination: PathBuf,
    /// For a case conflict, the name that is already there under a different
    /// spelling.
    #[serde(with = "crate::store::path_bytes_option")]
    pub existing: Option<PathBuf>,
}

impl Conflict {
    pub fn exists(source: Option<PathBuf>, destination: PathBuf) -> Self {
        Self {
            kind: ConflictKind::Exists,
            source,
            destination,
            existing: None,
        }
    }
}

/// What to do about it.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Resolution {
    /// Leave the destination alone and count the item as skipped rather than
    /// failed. A skipped item does not make the job fail.
    Skip,
    /// Replace the destination. The replacement is still written to a
    /// temporary name and renamed over the target, so an interrupted overwrite
    /// leaves the original rather than half of the new one.
    Overwrite,
    /// Write beside it under a generated name: `report.txt` becomes
    /// `report (copy).txt`, then `report (copy 2).txt`.
    Rename,
    /// Give up on the job. Distinct from skip: the user is saying stop, not
    /// saying stop for this one.
    Cancel,
}

/// How far the answer reaches.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolutionScope {
    /// Only this conflict. The next one asks again.
    ThisItem,
    /// This one and every later conflict of the same kind in this job. This is
    /// the answer to "a thousand times".
    ApplyToRemaining,
}

/// One answer, with its reach.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ConflictDecision {
    pub resolution: Resolution,
    pub scope: ResolutionScope,
}

impl ConflictDecision {
    pub fn once(resolution: Resolution) -> Self {
        Self {
            resolution,
            scope: ResolutionScope::ThisItem,
        }
    }

    pub fn for_remaining(resolution: Resolution) -> Self {
        Self {
            resolution,
            scope: ResolutionScope::ApplyToRemaining,
        }
    }
}

/// The standing answers a job has collected, keyed by conflict kind.
///
/// Keying by kind rather than holding one blanket answer is deliberate: a user
/// who said "overwrite every existing file" has not said anything about what to
/// do when the disk fills up, and the job must still ask.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct ConflictPolicy {
    standing: Vec<(ConflictKind, Resolution)>,
}

impl ConflictPolicy {
    pub fn new() -> Self {
        Self::default()
    }

    /// A policy that answers every conflict of one kind up front, for a caller
    /// that already knows what it wants — a duplicate always renames, for
    /// example.
    pub fn always(kind: ConflictKind, resolution: Resolution) -> Self {
        let mut policy = Self::new();
        policy.remember(kind, resolution);
        policy
    }

    pub fn remember(&mut self, kind: ConflictKind, resolution: Resolution) {
        match self.standing.iter_mut().find(|(seen, _)| *seen == kind) {
            Some(slot) => slot.1 = resolution,
            None => self.standing.push((kind, resolution)),
        }
    }

    /// The standing answer for this conflict, if there is one.
    pub fn answer(&self, conflict: &Conflict) -> Option<Resolution> {
        self.standing
            .iter()
            .find(|(kind, _)| *kind == conflict.kind)
            .map(|(_, resolution)| *resolution)
    }

    /// Records a decision, keeping it only when its scope says to.
    pub fn apply(&mut self, conflict: &Conflict, decision: ConflictDecision) {
        if decision.scope == ResolutionScope::ApplyToRemaining {
            self.remember(conflict.kind, decision.resolution);
        }
    }

    pub fn is_empty(&self) -> bool {
        self.standing.is_empty()
    }
}

/// Generates the next free name beside an occupied one.
///
/// Works on bytes rather than on a `String`, so a name that is not valid UTF-8
/// gets a numbered sibling like every other name instead of being refused. The
/// suffix goes before the extension, which is what makes the result still open
/// in the same application.
pub fn next_available_name(directory: &std::path::Path, name: &std::ffi::OsStr) -> PathBuf {
    use std::ffi::OsString;
    use std::os::unix::ffi::{OsStrExt, OsStringExt};

    let bytes = name.as_bytes();
    // The extension split ignores a leading dot, so `.bashrc` is a stem and not
    // an empty name with an extension.
    let split = bytes
        .iter()
        .rposition(|byte| *byte == b'.')
        .filter(|index| *index > 0);
    let (stem, extension) = match split {
        Some(index) => (&bytes[..index], &bytes[index..]),
        None => (bytes, &[][..]),
    };

    for attempt in 1u32.. {
        let mut candidate = Vec::with_capacity(bytes.len() + 12);
        candidate.extend_from_slice(stem);
        if attempt == 1 {
            candidate.extend_from_slice(b" (copy)");
        } else {
            candidate.extend_from_slice(format!(" (copy {attempt})").as_bytes());
        }
        candidate.extend_from_slice(extension);
        let path = directory.join(OsString::from_vec(candidate));
        if !path.symlink_metadata().is_ok() {
            return path;
        }
        // A directory somebody is filling as fast as this loop reads it is not
        // a case worth an unbounded search; a thousand attempts is already far
        // past any real collision.
        if attempt > 1000 {
            break;
        }
    }
    directory.join(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;

    fn exists_conflict() -> Conflict {
        Conflict::exists(Some("/src/a".into()), "/dst/a".into())
    }

    #[test]
    fn an_answer_scoped_to_one_item_does_not_answer_the_next_one() {
        let mut policy = ConflictPolicy::new();
        let conflict = exists_conflict();
        policy.apply(&conflict, ConflictDecision::once(Resolution::Overwrite));
        assert_eq!(policy.answer(&conflict), None);
        assert!(policy.is_empty());
    }

    #[test]
    fn an_answer_applied_to_the_remaining_conflicts_answers_them_all() {
        let mut policy = ConflictPolicy::new();
        let conflict = exists_conflict();
        policy.apply(
            &conflict,
            ConflictDecision::for_remaining(Resolution::Rename),
        );
        let later = Conflict::exists(Some("/src/z".into()), "/dst/z".into());
        assert_eq!(policy.answer(&later), Some(Resolution::Rename));
    }

    #[test]
    fn a_standing_answer_about_names_says_nothing_about_a_full_disk() {
        let mut policy = ConflictPolicy::new();
        policy.apply(
            &exists_conflict(),
            ConflictDecision::for_remaining(Resolution::Overwrite),
        );
        let full = Conflict {
            kind: ConflictKind::NoSpace,
            source: Some("/src/b".into()),
            destination: "/dst/b".into(),
            existing: None,
        };
        assert_eq!(policy.answer(&full), None);
    }

    #[test]
    fn overwrite_is_meaningless_for_a_disk_that_is_full() {
        assert!(ConflictKind::Exists.accepts_overwrite());
        assert!(ConflictKind::CaseConflict.accepts_overwrite());
        assert!(!ConflictKind::NoSpace.accepts_overwrite());
        assert!(!ConflictKind::Permission.accepts_overwrite());
    }

    #[test]
    fn a_generated_name_keeps_the_extension_so_the_file_still_opens() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("report.txt"), b"x").unwrap();
        let first = next_available_name(directory.path(), OsStr::new("report.txt"));
        assert_eq!(first.file_name().unwrap(), OsStr::new("report (copy).txt"));
        std::fs::write(&first, b"x").unwrap();
        let second = next_available_name(directory.path(), OsStr::new("report.txt"));
        assert_eq!(
            second.file_name().unwrap(),
            OsStr::new("report (copy 2).txt")
        );
    }

    #[test]
    fn a_dotfile_is_numbered_as_a_stem_not_as_an_extension() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join(".bashrc"), b"x").unwrap();
        let generated = next_available_name(directory.path(), OsStr::new(".bashrc"));
        assert_eq!(generated.file_name().unwrap(), OsStr::new(".bashrc (copy)"));
    }

    #[test]
    fn a_name_that_is_not_utf8_still_gets_a_numbered_sibling() {
        let directory = tempfile::tempdir().unwrap();
        let name = OsStr::from_bytes(b"broken\xff.bin");
        std::fs::write(directory.path().join(name), b"x").unwrap();
        let generated = next_available_name(directory.path(), name);
        assert_eq!(
            generated.file_name().unwrap().as_bytes(),
            b"broken\xff (copy).bin"
        );
    }
}
