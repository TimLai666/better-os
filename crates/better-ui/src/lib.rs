//! Shared UI view models. GPUI rendering primitives are added in the GUI slice.

use better_core::ComponentManifest;
use gpui::{Hsla, IntoElement, ParentElement, Pixels, SharedString, Styled, div};
use gpui_component::*;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Locale {
    #[default]
    System,
    EnUs,
    ZhTw,
}

impl Locale {
    pub fn resolved(self) -> Self {
        match self {
            Self::System => {
                let language = std::env::var("LANG")
                    .unwrap_or_default()
                    .to_ascii_lowercase();
                if language.contains("zh_tw")
                    || language.contains("zh-tw")
                    || language.contains("hant")
                {
                    Self::ZhTw
                } else {
                    Self::EnUs
                }
            }
            locale => locale,
        }
    }

    pub const fn config_value(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::EnUs => "en-US",
            Self::ZhTw => "zh-TW",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value {
            "en-US" | "en-us" | "en" => Self::EnUs,
            "zh-TW" | "zh-tw" | "zh_Hant" | "zh-hant" => Self::ZhTw,
            _ => Self::System,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::System => "System language",
            Self::EnUs => "English (United States)",
            Self::ZhTw => "繁體中文（台灣）",
        }
    }

    pub const fn next(self) -> Self {
        match self {
            Self::System => Self::EnUs,
            Self::EnUs => Self::ZhTw,
            Self::ZhTw => Self::System,
        }
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locale_values_round_trip_and_cycle() {
        for locale in [Locale::System, Locale::EnUs, Locale::ZhTw] {
            assert_eq!(Locale::parse(locale.config_value()), locale);
        }
        assert_eq!(Locale::System.next(), Locale::EnUs);
        assert_eq!(Locale::EnUs.next(), Locale::ZhTw);
        assert_eq!(Locale::ZhTw.next(), Locale::System);
    }
}
