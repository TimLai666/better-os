//! The search field, and what it does to the content area.
//!
//! `files-search` owns the query, the ranking, and the provider. What is left
//! here is the part that belongs to a window: how much of a directory to
//! consider per frame, what the content area draws while a search is running,
//! and what the scope line says.
//!
//! **Typing does not block navigation.** A keystroke restarts the run and
//! nothing else; the run then consumes [`SLICE`] entries per frame from the
//! model the pane already holds. On a 100,000-entry directory that is a
//! fraction of a millisecond of work per frame, spread over about a second,
//! with every intermediate result already in final order. Navigating away
//! clears the search rather than waiting for it.
//!
//! **The scope is visible.** [`SearchState::scope_label`] is drawn beside the
//! field, because a search whose scope is implicit is a search whose empty
//! result means nothing.

use files_core::{DirectoryModel, Entry, EntryId, ListingStatus, Location};
use files_search::{
    CurrentDirectoryProvider, Filters, SearchHit, SearchProvider, SearchQuery, SearchRun,
    SearchScope,
};

use crate::i18n::Copy;

/// How many entries one frame considers.
///
/// Sized so the work is invisible rather than so the search finishes fast:
/// 4,096 name comparisons is tens of microseconds, and a directory big enough
/// to need more than a handful of frames is a directory whose listing is still
/// streaming anyway.
pub const SLICE: usize = 4_096;

/// One search in the window.
pub struct SearchState {
    provider: CurrentDirectoryProvider,
    /// What is in the field. Kept separately from the run's query so an empty
    /// field can close the search without a run existing.
    pub text: String,
    /// Issue #6: hidden files follow an explicit search setting, not the
    /// view's. Off by default, and persisted with the other preferences.
    pub include_hidden: bool,
    pub filters: Filters,
    run: Option<Box<dyn SearchRun>>,
    scope: Option<SearchScope>,
    /// How far through the model's entries the run has been fed.
    cursor: usize,
}

impl Default for SearchState {
    fn default() -> Self {
        Self {
            provider: CurrentDirectoryProvider::default(),
            text: String::new(),
            include_hidden: false,
            filters: Filters::default(),
            run: None,
            scope: None,
            cursor: 0,
        }
    }
}

impl SearchState {
    /// Whether the content area is showing search results rather than the
    /// directory.
    pub fn is_active(&self) -> bool {
        self.run.is_some()
    }

    pub fn hits(&self) -> &[SearchHit] {
        self.run.as_ref().map_or(&[], |run| run.hits())
    }

    pub fn considered(&self) -> usize {
        self.run.as_ref().map_or(0, |run| run.considered())
    }

    pub fn scope(&self) -> Option<&SearchScope> {
        self.scope.as_ref()
    }

    /// Whether every candidate has been considered.
    pub fn is_complete(&self) -> bool {
        self.run
            .as_ref()
            .is_some_and(|run| run.state().is_finished())
    }

    /// Types into the field. Restarts the run against the given location.
    ///
    /// An empty query with no filters closes the search, which is what pressing
    /// Escape or clearing the field does. That is deliberate: an empty search
    /// showing every entry is indistinguishable from the directory, and leaving
    /// the results view up would hide the fact that it is a view.
    pub fn set_text(&mut self, text: impl Into<String>, location: &Location) {
        self.text = text.into();
        self.restart(location);
    }

    pub fn set_include_hidden(&mut self, include: bool, location: &Location) {
        self.include_hidden = include;
        if self.is_active() {
            self.restart(location);
        }
    }

    pub fn set_filters(&mut self, filters: Filters, location: &Location) {
        self.filters = filters;
        self.restart(location);
    }

    /// Restarts against the current location. Called when the query changes and
    /// when the pane reloads under a running search.
    pub fn restart(&mut self, location: &Location) {
        let query = SearchQuery::new(
            self.text.clone(),
            SearchScope::CurrentLocation(location.clone()),
        )
        .including_hidden(self.include_hidden)
        .with_filters(self.filters.clone());
        if query.is_empty() {
            self.clear();
            return;
        }
        self.scope = Some(query.scope.clone());
        self.run = Some(self.provider.begin(query));
        self.cursor = 0;
    }

    /// Ends the search. Navigating away does this, and so does clearing the
    /// field.
    pub fn clear(&mut self) {
        self.run = None;
        self.scope = None;
        self.cursor = 0;
    }

    /// Ends the search and empties the field.
    pub fn close(&mut self) {
        self.text.clear();
        self.clear();
    }

    /// Feeds one frame's worth of the model to the run.
    ///
    /// Returns whether anything changed, which is what decides a redraw. The
    /// model may still be streaming: entries that arrive after the cursor has
    /// passed them are picked up because the cursor is an index into a list
    /// that only grows at the end during a listing.
    pub fn pump(&mut self, model: &DirectoryModel) -> bool {
        let Some(run) = self.run.as_mut() else {
            return false;
        };
        // Search sees everything the model holds, including entries the view is
        // hiding, because the hidden rule for a search is the search's own.
        let total = model.total_len();
        if self.cursor >= total {
            if matches!(model.status(), ListingStatus::Complete) && !run.state().is_finished() {
                run.mark_complete();
                return true;
            }
            return false;
        }
        let end = (self.cursor + SLICE).min(total);
        // `iter_all`, not `iter_visible`: the hidden rule for a search is the
        // search's own, so it has to see what the view is hiding and decide
        // for itself.
        let slice: Vec<Entry> = model
            .iter_all()
            .skip(self.cursor)
            .take(end - self.cursor)
            .cloned()
            .collect();
        run.offer(&slice);
        self.cursor = end;
        if self.cursor >= total && matches!(model.status(), ListingStatus::Complete) {
            run.mark_complete();
        }
        true
    }

    /// The entry ids to draw, in result order.
    pub fn result_ids(&self) -> Vec<EntryId> {
        self.hits().iter().map(|hit| hit.id.clone()).collect()
    }

    /// The line under the field: what was searched and how far it got.
    pub fn status_line(&self, c: &'static Copy) -> String {
        if !self.is_active() {
            return String::new();
        }
        let found = self.hits().len();
        if self.is_complete() {
            format!("{found} {}", c.search_matches)
        } else {
            format!(
                "{found} {} · {} {}",
                c.search_matches,
                self.considered(),
                c.search_scanned
            )
        }
    }

    /// The scope, named for a person.
    pub fn scope_label(&self, location: &Location, c: &'static Copy) -> String {
        match self.scope.as_ref() {
            Some(SearchScope::CurrentLocation(_)) | None => {
                format!("{} {}", c.search_in, location.display_name())
            }
            Some(SearchScope::Recursive(_)) => c.search_scope_recursive.to_string(),
            Some(SearchScope::Indexed) => c.search_scope_indexed.to_string(),
        }
    }

    /// The empty-state message, or `None` when there is something to draw.
    pub fn empty_state(&self, c: &'static Copy) -> Option<&'static str> {
        if !self.is_active() || !self.hits().is_empty() {
            return None;
        }
        Some(if self.is_complete() {
            c.search_no_matches
        } else {
            c.search_running
        })
    }
}
