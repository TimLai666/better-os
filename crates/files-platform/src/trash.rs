//! Reading the freedesktop trash.
//!
//! The read side only. Restoring and permanently deleting are durable jobs and
//! belong to ticket 33; nothing here moves, writes, or unlinks anything.
//!
//! The layout is the specification's: a trash directory holds `info/` with one
//! `<name>.trashinfo` per item and `files/` with the item itself under the same
//! name. The info file carries the original path and the deletion time, which
//! is why a trashed item can say where it came from without the original
//! directory still existing.
//!
//! An item whose info file exists but whose data does not is skipped and
//! reported, not listed. Showing a row that cannot be restored or opened would
//! be worse than saying the trash has an orphaned record.

use std::fs;
use std::path::{Path, PathBuf};

use files_core::entry::{
    Entry, EntryBody, EntryKind, EntrySize, FileTime, HiddenState, PermissionsSummary, TrashedFacts,
};
use files_core::error::ListingError;
use files_core::listing::{Cancelled, ListingSink};
use files_core::location::LocalPath;

/// One trash directory: the home one, or a device's `.Trash-<uid>`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TrashDirectory {
    root: PathBuf,
}

impl TrashDirectory {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// The home trash: `$XDG_DATA_HOME/Trash`, falling back to
    /// `~/.local/share/Trash` as the Base Directory Specification says.
    pub fn home_from_env() -> Option<Self> {
        let data_home = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .or_else(|| {
                std::env::var_os("HOME")
                    .map(PathBuf::from)
                    .map(|home| home.join(".local/share"))
            })?;
        Some(Self::new(data_home.join("Trash")))
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn info_dir(&self) -> PathBuf {
        self.root.join("info")
    }

    pub fn files_dir(&self) -> PathBuf {
        self.root.join("files")
    }

    /// Whether this trash directory exists. An absent trash is empty, not an
    /// error: nothing has been deleted yet on a fresh account.
    pub fn exists(&self) -> bool {
        self.info_dir().is_dir()
    }
}

/// What one `.trashinfo` file says.
#[derive(Clone, Debug, Eq, PartialEq)]
struct TrashInfo {
    original_path: PathBuf,
    deleted_at: Option<FileTime>,
}

/// Streams the trash into a listing sink, using the same protocol a directory
/// listing uses.
pub fn read_trash(trash: &TrashDirectory, sink: &mut ListingSink) -> Result<(), Cancelled> {
    let info_dir = trash.info_dir();
    let files_dir = trash.files_dir();
    let Ok(iterator) = fs::read_dir(&info_dir) else {
        // An absent or unreadable trash lists as empty rather than failing.
        // The user's answer to "what is in my trash" is "nothing I can see",
        // and a failed listing would say something stronger than that.
        return Ok(());
    };

    for item in iterator.flatten() {
        if sink.is_cancelled() {
            return Err(Cancelled);
        }
        let file_name = item.file_name();
        let name = file_name.to_string_lossy();
        let Some(stem) = name.strip_suffix(".trashinfo") else {
            continue;
        };
        let stem = stem.to_string();
        let Ok(contents) = fs::read_to_string(item.path()) else {
            sink.skip(
                stem,
                ListingError::Io {
                    path: item.path().to_string_lossy().into_owned(),
                    reason: "unreadable_trashinfo".to_string(),
                },
            )?;
            continue;
        };
        let Some(info) = parse_trash_info(&contents) else {
            sink.skip(
                stem,
                ListingError::Io {
                    path: item.path().to_string_lossy().into_owned(),
                    reason: "malformed_trashinfo".to_string(),
                },
            )?;
            continue;
        };

        let stored = files_dir.join(&stem);
        let Ok(metadata) = fs::symlink_metadata(&stored) else {
            sink.skip(
                stem,
                ListingError::NotFound {
                    path: stored.to_string_lossy().into_owned(),
                },
            )?;
            continue;
        };
        let Ok(stored_path) = LocalPath::new(&stored) else {
            sink.skip(
                stem,
                ListingError::Io {
                    path: stored.to_string_lossy().into_owned(),
                    reason: "unrepresentable_path".to_string(),
                },
            )?;
            continue;
        };

        let display_name = info
            .original_path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| stem.clone());
        let kind = if metadata.is_dir() {
            EntryKind::Directory
        } else if metadata.is_file() {
            EntryKind::File
        } else {
            EntryKind::Unknown
        };
        sink.push(Entry {
            name: display_name,
            kind,
            size: if metadata.is_file() {
                EntrySize::Bytes(metadata.len())
            } else {
                EntrySize::Unknown
            },
            modified: metadata.modified().ok().map(FileTime::from_system_time),
            permissions: PermissionsSummary::UNKNOWN,
            // Nothing in the trash is hidden by the dot rule. A trashed
            // dotfile is in the trash because the user put it there, and
            // hiding it would make it unrecoverable through the interface.
            hidden: HiddenState::Visible,
            mime: None,
            body: EntryBody::Trashed(TrashedFacts {
                item: stem,
                original_path: info.original_path,
                deleted_at: info.deleted_at,
                stored_path,
            }),
        })?;
    }
    Ok(())
}

/// Parses a `.trashinfo` file.
///
/// Returns `None` when the required `Path` key is absent, because an item
/// whose original location is unknown cannot be restored and must not be
/// presented as if it could.
fn parse_trash_info(contents: &str) -> Option<TrashInfo> {
    let mut in_section = false;
    let mut original_path = None;
    let mut deleted_at = None;
    for line in contents.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_section = line == "[Trash Info]";
            continue;
        }
        if !in_section {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key.trim() {
            "Path" => original_path = Some(PathBuf::from(percent_decode(value.trim()))),
            "DeletionDate" => deleted_at = parse_deletion_date(value.trim()),
            _ => {}
        }
    }
    Some(TrashInfo {
        original_path: original_path?,
        deleted_at,
    })
}

/// The specification percent-encodes the original path.
fn percent_decode(value: &str) -> String {
    if !value.contains('%') {
        return value.to_string();
    }
    let bytes = value.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let Ok(byte) = u8::from_str_radix(&value[index + 1..index + 3], 16) {
                out.push(byte);
                index += 3;
                continue;
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Parses the specification's `YYYY-MM-DDThh:mm:ss` local-time stamp.
///
/// The value has no timezone, so it is read as UTC and the result is used for
/// ordering rather than presented as an exact instant. Saying so here is
/// better than a conversion that silently shifts by the local offset.
fn parse_deletion_date(value: &str) -> Option<FileTime> {
    let (date, time) = value.split_once('T')?;
    let mut date_parts = date.split('-');
    let year: i64 = date_parts.next()?.parse().ok()?;
    let month: i64 = date_parts.next()?.parse().ok()?;
    let day: i64 = date_parts.next()?.parse().ok()?;
    let mut time_parts = time.split(':');
    let hour: i64 = time_parts.next()?.parse().ok()?;
    let minute: i64 = time_parts.next()?.parse().ok()?;
    let second: i64 = time_parts.next().unwrap_or("0").parse().ok()?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let days = days_from_civil(year, month, day);
    Some(FileTime::new(
        days * 86_400 + hour * 3_600 + minute * 60 + second,
        0,
    ))
}

/// Howard Hinnant's `days_from_civil`: days since 1970-01-01 for a proleptic
/// Gregorian date. Used rather than a date crate because this is the only date
/// arithmetic in Better Files.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let day_of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;
    use files_core::listing::{ListingEvent, ListingRequest, ListingSession};
    use files_core::location::{Location, TrashLocation};

    /// Builds a trash directory that follows the specification's layout.
    fn fixture() -> (tempfile::TempDir, TrashDirectory) {
        let root = tempfile::tempdir().unwrap();
        let trash = TrashDirectory::new(root.path().join("Trash"));
        fs::create_dir_all(trash.info_dir()).unwrap();
        fs::create_dir_all(trash.files_dir()).unwrap();
        (root, trash)
    }

    fn add_item(trash: &TrashDirectory, stem: &str, info: &str, contents: &[u8]) {
        fs::write(trash.info_dir().join(format!("{stem}.trashinfo")), info).unwrap();
        fs::write(trash.files_dir().join(stem), contents).unwrap();
    }

    fn list(trash: &TrashDirectory) -> (Vec<Entry>, Vec<String>) {
        let request = ListingRequest::new(Location::Trash(TrashLocation::Root));
        let (mut session, mut sink) = ListingSession::start(&request);
        read_trash(trash, &mut sink).unwrap();
        sink.finish().unwrap();
        let mut entries = Vec::new();
        let mut skipped = Vec::new();
        for event in session.drain() {
            match event {
                ListingEvent::Batch(batch) => entries.extend(batch.entries),
                ListingEvent::Complete(summary) => {
                    skipped.extend(summary.skipped.into_iter().map(|entry| entry.name));
                }
                _ => {}
            }
        }
        entries.sort_by(|left, right| left.name.cmp(&right.name));
        (entries, skipped)
    }

    #[test]
    fn a_trashed_item_reports_where_it_came_from_and_when() {
        let (_root, trash) = fixture();
        add_item(
            &trash,
            "report.txt",
            "[Trash Info]\nPath=/home/user/Documents/report.txt\nDeletionDate=2024-03-05T10:15:30\n",
            b"contents",
        );
        let (entries, skipped) = list(&trash);
        assert!(skipped.is_empty());
        assert_eq!(entries.len(), 1);
        let entry = &entries[0];
        assert_eq!(entry.name, "report.txt");
        assert_eq!(entry.size, EntrySize::Bytes(8));
        match &entry.body {
            EntryBody::Trashed(facts) => {
                assert_eq!(
                    facts.original_path,
                    Path::new("/home/user/Documents/report.txt")
                );
                assert_eq!(facts.deleted_at, Some(FileTime::new(1_709_633_730, 0)));
                assert_eq!(facts.item, "report.txt");
            }
            other => panic!("expected a trashed entry, got {other:?}"),
        }
        // A trashed item is not a path a consumer may act on directly.
        assert_eq!(entry.as_local_path(), None);
    }

    #[test]
    fn a_percent_encoded_original_path_is_decoded() {
        let (_root, trash) = fixture();
        add_item(
            &trash,
            "holiday",
            "[Trash Info]\nPath=/home/user/My%20Photos/holiday%20%232.jpg\nDeletionDate=2024-01-01T00:00:00\n",
            b"x",
        );
        let (entries, _) = list(&trash);
        match &entries[0].body {
            EntryBody::Trashed(facts) => assert_eq!(
                facts.original_path,
                Path::new("/home/user/My Photos/holiday #2.jpg")
            ),
            other => panic!("expected a trashed entry, got {other:?}"),
        }
        assert_eq!(entries[0].name, "holiday #2.jpg");
    }

    #[test]
    fn an_info_file_with_no_matching_data_is_reported_not_listed() {
        let (_root, trash) = fixture();
        fs::write(
            trash.info_dir().join("orphan.trashinfo"),
            "[Trash Info]\nPath=/home/user/orphan\nDeletionDate=2024-01-01T00:00:00\n",
        )
        .unwrap();
        let (entries, skipped) = list(&trash);
        assert!(entries.is_empty());
        assert_eq!(skipped, ["orphan"]);
    }

    #[test]
    fn an_info_file_without_a_path_is_refused() {
        let (_root, trash) = fixture();
        add_item(
            &trash,
            "nameless",
            "[Trash Info]\nDeletionDate=2024-01-01T00:00:00\n",
            b"x",
        );
        let (entries, skipped) = list(&trash);
        assert!(entries.is_empty());
        assert_eq!(skipped, ["nameless"]);
    }

    #[test]
    fn a_trashed_dotfile_is_visible_because_the_user_put_it_there() {
        let (_root, trash) = fixture();
        add_item(
            &trash,
            ".bashrc",
            "[Trash Info]\nPath=/home/user/.bashrc\nDeletionDate=2024-01-01T00:00:00\n",
            b"x",
        );
        let (entries, _) = list(&trash);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].hidden, HiddenState::Visible);
    }

    #[test]
    fn a_trashed_directory_is_listed_as_a_directory() {
        let (_root, trash) = fixture();
        fs::write(
            trash.info_dir().join("Project.trashinfo"),
            "[Trash Info]\nPath=/home/user/Project\nDeletionDate=2024-02-02T12:00:00\n",
        )
        .unwrap();
        fs::create_dir(trash.files_dir().join("Project")).unwrap();
        let (entries, _) = list(&trash);
        assert_eq!(entries[0].kind, EntryKind::Directory);
        assert_eq!(entries[0].size, EntrySize::Unknown);
    }

    #[test]
    fn an_absent_trash_directory_lists_as_empty() {
        let root = tempfile::tempdir().unwrap();
        let trash = TrashDirectory::new(root.path().join("no-trash-here"));
        assert!(!trash.exists());
        let (entries, skipped) = list(&trash);
        assert!(entries.is_empty());
        assert!(skipped.is_empty());
    }

    #[test]
    fn the_civil_date_conversion_matches_known_instants() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2000, 3, 1), 11_017);
        assert_eq!(
            parse_deletion_date("1970-01-01T00:00:00"),
            Some(FileTime::EPOCH)
        );
        assert_eq!(parse_deletion_date("not-a-date"), None);
        assert_eq!(parse_deletion_date("2024-13-01T00:00:00"), None);
    }
}
