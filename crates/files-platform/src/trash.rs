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
//! Trashing something from a mounted device into the home trash therefore
//! fails with [`TrashError::CrossDevice`].
//!
//! ## Per-volume trash
//!
//! The specification gives every mounted device its own trash, so deleting a
//! large file on a USB disk does not copy it onto the home partition. The top
//! directory of an item is the mount point of its device, found by
//! [`top_directory`] walking up while the device number stays the same. On
//! that top directory, [`volume_trash`] uses `$topdir/.Trash/$uid` when
//! `$topdir/.Trash` is a real directory with the sticky bit set — an
//! administrator's shared trash — and otherwise `$topdir/.Trash-$uid`,
//! created with mode 0700. A shared `.Trash` that is a symbolic link or lacks
//! the sticky bit is refused, because either would let another user read or
//! replace what this user deletes.
//!
//! A `.trashinfo` in a volume trash records the original path relative to the
//! top directory, so the record stays right when the device is mounted
//! somewhere else next time. A [`TrashDirectory`] recognises the two volume
//! layouts from its own root and resolves a relative path back against its top
//! directory; a relative path that would climb out of it is refused.
//!
//! Nothing in the write side converts a path to a `String`. An original path
//! is percent-encoded from its bytes and decoded back to bytes, so a file whose
//! name is not valid UTF-8 goes into the trash and comes back out under the
//! name it actually had.

use std::ffi::{OsStr, OsString};
use std::fs::{self, File};
use std::io::Write;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use thiserror::Error;

use files_core::entry::{
    Entry, EntryBody, EntryKind, EntrySize, FileTime, HiddenState, PermissionsSummary, TrashedFacts,
};
use files_core::error::ListingError;
use files_core::listing::{Cancelled, ListingSink};
use files_core::location::LocalPath;

use crate::mounts::MountTable;

/// One trash directory: the home one, or a device's `.Trash-<uid>` or
/// `.Trash/<uid>`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TrashDirectory {
    root: PathBuf,
    /// The top directory of the device this trash belongs to, when the root is
    /// one of the specification's two volume layouts. `None` for the home
    /// trash and for any other directory used as one.
    topdir: Option<PathBuf>,
}

impl TrashDirectory {
    /// A trash directory at `root`.
    ///
    /// A root named `$topdir/.Trash-<digits>` or `$topdir/.Trash/<digits>` is a
    /// volume trash, and knows its top directory from that name alone. That is
    /// what lets a restore or a purge named only by its trash root resolve a
    /// relative record without asking the mount table again.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        let topdir = volume_topdir_of(&root);
        Self { root, topdir }
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

    /// The top directory of the device, for a volume trash.
    pub fn topdir(&self) -> Option<&Path> {
        self.topdir.as_deref()
    }

    /// What a relative `Path` in a record is relative to: the top directory of
    /// a volume trash, and the directory the home trash sits in otherwise,
    /// which is `$XDG_DATA_HOME` as the specification says.
    fn relative_base(&self) -> &Path {
        match &self.topdir {
            Some(topdir) => topdir,
            None => self.root.parent().unwrap_or(Path::new("/")),
        }
    }

    /// Turns a record's `Path` into the absolute path it names.
    ///
    /// A relative path may not contain `..` or any other component that
    /// leaves the base: a record on a removable disk is written by whoever
    /// last had the disk, and a restore must not be steered outside the device
    /// by it.
    fn resolve(&self, recorded: &Path) -> Option<PathBuf> {
        if recorded.is_absolute() {
            return Some(recorded.to_path_buf());
        }
        let mut components = recorded.components().peekable();
        components.peek()?;
        if !components.all(|component| matches!(component, Component::Normal(_))) {
            return None;
        }
        Some(self.relative_base().join(recorded))
    }

    /// What a new record says for `original`: relative to the top directory
    /// for a volume trash, absolute for the home trash.
    fn record_path(&self, original: &Path) -> PathBuf {
        let Some(topdir) = &self.topdir else {
            return original.to_path_buf();
        };
        // The top directory was found from a canonical path, so the item's
        // location is compared in the same form. Only the parent is resolved:
        // trashing a symlink trashes the link, not what it points at.
        let located = match (original.parent(), original.file_name()) {
            (Some(parent), Some(name)) => fs::canonicalize(parent)
                .map(|parent| parent.join(name))
                .unwrap_or_else(|_| original.to_path_buf()),
            _ => original.to_path_buf(),
        };
        match located.strip_prefix(topdir) {
            Ok(relative) if !relative.as_os_str().is_empty() => relative.to_path_buf(),
            _ => original.to_path_buf(),
        }
    }
}

/// The top directory a volume trash root implies, if it is one.
fn volume_topdir_of(root: &Path) -> Option<PathBuf> {
    let name = root.file_name()?.as_bytes();
    let parent = root.parent()?;
    if let Some(uid) = name.strip_prefix(b".Trash-") {
        if is_uid(uid) {
            return Some(parent.to_path_buf());
        }
    }
    if is_uid(name) && parent.file_name().map(OsStrExt::as_bytes) == Some(b".Trash") {
        return parent.parent().map(Path::to_path_buf);
    }
    None
}

fn is_uid(bytes: &[u8]) -> bool {
    !bytes.is_empty() && bytes.iter().all(u8::is_ascii_digit)
}

/// The trash root a stored item lives under: `<root>/files/<item>`.
///
/// A listing that merges several trashes hands out entries whose trash is only
/// known from where their bytes are, and this is the one place that knows the
/// layout well enough to walk back from there.
pub fn trash_root_of(stored_path: &Path) -> Option<PathBuf> {
    let files = stored_path.parent()?;
    if files.file_name() != Some(OsStr::new("files")) {
        return None;
    }
    files.parent().map(Path::to_path_buf)
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
        let Some(info) = parse_trash_info(&contents).and_then(|info| {
            Some(TrashInfo {
                original_path: trash.resolve(&info.original_path)?,
                ..info
            })
        }) else {
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
        if bytes[index] == b'%'
            && index + 2 < bytes.len()
            && let Ok(byte) = u8::from_str_radix(&value[index + 1..index + 3], 16)
        {
            out.push(byte);
            index += 3;
            continue;
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
    /// A per-user trash directory exists and must not be used: it is a
    /// symbolic link, not a directory, or owned by someone else.
    #[error("files.trash.error.unusable:{}:{reason}", .path.to_string_lossy())]
    Unusable { path: PathBuf, reason: String },
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
        percent_encode(&trash.record_path(&original)),
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
    parse_trash_info(&contents)
        .and_then(|info| trash.resolve(&info.original_path))
        .ok_or_else(|| TrashError::NoRecord {
            item: item.to_string(),
        })
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

// --- Per-volume trash ------------------------------------------------------

/// Answers which device a path is on.
///
/// The host's answer is `lstat`'s `st_dev`. The seam exists because a second
/// device is something a test suite cannot mount without privilege, and the
/// walk that finds a top directory only does anything interesting when there
/// is one.
pub trait DeviceProbe {
    fn device_of(&self, path: &Path) -> std::io::Result<u64>;
}

/// The running host's devices.
#[derive(Clone, Copy, Debug, Default)]
pub struct HostDevices;

impl DeviceProbe for HostDevices {
    fn device_of(&self, path: &Path) -> std::io::Result<u64> {
        fs::symlink_metadata(path).map(|metadata| metadata.dev())
    }
}

/// The mount point of the device `directory` is on: the highest ancestor still
/// on the same device.
///
/// The walk is lexical, so `directory` must be canonical. Walking `..` through
/// a symbolic link would climb a different tree from the one the kernel sees.
pub fn top_directory(directory: &Path, probe: &dyn DeviceProbe) -> std::io::Result<PathBuf> {
    let device = probe.device_of(directory)?;
    let mut top = directory;
    while let Some(parent) = top.parent() {
        if probe.device_of(parent)? != device {
            break;
        }
        top = parent;
    }
    Ok(top.to_path_buf())
}

/// Whether `item` is on the same device as `trash`, so trashing it there is a
/// `rename(2)`.
///
/// The trash root may not exist yet on a fresh account, so its device is read
/// from the nearest ancestor that does.
pub fn shares_device(
    trash: &TrashDirectory,
    item: &Path,
    probe: &dyn DeviceProbe,
) -> std::io::Result<bool> {
    let parent = canonical_parent(item)?;
    let item_device = probe.device_of(&parent)?;
    let mut candidate = Some(trash.root());
    while let Some(path) = candidate {
        match probe.device_of(path) {
            Ok(device) => return Ok(device == item_device),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                candidate = path.parent();
            }
            Err(error) => return Err(error),
        }
    }
    Err(std::io::Error::from(std::io::ErrorKind::NotFound))
}

fn canonical_parent(item: &Path) -> std::io::Result<PathBuf> {
    let parent = absolute(item)
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("/"));
    fs::canonicalize(parent)
}

/// What `$topdir/.Trash`, the administrator's shared trash, turned out to be.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SharedTrash {
    /// A real directory with the sticky bit: used through `.Trash/$uid`.
    Usable,
    /// Not there. The ordinary case on a removable disk.
    Missing,
    /// A symbolic link. Refused: it could point anywhere, including into
    /// another user's files.
    Symlink,
    NotADirectory,
    /// A directory without the sticky bit. Refused: without it any user could
    /// delete or replace another user's trashed files.
    NotSticky,
    /// Could not be examined.
    Unreadable,
    /// Usable, but this user's directory inside it was not: a symbolic link,
    /// not a directory, or owned by someone else.
    UserDirectoryUnusable,
}

impl SharedTrash {
    /// A stable key for the operation log.
    pub fn key(self) -> &'static str {
        match self {
            SharedTrash::Usable => "files.trash.shared.usable",
            SharedTrash::Missing => "files.trash.shared.missing",
            SharedTrash::Symlink => "files.trash.shared.symlink",
            SharedTrash::NotADirectory => "files.trash.shared.not_a_directory",
            SharedTrash::NotSticky => "files.trash.shared.not_sticky",
            SharedTrash::Unreadable => "files.trash.shared.unreadable",
            SharedTrash::UserDirectoryUnusable => "files.trash.shared.user_directory_unusable",
        }
    }
}

/// Checks `$topdir/.Trash` the way the specification requires before anything
/// is put in it or taken out of it.
pub fn shared_trash_status(topdir: &Path) -> SharedTrash {
    match fs::symlink_metadata(topdir.join(".Trash")) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => SharedTrash::Missing,
        Err(_) => SharedTrash::Unreadable,
        Ok(metadata) if metadata.file_type().is_symlink() => SharedTrash::Symlink,
        Ok(metadata) if !metadata.is_dir() => SharedTrash::NotADirectory,
        Ok(metadata) if metadata.mode() & STICKY == 0 => SharedTrash::NotSticky,
        Ok(_) => SharedTrash::Usable,
    }
}

const STICKY: u32 = 0o1000;

/// The trash a device's items go to, and why it is that one.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VolumeTrash {
    pub directory: TrashDirectory,
    /// What the shared `.Trash` was. Anything other than `Usable` or
    /// `Missing` is a check that failed, which the specification says to
    /// report; the caller logs it.
    pub shared: SharedTrash,
}

/// Picks, and creates when needed, the trash on the device whose top directory
/// is `topdir`.
///
/// `$topdir/.Trash/$uid` when the shared `.Trash` passes its checks, and
/// `$topdir/.Trash-$uid` otherwise. Either per-user directory must be a real
/// directory owned by `uid`; a missing one is created with mode 0700.
pub fn volume_trash(topdir: &Path, uid: u32) -> Result<VolumeTrash, TrashError> {
    let mut shared = shared_trash_status(topdir);
    if shared == SharedTrash::Usable {
        let root = topdir.join(".Trash").join(uid.to_string());
        if ensure_private_directory(&root, uid).is_ok() {
            return Ok(VolumeTrash {
                directory: TrashDirectory::new(root),
                shared,
            });
        }
        shared = SharedTrash::UserDirectoryUnusable;
    }
    let root = topdir.join(format!(".Trash-{uid}"));
    ensure_private_directory(&root, uid)?;
    Ok(VolumeTrash {
        directory: TrashDirectory::new(root),
        shared,
    })
}

/// The volume trash for `item`, found from the device its directory is on.
pub fn volume_trash_for(
    item: &Path,
    uid: u32,
    probe: &dyn DeviceProbe,
) -> Result<VolumeTrash, TrashError> {
    let parent = canonical_parent(item).map_err(|error| TrashError::io(item, &error))?;
    let topdir = top_directory(&parent, probe).map_err(|error| TrashError::io(&parent, &error))?;
    volume_trash(&topdir, uid)
}

/// Makes sure a per-user trash directory is one this user may use, creating it
/// with mode 0700 when it is missing.
fn ensure_private_directory(path: &Path, uid: u32) -> Result<(), TrashError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => check_private_directory(path, &metadata, uid),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            match fs::DirBuilder::new().mode(0o700).create(path) {
                Ok(()) => {
                    // The umask can only have removed bits, but it could have
                    // removed the owner's own; the mode is set, not hoped for.
                    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
                        .map_err(|error| TrashError::io(path, &error))
                }
                // Somebody else created it in between: check what they made.
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    let metadata =
                        fs::symlink_metadata(path).map_err(|error| TrashError::io(path, &error))?;
                    check_private_directory(path, &metadata, uid)
                }
                Err(error) => Err(TrashError::io(path, &error)),
            }
        }
        Err(error) => Err(TrashError::io(path, &error)),
    }
}

fn check_private_directory(
    path: &Path,
    metadata: &fs::Metadata,
    uid: u32,
) -> Result<(), TrashError> {
    let reason = if metadata.file_type().is_symlink() {
        "symlink"
    } else if !metadata.is_dir() {
        "not_a_directory"
    } else if metadata.uid() != uid {
        "wrong_owner"
    } else {
        return Ok(());
    };
    Err(TrashError::Unusable {
        path: path.to_path_buf(),
        reason: reason.to_string(),
    })
}

/// Filesystems that are never a user's storage, so their mount points are not
/// searched for a trash. Kernel interfaces, automount triggers — asking one
/// about `.Trash-$uid` would mount something — package images, and FUSE views
/// of storage that is already mounted elsewhere.
const NOT_STORAGE: &[&str] = &[
    "autofs",
    "binfmt_misc",
    "bpf",
    "cgroup",
    "cgroup2",
    "configfs",
    "debugfs",
    "devpts",
    "devtmpfs",
    "efivarfs",
    "fuse.gvfsd-fuse",
    "fuse.portal",
    "fusectl",
    "hugetlbfs",
    "mqueue",
    "nsfs",
    "proc",
    "pstore",
    "rpc_pipefs",
    "securityfs",
    "squashfs",
    "sysfs",
    "tracefs",
];

/// Every existing volume trash this user can read, one per volume.
///
/// Nothing is created here. A shared `.Trash` that fails its checks is not
/// read, for the same reason it is not written. A volume mounted twice — a
/// bind mount — lists once.
pub fn volume_trashes(mounts: &MountTable, uid: u32) -> Vec<TrashDirectory> {
    let mut seen = std::collections::HashSet::new();
    let mut found = Vec::new();
    for mount in mounts.mounts() {
        if NOT_STORAGE.contains(&mount.filesystem.as_str()) {
            continue;
        }
        let topdir = &mount.mount_point;
        let mut candidates = Vec::with_capacity(2);
        if shared_trash_status(topdir) == SharedTrash::Usable {
            candidates.push(topdir.join(".Trash").join(uid.to_string()));
        }
        candidates.push(topdir.join(format!(".Trash-{uid}")));
        for root in candidates {
            let Ok(metadata) = fs::symlink_metadata(&root) else {
                continue;
            };
            if check_private_directory(&root, &metadata, uid).is_err() {
                continue;
            }
            let trash = TrashDirectory::new(&root);
            if fs::read_dir(trash.info_dir()).is_err() {
                continue;
            }
            if seen.insert((metadata.dev(), metadata.ino())) {
                found.push(trash);
            }
        }
    }
    found
}

/// The real user ID of this process, from `/proc/self/status`.
///
/// Read from the kernel's status file rather than through a C binding, which
/// this crate does not otherwise need. `None` only when `/proc` is not
/// mounted, and a caller then has no volume trash to offer.
pub fn current_uid() -> Option<u32> {
    parse_status_uid(&fs::read_to_string("/proc/self/status").ok()?)
}

/// The first field of the `Uid:` line, which is the real user ID — the one the
/// specification's `$uid` means.
fn parse_status_uid(status: &str) -> Option<u32> {
    status
        .lines()
        .find_map(|line| line.strip_prefix("Uid:"))?
        .split_whitespace()
        .next()?
        .parse()
        .ok()
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

    // --- Per-volume trash -------------------------------------------------

    use std::collections::HashMap;

    /// A device map a test controls: the longest matching prefix names the
    /// device, the way a mount table would if the suite could mount anything.
    struct FakeDevices {
        devices: HashMap<PathBuf, u64>,
    }

    impl FakeDevices {
        fn new(entries: &[(&Path, u64)]) -> Self {
            Self {
                devices: entries
                    .iter()
                    .map(|(path, device)| (path.to_path_buf(), *device))
                    .collect(),
            }
        }
    }

    impl DeviceProbe for FakeDevices {
        fn device_of(&self, path: &Path) -> std::io::Result<u64> {
            self.devices
                .iter()
                .filter(|(prefix, _)| path.starts_with(prefix))
                .max_by_key(|(prefix, _)| prefix.as_os_str().len())
                .map(|(_, device)| *device)
                .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound))
        }
    }

    fn uid() -> u32 {
        current_uid().expect("this process has a uid")
    }

    #[test]
    fn the_top_directory_is_where_the_device_number_changes() {
        let devices = FakeDevices::new(&[(Path::new("/"), 1), (Path::new("/media/user/STICK"), 7)]);
        assert_eq!(
            top_directory(Path::new("/media/user/STICK/photos/2024"), &devices).unwrap(),
            Path::new("/media/user/STICK")
        );
        // A directory that is itself the mount point is its own top.
        assert_eq!(
            top_directory(Path::new("/media/user/STICK"), &devices).unwrap(),
            Path::new("/media/user/STICK")
        );
        // Everything on the root device walks all the way up.
        assert_eq!(
            top_directory(Path::new("/home/user/Documents"), &devices).unwrap(),
            Path::new("/")
        );
    }

    #[test]
    fn a_device_that_cannot_be_asked_about_stops_the_walk_with_an_error() {
        let devices = FakeDevices::new(&[(Path::new("/media"), 3)]);
        assert!(top_directory(Path::new("/media/user"), &devices).is_err());
    }

    #[test]
    fn the_shared_trash_is_refused_when_missing_a_symlink_or_not_sticky() {
        let top = tempfile::tempdir().unwrap();
        assert_eq!(shared_trash_status(top.path()), SharedTrash::Missing);

        let elsewhere = top.path().join("elsewhere");
        fs::create_dir(&elsewhere).unwrap();
        fs::set_permissions(&elsewhere, fs::Permissions::from_mode(0o1777)).unwrap();
        std::os::unix::fs::symlink(&elsewhere, top.path().join(".Trash")).unwrap();
        assert_eq!(shared_trash_status(top.path()), SharedTrash::Symlink);
        fs::remove_file(top.path().join(".Trash")).unwrap();

        fs::write(top.path().join(".Trash"), b"not a directory").unwrap();
        assert_eq!(shared_trash_status(top.path()), SharedTrash::NotADirectory);
        fs::remove_file(top.path().join(".Trash")).unwrap();

        fs::create_dir(top.path().join(".Trash")).unwrap();
        fs::set_permissions(top.path().join(".Trash"), fs::Permissions::from_mode(0o777)).unwrap();
        assert_eq!(shared_trash_status(top.path()), SharedTrash::NotSticky);

        fs::set_permissions(
            top.path().join(".Trash"),
            fs::Permissions::from_mode(0o1777),
        )
        .unwrap();
        assert_eq!(shared_trash_status(top.path()), SharedTrash::Usable);
    }

    #[test]
    fn a_volume_without_a_usable_shared_trash_gets_a_private_one_with_mode_0700() {
        let top = tempfile::tempdir().unwrap();
        // Present but not sticky: the specification says it must not be used.
        fs::create_dir(top.path().join(".Trash")).unwrap();
        fs::set_permissions(top.path().join(".Trash"), fs::Permissions::from_mode(0o777)).unwrap();

        let uid = uid();
        let volume = volume_trash(top.path(), uid).unwrap();
        assert_eq!(volume.shared, SharedTrash::NotSticky);
        let expected = top.path().join(format!(".Trash-{uid}"));
        assert_eq!(volume.directory.root(), expected);
        assert_eq!(volume.directory.topdir(), Some(top.path()));
        let mode = fs::metadata(&expected).unwrap().permissions().mode() & 0o7777;
        assert_eq!(mode, 0o700);
        assert!(
            !top.path().join(".Trash").join(uid.to_string()).exists(),
            "nothing was created inside the rejected shared trash"
        );
    }

    #[test]
    fn a_sticky_shared_trash_is_used_through_a_per_user_directory() {
        let top = tempfile::tempdir().unwrap();
        fs::create_dir(top.path().join(".Trash")).unwrap();
        fs::set_permissions(
            top.path().join(".Trash"),
            fs::Permissions::from_mode(0o1777),
        )
        .unwrap();

        let uid = uid();
        let volume = volume_trash(top.path(), uid).unwrap();
        assert_eq!(volume.shared, SharedTrash::Usable);
        assert_eq!(
            volume.directory.root(),
            top.path().join(".Trash").join(uid.to_string())
        );
        assert_eq!(volume.directory.topdir(), Some(top.path()));
        assert!(!top.path().join(format!(".Trash-{uid}")).exists());
    }

    #[test]
    fn a_private_trash_that_is_a_symlink_is_refused_rather_than_followed() {
        let top = tempfile::tempdir().unwrap();
        let uid = uid();
        let decoy = top.path().join("decoy");
        fs::create_dir(&decoy).unwrap();
        std::os::unix::fs::symlink(&decoy, top.path().join(format!(".Trash-{uid}"))).unwrap();
        assert!(volume_trash(top.path(), uid).is_err());
        assert_eq!(fs::read_dir(&decoy).unwrap().count(), 0);
    }

    #[test]
    fn a_volume_trash_records_the_path_relative_to_its_top_directory() {
        let top = tempfile::tempdir().unwrap();
        let folder = top.path().join("photos");
        fs::create_dir(&folder).unwrap();
        let file = folder.join("beach.jpg");
        fs::write(&file, b"jpeg").unwrap();

        let volume = volume_trash(top.path(), uid()).unwrap().directory;
        let item = move_to_trash(&volume, &file).unwrap();
        let record = fs::read_to_string(&item.info_path).unwrap();
        assert!(
            record.contains("\nPath=photos/beach.jpg\n"),
            "expected a relative path, got {record}"
        );
        // The original path callers see is absolute again.
        assert_eq!(item.original_path, file);
        assert_eq!(original_path_of(&volume, &item.item).unwrap(), file);

        // The read side resolves it against the top directory too.
        let (entries, skipped) = list(&volume);
        assert!(skipped.is_empty());
        match &entries[0].body {
            EntryBody::Trashed(facts) => assert_eq!(facts.original_path, file),
            other => panic!("expected a trashed entry, got {other:?}"),
        }

        assert_eq!(restore(&volume, &item.item).unwrap(), file);
        assert_eq!(fs::read(&file).unwrap(), b"jpeg");
    }

    #[test]
    fn a_trash_directory_named_by_its_root_knows_its_top_directory() {
        let uid = uid();
        let private = TrashDirectory::new(format!("/media/user/STICK/.Trash-{uid}"));
        assert_eq!(private.topdir(), Some(Path::new("/media/user/STICK")));
        let shared = TrashDirectory::new(format!("/media/user/STICK/.Trash/{uid}"));
        assert_eq!(shared.topdir(), Some(Path::new("/media/user/STICK")));
        let home = TrashDirectory::new("/home/user/.local/share/Trash");
        assert_eq!(home.topdir(), None);
        assert_eq!(
            trash_root_of(Path::new("/media/user/STICK/.Trash-1000/files/a.txt")),
            Some(PathBuf::from("/media/user/STICK/.Trash-1000"))
        );
    }

    #[test]
    fn a_relative_record_that_climbs_out_of_its_top_directory_is_refused() {
        let top = tempfile::tempdir().unwrap();
        let volume = volume_trash(top.path(), uid()).unwrap().directory;
        ensure_trash(&volume).unwrap();
        add_item(
            &volume,
            "escape",
            "[Trash Info]\nPath=../../etc/passwd\nDeletionDate=2024-01-01T00:00:00\n",
            b"x",
        );
        let (entries, skipped) = list(&volume);
        assert!(entries.is_empty());
        assert_eq!(skipped, ["escape"]);
        assert!(matches!(
            original_path_of(&volume, "escape"),
            Err(TrashError::NoRecord { .. })
        ));
    }

    #[test]
    fn a_relative_record_in_the_home_trash_is_read_against_the_data_directory() {
        let (root, trash) = fixture();
        add_item(
            &trash,
            "kept.txt",
            "[Trash Info]\nPath=notes/kept.txt\nDeletionDate=2024-01-01T00:00:00\n",
            b"x",
        );
        assert_eq!(
            original_path_of(&trash, "kept.txt").unwrap(),
            root.path().join("notes/kept.txt")
        );
    }

    #[test]
    fn every_readable_volume_trash_is_found_from_the_mount_table() {
        let root = tempfile::tempdir().unwrap();
        let uid = uid();
        let stick = root.path().join("stick");
        let disk = root.path().join("disk");
        let empty = root.path().join("empty");
        let proc_like = root.path().join("proc");
        for path in [&stick, &disk, &empty, &proc_like] {
            fs::create_dir(path).unwrap();
        }
        // A private trash on one volume, a shared one on another, none on a
        // third, and a trash-shaped directory on a pseudo filesystem that must
        // not be looked at.
        fs::create_dir_all(stick.join(format!(".Trash-{uid}/info"))).unwrap();
        fs::create_dir(disk.join(".Trash")).unwrap();
        fs::set_permissions(disk.join(".Trash"), fs::Permissions::from_mode(0o1777)).unwrap();
        fs::create_dir_all(disk.join(format!(".Trash/{uid}/info"))).unwrap();
        fs::create_dir_all(proc_like.join(format!(".Trash-{uid}/info"))).unwrap();

        let mountinfo = root.path().join("mountinfo");
        fs::write(
            &mountinfo,
            format!(
                "1 0 8:1 / {} rw - vfat /dev/sdb1 rw\n\
                 2 0 8:2 / {} rw - ext4 /dev/sdc1 rw\n\
                 3 0 8:3 / {} rw - ext4 /dev/sdd1 rw\n\
                 4 0 0:5 / {} rw - proc proc rw\n\
                 5 0 8:1 / {} rw - vfat /dev/sdb1 rw\n",
                stick.display(),
                disk.display(),
                empty.display(),
                proc_like.display(),
                // The same volume mounted twice lists once.
                stick.display(),
            ),
        )
        .unwrap();
        let table = crate::mounts::read_mount_table(&mountinfo);
        let mut roots: Vec<PathBuf> = volume_trashes(&table, uid)
            .into_iter()
            .map(|trash| trash.root().to_path_buf())
            .collect();
        roots.sort();
        let mut expected = vec![
            stick.join(format!(".Trash-{uid}")),
            disk.join(format!(".Trash/{uid}")),
        ];
        expected.sort();
        assert_eq!(roots, expected);
        // Listing never creates a trash.
        assert!(!empty.join(format!(".Trash-{uid}")).exists());
    }

    #[test]
    fn the_uid_is_the_real_one_from_the_status_file() {
        let status = "Name:\tbash\nUid:\t1000\t0\t0\t0\nGid:\t1000\t1000\t1000\t1000\n";
        assert_eq!(parse_status_uid(status), Some(1000));
        assert_eq!(parse_status_uid("Name:\tbash\n"), None);
        assert_eq!(
            current_uid(),
            Some(fs::metadata("/proc/self").unwrap().uid()),
            "the running process reports the uid it owns /proc/self with"
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
