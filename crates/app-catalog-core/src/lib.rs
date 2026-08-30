//! The one application catalog Better OS components share.
//!
//! Every Better OS surface that needs to know what is installed — the file
//! manager's Applications location, the launcher index, the application
//! chooser — reads records produced here. Nothing else parses a `.desktop`
//! file, because a second parser is a second set of rules about what counts as
//! an application.
//!
//! Two properties hold throughout:
//!
//! - A desktop entry is untrusted input. It is validated before a record
//!   exists, and a rejection carries a stable machine key rather than a panic
//!   or a half-populated record.
//! - An application's identity is its desktop ID, never a path. A record that
//!   has no single canonical executable says so instead of offering a guess a
//!   caller might try to run.
//!
//! This crate is pure: it does no I/O, has no GPUI dependency, and does not
//! know what a render thread is. Directory discovery, change watching, and
//! process spawning live in `app-catalog-platform`.

pub mod catalog;
pub mod entry;
pub mod error;
pub mod exec;
pub mod plan;
pub mod record;

pub use catalog::{Catalog, CatalogBuilder, DirectoryRank, RejectedEntry, ShadowedEntry};
pub use entry::{DesktopFile, Locale, LocalizedList, LocalizedText};
pub use error::{EntryError, LaunchError};
pub use exec::{ExecLine, Invocation, LaunchTarget, TargetAcceptance};
pub use plan::{DBusActivation, DBusMethod, LaunchPlan};
pub use record::{
    ApplicationRecord, CapabilityFlags, DesktopAction, DesktopEnvironments, DesktopId, EntryScope,
    EntrySource, EntryWarning, ExclusionReason, ExecutableProbe, ExecutableStatus, IconReference,
    MimeType, NoCanonicalExecutable, NoProbe, SourceKind, Visibility, VisibilityRules,
};
