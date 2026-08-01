//! Small, deterministic layout policy checks used by the GPUI views.
//!
//! The rendered rows also use `min_w_0` and `flex_wrap`; this policy makes the
//! breakpoint explicit for long localized action labels.

pub(crate) const MIN_WINDOW_WIDTH: f32 = 720.0;
pub(crate) const MIN_WINDOW_HEIGHT: f32 = 540.0;
pub(crate) const COMPACT_VIEWPORT_WIDTH: f32 = 1040.0;

#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ActionLayout {
    Inline,
    Wrapped,
}

#[cfg(test)]
pub(crate) fn action_layout(
    viewport_width: f32,
    scale: f32,
    longest_label_chars: usize,
) -> ActionLayout {
    let logical_width = viewport_width / scale;
    let label_allowance = longest_label_chars as f32 * 9.0 + 320.0;
    if logical_width < 680.0 || logical_width < label_allowance {
        ActionLayout::Wrapped
    } else {
        ActionLayout::Inline
    }
}
