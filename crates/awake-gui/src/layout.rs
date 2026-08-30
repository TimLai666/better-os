//! Small, deterministic layout policy checks used by the GPUI views.
//!
//! The rendered rows also use `min_w_0` and `flex_wrap`; this policy makes the
//! breakpoint explicit for long localized action labels, so the question "does
//! this wrap at 150% scaling in Chinese" has an answer that a test can assert
//! without a display server.
//!
//! This mirrors `manager-gui::layout`. The allowance is wider here because an
//! action row in this window sits beside a session summary rather than beside a
//! plain list.

pub(crate) const MIN_WINDOW_WIDTH: f32 = 760.0;
pub(crate) const MIN_WINDOW_HEIGHT: f32 = 560.0;
/// Below this the sidebar collapses to icons.
pub(crate) const COMPACT_VIEWPORT_WIDTH: f32 = 1040.0;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ActionLayout {
    Inline,
    Wrapped,
}

/// Whether a row of action buttons still fits on one line.
///
/// `scale` is the desktop text scaling factor, so 1.25 means every label is a
/// quarter wider in physical pixels while the window is not.
pub(crate) fn action_layout(
    viewport_width: f32,
    scale: f32,
    longest_label_chars: usize,
) -> ActionLayout {
    let logical_width = viewport_width / scale;
    // 320px is the session summary the action row shares its line with; 9px is
    // the per-character allowance a label needs at the base text size.
    let label_allowance = longest_label_chars as f32 * 9.0 + 320.0;
    if logical_width < 680.0 || logical_width < label_allowance {
        ActionLayout::Wrapped
    } else {
        ActionLayout::Inline
    }
}
