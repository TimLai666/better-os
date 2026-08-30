//! What one row in a listing is.
//!
//! An entry carries typed facts, not strings a view has to re-parse. The
//! central decision is that an entry's body is a closed enum: a filesystem
//! entry has a path, an application entry has a desktop ID, and a trashed
//! entry has a trash item name plus where it came from. There is no accessor
//! that turns the second or third into a path, which is what keeps Issue #4's
//! rule — the Applications location must not invent paths that a program might
//! mistake for executables — true by construction rather than by review.

use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use app_catalog_core::{DesktopId, IconReference, MimeType};

use crate::location::LocalPath;

/// A file timestamp as the filesystem reports it: seconds from the Unix epoch,
/// signed so a file dated before 1970 is representable, plus nanoseconds.
///
/// This is deliberately not `storage_core::Timestamp`, which counts from when
/// a service started observing and would be meaningless for a file written
/// last year.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct FileTime {
    pub seconds: i64,
    pub nanoseconds: u32,
}

impl FileTime {
    pub const EPOCH: FileTime = FileTime {
        seconds: 0,
        nanoseconds: 0,
    };

    pub fn new(seconds: i64, nanoseconds: u32) -> Self {
        Self {
            seconds,
            nanoseconds: nanoseconds.min(999_999_999),
        }
    }

    /// Converts a `SystemTime`, including one before the epoch.
    pub fn from_system_time(time: SystemTime) -> Self {
        match time.duration_since(UNIX_EPOCH) {
            Ok(elapsed) => Self::new(elapsed.as_secs() as i64, elapsed.subsec_nanos()),
            Err(error) => {
                let before = error.duration();
                if before.subsec_nanos() == 0 {
                    Self::new(-(before.as_secs() as i64), 0)
                } else {
                    Self::new(
                        -(before.as_secs() as i64) - 1,
                        1_000_000_000 - before.subsec_nanos(),
                    )
                }
            }
        }
    }

    pub fn to_system_time(self) -> Option<SystemTime> {
        if self.seconds >= 0 {
            UNIX_EPOCH.checked_add(Duration::new(self.seconds as u64, self.nanoseconds))
        } else {
            let seconds = self.seconds.unsigned_abs();
            UNIX_EPOCH
                .checked_sub(Duration::from_secs(seconds))
                .and_then(|time| time.checked_add(Duration::from_nanos(self.nanoseconds as u64)))
        }
    }
}

/// How large an entry is.
///
/// A directory's size is not its contents' total until something walks it, and
/// pretending it is zero sorts every folder to one end. `Unknown` says the
/// question has not been answered yet; `NotApplicable` says it has no answer.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum EntrySize {
    Bytes(u64),
    /// Not measured yet. Deep folder sizes are separate, cancellable work.
    Unknown,
    /// The entry has no size: a virtual application row, a device node.
    NotApplicable,
}

impl EntrySize {
    pub fn bytes(self) -> Option<u64> {
        match self {
            EntrySize::Bytes(value) => Some(value),
            _ => None,
        }
    }
}

/// What the user can do with an entry, summarized from the mode bits and the
/// effective user's relationship to the file.
///
/// A summary, not the raw mode: a view that shows "read-only" needs the answer
/// for this user, and re-deriving it from `0o644` plus an owner UID in three
/// different places is how the three places end up disagreeing.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct PermissionsSummary {
    pub readable: bool,
    pub writable: bool,
    pub executable: bool,
    /// The raw Unix mode when the platform reported one, for the details pane.
    pub mode: Option<u32>,
    /// Whether the containing filesystem is mounted read-only, which makes a
    /// writable-looking file unwritable anyway.
    pub read_only_medium: bool,
}

impl PermissionsSummary {
    pub const UNKNOWN: PermissionsSummary = PermissionsSummary {
        readable: false,
        writable: false,
        executable: false,
        mode: None,
        read_only_medium: false,
    };

    /// Whether the user can actually change this entry, medium included.
    pub fn effectively_writable(self) -> bool {
        self.writable && !self.read_only_medium
    }
}

/// What kind of thing an entry is, after any symlink has been followed.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum EntryKind {
    Directory,
    File,
    /// An installed application, which is a row in the Applications location
    /// and never a file on disk.
    Application,
    /// A socket, FIFO, or device node. Grouped rather than enumerated because
    /// the views treat them identically.
    Special,
    /// The kind could not be determined — a broken link, or a `stat` that
    /// failed with a permission error.
    Unknown,
}

impl EntryKind {
    pub fn is_directory(self) -> bool {
        matches!(self, EntryKind::Directory)
    }
}

/// Where a symlink leads.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SymlinkStatus {
    /// Not a symlink.
    None,
    /// A symlink whose target was resolved. The kind on the entry is the
    /// target's kind; `target` is the link text as stored.
    Resolved { target: PathBuf },
    /// A symlink pointing at something that is not there.
    Broken { target: PathBuf },
    /// A symlink chain that comes back to itself. The entry still appears in
    /// the listing; only its target is unresolvable.
    Loop { target: PathBuf },
}

impl SymlinkStatus {
    pub fn is_symlink(&self) -> bool {
        !matches!(self, SymlinkStatus::None)
    }

    pub fn target(&self) -> Option<&PathBuf> {
        match self {
            SymlinkStatus::None => None,
            SymlinkStatus::Resolved { target }
            | SymlinkStatus::Broken { target }
            | SymlinkStatus::Loop { target } => Some(target),
        }
    }
}

/// Why an entry is hidden.
///
/// Issue #6 requires hidden status to come from platform rules rather than
/// filename inspection alone, so the reason travels with the answer: a view
/// can say "hidden by this folder's `.hidden` file" instead of leaving the
/// user to guess why a normally named file disappeared.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum HiddenReason {
    /// The name begins with a dot.
    Dotfile,
    /// The name is listed in the directory's `.hidden` file.
    DirectoryHiddenFile,
    /// The name ends with `~`, the freedesktop backup convention.
    BackupFile,
}

/// Whether an entry is hidden, and why.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum HiddenState {
    Visible,
    Hidden(HiddenReason),
}

impl HiddenState {
    pub fn is_hidden(self) -> bool {
        matches!(self, HiddenState::Hidden(_))
    }

    pub fn reason(self) -> Option<HiddenReason> {
        match self {
            HiddenState::Hidden(reason) => Some(reason),
            HiddenState::Visible => None,
        }
    }
}

/// Facts about an entry that lives on a filesystem.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileFacts {
    pub path: LocalPath,
    pub symlink: SymlinkStatus,
    /// The device this entry sits on, when the listing was able to attribute
    /// it. Present for anything under an external mount, so a view can mark it
    /// and so a removal check knows which entries are affected.
    pub device: Option<storage_core::IdentityKey>,
}

/// Facts about an installed application, taken from the shared catalog.
///
/// There is no path here and no accessor that produces one. Opening goes
/// through the catalog's desktop ID, which is what
/// [`crate::applications`] turns into a launch intent.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApplicationFacts {
    pub desktop_id: DesktopId,
    pub icon: Option<IconReference>,
    pub categories: Vec<String>,
    pub comment: Option<String>,
}

/// Facts about an item sitting in the trash.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TrashedFacts {
    /// The `.trashinfo` stem, which is the item's identity inside the trash.
    pub item: String,
    /// Where the item was before it was deleted. Shown to the user and used by
    /// restore in ticket 33; it is not where the data is now.
    pub original_path: PathBuf,
    /// When it was deleted, as recorded in the trash info file.
    pub deleted_at: Option<FileTime>,
    /// Where the bytes actually are, under the trash's `files/` directory.
    pub stored_path: LocalPath,
}

/// The typed body of an entry. Adding a location kind that lists something new
/// adds a variant here, and the compiler names every view that has to decide
/// how to draw it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EntryBody {
    File(FileFacts),
    Application(ApplicationFacts),
    Trashed(TrashedFacts),
}

/// The identity a selection is filed under.
///
/// Selection has to survive entries being inserted around it as a listing
/// streams in, so it cannot be an index. Within one listing a name is unique
/// for a directory and a desktop ID is unique for the Applications location.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum EntryId {
    /// A filename, as the directory reported it.
    Name(String),
    /// A desktop ID from the shared catalog.
    Application(String),
    /// A trash item stem.
    TrashItem(String),
}

/// One row in a listing.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Entry {
    /// The display name. For a file this is the filename, lossily decoded when
    /// it is not valid UTF-8 so a badly encoded name is still listed.
    pub name: String,
    pub kind: EntryKind,
    pub size: EntrySize,
    pub modified: Option<FileTime>,
    pub permissions: PermissionsSummary,
    pub hidden: HiddenState,
    /// The detected type, when the listing resolved one. Detection is separate,
    /// schedulable work, so an entry with `None` means "not asked yet".
    pub mime: Option<MimeType>,
    pub body: EntryBody,
}

impl Entry {
    /// A filesystem entry with the fields a `readdir` pass can fill in.
    /// Metadata that costs a `stat` is filled in by the metadata pass.
    pub fn file(name: impl Into<String>, path: LocalPath, kind: EntryKind) -> Self {
        Self {
            name: name.into(),
            kind,
            size: EntrySize::Unknown,
            modified: None,
            permissions: PermissionsSummary::UNKNOWN,
            hidden: HiddenState::Visible,
            mime: None,
            body: EntryBody::File(FileFacts {
                path,
                symlink: SymlinkStatus::None,
                device: None,
            }),
        }
    }

    /// The identity this entry is selected and sorted by.
    pub fn id(&self) -> EntryId {
        match &self.body {
            EntryBody::File(_) => EntryId::Name(self.name.clone()),
            EntryBody::Application(facts) => {
                EntryId::Application(facts.desktop_id.as_str().to_string())
            }
            EntryBody::Trashed(facts) => EntryId::TrashItem(facts.item.clone()),
        }
    }

    /// The filesystem path for this entry, when it has one.
    ///
    /// An application entry answers `None`. There is no other accessor and no
    /// `From<Entry> for PathBuf`, so no consumer can turn an Applications row
    /// into something it passes to `std::fs`.
    pub fn as_local_path(&self) -> Option<&LocalPath> {
        match &self.body {
            EntryBody::File(facts) => Some(&facts.path),
            // A trashed item's bytes are at a real path, but that path is the
            // trash's internal storage, not the item. Restore and delete go
            // through the trash operations in ticket 33.
            EntryBody::Trashed(_) => None,
            EntryBody::Application(_) => None,
        }
    }

    pub fn symlink(&self) -> &SymlinkStatus {
        match &self.body {
            EntryBody::File(facts) => &facts.symlink,
            _ => &SymlinkStatus::None,
        }
    }

    /// The name used for extension-based sorting: everything after the last
    /// dot, empty when there is none. A leading dot is not an extension, so
    /// `.bashrc` sorts with the extensionless entries rather than under `b`.
    pub fn extension(&self) -> &str {
        match self.name.rfind('.') {
            Some(0) | None => "",
            Some(index) => &self.name[index + 1..],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn application_entry() -> Entry {
        Entry {
            name: "Text Editor".to_string(),
            kind: EntryKind::Application,
            size: EntrySize::NotApplicable,
            modified: None,
            permissions: PermissionsSummary::UNKNOWN,
            hidden: HiddenState::Visible,
            mime: None,
            body: EntryBody::Application(ApplicationFacts {
                desktop_id: DesktopId::new("org.example.Editor.desktop").unwrap(),
                icon: None,
                categories: vec!["Utility".to_string()],
                comment: None,
            }),
        }
    }

    #[test]
    fn an_application_entry_has_no_path() {
        assert_eq!(application_entry().as_local_path(), None);
    }

    #[test]
    fn a_file_entry_keeps_the_path_it_was_listed_at() {
        let entry = Entry::file(
            "notes.txt",
            LocalPath::new("/home/user/notes.txt").unwrap(),
            EntryKind::File,
        );
        assert_eq!(
            entry.as_local_path().map(|path| path.as_path()),
            Some(std::path::Path::new("/home/user/notes.txt"))
        );
        assert_eq!(entry.id(), EntryId::Name("notes.txt".to_string()));
    }

    #[test]
    fn a_dotfile_name_has_no_extension() {
        let entry = Entry::file(
            ".bashrc",
            LocalPath::new("/home/user/.bashrc").unwrap(),
            EntryKind::File,
        );
        assert_eq!(entry.extension(), "");
        let archive = Entry::file(
            "backup.tar.gz",
            LocalPath::new("/home/user/backup.tar.gz").unwrap(),
            EntryKind::File,
        );
        assert_eq!(archive.extension(), "gz");
    }

    #[test]
    fn a_pre_epoch_timestamp_round_trips() {
        let time = SystemTime::UNIX_EPOCH - Duration::from_millis(1_500);
        let file_time = FileTime::from_system_time(time);
        assert_eq!(file_time.seconds, -2);
        assert_eq!(file_time.nanoseconds, 500_000_000);
        assert_eq!(file_time.to_system_time(), Some(time));
    }

    #[test]
    fn a_read_only_medium_overrides_a_writable_mode() {
        let permissions = PermissionsSummary {
            readable: true,
            writable: true,
            executable: false,
            mode: Some(0o644),
            read_only_medium: true,
        };
        assert!(!permissions.effectively_writable());
    }
}
