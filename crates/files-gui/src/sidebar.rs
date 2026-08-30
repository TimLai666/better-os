//! The sidebar's four sections, as data.
//!
//! Issue #6 requires built-in locations, devices, Applications, and user
//! Favorites to stay distinct, so [`SidebarSection`] is a closed enum and a row
//! carries the section it belongs to rather than the sidebar inferring one from
//! what the row happens to point at. A pinned Home and the built-in Home are
//! two rows in two sections, and removing the pin does not remove the place.
//!
//! Availability is a state, never a deletion. A bookmark whose folder is gone
//! stays in the list as [`Availability::Unavailable`]; the file on disk is not
//! touched, because "the disk is not plugged in right now" and "the user wants
//! this gone" are different sentences and only the second one is an edit.
//!
//! Devices here are the fallback, for a session where neither the storage
//! service nor an in-process engine can produce a state. It is what a mount
//! table can honestly say: which filesystems are mounted, which of them look
//! external, and how much their identity is worth. Every one of them reads as
//! unknown, and nothing here guesses a readiness state. The real device rows
//! come from [`crate::devices`], which is fed by the storage layer.

use std::collections::HashMap;
use std::path::Path;

use files_core::{LocalPath, Location, TrashLocation};
use files_platform::{MountTable, UserDirectories};
use storage_core::{DeviceStateKind, IdentityKey};

use crate::bookmarks::BookmarkFile;
use crate::i18n::{Copy, user_directory_label};

/// The four sections, in the order they are drawn.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum SidebarSection {
    Places,
    Devices,
    Applications,
    Favorites,
}

impl SidebarSection {
    pub const ALL: [SidebarSection; 4] = [
        SidebarSection::Places,
        SidebarSection::Devices,
        SidebarSection::Applications,
        SidebarSection::Favorites,
    ];

    pub fn title(self, c: &'static Copy) -> &'static str {
        match self {
            SidebarSection::Places => c.sidebar_places,
            SidebarSection::Devices => c.sidebar_devices,
            SidebarSection::Applications => c.sidebar_applications,
            SidebarSection::Favorites => c.sidebar_favorites,
        }
    }
}

/// Whether the row can be opened right now.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Availability {
    Available,
    /// The target is not there. The row stays; the click is refused.
    Unavailable,
}

impl Availability {
    pub fn is_available(self) -> bool {
        matches!(self, Availability::Available)
    }
}

/// One sidebar row.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SidebarRow {
    pub section: SidebarSection,
    /// Stable within one build of the sidebar, used as the element id.
    pub key: String,
    pub label: String,
    pub location: Location,
    pub availability: Availability,
    /// The row's position among the Favorites, for reorder and removal. `None`
    /// for every row that is not a bookmark.
    pub bookmark_index: Option<usize>,
    /// What a device row can say about pulling the cable out.
    pub device_state: Option<DeviceStateKind>,
    /// True when the device's identity only holds for this connection.
    pub identity_volatile: bool,
}

impl SidebarRow {
    /// Whether a right-click offers the bookmark actions.
    pub fn is_bookmark(&self) -> bool {
        self.bookmark_index.is_some()
    }
}

/// Where a device's Direct Removal state comes from.
///
/// A trait rather than a concrete type so the fallback can be tested with a
/// state the host does not have.
pub trait DeviceStates {
    fn state_of(&self, key: &IdentityKey) -> Option<DeviceStateKind>;
}

/// What the sidebar knows when nothing is supplying device states: nothing.
///
/// Every device reads as unknown, which is the honest answer and the one that
/// keeps a row from claiming a disk is safe to unplug.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoDeviceStates;

impl DeviceStates for NoDeviceStates {
    fn state_of(&self, _key: &IdentityKey) -> Option<DeviceStateKind> {
        None
    }
}

impl DeviceStates for HashMap<IdentityKey, DeviceStateKind> {
    fn state_of(&self, key: &IdentityKey) -> Option<DeviceStateKind> {
        self.get(key).copied()
    }
}

/// Whether a path is there and usable. Injected so the availability tests do
/// not need a real filesystem, and so a slow network mount can be probed
/// somewhere other than the render thread later.
pub trait LocationProbe {
    fn is_available(&self, path: &Path) -> bool;
}

/// The real answer, from the filesystem.
#[derive(Clone, Copy, Debug, Default)]
pub struct FilesystemProbe;

impl LocationProbe for FilesystemProbe {
    fn is_available(&self, path: &Path) -> bool {
        path.is_dir()
    }
}

/// A probe backed by a fixed set of present paths.
#[derive(Clone, Debug, Default)]
pub struct FixedProbe {
    present: Vec<std::path::PathBuf>,
}

impl FixedProbe {
    pub fn with(paths: impl IntoIterator<Item = std::path::PathBuf>) -> Self {
        Self {
            present: paths.into_iter().collect(),
        }
    }
}

impl LocationProbe for FixedProbe {
    fn is_available(&self, path: &Path) -> bool {
        self.present.iter().any(|present| present == path)
    }
}

/// Everything the sidebar needs to draw itself.
pub struct SidebarInputs<'a> {
    pub directories: &'a UserDirectories,
    pub mounts: &'a MountTable,
    pub bookmarks: &'a BookmarkFile,
    pub states: &'a dyn DeviceStates,
    pub probe: &'a dyn LocationProbe,
}

/// Builds the rows for one frame.
pub fn build_rows(inputs: &SidebarInputs<'_>, c: &'static Copy) -> Vec<SidebarRow> {
    let mut rows = Vec::new();

    for resolved in inputs.directories.sidebar() {
        let available = resolved.present
            && resolved
                .location
                .as_local_path()
                .is_some_and(|path| inputs.probe.is_available(path.as_path()));
        rows.push(SidebarRow {
            section: SidebarSection::Places,
            key: format!("place-{}", resolved.directory.key()),
            label: user_directory_label(resolved.directory, c).to_string(),
            location: resolved.location.clone(),
            availability: availability(available),
            bookmark_index: None,
            device_state: None,
            identity_volatile: false,
        });
    }
    // Trash is a place, not a device and not a folder. It is always reachable:
    // an empty trash is still the Trash.
    rows.push(SidebarRow {
        section: SidebarSection::Places,
        key: "place-trash".to_string(),
        label: c.place_trash.to_string(),
        location: Location::Trash(TrashLocation::Root),
        availability: Availability::Available,
        bookmark_index: None,
        device_state: None,
        identity_volatile: false,
    });

    for mount in inputs.mounts.external_mounts() {
        let key = mount.identity.key();
        rows.push(SidebarRow {
            section: SidebarSection::Devices,
            key: format!("device-{}", key.as_str()),
            label: device_label(mount.mount_point.as_path(), &mount.identity.display_name()),
            location: Location::Local(
                LocalPath::new(mount.mount_point.clone()).unwrap_or_else(|_| LocalPath::root()),
            ),
            availability: availability(inputs.probe.is_available(&mount.mount_point)),
            bookmark_index: None,
            device_state: Some(
                inputs
                    .states
                    .state_of(key)
                    .unwrap_or(DeviceStateKind::Unknown),
            ),
            identity_volatile: !mount.identity.confidence().persistable(),
        });
    }

    rows.push(SidebarRow {
        section: SidebarSection::Applications,
        key: "applications".to_string(),
        label: c.sidebar_applications.to_string(),
        location: Location::Applications,
        availability: Availability::Available,
        bookmark_index: None,
        device_state: None,
        identity_volatile: false,
    });

    for (index, bookmark) in inputs.bookmarks.bookmarks().iter().enumerate() {
        let available = bookmark
            .path()
            .is_some_and(|path| inputs.probe.is_available(path));
        rows.push(SidebarRow {
            section: SidebarSection::Favorites,
            key: format!("favorite-{index}"),
            label: bookmark.display_name(),
            location: bookmark.location().clone(),
            availability: availability(available),
            bookmark_index: Some(index),
            device_state: None,
            identity_volatile: false,
        });
    }

    rows
}

/// The rows of one section, in order.
pub fn section_rows(
    rows: &[SidebarRow],
    section: SidebarSection,
) -> impl Iterator<Item = &SidebarRow> {
    rows.iter().filter(move |row| row.section == section)
}

fn availability(available: bool) -> Availability {
    if available {
        Availability::Available
    } else {
        Availability::Unavailable
    }
}

/// The name a device row shows.
///
/// The mount point's own last component is what the user recognizes — a disk
/// mounted at `/media/tim/PHOTOS` is "PHOTOS" — and the identity's display name
/// is the fallback when there is no such component. Neither is a UDisks2 object
/// path or a `/dev` node, which Issue #6 keeps off the normal path.
fn device_label(mount_point: &Path, identity_name: &str) -> String {
    mount_point
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| identity_name.to_string())
}
