//! The host half of Better Files.
//!
//! `files-core` decides what a location, an entry, and a listing are. This
//! crate is the only place that touches the host to produce them: it reads
//! directories, resolves the XDG user directories, reads the freedesktop
//! trash, detects MIME types, enumerates mount points, and watches for
//! changes. There is no GPUI dependency here, so every one of those can run on
//! any thread a consumer chooses.
//!
//! There is no desktop-entry parser here, and there will not be one. The
//! Applications location is filled from the shared catalog through
//! `files_core::applications`, which is the seam ENG.md calls "one catalog, no
//! second scanner".

pub mod hidden;
pub mod local;
pub mod mime;
pub mod mounts;
pub mod trash;
pub mod watch;
pub mod xdg;

use thiserror::Error;

pub use hidden::read_hidden_rules;
pub use local::{LocalDirectoryReader, ReaderConfig, list_directory_blocking};
pub use mime::{GlobMimeDetector, MimeDetector, SharedMimeDetector, detector_from_env};
pub use mounts::{MountPoint, MountTable, external_devices, read_mount_table};
pub use trash::{
    TrashDirectory, TrashError, TrashedItem, ensure_trash, move_to_trash, original_path_of, purge,
    read_trash, restore, restore_to,
};
pub use watch::{DirectoryWatcher, WatchBackend, WatchEvent, refresh_for};
pub use xdg::{ResolvedDirectory, UserDirectories, UserDirectory};

/// Host-side failures. Every variant renders as a stable machine key, the
/// convention the rest of the workspace follows.
#[derive(Clone, Debug, Eq, PartialEq, Error)]
pub enum PlatformError {
    #[error("files.platform.error.watch_failed:{0}")]
    WatchFailed(String),
    #[error("files.platform.error.read_failed:{path}:{reason}")]
    ReadFailed { path: String, reason: String },
    #[error("files.platform.error.not_a_directory:{0}")]
    NotADirectory(String),
}
