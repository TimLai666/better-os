//! Applying the appearance: dark, light, or the desktop's, in the Fluent
//! palette.

use gpui::{Context, Window, WindowAppearance};

use crate::Appearance;
use crate::app::GitApp;

/// Repaints the window in the chosen appearance. `System` reads the
/// desktop's appearance now; the window follows later changes through
/// GPUI's appearance observer in `render`.
pub fn apply(appearance: Appearance, window: &mut Window, cx: &mut Context<GitApp>) {
    let dark = match appearance {
        Appearance::Dark => true,
        Appearance::Light => false,
        Appearance::System => matches!(
            window.appearance(),
            WindowAppearance::Dark | WindowAppearance::VibrantDark
        ),
    };
    better_ui::fluent::set_appearance(dark, Some(window), cx);
}
