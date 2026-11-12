//! Deterministic layout policy, so the localized layouts can be tested.
//!
//! A GPUI window cannot be laid out in CI, so the parts of the layout a
//! translation or a narrow window can break are decided by these functions
//! and asserted directly: when the navigation pane folds to its icons, how
//! many cards sit in a row, how many commands fit beside a page title before
//! the rest move into the overflow menu, and whether a table column is wide
//! enough for its heading in either language.
//!
//! Character width is estimated rather than measured. A CJK ideograph
//! occupies roughly twice the advance of a Latin letter at the same size, and
//! the estimate is deliberately generous, because the failure this guards
//! against is a clipped label rather than a slightly loose column.

pub(crate) const MIN_WINDOW_WIDTH: f32 = 800.0;
pub(crate) const MIN_WINDOW_HEIGHT: f32 = 600.0;

/// Below this width the navigation pane shows only its icons, as WinUI's
/// NavigationView does in its Auto mode (compact from 641 to 1007 pixels).
pub(crate) const EXPANDED_NAVIGATION_MIN_WIDTH: f32 = 1008.0;

/// The page's margin on each side.
pub(crate) const PAGE_MARGIN: f32 = 24.0;

/// Advance width of one Latin character of Body text (14px).
const LATIN_ADVANCE: f32 = 7.6;

/// Whether the pane starts folded at this window width.
pub(crate) fn navigation_compact(window_width: f32) -> bool {
    window_width < EXPANDED_NAVIGATION_MIN_WIDTH
}

/// The width a page's content has, inside the pane and the margins.
pub(crate) fn content_width(window_width: f32, compact: bool) -> f32 {
    let pane = if compact { 48.0 } else { 320.0 };
    (window_width - pane - 1.0 - 2.0 * PAGE_MARGIN).max(0.0)
}

/// How many overview cards share a row: three where three fit at 260 pixels
/// each, otherwise two, otherwise one.
pub(crate) fn card_columns(content_width: f32) -> usize {
    let gap = 16.0;
    (1..=3)
        .rev()
        .find(|columns| {
            let columns = *columns as f32;
            (content_width - gap * (columns - 1.0)) / columns >= 260.0
        })
        .unwrap_or(1)
}

/// The estimated rendered width of a label, counting wide scripts as wide.
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

/// The width a command bar button takes: 16px icon, the label, and WinUI's
/// padding.
pub(crate) fn command_width(label: &str) -> f32 {
    16.0 + 8.0 + label_width(label) + 24.0
}

/// How many of a page's commands fit beside its title; the rest go into the
/// overflow menu. The title takes its own width first.
pub(crate) fn commands_that_fit(content_width: f32, title: &str, labels: &[&str]) -> usize {
    let title = label_width(title) * 2.0 + 24.0;
    let widths: Vec<gpui::Pixels> = labels
        .iter()
        .map(|label| gpui::px(command_width(label)))
        .collect();
    better_ui::fluent::command_bar::fit_count(
        &widths,
        gpui::px((content_width - title).max(0.0)),
        gpui::px(40.0),
    )
}

#[cfg(test)]
/// Whether a column heading fits its column. A fixed column's width
/// includes 12px of padding on each side and room for the sort chevron.
pub(crate) fn heading_fits(label: &str, column_width: f32) -> bool {
    label_width(label) + 24.0 + 16.0 <= column_width
}

#[cfg(test)]
/// The width the Details table's flexible columns are left with.
pub(crate) fn flexible_width(content_width: f32, fixed: &[f32]) -> f32 {
    content_width - fixed.iter().sum::<f32>()
}
