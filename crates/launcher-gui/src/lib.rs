//! Better Launcher: the Windows 11 Start menu, on a Linux desktop.
//!
//! One window holds the search box and, under it, the Start page (pinned
//! apps and Recommended), All apps from A to Z with a letter jump, or search
//! results with the selected result's actions beside them. Typing replaces
//! the page with results and clearing the search brings it back; there is no
//! second window and no mode switch, because the query alone decides between
//! browsing and searching (Issue #2).
//!
//! Where each thing lives:
//!
//! - [`model`] is everything that can be decided without a window: the
//!   pages, which item the keyboard is on and where each key takes it, what
//!   each item's menu offers, pinning and reordering, launch outcomes. It has
//!   no GPUI dependency, so all of it is tested with no display backend.
//! - [`ranking`] puts a calculation and files or folders in front of the
//!   applications `launcher-core` ranked; [`calculator`] evaluates the sums.
//! - [`pins`] reads and writes the pinned apps and the launch history.
//! - [`overlay`] draws the model with `better_ui::fluent` and routes keys and
//!   clicks back into it. It decides nothing.
//! - [`i18n`] holds the wording.

pub mod calculator;
pub mod command_line;
pub mod i18n;
pub mod model;
pub mod overlay;
pub mod pins;
pub mod ranking;
pub mod startup;
#[cfg(test)]
mod tests;

pub use i18n::Locale;
pub use model::{Activation, Effect, LoadState, NavKey, Notice, OverlayModel, nav_key};
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

/// The Start menu's size, as Windows 11 draws it at 100%: 640 by 720. A
/// display too small for that gets what fits, down to the minimum.
pub const START_WIDTH: f32 = 640.0;
pub const START_HEIGHT: f32 = 720.0;

/// The smallest the window is allowed to get: the six-column pinned grid
/// and the search box still fit.
pub const MIN_WINDOW_WIDTH: f32 = 640.0;
pub const MIN_WINDOW_HEIGHT: f32 = 480.0;

/// What the Start menu leaves around itself on a small display.
pub const DISPLAY_MARGIN: f32 = 48.0;

/// The window's size on a display of the given logical size: the Start
/// menu's own size, shrunk to fit a small display, never below the minimum.
pub fn overlay_size(display_width: f32, display_height: f32) -> (f32, f32) {
    (
        START_WIDTH
            .min(display_width - DISPLAY_MARGIN)
            .max(MIN_WINDOW_WIDTH),
        START_HEIGHT
            .min(display_height - DISPLAY_MARGIN)
            .max(MIN_WINDOW_HEIGHT),
    )
}
