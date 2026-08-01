//! Shared UI view models. GPUI rendering primitives are added in the GUI slice.

use better_core::ComponentManifest;
use gpui::{Hsla, IntoElement, ParentElement, Pixels, SharedString, Styled, div};
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
