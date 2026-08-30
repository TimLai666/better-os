//! The freedesktop trash, read and write.
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
//!
//! ## The write side
//!
//! Trashing an item claims its name by creating the info file with `O_EXCL`
//! before moving anything. That ordering is the specification's and it matters:
//! two processes trashing `notes.txt` at the same moment cannot both win the
//! name, so neither ends up with the other's data under its own record.
//!
//! The move itself is a `rename(2)`, which works only within one filesystem.
//! Trashing something from a mounted device therefore fails with
//! [`TrashError::CrossDevice`], and the caller decides what to do about it —
//! `files-operations` falls back to copying into the home trash and deleting
//! the source. The per-device `.Trash-$uid` directory the specification also
//! allows is not created here: creating a top-level `.Trash` on a removable
//! device, checking its sticky bit, and falling back to `.Trash-$uid` is a
//! separate piece of work with its own permission cases, and pretending to
//! support it while silently using the home trash would put the user's files
//! somewhere they did not expect.
//!
//! Nothing in the write side converts a path to a `String`. An original path
//! is percent-encoded from its bytes and decoded back to bytes, so a file whose
//! name is not valid UTF-8 goes into the trash and comes back out under the
//! name it actually had.

use std::ffi::{OsStr, OsString};
use std::fs::{self, File};
use std::io::Write;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use thiserror::Error;

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
            "Path" => {
                original_path = Some(PathBuf::from(OsString::from_vec(percent_decode(
                    value.trim(),
                ))))
            }
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
///
/// The result is bytes, not a `String`. A path that is not valid UTF-8 is
/// exactly what percent-encoding exists to carry, and decoding it into a
/// `String` would replace those bytes and hand back a path that restores to
/// the wrong name.
fn percent_decode(value: &str) -> Vec<u8> {
    if !value.contains('%') {
        return value.as_bytes().to_vec();
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
    out
}

/// The inverse: everything outside the specification's unreserved set becomes
/// `%XX`.
///
/// `/` is left alone so the stored path stays readable, which is what every
/// other trash implementation does and what makes the file inspectable by
/// hand.
fn percent_encode(path: &Path) -> String {
    let mut out = String::new();
    for byte in path.as_os_str().as_bytes() {
        let byte = *byte;
        let unreserved =
            byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~' | b'/');
        if unreserved {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
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

/// Formats an instant the way `.trashinfo` wants it.
///
/// The specification's field has no timezone. The read side already documents
/// that it interprets the value as UTC for ordering rather than as an exact
/// local instant, and the write side matches it, so a round trip through this
/// module is exact.
fn format_deletion_date(seconds: i64) -> String {
    let days = seconds.div_euclid(86_400);
    let rest = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let (hour, minute, second) = (rest / 3600, (rest % 3600) / 60, rest % 60);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}")
}

/// Howard Hinnant's `civil_from_days`, the inverse of [`days_from_civil`].
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let days = days + 719_468;
    let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    (if month <= 2 { year + 1 } else { year }, month, day)
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

// --- The write side ------------------------------------------------------

/// `EXDEV`, spelled out because this crate has no `libc` dependency and does
/// not need one for a single constant. It is 18 on every Linux architecture.
const EXDEV: i32 = 18;

/// Why a trash operation would not happen.
#[derive(Clone, Debug, Eq, PartialEq, Error)]
pub enum TrashError {
    /// The item is on a different filesystem from the trash directory, so
    /// `rename(2)` will not move it. The caller decides between the home-trash
    /// copy-and-delete fallback and refusing.
    #[error("files.trash.error.cross_device:{}", .path.to_string_lossy())]
    CrossDevice { path: PathBuf },
    /// Nothing is at the path being trashed.
    #[error("files.trash.error.not_found:{}", .path.to_string_lossy())]
    NotFound { path: PathBuf },
    /// The item's `.trashinfo` record is missing, unreadable, or has no `Path`
    /// key. Without it nothing can say where the item came from.
    #[error("files.trash.error.no_record:{item}")]
    NoRecord { item: String },
    /// Something already occupies the path a restore would put the item back
    /// at. Reported rather than resolved: the choice between overwriting,
    /// renaming, and skipping belongs to the job that asked.
    #[error("files.trash.error.destination_occupied:{}", .path.to_string_lossy())]
    DestinationOccupied { path: PathBuf },
    /// The directory the item came from no longer exists.
    #[error("files.trash.error.original_parent_missing:{}", .path.to_string_lossy())]
    OriginalParentMissing { path: PathBuf },
    #[error("files.trash.error.io:{}:{reason}", .path.to_string_lossy())]
    Io { path: PathBuf, reason: String },
}

impl TrashError {
    fn io(path: impl Into<PathBuf>, error: &std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            reason: error.kind().to_string(),
        }
    }
}

/// What a successful trashing produced.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TrashedItem {
    /// The `.trashinfo` stem, which is the item's identity for restore and
    /// purge. It differs from the original file name when the name was taken.
    pub item: String,
    pub info_path: PathBuf,
    pub stored_path: PathBuf,
    pub original_path: PathBuf,
    pub deleted_at_seconds: i64,
}

/// Creates `info/` and `files/` if they are not there yet.
pub fn ensure_trash(trash: &TrashDirectory) -> Result<(), TrashError> {
    for directory in [trash.info_dir(), trash.files_dir()] {
        fs::create_dir_all(&directory).map_err(|error| TrashError::io(&directory, &error))?;
    }
    Ok(())
}

/// Moves one item into the trash.
///
/// The order is the specification's, and it is the whole collision story:
///
/// 1. Pick a candidate name.
/// 2. Create `info/<name>.trashinfo` with `O_EXCL`. Losing this race means
///    another process took the name; try the next candidate.
/// 3. `rename` the item to `files/<name>`.
///
/// If step 3 fails the info file is removed again, so a failed trashing never
/// leaves a record pointing at nothing.
pub fn move_to_trash(trash: &TrashDirectory, source: &Path) -> Result<TrashedItem, TrashError> {
    let metadata = fs::symlink_metadata(source).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            TrashError::NotFound {
                path: source.to_path_buf(),
            }
        } else {
            TrashError::io(source, &error)
        }
    })?;
    let _ = metadata;
    ensure_trash(trash)?;

    let original = absolute(source);
    let base = source
        .file_name()
        .map(OsStr::to_os_string)
        .unwrap_or_else(|| OsString::from("unnamed"));
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or(0);
    let record = format!(
        "[Trash Info]\nPath={}\nDeletionDate={}\n",
        percent_encode(&original),
        format_deletion_date(seconds)
    );

    for attempt in 0u32..10_000 {
        // The stored file and its record must share one name, so the
        // identifier is computed once and used for both.
        let candidate = candidate_display(&candidate_name(&base, attempt));
        let info_path = trash.info_dir().join(format!("{candidate}.trashinfo"));
        let mut file = match File::options()
            .write(true)
            .create_new(true)
            .open(&info_path)
        {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(TrashError::io(&info_path, &error)),
        };
        if let Err(error) = file.write_all(record.as_bytes()) {
            let _ = fs::remove_file(&info_path);
            return Err(TrashError::io(&info_path, &error));
        }
        drop(file);

        let stored_path = trash.files_dir().join(&candidate);
        // `candidate` is a `String` here only because percent-encoding made it
        // one; the original bytes live in the record's `Path` key.
        match fs::rename(source, &stored_path) {
            Ok(()) => {
                return Ok(TrashedItem {
                    item: candidate,
                    info_path,
                    stored_path,
                    original_path: original,
                    deleted_at_seconds: seconds,
                });
            }
            Err(error) => {
                // The record must not outlive the failure that stopped the move.
                let _ = fs::remove_file(&info_path);
                return Err(match error.raw_os_error() {
                    Some(EXDEV) => TrashError::CrossDevice {
                        path: source.to_path_buf(),
                    },
                    _ => TrashError::io(source, &error),
                });
            }
        }
    }
    Err(TrashError::Io {
        path: source.to_path_buf(),
        reason: "no_free_trash_name".to_string(),
    })
}

/// Where a trashed item says it came from.
pub fn original_path_of(trash: &TrashDirectory, item: &str) -> Result<PathBuf, TrashError> {
    let info_path = trash.info_dir().join(format!("{item}.trashinfo"));
    let contents = fs::read_to_string(&info_path).map_err(|_| TrashError::NoRecord {
        item: item.to_string(),
    })?;
    let info = parse_trash_info(&contents).ok_or_else(|| TrashError::NoRecord {
        item: item.to_string(),
    })?;
    Ok(info.original_path)
}

/// Puts a trashed item back where it came from.
///
/// The destination is checked first and an occupied one is refused, not
/// overwritten. A restore that silently replaced a newer file with the deleted
/// one would be the single most destructive thing a file manager could do
/// quietly.
pub fn restore(trash: &TrashDirectory, item: &str) -> Result<PathBuf, TrashError> {
    let original = original_path_of(trash, item)?;
    restore_to(trash, item, &original)
}

/// Puts a trashed item at a chosen path, which is how a caller answers a
/// collision with "restore it beside the file that is already there".
pub fn restore_to(
    trash: &TrashDirectory,
    item: &str,
    destination: &Path,
) -> Result<PathBuf, TrashError> {
    let stored = trash.files_dir().join(item);
    if fs::symlink_metadata(&stored).is_err() {
        return Err(TrashError::NoRecord {
            item: item.to_string(),
        });
    }
    if fs::symlink_metadata(destination).is_ok() {
        return Err(TrashError::DestinationOccupied {
            path: destination.to_path_buf(),
        });
    }
    let parent = destination.parent().unwrap_or(Path::new("/"));
    if !parent.is_dir() {
        return Err(TrashError::OriginalParentMissing {
            path: parent.to_path_buf(),
        });
    }
    match fs::rename(&stored, destination) {
        Ok(()) => {}
        Err(error) => {
            return Err(match error.raw_os_error() {
                Some(EXDEV) => TrashError::CrossDevice {
                    path: destination.to_path_buf(),
                },
                _ => TrashError::io(destination, &error),
            });
        }
    }
    // The record goes only after the data is safely back.
    let _ = fs::remove_file(trash.info_dir().join(format!("{item}.trashinfo")));
    Ok(destination.to_path_buf())
}

/// Removes one item from the trash for good.
///
/// The data goes first and the record second, so an interruption leaves an
/// orphaned record — which the read side already skips and reports — rather
/// than a record-less file that nothing can name.
pub fn purge(trash: &TrashDirectory, item: &str) -> Result<(), TrashError> {
    let stored = trash.files_dir().join(item);
    match fs::symlink_metadata(&stored) {
        Ok(metadata) if metadata.is_dir() => {
            fs::remove_dir_all(&stored).map_err(|error| TrashError::io(&stored, &error))?;
        }
        Ok(_) => {
            fs::remove_file(&stored).map_err(|error| TrashError::io(&stored, &error))?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(TrashError::io(&stored, &error)),
    }
    let info_path = trash.info_dir().join(format!("{item}.trashinfo"));
    match fs::remove_file(&info_path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(TrashError::io(&info_path, &error)),
    }
}

/// The `attempt`-th candidate name for an item called `base`.
///
/// The suffix goes before the extension so an item restored under a
/// uniquified name still looks like the file it is.
fn candidate_name(base: &OsStr, attempt: u32) -> OsString {
    if attempt == 0 {
        return base.to_os_string();
    }
    let bytes = base.as_bytes();
    let split = bytes
        .iter()
        .rposition(|byte| *byte == b'.')
        .filter(|index| *index > 0);
    let (stem, extension) = match split {
        Some(index) => (&bytes[..index], &bytes[index..]),
        None => (bytes, &[][..]),
    };
    let mut out = Vec::with_capacity(bytes.len() + 8);
    out.extend_from_slice(stem);
    out.extend_from_slice(format!(".{attempt}").as_bytes());
    out.extend_from_slice(extension);
    OsString::from_vec(out)
}

/// The item identifier used in the `.trashinfo` file name.
///
/// The specification names the info file after the stored file, and a stored
/// file whose name is not valid UTF-8 has no `String` form. Percent-encoding
/// the bytes that will not survive keeps the identifier a `String` — which is
/// what the read side already returns — without ever losing a byte, and the
/// stored file keeps its real name.
fn candidate_display(name: &OsStr) -> String {
    match name.to_str() {
        Some(text) => text.to_string(),
        None => percent_encode(Path::new(name)),
    }
}

/// The path as an absolute one, without resolving symlinks.
fn absolute(path: &Path) -> PathBuf {
    if path.is_absolute() {
        return path.to_path_buf();
    }
    match std::env::current_dir() {
        Ok(current) => current.join(path),
        Err(_) => path.to_path_buf(),
    }
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

    // --- The write side ---------------------------------------------------

    fn empty_trash() -> (tempfile::TempDir, TrashDirectory) {
        let root = tempfile::tempdir().unwrap();
        let trash = TrashDirectory::new(root.path().join("Trash"));
        (root, trash)
    }

    #[test]
    fn trashing_an_item_moves_it_and_records_where_it_came_from() {
        let (root, trash) = empty_trash();
        let file = root.path().join("notes.txt");
        fs::write(&file, b"content").unwrap();

        let item = move_to_trash(&trash, &file).unwrap();
        assert_eq!(item.item, "notes.txt");
        assert!(!file.exists());
        assert_eq!(fs::read(&item.stored_path).unwrap(), b"content");
        let record = fs::read_to_string(&item.info_path).unwrap();
        assert!(record.starts_with("[Trash Info]\n"));
        assert!(record.contains(&format!("Path={}", file.display())));

        // And the read side sees exactly one item, with its original path.
        let (entries, skipped) = list(&trash);
        assert!(skipped.is_empty());
        assert_eq!(entries.len(), 1);
        match &entries[0].body {
            EntryBody::Trashed(facts) => assert_eq!(facts.original_path, file),
            other => panic!("expected a trashed entry, got {other:?}"),
        }
    }

    #[test]
    fn two_items_with_the_same_name_both_fit_in_the_trash() {
        let (root, trash) = empty_trash();
        let first = root.path().join("a/report.txt");
        let second = root.path().join("b/report.txt");
        fs::create_dir_all(first.parent().unwrap()).unwrap();
        fs::create_dir_all(second.parent().unwrap()).unwrap();
        fs::write(&first, b"first").unwrap();
        fs::write(&second, b"second").unwrap();

        let one = move_to_trash(&trash, &first).unwrap();
        let two = move_to_trash(&trash, &second).unwrap();
        assert_eq!(one.item, "report.txt");
        assert_eq!(two.item, "report.1.txt");
        assert_eq!(fs::read(&one.stored_path).unwrap(), b"first");
        assert_eq!(fs::read(&two.stored_path).unwrap(), b"second");
        // Both keep their own original path, which is what makes both
        // restorable to the right place.
        assert_eq!(original_path_of(&trash, &one.item).unwrap(), first);
        assert_eq!(original_path_of(&trash, &two.item).unwrap(), second);
    }

    #[test]
    fn a_restore_puts_the_item_back_and_takes_the_record_with_it() {
        let (root, trash) = empty_trash();
        let file = root.path().join("deep/notes.txt");
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, b"content").unwrap();
        let item = move_to_trash(&trash, &file).unwrap();

        let restored = restore(&trash, &item.item).unwrap();
        assert_eq!(restored, file);
        assert_eq!(fs::read(&file).unwrap(), b"content");
        assert!(!item.stored_path.exists());
        assert!(!item.info_path.exists());
    }

    #[test]
    fn a_restore_onto_an_occupied_path_is_refused_rather_than_overwriting() {
        let (root, trash) = empty_trash();
        let file = root.path().join("notes.txt");
        fs::write(&file, b"old").unwrap();
        let item = move_to_trash(&trash, &file).unwrap();
        fs::write(&file, b"something newer").unwrap();

        let error = restore(&trash, &item.item).unwrap_err();
        assert!(matches!(error, TrashError::DestinationOccupied { .. }));
        // Neither side was touched.
        assert_eq!(fs::read(&file).unwrap(), b"something newer");
        assert_eq!(fs::read(&item.stored_path).unwrap(), b"old");

        // The caller resolves it by naming somewhere else.
        let beside = root.path().join("notes (restored).txt");
        assert_eq!(restore_to(&trash, &item.item, &beside).unwrap(), beside);
        assert_eq!(fs::read(&beside).unwrap(), b"old");
    }

    #[test]
    fn a_restore_whose_original_directory_is_gone_says_so() {
        let (root, trash) = empty_trash();
        let directory = root.path().join("vanishing");
        fs::create_dir(&directory).unwrap();
        let file = directory.join("notes.txt");
        fs::write(&file, b"x").unwrap();
        let item = move_to_trash(&trash, &file).unwrap();
        fs::remove_dir(&directory).unwrap();

        let error = restore(&trash, &item.item).unwrap_err();
        assert!(matches!(error, TrashError::OriginalParentMissing { .. }));
        assert!(item.stored_path.exists(), "the item is still recoverable");
    }

    #[test]
    fn purging_removes_the_data_and_the_record_together() {
        let (root, trash) = empty_trash();
        let tree = root.path().join("project");
        fs::create_dir_all(tree.join("src")).unwrap();
        fs::write(tree.join("src/main.rs"), b"fn main() {}").unwrap();
        let item = move_to_trash(&trash, &tree).unwrap();
        assert!(item.stored_path.join("src/main.rs").exists());

        purge(&trash, &item.item).unwrap();
        assert!(!item.stored_path.exists());
        assert!(!item.info_path.exists());
        let (entries, skipped) = list(&trash);
        assert!(entries.is_empty() && skipped.is_empty());
    }

    #[test]
    fn a_name_that_is_not_utf8_goes_into_the_trash_and_comes_back_out_intact() {
        let (root, trash) = empty_trash();
        let name = OsStr::from_bytes(b"caf\xe9\xff.txt");
        let file = root.path().join(name);
        fs::write(&file, b"content").unwrap();

        let item = move_to_trash(&trash, &file).unwrap();
        // The record's `Path` key carries the real bytes, percent-encoded.
        assert_eq!(original_path_of(&trash, &item.item).unwrap(), file);
        let restored = restore(&trash, &item.item).unwrap();
        assert_eq!(restored.file_name().unwrap().as_bytes(), name.as_bytes());
        assert_eq!(fs::read(&file).unwrap(), b"content");
    }

    #[test]
    fn trashing_something_that_is_not_there_says_so_rather_than_leaving_a_record() {
        let (root, trash) = empty_trash();
        let error = move_to_trash(&trash, &root.path().join("absent")).unwrap_err();
        assert!(matches!(error, TrashError::NotFound { .. }));
        assert!(!trash.info_dir().exists() || fs::read_dir(trash.info_dir()).unwrap().count() == 0);
    }

    #[test]
    fn a_deletion_date_written_here_is_read_back_as_the_same_instant() {
        assert_eq!(format_deletion_date(0), "1970-01-01T00:00:00");
        assert_eq!(format_deletion_date(1_709_633_730), "2024-03-05T10:15:30");
        for seconds in [
            0i64,
            1,
            86_399,
            86_400,
            951_782_400,
            1_709_633_730,
            4_102_444_800,
        ] {
            let text = format_deletion_date(seconds);
            assert_eq!(
                parse_deletion_date(&text),
                Some(FileTime::new(seconds, 0)),
                "round trip failed for {text}"
            );
        }
    }

    #[test]
    fn percent_encoding_round_trips_a_path_with_spaces_and_invalid_bytes() {
        let path = PathBuf::from(OsString::from_vec(
            b"/home/user/My Photos/holiday \xff #2.jpg".to_vec(),
        ));
        let encoded = percent_encode(&path);
        assert!(!encoded.contains(' '));
        assert_eq!(
            PathBuf::from(OsString::from_vec(percent_decode(&encoded))),
            path
        );
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
