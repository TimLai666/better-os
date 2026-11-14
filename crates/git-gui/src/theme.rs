//! Applying the appearance: dark, light, or the desktop's, in the Fluent
//! palette.

use gpui::{Context, Subscription, Window};

use crate::Appearance;
use crate::app::GitApp;

/// Repaints the window in the chosen appearance. For `System` it keeps
/// repainting whenever the desktop switches, for as long as the returned
/// subscription is held. Reading the desktop once was not enough: the
/// desktop's appearance reaches GPUI only after the window has opened, so a
/// stored `system` opened light on a dark desktop (ticket 110).
pub fn apply(
    appearance: Appearance,
    window: &mut Window,
    cx: &mut Context<GitApp>,
) -> Option<Subscription> {
    appearance.fluent().apply_and_follow(window, cx)
}
