//! Favorites, stored in the file every other GTK file manager already reads.
//!
//! Issue #6 asks for "standard XDG bookmark data where compatibility is
//! practical", and here it is practical: `$XDG_CONFIG_HOME/gtk-3.0/bookmarks`
//! is one URI per line with an optional display label after a space, which is
//! exactly the three things a Favorites row needs — a target, an order, and a
//! label. Nautilus, Nemo, Thunar, and PCManFM all read it, so a folder pinned
//! in Better Files is pinned in the session's other file managers too.
//!
//! **Foreign lines survive byte for byte.** A line this build does not
//! understand — a `sftp://` bookmark, a comment, a blank line, a scheme added
//! by a newer GTK — is kept as the exact bytes it was read as, at the exact
//! index it was read at, and written back unchanged. Reordering moves our
//! bookmarks among the positions our bookmarks already occupy, so a foreign
//! line never moves and never disappears. That is the same rule the app
//! chooser applies to `mimeapps.list`, and for the same reason: this file
//! belongs to the desktop, not to us.
//!
//! Labels do go in the shared file, because the format has a place for them.
//! Nothing is stored in a private side-car.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use files_core::{LocalPath, Location};

/// One line of the file.
#[derive(Clone, Debug, Eq, PartialEq)]
enum Line {
    /// A `file://` bookmark this build understands.
    Bookmark(Bookmark),
    /// Anything else, kept exactly as read.
    Foreign(String),
}

/// A pinned folder.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Bookmark {
    /// The URI as it will be written. Preserved verbatim from the file when it
    /// was read from one, so a percent-encoding this build would have produced
    /// differently does not rewrite somebody else's line.
    uri: String,
    location: Location,
    label: Option<String>,
}

impl Bookmark {
    /// Builds a bookmark for a local directory.
    pub fn for_path(path: &LocalPath) -> Self {
        Self {
            uri: encode_file_uri(path.as_path()),
            location: Location::Local(path.clone()),
            label: None,
        }
    }

    pub fn location(&self) -> &Location {
        &self.location
    }

    pub fn uri(&self) -> &str {
        &self.uri
    }

    pub fn label(&self) -> Option<&str> {
        self.label.as_deref()
    }

    /// The text the sidebar row shows: the chosen label, or the folder name.
    pub fn display_name(&self) -> String {
        match &self.label {
            Some(label) => label.clone(),
            None => self.location.display_name(),
        }
    }

    pub fn path(&self) -> Option<&Path> {
        self.location.as_local_path().map(LocalPath::as_path)
    }
}

/// The whole file, parsed, with every line accounted for.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct BookmarkFile {
    lines: Vec<Line>,
    /// Whether the file ended with a newline, so a round trip is exact.
    trailing_newline: bool,
}

/// What a bookmark edit produced.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PinOutcome {
    /// A new bookmark was appended.
    Pinned,
    /// The location was already pinned; nothing changed.
    AlreadyPinned,
    /// The location cannot be pinned: it has no filesystem path.
    NotPinnable,
}

impl BookmarkFile {
    pub fn parse(text: &str) -> Self {
        let trailing_newline = text.ends_with('\n');
        let body = text.strip_suffix('\n').unwrap_or(text);
        let lines = if body.is_empty() && text.is_empty() {
            Vec::new()
        } else {
            body.split('\n').map(parse_line).collect()
        };
        Self {
            lines,
            trailing_newline,
        }
    }

    /// The exact bytes to write.
    pub fn render(&self) -> String {
        let mut out = String::new();
        for (index, line) in self.lines.iter().enumerate() {
            if index > 0 {
                out.push('\n');
            }
            match line {
                Line::Foreign(raw) => out.push_str(raw),
                Line::Bookmark(bookmark) => {
                    out.push_str(&bookmark.uri);
                    if let Some(label) = &bookmark.label {
                        out.push(' ');
                        out.push_str(label);
                    }
                }
            }
        }
        if self.trailing_newline && !out.is_empty() {
            out.push('\n');
        }
        out
    }

    /// The bookmarks, in sidebar order.
    pub fn bookmarks(&self) -> Vec<&Bookmark> {
        self.lines
            .iter()
            .filter_map(|line| match line {
                Line::Bookmark(bookmark) => Some(bookmark),
                Line::Foreign(_) => None,
            })
            .collect()
    }

    pub fn len(&self) -> usize {
        self.positions().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn get(&self, index: usize) -> Option<&Bookmark> {
        self.bookmarks().get(index).copied()
    }

    pub fn contains(&self, location: &Location) -> bool {
        self.bookmarks()
            .iter()
            .any(|bookmark| &bookmark.location == location)
    }

    /// Pins a location. Appending rather than inserting is deliberate: a new
    /// favorite goes at the end of Favorites, which is where a user who just
    /// dropped one looks for it.
    pub fn pin(&mut self, location: &Location) -> PinOutcome {
        let Some(path) = location.as_local_path() else {
            return PinOutcome::NotPinnable;
        };
        if self.contains(location) {
            return PinOutcome::AlreadyPinned;
        }
        self.lines.push(Line::Bookmark(Bookmark::for_path(path)));
        PinOutcome::Pinned
    }

    /// Removes one bookmark. The directory it pointed at is untouched.
    pub fn remove(&mut self, index: usize) -> Option<Bookmark> {
        let position = *self.positions().get(index)?;
        match self.lines.remove(position) {
            Line::Bookmark(bookmark) => Some(bookmark),
            foreign => {
                // Unreachable by construction; restoring it is still cheaper
                // than losing somebody else's line to a bug here.
                self.lines.insert(position, foreign);
                None
            }
        }
    }

    /// Renames the bookmark's label. The directory keeps its own name.
    ///
    /// An empty label clears it, so the row falls back to the folder name
    /// rather than showing a blank row.
    pub fn set_label(&mut self, index: usize, label: &str) -> bool {
        let Some(position) = self.positions().get(index).copied() else {
            return false;
        };
        let trimmed = label.trim();
        if let Line::Bookmark(bookmark) = &mut self.lines[position] {
            bookmark.label = if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            };
            return true;
        }
        false
    }

    /// Moves a bookmark one place earlier in Favorites.
    ///
    /// The swap is between the two *bookmark* positions, so every foreign line
    /// keeps the index it had.
    pub fn move_up(&mut self, index: usize) -> bool {
        if index == 0 {
            return false;
        }
        self.swap_positions(index, index - 1)
    }

    pub fn move_down(&mut self, index: usize) -> bool {
        self.swap_positions(index, index + 1)
    }

    /// Moves a bookmark to an arbitrary slot, which is what a drop between two
    /// rows means.
    pub fn move_to(&mut self, from: usize, to: usize) -> bool {
        if from == to {
            return false;
        }
        let positions = self.positions();
        if from >= positions.len() || to >= positions.len() {
            return false;
        }
        let moved = self.lines.remove(positions[from]);
        // Removing the source shifted every later bookmark down by one slot,
        // so the target is recomputed against what is left rather than
        // against the indices the caller saw.
        let remaining = self.positions();
        let insert_at = if to < from {
            // Land in front of the bookmark that currently holds the target
            // index, which is still the one the caller pointed at.
            remaining.get(to).copied().unwrap_or(self.lines.len())
        } else {
            // Land behind the bookmark that used to hold the target index and
            // is now one slot earlier.
            remaining
                .get(to - 1)
                .map(|position| position + 1)
                .unwrap_or(self.lines.len())
        };
        self.lines.insert(insert_at.min(self.lines.len()), moved);
        true
    }

    fn swap_positions(&mut self, left: usize, right: usize) -> bool {
        let positions = self.positions();
        let (Some(a), Some(b)) = (positions.get(left), positions.get(right)) else {
            return false;
        };
        self.lines.swap(*a, *b);
        true
    }

    /// Where in `lines` each bookmark sits.
    fn positions(&self) -> Vec<usize> {
        self.lines
            .iter()
            .enumerate()
            .filter_map(|(index, line)| matches!(line, Line::Bookmark(_)).then_some(index))
            .collect()
    }

    /// How many lines this build did not understand. The persistence test
    /// asserts this is not zero for a file that had foreign entries, so
    /// "preserved" cannot pass by having parsed nothing.
    pub fn foreign_line_count(&self) -> usize {
        self.lines.len() - self.positions().len()
    }
}

fn parse_line(raw: &str) -> Line {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return Line::Foreign(raw.to_string());
    }
    let (uri, label) = match raw.split_once(' ') {
        Some((uri, label)) => (uri, Some(label.trim().to_string())),
        None => (raw, None),
    };
    let Some(encoded) = uri.strip_prefix("file://") else {
        return Line::Foreign(raw.to_string());
    };
    let decoded = decode_percent(encoded);
    let Ok(path) = LocalPath::new(decoded) else {
        return Line::Foreign(raw.to_string());
    };
    Line::Bookmark(Bookmark {
        uri: uri.to_string(),
        location: Location::Local(path),
        label: label.filter(|value| !value.is_empty()),
    })
}

/// Percent-encodes a path into a `file://` URI the way GTK writes one:
/// unreserved ASCII and `/` pass through, everything else becomes `%XX` over
/// the path's raw bytes.
fn encode_file_uri(path: &Path) -> String {
    use std::os::unix::ffi::OsStrExt as _;
    let mut out = String::from("file://");
    for byte in path.as_os_str().as_bytes() {
        let byte = *byte;
        let unreserved = byte.is_ascii_alphanumeric()
            || matches!(byte, b'-' | b'.' | b'_' | b'~' | b'/' | b'@' | b'+' | b'$');
        if unreserved {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// Decodes `%XX` sequences back to bytes. Anything that is not a valid escape
/// is kept literally, because a bookmark line with a stray `%` is still a
/// bookmark and refusing it would drop somebody's favorite.
fn decode_percent(value: &str) -> PathBuf {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt as _;
    let source = value.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(source.len());
    let mut index = 0usize;
    while index < source.len() {
        if source[index] == b'%' && index + 2 < source.len() {
            let high = (source[index + 1] as char).to_digit(16);
            let low = (source[index + 2] as char).to_digit(16);
            if let (Some(high), Some(low)) = (high, low) {
                out.push((high * 16 + low) as u8);
                index += 3;
                continue;
            }
        }
        out.push(source[index]);
        index += 1;
    }
    PathBuf::from(OsString::from_vec(out))
}

/// The bookmark file on disk.
#[derive(Clone, Debug)]
pub struct BookmarkStore {
    path: PathBuf,
}

impl BookmarkStore {
    pub fn at_path(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// `$XDG_CONFIG_HOME/gtk-3.0/bookmarks`, the shared location.
    pub fn from_env() -> Self {
        Self::at_path(crate::prefs::config_home().join("gtk-3.0/bookmarks"))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Reads the file. A missing file is an empty Favorites section, not an
    /// error: a user who has never pinned anything has no such file.
    pub fn load(&self) -> BookmarkFile {
        match fs::read_to_string(&self.path) {
            Ok(text) => BookmarkFile::parse(&text),
            Err(_) => BookmarkFile::default(),
        }
    }

    /// Writes through a temporary file and a rename, so an interrupted save
    /// cannot leave a half-written bookmarks file for the whole desktop.
    pub fn save(&self, file: &BookmarkFile) -> io::Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let temporary = self.path.with_extension("better-files-tmp");
        fs::write(&temporary, file.render().as_bytes())?;
        fs::rename(&temporary, &self.path)
    }
}
