//! Ticket 172: every `gestures.*` key the touchpad crates can put in front
//! of a person reads as its own sentence in both languages, in Better
//! Manager's Touchpad page and in the standalone window alike, and never as
//! the key or the catch-all "That didn't work".

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use touchpad_views::i18n::{Locale, problem_detail, problem_sentence};

/// File names in the store's folder, not messages.
const NOT_MESSAGES: &[&str] = &["gestures.json", "gestures.lock"];

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
        if line.trim_start().starts_with("//") {
            continue;
        }
        out.push_str(line);
        out.push('\n');
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
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        // The table is the answer, not the question.
        if path.extension().is_none_or(|e| e != "rs")
            || stem == "tests"
            || stem.ends_with("_tests")
            || path.ends_with("touchpad-views/src/i18n.rs")
        {
            continue;
        }
        let text = production(&std::fs::read_to_string(&path).unwrap());
        for (at, _) in text.match_indices("\"gestures.") {
            let rest = &text[at + 1..];
            let end = rest
                .find(|c: char| {
                    !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '.')
                })
                .unwrap_or(rest.len());
            let key = rest[..end].trim_end_matches('.');
            if !NOT_MESSAGES.contains(&key) {
                into.insert(key.to_string());
            }
        }
    }
}

#[test]
fn every_key_the_touchpad_crates_emit_has_its_own_sentence_in_both_languages() {
    let crates = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut keys = BTreeSet::new();
    for member in [
        "touchpad-gestures",
        "touchpad-views",
        "touchpad-gui",
        "touchpad-platform",
    ] {
        keys_in(&crates.join(member).join("src"), &mut keys);
    }
    for required in [
        "gestures.export.no_path",
        "gestures.import.no_such_file",
        "gestures.contacts_out_of_range",
    ] {
        assert!(keys.contains(required), "the scan missed {required}");
    }
    let fallback = [
        problem_sentence("not.a.key", Locale::EnUs),
        problem_sentence("not.a.key", Locale::ZhTw),
    ];
    for key in &keys {
        for locale in [Locale::EnUs, Locale::ZhTw] {
            let said = problem_sentence(&format!("{key}:/home/a/gestures.json"), locale);
            assert!(
                !fallback.contains(&said),
                "{key} has no sentence of its own"
            );
            assert!(!said.contains("gestures."), "{key}: {said}");
        }
    }
}

#[test]
fn the_details_of_a_problem_are_what_follows_its_key() {
    assert_eq!(
        problem_detail("gestures.import.no_such_file:/home/a/x.json"),
        Some("/home/a/x.json")
    );
    assert_eq!(problem_detail("gestures.export.no_path"), None);
    assert_eq!(
        problem_detail("Permission denied (os error 13)"),
        Some("Permission denied (os error 13)")
    );
}
