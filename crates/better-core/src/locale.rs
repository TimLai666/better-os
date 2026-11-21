//! The one rule for which language a locale tag gets in a message Better OS
//! sends outside its windows (a desktop notification): Traditional Chinese
//! for Taiwan, Hong Kong, and any `zh_Hant` tag, English otherwise. Better
//! Awake's notifications and the USB-C recovery's both ask here.

/// Whether a locale tag (`zh_TW.UTF-8`, `zh-Hant`, `zh_HK`) is shown in
/// Traditional Chinese.
pub fn is_traditional_chinese(tag: &str) -> bool {
    let tag = tag.trim().replace('-', "_").to_ascii_lowercase();
    tag.starts_with("zh_tw") || tag.starts_with("zh_hant") || tag.starts_with("zh_hk")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn taiwan_hong_kong_and_hant_are_traditional_chinese() {
        for tag in [
            "zh_TW.UTF-8",
            "zh-TW",
            "zh_Hant",
            "zh-Hant-TW",
            "zh_HK.UTF-8",
            " zh_tw ",
        ] {
            assert!(is_traditional_chinese(tag), "{tag}");
        }
        for tag in ["zh_CN.UTF-8", "en_US.UTF-8", "", "C"] {
            assert!(!is_traditional_chinese(tag), "{tag}");
        }
    }
}
