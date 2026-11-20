//! Ticket 147's two checks on the windows themselves, by reading their
//! source: every picture a window names is drawn and served, and no window
//! names a toolkit line icon for something that is a thing or an action.
//!
//! The windows are every `*-gui` crate and the Fluent gallery beside this
//! one, plus this crate's own controls. Nothing here is a list of call
//! sites: the names come from the source each time the test runs.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};

use better_ui::icon_set::{Picture, load};

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

/// Functions that feed gpui-component's `SidebarMenuItem`, which takes only
/// a single-colour toolkit `Icon`: a picture there needs the window's
/// navigation moved to the kit's `NavigationView`. Recorded in ticket 147.
const SIDEBAR_FEEDS: &[(&str, &str)] = &[
    ("awake-gui/src/shell.rs", "fn section_icon("),
    ("touchpad-gui/src/pages.rs", "fn page_icon("),
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

/// Every picture a window names exists in the set and is served, both its
/// layers; and every window that names a picture serves the set from its
/// own `AssetSource`, or no picture it names would load.
#[test]
fn every_picture_a_window_uses_is_drawn_and_served() {
    let by_name: BTreeMap<&str, Picture> = Picture::ALL.iter().map(|p| (p.name(), *p)).collect();
    let sources = sources();
    let mut used = HashSet::new();
    let mut missing = Vec::new();
    let mut crates_using = BTreeSet::new();
    for (file, text) in &sources {
        for (name, line) in names_after(text, "Picture::") {
            // `Picture::ALL` and the type's own methods are not pictures.
            if name == "ALL" || !name.starts_with(|c: char| c.is_ascii_uppercase()) {
                continue;
            }
            match by_name.get(name.as_str()) {
                Some(picture) => {
                    used.insert(*picture);
                    crates_using.insert(file.split('/').next().unwrap().to_string());
                }
                None => missing.push(format!("{file}:{line} names Picture::{name}")),
            }
        }
    }
    assert!(missing.is_empty(), "no drawing: {missing:#?}");
    assert!(
        used.len() > 50,
        "the scan found only {} pictures",
        used.len()
    );
    for picture in &used {
        assert!(
            load(picture.path()).is_some(),
            "{} is not served",
            picture.name()
        );
        if let Some((path, _)) = picture.second_layer() {
            assert!(load(path).is_some(), "{path} is not served");
        }
    }
    // A window crate that names a picture must hand GPUI the set.
    for window in crates_using
        .iter()
        .filter(|name| name.as_str() != "better-ui")
    {
        let serves = sources
            .iter()
            .filter(|(file, _)| file.starts_with(&format!("{window}/")))
            .any(|(_, text)| text.contains("icon_set::load("));
        assert!(
            serves,
            "{window} draws pictures but never serves icon_set::load"
        );
    }
}

/// No window names a toolkit line icon for a thing or an action: every
/// `IconName` left is a plain symbol or a status mark, apart from the
/// recorded sidebars that cannot take a picture.
#[test]
fn no_window_names_a_toolkit_icon_for_a_thing_or_an_action() {
    let symbols: BTreeSet<&str> = SYMBOLS.iter().map(|(name, _)| *name).collect();
    let mut offences = Vec::new();
    let mut seen = 0;
    for (file, text) in sources() {
        if FIXTURES.contains(&file.as_str()) {
            continue;
        }
        let sidebar = SIDEBAR_FEEDS
            .iter()
            .find(|(path, _)| *path == file)
            .map(|(_, function)| function_lines(&text, function));
        for (name, line) in names_after(&text, "IconName::") {
            seen += 1;
            if symbols.contains(name.as_str()) {
                continue;
            }
            if sidebar.as_ref().is_some_and(|lines| lines.contains(&line)) {
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

/// The lines of the function that starts with `signature`, up to its
/// closing brace at the start of a line.
fn function_lines(text: &str, signature: &str) -> std::ops::RangeInclusive<usize> {
    let lines: Vec<&str> = text.lines().collect();
    let start = lines
        .iter()
        .position(|line| line.contains(signature))
        .unwrap_or_else(|| panic!("{signature} is gone; update SIDEBAR_FEEDS"));
    let end = lines[start..]
        .iter()
        .position(|line| *line == "}")
        .map(|offset| start + offset)
        .unwrap();
    (start + 1)..=(end + 1)
}

#[test]
fn the_scanner_reads_names_after_a_prefix() {
    let found = names_after(
        "a(IconName::Close, Picture::Copy) IconName::foo",
        "IconName::",
    );
    assert_eq!(found, vec![("Close".to_string(), 1)]);
    assert_eq!(function_lines("x\nfn f() {\n  a\n}\ny", "fn f("), 2..=4);
}
