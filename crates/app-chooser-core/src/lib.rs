//! The compatibility, ranking, and association logic behind Better App Chooser.
//!
//! This crate answers three questions and nothing else:
//!
//! - Which installed applications are plausible for this file, and in what
//!   order? [`rank`] sorts the shared catalog's records into the three sections
//!   Issue #4 requires, deterministically, with no GPUI and no render thread.
//! - What did the user actually choose? [`AppSelection`] is an application
//!   identity — a desktop ID and an optional action — never a path inside a
//!   virtual Applications location. It carries an executable path only when it
//!   was produced by the separate executable-selection mode.
//! - How is "Always use for this file type" written down without damaging
//!   anything else the user has configured? [`AssociationStore`] edits exactly
//!   one line of the user's `mimeapps.list` and writes a rollback record before
//!   it touches the file at all.
//!
//! Two rules run through the whole crate:
//!
//! - There is no MIME database here. Type relationships come from the
//!   `shared-mime-info` data files that are already installed, read only, and
//!   an absent database degrades to "no known relationships" rather than to a
//!   guess. See [`MimeGraph`].
//! - The user's `mimeapps.list` is untrusted input. It is parsed into lines
//!   that are preserved verbatim, and anything this crate does not understand
//!   survives a write unchanged rather than being normalized away.
//!
//! Launching is not implemented here. Open Once hands the selection to the
//! shared launch path in `app-catalog-platform`, which is the only place a
//! process is started.

pub mod association;
pub mod executable;
pub mod mime;
pub mod mimeapps;
pub mod ranking;
pub mod selection;

pub use association::{
    AssociationError, AssociationOutcome, AssociationRollback, AssociationStore,
    AssociationWarning, PreviousDefault,
};
pub use executable::{
    ExecutableResolution, ExecutableWarning, accept_executable_path, browse_roots,
    list_executables, resolve_executable,
};
pub use mime::{MimeGraph, MimeResolution};
pub use mimeapps::{
    ADDED_ASSOCIATIONS, DEFAULT_APPLICATIONS, MimeAppsFile, MimeAssociations, REMOVED_ASSOCIATIONS,
};
pub use ranking::{
    ChooserRequest, ChooserSections, Compatibility, RankedApplication, UsageHistory, rank,
};
pub use selection::{AppSelection, AssociationMode};
