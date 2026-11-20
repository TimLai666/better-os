//! What a provider's reason key says, in words a person can read.
//!
//! The service reports why a reading is unavailable as a stable key
//! (`awake.provider.no_battery`), never a sentence, so the wire stays
//! language-free. Every window that shows one — the Better Awake window and
//! Better Manager's Keep awake page — turns it into a sentence here, from one
//! table. A key may carry a detail after a colon
//! (`awake.provider.path_unwatchable:Permission denied`); the detail follows the
//! sentence in parentheses. A key the table does not know is shown as
//! "Unknown reason" with the key in parentheses, never as the key alone.

/// The languages the windows ship.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Language {
    English,
    TraditionalChinese,
}

/// Every provider reason: the key, the English sentence, and the Taiwan
/// Mandarin sentence.
pub const PROVIDER_REASONS: &[(&str, &str, &str)] = &[
    (
        "awake.provider.not_sampled",
        "This reading has not been taken yet.",
        "還沒有讀取過這項資料。",
    ),
    (
        "awake.provider.awaiting_second_sample",
        "This needs two readings a few seconds apart, and only one has been taken so far.",
        "這項資料需要相隔幾秒讀取兩次，目前只讀到一次。",
    ),
    (
        "awake.provider.counters_did_not_advance",
        "The system's counters did not change between two readings, so no rate can be worked out.",
        "系統計數器在兩次讀取之間沒有變化，所以算不出速率。",
    ),
    (
        "awake.provider.malformed_stat",
        "The system's processor statistics could not be understood.",
        "看不懂系統的處理器統計資料。",
    ),
    (
        "awake.provider.interface_missing",
        "The system file this reading comes from does not exist on this computer.",
        "這台電腦上沒有提供這項資料的系統檔案。",
    ),
    (
        "awake.provider.permission_denied",
        "Better Awake is not allowed to read the system file this reading comes from.",
        "Better Awake 沒有權限讀取提供這項資料的系統檔案。",
    ),
    (
        "awake.provider.read_failed",
        "The system file this reading comes from could not be read.",
        "無法讀取提供這項資料的系統檔案。",
    ),
    (
        "awake.provider.no_battery",
        "This computer has no battery.",
        "這台電腦沒有電池。",
    ),
    (
        "awake.provider.no_mains_supply",
        "This computer reports no power adapter, so whether it is plugged in cannot be told.",
        "這台電腦沒有回報電源變壓器，所以無法判斷是否接上電源。",
    ),
    (
        "awake.provider.no_power_supply",
        "This computer reports no battery and no power adapter.",
        "這台電腦沒有回報電池，也沒有回報電源變壓器。",
    ),
    (
        "awake.provider.wrong_provider",
        "Better Awake asked the wrong source for this reading. This is a fault in Better Awake.",
        "Better Awake 向錯誤的來源要求這項資料，這是 Better Awake 本身的問題。",
    ),
    (
        "awake.provider.no_drm_connectors",
        "The graphics driver lists no display connectors, so an external display cannot be detected.",
        "顯示卡驅動程式沒有列出任何顯示連接埠，所以偵測不到外接螢幕。",
    ),
    (
        "awake.provider.no_alsa_playback_device",
        "No sound playback device was found. Better Awake reads sound through ALSA, so Bluetooth and network speakers are not seen.",
        "找不到播放聲音的裝置。Better Awake 是透過 ALSA 讀取聲音狀態，看不到藍牙和網路喇叭。",
    ),
    (
        "awake.provider.fullscreen_needs_compositor_adapter",
        "Only the desktop itself knows whether a window is full screen, and the part that would ask it is not built yet.",
        "只有桌面本身知道視窗是否為全螢幕，而向桌面詢問的元件還沒有做好。",
    ),
    (
        "awake.provider.clock_out_of_range",
        "The system clock reads a time outside what Better Awake can handle.",
        "系統時鐘的時間超出 Better Awake 能處理的範圍。",
    ),
    (
        "awake.provider.no_time_zone",
        "This computer has no time zone set, so the local time a schedule needs is not known.",
        "這台電腦沒有設定時區，所以不知道排程需要的當地時間。",
    ),
    (
        "awake.provider.path_unwatchable",
        "This file or folder cannot be watched for changes.",
        "無法監看這個檔案或資料夾的變動。",
    ),
    (
        "awake.provider.watch_unavailable",
        "Watching files for changes is not available on this computer.",
        "這台電腦無法監看檔案變動。",
    ),
];

/// The sentence for a provider reason key, in `language`.
pub fn provider_reason(key: &str, language: Language) -> String {
    let key = key.trim();
    let (base, detail) = match key.split_once(':') {
        Some((base, detail)) => (base, Some(detail.trim()).filter(|d| !d.is_empty())),
        None => (key, None),
    };
    let sentence = PROVIDER_REASONS
        .iter()
        .find(|(known, _, _)| *known == base)
        .map(|(_, english, chinese)| match language {
            Language::English => *english,
            Language::TraditionalChinese => *chinese,
        });
    match (sentence, detail, language) {
        (Some(sentence), None, _) => sentence.to_string(),
        (Some(sentence), Some(detail), Language::English) => format!("{sentence} ({detail})"),
        (Some(sentence), Some(detail), Language::TraditionalChinese) => {
            format!("{sentence}（{detail}）")
        }
        (None, _, Language::English) => format!("Unknown reason ({key})"),
        (None, _, Language::TraditionalChinese) => format!("原因不明（{key}）"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use std::path::Path;

    /// Every `awake.provider.*` key written in a crate's sources.
    fn keys_in(directory: &Path, into: &mut BTreeSet<String>) {
        for entry in std::fs::read_dir(directory).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                keys_in(&path, into);
                continue;
            }
            if path.extension().is_none_or(|extension| extension != "rs") {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap();
            for (at, _) in text.match_indices("\"awake.provider.") {
                let rest = &text[at + 1..];
                let end = rest
                    .find(|c: char| !(c.is_ascii_lowercase() || c == '_' || c == '.'))
                    .unwrap_or(rest.len());
                into.insert(rest[..end].trim_end_matches('.').to_string());
            }
        }
    }

    fn emitted() -> BTreeSet<String> {
        let crates = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let mut keys = BTreeSet::new();
        keys_in(&crates.join("awake-platform/src"), &mut keys);
        keys_in(&crates.join("awake-core/src"), &mut keys);
        keys
    }

    #[test]
    fn every_key_a_provider_can_emit_has_a_sentence_in_both_languages() {
        let keys = emitted();
        for required in [
            "awake.provider.no_time_zone",
            "awake.provider.fullscreen_needs_compositor_adapter",
            "awake.provider.no_alsa_playback_device",
            "awake.provider.path_unwatchable",
        ] {
            assert!(keys.contains(required), "the scan missed {required}");
        }
        for key in &keys {
            let row = PROVIDER_REASONS.iter().find(|(known, _, _)| known == key);
            let (_, english, chinese) = row.unwrap_or_else(|| panic!("no sentence for {key}"));
            assert!(
                english.ends_with('.') && !english.contains("awake."),
                "{key}"
            );
            assert!(
                chinese.ends_with('。') && !chinese.contains("awake."),
                "{key}"
            );
            assert!(!provider_reason(key, Language::English).contains("awake."));
            assert!(!provider_reason(key, Language::TraditionalChinese).contains("awake."));
        }
        for (key, _, _) in PROVIDER_REASONS {
            assert!(
                keys.contains(*key),
                "{key} has a sentence but nothing emits it"
            );
        }
    }

    #[test]
    fn a_detail_follows_the_sentence_in_parentheses() {
        assert_eq!(
            provider_reason(
                "awake.provider.interface_missing:/sys/class/drm",
                Language::English
            ),
            "The system file this reading comes from does not exist on this computer. (/sys/class/drm)"
        );
        assert_eq!(
            provider_reason(
                "awake.provider.interface_missing:/sys/class/drm",
                Language::TraditionalChinese
            ),
            "這台電腦上沒有提供這項資料的系統檔案。（/sys/class/drm）"
        );
        assert_eq!(
            provider_reason("awake.provider.no_battery", Language::English),
            "This computer has no battery."
        );
    }

    #[test]
    fn an_unknown_key_says_so_and_names_the_key_never_shows_it_alone() {
        assert_eq!(
            provider_reason("awake.provider.something_new", Language::English),
            "Unknown reason (awake.provider.something_new)"
        );
        assert_eq!(
            provider_reason("awake.provider.something_new", Language::TraditionalChinese),
            "原因不明（awake.provider.something_new）"
        );
    }
}
