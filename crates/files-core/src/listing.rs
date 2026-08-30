//! The streaming listing protocol.
//!
//! A directory is never delivered as a `Vec`. A producer — a reader thread in
//! `files-platform`, or a fake in a test — pushes entries into a
//! [`ListingSink`], which flushes them as [`ListingBatch`]es over a channel to
//! a [`ListingSession`] the consumer drains without blocking. The render
//! thread's whole involvement is draining a queue.
//!
//! Cancellation is part of the protocol rather than bolted on. Every push
//! checks the token, so a producer that is halfway through a hundred thousand
//! `stat` calls stops at the next entry instead of finishing work nobody is
//! going to look at. The producer cannot ignore it: `push` returns
//! `Err(Cancelled)` and there is no way to emit an entry without calling it.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};

use crate::entry::Entry;
use crate::error::ListingError;
use crate::location::Location;

/// How many entries a batch holds before it is sent.
///
/// Small enough that the first batch appears almost immediately on a large
/// directory, large enough that a hundred thousand entries do not become a
/// hundred thousand channel sends.
pub const DEFAULT_BATCH_SIZE: usize = 256;

/// A shared stop flag.
///
/// Cloning shares the flag; cancelling any clone cancels all of them. This is
/// the only channel between a consumer navigating away and a producer already
/// running on another thread.
#[derive(Clone, Debug, Default)]
pub struct CancellationToken {
    flag: Arc<AtomicBool>,
}

impl CancellationToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.flag.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.flag.load(Ordering::Acquire)
    }
}

/// The producer was told to stop.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Cancelled;

/// Identifies one listing run.
///
/// A late batch from a listing the user already navigated away from carries
/// the old id, so a consumer can drop it even if it arrives after a new
/// listing has started.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ListingId(u64);

impl ListingId {
    pub fn next() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        ListingId(COUNTER.fetch_add(1, Ordering::Relaxed))
    }

    pub fn value(self) -> u64 {
        self.0
    }
}

/// An entry the listing could not deliver, reported alongside the ones it
/// could.
///
/// One unreadable file does not fail a directory. The rest of the listing
/// continues and the failure is named here, so the view can show it rather
/// than silently returning a short list.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SkippedEntry {
    pub name: String,
    pub error: ListingError,
}

/// A chunk of a listing.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ListingBatch {
    pub listing: ListingId,
    /// Batch number within this listing, starting at zero. Present so a
    /// consumer can assert it saw every batch.
    pub sequence: u64,
    pub entries: Vec<Entry>,
}

/// How a listing ended.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ListingSummary {
    pub listing: ListingId,
    pub total: usize,
    pub skipped: Vec<SkippedEntry>,
}

/// Everything a producer can say.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ListingEvent {
    Batch(ListingBatch),
    /// The directory was delivered in full.
    Complete(ListingSummary),
    /// The listing stopped because the location itself could not be read.
    Failed {
        listing: ListingId,
        error: ListingError,
    },
    /// The listing stopped because it was cancelled. Distinct from `Failed`:
    /// nothing went wrong, the answer is simply no longer wanted.
    Cancelled {
        listing: ListingId,
        delivered: usize,
    },
}

impl ListingEvent {
    pub fn listing(&self) -> ListingId {
        match self {
            ListingEvent::Batch(batch) => batch.listing,
            ListingEvent::Complete(summary) => summary.listing,
            ListingEvent::Failed { listing, .. } | ListingEvent::Cancelled { listing, .. } => {
                *listing
            }
        }
    }

    pub fn is_terminal(&self) -> bool {
        !matches!(self, ListingEvent::Batch(_))
    }
}

/// What to list.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ListingRequest {
    pub listing: ListingId,
    pub location: Location,
    pub batch_size: usize,
    /// Hide backup files, which the freedesktop rules leave to the
    /// application. Carried on the request because it changes what the reader
    /// records on each entry, not what a filter later drops.
    pub hide_backup_files: bool,
}

impl ListingRequest {
    pub fn new(location: Location) -> Self {
        Self {
            listing: ListingId::next(),
            location,
            batch_size: DEFAULT_BATCH_SIZE,
            hide_backup_files: false,
        }
    }

    pub fn with_batch_size(mut self, batch_size: usize) -> Self {
        self.batch_size = batch_size.max(1);
        self
    }

    pub fn hiding_backup_files(mut self, hide: bool) -> Self {
        self.hide_backup_files = hide;
        self
    }
}

/// The producer's end.
///
/// Dropped without `finish` or `fail`, it emits `Cancelled`, so a reader
/// thread that panics or returns early can never leave a view waiting forever
/// for a batch that is not coming.
pub struct ListingSink {
    listing: ListingId,
    sender: Sender<ListingEvent>,
    token: CancellationToken,
    batch_size: usize,
    buffer: Vec<Entry>,
    sequence: u64,
    delivered: usize,
    skipped: Vec<SkippedEntry>,
    finished: bool,
}

impl ListingSink {
    pub fn listing(&self) -> ListingId {
        self.listing
    }

    pub fn token(&self) -> &CancellationToken {
        &self.token
    }

    /// Whether the consumer has navigated away. A producer doing expensive
    /// per-entry work should check this between entries as well as relying on
    /// `push`'s return value.
    pub fn is_cancelled(&self) -> bool {
        self.token.is_cancelled()
    }

    /// Adds one entry, flushing a full batch.
    pub fn push(&mut self, entry: Entry) -> Result<(), Cancelled> {
        if self.token.is_cancelled() {
            return Err(Cancelled);
        }
        self.buffer.push(entry);
        if self.buffer.len() >= self.batch_size {
            self.flush()?;
        }
        Ok(())
    }

    /// Records an entry that could not be read, without ending the listing.
    pub fn skip(&mut self, name: impl Into<String>, error: ListingError) -> Result<(), Cancelled> {
        if self.token.is_cancelled() {
            return Err(Cancelled);
        }
        self.skipped.push(SkippedEntry {
            name: name.into(),
            error,
        });
        Ok(())
    }

    /// Sends whatever is buffered, even a partial batch. A reader calls this
    /// when it has read everything currently available but is not done, so the
    /// first entries of a slow directory appear immediately.
    pub fn flush(&mut self) -> Result<(), Cancelled> {
        if self.token.is_cancelled() {
            return Err(Cancelled);
        }
        if self.buffer.is_empty() {
            return Ok(());
        }
        let entries = std::mem::take(&mut self.buffer);
        self.delivered += entries.len();
        let batch = ListingBatch {
            listing: self.listing,
            sequence: self.sequence,
            entries,
        };
        self.sequence += 1;
        // A closed receiver means the consumer went away without cancelling.
        // Treated as cancellation, because the work has the same value either
        // way: none.
        if self.sender.send(ListingEvent::Batch(batch)).is_err() {
            self.token.cancel();
            return Err(Cancelled);
        }
        Ok(())
    }

    /// Ends the listing successfully.
    pub fn finish(mut self) -> Result<(), Cancelled> {
        self.flush()?;
        self.finished = true;
        let summary = ListingSummary {
            listing: self.listing,
            total: self.delivered,
            skipped: std::mem::take(&mut self.skipped),
        };
        let _ = self.sender.send(ListingEvent::Complete(summary));
        Ok(())
    }

    /// Ends the listing because the location could not be read.
    pub fn fail(mut self, error: ListingError) {
        self.finished = true;
        let _ = self.sender.send(ListingEvent::Failed {
            listing: self.listing,
            error,
        });
    }
}

impl Drop for ListingSink {
    fn drop(&mut self) {
        if !self.finished {
            let _ = self.sender.send(ListingEvent::Cancelled {
                listing: self.listing,
                delivered: self.delivered,
            });
        }
    }
}

/// The consumer's end.
#[derive(Debug)]
pub struct ListingSession {
    listing: ListingId,
    location: Location,
    receiver: Receiver<ListingEvent>,
    token: CancellationToken,
    complete: bool,
}

impl ListingSession {
    /// Creates a paired session and sink for one request.
    pub fn start(request: &ListingRequest) -> (ListingSession, ListingSink) {
        let (sender, receiver) = channel();
        let token = CancellationToken::new();
        let session = ListingSession {
            listing: request.listing,
            location: request.location.clone(),
            receiver,
            token: token.clone(),
            complete: false,
        };
        let sink = ListingSink {
            listing: request.listing,
            sender,
            token,
            batch_size: request.batch_size.max(1),
            buffer: Vec::with_capacity(request.batch_size.max(1)),
            sequence: 0,
            delivered: 0,
            skipped: Vec::new(),
            finished: false,
        };
        (session, sink)
    }

    pub fn listing(&self) -> ListingId {
        self.listing
    }

    pub fn location(&self) -> &Location {
        &self.location
    }

    pub fn token(&self) -> &CancellationToken {
        &self.token
    }

    /// Stops the producer. Safe to call more than once and after completion.
    pub fn cancel(&self) {
        self.token.cancel();
    }

    /// Whether a terminal event has been drained.
    pub fn is_complete(&self) -> bool {
        self.complete
    }

    /// Takes everything the producer has sent so far without waiting.
    ///
    /// This is what a frame calls. It never blocks, so a slow directory costs
    /// the render thread nothing beyond the entries that actually arrived.
    pub fn drain(&mut self) -> Vec<ListingEvent> {
        let mut events = Vec::new();
        loop {
            match self.receiver.try_recv() {
                Ok(event) => {
                    if event.is_terminal() {
                        self.complete = true;
                    }
                    events.push(event);
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.complete = true;
                    break;
                }
            }
        }
        events
    }

    /// Blocks for the next event. For tests and for a headless consumer;
    /// nothing on a render thread calls this.
    pub fn recv(&mut self) -> Option<ListingEvent> {
        match self.receiver.recv() {
            Ok(event) => {
                if event.is_terminal() {
                    self.complete = true;
                }
                Some(event)
            }
            Err(_) => {
                self.complete = true;
                None
            }
        }
    }
}

impl Drop for ListingSession {
    /// Dropping the consumer cancels the producer. A closed tab does not leave
    /// a thread reading a directory nobody is watching.
    fn drop(&mut self) {
        self.token.cancel();
    }
}

/// Produces listings for locations it supports.
///
/// The trait exists so `files-core` can be tested against a fake producer and
/// so the Applications location, the trash, and the local filesystem are all
/// reached the same way.
pub trait DirectoryReader {
    /// Starts producing. Implementations must not block the caller: either
    /// spawn, or complete quickly for a location backed by data already in
    /// memory.
    fn start(&self, request: ListingRequest, sink: ListingSink);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entry::EntryKind;
    use crate::location::LocalPath;

    fn entry(name: &str) -> Entry {
        Entry::file(
            name,
            LocalPath::new(format!("/data/{name}")).unwrap(),
            EntryKind::File,
        )
    }

    #[test]
    fn entries_arrive_in_batches_before_the_listing_completes() {
        let request = ListingRequest::new(Location::local("/data").unwrap()).with_batch_size(2);
        let (mut session, mut sink) = ListingSession::start(&request);
        for index in 0..5 {
            sink.push(entry(&format!("file{index}"))).unwrap();
        }
        let events = session.drain();
        assert_eq!(events.len(), 2);
        assert!(!session.is_complete());
        sink.finish().unwrap();
        let tail = session.drain();
        assert_eq!(tail.len(), 2);
        assert!(session.is_complete());
        match tail.last().unwrap() {
            ListingEvent::Complete(summary) => assert_eq!(summary.total, 5),
            other => panic!("expected completion, got {other:?}"),
        }
    }

    #[test]
    fn cancelling_stops_the_producer_at_the_next_entry() {
        let request = ListingRequest::new(Location::local("/data").unwrap()).with_batch_size(2);
        let (session, mut sink) = ListingSession::start(&request);
        sink.push(entry("first")).unwrap();
        session.cancel();
        assert_eq!(sink.push(entry("second")), Err(Cancelled));
        assert_eq!(sink.flush(), Err(Cancelled));
        assert!(sink.is_cancelled());
    }

    #[test]
    fn dropping_the_sink_without_finishing_reports_cancellation() {
        let request = ListingRequest::new(Location::local("/data").unwrap());
        let (mut session, sink) = ListingSession::start(&request);
        let listing = sink.listing();
        drop(sink);
        let events = session.drain();
        assert_eq!(
            events,
            vec![ListingEvent::Cancelled {
                listing,
                delivered: 0
            }]
        );
        assert!(session.is_complete());
    }

    #[test]
    fn dropping_the_session_cancels_the_producer() {
        let request = ListingRequest::new(Location::local("/data").unwrap());
        let (session, mut sink) = ListingSession::start(&request);
        drop(session);
        assert_eq!(sink.push(entry("first")), Err(Cancelled));
    }

    #[test]
    fn an_unreadable_entry_is_skipped_without_ending_the_listing() {
        let request = ListingRequest::new(Location::local("/data").unwrap());
        let (mut session, mut sink) = ListingSession::start(&request);
        sink.push(entry("readable")).unwrap();
        sink.skip(
            "locked",
            ListingError::PermissionDenied {
                path: "/data/locked".to_string(),
            },
        )
        .unwrap();
        sink.finish().unwrap();
        let events = session.drain();
        match events.last().unwrap() {
            ListingEvent::Complete(summary) => {
                assert_eq!(summary.total, 1);
                assert_eq!(summary.skipped.len(), 1);
                assert_eq!(summary.skipped[0].name, "locked");
            }
            other => panic!("expected completion, got {other:?}"),
        }
    }
}
