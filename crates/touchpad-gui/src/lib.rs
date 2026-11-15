//! Better Touchpad's control centre.
//!
//! The window is a renderer. Every decision it draws — which controls exist,
//! whether one is available, what a value reads as, what an apply did, what a
//! restore would put back — comes from [`model::TouchpadModel`], which has no
//! GPUI dependency and is asserted without a display server.
//!
//! Nothing in this crate constructs a backend command. There is no `gsettings`,
//! no `dconf`, no `xinput`, and no `std::process::Command`; every change goes
//! through `touchpad-platform`'s typed API. A test in `tests.rs` asserts that
//! over this crate's own source rather than leaving it to review.

pub mod app;
pub mod command_line;
pub mod pages;
pub mod pages_gestures;
pub mod restart;
#[cfg(test)]
mod tests;

// The GPUI-free half moved to `touchpad-views` (ticket 88) so Better
// Manager's Touchpad page draws from the same models; the paths stay the same
// for this window.
pub use touchpad_views::{
    ActionLayout, Locale, Page, PointerTrace, Startup, StartupOptions, TouchpadModel,
    action_layout, gestures_model, i18n, label_fits, label_width, model, startup,
};

pub use app::TouchpadApp;

/// The window's minimum size. At this width the action row still fits on one
/// line in both shipped locales at 100% scaling.
pub const MIN_WINDOW_WIDTH: f32 = 760.0;
pub const MIN_WINDOW_HEIGHT: f32 = 560.0;

/// Below this viewport width the sidebar collapses to icons and the two-column
/// rows stack.
pub const COMPACT_VIEWPORT_WIDTH: f32 = 1000.0;
