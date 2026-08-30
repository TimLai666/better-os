//! Where the providers read from, and what a failed read means.
//!
//! Every kernel interface is reached through a [`Roots`], so the production code
//! path is the one the tests drive against a captured tree. This mirrors
//! `monitor-collectors-linux`'s seam deliberately; what it does not do is import
//! it. That crate's read helpers return `monitor_core::Observation`, so reusing
//! them would make every Better Awake binary depend on the whole Better Monitor
//! metric-contract stack to answer "is the charger plugged in". The conventions
//! are shared; the dependency is not.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// The `/proc` and `/sys` trees a provider reads.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Roots {
    proc_dir: PathBuf,
    sys_dir: PathBuf,
}

impl Roots {
    /// The real machine.
    pub fn system() -> Self {
        Self {
            proc_dir: PathBuf::from("/proc"),
            sys_dir: PathBuf::from("/sys"),
        }
    }

    /// A captured snapshot laid out as `<root>/proc` and `<root>/sys`.
    pub fn at(root: impl AsRef<Path>) -> Self {
        let root = root.as_ref();
        Self {
            proc_dir: root.join("proc"),
            sys_dir: root.join("sys"),
        }
    }

    pub fn new(proc_dir: impl Into<PathBuf>, sys_dir: impl Into<PathBuf>) -> Self {
        Self {
            proc_dir: proc_dir.into(),
            sys_dir: sys_dir.into(),
        }
    }

    pub fn proc_path(&self, relative: impl AsRef<Path>) -> PathBuf {
        self.proc_dir.join(relative)
    }

    pub fn sys_path(&self, relative: impl AsRef<Path>) -> PathBuf {
        self.sys_dir.join(relative)
    }

    pub fn proc_dir(&self) -> &Path {
        &self.proc_dir
    }

    pub fn sys_dir(&self) -> &Path {
        &self.sys_dir
    }
}

impl Default for Roots {
    fn default() -> Self {
        Self::system()
    }
}

/// Why a read of a kernel interface produced no bytes.
///
/// The three cases are kept apart because they mean different things to a user:
/// a missing file means this machine has no such hardware or no such kernel
/// feature, a permission error means the file is there and we may not have it,
/// and anything else says nothing at all about support.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReadError {
    Missing { path: PathBuf },
    PermissionDenied { path: PathBuf },
    Failed { path: PathBuf, detail: String },
}

impl ReadError {
    pub fn path(&self) -> &Path {
        match self {
            ReadError::Missing { path }
            | ReadError::PermissionDenied { path }
            | ReadError::Failed { path, .. } => path,
        }
    }

    /// The stable key a provider reports when this read is the reason it cannot
    /// answer. Presentation layers own the wording; this is never a sentence.
    pub fn as_key(&self) -> &'static str {
        match self {
            ReadError::Missing { .. } => "awake.provider.interface_missing",
            ReadError::PermissionDenied { .. } => "awake.provider.permission_denied",
            ReadError::Failed { .. } => "awake.provider.read_failed",
        }
    }

    /// The key with the path that produced it, which is what a person
    /// diagnosing an unavailable provider actually needs.
    pub fn explanation(&self) -> String {
        format!("{}:{}", self.as_key(), self.path().display())
    }

    fn from_io(path: &Path, error: &io::Error) -> Self {
        match error.kind() {
            io::ErrorKind::NotFound => ReadError::Missing {
                path: path.to_path_buf(),
            },
            io::ErrorKind::PermissionDenied => ReadError::PermissionDenied {
                path: path.to_path_buf(),
            },
            _ => ReadError::Failed {
                path: path.to_path_buf(),
                detail: error.to_string(),
            },
        }
    }
}

/// Reads a kernel interface as text.
///
/// `/proc` and `/sys` text files are ASCII, but a truncated multi-byte read is
/// still possible, so invalid bytes are replaced rather than turned into a
/// failure that would look like a missing interface.
pub fn read_text(path: &Path) -> Result<String, ReadError> {
    match fs::read(path) {
        Ok(bytes) => Ok(String::from_utf8_lossy(&bytes).into_owned()),
        Err(error) => Err(ReadError::from_io(path, &error)),
    }
}

/// Reads a single-value `/sys` attribute with its trailing newline removed.
pub fn read_attribute(path: &Path) -> Result<String, ReadError> {
    read_text(path).map(|text| text.trim().to_string())
}

/// Reads a `/sys` attribute as an unsigned integer.
pub fn read_u64_attribute(path: &Path) -> Result<u64, ReadError> {
    let raw = read_attribute(path)?;
    raw.parse::<u64>().map_err(|_| ReadError::Failed {
        path: path.to_path_buf(),
        detail: format!("not an unsigned integer: {raw:?}"),
    })
}

/// The immediate children of a directory, sorted so a sample is deterministic.
pub fn list_dir(path: &Path) -> Result<Vec<PathBuf>, ReadError> {
    let entries = fs::read_dir(path).map_err(|error| ReadError::from_io(path, &error))?;
    let mut collected = Vec::new();
    // An entry that vanishes mid-iteration is normal under `/proc`; the rest of
    // the listing is still a valid answer.
    for entry in entries.flatten() {
        collected.push(entry.path());
    }
    collected.sort();
    Ok(collected)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_interface_and_an_unreadable_one_are_different_answers() {
        assert_eq!(
            ReadError::Missing {
                path: PathBuf::from("/sys/class/power_supply")
            }
            .as_key(),
            "awake.provider.interface_missing"
        );
        assert_eq!(
            ReadError::PermissionDenied {
                path: PathBuf::from("/proc/1/cgroup")
            }
            .as_key(),
            "awake.provider.permission_denied"
        );
    }

    #[test]
    fn an_explanation_names_the_path_a_person_would_go_and_look_at() {
        let error = ReadError::Missing {
            path: PathBuf::from("/sys/class/drm"),
        };
        assert_eq!(
            error.explanation(),
            "awake.provider.interface_missing:/sys/class/drm"
        );
    }

    #[test]
    fn reading_a_path_that_is_not_there_reports_missing_rather_than_failing() {
        let error = read_text(Path::new("/proc/definitely-not-a-kernel-interface")).unwrap_err();
        assert!(matches!(error, ReadError::Missing { .. }));
    }

    #[test]
    fn a_captured_tree_is_addressed_the_same_way_the_real_one_is() {
        let roots = Roots::at("/tmp/fixture");
        assert_eq!(
            roots.sys_path("class/power_supply"),
            PathBuf::from("/tmp/fixture/sys/class/power_supply")
        );
        assert_eq!(
            roots.proc_path("stat"),
            PathBuf::from("/tmp/fixture/proc/stat")
        );
    }

    #[test]
    fn a_non_numeric_attribute_is_a_failure_and_not_a_zero() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("capacity");
        fs::write(&path, b"unknown\n").unwrap();
        assert!(matches!(
            read_u64_attribute(&path),
            Err(ReadError::Failed { .. })
        ));
    }
}
