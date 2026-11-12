//! `InfoBadge`: the small accent mark a navigation item carries when its page
//! has something to say — a count of things waiting, or a dot for "something
//! is going on".
//!
//! WinUI's `InfoBadge` comes in three shapes; the kit has the two a
//! navigation pane uses. The count is 16px tall on the accent fill with 8px
//! corners and Caption text in `TextOnAccentFillColorPrimary`, at least 16px
//! wide with 4px of padding either side of a longer number; the dot is a 6px
//! accent circle. It is the one rounded-end shape in the kit, because WinUI's
//! badge is one. WinUI's own badge resources were not in the copy of
//! `microsoft-ui-xaml` this kit was read from, so these two sizes follow the
//! control-center mockups (`design/control-center/home-*.html`) rather than a
//! XAML file, and say so here. A count above 99 reads `99+`, so the badge
//! never grows wider than three characters.

use gpui::{
    App, IntoElement, ParentElement, Pixels, RenderOnce, SharedString, Styled, Window, div, px,
};

use super::{CAPTION, palette, text};

/// The tallest number the badge spells out; anything above it is `99+`.
pub const MAX_SHOWN_COUNT: usize = 99;
/// The badge's height, and the narrowest a count badge is.
pub const COUNT_HEIGHT: Pixels = px(16.0);
/// The dot's diameter.
pub const DOT_SIZE: Pixels = px(6.0);

/// What a badge shows.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum InfoBadge {
    /// "Something is happening here", with no number.
    Dot,
    /// How many things are waiting. Never zero: see [`InfoBadge::count`].
    Count(usize),
}

impl InfoBadge {
    /// A count badge, or none at all for zero: a page with nothing waiting
    /// carries no badge, rather than a badge that says 0.
    pub fn count(count: usize) -> Option<InfoBadge> {
        (count > 0).then_some(InfoBadge::Count(count))
    }

    /// The text inside the badge. The dot has none.
    pub fn text(self) -> Option<String> {
        match self {
            InfoBadge::Dot => None,
            InfoBadge::Count(count) if count > MAX_SHOWN_COUNT => {
                Some(format!("{MAX_SHOWN_COUNT}+"))
            }
            InfoBadge::Count(count) => Some(count.to_string()),
        }
    }
}

/// The drawn badge.
#[derive(IntoElement)]
pub struct Badge {
    badge: InfoBadge,
}

impl Badge {
    pub fn new(badge: InfoBadge) -> Self {
        Self { badge }
    }
}

impl RenderOnce for Badge {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let p = palette(cx);
        match self.badge.text() {
            None => div().flex_none().size(DOT_SIZE).rounded_full().bg(p.accent),
            Some(number) => text(div(), CAPTION)
                .flex_none()
                .h(COUNT_HEIGHT)
                .min_w(COUNT_HEIGHT)
                .px(px(4.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(8.0))
                .bg(p.accent)
                .text_color(p.text_on_accent)
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child(SharedString::from(number)),
        }
    }
}
