//! Recents: the files the person opened lately, from the shared GTK list
//! `$XDG_DATA_HOME/recently-used.xbel`, the same one GNOME's file chooser,
//! Nautilus and folio read and write (folio: `src/Sidebar.qml`).
//!
//! Only local files that still exist are listed, newest first, each once. The
//! file is read, never written: Better Files does not add to anybody's
//! history by listing it.

use std::fs;
use std::path::{Path, PathBuf};

use files_core::entry::{
    Entry, EntryBody, EntryKind, EntrySize, FileFacts, FileTime, SymlinkStatus,
};
use files_core::location::LocalPath;

/// How many are listed at most.
pub const LIMIT: usize = 200;

/// One remembered file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecentItem {
    pub path: PathBuf,
    /// When it was last used, as the list's ISO 8601 text (sortable as text).
    pub when: String,
}

/// Where the list is: `$XDG_DATA_HOME/recently-used.xbel`, or
/// `~/.local/share/recently-used.xbel`.
pub fn recent_file_from_env() -> Option<PathBuf> {
    crate::xdg::data_dirs_from_env()
        .into_iter()
        .next()
        .map(|data_home| data_home.join("recently-used.xbel"))
}

/// The `file://` bookmarks in an `.xbel` document, newest first, each path
/// once. Other schemes (a `sftp://` file, a `recent://` entry) are left out.
pub fn parse_xbel(xml: &str) -> Vec<RecentItem> {
    let mut items: Vec<RecentItem> = Vec::new();
    let mut rest = xml;
    while let Some(start) = rest.find("<bookmark ") {
        rest = &rest[start + "<bookmark ".len()..];
        let Some(end) = rest.find('>') else { break };
        let attributes = &rest[..end];
        let Some(href) = attribute(attributes, "href") else {
            continue;
        };
        let Some(path) = href.strip_prefix("file://").and_then(percent_decode) else {
            continue;
        };
        if !path.starts_with('/') {
            continue;
        }
        let when = attribute(attributes, "visited")
            .or_else(|| attribute(attributes, "modified"))
            .or_else(|| attribute(attributes, "added"))
            .unwrap_or_default();
        let path = PathBuf::from(path);
        match items.iter_mut().find(|item| item.path == path) {
            Some(known) if known.when < when => known.when = when,
            Some(_) => {}
            None => items.push(RecentItem { path, when }),
        }
    }
    items.sort_by(|a, b| b.when.cmp(&a.when));
    items
}

fn attribute(attributes: &str, name: &str) -> Option<String> {
    let key = format!("{name}=\"");
    let start = attributes
        .match_indices(&key)
        .find(|(at, _)| *at == 0 || attributes.as_bytes()[at - 1].is_ascii_whitespace())?
        .0
        + key.len();
    let end = attributes[start..].find('"')? + start;
    Some(
        attributes[start..end]
            .replace("&amp;", "&")
            .replace("&quot;", "\"")
            .replace("&apos;", "'")
            .replace("&lt;", "<")
            .replace("&gt;", ">"),
    )
}

/// `%20` and friends back to bytes; `None` for a malformed escape or a path
/// that is not UTF-8.
fn percent_decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let hex = text.get(index + 1..index + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// The rows of the Recents view: the files of the list that still exist,
/// newest first, at most [`LIMIT`]. A name the list holds twice (two
/// `notes.txt` in two folders) is shown once, the newer, because a row is
/// identified by its name.
pub fn recent_entries(list: &Path, mime: Option<&dyn crate::MimeDetector>) -> Vec<Entry> {
    let Ok(text) = fs::read_to_string(list) else {
        return Vec::new();
    };
    let mut names = std::collections::HashSet::new();
    let mut entries = Vec::new();
    for item in parse_xbel(&text) {
        if entries.len() >= LIMIT {
            break;
        }
        let Ok(metadata) = fs::metadata(&item.path) else {
            continue;
        };
        if !metadata.is_file() {
            continue;
        }
        let Some(name) = item
            .path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
        else {
            continue;
        };
        if !names.insert(name.clone()) {
            continue;
        }
        let Ok(path) = LocalPath::new(item.path.clone()) else {
            continue;
        };
        let mut entry = Entry::file(name, path.clone(), EntryKind::File);
        entry.size = EntrySize::Bytes(metadata.len());
        entry.modified = metadata.modified().ok().map(FileTime::from_system_time);
        entry.mime = mime.and_then(|detector| detector.detect(&entry.name));
        entry.body = EntryBody::File(FileFacts {
            path,
            symlink: SymlinkStatus::None,
            device: None,
        });
        entries.push(entry);
    }
    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    const XBEL: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<xbel version="1.0" xmlns:bookmark="http://www.freedesktop.org/standards/desktop-bookmarks">
  <bookmark href="file:///home/a/old.txt" added="2026-01-01T10:00:00Z" modified="2026-01-01T10:00:00Z" visited="2026-01-01T10:00:00Z"/>
  <bookmark href="file:///home/a/My%20Report.pdf" added="2026-03-01T10:00:00Z" modified="2026-03-02T10:00:00Z" visited="2026-03-05T10:00:00Z">
    <info/>
  </bookmark>
  <bookmark href="sftp://host/remote.txt" visited="2026-04-01T00:00:00Z"/>
  <bookmark href="file:///home/a/old.txt" visited="2026-02-01T10:00:00Z"/>
  <bookmark href="file:///home/a/R%26D%20%E6%96%87%E4%BB%B6.md" visited="2026-02-15T00:00:00Z"/>
</xbel>"#;

    #[test]
    fn local_files_newest_first_each_once_and_decoded() {
        let items = parse_xbel(XBEL);
        let paths: Vec<&str> = items
            .iter()
            .map(|item| item.path.to_str().unwrap())
            .collect();
        assert_eq!(
            paths,
            [
                "/home/a/My Report.pdf",
                "/home/a/R&D 文件.md",
                "/home/a/old.txt"
            ]
        );
        // The later visit of old.txt counts.
        assert_eq!(items[2].when, "2026-02-01T10:00:00Z");
    }

    #[test]
    fn a_broken_or_hostile_list_gives_nothing_rather_than_a_guess() {
        assert!(parse_xbel("<xbel><bookmark href=\"file:///x%ZZ\"/></xbel>").is_empty());
        assert!(parse_xbel("not xml at all").is_empty());
        assert!(parse_xbel("<bookmark href=\"file://relative\"/>").is_empty());
    }

    #[test]
    fn only_files_that_still_exist_are_listed() {
        let root = tempfile::tempdir().unwrap();
        let here = root.path().join("here.txt");
        fs::write(&here, b"12345").unwrap();
        let list = root.path().join("recently-used.xbel");
        fs::write(
            &list,
            format!(
                "<xbel><bookmark href=\"file://{}\" visited=\"2026-01-02T00:00:00Z\"/>\
                 <bookmark href=\"file://{}/gone.txt\" visited=\"2026-01-03T00:00:00Z\"/></xbel>",
                here.display(),
                root.path().display()
            ),
        )
        .unwrap();
        let entries = recent_entries(&list, None);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "here.txt");
        assert_eq!(entries[0].size, EntrySize::Bytes(5));
        assert!(recent_entries(&root.path().join("missing.xbel"), None).is_empty());
    }
}
