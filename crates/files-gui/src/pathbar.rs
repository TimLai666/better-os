//! The address bar's breadcrumbs, decided without a window.
//!
//! A folder under the home folder reads from Home (`Home › Documents ›
//! reports`), the way folio's `PathBar.qml` and File Explorer start from a
//! named place rather than from `/`; anything else reads from the root. Trash
//! and Applications are one crumb each. Clicking a crumb opens it; clicking
//! the empty part of the bar, or `Ctrl+L`, turns it into a text field.

use std::path::{Path, PathBuf};

use files_core::{LocalPath, Location, TrashLocation};

use crate::i18n::Copy;

/// One crumb: what it says and where it goes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Crumb {
    pub label: String,
    pub location: Location,
}

/// The crumbs for `location`, first to last. `home` is the person's home
/// folder, when there is one.
pub fn crumbs(location: &Location, home: Option<&Path>, c: &'static Copy) -> Vec<Crumb> {
    crumbs_named(
        location,
        home,
        c,
        &files_platform::UserDirectories::default(),
    )
}

/// The crumbs, with the person's own folders (Documents, Pictures, …)
/// named as the sidebar names them, in the window's language: 文件, not
/// the folder's name on disk.
pub fn crumbs_named(
    location: &Location,
    home: Option<&Path>,
    c: &'static Copy,
    directories: &files_platform::UserDirectories,
) -> Vec<Crumb> {
    match location {
        Location::Local(path) => local_crumbs(path.as_path(), home, c, directories),
        Location::Trash(_) => vec![Crumb {
            label: c.place_trash.to_string(),
            location: Location::Trash(TrashLocation::Root),
        }],
        Location::Recent => vec![Crumb {
            label: c.place_recent.to_string(),
            location: Location::Recent,
        }],
        Location::Applications => vec![Crumb {
            label: c.sidebar_applications.to_string(),
            location: Location::Applications,
        }],
        other => vec![Crumb {
            label: crate::toolbar::display_path(other),
            location: other.clone(),
        }],
    }
}

fn local_crumbs(
    path: &Path,
    home: Option<&Path>,
    c: &'static Copy,
    directories: &files_platform::UserDirectories,
) -> Vec<Crumb> {
    let named = |at: &Path| {
        directories.sidebar().into_iter().find_map(|resolved| {
            (resolved.location.as_local_path()?.as_path() == at)
                .then(|| crate::i18n::user_directory_label(resolved.directory, c).to_string())
        })
    };
    let mut out = Vec::new();
    let (start, rest): (PathBuf, &Path) = match home.and_then(|home| {
        path.strip_prefix(home)
            .ok()
            .map(|rest| (home.to_path_buf(), rest))
    }) {
        Some((home, rest)) => {
            out.push(Crumb {
                label: c.place_home.to_string(),
                location: local(&home),
            });
            (home, rest)
        }
        None => {
            out.push(Crumb {
                label: "/".to_string(),
                location: Location::Local(LocalPath::root()),
            });
            (PathBuf::from("/"), path.strip_prefix("/").unwrap_or(path))
        }
    };
    let mut at = start;
    for part in rest.components() {
        at.push(part);
        out.push(Crumb {
            label: named(&at).unwrap_or_else(|| part.as_os_str().to_string_lossy().into_owned()),
            location: local(&at),
        });
    }
    out
}

fn local(path: &Path) -> Location {
    LocalPath::new(path.to_path_buf())
        .map(Location::Local)
        .unwrap_or_else(|_| Location::Local(LocalPath::root()))
}

/// How many crumbs to draw before the oldest fold into `…`, for a bar this
/// wide. A crumb is counted at its text plus the chevron after it, which is
/// what the bar draws.
pub fn visible_crumbs(crumbs: &[Crumb], width: f32) -> usize {
    const CHEVRON: f32 = 28.0;
    const FOLD: f32 = 40.0;
    let widths: Vec<f32> = crumbs
        .iter()
        .map(|crumb| crate::layout::label_width(&crumb.label) + CHEVRON)
        .collect();
    let total: f32 = widths.iter().sum();
    if total <= width {
        return crumbs.len();
    }
    // Keep the last crumbs, as many as fit beside the fold, and never fewer
    // than one: the current folder is always shown.
    let mut used = FOLD;
    let mut count = 0;
    for width_of in widths.iter().rev() {
        if used + width_of > width && count > 0 {
            break;
        }
        used += width_of;
        count += 1;
    }
    // `BreadcrumbBar::max_visible` counts the fold as one item, and folds
    // only when there are more crumbs than that: "Home › Documents" in a
    // narrow bar has to fold to "… › Documents", not show both and clip.
    (count + 1).min(crumbs.len() - 1)
}

/// Tab in the path field: completes the last part of a typed path to the
/// folder it names, as folio's path bar does (`plans/folio.md:74`). One
/// match completes fully and adds the `/`; several complete as far as they
/// agree. `~` is the home folder. A name is matched with case first, then
/// without. `None` when nothing matches or the text is not a local path.
pub fn complete_path(text: &str, home: Option<&Path>) -> Option<String> {
    let expanded = match (text.strip_prefix('~'), home) {
        (Some(rest), Some(home)) => format!("{}{rest}", home.display()),
        _ => text.to_string(),
    };
    if !expanded.starts_with('/') {
        return None;
    }
    let split = expanded.rfind('/')?;
    let (folder, prefix) = (&expanded[..=split], &expanded[split + 1..]);
    let mut names: Vec<String> = std::fs::read_dir(folder)
        .ok()?
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        // Hidden folders only when the typed part asks for them.
        .filter(|name| !name.starts_with('.') || prefix.starts_with('.'))
        .collect();
    names.sort();
    let mut matches: Vec<&String> = names
        .iter()
        .filter(|name| name.starts_with(prefix))
        .collect();
    if matches.is_empty() {
        let lower = prefix.to_lowercase();
        matches = names
            .iter()
            .filter(|name| name.to_lowercase().starts_with(&lower))
            .collect();
    }
    let completed = match matches.as_slice() {
        [] => return None,
        [only] => format!("{folder}{only}/"),
        several => {
            let first = several[0];
            let mut common = first.len();
            for other in &several[1..] {
                common = first
                    .char_indices()
                    .zip(other.chars())
                    .take_while(|((_, a), b)| a == b)
                    .last()
                    .map(|((at, a), _)| at + a.len_utf8())
                    .unwrap_or(0)
                    .min(common);
            }
            if common <= prefix.len() {
                return None;
            }
            format!("{folder}{}", &first[..common])
        }
    };
    (completed != expanded).then_some(completed)
}
