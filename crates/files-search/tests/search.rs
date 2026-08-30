//! What search promises: an order that does not change, results that arrive in
//! pieces, and a hidden-file rule that is the search's own.

use files_core::{
    Entry, EntryKind, EntrySize, FileTime, HiddenReason, HiddenState, LocalPath, Location,
};
use files_search::rank::compare_hits;
use files_search::{
    CurrentDirectoryProvider, DefaultRanker, Filters, MatchKind, Ranker, RunDemand, RunState,
    SearchProvider, SearchQuery, SearchScope,
};

fn entry(name: &str) -> Entry {
    Entry::file(
        name,
        LocalPath::new(format!("/home/tim/{name}")).unwrap(),
        EntryKind::File,
    )
}

fn folder(name: &str) -> Entry {
    Entry::file(
        name,
        LocalPath::new(format!("/home/tim/{name}")).unwrap(),
        EntryKind::Directory,
    )
}

fn hidden(name: &str) -> Entry {
    let mut entry = entry(name);
    entry.hidden = HiddenState::Hidden(HiddenReason::Dotfile);
    entry
}

fn sized(name: &str, bytes: u64) -> Entry {
    let mut entry = entry(name);
    entry.size = EntrySize::Bytes(bytes);
    entry
}

fn at_time(name: &str, seconds: i64) -> Entry {
    let mut entry = entry(name);
    entry.modified = Some(FileTime::new(seconds, 0));
    entry
}

fn here() -> SearchScope {
    SearchScope::CurrentLocation(Location::local("/home/tim").unwrap())
}

fn run_over(query: SearchQuery, entries: &[Entry]) -> Vec<String> {
    let provider = CurrentDirectoryProvider::default();
    let mut run = provider.begin(query);
    run.offer(entries);
    run.mark_complete();
    run.hits().iter().map(|hit| hit.name.clone()).collect()
}

// --- Ranking ------------------------------------------------------------

#[test]
fn exact_beats_prefix_beats_substring_beats_fuzzy() {
    let names = [
        entry("report"),
        entry("report-draft"),
        entry("annual-report"),
        entry("recipe-for-pasta-on-tuesday"),
        entry("nothing-here"),
    ];
    assert_eq!(
        run_over(SearchQuery::new("report", here()), &names),
        [
            "report",
            "report-draft",
            "annual-report",
            "recipe-for-pasta-on-tuesday"
        ]
    );
}

#[test]
fn each_match_kind_is_identified_rather_than_collapsed_into_a_number() {
    let ranker = DefaultRanker;
    assert_eq!(
        ranker.score("notes", "notes").unwrap().kind,
        MatchKind::Exact
    );
    assert_eq!(
        ranker.score("notes.txt", "notes").unwrap().kind,
        MatchKind::Prefix
    );
    assert_eq!(
        ranker.score("my-notes.txt", "notes").unwrap().kind,
        MatchKind::Substring
    );
    assert_eq!(
        ranker.score("nice old tests", "notes").unwrap().kind,
        MatchKind::Fuzzy
    );
    assert_eq!(ranker.score("unrelated", "notes"), None);
    assert_eq!(ranker.id(), "default");
}

#[test]
fn matching_ignores_case_and_the_offset_counts_characters_not_bytes() {
    let ranker = DefaultRanker;
    assert_eq!(
        ranker.score("REPORT.PDF", "report").unwrap().kind,
        MatchKind::Prefix
    );
    // Four characters before "report", eight bytes.
    let score = ranker.score("réés-report", "report").unwrap();
    assert_eq!(score.offset, 5, "offsets are character indices");
}

#[test]
fn an_equal_score_still_has_exactly_one_order() {
    let names = [entry("b-file"), entry("a-file"), entry("c-file")];
    let first = run_over(SearchQuery::new("file", here()), &names);
    let second = run_over(SearchQuery::new("file", here()), &names);
    assert_eq!(first, ["a-file", "b-file", "c-file"]);
    assert_eq!(first, second, "the same query twice gives the same order");
}

#[test]
fn a_shorter_name_wins_a_tie_because_more_of_it_matched() {
    let names = [entry("reporting-service"), entry("reports")];
    assert_eq!(
        run_over(SearchQuery::new("report", here()), &names),
        ["reports", "reporting-service"]
    );
}

#[test]
fn comparing_two_hits_is_the_same_rule_the_run_uses() {
    let ranker = DefaultRanker;
    let exact = ranker.score("notes", "notes").unwrap();
    let prefix = ranker.score("notes.txt", "notes").unwrap();
    assert_eq!(
        compare_hits(&exact, "notes", &prefix, "notes.txt"),
        std::cmp::Ordering::Less
    );
}

// --- Filters ------------------------------------------------------------

#[test]
fn an_extension_filter_narrows_without_any_text() {
    let names = [entry("a.rs"), entry("b.txt"), entry("c.RS")];
    let query = SearchQuery::new("", here()).with_filters(Filters {
        extension: Some("rs".to_string()),
        ..Filters::default()
    });
    let mut hits = run_over(query, &names);
    hits.sort();
    assert_eq!(hits, ["a.rs", "c.RS"]);
}

#[test]
fn a_kind_filter_separates_folders_from_files() {
    let names = [folder("src"), entry("src.txt")];
    let query = SearchQuery::new("src", here()).with_filters(Filters {
        kinds: vec![EntryKind::Directory],
        ..Filters::default()
    });
    assert_eq!(run_over(query, &names), ["src"]);
}

#[test]
fn a_size_filter_refuses_an_entry_whose_size_is_unknown() {
    let names = [sized("known.bin", 500), entry("unknown.bin")];
    let query = SearchQuery::new("", here()).with_filters(Filters {
        min_bytes: Some(100),
        max_bytes: Some(1000),
        ..Filters::default()
    });
    assert_eq!(
        run_over(query, &names),
        ["known.bin"],
        "an unreadable size is not evidence that it is in range"
    );
}

#[test]
fn a_date_filter_bounds_both_ends_and_refuses_an_unknown_time() {
    let names = [
        at_time("old.txt", 100),
        at_time("middle.txt", 500),
        at_time("new.txt", 900),
        entry("undated.txt"),
    ];
    let query = SearchQuery::new("", here()).with_filters(Filters {
        modified_after: Some(FileTime::new(200, 0)),
        modified_before: Some(FileTime::new(800, 0)),
        ..Filters::default()
    });
    assert_eq!(run_over(query, &names), ["middle.txt"]);
}

#[test]
fn an_empty_query_with_no_filters_is_recognized_as_empty() {
    assert!(SearchQuery::new("   ", here()).is_empty());
    assert!(
        !SearchQuery::new("", here())
            .with_filters(Filters {
                extension: Some("rs".to_string()),
                ..Filters::default()
            })
            .is_empty()
    );
}

// --- Hidden files -------------------------------------------------------

#[test]
fn hidden_entries_follow_the_search_setting_not_the_view() {
    let names = [entry("config.toml"), hidden(".config")];
    assert_eq!(
        run_over(SearchQuery::new("config", here()), &names),
        ["config.toml"]
    );
    let mut with_hidden = run_over(
        SearchQuery::new("config", here()).including_hidden(true),
        &names,
    );
    with_hidden.sort();
    assert_eq!(with_hidden, [".config", "config.toml"]);
}

// --- Streaming ----------------------------------------------------------

#[test]
fn results_arrive_in_slices_and_stay_ordered_the_whole_way() {
    let names: Vec<Entry> = (0..500)
        .map(|index| entry(&format!("report-{index:03}.txt")))
        .collect();
    let provider = CurrentDirectoryProvider::default();
    let mut run = provider.begin(SearchQuery::new("report", here()));
    assert_eq!(run.demand(), RunDemand::Candidates);

    let mut seen_after_each_slice = Vec::new();
    for slice in names.chunks(64) {
        run.offer(slice);
        assert_eq!(run.state(), RunState::Streaming);
        // Every intermediate result is already in final order.
        let hit_names: Vec<&str> = run.hits().iter().map(|hit| hit.name.as_str()).collect();
        let mut sorted = hit_names.clone();
        sorted.sort();
        assert_eq!(hit_names, sorted);
        seen_after_each_slice.push(run.hits().len());
    }
    run.mark_complete();

    assert_eq!(run.state(), RunState::Finished);
    assert!(run.state().is_finished());
    assert_eq!(run.hits().len(), 500);
    assert_eq!(run.considered(), 500);
    assert_eq!(
        seen_after_each_slice.first(),
        Some(&64),
        "the first slice produced results before the rest arrived"
    );
    assert!(
        seen_after_each_slice.windows(2).all(|w| w[0] < w[1]),
        "each slice adds results rather than replacing them"
    );
}

#[test]
fn a_run_that_was_never_fed_reports_nothing_rather_than_failing() {
    let provider = CurrentDirectoryProvider::default();
    let mut run = provider.begin(SearchQuery::new("anything", here()));
    assert!(run.hits().is_empty());
    assert_eq!(run.considered(), 0);
    // A self-driven step on a candidate-fed run is a no-op, not an error.
    assert_eq!(run.advance(100), RunState::Finished);
}

// --- Scope --------------------------------------------------------------

#[test]
fn the_scope_is_a_value_the_ui_can_show_and_the_provider_can_refuse() {
    let provider = CurrentDirectoryProvider::default();
    assert!(provider.supports(&here()));
    assert!(!provider.supports(&SearchScope::Indexed));
    assert!(!provider.supports(&SearchScope::Recursive(
        Location::local("/home/tim").unwrap()
    )));

    assert_eq!(here().key(), "files.search.scope.current_location");
    assert_eq!(SearchScope::Indexed.key(), "files.search.scope.indexed");
    assert_eq!(
        here().location(),
        Some(&Location::local("/home/tim").unwrap())
    );
    assert_eq!(SearchScope::Indexed.location(), None);
    assert_eq!(provider.id(), "current-location");
    assert_eq!(provider.ranker_id(), "default");
}

#[test]
fn searching_the_applications_location_works_the_same_way() {
    // The Applications location is not a directory, and search must not care.
    let scope = SearchScope::CurrentLocation(Location::Applications);
    let provider = CurrentDirectoryProvider::default();
    assert!(provider.supports(&scope));
    let mut run = provider.begin(SearchQuery::new("text", scope));
    run.offer(&[entry("Text Editor"), entry("Calculator")]);
    assert_eq!(run.hits().len(), 1);
    assert_eq!(run.hits()[0].name, "Text Editor");
}
