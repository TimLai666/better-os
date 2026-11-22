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

/// The four kinds of row [`build_rows`] makes. The order the navigation pane
/// draws its groups in is [`NAV_ORDER`], not this enum's.
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

/// One group of lines in the navigation pane, under its own header or
/// separator.
///
/// The pane draws more than the four row sections: the Dropbox folder, each
/// plugin's section, and the tailnet's machines come from elsewhere, and they
/// are ordered here with the rest so the order is decided once.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum NavGroup {
    /// Home, the person's folders, Recent, Trash.
    Places,
    /// The pinned folders.
    Favorites,
    /// The Applications view, after a separator.
    Applications,
    /// The Dropbox folder, when Dropbox is installed.
    Dropbox,
    /// Each plugin's section, while it has items.
    Plugins,
    /// This PC and every drive.
    Devices,
    /// The tailnet's online machines.
    Tailnet,
}

/// The navigation pane's groups, top to bottom, and the only place that
/// order is written. Drives sit above the tailnet's machines (ticket 132):
/// the disks in the computer come before other computers. Keyboard
/// navigation moves through the lines in the order they are drawn, so it
/// follows this too.
pub const NAV_ORDER: [NavGroup; 7] = [
    NavGroup::Places,
    NavGroup::Favorites,
    NavGroup::Applications,
    NavGroup::Dropbox,
    NavGroup::Plugins,
    NavGroup::Devices,
    NavGroup::Tailnet,
];

/// The navigation pane's lines: `lines_of` builds one group's lines, and is
/// asked for every group once, in [`NAV_ORDER`]; the result is those lines
/// in that order. `FilesApp::nav_targets` is this call, so the order a
/// window draws is the order tested here.
pub fn in_nav_order<T>(mut lines_of: impl FnMut(NavGroup) -> Vec<T>) -> Vec<T> {
    NAV_ORDER.into_iter().flat_map(&mut lines_of).collect()
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
    /// For a favourite on a server, whether its connection is there
    /// (ticket 310); `None` for anything else, or not known yet.
    pub connected: Option<bool>,
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

/// The window's probe (ticket 310): a folder on a local disk is looked at
/// at once; one on a network or FUSE filesystem never on the calling
/// thread. Its answer is the last a probe thread of its own brought back
/// (every 10 s at most, one thread per folder at a time), and until the
/// first one comes back it is taken as there, so a dead mount never holds
/// the sidebar up and never draws a row "Unavailable" that has not been
/// looked at.
#[derive(Clone, Default)]
pub struct BackgroundProbe {
    table: std::sync::Arc<std::sync::Mutex<Option<(std::time::Instant, MountTable)>>>,
    known:
        std::sync::Arc<std::sync::Mutex<HashMap<std::path::PathBuf, (std::time::Instant, bool)>>>,
    running: std::sync::Arc<std::sync::Mutex<std::collections::HashSet<std::path::PathBuf>>>,
}

/// How often a folder on a network or FUSE filesystem is looked at again.
const BACKGROUND_PROBE_AGE: std::time::Duration = std::time::Duration::from_secs(10);

/// How old the mount table may be before it is read again.
const MOUNT_TABLE_AGE: std::time::Duration = std::time::Duration::from_secs(5);

impl BackgroundProbe {
    fn reach(&self, path: &Path) -> files_platform::Reach {
        let mut table = self.table.lock().expect("mount table");
        if table
            .as_ref()
            .is_none_or(|(read, _)| read.elapsed() >= MOUNT_TABLE_AGE)
        {
            // `/proc/self/mountinfo`: the kernel's own list, no filesystem
            // is touched to read it.
            *table = Some((std::time::Instant::now(), MountTable::from_env()));
        }
        table
            .as_ref()
            .map_or(files_platform::Reach::Local, |(_, table)| table.reach(path))
    }

    /// The last answer for `path` (`true` until there is one), starting a
    /// look when it is due.
    fn remembered(&self, path: &Path) -> bool {
        let known = self.known.lock().expect("probe answers").get(path).copied();
        if known.is_none_or(|(at, _)| at.elapsed() >= BACKGROUND_PROBE_AGE)
            && self
                .running
                .lock()
                .expect("probes")
                .insert(path.to_path_buf())
        {
            let (path_owned, answers, running) = (
                path.to_path_buf(),
                std::sync::Arc::clone(&self.known),
                std::sync::Arc::clone(&self.running),
            );
            let spawned = std::thread::Builder::new()
                .name("files-sidebar-probe".to_string())
                .spawn(move || {
                    let there = path_owned.is_dir();
                    answers
                        .lock()
                        .expect("probe answers")
                        .insert(path_owned.clone(), (std::time::Instant::now(), there));
                    running.lock().expect("probes").remove(&path_owned);
                });
            if spawned.is_err() {
                self.running.lock().expect("probes").remove(path);
            }
        }
        known.is_none_or(|(_, there)| there)
    }
}

impl LocationProbe for BackgroundProbe {
    fn is_available(&self, path: &Path) -> bool {
        match self.reach(path) {
            files_platform::Reach::Local => path.is_dir(),
            files_platform::Reach::Network | files_platform::Reach::Fuse => self.remembered(path),
        }
    }
}

/// Whether a location is on a server and whether it is connected, as the
/// sidebar needs it (ticket 310).
pub trait RemoteState {
    /// `None`: not on a server. `Some(None)`: on one, not known yet.
    /// `Some(Some(connected))` otherwise.
    fn connection(&self, location: &Location) -> Option<Option<bool>>;
}

/// Nothing is on a server.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoRemote;

impl RemoteState for NoRemote {
    fn connection(&self, _location: &Location) -> Option<Option<bool>> {
        None
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
    /// Which favourites are on servers, and connected (ticket 310).
    pub remote: &'a dyn RemoteState,
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
            connected: None,
        });
    }
    // Recents: the files opened lately, from the shared GTK list, as in
    // folio's sidebar.
    rows.push(SidebarRow {
        section: SidebarSection::Places,
        key: "place-recent".to_string(),
        label: c.place_recent.to_string(),
        location: Location::Recent,
        availability: Availability::Available,
        bookmark_index: None,
        device_state: None,
        identity_volatile: false,
        connected: None,
    });
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
        connected: None,
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
            connected: None,
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
        connected: None,
    });

    // A bookmark of a user folder the Places section already lists (the ones
    // xdg-user-dirs writes at login: Documents, Music, …) is not shown again
    // under Favorites, as GNOME Files' own sidebar skips it. The line stays in
    // the shared file and keeps its index, so reordering is unaffected. Home
    // is not one of them: xdg-user-dirs never writes it, so a pinned Home is
    // the person's own pin and stays a Favorites row (see the module doc).
    let home = inputs.directories.home();
    let places: Vec<Location> = rows
        .iter()
        .filter(|row| row.section == SidebarSection::Places)
        .map(|row| row.location.clone())
        .filter(|location| Some(location) != home)
        .collect();
    for (index, bookmark) in inputs.bookmarks.bookmarks().iter().enumerate() {
        if places.contains(bookmark.location()) {
            continue;
        }
        // A server folder is always there to be opened: opening it connects
        // (ticket 310). Whether it is connected comes from GVfs's mount list;
        // its folder is never looked at here.
        let connection = inputs.remote.connection(bookmark.location());
        let available = match connection {
            Some(_) => true,
            None => bookmark
                .path()
                .is_some_and(|path| inputs.probe.is_available(path)),
        };
        rows.push(SidebarRow {
            section: SidebarSection::Favorites,
            key: format!("favorite-{index}"),
            label: bookmark.display_name(),
            location: bookmark.location().clone(),
            availability: availability(available),
            bookmark_index: Some(index),
            device_state: None,
            identity_volatile: false,
            connected: connection.flatten(),
        });
    }

    rows
}

/// The rows the sidebar draws. A place that is not there — a home with no
/// Music folder — is left out rather than drawn as "Unavailable", the way
/// folio lists only the folders home really has (`places.cpp`). A pinned
/// folder that is gone stays, so it can be unpinned, and says so.
pub fn shown_rows(rows: Vec<SidebarRow>) -> Vec<SidebarRow> {
    rows.into_iter()
        .filter(|row| row.section != SidebarSection::Places || row.availability.is_available())
        .collect()
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

/// What a sidebar line stands for when the highlight is chosen (ticket 175).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RowPlace {
    /// A favourite or a folder row: highlighted only while it is the open
    /// folder itself.
    Exact(Location),
    /// A drive, a phone, or a server this window mounted, by the folder it
    /// is mounted at: highlighted while the open folder is in it and no line
    /// is more specific.
    Mount(std::path::PathBuf),
}

/// The line to highlight for the open folder: a favourite or folder row
/// that is exactly it, else the line whose mount is the longest prefix of
/// its path. The first line wins a tie.
///
/// `foreign` is the mount point of the network or FUSE filesystem the open
/// folder is on, when it is on one (a GVfs share under
/// `/run/user/<uid>/gvfs`, an NFS or SMB mount, sshfs). A line outside that
/// filesystem never contains the folder: the system drive's `/` is a prefix
/// of every GVfs path, and it is not where the files are. So only a line at
/// or inside the foreign mount can light, and a folder there with no such
/// line lights nothing.
///
/// `None` when no line contains the folder (the Trash, Recent, a share with
/// no row).
pub fn highlighted_row(
    places: &[Option<RowPlace>],
    current: &Location,
    foreign: Option<&std::path::Path>,
) -> Option<usize> {
    let exact = places
        .iter()
        .position(|place| matches!(place, Some(RowPlace::Exact(location)) if location == current));
    if exact.is_some() {
        return exact;
    }
    let path = current.as_local_path()?.as_path();
    places
        .iter()
        .enumerate()
        .filter_map(|(index, place)| match place {
            Some(RowPlace::Mount(point))
                if path.starts_with(point)
                    && foreign.is_none_or(|foreign| point.starts_with(foreign)) =>
            {
                Some((index, point.components().count()))
            }
            _ => None,
        })
        .max_by_key(|&(index, depth)| (depth, std::cmp::Reverse(index)))
        .map(|(index, _)| index)
}

#[cfg(test)]
mod highlight_tests {
    use super::{RowPlace, highlighted_row};
    use files_core::Location;
    use std::path::{Path, PathBuf};

    fn local(path: &str) -> Location {
        Location::local(path).unwrap()
    }

    fn mount(path: &str) -> Option<RowPlace> {
        Some(RowPlace::Mount(PathBuf::from(path)))
    }

    fn exact(path: &str) -> Option<RowPlace> {
        Some(RowPlace::Exact(local(path)))
    }

    const GVFS: &str = "/run/user/1000/gvfs";
    const NAS: &str = "/run/user/1000/gvfs/sftp:host=nas";
    const PHONE: &str = "/run/user/1000/gvfs/mtp:host=Pixel_8_1234";

    /// A sidebar like the window's: Home and Documents as places, a pinned
    /// folder, a header, the system drive, a second drive, a drive mounted
    /// inside the second one, a tailnet machine this window mounted, and a
    /// phone.
    fn sidebar() -> Vec<Option<RowPlace>> {
        vec![
            exact("/home/a"),
            exact("/home/a/Documents"),
            exact("/home/a/Projects/site"),
            None,
            mount("/"),
            mount("/media/a/Data"),
            mount("/media/a/Data/inner"),
            mount(NAS),
            mount(PHONE),
        ]
    }

    fn lit(current: &str) -> Option<usize> {
        highlighted_row(&sidebar(), &local(current), None)
    }

    fn lit_on_gvfs(current: &str) -> Option<usize> {
        highlighted_row(&sidebar(), &local(current), Some(Path::new(GVFS)))
    }

    #[test]
    fn the_root_folder_lights_the_system_drive() {
        assert_eq!(lit("/"), Some(4));
    }

    #[test]
    fn a_folder_on_the_system_drive_with_a_favourite_lights_the_favourite() {
        assert_eq!(lit("/home/a/Projects/site"), Some(2));
        assert_eq!(lit("/home/a"), Some(0));
    }

    #[test]
    fn a_folder_on_the_system_drive_without_a_favourite_lights_the_system_drive() {
        // Not Home, though Home contains it: a place lights only when it is
        // the open folder.
        assert_eq!(lit("/home/a/Projects/site/src"), Some(4));
        assert_eq!(lit("/etc"), Some(4));
    }

    #[test]
    fn a_folder_on_a_second_drive_lights_that_drive() {
        assert_eq!(lit("/media/a/Data"), Some(5));
        assert_eq!(lit("/media/a/Data/photos/2026"), Some(5));
    }

    #[test]
    fn a_folder_on_a_drive_mounted_inside_another_lights_the_inner_drive() {
        assert_eq!(lit("/media/a/Data/inner/music"), Some(6));
        // A sibling whose name only starts the same is not on it.
        assert_eq!(lit("/media/a/Data/inner-old"), Some(5));
    }

    /// Review of ticket 175: a mounted tailnet machine is browsed at its
    /// GVfs path, and `/` is a prefix of it.
    #[test]
    fn a_server_this_window_mounted_lights_its_own_row_not_the_system_drive() {
        assert_eq!(lit_on_gvfs(NAS), Some(7));
        assert_eq!(lit_on_gvfs(&format!("{NAS}/home/ann/photos")), Some(7));
    }

    #[test]
    fn a_phone_lights_its_own_row() {
        assert_eq!(
            lit_on_gvfs(&format!("{PHONE}/Internal storage/DCIM")),
            Some(8)
        );
    }

    #[test]
    fn a_gvfs_or_fuse_folder_with_no_row_lights_nothing() {
        // An SMB share mounted from the path bar, which has no row.
        assert_eq!(
            lit_on_gvfs(&format!("{GVFS}/smb-share:server=nas,share=media/films")),
            None
        );
        // An sshfs or NFS mount of its own.
        let sshfs = highlighted_row(
            &sidebar(),
            &local("/home/a/remote/project"),
            Some(Path::new("/home/a/remote")),
        );
        assert_eq!(sshfs, None);
    }

    #[test]
    fn a_place_with_no_containing_line_lights_nothing() {
        for place in [
            Location::Trash(files_core::TrashLocation::Root),
            Location::Recent,
        ] {
            assert_eq!(highlighted_row(&sidebar(), &place, None), None, "{place:?}");
        }
        let no_drives = vec![exact("/home/a"), None];
        assert_eq!(highlighted_row(&no_drives, &local("/home/a/x"), None), None);
    }

    #[test]
    fn the_trash_row_lights_for_the_trash_itself() {
        let places = vec![Some(RowPlace::Exact(Location::Trash(
            files_core::TrashLocation::Root,
        )))];
        assert_eq!(
            highlighted_row(
                &places,
                &Location::Trash(files_core::TrashLocation::Root),
                None
            ),
            Some(0)
        );
    }
}
