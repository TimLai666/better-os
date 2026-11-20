//! What the application catalog's keys say, in words (ticket 172).
//!
//! An entry that is refused, a launch that cannot be built, and a start that
//! fails are reported as stable keys (`catalog.launch.error.relative_path`),
//! never sentences. Every window that shows one (Better Launcher, the
//! application chooser, Better Files' applications) says it from this table.
//! A key may carry a detail after a colon; the detail is the window's to show
//! or leave out, the key never is.

/// The languages the windows ship.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Language {
    English,
    TraditionalChinese,
}

/// Every key: the key, the English sentence, and the Taiwan Mandarin one.
pub const REASONS: &[(&str, &str, &str)] = &[
    (
        "catalog.error.invalid_desktop_id",
        "The application's file name isn't a valid application name.",
        "應用程式的檔名不是有效的應用程式名稱。",
    ),
    (
        "catalog.error.entry_too_large",
        "The application's description file is larger than Better OS reads.",
        "應用程式的描述檔超過 Better OS 會讀取的大小。",
    ),
    (
        "catalog.error.invalid_encoding",
        "The application's description file isn't text Better OS can read.",
        "應用程式的描述檔不是 Better OS 讀得懂的文字。",
    ),
    (
        "catalog.error.unreadable",
        "The application's description file couldn't be read.",
        "無法讀取應用程式的描述檔。",
    ),
    (
        "catalog.error.content_before_group",
        "The application's description file is not laid out correctly.",
        "應用程式的描述檔格式不正確。",
    ),
    (
        "catalog.error.invalid_group_header",
        "The application's description file is not laid out correctly.",
        "應用程式的描述檔格式不正確。",
    ),
    (
        "catalog.error.duplicate_group",
        "The application's description file repeats a section.",
        "應用程式的描述檔重複了某個段落。",
    ),
    (
        "catalog.error.invalid_line",
        "The application's description file has a line Better OS can't read.",
        "應用程式的描述檔有一行 Better OS 讀不懂。",
    ),
    (
        "catalog.error.invalid_key",
        "The application's description file has a setting name Better OS can't read.",
        "應用程式的描述檔有個 Better OS 讀不懂的設定名稱。",
    ),
    (
        "catalog.error.duplicate_key",
        "The application's description file sets the same thing twice.",
        "應用程式的描述檔把同一項設定寫了兩次。",
    ),
    (
        "catalog.error.value_too_long",
        "A setting in the application's description file is too long.",
        "應用程式描述檔裡有項設定太長了。",
    ),
    (
        "catalog.error.missing_desktop_entry_group",
        "The application's description file has no application section.",
        "應用程式的描述檔沒有應用程式段落。",
    ),
    (
        "catalog.error.missing_field",
        "The application's description file leaves out something it needs.",
        "應用程式的描述檔缺少必要的內容。",
    ),
    (
        "catalog.error.unsupported_type",
        "The description file isn't for an application.",
        "這個描述檔不是應用程式的。",
    ),
    (
        "catalog.error.invalid_boolean",
        "A setting in the application's description file must be true or false.",
        "應用程式描述檔裡有項設定必須是 true 或 false。",
    ),
    (
        "catalog.error.control_character",
        "The application's description file contains a character that can't be used.",
        "應用程式的描述檔含有不能使用的字元。",
    ),
    (
        "catalog.error.too_many_actions",
        "The application lists more actions than Better OS shows.",
        "應用程式列出的動作超過 Better OS 會顯示的數量。",
    ),
    (
        "catalog.error.invalid_action_id",
        "One of the application's actions has a name that can't be used.",
        "應用程式有個動作的名稱不能使用。",
    ),
    (
        "catalog.error.missing_action_group",
        "The application names an action it doesn't describe.",
        "應用程式列出了一個沒有說明的動作。",
    ),
    (
        "catalog.error.duplicate_action_id",
        "The application lists the same action twice.",
        "應用程式把同一個動作列了兩次。",
    ),
    (
        "catalog.error.exec_empty",
        "The application's command is empty.",
        "應用程式的指令是空的。",
    ),
    (
        "catalog.error.exec_unterminated_quote",
        "The application's command has a quote that is never closed.",
        "應用程式的指令有個沒有結束的引號。",
    ),
    (
        "catalog.error.exec_trailing_escape",
        "The application's command ends in the middle of an escape.",
        "應用程式的指令在跳脫字元中間就結束了。",
    ),
    (
        "catalog.error.exec_unknown_field_code",
        "The application's command uses a placeholder Better OS doesn't know.",
        "應用程式的指令用了 Better OS 不認得的替代符號。",
    ),
    (
        "catalog.error.exec_multiple_target_field_codes",
        "The application's command names its files in more than one way.",
        "應用程式的指令用了不只一種方式指定檔案。",
    ),
    (
        "catalog.error.exec_field_code_placement",
        "The application's command puts a placeholder where it can't be used.",
        "應用程式的指令把替代符號放在不能用的位置。",
    ),
    (
        "catalog.launch.error.targets_not_supported",
        "This application can't open that many files at once.",
        "這個應用程式無法一次開啟這麼多檔案。",
    ),
    (
        "catalog.launch.error.relative_path",
        "The file to open must be named by its full path.",
        "要開啟的檔案必須用完整路徑指定。",
    ),
    (
        "catalog.launch.error.non_utf8_path",
        "The file's name can't be handed to this application.",
        "這個檔案的名稱無法交給這個應用程式。",
    ),
    (
        "catalog.launch.error.embedded_nul",
        "The file's name contains a character that can't be handed to an application.",
        "這個檔案的名稱含有無法交給應用程式的字元。",
    ),
    (
        "catalog.launch.error.target_too_long",
        "The file's name is too long to hand to this application.",
        "這個檔案的名稱太長，無法交給這個應用程式。",
    ),
    (
        "catalog.launch.error.invalid_uri",
        "The address to open isn't valid.",
        "要開啟的位址無效。",
    ),
    (
        "catalog.launch.error.uri_not_a_local_file",
        "This application opens only files on this computer.",
        "這個應用程式只能開啟這台電腦上的檔案。",
    ),
    (
        "catalog.launch.error.unknown_action",
        "The application no longer has that action.",
        "這個應用程式已經沒有那個動作了。",
    ),
    (
        "catalog.launch.error.no_launch_definition",
        "The application doesn't say how to start it.",
        "這個應用程式沒有說明要怎麼啟動。",
    ),
    (
        "catalog.platform.error.spawn_failed",
        "The application's program couldn't be started.",
        "無法啟動應用程式的程式。",
    ),
    (
        "catalog.platform.error.watch_failed",
        "Better OS can't watch for applications being installed or removed, so the list won't update by itself.",
        "Better OS 無法監看應用程式的安裝或移除，所以清單不會自動更新。",
    ),
    (
        "catalog.platform.error.activation_failed",
        "The application didn't answer when it was asked to start.",
        "要求應用程式啟動時，它沒有回應。",
    ),
];

/// The sentence for `key` (or `key:detail`), if it is one of the catalog's.
pub fn sentence(key: &str, language: Language) -> Option<&'static str> {
    let base = key.trim().split(':').next().unwrap_or_default();
    REASONS.iter().find(|(known, _, _)| *known == base).map(
        |(_, english, chinese)| match language {
            Language::English => *english,
            Language::TraditionalChinese => *chinese,
        },
    )
}

/// What follows a key: an application's id, a program, a count. Empty for a
/// bare key.
pub fn detail(key: &str) -> &str {
    key.trim()
        .split_once(':')
        .map(|(_, rest)| rest.trim())
        .unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::{Language, REASONS, detail, sentence};
    use std::collections::BTreeSet;
    use std::path::Path;

    fn production(text: &str) -> String {
        let lines: Vec<&str> = text.lines().collect();
        let mut out = String::new();
        for (index, line) in lines.iter().enumerate() {
            if line.trim() == "#[cfg(test)]"
                && lines
                    .get(index + 1)
                    .is_some_and(|next| next.trim_start().starts_with("mod "))
            {
                break;
            }
            if !line.trim_start().starts_with("//") {
                out.push_str(line);
                out.push('\n');
            }
        }
        out
    }

    fn keys_in(directory: &Path, into: &mut BTreeSet<String>) {
        for entry in std::fs::read_dir(directory).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                keys_in(&path, into);
                continue;
            }
            // The table is the answer, not the question.
            if path.extension().is_none_or(|e| e != "rs")
                || path.ends_with("app-catalog-core/src/reasons.rs")
            {
                continue;
            }
            let text = production(&std::fs::read_to_string(&path).unwrap());
            for (at, _) in text.match_indices("\"catalog.") {
                let rest = &text[at + 1..];
                let end = rest
                    .find(|c: char| {
                        !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '.')
                    })
                    .unwrap_or(rest.len());
                let key = rest[..end].trim_end_matches('.');
                if key.matches('.').count() >= 2 {
                    into.insert(key.to_string());
                }
            }
        }
    }

    #[test]
    fn every_key_the_catalog_crates_emit_has_a_sentence_in_both_languages() {
        let crates = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let mut keys = BTreeSet::new();
        keys_in(&crates.join("app-catalog-core/src"), &mut keys);
        keys_in(&crates.join("app-catalog-platform/src"), &mut keys);
        for required in [
            "catalog.launch.error.relative_path",
            "catalog.platform.error.spawn_failed",
            "catalog.error.exec_empty",
        ] {
            assert!(keys.contains(required), "the scan missed {required}");
        }
        for key in &keys {
            for language in [Language::English, Language::TraditionalChinese] {
                let said = sentence(&format!("{key}:x"), language)
                    .unwrap_or_else(|| panic!("no sentence for {key}"));
                assert!(!said.contains("catalog."), "{key}: {said}");
            }
        }
        for (key, english, chinese) in REASONS {
            assert!(
                keys.contains(*key),
                "{key} has a sentence but nothing emits it"
            );
            assert!(english.ends_with('.') && chinese.ends_with('。'), "{key}");
        }
    }

    #[test]
    fn a_detail_is_what_follows_the_key() {
        assert_eq!(
            detail("catalog.platform.error.spawn_failed:foo:not found"),
            "foo:not found"
        );
        assert_eq!(detail("catalog.launch.error.relative_path"), "");
        assert_eq!(
            sentence("catalog.nothing.like_this", Language::English),
            None
        );
    }
}
