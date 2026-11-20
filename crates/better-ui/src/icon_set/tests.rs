//! Checks every drawing against DESIGN.md's Icons rules and against what
//! GPUI's SVG renderer (usvg) draws.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::Path;

use super::catalog::{ASSET_PREFIX, Kind, Picture, load};

/// One element of a parsed SVG: its name and attributes.
struct Element {
    name: String,
    attributes: BTreeMap<String, String>,
}

/// Parses the small XML subset the icon files use (elements, double-quoted
/// attributes, no text, no comments, no entities) and fails on anything
/// that is not well formed: an unclosed or mismatched tag, an attribute
/// without a quoted value, a repeated attribute, text outside a tag.
fn parse(svg: &str) -> Result<Vec<Element>, String> {
    let mut elements = Vec::new();
    let mut open: Vec<String> = Vec::new();
    let mut rest = svg.trim_end();
    while !rest.is_empty() {
        let Some(after) = rest.strip_prefix('<') else {
            return Err(format!("text outside a tag: {:.20}", rest));
        };
        let end = after.find('>').ok_or("a tag is never closed")?;
        let tag = &after[..end];
        rest = &after[end + 1..];
        if let Some(name) = tag.strip_prefix('/') {
            match open.pop() {
                Some(top) if top == name.trim() => continue,
                other => return Err(format!("</{name}> closes {other:?}")),
            }
        }
        if tag.starts_with('!') || tag.starts_with('?') {
            return Err(format!("a declaration or comment: <{tag}>"));
        }
        let (tag, self_closing) = match tag.strip_suffix('/') {
            Some(tag) => (tag, true),
            None => (tag, false),
        };
        let name_end = tag.find(char::is_whitespace).unwrap_or(tag.len());
        let name = tag[..name_end].to_string();
        if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric()) {
            return Err(format!("a bad element name in <{tag}>"));
        }
        let mut attributes = BTreeMap::new();
        let mut text = tag[name_end..].trim_start();
        while !text.is_empty() {
            let eq = text
                .find('=')
                .ok_or(format!("an attribute without a value in <{tag}>"))?;
            let key = text[..eq].trim().to_string();
            let value = text[eq + 1..]
                .strip_prefix('"')
                .ok_or(format!("an unquoted value in <{tag}>"))?;
            let close = value
                .find('"')
                .ok_or(format!("an unclosed value in <{tag}>"))?;
            if key.is_empty()
                || attributes
                    .insert(key.clone(), value[..close].to_string())
                    .is_some()
            {
                return Err(format!("a bad or repeated attribute {key:?} in <{tag}>"));
            }
            text = value[close + 1..].trim_start();
        }
        if !self_closing {
            open.push(name.clone());
        }
        elements.push(Element { name, attributes });
    }
    if let Some(unclosed) = open.pop() {
        return Err(format!("<{unclosed}> is never closed"));
    }
    Ok(elements)
}

fn elements(picture: Picture) -> Vec<Element> {
    parse(picture.svg()).unwrap_or_else(|e| panic!("{}: {e}", picture.name()))
}

/// Every file in the set: each picture's main file, and each action's
/// second layer, as (asset path, SVG).
fn files() -> Vec<(&'static str, &'static str)> {
    Picture::ALL
        .iter()
        .flat_map(|p| std::iter::once((p.path(), p.svg())).chain(p.second_layer()))
        .collect()
}

/// A file's name without `.svg`, with the second layer's `.2` as `-2`: what
/// its ids start with.
fn stem(path: &str) -> String {
    path.rsplit('/')
        .next()
        .unwrap()
        .trim_end_matches(".svg")
        .replace('.', "-")
}

/// A colour written as a literal: not `none`, and not a gradient reference.
fn literal(value: Option<&String>) -> Option<String> {
    let value = value?.trim().to_ascii_lowercase();
    (value != "none" && !value.starts_with("url(")).then_some(value)
}

fn is_gradient(name: &str) -> bool {
    name == "linearGradient" || name == "radialGradient"
}

#[test]
fn every_file_is_a_24px_svg_whose_tag_comes_first() {
    for (name, svg) in files() {
        assert!(!svg.trim().is_empty(), "{name} is empty");
        assert!(
            svg.starts_with("<svg"),
            "{name}: something comes before <svg"
        );
        assert!(svg[..100.min(svg.len())].contains("<svg"), "{name}");
        let parsed = parse(svg).unwrap_or_else(|e| panic!("{name}: {e}"));
        let root = &parsed[0];
        assert_eq!(root.name, "svg", "{name}");
        assert_eq!(
            root.attributes.get("viewBox").map(String::as_str),
            Some("0 0 24 24"),
            "{name}"
        );
        assert_eq!(
            root.attributes.get("xmlns").map(String::as_str),
            Some("http://www.w3.org/2000/svg"),
            "{name}"
        );
    }
}

/// usvg draws none of these the way a browser would, or at all: a class
/// needs a stylesheet, `currentColor` has no text colour behind an `img`,
/// and a filter is a glow or a shadow the rules forbid anyway.
#[test]
fn no_file_uses_what_the_renderer_or_the_rules_rule_out() {
    for (name, svg) in files() {
        for forbidden in [
            "class=",
            "currentColor",
            "<filter",
            "<style",
            "<image",
            "<text",
            "<use",
        ] {
            assert!(!svg.contains(forbidden), "{name} uses {forbidden}");
        }
    }
}

#[test]
fn every_id_is_unique_and_every_reference_resolves() {
    for (name, svg) in files() {
        let parsed = parse(svg).unwrap_or_else(|e| panic!("{name}: {e}"));
        let mut ids = HashSet::new();
        for element in &parsed {
            if let Some(id) = element.attributes.get("id") {
                assert!(ids.insert(id.clone()), "{name}: id {id} twice");
                assert!(
                    id.starts_with(&stem(name)),
                    "{name}: id {id} is not named after its file"
                );
            }
        }
        for element in &parsed {
            for value in element.attributes.values() {
                if let Some(rest) = value.strip_prefix("url(#") {
                    let id = rest.trim_end_matches(')');
                    assert!(ids.contains(id), "{name}: url(#{id}) has no target");
                }
            }
        }
    }
}

/// An object icon is a filled picture: something in it has a fill, so it
/// is never a line drawing, and it uses at most four colours besides its
/// gradient stops.
#[test]
fn object_icons_are_filled_with_at_most_four_colours() {
    for &picture in Picture::ALL.iter().filter(|p| p.kind() == Kind::Object) {
        let parsed = elements(picture);
        let filled = parsed.iter().skip(1).any(|e| {
            !is_gradient(&e.name)
                && e.name != "stop"
                && e.name != "defs"
                && e.attributes.get("fill").is_some_and(|f| f != "none")
        });
        assert!(filled, "{} has no filled shape", picture.name());
        let fills: BTreeSet<String> = parsed
            .iter()
            .filter_map(|e| literal(e.attributes.get("fill")))
            .collect();
        assert!(
            fills.len() <= 4,
            "{} has {} fill colours: {fills:?}",
            picture.name(),
            fills.len()
        );
    }
}

/// An action icon is at most two masks: every layer is drawn in black, the
/// window tints it, and nothing in it has a colour of its own or a
/// gradient. White appears only inside a `<mask>`, where it means "keep".
#[test]
fn action_icons_are_masks_the_window_tints() {
    for &picture in Picture::ALL.iter().filter(|p| p.kind() == Kind::Action) {
        let layers = std::iter::once((picture.path(), picture.svg())).chain(picture.second_layer());
        for (name, svg) in layers {
            let parsed = parse(svg).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert!(
                !parsed.iter().any(|e| is_gradient(&e.name)),
                "{name} has a gradient"
            );
            let colours: BTreeSet<String> = parsed
                .iter()
                .flat_map(|e| {
                    [
                        literal(e.attributes.get("fill")),
                        literal(e.attributes.get("stroke")),
                    ]
                })
                .flatten()
                .collect();
            let allowed: &[&str] = if svg.contains("<mask") {
                &["#000", "#fff"]
            } else {
                &["#000"]
            };
            assert!(
                colours.iter().all(|c| allowed.contains(&c.as_str())),
                "{name} has colours {colours:?}"
            );
            assert!(colours.contains("#000"), "{name} draws nothing");
        }
    }
}

/// Only an action has a second layer; an object is one picture.
#[test]
fn only_actions_have_a_second_layer_and_it_is_served() {
    for &picture in Picture::ALL {
        if let Some((path, svg)) = picture.second_layer() {
            assert_eq!(picture.kind(), Kind::Action, "{}", picture.name());
            assert_eq!(path, picture.path().replace(".svg", ".2.svg"));
            assert_eq!(load(path), Some(svg.as_bytes()));
        }
    }
    assert!(Picture::Copy.second_layer().is_some());
    assert!(Picture::Refresh.second_layer().is_none());
}

/// Delete and End task destroy something and are drawn in the critical
/// colour; the rest follow where they are.
#[test]
fn only_destroying_actions_are_destructive() {
    let destructive: Vec<_> = Picture::ALL
        .iter()
        .filter(|p| p.destructive())
        .map(|p| p.name())
        .collect();
    assert_eq!(destructive, ["Delete", "EndTask"]);
}

#[test]
fn every_path_is_unique_and_load_serves_it() {
    let mut paths = HashSet::new();
    let mut names = HashSet::new();
    for &picture in Picture::ALL {
        let path = picture.path();
        assert!(paths.insert(path), "{path} twice");
        assert!(names.insert(picture.name()), "{} twice", picture.name());
        assert!(
            path.starts_with(&format!("{ASSET_PREFIX}{}/", picture.kind().directory())),
            "{path} is not in its kind's folder"
        );
        assert_eq!(load(path), Some(picture.svg().as_bytes()));
        assert_eq!(super::catalog::Picture::from_path(path), Some(picture));
    }
    assert_eq!(load("icons/folder.svg"), None);
    assert_eq!(load("better-ui/icons/object/no-such-icon.svg"), None);
    assert_eq!(load(""), None);
}

/// The enum names every drawing on disk and nothing that is not there, so
/// no file is left out of the set.
#[test]
fn every_file_on_disk_is_in_the_set() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/icons");
    for kind in [Kind::Object, Kind::Action] {
        let on_disk: BTreeSet<String> = std::fs::read_dir(root.join(kind.directory()))
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        let in_set: BTreeSet<String> = files()
            .into_iter()
            .filter(|(path, _)| path.starts_with(&format!("{ASSET_PREFIX}{}/", kind.directory())))
            .map(|(path, _)| path.rsplit('/').next().unwrap().to_string())
            .collect();
        assert_eq!(on_disk, in_set, "{kind:?}");
    }
}

#[test]
fn the_parser_refuses_what_is_not_well_formed() {
    assert!(parse(r#"<svg a="1"><g></svg>"#).is_err());
    assert!(parse(r#"<svg a=1/>"#).is_err());
    assert!(parse(r#"<svg a="1" a="2"/>"#).is_err());
    assert!(parse(r#"<!-- x --><svg/>"#).is_err());
    assert!(parse(r#"<svg>text</svg>"#).is_err());
    assert!(parse(r#"<svg><path d="M0 0"/></svg>"#).is_ok());
}
