//! Shared UI view models. GPUI rendering primitives are added in the GUI slice.

use better_core::ComponentManifest;
use gpui::{Hsla, IntoElement, ParentElement, Pixels, SharedString, Styled, div, px};
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

/// Selects Traditional Chinese or English copy from the resolved locale.
pub fn localized(locale: Locale, en_us: &'static str, zh_tw: &'static str) -> &'static str {
    match locale.resolved() {
        Locale::ZhTw => zh_tw,
        _ => en_us,
    }
}

/// Expands interface copy for pseudo-localization overflow tests.
pub fn pseudo_long_text(input: &str) -> String {
    const PAD: &str = " · extended";
    let mut output = String::with_capacity(input.len().saturating_mul(2) + 8);
    output.push('⟦');
    for segment in input.split_inclusive('\n') {
        let (line, newline) = segment
            .strip_suffix('\n')
            .map_or((segment, false), |line| (line, true));
        output.push_str(line);
        if !line.trim().is_empty() {
            output.push_str(PAD);
        }
        if newline {
            output.push('\n');
        }
    }
    output.push('⟧');
    output
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SupportStateKind {
    Success,
    Info,
    Unavailable,
    PermissionDenied,
    Stale,
    CollectorError,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SupportState {
    pub kind: SupportStateKind,
    pub title: String,
    pub detail: String,
}

impl SupportState {
    pub fn new(
        kind: SupportStateKind,
        title: impl Into<String>,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            title: title.into(),
            detail: detail.into(),
        }
    }
}

#[derive(Clone, Copy)]
pub struct SupportStatePalette {
    pub border: Hsla,
    pub background: Hsla,
    pub foreground: Hsla,
    pub muted_foreground: Hsla,
    pub success: Hsla,
    pub warning: Hsla,
    pub danger: Hsla,
    pub info: Hsla,
    pub radius: Pixels,
}

impl SupportStatePalette {
    fn accent(self, kind: SupportStateKind) -> Hsla {
        match kind {
            SupportStateKind::Success => self.success,
            SupportStateKind::Info => self.info,
            SupportStateKind::Unavailable => self.muted_foreground,
            SupportStateKind::PermissionDenied | SupportStateKind::CollectorError => self.danger,
            SupportStateKind::Stale => self.warning,
        }
    }
}

pub fn support_state_panel(state: SupportState, palette: SupportStatePalette) -> impl IntoElement {
    let accent = palette.accent(state.kind);
    h_flex()
        .items_center()
        .gap_3()
        .min_w_0()
        .rounded(palette.radius)
        .border_1()
        .border_color(palette.border)
        .bg(palette.background)
        .p_3()
        .child(div().size_3().flex_shrink_0().rounded(px(99.0)).bg(accent))
        .child(
            v_flex()
                .min_w_0()
                .gap_1()
                .child(
                    div()
                        .font_bold()
                        .text_color(accent)
                        .child(state.title.clone()),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(palette.muted_foreground)
                        .child(state.detail.clone()),
                ),
        )
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

    #[test]
    fn localized_copy_uses_the_resolved_locale() {
        assert_eq!(localized(Locale::EnUs, "Settings", "設定"), "Settings");
        assert_eq!(localized(Locale::ZhTw, "Settings", "設定"), "設定");
    }

    #[test]
    fn pseudo_long_copy_is_readable_and_longer() {
        let source = "Force stop\nPermission required";
        let expanded = pseudo_long_text(source);
        assert!(expanded.starts_with('⟦'));
        assert!(expanded.ends_with('⟧'));
        assert!(expanded.contains("Force stop"));
        assert!(expanded.contains("Permission required"));
        assert!(expanded.contains('\n'));
        assert!(expanded.len() >= source.len() + 20);
    }

    #[test]
    fn support_state_keeps_semantics_separate_from_copy() {
        let state = SupportState::new(
            SupportStateKind::PermissionDenied,
            "Permission required",
            "Linux rejected the operation",
        );
        assert_eq!(state.kind, SupportStateKind::PermissionDenied);
        assert_eq!(state.title, "Permission required");
        assert_eq!(state.detail, "Linux rejected the operation");
    }
}
