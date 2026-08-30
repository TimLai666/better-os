//! Deterministic layout policy, so the localized layouts can be tested.
//!
//! A GPUI window cannot be laid out in CI, so the parts of the layout that a
//! translation can break are decided by these functions and asserted directly:
//! whether a column is wide enough for its longest header in either language,
//! and whether the action row has to wrap at a given viewport and scale.
//!
//! Character width is estimated rather than measured. A CJK ideograph occupies
//! roughly twice the advance of a Latin letter at the same size, and the
//! estimate is deliberately generous, because the failure this guards against
//! is a clipped label rather than a slightly loose column.

#[cfg(test)]
use crate::tables::ProcessColumnLayout;
#[cfg(test)]
use monitor_views::ProcessColumn;

pub(crate) const MIN_WINDOW_WIDTH: f32 = 860.0;
pub(crate) const MIN_WINDOW_HEIGHT: f32 = 600.0;
pub(crate) const COMPACT_VIEWPORT_WIDTH: f32 = 1100.0;

/// Advance width of one Latin character at the table's text size.
#[cfg(test)]
const LATIN_ADVANCE: f32 = 7.6;

/// Horizontal padding a table cell adds around its label.
#[cfg(test)]
const CELL_PADDING: f32 = 22.0;

/// The estimated rendered width of a label, counting wide scripts as wide.
#[cfg(test)]
pub(crate) fn label_width(label: &str) -> f32 {
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
#[cfg(test)]
pub(crate) fn header_fits(label: &str, column_width: f32) -> bool {
    label_width(label) + CELL_PADDING <= column_width
}

/// The width the process table asks for in total.
#[cfg(test)]
pub(crate) fn process_table_width(columns: &[ProcessColumn]) -> f32 {
    columns
        .iter()
        .map(|column| ProcessColumnLayout::width_of(*column))
        .sum()
}

/// Whether the table fits the viewport or has to scroll sideways.
///
/// Scrolling is a supported outcome; silently clipping a column is not, which
/// is why this is a decision the tests can assert rather than an accident of
/// the flex layout.
#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TableLayout {
    Fits,
    HorizontalScroll,
}

#[cfg(test)]
pub(crate) fn table_layout(viewport_width: f32, scale: f32, total_columns: f32) -> TableLayout {
    // At a larger scale the same window holds fewer logical pixels.
    let logical = viewport_width / scale;
    // The sidebar and page padding are not available to the table.
    let available = logical - 300.0;
    if total_columns <= available {
        TableLayout::Fits
    } else {
        TableLayout::HorizontalScroll
    }
}

/// Whether a row of action buttons fits on one line.
#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ActionLayout {
    Inline,
    Wrapped,
}

#[cfg(test)]
pub(crate) fn action_layout(viewport_width: f32, scale: f32, labels: &[&str]) -> ActionLayout {
    let logical_width = viewport_width / scale;
    let needed: f32 = labels
        .iter()
        .map(|label| label_width(label) + 34.0)
        .sum::<f32>()
        + 320.0;
    if logical_width < 700.0 || logical_width < needed {
        ActionLayout::Wrapped
    } else {
        ActionLayout::Inline
    }
}
