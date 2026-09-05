//! Small, deterministic layout policy checks used by the GPUI views.
//!
//! The rendered rows also use `min_w_0` and `flex_wrap`; this policy makes the
//! breakpoint explicit for long localized action labels.

pub(crate) const MIN_WINDOW_WIDTH: f32 = 720.0;
pub(crate) const MIN_WINDOW_HEIGHT: f32 = 540.0;
pub(crate) const COMPACT_VIEWPORT_WIDTH: f32 = 1040.0;

/// The narrowest a step-list label column may become before the row that holds
/// it has to wrap. It is also the `min_w` the rendered column carries, so the
/// policy below and the element agree on one number.
pub(crate) const STEP_LABEL_MIN_WIDTH: f32 = 220.0;

/// Icon well plus its gap: the part of a bullet row that is never label.
#[cfg(test)]
const STEP_ROW_GUTTER: f32 = 40.0;

/// Page padding, the surface's own padding and border, and the gap between the
/// two first-run columns. Kept beside the policy so a padding change that
/// starves the label fails a test rather than a screenshot.
#[cfg(test)]
const PAGE_PADDING: f32 = 40.0;
#[cfg(test)]
const SURFACE_INSET: f32 = 34.0;
#[cfg(test)]
const COLUMN_GAP: f32 = 20.0;
#[cfg(test)]
const COLUMN_MIN_WIDTH: f32 = 280.0;

/// The widest the centred content column is allowed to grow.
#[cfg(test)]
const CONTENT_MAX_WIDTH: f32 = 1040.0;

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

/// Whether the two first-run columns sit beside each other or stack.
#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ColumnLayout {
    SideBySide,
    Stacked,
}

/// How wide one of the two first-run columns is, and whether they wrapped.
///
/// The row carries `flex_wrap`, so when two columns no longer fit at their
/// declared minimum the row stacks them and each one gets the full width. That
/// is the behaviour this reproduces.
#[cfg(test)]
pub(crate) fn first_run_column(viewport_width: f32, scale: f32) -> (ColumnLayout, f32) {
    let logical_width = viewport_width / scale;
    let content = (logical_width - PAGE_PADDING).min(CONTENT_MAX_WIDTH);
    let inner = content - SURFACE_INSET;
    if inner >= COLUMN_MIN_WIDTH * 2.0 + COLUMN_GAP {
        (ColumnLayout::SideBySide, (inner - COLUMN_GAP) / 2.0)
    } else {
        (ColumnLayout::Stacked, inner)
    }
}

/// The width a step-list label actually receives.
///
/// This is the number the field report was about: the label column had no flex
/// grow factor, so it never claimed the column's width and rendered at its
/// min-content size instead — one character wide in Chinese. The rendered
/// column now grows into the space this function describes and carries
/// `STEP_LABEL_MIN_WIDTH` as its floor.
#[cfg(test)]
pub(crate) fn step_label_width(viewport_width: f32, scale: f32) -> f32 {
    let (_, column) = first_run_column(viewport_width, scale);
    (column - STEP_ROW_GUTTER).max(STEP_LABEL_MIN_WIDTH)
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

/// The advance width of one character at `text_sm`. A Chinese glyph is
/// full-width; Latin text averages a little under half that.
#[cfg(test)]
pub(crate) fn character_advance(locale: crate::i18n::Locale) -> f32 {
    match locale {
        crate::i18n::Locale::ZhTw => 14.0,
        crate::i18n::Locale::EnUs | crate::i18n::Locale::System => 7.5,
    }
}
