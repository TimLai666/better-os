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

/// How many rows a viewport of this height shows, which is the page size for
/// Page Up and Page Down and the count the virtualized list draws.
pub fn visible_rows(viewport_height: f32, row_height: f32) -> usize {
    if row_height <= 0.0 {
        return 1;
    }
    ((viewport_height / row_height).ceil() as usize).max(1)
}
