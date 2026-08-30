//! What can go wrong, named rather than flattened.
//!
//! Issue #6 lists the conditions Better Files has to say something specific
//! about: a full disk, a permission refusal, a device that disappears, a
//! symlink loop, a very long path, a filename that is not UTF-8, and a source
//! that changed under the job. Each of those is its own variant here, because
//! the sentence the interface shows and the action it offers differ. "Copy
//! failed: I/O error" is the answer this taxonomy exists to avoid.
//!
//! Every variant renders as a stable machine key, the convention `files-core`,
//! `manager-core`, and `app-catalog-core` already follow, so a translated
//! string is keyed off the variant instead of matched against English prose.
//! The path travels as a `PathBuf`, not a `String`: a failure on a name that
//! is not valid UTF-8 has to name the real file, and only the `Display`
//! rendering is lossy.

use std::io;
use std::path::{Path, PathBuf};

use thiserror::Error;

/// A failure attributable to one path, or to the job as a whole.
///
/// The paths are serialized as raw bytes rather than as strings. `serde_json`
/// refuses a `PathBuf` that is not valid UTF-8, and a failure report about a
/// file whose name is not valid UTF-8 is exactly the report that must survive.
#[derive(Clone, Debug, Eq, PartialEq, Error, serde::Serialize, serde::Deserialize)]
pub enum OperationError {
    /// The filesystem has no room left. Distinct from a quota refusal because
    /// the user can act on one and usually cannot act on the other.
    #[error("files.operation.error.no_space:{}", display_path(.path))]
    NoSpace {
        #[serde(with = "crate::store::path_bytes")]
        path: PathBuf,
    },
    /// The user's quota is exhausted even though the filesystem has room.
    #[error("files.operation.error.quota_exceeded:{}", display_path(.path))]
    QuotaExceeded {
        #[serde(with = "crate::store::path_bytes")]
        path: PathBuf,
    },
    #[error("files.operation.error.permission_denied:{}", display_path(.path))]
    PermissionDenied {
        #[serde(with = "crate::store::path_bytes")]
        path: PathBuf,
    },
    /// The filesystem refuses every write. An external card with its lock
    /// switch on reads exactly like this.
    #[error("files.operation.error.read_only:{}", display_path(.path))]
    ReadOnly {
        #[serde(with = "crate::store::path_bytes")]
        path: PathBuf,
    },
    #[error("files.operation.error.not_found:{}", display_path(.path))]
    NotFound {
        #[serde(with = "crate::store::path_bytes")]
        path: PathBuf,
    },
    #[error("files.operation.error.already_exists:{}", display_path(.path))]
    AlreadyExists {
        #[serde(with = "crate::store::path_bytes")]
        path: PathBuf,
    },
    #[error("files.operation.error.not_a_directory:{}", display_path(.path))]
    NotADirectory {
        #[serde(with = "crate::store::path_bytes")]
        path: PathBuf,
    },
    #[error("files.operation.error.is_a_directory:{}", display_path(.path))]
    IsADirectory {
        #[serde(with = "crate::store::path_bytes")]
        path: PathBuf,
    },
    /// A directory would be copied into itself or into one of its own
    /// descendants. Detected before any byte moves, because the copy would
    /// otherwise never terminate.
    #[error("files.operation.error.destination_inside_source:{}", display_path(.path))]
    DestinationInsideSource {
        #[serde(with = "crate::store::path_bytes")]
        path: PathBuf,
    },
    /// The recursive walk reached a directory it had already entered. Found
    /// with a visited set of `(device, inode)` pairs rather than a depth cap,
    /// so a legitimately deep tree is not mistaken for a loop.
    #[error("files.operation.error.symlink_loop:{}", display_path(.path))]
    SymlinkLoop {
        #[serde(with = "crate::store::path_bytes")]
        path: PathBuf,
    },
    #[error("files.operation.error.name_too_long:{}", display_path(.path))]
    NameTooLong {
        #[serde(with = "crate::store::path_bytes")]
        path: PathBuf,
    },
    /// The name is not usable as a filename at all: empty, `.`, `..`, or
    /// carrying a separator or a NUL.
    #[error("files.operation.error.invalid_name:{}", display_name(.name))]
    InvalidName {
        #[serde(with = "crate::store::path_bytes")]
        name: PathBuf,
    },
    /// The device backing the path went away mid-operation. Separate from a
    /// generic I/O failure because nothing the user does to the file will fix
    /// it, and because a job that hit this must not delete a source.
    #[error("files.operation.error.device_lost:{}", display_path(.path))]
    DeviceLost {
        #[serde(with = "crate::store::path_bytes")]
        path: PathBuf,
    },
    /// Source and destination are on different filesystems, so the rename
    /// fast path does not apply. Reported as a fact the move planner consumes,
    /// not as a failure the user sees.
    #[error("files.operation.error.cross_device:{}", display_path(.path))]
    CrossDevice {
        #[serde(with = "crate::store::path_bytes")]
        path: PathBuf,
    },
    /// The file changed since the job looked at it. Checked immediately before
    /// every destructive step, so a move never deletes a source that somebody
    /// else rewrote while the copy was running.
    #[error("files.operation.error.externally_modified:{}", display_path(.path))]
    ExternallyModified {
        #[serde(with = "crate::store::path_bytes")]
        path: PathBuf,
    },
    /// The copy finished but the destination does not match the source.
    #[error("files.operation.error.verification_failed:{}", display_path(.path))]
    VerificationFailed {
        #[serde(with = "crate::store::path_bytes")]
        path: PathBuf,
        reason: String,
    },
    /// A permanent delete arrived without the explicit confirmation its spec
    /// requires. It is an error rather than a prompt because the engine has no
    /// user to ask.
    #[error("files.operation.error.confirmation_required")]
    ConfirmationRequired,
    /// The user cancelled while this item was being worked on.
    #[error("files.operation.error.cancelled:{}", display_path(.path))]
    Cancelled {
        #[serde(with = "crate::store::path_bytes")]
        path: PathBuf,
    },
    /// The process holding the job stopped while the job was running. Produced
    /// only by recovery on the next start, never by a live worker.
    #[error("files.operation.error.interrupted")]
    Interrupted,
    /// A conflict the user declined to resolve.
    #[error("files.operation.error.conflict_unresolved:{}", display_path(.path))]
    ConflictUnresolved {
        #[serde(with = "crate::store::path_bytes")]
        path: PathBuf,
    },
    /// The trash cannot be used: it has no home directory, or the info file a
    /// restore needs is gone.
    #[error("files.operation.error.trash_unavailable:{reason}")]
    TrashUnavailable { reason: String },
    /// Anything the classifier did not recognise, carrying the raw errno so a
    /// bug report says which one it was.
    #[error("files.operation.error.io:{}:{reason}", display_path(.path))]
    Io {
        #[serde(with = "crate::store::path_bytes")]
        path: PathBuf,
        reason: String,
        errno: Option<i32>,
    },
}

impl OperationError {
    /// Classifies a `std::io::Error` against the conditions Issue #6 names.
    ///
    /// The classification is by raw errno rather than by `ErrorKind`, because
    /// several of the interesting ones — `ENOSPC`, `EDQUOT`, `ELOOP`,
    /// `ENAMETOOLONG`, `EXDEV`, `ENODEV` — are still `ErrorKind::Uncategorized`
    /// on the stable toolchain this workspace builds against.
    pub fn from_io(path: impl Into<PathBuf>, error: &io::Error) -> Self {
        let path = path.into();
        match error.raw_os_error() {
            Some(libc::ENOSPC) => Self::NoSpace { path },
            Some(libc::EDQUOT) => Self::QuotaExceeded { path },
            Some(libc::EACCES) | Some(libc::EPERM) => Self::PermissionDenied { path },
            Some(libc::EROFS) => Self::ReadOnly { path },
            Some(libc::ENOENT) => Self::NotFound { path },
            Some(libc::EEXIST) | Some(libc::ENOTEMPTY) => Self::AlreadyExists { path },
            Some(libc::ENOTDIR) => Self::NotADirectory { path },
            Some(libc::EISDIR) => Self::IsADirectory { path },
            Some(libc::ELOOP) | Some(libc::EMLINK) => Self::SymlinkLoop { path },
            Some(libc::ENAMETOOLONG) => Self::NameTooLong { path },
            Some(libc::EXDEV) => Self::CrossDevice { path },
            // A disk pulled mid-write reports one of these three depending on
            // how far the request got.
            Some(libc::ENODEV) | Some(libc::ENXIO) | Some(libc::EIO) => Self::DeviceLost { path },
            other => Self::Io {
                path,
                reason: error.kind().to_string(),
                errno: other,
            },
        }
    }

    /// The path the failure is about, when it is about one.
    pub fn path(&self) -> Option<&Path> {
        match self {
            Self::NoSpace { path }
            | Self::QuotaExceeded { path }
            | Self::PermissionDenied { path }
            | Self::ReadOnly { path }
            | Self::NotFound { path }
            | Self::AlreadyExists { path }
            | Self::NotADirectory { path }
            | Self::IsADirectory { path }
            | Self::DestinationInsideSource { path }
            | Self::SymlinkLoop { path }
            | Self::NameTooLong { path }
            | Self::DeviceLost { path }
            | Self::CrossDevice { path }
            | Self::ExternallyModified { path }
            | Self::VerificationFailed { path, .. }
            | Self::Cancelled { path }
            | Self::ConflictUnresolved { path }
            | Self::Io { path, .. } => Some(path),
            Self::InvalidName { name } => Some(name),
            Self::ConfirmationRequired | Self::Interrupted | Self::TrashUnavailable { .. } => None,
        }
    }

    /// The stable key without the path, for a consumer keying a translation.
    pub fn key(&self) -> &'static str {
        match self {
            Self::NoSpace { .. } => "files.operation.error.no_space",
            Self::QuotaExceeded { .. } => "files.operation.error.quota_exceeded",
            Self::PermissionDenied { .. } => "files.operation.error.permission_denied",
            Self::ReadOnly { .. } => "files.operation.error.read_only",
            Self::NotFound { .. } => "files.operation.error.not_found",
            Self::AlreadyExists { .. } => "files.operation.error.already_exists",
            Self::NotADirectory { .. } => "files.operation.error.not_a_directory",
            Self::IsADirectory { .. } => "files.operation.error.is_a_directory",
            Self::DestinationInsideSource { .. } => {
                "files.operation.error.destination_inside_source"
            }
            Self::SymlinkLoop { .. } => "files.operation.error.symlink_loop",
            Self::NameTooLong { .. } => "files.operation.error.name_too_long",
            Self::InvalidName { .. } => "files.operation.error.invalid_name",
            Self::DeviceLost { .. } => "files.operation.error.device_lost",
            Self::CrossDevice { .. } => "files.operation.error.cross_device",
            Self::ExternallyModified { .. } => "files.operation.error.externally_modified",
            Self::VerificationFailed { .. } => "files.operation.error.verification_failed",
            Self::ConfirmationRequired => "files.operation.error.confirmation_required",
            Self::Cancelled { .. } => "files.operation.error.cancelled",
            Self::Interrupted => "files.operation.error.interrupted",
            Self::ConflictUnresolved { .. } => "files.operation.error.conflict_unresolved",
            Self::TrashUnavailable { .. } => "files.operation.error.trash_unavailable",
            Self::Io { .. } => "files.operation.error.io",
        }
    }

    /// Whether retrying this item on its own could plausibly succeed.
    ///
    /// A full disk becomes retryable the moment the user frees space, and a
    /// permission refusal becomes retryable the moment they fix the mode.
    /// A symlink loop or a destination inside its own source will fail again
    /// identically, so offering retry there would be a lie.
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            Self::NoSpace { .. }
                | Self::QuotaExceeded { .. }
                | Self::PermissionDenied { .. }
                | Self::ReadOnly { .. }
                | Self::DeviceLost { .. }
                | Self::ExternallyModified { .. }
                | Self::VerificationFailed { .. }
                | Self::Cancelled { .. }
                | Self::Interrupted
                | Self::ConflictUnresolved { .. }
                | Self::Io { .. }
        )
    }
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn display_name(name: &Path) -> String {
    name.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn io(errno: i32) -> io::Error {
        io::Error::from_raw_os_error(errno)
    }

    #[test]
    fn every_condition_issue_six_names_has_its_own_variant() {
        let path = Path::new("/tmp/x");
        assert_eq!(
            OperationError::from_io(path, &io(libc::ENOSPC)).key(),
            "files.operation.error.no_space"
        );
        assert_eq!(
            OperationError::from_io(path, &io(libc::EACCES)).key(),
            "files.operation.error.permission_denied"
        );
        assert_eq!(
            OperationError::from_io(path, &io(libc::ENODEV)).key(),
            "files.operation.error.device_lost"
        );
        assert_eq!(
            OperationError::from_io(path, &io(libc::ELOOP)).key(),
            "files.operation.error.symlink_loop"
        );
        assert_eq!(
            OperationError::from_io(path, &io(libc::ENAMETOOLONG)).key(),
            "files.operation.error.name_too_long"
        );
        assert_eq!(
            OperationError::from_io(path, &io(libc::EXDEV)).key(),
            "files.operation.error.cross_device"
        );
        assert_eq!(
            OperationError::from_io(path, &io(libc::EROFS)).key(),
            "files.operation.error.read_only"
        );
        assert_eq!(
            OperationError::from_io(path, &io(libc::EDQUOT)).key(),
            "files.operation.error.quota_exceeded"
        );
    }

    #[test]
    fn an_unrecognised_errno_keeps_its_number_instead_of_being_swallowed() {
        let error = OperationError::from_io("/tmp/x", &io(libc::EAGAIN));
        match error {
            OperationError::Io { errno, .. } => assert_eq!(errno, Some(libc::EAGAIN)),
            other => panic!("expected a generic I/O error, got {other:?}"),
        }
    }

    #[test]
    fn a_failure_keeps_the_path_even_when_it_is_not_utf8() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;
        let name = PathBuf::from(OsStr::from_bytes(b"/tmp/\xff\xfeinvalid"));
        let error = OperationError::from_io(&name, &io(libc::EACCES));
        assert_eq!(error.path(), Some(name.as_path()));
        // Only the rendering is lossy; the stored path still round-trips.
        assert!(
            error
                .to_string()
                .starts_with("files.operation.error.permission_denied:")
        );
    }

    #[test]
    fn retry_is_offered_only_where_it_could_work() {
        assert!(OperationError::NoSpace { path: "/x".into() }.is_retryable());
        assert!(!OperationError::SymlinkLoop { path: "/x".into() }.is_retryable());
        assert!(!OperationError::ConfirmationRequired.is_retryable());
    }
}
