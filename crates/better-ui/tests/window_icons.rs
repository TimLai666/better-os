//! Ticket 147's two checks on the windows themselves, by reading their
//! source: every picture a window names is drawn and served, and no window
//! names a toolkit line icon for something that is a thing or an action.
//!
//! The windows are every `*-gui` crate and the Fluent gallery beside this
//! one, plus this crate's own controls. Nothing here is a list of call
//! sites: the names come from the source each time the test runs.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// The toolkit icons a window may still name: DESIGN.md's plain symbols
/// (arrows, chevrons, close, check, plus and minus as bare marks, the window
/// buttons, the ellipsis, the spinner), the info bar's status marks, and
/// the few marks ticket 147's sorting kept on purpose, each with its reason.
const SYMBOLS: &[(&str, &str)] = &[
    ("ArrowDown", "an arrow"),
    ("ArrowLeft", "an arrow"),
    ("ArrowRight", "an arrow"),
    ("ArrowUp", "an arrow"),
    ("Check", "a check mark"),
    ("ChevronDown", "a chevron"),
    ("ChevronLeft", "a chevron"),
    ("ChevronRight", "a chevron"),
    ("ChevronUp", "a chevron"),
    ("CircleCheck", "a status mark"),
    ("CircleX", "a status mark"),
    ("Close", "close"),
    ("Ellipsis", "the ellipsis"),
    ("Info", "a status mark"),
    ("Loader", "the spinner"),
    ("Menu", "the navigation pane's toggle"),
    ("Minus", "a bare mark in a stepper"),
    ("Plus", "a bare mark in a stepper or tab strip"),
    ("SortAscending", "an arrow"),
    ("SortDescending", "an arrow"),
    ("TriangleAlert", "a status mark"),
    ("WindowClose", "a window button"),
    ("WindowMaximize", "a window button"),
    ("WindowMinimize", "a window button"),
    ("WindowRestore", "a window button"),
    ("Undo", "Undo is an arrow"),
    ("Redo", "Redo is an arrow"),
    ("ExternalLink", "a hyperlink's trailing mark"),
    ("Eye", "a text box's reveal mark"),
    ("Search", "a text box's or navigation pane's own find mark"),
    ("Maximize", "Better Git's expand-file mark"),
    ("Github", "GitHub's own mark, not redrawn"),
];

/// The kit's own tests use toolkit icons as fixtures for its controls.
const FIXTURES: &[&str] = &["better-ui/src/fluent/tests.rs"];

fn crates_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

/// Every window crate: each `*-gui` crate, the gallery, and this crate.
fn windows() -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(crates_dir())
        .unwrap()
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with("-gui") || name == "fluent-gallery" || name == "better-ui")
        .collect();
    names.sort();
    names
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(|entry| entry.ok()) {
        let path = entry.path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

/// Each `<prefix>Name` in `text`, with the line it is on.
fn names_after(text: &str, prefix: &str) -> Vec<(String, usize)> {
    let mut found = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let mut rest = line;
        while let Some(at) = rest.find(prefix) {
            let after = &rest[at + prefix.len()..];
            let name: String = after
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            if name.starts_with(|c: char| c.is_ascii_uppercase()) {
                found.push((name.clone(), index + 1));
            }
            rest = &after[name.len()..];
        }
    }
    found
}

/// The source of every window crate, by its path under `crates/`.
fn sources() -> BTreeMap<String, String> {
    let root = crates_dir();
    let mut out = BTreeMap::new();
    for window in windows() {
        let mut files = Vec::new();
        rust_files(&root.join(&window).join("src"), &mut files);
        for file in files {
            let key = file
                .strip_prefix(&root)
                .unwrap()
                .to_string_lossy()
                .into_owned();
            out.insert(key, std::fs::read_to_string(&file).unwrap());
        }
    }
    out
}

/// The text from `at` up to the call's closing parenthesis.
fn call_argument(text: &str) -> &str {
    let mut depth = 0usize;
    for (index, c) in text.char_indices() {
        match c {
            '(' => depth += 1,
            ')' if depth == 0 => return &text[..index],
            ')' => depth -= 1,
            _ => {}
        }
    }
    text
}

/// Every window that starts GPUI hands it an asset source, and every asset
/// source a window hands GPUI serves the icon set: otherwise no picture in
/// that window loads. Checked at each `application()` start and each
/// `with_assets(...)` call, binary by binary, not crate by crate: the type
/// named there must have an `AssetSource` impl in the same crate that calls
/// `icon_set::load`.
#[test]
fn every_window_hands_gpui_an_asset_source_that_serves_the_icon_set() {
    let sources = sources();
    let mut faults = Vec::new();
    let mut calls = 0;
    for (file, text) in &sources {
        let window = file.split('/').next().unwrap();
        for (at, _) in text.match_indices("gpui_platform::application()") {
            let statement = &text[at..text[at..].find(';').map_or(text.len(), |end| at + end)];
            if !statement.contains(".with_assets(") {
                faults.push(format!("{file}: starts GPUI without an asset source"));
            }
        }
        for (at, _) in text.match_indices(".with_assets(") {
            calls += 1;
            let argument = call_argument(&text[at + ".with_assets(".len()..]).trim();
            let ty = argument.rsplit("::").next().unwrap_or(argument);
            if argument.starts_with("gpui_component_assets") {
                faults.push(format!("{file}: hands GPUI the toolkit's assets alone"));
                continue;
            }
            let serves = sources
                .iter()
                .filter(|(other, _)| other.starts_with(&format!("{window}/")))
                .filter(|(_, other)| other.contains(&format!("AssetSource for {ty} ")))
                .any(|(_, other)| {
                    let impl_at = other.find(&format!("AssetSource for {ty} ")).unwrap();
                    let body = &other[impl_at..];
                    let body = &body[..body.find("\n}").unwrap_or(body.len())];
                    body.contains("icon_set::load(")
                });
            if !serves {
                faults.push(format!(
                    "{file}: hands GPUI {argument}, whose AssetSource does not serve icon_set::load"
                ));
            }
        }
    }
    assert!(calls >= 9, "the scan found only {calls} with_assets calls");
    assert!(faults.is_empty(), "{faults:#?}");
}

/// No window names a toolkit line icon for a thing or an action: every
/// `IconName` left is a plain symbol or a status mark.
#[test]
fn no_window_names_a_toolkit_icon_for_a_thing_or_an_action() {
    let symbols: BTreeSet<&str> = SYMBOLS.iter().map(|(name, _)| *name).collect();
    let mut offences = Vec::new();
    let mut seen = 0;
    for (file, text) in sources() {
        if FIXTURES.contains(&file.as_str()) {
            continue;
        }
        for (name, line) in names_after(&text, "IconName::") {
            seen += 1;
            if symbols.contains(name.as_str()) {
                continue;
            }
            offences.push(format!("{file}:{line} IconName::{name}"));
        }
    }
    assert!(seen > 50, "the scan found only {seen} toolkit icons");
    assert!(
        offences.is_empty(),
        "toolkit icons standing for a thing or an action: {offences:#?}"
    );
}

/// The same rule against the other ways to reach a toolkit icon: an alias
/// (`IconName as Glyphs`, after which `Glyphs::Folder` escapes the scan
/// above), a glob import of the toolkit (after which a bare `Folder` does),
/// and a kit SVG named by its path (`"icons/folder.svg"`).
#[test]
fn no_window_reaches_a_toolkit_icon_another_way() {
    let symbols: BTreeSet<&str> = SYMBOLS.iter().map(|(name, _)| *name).collect();
    let mut offences = Vec::new();
    for (file, text) in sources() {
        if FIXTURES.contains(&file.as_str()) {
            continue;
        }
        for (at, _) in text.match_indices("use gpui_component::") {
            let statement = &text[at..text[at..].find(';').map_or(text.len(), |end| at + end)];
            let glob = statement
                .split(|c: char| c == ',' || c == '{' || c == '}' || c.is_whitespace())
                .any(|part| part == "*" || part.ends_with("::*"));
            if glob {
                let line = text[..at].lines().count() + 1;
                offences.push(format!("{file}:{line}: glob-imports the toolkit"));
            }
        }
        for (index, line) in text.lines().enumerate() {
            let at = format!("{file}:{}", index + 1);
            let code = line.trim_start();
            if code.starts_with("//") {
                continue;
            }
            if code.contains("IconName as ") {
                offences.push(format!("{at}: renames IconName"));
            }
            let mut rest = line;
            while let Some(found) = rest.find("\"icons/") {
                let after = &rest[found + "\"icons/".len()..];
                let name: String = after
                    .chars()
                    .take_while(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '-')
                    .collect();
                let pascal: String = name
                    .split('-')
                    .map(|part| {
                        let mut chars = part.chars();
                        chars
                            .next()
                            .map(|first| first.to_ascii_uppercase().to_string() + chars.as_str())
                            .unwrap_or_default()
                    })
                    .collect();
                if after[name.len()..].starts_with(".svg") && !symbols.contains(pascal.as_str()) {
                    offences.push(format!("{at}: names the kit's icons/{name}.svg"));
                }
                rest = &after[name.len()..];
            }
        }
    }
    assert!(
        offences.is_empty(),
        "toolkit icons reached another way: {offences:#?}"
    );
}

#[test]
fn the_scanner_reads_names_after_a_prefix() {
    let found = names_after(
        "a(IconName::Close, Picture::Copy) IconName::foo",
        "IconName::",
    );
    assert_eq!(found, vec![("Close".to_string(), 1)]);
}
