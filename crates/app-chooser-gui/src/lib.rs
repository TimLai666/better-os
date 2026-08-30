//! The reusable Better App Chooser surface.
//!
//! `AppChooser` is a plain GPUI view. Better Files embeds it in its own window
//! and subscribes to [`ChooserEvent`]; the binary in this crate opens it as a
//! standalone window, which is how the surface is exercised without a file
//! manager existing yet.
//!
//! Everything the surface decides comes from `app-chooser-core`. This crate
//! renders, localizes, and routes the two actions; it owns no ranking rule and
//! no association logic of its own.

pub mod chooser;
pub mod cli;
pub mod i18n;
#[cfg(test)]
mod tests;

pub use chooser::{AppChooser, ChooserEvent, ChooserMode, ChooserTarget};
pub use i18n::Locale;

/// The chooser window's minimum size. At this width the three action labels
/// still sit on one row in both shipped locales at 100% scaling.
pub const MIN_WINDOW_WIDTH: f32 = 680.0;
pub const MIN_WINDOW_HEIGHT: f32 = 520.0;

/// Whether the action row fits on one line or has to wrap. The rendered row
/// already carries `flex_wrap`; this makes the breakpoint explicit so a long
/// localized label cannot silently push a button out of view, which is the
/// policy the manager GUI already follows.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ActionLayout {
    Inline,
    Wrapped,
}

/// A rough advance width per character. A CJK glyph is about twice as wide as
/// a Latin one, which is the difference that decides the breakpoint between
/// `zh-TW` and `en-US`.
fn label_width(label: &str) -> f32 {
    label
        .chars()
        .map(|character| if character.is_ascii() { 8.5 } else { 17.0 })
        .sum()
}

/// The layout for a row of buttons at a given viewport width and scale.
pub fn action_layout(viewport_width: f32, scale: f32, labels: &[&str]) -> ActionLayout {
    let logical_width = viewport_width / scale;
    // Each button carries its own padding, and the row carries a gap plus the
    // window's own horizontal padding.
    let needed: f32 = labels
        .iter()
        .map(|label| label_width(label) + 48.0)
        .sum::<f32>()
        + 56.0;
    if logical_width >= needed {
        ActionLayout::Inline
    } else {
        ActionLayout::Wrapped
    }
}
