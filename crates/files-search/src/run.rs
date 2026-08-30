//! Providers, and the run that streams one search's results.
//!
//! A run is advanced in slices the caller sizes, which is how results stream
//! incrementally and how typing stays cheap. The caller decides the budget from
//! its own frame time; nothing here has an opinion about milliseconds.

use std::sync::Arc;

use files_core::{Entry, EntryId};

use crate::query::{SearchQuery, SearchScope};
use crate::rank::{DefaultRanker, Ranker, Score, compare_hits};

/// One result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SearchHit {
    pub id: EntryId,
    pub name: String,
    pub score: Score,
}

/// Whether a run needs candidates from the caller, or produces its own.
///
/// The current-location provider is fed the entries the pane already holds,
/// which is why searching where you already are costs no I/O. An indexed
/// provider would answer [`RunDemand::SelfDriven`] and be advanced instead.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RunDemand {
    Candidates,
    SelfDriven,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RunState {
    /// More results may still arrive.
    Streaming,
    /// Everything in scope has been considered.
    Finished,
}

impl RunState {
    pub fn is_finished(self) -> bool {
        matches!(self, RunState::Finished)
    }
}

/// One search in progress.
pub trait SearchRun: Send {
    fn demand(&self) -> RunDemand;

    /// Considers a slice of candidates. Returns how many became hits.
    ///
    /// The default is zero, so a self-driven provider does not have to write a
    /// method that says "not me".
    fn offer(&mut self, entries: &[Entry]) -> usize {
        let _ = entries;
        0
    }

    /// Produces up to `budget` more results from the run's own source.
    fn advance(&mut self, budget: usize) -> RunState {
        let _ = budget;
        RunState::Finished
    }

    /// The hits so far, best first. Callers read this every frame.
    fn hits(&self) -> &[SearchHit];

    /// Tells the run there are no more candidates coming — the listing that
    /// was feeding it completed.
    fn mark_complete(&mut self);

    fn state(&self) -> RunState;

    /// How many candidates have been considered, for the "searching 3,412 of
    /// 100,000" line a long search needs.
    fn considered(&self) -> usize;
}

/// Something that can answer a query.
pub trait SearchProvider {
    fn id(&self) -> &'static str;

    /// Whether this provider covers the scope. A provider that does not is
    /// skipped rather than asked and refused.
    fn supports(&self, scope: &SearchScope) -> bool;

    fn begin(&self, query: SearchQuery) -> Box<dyn SearchRun>;
}

/// Searches the entries of the location that is already open.
pub struct CurrentDirectoryProvider {
    ranker: Arc<dyn Ranker>,
}

impl Default for CurrentDirectoryProvider {
    fn default() -> Self {
        Self {
            ranker: Arc::new(DefaultRanker),
        }
    }
}

impl CurrentDirectoryProvider {
    pub fn with_ranker(ranker: Arc<dyn Ranker>) -> Self {
        Self { ranker }
    }

    pub fn ranker_id(&self) -> &'static str {
        self.ranker.id()
    }
}

impl SearchProvider for CurrentDirectoryProvider {
    fn id(&self) -> &'static str {
        "current-location"
    }

    fn supports(&self, scope: &SearchScope) -> bool {
        matches!(scope, SearchScope::CurrentLocation(_))
    }

    fn begin(&self, query: SearchQuery) -> Box<dyn SearchRun> {
        Box::new(CurrentDirectoryRun::with_ranker(query, self.ranker.clone()))
    }
}

/// One current-location search.
///
/// It holds a handle to the ranker rather than borrowing the provider, so a run
/// can outlive the provider that started it and be moved to another thread if a
/// later version wants that.
pub struct CurrentDirectoryRun {
    query: SearchQuery,
    normalized: String,
    ranker: Arc<dyn Ranker>,
    hits: Vec<SearchHit>,
    considered: usize,
    state: RunState,
}

impl CurrentDirectoryRun {
    /// A run with the ranker that ships.
    pub fn new(query: SearchQuery) -> Self {
        Self::with_ranker(query, Arc::new(DefaultRanker))
    }

    pub fn with_ranker(query: SearchQuery, ranker: Arc<dyn Ranker>) -> Self {
        let normalized = query.normalized_text();
        Self {
            query,
            normalized,
            ranker,
            hits: Vec::new(),
            considered: 0,
            state: RunState::Streaming,
        }
    }

    pub fn query(&self) -> &SearchQuery {
        &self.query
    }

    /// Inserts a hit in order, so `hits()` is always sorted and the caller
    /// never has to sort a partial result that is about to change.
    fn insert(&mut self, hit: SearchHit) {
        let position = self
            .hits
            .binary_search_by(|existing| {
                compare_hits(&existing.score, &existing.name, &hit.score, &hit.name)
            })
            .unwrap_or_else(|position| position);
        self.hits.insert(position, hit);
    }
}

impl SearchRun for CurrentDirectoryRun {
    fn demand(&self) -> RunDemand {
        RunDemand::Candidates
    }

    fn offer(&mut self, entries: &[Entry]) -> usize {
        let mut produced = 0;
        for entry in entries {
            self.considered += 1;
            // The hidden setting is the search's own, not the view's.
            if entry.hidden.is_hidden() && !self.query.include_hidden {
                continue;
            }
            if !self.query.filters.accepts(entry) {
                continue;
            }
            let Some(score) = self.ranker.score(&entry.name, &self.normalized) else {
                continue;
            };
            self.insert(SearchHit {
                id: entry.id(),
                name: entry.name.clone(),
                score,
            });
            produced += 1;
        }
        produced
    }

    fn hits(&self) -> &[SearchHit] {
        &self.hits
    }

    fn mark_complete(&mut self) {
        self.state = RunState::Finished;
    }

    fn state(&self) -> RunState {
        self.state
    }

    fn considered(&self) -> usize {
        self.considered
    }
}
