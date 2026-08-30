//! Better Files' domain model: locations, entries, listings, and navigation.
//!
//! Two rules shape everything here, both from Issue #6.
//!
//! **A location is a type, not a path.** [`Location`] is a closed enum. Trash,
//! Recent, Applications, a specific storage device, and the network kinds that
//! do not exist yet are each their own shape, and the only way to get a
//! filesystem path out of one is [`Location::as_local_path`], which answers
//! `None` for the ones that have none. The same rule runs through
//! [`Entry`]: an Applications row is an [`EntryBody::Application`] carrying a
//! desktop ID, and there is no accessor, conversion, or trait implementation
//! that turns it into a `PathBuf`.
//!
//! **Nothing scans a directory synchronously.** A listing is a stream of
//! batches over a channel ([`ListingSession`] / [`ListingSink`]), carrying a
//! [`CancellationToken`] the consumer trips by navigating away. A producer
//! cannot emit an entry without checking it, so an abandoned listing stops at
//! its next entry instead of finishing work nobody will see.
//!
//! This crate is pure. It does no I/O, has no GPUI dependency, and never
//! parses a desktop entry — the Applications location consumes records the
//! shared `app-catalog-core` catalog produced. `files-platform` is the half
//! that touches the host.

pub mod applications;
pub mod cache;
pub mod entry;
pub mod error;
pub mod hidden;
pub mod history;
pub mod listing;
pub mod location;
pub mod model;
pub mod pane;
pub mod selection;
pub mod sort;
pub mod tabs;

pub use applications::{ApplicationView, OpenIntent, OpenRefusal, list_applications, open_intent};
pub use cache::ListingCache;
pub use entry::{
    ApplicationFacts, Entry, EntryBody, EntryId, EntryKind, EntrySize, FileFacts, FileTime,
    HiddenReason, HiddenState, PermissionsSummary, SymlinkStatus, TrashedFacts,
};
pub use error::{ListingError, LocationError, NavigationError};
pub use hidden::{HiddenPreference, HiddenRules};
pub use history::{DEFAULT_HISTORY_LIMIT, History};
pub use listing::{
    CancellationToken, Cancelled, DEFAULT_BATCH_SIZE, DirectoryReader, ListingBatch, ListingEvent,
    ListingId, ListingRequest, ListingSession, ListingSink, ListingSummary, SkippedEntry,
};
pub use location::{
    DeviceLocation, LocalPath, Location, LocationKind, NetworkLocation, NetworkScheme,
    TrashLocation, UnsupportedLocation, UnsupportedReason,
};
pub use model::{DirectoryModel, ListingStatus, RefreshEvent};
pub use pane::Pane;
pub use selection::Selection;
pub use sort::{SortDirection, SortKey, SortOrder, natural_compare};
pub use tabs::{ClosedTab, Tab, TabId, TabSet, ViewPreferences};
