//! Shared UI view models. GPUI rendering primitives are added in the GUI slice.

use better_core::ComponentManifest;
use gpui::{AnyElement, Hsla, IntoElement, ParentElement, Pixels, SharedString, Styled, div};
use gpui_component::*;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StatusCard {
    pub title: String,
    pub value: String,
    pub detail: String,
}

pub fn component_card(manifest: &ComponentManifest) -> StatusCard {
    StatusCard {
        title: manifest.display_name.clone(),
        value: manifest.version.to_string(),
        detail: format!("{} component", manifest.component_type_label()),
    }
}

/// Shared status card primitive used by manager and monitor shells.
pub fn status_card(
    title: impl Into<SharedString>,
    value: impl Into<SharedString>,
) -> impl IntoElement {
    div().v_flex().child(title.into()).child(value.into())
}

/// Shared root heading primitive used by both first-party desktop applications.
pub fn page_heading(title: impl Into<SharedString>) -> impl IntoElement {
    div().child(title.into())
}

/// Theme-aware surface primitive shared by first-party desktop applications.
pub fn surface(
    child: impl IntoElement,
    border: Hsla,
    background: Hsla,
    radius: Pixels,
) -> impl IntoElement {
    div()
        .min_w_0()
        .p_4()
        .rounded(radius)
        .border_1()
        .border_color(border)
        .bg(background)
        .child(child)
}

/// How a badge reads. The colors come from the calling shell's theme so a
/// primitive never carries a palette of its own.
#[derive(Clone, Copy, Debug)]
pub struct BadgeStyle {
    pub foreground: Hsla,
    pub background: Hsla,
    pub border: Hsla,
}

/// A small labelled chip. Both the application source badge and the MIME
/// compatibility badge the app chooser shows are this primitive with different
/// styles, so they cannot drift apart visually.
pub fn badge(label: impl Into<SharedString>, style: BadgeStyle) -> impl IntoElement {
    div()
        .px_2()
        .py_0p5()
        .rounded_full()
        .border_1()
        .border_color(style.border)
        .bg(style.background)
        .text_xs()
        .text_color(style.foreground)
        .child(label.into())
}

/// The grid presentation of one application: icon glyph, name, and the badges
/// the caller decided to show.
pub fn application_tile(
    glyph: impl Into<SharedString>,
    name: impl Into<SharedString>,
    detail: impl Into<SharedString>,
    badges: Vec<AnyElement>,
    style: TileStyle,
) -> impl IntoElement {
    div()
        .v_flex()
        .min_w_0()
        .gap_2()
        .p_3()
        .rounded(style.radius)
        .border_1()
        .border_color(style.border)
        .bg(style.background)
        .child(
            div()
                .size_10()
                .flex()
                .items_center()
                .justify_center()
                .rounded(style.radius)
                .bg(style.glyph_background)
                .text_color(style.glyph_foreground)
                .child(glyph.into()),
        )
        .child(
            div()
                .min_w_0()
                .text_sm()
                .font_semibold()
                .text_color(style.foreground)
                .child(name.into()),
        )
        .child(
            div()
                .min_w_0()
                .text_xs()
                .text_color(style.muted_foreground)
                .child(detail.into()),
        )
        .child(div().flex().flex_wrap().gap_1().children(badges))
}

/// The list presentation of one application. Same information as the tile, laid
/// out for scanning a long list rather than for a grid.
pub fn application_list_row(
    glyph: impl Into<SharedString>,
    name: impl Into<SharedString>,
    detail: impl Into<SharedString>,
    badges: Vec<AnyElement>,
    style: TileStyle,
) -> impl IntoElement {
    div()
        .flex()
        .w_full()
        .min_w_0()
        .items_center()
        .gap_3()
        .px_3()
        .py_2()
        .rounded(style.radius)
        .border_1()
        .border_color(style.border)
        .bg(style.background)
        .child(
            div()
                .size_8()
                .flex()
                .items_center()
                .justify_center()
                .rounded(style.radius)
                .bg(style.glyph_background)
                .text_color(style.glyph_foreground)
                .child(glyph.into()),
        )
        .child(
            div()
                .v_flex()
                .flex_1()
                .min_w_0()
                .gap_0p5()
                .child(
                    div()
                        .min_w_0()
                        .text_sm()
                        .font_semibold()
                        .text_color(style.foreground)
                        .child(name.into()),
                )
                .child(
                    div()
                        .min_w_0()
                        .text_xs()
                        .text_color(style.muted_foreground)
                        .child(detail.into()),
                ),
        )
        .child(div().flex().flex_wrap().gap_1().children(badges))
}

/// The colors an application tile or row uses, including its selected state.
/// A selected item differs by border and background rather than by a color the
/// primitive invented.
#[derive(Clone, Copy, Debug)]
pub struct TileStyle {
    pub foreground: Hsla,
    pub muted_foreground: Hsla,
    pub border: Hsla,
    pub background: Hsla,
    pub glyph_foreground: Hsla,
    pub glyph_background: Hsla,
    pub radius: Pixels,
}

/// A centered state message: nothing found, still loading, or an operation that
/// failed. One primitive so all three read the same way.
pub fn state_message(
    title: impl Into<SharedString>,
    detail: impl Into<SharedString>,
    foreground: Hsla,
    muted_foreground: Hsla,
) -> impl IntoElement {
    div()
        .v_flex()
        .w_full()
        .items_center()
        .gap_2()
        .py_10()
        .child(
            div()
                .text_sm()
                .font_semibold()
                .text_color(foreground)
                .child(title.into()),
        )
        .child(
            div()
                .text_xs()
                .text_color(muted_foreground)
                .child(detail.into()),
        )
}

/// A full-width notice bar. Used for a launch failure and for the explanation
/// shown when a chosen application does not declare the selected file type.
pub fn notice(
    message: impl Into<SharedString>,
    foreground: Hsla,
    background: Hsla,
    radius: Pixels,
) -> impl IntoElement {
    div()
        .w_full()
        .min_w_0()
        .px_3()
        .py_2()
        .rounded(radius)
        .bg(background)
        .text_sm()
        .text_color(foreground)
        .child(message.into())
}

trait ComponentTypeLabel {
    fn component_type_label(&self) -> &'static str;
}

impl ComponentTypeLabel for ComponentManifest {
    fn component_type_label(&self) -> &'static str {
        match &self.component_type {
            better_core::ComponentType::Replacement => "replacement",
            better_core::ComponentType::Enhancement => "enhancement",
            better_core::ComponentType::Diagnostic => "diagnostic",
        }
    }
}
