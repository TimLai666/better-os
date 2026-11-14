//! Deterministic layout policy, so the localized layouts can be tested.
//!
//! A GPUI window cannot be laid out in CI, so the parts of the layout a
//! translation can break are decided by these functions and asserted directly:
//! whether a column header fits, whether the toolbar has to wrap, and whether
//! the sidebar is wide enough for the longest section label in either
//! language at 100, 125, and 150 percent.
//!
//! Character width is estimated rather than measured, the same way Better
//! Monitor estimates it. A CJK ideograph occupies roughly twice the advance of
//! a Latin letter at the same size, and the estimate is deliberately generous,
//! because the failure this guards against is a clipped label rather than a
//! slightly loose column.

pub const MIN_WINDOW_WIDTH: f32 = 880.0;
pub const MIN_WINDOW_HEIGHT: f32 = 560.0;
/// Below this the sidebar collapses to icons and the toolbar wraps.
pub const COMPACT_VIEWPORT_WIDTH: f32 = 1_040.0;
/// The sidebar's width when it is expanded.
pub const SIDEBAR_WIDTH: f32 = 236.0;
/// Horizontal padding a sidebar row spends on its glyph, gap, and insets.
pub const SIDEBAR_ROW_CHROME: f32 = 62.0;

/// A pane whose edge the person can drag (ticket 119's operations panel,
/// ticket 118's sidebar): the width it starts at, how narrow it may get, and
/// how much of the window it may take. One policy for every such edge, so
/// they clamp, remember and reset alike.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PaneEdge {
    /// The width it has until dragged, and after a double-click.
    pub default: f32,
    /// Narrower than this and its labels stop being readable.
    pub min: f32,
    /// The most of the window it may take, so the folder keeps a usable
    /// width.
    pub max_fraction: f32,
}

impl PaneEdge {
    /// The widest the pane may be in a window this wide; never below `min`.
    pub fn max(&self, window_width: f32) -> f32 {
        (window_width * self.max_fraction).max(self.min)
    }

    /// `width`, kept between the edge's limits for this window. Anything
    /// that is not a width (not a number, below zero) is the default.
    pub fn clamp(&self, width: f32, window_width: f32) -> f32 {
        let width = if width.is_finite() && width > 0.0 {
            width
        } else {
            self.default
        };
        width.clamp(self.min, self.max(window_width))
    }

    /// The width to draw from what was stored: the stored width when there
    /// is one, the default otherwise, either clamped for this window.
    pub fn resolve(&self, stored: Option<u32>, window_width: f32) -> f32 {
        self.clamp(
            stored.map_or(self.default, |width| width as f32),
            window_width,
        )
    }

    /// What to store after a drag: the clamped width in whole pixels, or
    /// nothing when it is the default, so a reset is stored as a reset.
    pub fn store(&self, width: f32, window_width: f32) -> Option<u32> {
        let width = self.clamp(width, window_width).round();
        (width != self.default).then_some(width as u32)
    }
}

/// The file operations panel (ticket 119): about what a job's title, its
/// progress bar and a count need, and at most a third of the window.
pub const OPERATIONS_EDGE: PaneEdge = PaneEdge {
    default: 320.0,
    min: 240.0,
    max_fraction: 1.0 / 3.0,
};

/// How wide the strip a person grabs to drag a pane's edge is: wider than the
/// 1px divider drawn in it, as in Windows 11 File Explorer.
pub const PANE_EDGE_HIT_WIDTH: f32 = 8.0;

/// Advance width of one Latin character at the body text size.
const LATIN_ADVANCE: f32 = 7.6;
/// Horizontal padding a table cell adds around its label.
const CELL_PADDING: f32 = 22.0;

/// The estimated rendered width of a label, counting wide scripts as wide.
pub fn label_width(label: &str) -> f32 {
    label
        .chars()
        .map(|character| {
            let wide = matches!(
                character as u32,
                0x1100..=0x115F
                    | 0x2E80..=0xA4CF
                    | 0xAC00..=0xD7A3
                    | 0xF900..=0xFAFF
                    | 0xFE30..=0xFE6F
                    | 0xFF00..=0xFF60
                    | 0xFFE0..=0xFFE6
            );
            if wide { 2.0 } else { 1.0 }
        })
        .sum::<f32>()
        * LATIN_ADVANCE
}

/// Whether a column header fits without being clipped.
pub fn header_fits(label: &str, column_width: f32) -> bool {
    label_width(label) + CELL_PADDING <= column_width
}

/// Whether a sidebar row's label fits the sidebar at this scale.
///
/// The sidebar is a fixed width, so a label that does not fit is truncated
/// rather than pushing the content area over. This says which one happens, so
/// a translation that would truncate is caught by a test rather than by a
/// screenshot.
pub fn sidebar_label_fits(label: &str, scale: f32) -> bool {
    label_width(label) * scale + SIDEBAR_ROW_CHROME * scale <= SIDEBAR_WIDTH
}

/// What the row of controls does at a given viewport and scale.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ControlLayout {
    Inline,
    Wrapped,
}

/// Whether the toolbar's labelled controls fit on one line.
///
/// Wrapping is a supported outcome; clipping a control is not, which is why
/// this is a decision a test can assert rather than an accident of the flex
/// layout.
pub fn toolbar_layout(viewport_width: f32, scale: f32, labels: &[&str]) -> ControlLayout {
    let logical = viewport_width / scale;
    // The sidebar, the path field, and the window padding are not available to
    // the labelled controls.
    let available = logical - SIDEBAR_WIDTH - 320.0 - 48.0;
    let needed: f32 = labels
        .iter()
        .map(|label| label_width(label) + 30.0)
        .sum::<f32>();
    if logical < COMPACT_VIEWPORT_WIDTH || needed > available {
        ControlLayout::Wrapped
    } else {
        ControlLayout::Inline
    }
}

/// What the detailed list does when its columns are wider than the viewport.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TableLayout {
    Fits,
    HorizontalScroll,
}

pub fn table_layout(viewport_width: f32, scale: f32, total_columns: f32) -> TableLayout {
    let logical = viewport_width / scale;
    let available = logical - SIDEBAR_WIDTH - 48.0;
    if total_columns <= available {
        TableLayout::Fits
    } else {
        TableLayout::HorizontalScroll
    }
}

/// The width each pane of a split window lays itself out for, given the
/// window's width and the navigation pane's.
///
/// Every width a pane computes (the tiles across, the list columns that fit,
/// the breadcrumbs, the command bar) is "its viewport minus the navigation
/// pane", so each half is handed a viewport whose width, less the navigation
/// pane, is the half it really has: the window's content area split in two,
/// less the divider between them.
pub fn split_viewport_width(window_width: f32, nav_width: f32) -> f32 {
    let half = ((window_width - nav_width - 1.0) / 2.0).max(0.0);
    nav_width + half
}

/// The width of the panels beside the content area: the app details (360)
/// or the preview (320, hidden in a compact window), and the operation
/// center (420). Every one that is drawn takes its width from the content.
pub fn side_panels_width(details: bool, preview: bool, compact: bool, operations: bool) -> f32 {
    let column = if details {
        360.0
    } else if preview && !compact {
        320.0
    } else {
        0.0
    };
    column + if operations { 420.0 } else { 0.0 }
}

/// How many rows a viewport of this height shows, which is the page size for
/// Page Up and Page Down and the count the virtualized list draws.
pub fn visible_rows(viewport_height: f32, row_height: f32) -> usize {
    if row_height <= 0.0 {
        return 1;
    }
    ((viewport_height / row_height).ceil() as usize).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pane_edge_clamps_between_readable_and_a_share_of_the_window() {
        let edge = OPERATIONS_EDGE;
        assert_eq!(edge.clamp(100.0, 1_200.0), edge.min);
        assert_eq!(edge.clamp(900.0, 1_200.0), 400.0, "a third of the window");
        assert_eq!(edge.clamp(300.0, 1_200.0), 300.0);
        // At the window's minimum width a third is less than the default.
        assert!((edge.resolve(None, MIN_WINDOW_WIDTH) - MIN_WINDOW_WIDTH / 3.0).abs() < 0.01);
        // A tiny window still leaves the pane readable.
        assert_eq!(edge.clamp(300.0, 300.0), edge.min);
    }

    #[test]
    fn a_stored_width_comes_back_and_nonsense_is_the_default() {
        let edge = OPERATIONS_EDGE;
        assert_eq!(edge.resolve(Some(280), 1_200.0), 280.0);
        assert_eq!(edge.resolve(None, 1_200.0), edge.default);
        assert_eq!(edge.clamp(f32::NAN, 1_200.0), edge.default);
        assert_eq!(edge.clamp(-5.0, 1_200.0), edge.default);
        assert_eq!(edge.resolve(Some(0), 1_200.0), edge.default);
        assert_eq!(edge.resolve(Some(u32::MAX), 1_200.0), 400.0);
    }

    #[test]
    fn a_reset_is_stored_as_no_width_at_all() {
        let edge = OPERATIONS_EDGE;
        assert_eq!(edge.store(edge.default, 1_200.0), None);
        assert_eq!(edge.store(280.4, 1_200.0), Some(280));
        assert_eq!(edge.store(5_000.0, 1_200.0), Some(400));
    }
}
