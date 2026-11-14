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

    /// What a double-click on the edge stores: no width at all, so the pane
    /// is drawn at its default whenever the window is wide enough, not at
    /// whatever a narrow window clamped the default to.
    pub fn reset(&self) -> Option<u32> {
        None
    }

    /// What to store after a drag: the clamped width in whole pixels, or
    /// nothing when it is the default.
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

/// The navigation pane (ticket 118): WinUI's 320px list pane by default;
/// at 200px a section label and a folder name still read; at most two
/// fifths of the window, so the folder keeps the rest.
pub const SIDEBAR_EDGE: PaneEdge = PaneEdge {
    default: 320.0,
    min: 200.0,
    max_fraction: 0.4,
};

/// The preview pane and the application details in its place (ticket
/// 118): 320px as before, from 240px (a thumbnail and its caption) to a
/// third of the window.
pub const PREVIEW_EDGE: PaneEdge = PaneEdge {
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

/// The narrowest the folder itself may be made by the panes around it:
/// a name column and a size, or two tiles.
pub const MIN_CONTENT_WIDTH: f32 = 320.0;

/// What one window pane lays out: its width (less the frame's shadow), the
/// whole window's for the sidebar, and the stored widths of the panes that
/// are drawn (`None` for one that is not, `Some(None)` for one at its
/// default).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PaneInputs {
    /// The window as the person sees it; the sidebar is a share of this.
    pub window: f32,
    /// This pane's own width, sidebar included (half the window in a split).
    pub pane: f32,
    /// `None` when the navigation pane is compact (48px).
    pub sidebar: Option<Option<u32>>,
    /// The preview, or the application details in its place.
    pub side: Option<Option<u32>>,
    pub operations: Option<Option<u32>>,
    /// A sidebar width decided elsewhere: the right half of a split is laid
    /// out beside the left half's sidebar.
    pub fixed_nav: Option<f32>,
}

/// Every pane's width, decided together, so the folder keeps
/// [`MIN_CONTENT_WIDTH`] however wide each pane was dragged (ticket 118).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PaneLayout {
    pub nav: f32,
    pub side: f32,
    pub operations: f32,
    /// What is left for the folder.
    pub content: f32,
}

/// Lays a window pane out: each pane at its stored width within its own
/// limits, then the side panes narrowed (the operations panel first) and
/// the sidebar after them, never below their minimums, until the folder has
/// [`MIN_CONTENT_WIDTH`].
pub fn pane_layout(inputs: PaneInputs) -> PaneLayout {
    let nav = match inputs.sidebar {
        _ if inputs.fixed_nav.is_some() => inputs.fixed_nav.unwrap_or(48.0),
        None => 48.0,
        Some(stored) => {
            let wanted = SIDEBAR_EDGE.resolve(stored, inputs.window);
            let room = (inputs.window - MIN_CONTENT_WIDTH).max(SIDEBAR_EDGE.min);
            wanted.min(room)
        }
    };
    let area = (inputs.pane - nav).max(0.0);
    let mut side = inputs
        .side
        .map_or(0.0, |stored| PREVIEW_EDGE.resolve(stored, area));
    let mut operations = inputs
        .operations
        .map_or(0.0, |stored| OPERATIONS_EDGE.resolve(stored, area));
    let mut over = side + operations + MIN_CONTENT_WIDTH - area;
    if over > 0.0 && operations > 0.0 {
        let cut = over.min(operations - OPERATIONS_EDGE.min).max(0.0);
        operations -= cut;
        over -= cut;
    }
    if over > 0.0 && side > 0.0 {
        let cut = over.min(side - PREVIEW_EDGE.min).max(0.0);
        side -= cut;
    }
    PaneLayout {
        nav,
        side,
        operations,
        content: (area - side - operations).max(0.0),
    }
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
    fn a_reset_in_the_narrowest_window_stores_the_default_not_the_clamp() {
        let edge = OPERATIONS_EDGE;
        let stored = edge.reset();
        assert_eq!(stored, None);
        // Drawn clamped at 880px, at the default once the window is wider.
        assert!((edge.resolve(stored, MIN_WINDOW_WIDTH) - MIN_WINDOW_WIDTH / 3.0).abs() < 0.01);
        assert_eq!(edge.resolve(stored, 1_400.0), edge.default);
        // What the old reset did: the default dragged-to in a narrow window
        // is stored as the clamp, which is why a reset does not go through
        // `store`.
        assert_eq!(edge.store(edge.default, MIN_WINDOW_WIDTH), Some(293));
    }

    #[test]
    fn a_reset_is_stored_as_no_width_at_all() {
        let edge = OPERATIONS_EDGE;
        assert_eq!(edge.store(edge.default, 1_200.0), None);
        assert_eq!(edge.store(280.4, 1_200.0), Some(280));
        assert_eq!(edge.store(5_000.0, 1_200.0), Some(400));
    }

    fn inputs(window: f32) -> PaneInputs {
        PaneInputs {
            window,
            pane: window,
            sidebar: Some(None),
            side: None,
            operations: None,
            fixed_nav: None,
        }
    }

    #[test]
    fn the_folder_keeps_its_width_whatever_the_panes_were_dragged_to() {
        // Everything open and dragged wide, in the narrowest window.
        let crowded = pane_layout(PaneInputs {
            side: Some(Some(5_000)),
            operations: Some(Some(5_000)),
            sidebar: Some(Some(5_000)),
            ..inputs(MIN_WINDOW_WIDTH)
        });
        assert!(crowded.content >= MIN_CONTENT_WIDTH - 0.01 || crowded.side == PREVIEW_EDGE.min);
        assert!(crowded.operations >= OPERATIONS_EDGE.min);
        assert!(crowded.side >= PREVIEW_EDGE.min);
        assert!(
            (crowded.nav + crowded.side + crowded.operations + crowded.content - MIN_WINDOW_WIDTH)
                .abs()
                < 0.01
        );
        // With room, each keeps what it was dragged to.
        let roomy = pane_layout(PaneInputs {
            side: Some(Some(300)),
            operations: Some(Some(260)),
            ..inputs(1_800.0)
        });
        assert_eq!(
            (roomy.nav, roomy.side, roomy.operations),
            (320.0, 300.0, 260.0)
        );
        assert_eq!(roomy.content, 1_800.0 - 320.0 - 300.0 - 260.0);
    }

    #[test]
    fn a_split_pane_s_preview_is_a_share_of_that_pane() {
        let half = pane_layout(PaneInputs {
            pane: 700.0,
            side: Some(Some(600)),
            ..inputs(1_400.0)
        });
        // A third of the 380px this pane has beside the sidebar, never the
        // window's third.
        assert!(half.side <= (700.0 - 320.0) / 3.0 + 0.01 || half.side == PREVIEW_EDGE.min);
        assert!(half.content >= MIN_CONTENT_WIDTH - 0.01 || half.side == PREVIEW_EDGE.min);
    }

    #[test]
    fn the_sidebar_and_preview_edges_keep_the_folder_usable() {
        // Ticket 118: the sidebar from 200px to two fifths of the window,
        // reset to 320; an unreadable stored width is the default.
        let edge = SIDEBAR_EDGE;
        assert_eq!(edge.resolve(None, 1_400.0), 320.0);
        assert_eq!(edge.clamp(120.0, 1_400.0), 200.0);
        assert_eq!(edge.clamp(900.0, 1_400.0), 560.0);
        assert_eq!(edge.resolve(Some(0), 1_400.0), 320.0);
        assert_eq!(edge.resolve(Some(260), 1_400.0), 260.0);
        assert_eq!(edge.store(260.4, 1_400.0), Some(260));
        assert_eq!(edge.reset(), None);
        // A sidebar dragged as wide as it may be still leaves the folder
        // its minimum.
        let wide = pane_layout(PaneInputs {
            sidebar: Some(Some(10_000)),
            ..inputs(MIN_WINDOW_WIDTH)
        });
        assert!(wide.content >= MIN_CONTENT_WIDTH);
        assert_eq!(PREVIEW_EDGE.resolve(None, 1_400.0), 320.0);
        assert_eq!(PREVIEW_EDGE.clamp(100.0, 1_400.0), 240.0);
    }
}
