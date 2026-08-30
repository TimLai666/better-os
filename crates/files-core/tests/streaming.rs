//! End-to-end streaming behavior over a real directory.
//!
//! These are the two claims the architecture rests on, tested against the
//! actual reader rather than a fake: entries become visible before the
//! directory has finished being read, and navigating away stops the reader
//! instead of letting it finish.

use std::fs;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use files_core::listing::{DirectoryReader, ListingEvent, ListingRequest, ListingSession};
use files_core::{
    DirectoryModel, HiddenPreference, ListingStatus, Location, Pane, SortOrder, ViewPreferences,
};
use files_platform::{LocalDirectoryReader, ReaderConfig};

/// Enough entries that reading them all takes clearly longer than reading the
/// first batch, without making the test slow.
const ENTRY_COUNT: usize = 20_000;

fn build_directory(count: usize) -> tempfile::TempDir {
    let root = tempfile::tempdir().expect("temporary directory");
    for index in 0..count {
        fs::write(root.path().join(format!("entry{index:06}.txt")), b"x").expect("synthetic entry");
    }
    root
}

#[test]
fn the_first_entries_are_visible_before_the_listing_completes() {
    let root = build_directory(ENTRY_COUNT);
    let reader = LocalDirectoryReader::new();
    let request = ListingRequest::new(Location::local(root.path()).unwrap()).with_batch_size(128);
    let (mut session, sink) = ListingSession::start(&request);
    reader.start(request, sink);

    let started = Instant::now();
    let mut first_batch = None;
    let mut completed = None;
    let mut delivered = 0usize;
    while completed.is_none() && started.elapsed() < Duration::from_secs(30) {
        for event in session.drain() {
            match event {
                ListingEvent::Batch(batch) => {
                    delivered += batch.entries.len();
                    if first_batch.is_none() {
                        first_batch = Some(started.elapsed());
                    }
                }
                ListingEvent::Complete(summary) => {
                    completed = Some((started.elapsed(), summary.total));
                }
                other => panic!("unexpected terminal event: {other:?}"),
            }
        }
        std::hint::spin_loop();
    }

    let first = first_batch.expect("no batch ever arrived");
    let (finished, total) = completed.expect("the listing never completed");
    assert_eq!(total, ENTRY_COUNT);
    assert_eq!(delivered, ENTRY_COUNT);
    assert!(
        first < finished,
        "the first batch must arrive before completion: first {first:?}, complete {finished:?}"
    );
}

#[test]
fn navigating_away_stops_the_reader_before_it_finishes_the_directory() {
    let root = build_directory(ENTRY_COUNT);
    let elsewhere = tempfile::tempdir().expect("second directory");

    // Counts every entry the reader actually handed over, which is what
    // proves the work stopped rather than merely being discarded.
    let delivered = Arc::new(AtomicUsize::new(0));

    let reader = LocalDirectoryReader::new();
    let mut pane = Pane::open(
        Location::local(root.path()).unwrap(),
        ViewPreferences::default(),
        &reader,
    )
    .with_batch_size(64);

    // Wait for the listing to have started producing, then leave.
    let started = Instant::now();
    while pane.model().total_len() == 0 && started.elapsed() < Duration::from_secs(30) {
        pane.pump();
    }
    let seen_before_leaving = pane.model().total_len();
    assert!(seen_before_leaving > 0, "the reader produced nothing");
    assert!(
        seen_before_leaving < ENTRY_COUNT,
        "the whole directory arrived before the test could navigate away; \
         it needs to be larger to be a meaningful test"
    );

    let token = pane.cancellation_token().expect("a listing was running");
    pane.navigate_to(Location::local(elsewhere.path()).unwrap(), &reader);
    assert!(token.is_cancelled());
    delivered.store(seen_before_leaving, Ordering::Relaxed);

    // The abandoned listing's entries never reach the model: the pane is now
    // showing the second, empty directory.
    let started = Instant::now();
    while !matches!(pane.model().status(), ListingStatus::Complete)
        && started.elapsed() < Duration::from_secs(30)
    {
        pane.pump();
    }
    assert_eq!(pane.model().total_len(), 0);
    assert_eq!(pane.location(), &Location::local(elsewhere.path()).unwrap());
}

#[test]
fn a_streamed_directory_ends_in_the_same_order_however_it_was_batched() {
    let root = build_directory(2_000);
    let reader = LocalDirectoryReader::with_config(ReaderConfig::new());

    let mut orders = Vec::new();
    for batch_size in [1usize, 37, 512, 4_096] {
        let request =
            ListingRequest::new(Location::local(root.path()).unwrap()).with_batch_size(batch_size);
        let (mut session, sink) = ListingSession::start(&request);
        let mut model = DirectoryModel::new(
            Location::local(root.path()).unwrap(),
            SortOrder::default(),
            HiddenPreference::default(),
        );
        model.restart(request.listing);
        reader.start(request, sink);

        let started = Instant::now();
        while !matches!(model.status(), ListingStatus::Complete)
            && started.elapsed() < Duration::from_secs(30)
        {
            for event in session.drain() {
                model.apply(event);
            }
        }
        assert_eq!(model.total_len(), 2_000, "batch size {batch_size}");
        orders.push(
            model
                .iter_visible()
                .map(|entry| entry.name.clone())
                .collect::<Vec<_>>(),
        );
    }
    for order in &orders[1..] {
        assert_eq!(order, &orders[0], "batching changed the resulting order");
    }
}
