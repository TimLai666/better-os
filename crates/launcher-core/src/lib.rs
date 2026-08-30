//! Better Launcher's search brain: the index, the matcher, the ranking, and
//! the two models the overlay draws.
//!
//! Issue #2 asks for one overlay with two states driven by the current query,
//! and requires the search engine to be isolated here so it can be
//! benchmarked without starting a window. Everything in this crate follows
//! from those two sentences:
//!
//! - [`SearchIndex`] is built from the shared catalog's records. There is no
//!   desktop-entry parser here. `app-catalog-core` owns that, and a second
//!   parser would be a second set of rules about what counts as an
//!   application.
//! - [`LauncherState`] holds a query and nothing else. [`LauncherState::view`]
//!   returns the application library for a blank query and ranked results for
//!   anything else, so clearing the search row cannot leave the surface in a
//!   stale mode.
//! - Ranking is deterministic. The order of two results never depends on the
//!   order records arrived in, on hashing, or on the clock. See
//!   [`matcher`] for the rules and their precedence.
//! - Usage history reaches ranking only through [`UsageStore`], and only when
//!   [`RankingOptions::usage_weighting`] is switched on. It is off by default,
//!   because Issue #2 defers that decision to an ADR rather than settling it
//!   in code.
//!
//! What this crate does not do is as load-bearing as what it does. It performs
//! no I/O of any kind: no network request, no file read, no clock. It depends
//! on one crate, `app-catalog-core`, which depends on `thiserror` and nothing
//! else, so its benchmarks run with no display backend and its dependency
//! graph is small enough to check in a test — see `tests/dependencies.rs`.
//! Nothing here reads a running process, and only an application's program
//! name is indexed, never the arguments beside it.
//!
//! ```
//! use app_catalog_core::{DesktopFile, DesktopId, EntryScope, ApplicationRecord, NoProbe};
//! use launcher_core::{IndexOptions, LauncherState, LauncherView, NoUsage, RankingOptions, SearchIndex};
//!
//! let entry = "[Desktop Entry]\nType=Application\nName=Text Editor\nKeywords=notes;\nExec=gedit %U\n";
//! let file = DesktopFile::parse(entry).unwrap();
//! let record = ApplicationRecord::from_desktop_file(
//!     DesktopId::new("org.gnome.gedit.desktop").unwrap(),
//!     "/usr/share/applications/org.gnome.gedit.desktop".into(),
//!     EntryScope::System,
//!     &file,
//!     &NoProbe,
//! )
//! .unwrap();
//!
//! let index = SearchIndex::build([&record], &IndexOptions::new());
//! let mut state = LauncherState::new();
//! assert!(matches!(state.view(&index, &RankingOptions::default(), &NoUsage), LauncherView::Browse(_)));
//!
//! state.set_query("edit");
//! let results = index.search("edit", &RankingOptions::default(), &NoUsage);
//! assert_eq!(results.results()[0].application.display_name, "Text Editor");
//! ```

pub mod index;
pub mod matcher;
pub mod model;
pub mod text;
pub mod usage;

pub use index::{IndexOptions, SearchIndex};
pub use matcher::{FieldKind, FieldMatch, MatchKind};
pub use model::{
    BrowseModel, BrowseSection, LauncherApplication, LauncherState, LauncherView, MAIN_CATEGORIES,
    OTHER_CATEGORY, RankingOptions, SearchModel, SearchResult,
};
pub use text::FoldedText;
pub use usage::{InMemoryUsage, LaunchEvent, NoUsage, UsageStore};
