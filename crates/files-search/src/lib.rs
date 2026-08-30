//! Search for Better Files: the query, the ranking, and the provider seam.
//!
//! Issue #6 asks for two levels of search and says the interfaces must separate
//! **provider**, **ranking**, and **UI** even though only the first level ships.
//! That separation is the whole point of this crate, so it is worth naming what
//! each part owns.
//!
//! - [`SearchQuery`] and [`Filters`] are what was asked for. They can express
//!   the whole list Issue #6 names — fuzzy, prefix, extension, type, date, and
//!   size — regardless of which provider answers.
//! - [`Ranker`] decides how well a name matches. [`DefaultRanker`] is the one
//!   that ships, and swapping it changes ordering without touching anything
//!   that walks a directory or draws a row.
//! - [`SearchProvider`] finds candidates. The one that ships,
//!   [`CurrentDirectoryProvider`], is fed the entries the pane already has, so
//!   searching the current location costs no I/O at all. An indexed provider
//!   later produces its own candidates and says so through [`RunDemand`],
//!   which is the reason that enum exists.
//! - Nothing here draws anything or knows what a frame is.
//!
//! **Typing does not block navigation** because no method here does I/O or
//! waits. A run is advanced in bounded slices — [`SearchRun::offer`] takes as
//! many entries as the caller decided to spend this frame — so a search over
//! a hundred thousand entries is spread across frames instead of stalling one.

pub mod query;
pub mod rank;
pub mod run;

pub use query::{Filters, SearchQuery, SearchScope};
pub use rank::{DefaultRanker, MatchKind, Ranker, Score};
pub use run::{
    CurrentDirectoryProvider, CurrentDirectoryRun, RunDemand, RunState, SearchHit, SearchProvider,
    SearchRun,
};
