//! The status row's keyboard layout button (design/login-screen-spec.md
//! section 3.7).
//!
//! The layouts are the system's: `org.freedesktop.locale1`'s `X11Layout` and
//! `X11Variant`, comma-separated lists in the same order (`/etc/default/keyboard`
//! on the owner's machine has `XKBLAYOUT="us"`). The button shows the XKB
//! layout code in capitals (`US`) and is named for a screen reader with the
//! layout's full name, 「鍵盤配置：英文（美式）」 / "Keyboard layout: English
//! (US)". A layout this table does not know is named by its XKB code as it
//! is (`xx`, `xx(variant)`), never by a guess.

use crate::copy::{Language, fill};

/// One XKB layout and its variant (empty for the layout's default).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KeyboardLayout {
    pub layout: String,
    pub variant: String,
}

/// (layout, variant, zh-TW, English), from xkeyboard-config's descriptions.
const NAMES: &[(&str, &str, &str, &str)] = &[
    ("us", "", "英文（美式）", "English (US)"),
    (
        "us",
        "intl",
        "英文（美式，國際版含無效鍵）",
        "English (US, intl., with dead keys)",
    ),
    ("us", "dvorak", "英文（Dvorak）", "English (Dvorak)"),
    ("us", "colemak", "英文（Colemak）", "English (Colemak)"),
    ("gb", "", "英文（英式）", "English (UK)"),
    ("tw", "", "台灣", "Taiwanese"),
    ("cn", "", "中文", "Chinese"),
    ("jp", "", "日文", "Japanese"),
    ("kr", "", "韓文", "Korean"),
    ("de", "", "德文", "German"),
    ("ch", "", "德文（瑞士）", "German (Switzerland)"),
    ("fr", "", "法文", "French"),
    ("ca", "", "法文（加拿大）", "French (Canada)"),
    ("es", "", "西班牙文", "Spanish"),
    ("it", "", "義大利文", "Italian"),
    ("pt", "", "葡萄牙文", "Portuguese"),
    ("br", "", "葡萄牙文（巴西）", "Portuguese (Brazil)"),
    ("ru", "", "俄文", "Russian"),
];

impl KeyboardLayout {
    pub fn new(layout: &str, variant: &str) -> Self {
        Self {
            layout: layout.trim().to_string(),
            variant: variant.trim().to_string(),
        }
    }

    /// The layouts `locale1` lists, in order. Empty entries are dropped; a
    /// variant list shorter than the layout list leaves the rest default.
    pub fn from_locale1(x11_layout: &str, x11_variant: &str) -> Vec<Self> {
        let variants: Vec<&str> = x11_variant.split(',').collect();
        x11_layout
            .split(',')
            .enumerate()
            .filter(|(_, layout)| !layout.trim().is_empty())
            .map(|(index, layout)| Self::new(layout, variants.get(index).copied().unwrap_or("")))
            .collect()
    }

    /// The button's text: the layout code in capitals.
    pub fn short_name(&self) -> String {
        self.layout.to_ascii_uppercase()
    }

    /// The layout's full name, or its XKB code when the table does not know
    /// it.
    pub fn full_name(&self, language: Language) -> String {
        let known = NAMES
            .iter()
            .find(|(layout, variant, _, _)| *layout == self.layout && *variant == self.variant);
        match known {
            Some((_, _, zh, en)) => match language {
                Language::ZhTw => (*zh).to_string(),
                Language::EnUs => (*en).to_string(),
            },
            None if self.variant.is_empty() => self.layout.clone(),
            None => format!("{}({})", self.layout, self.variant),
        }
    }

    /// The button's accessible name.
    pub fn spoken_name(&self, language: Language) -> String {
        fill(
            language.copy().keyboard_layout,
            &[("layout", &self.full_name(language))],
        )
    }
}

/// Whether the button opens a menu of layouts: only with more than one.
pub fn layout_button_opens_menu(layouts: &[KeyboardLayout]) -> bool {
    layouts.len() > 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_owners_us_layout() {
        let layouts = KeyboardLayout::from_locale1("us", "");
        assert_eq!(layouts, [KeyboardLayout::new("us", "")]);
        let us = &layouts[0];
        assert_eq!(us.short_name(), "US");
        assert_eq!(us.spoken_name(Language::ZhTw), "鍵盤配置：英文（美式）");
        assert_eq!(
            us.spoken_name(Language::EnUs),
            "Keyboard layout: English (US)"
        );
        assert!(!layout_button_opens_menu(&layouts));
    }

    #[test]
    fn several_layouts_pair_with_their_variants() {
        let layouts = KeyboardLayout::from_locale1("us,de,fr", "dvorak,,");
        assert_eq!(layouts.len(), 3);
        assert_eq!(layouts[0].full_name(Language::EnUs), "English (Dvorak)");
        assert_eq!(layouts[1].full_name(Language::ZhTw), "德文");
        assert_eq!(layouts[2].short_name(), "FR");
        assert!(layout_button_opens_menu(&layouts));
        let short_variants = KeyboardLayout::from_locale1("us,gb", "intl");
        assert_eq!(short_variants[1].variant, "");
    }

    #[test]
    fn an_unknown_layout_is_named_by_its_code_not_a_guess() {
        let layout = KeyboardLayout::new("xx", "");
        assert_eq!(layout.full_name(Language::ZhTw), "xx");
        let variant = KeyboardLayout::new("us", "workman");
        assert_eq!(variant.full_name(Language::EnUs), "us(workman)");
        assert!(KeyboardLayout::from_locale1("", "").is_empty());
    }
}
