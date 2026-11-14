//! A label chip: a short name attached to a row, such as the branches and
//! tags on a commit in a branch graph.
//!
//! WinUI has no chip control; this is the smallest shape the kit's rules
//! allow. It sits in the page, so its corners are 4px, with Caption text,
//! 20px tall, and an optional 12px leading icon. A chip that names a data
//! series (a graph lane) is tinted with that series' hue, as the area under
//! a chart line is ([`ChartHue::area`]), and outlined in it; otherwise it
//! has the control fill and stroke. The emphasized chip (the current branch)
//! has Body Strong's weight and a check mark, never the accent, which stays
//! for the primary action, the selection, and focus.

use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, FontWeight, IntoElement, ParentElement, Pixels, RenderOnce, Rgba, SharedString, Styled,
    Window, div, px,
};
use gpui_component::{Icon, IconName};

use super::chart::ChartHue;
use super::{CAPTION, Palette, palette, radius, text};

/// The chip's height.
pub const CHIP_HEIGHT: Pixels = px(20.0);

/// A chip's fill, stroke, and text colors.
///
/// Expected results:
/// - A hued chip is filled with the hue's area tint and outlined in the
///   hue's line color for the appearance.
/// - A plain chip uses the control fill and the control stroke.
/// - The text is always the primary text color, so a chip is as readable
///   in either appearance as the row it sits on.
pub fn chip_colors(hue: Option<ChartHue>, p: &Palette) -> (Rgba, Rgba, Rgba) {
    let dark = p.is_dark();
    match hue {
        Some(hue) => (hue.area(dark), hue.color(dark), p.text_primary),
        None => (p.control_fill, p.stroke, p.text_primary),
    }
}

#[derive(IntoElement)]
pub struct Chip {
    text: SharedString,
    icon: Option<Icon>,
    hue: Option<ChartHue>,
    emphasized: bool,
}

impl Chip {
    pub fn new(text: impl Into<SharedString>) -> Self {
        Self {
            text: text.into(),
            icon: None,
            hue: None,
            emphasized: false,
        }
    }

    pub fn icon(mut self, icon: impl Into<Icon>) -> Self {
        self.icon = Some(icon.into());
        self
    }

    /// Tints the chip with a data series' hue.
    pub fn hue(mut self, hue: ChartHue) -> Self {
        self.hue = Some(hue);
        self
    }

    /// Marks the chip as the one that matters most in its row (the current
    /// branch): heavier text and a check mark.
    pub fn emphasized(mut self, emphasized: bool) -> Self {
        self.emphasized = emphasized;
        self
    }
}

impl RenderOnce for Chip {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let p = palette(cx);
        let (fill, stroke, color) = chip_colors(self.hue, p);
        let icon = if self.emphasized {
            Some(Icon::new(IconName::Check))
        } else {
            self.icon
        };
        text(div(), CAPTION)
            .flex()
            .flex_none()
            .items_center()
            .gap(px(4.0))
            .h(CHIP_HEIGHT)
            .px(px(6.0))
            .rounded(radius::SM)
            .border_1()
            .border_color(stroke)
            .bg(fill)
            .text_color(color)
            .when_some(icon, |d, icon| {
                d.child(icon.size(px(12.0)).text_color(color))
            })
            .child(
                div()
                    .when(self.emphasized, |d| d.font_weight(FontWeight::SEMIBOLD))
                    .child(self.text),
            )
    }
}
