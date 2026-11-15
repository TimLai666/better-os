//! What Better Touchpad's screens decide, without GPUI.
//!
//! Every decision a Touchpad screen draws — which controls exist, whether one
//! is available, what a value reads as, what an apply did, what a restore
//! would put back, what the gesture preset would change — comes from
//! [`model::TouchpadModel`] and [`gestures_model::GestureScreen`], which are
//! asserted here without a display server. Better Manager's Touchpad page
//! (ticket 88) and the older `better-touchpad` window draw from them.
//!
//! Nothing in this crate constructs a backend command. There is no
//! `gsettings`, no `dconf`, no `xinput`, and no `std::process::Command`; every
//! change goes through `touchpad-platform`'s typed API. A test in `tests.rs`
//! asserts that over this crate's own source rather than leaving it to review.

pub mod gestures_model;
#[cfg(test)]
mod gestures_tests;
pub mod i18n;
pub mod model;
pub mod startup;
#[cfg(test)]
mod tests;

pub use i18n::Locale;
pub use model::{Page, PointerTrace, TouchpadModel};
pub use startup::{Startup, StartupOptions};

/// Whether a row of buttons fits on one line or has to wrap.
///
/// The rendered rows already carry `flex_wrap` and `min_w_0`; this makes the
/// breakpoint explicit so a long localized label cannot silently push a button
/// out of view.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ActionLayout {
    Inline,
    Wrapped,
}

/// A rough advance width per character. A CJK glyph is about twice as wide as
/// a Latin one, which is the difference that decides the breakpoint between
/// `zh-TW` and `en-US`.
pub fn label_width(label: &str) -> f32 {
    label
        .chars()
        .map(|character| if character.is_ascii() { 8.5 } else { 17.0 })
        .sum()
}

pub fn action_layout(viewport_width: f32, scale: f32, labels: &[&str]) -> ActionLayout {
    let logical_width = viewport_width / scale;
    // Each button carries its own padding, and the row carries a gap plus the
    // window's horizontal padding and the sidebar.
    let needed: f32 = labels
        .iter()
        .map(|label| label_width(label) + 44.0)
        .sum::<f32>()
        + 96.0;
    if logical_width >= needed {
        ActionLayout::Inline
    } else {
        ActionLayout::Wrapped
    }
}

/// Whether a label fits the column a settings row gives it.
///
/// A settings row is a label, a control, and a value. The label column is what
/// overflows first when a translation is longer than the English original.
pub fn label_fits(label: &str, column_width: f32, scale: f32) -> bool {
    label_width(label) <= column_width / scale
}
