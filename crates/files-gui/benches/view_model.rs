//! The 100,000-entry directory, through the view-model layer the window draws.
//!
//! `files-core`'s benchmark measures the listing and the model. This one
//! measures what the GUI adds on top: turning entries into the rows a frame
//! renders, keeping the keyboard cursor correct while entries stream in, and
//! answering a keystroke on a directory that large.
//!
//! The number that decides whether the architecture works is the contrast
//! between **one screenful** and **every row**. Both view modes format rows
//! inside the virtualized range callback, so a frame pays for a screenful; the
//! whole-directory figure is what a naive view that built every row would pay,
//! and it is reported beside it so the difference is a measurement rather than
//! a claim.
//!
//! There is no benchmark harness dependency, matching `files-core` and
//! `app-catalog-core`: wall-clock timings, exact about what they measure.
//!
//! Run with `cargo bench -p files-gui`, or `cargo bench -p files-gui -- --test`
//! for a single-iteration smoke run on a smaller tree.

use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

use files_core::{Entry, Location, Pane, ViewPreferences};
use files_gui::content::{ContentView, SelectionInput, rendered_row};
use files_gui::i18n::{EN_US, ZH_TW};
use files_gui::prefs::{ItemScale, ViewMode};
use files_gui::reader::FilesReader;
use files_platform::ReaderConfig;

/// The directory size the ticket sets as the target.
const ENTRY_COUNT: usize = 100_000;
/// The batch size a view would use.
const BATCH_SIZE: usize = 256;
/// A screenful of rows in the detailed list at the default row height.
const LIST_SCREENFUL: usize = 32;
/// A screenful of tiles in the grid at the default tile size.
const GRID_SCREENFUL: usize = 60;

fn report(label: &str, duration: Duration, detail: &str) {
    println!(
        "{label:<44} {:>10.3} ms   {detail}",
        duration.as_secs_f64() * 1000.0
    );
}

fn median(mut samples: Vec<Duration>) -> Duration {
    samples.sort();
    samples[samples.len() / 2]
}

fn build_flat(root: &Path, count: usize) {
    fs::create_dir_all(root).expect("benchmark directory");
    for index in 0..count {
        // A tenth of the entries are folders, so folders-first and the type
        // sort have something to do.
        if index % 10 == 0 {
            fs::create_dir_all(root.join(format!("folder{index:06}"))).expect("folder");
        } else {
            fs::write(root.join(format!("entry{index:06}.txt")), b"x").expect("entry");
        }
    }
}

/// Formats one range of rows, exactly the way the virtualized list does.
fn format_range(pane: &Pane, view: &ContentView, range: std::ops::Range<usize>) -> usize {
    let model = pane.model();
    let cursor = view.cursor();
    let mut drawn = 0usize;
    for index in range {
        let Some(entry) = model.visible(index) else {
            break;
        };
        let row = rendered_row(
            entry,
            model.selection().contains(&entry.id()),
            cursor == Some(index),
            &EN_US,
        );
        std::hint::black_box(&row);
        drawn += 1;
    }
    drawn
}

fn main() {
    let smoke = std::env::args().any(|argument| argument == "--test");
    let iterations = if smoke { 1 } else { 5 };
    let entry_count = if smoke { 2_000 } else { ENTRY_COUNT };

    let root = tempfile::tempdir().expect("benchmark root");
    let flat = root.path().join("flat");

    let started = Instant::now();
    build_flat(&flat, entry_count);
    println!(
        "files-gui view-model benchmarks: {entry_count} entries, batch size {BATCH_SIZE}, \
         median of {iterations} iteration(s)"
    );
    println!(
        "fixture written in {:.1} s\n",
        started.elapsed().as_secs_f64()
    );

    let location = Location::local(&flat).expect("location");
    let reader = FilesReader::new(ReaderConfig::new(), None);
    let preferences = ViewPreferences::default();

    // --- Progressive arrival. --------------------------------------------
    let mut first_visible = Vec::new();
    let mut fully_loaded = Vec::new();
    for _ in 0..iterations {
        let clock = Instant::now();
        let mut pane =
            Pane::open(location.clone(), preferences, &reader).with_batch_size(BATCH_SIZE);
        let view = ContentView::new(ViewMode::List, ItemScale::Medium);

        // The first frame that has something to draw: entries have arrived and
        // a screenful of rows has been formatted.
        let mut first = None;
        while first.is_none() {
            pane.pump();
            if pane.model().visible_len() > 0 {
                format_range(&pane, &view, 0..LIST_SCREENFUL);
                first = Some(clock.elapsed());
            }
            std::hint::spin_loop();
        }
        while pane.is_listing() {
            pane.pump();
            std::hint::spin_loop();
        }
        pane.pump();
        assert_eq!(pane.model().total_len(), entry_count);
        first_visible.push(first.expect("a first frame"));
        fully_loaded.push(clock.elapsed());
    }
    report(
        "first visible batch, rows formatted",
        median(first_visible),
        &format!("{LIST_SCREENFUL} rows drawable"),
    );
    report(
        "full model, listing complete",
        median(fully_loaded),
        &format!("{entry_count} entries"),
    );

    // One loaded pane for the rest of the measurements.
    let mut pane = Pane::open(location.clone(), preferences, &reader).with_batch_size(BATCH_SIZE);
    while pane.is_listing() {
        pane.pump();
        std::hint::spin_loop();
    }
    pane.pump();
    let total = pane.model().visible_len();
    println!();

    // --- What a frame costs, against what building everything would. ------
    for (label, mode, screenful) in [
        (
            "list: one screenful of rows",
            ViewMode::List,
            LIST_SCREENFUL,
        ),
        (
            "grid: one screenful of tiles",
            ViewMode::Grid,
            GRID_SCREENFUL,
        ),
    ] {
        let view = ContentView::new(mode, ItemScale::Medium);
        let mut samples = Vec::new();
        for index in 0..iterations {
            // A different window each time, so the measurement is not one
            // cached region of the model.
            let start = (index * (total / iterations.max(1))) % total.max(1);
            let clock = Instant::now();
            let drawn = format_range(&pane, &view, start..start + screenful);
            samples.push(clock.elapsed());
            assert!(drawn > 0);
        }
        report(label, median(samples), "what the viewport actually draws");
    }

    let view = ContentView::new(ViewMode::List, ItemScale::Medium);
    let mut samples = Vec::new();
    for _ in 0..iterations {
        let clock = Instant::now();
        let drawn = format_range(&pane, &view, 0..total);
        samples.push(clock.elapsed());
        assert_eq!(drawn, total);
    }
    let whole = median(samples);
    report(
        "every row formatted, for comparison",
        whole,
        "the cost a non-virtualized view would pay per frame",
    );
    println!();

    // --- Keystrokes on a directory this large. ----------------------------
    let mut view = ContentView::new(ViewMode::List, ItemScale::Medium);
    view.apply(pane.model_mut(), SelectionInput::Click(0), 1);

    for (label, input, columns) in [
        ("keystroke: arrow down", SelectionInput::Down, 1usize),
        ("keystroke: page down", SelectionInput::PageDown(32), 1),
        ("keystroke: end", SelectionInput::End, 1),
        ("keystroke: select all", SelectionInput::SelectAll, 1),
    ] {
        let mut samples = Vec::new();
        for _ in 0..iterations {
            let clock = Instant::now();
            view.apply(pane.model_mut(), input, columns);
            samples.push(clock.elapsed());
        }
        report(label, median(samples), &format!("{total} entries"));
    }

    // The cursor is re-derived from the selection's identity on every frame
    // that changed, so this is the per-frame cost of "insertion does not move
    // the selection".
    let mut samples = Vec::new();
    for _ in 0..iterations {
        let clock = Instant::now();
        view.resync(pane.model());
        samples.push(clock.elapsed());
    }
    report(
        "cursor resync after a batch arrives",
        median(samples),
        "cached hit; the cursor did not move",
    );

    // The uncached case: the cursor is somewhere deep in the list and the
    // index has to be found again.
    view.apply(pane.model_mut(), SelectionInput::End, 1);
    let mut samples = Vec::new();
    for _ in 0..iterations {
        let mut cold = ContentView::new(ViewMode::List, ItemScale::Medium);
        let clock = Instant::now();
        cold.resync(pane.model());
        samples.push(clock.elapsed());
        assert_eq!(cold.cursor(), Some(total - 1));
    }
    report(
        "cursor resync, index rediscovered",
        median(samples),
        "worst case: the focused entry is the last one",
    );
    println!();

    // --- Localization is not a cost. --------------------------------------
    for (label, copy) in [
        ("format rows, en-US", &EN_US),
        ("format rows, zh-TW", &ZH_TW),
    ] {
        let mut samples = Vec::new();
        for _ in 0..iterations {
            let clock = Instant::now();
            for index in 0..LIST_SCREENFUL {
                let Some(entry) = pane.model().visible(index) else {
                    break;
                };
                std::hint::black_box(rendered_row(entry, false, false, copy));
            }
            samples.push(clock.elapsed());
        }
        report(label, median(samples), "one screenful");
    }

    // Sorting the whole directory, which is what clicking a header costs.
    let mut samples = Vec::new();
    for index in 0..iterations {
        let order = if index % 2 == 0 {
            files_core::SortOrder::new(
                files_core::SortKey::Size,
                files_core::SortDirection::Descending,
            )
        } else {
            files_core::SortOrder::default()
        };
        let clock = Instant::now();
        pane.set_order(order);
        samples.push(clock.elapsed());
    }
    println!();
    report(
        "header click: re-sort the whole directory",
        median(samples),
        &format!("{total} entries"),
    );

    let mut samples = Vec::new();
    for _ in 0..iterations {
        let clock = Instant::now();
        pane.toggle_hidden();
        samples.push(clock.elapsed());
    }
    report(
        "Ctrl+H: re-filter without reloading",
        median(samples),
        "no listing restarted",
    );

    std::hint::black_box(Entry::file(
        "keep-the-entry-type-linked",
        files_core::LocalPath::root(),
        files_core::EntryKind::File,
    ));
}
