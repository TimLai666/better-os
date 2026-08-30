//! Failures the domain model can produce. Every variant renders as a stable
//! machine key, the convention `manager-core` and `app-catalog-core` already
//! follow, so a GUI can key a translated string off it instead of matching on
//! English prose.

use thiserror::Error;

use crate::location::LocationKind;

#[derive(Clone, Debug, Eq, PartialEq, Error)]
pub enum LocationError {
    #[error("files.location.error.relative_path:{0}")]
    RelativePath(String),
    #[error("files.location.error.invalid_name:{0}")]
    InvalidName(String),
    #[error("files.location.error.missing_authority:{0}")]
    MissingAuthority(&'static str),
    #[error("files.location.error.backend_unavailable:{0}")]
    BackendUnavailable(&'static str),
}

/// Why a listing stopped without delivering the whole directory.
///
/// A refusal is data the view renders, not a message logged after the fact.
/// "Not listable" and "permission denied" are distinct because the first is a
/// property of this build and the second is a property of the host.
#[derive(Clone, Debug, Eq, PartialEq, Error)]
pub enum ListingError {
    #[error("files.listing.error.not_listable:{0:?}")]
    NotListable(LocationKind),
    #[error("files.listing.error.permission_denied:{path}")]
    PermissionDenied { path: String },
    #[error("files.listing.error.not_found:{path}")]
    NotFound { path: String },
    #[error("files.listing.error.not_a_directory:{path}")]
    NotADirectory { path: String },
    /// The device backing the location went away while it was being read. It
    /// is reported rather than folded into a generic I/O error, because the
    /// view says something different about a disk that was unplugged.
    #[error("files.listing.error.device_lost:{path}")]
    DeviceLost { path: String },
    /// A symlink chain that never reaches a target. Reported at the entry that
    /// caused it, so the rest of the directory still lists.
    #[error("files.listing.error.symlink_loop:{path}")]
    SymlinkLoop { path: String },
    #[error("files.listing.error.name_too_long:{path}")]
    NameTooLong { path: String },
    #[error("files.listing.error.io:{path}:{reason}")]
    Io { path: String, reason: String },
}

/// Why the model would not do what was asked of it. These are programming
/// errors in a consumer, not host conditions.
#[derive(Clone, Debug, Eq, PartialEq, Error)]
pub enum NavigationError {
    #[error("files.navigation.error.no_such_tab:{0}")]
    NoSuchTab(u64),
    #[error("files.navigation.error.last_tab")]
    LastTab,
    #[error("files.navigation.error.nothing_to_restore")]
    NothingToRestore,
}
