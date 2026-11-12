//! Small, deterministic layout policy checks used by the GPUI views.
//!
//! The rendered rows also use `min_w_0` and `flex_wrap`; this policy makes the
//! breakpoint explicit for long localized action labels.

/// The window's minimum size, which the control center keeps at 960 × 600
/// (spec section 1).
pub(crate) const MIN_WINDOW_WIDTH: f32 = 960.0;
pub(crate) const MIN_WINDOW_HEIGHT: f32 = 600.0;

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

/// Roughly how many characters of a label fit on one line.
///
/// A collapsed column reads as one character per line, which is what makes this
/// worth asserting rather than only asserting a pixel width: the failure the
/// user saw is `1`.
#[cfg(test)]
pub(crate) fn characters_per_line(label_width: f32, character_advance: f32) -> usize {
    if character_advance <= 0.0 {
        return 0;
    }
    (label_width / character_advance).floor().max(0.0) as usize
}

/// A label narrower than this many characters is not a label any more.
#[cfg(test)]
pub(crate) const MIN_READABLE_CHARACTERS: usize = 12;

/// The narrowest the content column gets: the minimum window less the
/// compact pane (48px) and the page's 24px margins on either side. Today's
/// Better Manager screens lay out inside it.
#[cfg(test)]
pub(crate) const MIN_CONTENT_WIDTH: f32 = MIN_WINDOW_WIDTH - 48.0 - 48.0;
