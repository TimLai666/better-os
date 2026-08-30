//! Reading `/proc` and `/sys`, and turning a failed read into the right
//! observation state.
//!
//! The mapping here is the whole reason the crate does not use `?` and lose
//! the distinction: a missing file means the kernel does not expose the
//! interface, `EACCES` means this unprivileged process may not read it, and
//! anything else is a transient failure that says nothing about support.

use monitor_core::{Observation, UnknownReason, UnsupportedReason};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Why a read of a kernel interface did not produce bytes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReadError {
    /// The path is not there. On `/proc` and `/sys` that means the kernel was
    /// built without the feature, the driver does not expose it, or the entity
    /// went away.
    Missing { path: PathBuf },
    /// The path is there and this process may not read it.
    PermissionDenied { path: PathBuf },
    /// Present and permitted, but the read failed anyway.
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

    /// The observation a metric gets when its own file could not be read.
    pub fn into_observation(self) -> Observation {
        match self {
            ReadError::Missing { path } => {
                Observation::Unsupported(UnsupportedReason::InterfaceMissing {
                    path: path.display().to_string(),
                })
            }
            ReadError::PermissionDenied { path } => Observation::PermissionDenied {
                path: path.display().to_string(),
            },
            ReadError::Failed { path, detail } => Observation::Unknown(UnknownReason::ReadFailed {
                detail: format!("{}: {detail}", path.display()),
            }),
        }
    }

    /// The observation a metric gets when the entity it describes disappeared
    /// mid-scan. A process exiting between enumeration and reading is normal
    /// and is not evidence about the host's capabilities.
    pub fn into_entity_observation(self) -> Observation {
        match self {
            ReadError::Missing { .. } => Observation::Unknown(UnknownReason::EntityDisappeared),
            other => other.into_observation(),
        }
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

/// Read a kernel interface as UTF-8.
///
/// `/proc` and `/sys` text files are ASCII, but a process command line can
/// hold anything, so invalid bytes are replaced rather than rejected.
pub fn read_text(path: &Path) -> Result<String, ReadError> {
    match fs::read(path) {
        Ok(bytes) => Ok(String::from_utf8_lossy(&bytes).into_owned()),
        Err(error) => Err(ReadError::from_io(path, &error)),
    }
}

/// Read a `/sys` attribute that holds a single value, with the trailing
/// newline removed.
pub fn read_attribute(path: &Path) -> Result<String, ReadError> {
    read_text(path).map(|text| text.trim().to_string())
}

/// Read a `/sys` attribute as an unsigned integer.
///
/// A driver that reports a placeholder such as `-1` for an unknown link speed
/// is not malformed; it is telling us it does not know, so that becomes
/// `NotReported` rather than a parse failure.
pub fn read_u64_attribute(path: &Path) -> Result<u64, ReadError> {
    let raw = read_attribute(path)?;
    raw.parse::<u64>().map_err(|_| ReadError::Failed {
        path: path.to_path_buf(),
        detail: format!("not an unsigned integer: {raw:?}"),
    })
}

/// The immediate children of a directory, sorted so a report is deterministic.
pub fn list_dir(path: &Path) -> Result<Vec<PathBuf>, ReadError> {
    let entries = fs::read_dir(path).map_err(|error| ReadError::from_io(path, &error))?;
    let mut collected = Vec::new();
    for entry in entries {
        match entry {
            Ok(entry) => collected.push(entry.path()),
            // A directory entry that vanishes mid-iteration is normal under
            // `/proc`; the rest of the listing is still valid.
            Err(_) => continue,
        }
    }
    collected.sort();
    Ok(collected)
}

/// The number of entries in a directory, used for the process file-descriptor
/// count. Counting is separate from listing because `/proc/<pid>/fd` is
/// commonly unreadable and the caller needs that distinction, not a zero.
pub fn count_dir_entries(path: &Path) -> Result<u64, ReadError> {
    let entries = fs::read_dir(path).map_err(|error| ReadError::from_io(path, &error))?;
    Ok(entries.filter(Result::is_ok).count() as u64)
}

/// Input that did not match the documented format of the interface it came
/// from. Kept separate from `ReadError` because it means the parser and the
/// kernel disagree, which is a different problem from an absent file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MalformedInput {
    pub context: &'static str,
    pub detail: String,
}

impl MalformedInput {
    pub fn new(context: &'static str, detail: impl Into<String>) -> Self {
        Self {
            context,
            detail: detail.into(),
        }
    }

    pub fn into_observation(self) -> Observation {
        Observation::Unknown(UnknownReason::Malformed {
            detail: format!("{}: {}", self.context, self.detail),
        })
    }
}

/// Parse a whitespace-separated unsigned field, naming the interface so a
/// malformed observation says which file disagreed with the parser.
pub fn field_u64(
    context: &'static str,
    fields: &[&str],
    index: usize,
) -> Result<u64, MalformedInput> {
    let raw = fields
        .get(index)
        .ok_or_else(|| MalformedInput::new(context, format!("missing field {index}")))?;
    raw.parse::<u64>().map_err(|_| {
        MalformedInput::new(context, format!("field {index} is not a number: {raw:?}"))
    })
}

pub fn field_f64(
    context: &'static str,
    fields: &[&str],
    index: usize,
) -> Result<f64, MalformedInput> {
    let raw = fields
        .get(index)
        .ok_or_else(|| MalformedInput::new(context, format!("missing field {index}")))?;
    raw.parse::<f64>().map_err(|_| {
        MalformedInput::new(context, format!("field {index} is not a number: {raw:?}"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use monitor_core::ObservationState;

    #[test]
    fn a_missing_interface_becomes_unsupported_not_zero() {
        let error = ReadError::Missing {
            path: PathBuf::from("/proc/pressure/cpu"),
        };
        let observation = error.into_observation();
        assert_eq!(observation.state(), ObservationState::Unsupported);
        assert_eq!(observation.as_f64(), None);
    }

    #[test]
    fn an_unreadable_interface_becomes_permission_denied_not_unsupported() {
        let error = ReadError::PermissionDenied {
            path: PathBuf::from("/proc/1/fd"),
        };
        assert_eq!(
            error.into_observation().state(),
            ObservationState::PermissionDenied
        );
    }

    #[test]
    fn a_transient_failure_says_nothing_about_support() {
        let error = ReadError::Failed {
            path: PathBuf::from("/proc/stat"),
            detail: "input/output error".into(),
        };
        assert_eq!(error.into_observation().state(), ObservationState::Unknown);
    }

    #[test]
    fn a_process_that_exits_mid_scan_is_unknown_rather_than_unsupported() {
        let error = ReadError::Missing {
            path: PathBuf::from("/proc/4242/stat"),
        };
        assert_eq!(
            error.into_entity_observation().state(),
            ObservationState::Unknown
        );
    }

    #[test]
    fn reading_a_path_that_is_not_there_reports_missing() {
        let error = read_text(Path::new("/proc/definitely-not-a-kernel-interface")).unwrap_err();
        assert!(matches!(error, ReadError::Missing { .. }));
    }

    #[test]
    fn a_malformed_field_names_the_interface_that_disagreed() {
        let error = field_u64("/proc/stat", &["cpu", "not-a-number"], 1).unwrap_err();
        assert_eq!(error.context, "/proc/stat");
        let observation = error.into_observation();
        assert_eq!(observation.state(), ObservationState::Unknown);
    }

    #[test]
    fn a_field_past_the_end_of_a_truncated_line_is_malformed() {
        let error = field_u64("/proc/stat", &["cpu"], 4).unwrap_err();
        assert!(error.detail.contains("missing field 4"));
    }
}
