//! One navigable pane: history, the list it is showing, and the listing that
//! is filling it.
//!
//! This is where "cancel obsolete work when the user navigates away" is
//! actually enforced. A pane holds at most one live [`ListingSession`], and
//! every navigation replaces it — cancelling the old one first, so the reader
//! thread stops at its next entry rather than finishing a directory nobody is
//! looking at any more.
//!
//! The pane never blocks. [`Pane::pump`] drains whatever has arrived and
//! returns whether anything changed; a frame calls it and draws. There is no
//! method here that waits for a listing to finish.

use crate::hidden::HiddenPreference;
use crate::listing::{DirectoryReader, ListingRequest, ListingSession};
use crate::location::Location;
use crate::model::DirectoryModel;
use crate::sort::SortOrder;
use crate::tabs::ViewPreferences;

/// A single navigable view.
pub struct Pane {
    history: crate::history::History,
    model: DirectoryModel,
    session: Option<ListingSession>,
    preferences: ViewPreferences,
    batch_size: Option<usize>,
}

impl Pane {
    /// Opens a pane at a location and starts listing it.
    pub fn open(
        location: Location,
        preferences: ViewPreferences,
        reader: &dyn DirectoryReader,
    ) -> Self {
        let mut pane = Self {
            history: crate::history::History::new(location.clone()),
            model: DirectoryModel::new(location, preferences.order, preferences.hidden),
            session: None,
            preferences,
            batch_size: None,
        };
        pane.start_listing(reader);
        pane
    }

    /// Opens a pane on a history that already exists, and starts listing
    /// wherever that history currently is.
    ///
    /// This is what reopening a closed tab needs. [`crate::TabSet::close`]
    /// keeps the whole [`crate::History`], and restoring it into a pane built
    /// with [`Pane::open`] would have thrown that history away and left the
    /// user at the folder with Back greyed out — which is exactly the failure
    /// the recently-closed stack exists to prevent.
    pub fn resume(
        history: crate::history::History,
        preferences: ViewPreferences,
        reader: &dyn DirectoryReader,
    ) -> Self {
        let location = history.current().clone();
        let mut pane = Self {
            history,
            model: DirectoryModel::new(location, preferences.order, preferences.hidden),
            session: None,
            preferences,
            batch_size: None,
        };
        pane.start_listing(reader);
        pane
    }

    /// Overrides the batch size for subsequent listings. Tests and benchmarks
    /// use this to control how finely a listing is chunked.
    pub fn with_batch_size(mut self, batch_size: usize) -> Self {
        self.batch_size = Some(batch_size);
        self
    }

    pub fn location(&self) -> &Location {
        self.history.current()
    }

    pub fn history(&self) -> &crate::history::History {
        &self.history
    }

    pub fn model(&self) -> &DirectoryModel {
        &self.model
    }

    pub fn model_mut(&mut self) -> &mut DirectoryModel {
        &mut self.model
    }

    pub fn preferences(&self) -> ViewPreferences {
        self.preferences
    }

    /// Whether a listing is still running. Used to draw a progress hint, not
    /// to decide whether the list is usable — a partial list is usable.
    pub fn is_listing(&self) -> bool {
        self.session
            .as_ref()
            .is_some_and(|session| !session.is_complete())
    }

    /// Goes to a new location, cancelling whatever was still being listed.
    ///
    /// Returns false when the pane was already there, in which case nothing is
    /// cancelled and nothing is restarted.
    pub fn navigate_to(&mut self, location: Location, reader: &dyn DirectoryReader) -> bool {
        if !self.history.visit(location) {
            return false;
        }
        self.start_listing(reader);
        true
    }

    pub fn go_back(&mut self, reader: &dyn DirectoryReader) -> bool {
        if self.history.back().is_none() {
            return false;
        }
        self.start_listing(reader);
        true
    }

    pub fn go_forward(&mut self, reader: &dyn DirectoryReader) -> bool {
        if self.history.forward().is_none() {
            return false;
        }
        self.start_listing(reader);
        true
    }

    pub fn go_to_parent(&mut self, reader: &dyn DirectoryReader) -> bool {
        if self.history.go_to_parent().is_none() {
            return false;
        }
        self.start_listing(reader);
        true
    }

    /// Lists the current location again, discarding what is loaded. This is
    /// the explicit cache invalidation a watcher resynchronization triggers.
    pub fn reload(&mut self, reader: &dyn DirectoryReader) {
        self.start_listing(reader);
    }

    /// Changes the sort order. No listing is restarted: the entries are
    /// already here.
    pub fn set_order(&mut self, order: SortOrder) {
        self.preferences.order = order;
        self.model.set_order(order);
    }

    /// Shows or hides hidden entries without reloading, which is Issue #6's
    /// requirement for `Ctrl+H`.
    pub fn set_hidden_preference(&mut self, hidden: HiddenPreference) {
        self.preferences.hidden = hidden;
        self.model.set_hidden_preference(hidden);
    }

    /// Flips the hidden preference and returns the new value.
    pub fn toggle_hidden(&mut self) -> bool {
        let mut hidden = self.preferences.hidden;
        let shown = hidden.toggle();
        self.set_hidden_preference(hidden);
        shown
    }

    /// Drains whatever the producer has sent. Returns whether the model
    /// changed and therefore whether a redraw is needed.
    ///
    /// Every batch that has arrived since the last call is applied, then
    /// merged once. That is what keeps a large directory affordable: the
    /// merge touches the whole ordered list, so paying for it per frame
    /// rather than per batch is the difference between a listing that
    /// assembles in milliseconds and one that spends seconds re-merging.
    pub fn pump(&mut self) -> bool {
        let Some(session) = self.session.as_mut() else {
            return false;
        };
        let events = session.drain();
        let mut changed = false;
        for event in events {
            changed |= self.model.apply(event);
        }
        self.model.commit();
        changed
    }

    /// The token for the listing currently running, for a test or a caller
    /// that needs to prove cancellation happened.
    pub fn cancellation_token(&self) -> Option<crate::listing::CancellationToken> {
        self.session.as_ref().map(|session| session.token().clone())
    }

    fn start_listing(&mut self, reader: &dyn DirectoryReader) {
        // Cancel before replacing. Dropping the session would cancel it too,
        // but doing it explicitly means the old producer sees the flag before
        // the new one is even created, so the two never overlap in work.
        if let Some(previous) = &self.session {
            previous.cancel();
        }
        self.session = None;

        let location = self.history.current().clone();
        let mut request = ListingRequest::new(location.clone());
        if let Some(batch_size) = self.batch_size {
            request = request.with_batch_size(batch_size);
        }
        self.model = DirectoryModel::new(location, self.preferences.order, self.preferences.hidden);
        self.model.restart(request.listing);
        let (session, sink) = ListingSession::start(&request);
        self.session = Some(session);
        reader.start(request, sink);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entry::{Entry, EntryKind};
    use crate::listing::{Cancelled, ListingSink};
    use crate::location::LocalPath;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    /// A reader that emits a fixed number of entries, counting how many it
    /// actually produced. The counter is what proves cancellation stopped the
    /// work rather than merely being requested.
    #[derive(Default)]
    struct CountingReader {
        entries: usize,
        produced: Arc<AtomicUsize>,
        /// Sinks kept alive so the producer can be stepped from the test.
        pending: Mutex<Vec<ListingSink>>,
    }

    impl CountingReader {
        fn new(entries: usize) -> Self {
            Self {
                entries,
                produced: Arc::new(AtomicUsize::new(0)),
                pending: Mutex::new(Vec::new()),
            }
        }

        /// Runs every started listing to completion, stopping any that has
        /// been cancelled.
        fn run(&self) {
            let mut pending = self.pending.lock().unwrap();
            for mut sink in pending.drain(..) {
                let mut result = Ok(());
                for index in 0..self.entries {
                    let entry = Entry::file(
                        format!("entry{index:06}"),
                        LocalPath::new(format!("/d/entry{index:06}")).unwrap(),
                        EntryKind::File,
                    );
                    result = sink.push(entry);
                    if result.is_err() {
                        break;
                    }
                    self.produced.fetch_add(1, Ordering::Relaxed);
                }
                if result == Ok(()) {
                    let _ = sink.finish();
                }
            }
        }
    }

    impl DirectoryReader for CountingReader {
        fn start(&self, _request: ListingRequest, sink: ListingSink) {
            self.pending.lock().unwrap().push(sink);
        }
    }

    #[test]
    fn navigating_away_cancels_the_listing_that_was_running() {
        let reader = CountingReader::new(1_000);
        let mut pane = Pane::open(
            Location::local("/first").unwrap(),
            ViewPreferences::default(),
            &reader,
        );
        let first_token = pane.cancellation_token().unwrap();
        assert!(!first_token.is_cancelled());

        pane.navigate_to(Location::local("/second").unwrap(), &reader);
        assert!(
            first_token.is_cancelled(),
            "the abandoned listing must be cancelled before the new one starts"
        );
        let second_token = pane.cancellation_token().unwrap();
        assert!(!second_token.is_cancelled());

        // Only the second listing produces anything.
        reader.run();
        assert_eq!(reader.produced.load(Ordering::Relaxed), 1_000);
    }

    #[test]
    fn a_cancelled_producer_stops_instead_of_finishing_the_directory() {
        let reader = CountingReader::new(10_000);
        let mut pane = Pane::open(
            Location::local("/first").unwrap(),
            ViewPreferences::default(),
            &reader,
        );
        let token = pane.cancellation_token().unwrap();
        token.cancel();
        reader.run();
        // The producer stopped at its first push rather than walking ten
        // thousand entries.
        assert_eq!(reader.produced.load(Ordering::Relaxed), 0);
        // The model learns the listing was abandoned rather than sitting in
        // "loading" forever, and no entries arrived.
        assert!(pane.pump());
        assert_eq!(pane.model().total_len(), 0);
        assert_eq!(
            pane.model().status(),
            &crate::model::ListingStatus::Cancelled
        );
        assert!(!pane.is_listing());
    }

    #[test]
    fn back_and_forward_restart_the_listing_each_time() {
        let reader = CountingReader::new(2);
        let mut pane = Pane::open(
            Location::local("/a").unwrap(),
            ViewPreferences::default(),
            &reader,
        );
        pane.navigate_to(Location::local("/b").unwrap(), &reader);
        reader.run();
        pane.pump();
        assert_eq!(pane.model().total_len(), 2);

        assert!(pane.go_back(&reader));
        assert_eq!(pane.location(), &Location::local("/a").unwrap());
        // The model was reset by the navigation, before any new batch arrived.
        assert_eq!(pane.model().total_len(), 0);
        reader.run();
        pane.pump();
        assert_eq!(pane.model().total_len(), 2);

        assert!(pane.go_forward(&reader));
        assert_eq!(pane.location(), &Location::local("/b").unwrap());
        assert!(!pane.go_forward(&reader));
    }

    #[test]
    fn toggling_hidden_entries_does_not_start_a_new_listing() {
        let reader = CountingReader::new(4);
        let mut pane = Pane::open(
            Location::local("/a").unwrap(),
            ViewPreferences::default(),
            &reader,
        );
        reader.run();
        pane.pump();
        let token = pane.cancellation_token().unwrap();
        assert!(pane.toggle_hidden());
        assert!(
            !token.is_cancelled(),
            "revealing hidden entries must not restart the listing"
        );
        assert_eq!(pane.model().total_len(), 4);
    }

    #[test]
    fn resuming_a_pane_keeps_the_history_it_was_handed() {
        let reader = CountingReader::new(1);
        let mut history = crate::history::History::new(at("/a"));
        history.visit(at("/b"));
        history.visit(at("/c"));

        let mut pane = Pane::resume(history, ViewPreferences::default(), &reader);
        assert_eq!(pane.location(), &at("/c"));
        assert!(pane.history().can_go_back());
        assert!(pane.go_back(&reader));
        assert_eq!(pane.location(), &at("/b"));
        assert!(pane.go_back(&reader));
        assert_eq!(pane.location(), &at("/a"));
        assert!(!pane.go_back(&reader));
    }

    fn at(path: &str) -> Location {
        Location::local(path).unwrap()
    }

    #[test]
    fn dropping_the_pane_cancels_its_listing() {
        let reader = CountingReader::new(5);
        let pane = Pane::open(
            Location::local("/a").unwrap(),
            ViewPreferences::default(),
            &reader,
        );
        let token = pane.cancellation_token().unwrap();
        drop(pane);
        assert!(token.is_cancelled());
        reader.run();
        assert_eq!(reader.produced.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn a_producer_cannot_emit_after_cancellation() {
        let reader = CountingReader::new(1);
        let mut pane = Pane::open(
            Location::local("/a").unwrap(),
            ViewPreferences::default(),
            &reader,
        );
        let token = pane.cancellation_token().unwrap();
        token.cancel();
        let mut pending = reader.pending.lock().unwrap();
        let sink = pending.first_mut().unwrap();
        let entry = Entry::file("x", LocalPath::new("/d/x").unwrap(), EntryKind::File);
        assert_eq!(sink.push(entry), Err(Cancelled));
        drop(pending);
        assert!(!pane.pump());
    }
}
