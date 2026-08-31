//! Better Launcher's overlay.
//!
//! One window with a search row near the top and the application library
//! below it. Typing filters that library in place; emptying the search row
//! brings it back. There is no second window and no mode switch, because
//! `launcher-core` makes the query itself the only state that decides what is
//! on screen, and [`model::OverlayModel`] adds only a selection and a load
//! state on top of it.
//!
//! Where each thing lives:
//!
//! - [`model`] is everything that can be decided without a window: rows,
//!   selection, keyboard movement, launch outcomes. It has no GPUI dependency,
//!   so all of it is tested with no display backend.
//! - [`overlay`] draws that model with `better-ui` primitives and routes keys
//!   and clicks back into it. It decides nothing.
//! - [`i18n`] holds the wording.
//!
//! ## Deferred, on purpose
//!
//! Issue #2 defers the overlay's dimensions, its animation, its category
//! grouping, and the exact global shortcut. This build takes the smallest
//! honest position on each: a near-full-screen window sized from the display
//! ([`overlay_size`]), no animation, one deterministic flat grid with no
//! category sections, and a shortcut that is described but not installed. None
//! of them is hard-coded anywhere a later decision would have to hunt for.

pub mod i18n;
pub mod model;
pub mod overlay;
pub mod startup;
#[cfg(test)]
mod tests;

pub use i18n::Locale;
pub use model::{Activation, LoadState, Move, Notice, OverlayModel};
pub use overlay::{LauncherOverlay, OverlayEvent};

/// The launcher-level benchmarks, as `(name, workload, metric)`.
///
/// This is the single definition of what the harness measures. The benchmark
/// suite labels its rows from here and `tests/manifest.rs` asserts that
/// `components/manifests/better-launcher.yaml` declares exactly these, so a
/// manifest that promises a measurement nobody takes fails a test rather than
/// sitting there looking plausible.
pub const BENCHMARKS: [(&str, &str, &str); 5] = [
    (
        "warm-search-update",
        "synthetic-5000-entry-xdg-directory-keystroke-script",
        "milliseconds-p95-keystroke-to-updated-result-model",
    ),
    (
        "warm-overlay-open",
        "headless-process-start-with-warm-5000-entry-index",
        "milliseconds-process-start-to-first-renderable-model",
    ),
    (
        "application-list-update",
        "write-then-remove-one-desktop-entry-under-watch",
        "milliseconds-p95-filesystem-event-to-refreshed-model",
    ),
    (
        "idle-overhead",
        "headless-overlay-idle-for-a-measured-window",
        "cpu-percent-over-the-idle-window",
    ),
    (
        "idle-memory",
        "headless-overlay-idle-for-a-measured-window",
        "resident-kilobytes-at-the-end-of-the-idle-window",
    ),
];

/// The smallest the overlay is allowed to get. Below this the search row and
/// one row of tiles stop fitting together, which is the point at which the
/// screen stops being the thing Issue #2 describes.
pub const MIN_WINDOW_WIDTH: f32 = 640.0;
pub const MIN_WINDOW_HEIGHT: f32 = 480.0;

/// The fraction of the display the overlay covers.
///
/// Full-screen versus bounded is a deferred decision, so this is deliberately
/// one number in one place rather than a layer-shell integration or a
/// hard-coded pixel size. Near-full-screen keeps the desktop visible at the
/// edges, which is the reversible choice: going to true full screen later
/// changes this constant, going back from a compositor integration would not
/// be as cheap.
pub const OVERLAY_COVERAGE: f32 = 0.92;

/// One tile's width, including the gap that follows it. The grid wraps at
/// whatever count fits, so this is the only measurement the layout needs.
pub const TILE_WIDTH: f32 = 168.0;

/// The overlay's size on a display of the given logical size.
pub fn overlay_size(display_width: f32, display_height: f32) -> (f32, f32) {
    (
        (display_width * OVERLAY_COVERAGE).max(MIN_WINDOW_WIDTH),
        (display_height * OVERLAY_COVERAGE).max(MIN_WINDOW_HEIGHT),
    )
}

/// How many tiles fit across the application area at this width and interface
/// scale.
///
/// The keyboard needs this number as much as the layout does: Down means the
/// next row, and a row is only meaningful if the model and the grid agree on
/// how wide one is.
pub fn grid_columns(viewport_width: f32, scale: f32) -> usize {
    // The window's own horizontal padding, on both sides.
    const PADDING: f32 = 48.0;
    let logical_width = (viewport_width / scale.max(0.1)) - PADDING;
    ((logical_width / TILE_WIDTH).floor() as isize).clamp(1, 12) as usize
}

/// Whether the footer hints fit on one line or have to wrap.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HintLayout {
    Inline,
    Wrapped,
}

/// A rough advance width per character. A CJK glyph is about twice as wide as
/// a Latin one, which is the difference that decides the breakpoint between
/// `zh-TW` and `en-US`.
fn label_width(label: &str) -> f32 {
    label
        .chars()
        .map(|character| if character.is_ascii() { 7.0 } else { 14.0 })
        .sum()
}

/// The layout of the keyboard-hint row at a given viewport width and scale.
///
/// The same policy the manager GUI and the chooser follow: a localized label
/// that no longer fits reports that it wrapped rather than silently pushing
/// something out of view.
pub fn hint_layout(viewport_width: f32, scale: f32, labels: &[&str]) -> HintLayout {
    let logical_width = viewport_width / scale.max(0.1);
    let needed: f32 = labels
        .iter()
        .map(|label| label_width(label) + 24.0)
        .sum::<f32>()
        + 48.0;
    if logical_width >= needed {
        HintLayout::Inline
    } else {
        HintLayout::Wrapped
    }
}
