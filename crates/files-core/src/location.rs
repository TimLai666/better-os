//! The typed location model.
//!
//! Issue #6's architectural rule is enforced here rather than by convention:
//! not every location is a `std::path::PathBuf`. Trash, Recent, Applications,
//! a mounted device, and the network kinds that do not exist yet are separate
//! shapes of the same closed enum, so a consumer that wants a filesystem path
//! has to ask for one and handle the answer being "this location does not have
//! one".
//!
//! The enum is closed on purpose. A future SMB or SFTP backend adds a variant
//! and the compiler names every place that has to decide what to do about it.
//! Until then [`Location::Network`] represents such a location without
//! implementing it, and [`Location::Unsupported`] holds a location string this
//! build recognizes as a location but refuses to interpret.

use std::fmt;
use std::path::{Path, PathBuf};

use storage_core::DeviceIdentity;

use crate::error::LocationError;

/// An absolute path in the session's own filesystem namespace.
///
/// The only way to build one is from an absolute path, so a `LocalPath` in
/// hand is never a relative fragment that resolves against whatever working
/// directory the process happens to have.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct LocalPath(PathBuf);

impl LocalPath {
    /// Wraps an absolute path. A relative path is rejected rather than joined
    /// against the current directory.
    pub fn new(path: impl Into<PathBuf>) -> Result<Self, LocationError> {
        let path = path.into();
        if !path.is_absolute() {
            return Err(LocationError::RelativePath(path.to_string_lossy().into()));
        }
        Ok(Self(normalize_components(&path)))
    }

    /// The root of the filesystem, which always exists as a location even on a
    /// host where nothing else is readable.
    pub fn root() -> Self {
        Self(PathBuf::from("/"))
    }

    pub fn as_path(&self) -> &Path {
        &self.0
    }

    /// The child under this path with the given file name.
    ///
    /// A name containing a separator, `.`, or `..` is rejected: a listing
    /// producing such a name would otherwise let a crafted directory entry
    /// point the model outside the directory it came from.
    pub fn join_name(&self, name: &str) -> Result<Self, LocationError> {
        if name.is_empty()
            || name == "."
            || name == ".."
            || name.contains('/')
            || name.contains('\0')
        {
            return Err(LocationError::InvalidName(name.to_string()));
        }
        Ok(Self(self.0.join(name)))
    }

    /// The containing directory, or `None` at the filesystem root.
    pub fn parent(&self) -> Option<Self> {
        self.0.parent().map(|parent| Self(parent.to_path_buf()))
    }

    /// The final component, lossily decoded. A filename that is not valid
    /// UTF-8 still has a display name; it is never dropped from a listing.
    pub fn file_name(&self) -> String {
        match self.0.file_name() {
            Some(name) => name.to_string_lossy().into_owned(),
            None => "/".to_string(),
        }
    }
}

impl fmt::Display for LocalPath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0.to_string_lossy())
    }
}

/// Collapses `.` and repeated separators without resolving symlinks.
///
/// `..` is deliberately kept: resolving it lexically is wrong across a symlink,
/// and the platform layer is the only place that may ask the kernel.
fn normalize_components(path: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    if out.as_os_str().is_empty() {
        out.push("/");
    }
    out
}

/// Where inside Trash a location points.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum TrashLocation {
    /// The Trash listing itself: the deleted items, with their original paths
    /// and deletion times.
    Root,
    /// A directory inside a trashed item, reached by expanding a trashed
    /// folder. The identifier is the trash info name, not a user-facing path.
    Inside {
        /// The `.trashinfo` stem that identifies the trashed item.
        item: String,
        /// The path inside that item, relative to the trashed item's own root.
        relative: PathBuf,
    },
}

/// A location on a specific storage device, named by device identity rather
/// than by mount point.
///
/// A mount point moves between sessions and can be reused by a different disk.
/// Filing the location under the identity `storage-core` produced means a
/// restored tab either reopens on the same physical device or reports that the
/// device is not connected — it never silently opens a different one.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeviceLocation {
    pub identity: DeviceIdentity,
    /// Path relative to the device's mount point.
    pub relative: PathBuf,
}

impl DeviceLocation {
    pub fn new(identity: DeviceIdentity, relative: impl Into<PathBuf>) -> Self {
        Self {
            identity,
            relative: relative.into(),
        }
    }

    /// Resolves to a filesystem path only when the caller supplies the mount
    /// point the device is currently mounted at.
    ///
    /// There is no method that produces the path without one, because a device
    /// location has no path while the device is unmounted.
    pub fn resolve(&self, mount_point: &Path) -> Result<LocalPath, LocationError> {
        LocalPath::new(mount_point.join(&self.relative))
    }
}

/// A network location's protocol. Every variant is representable; none is
/// implemented in this build.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum NetworkScheme {
    Smb,
    Sftp,
    Webdav,
    /// A GVfs- or portal-backed mount reached through the session's own
    /// gateway rather than by a protocol this project speaks.
    Gvfs,
}

impl NetworkScheme {
    pub fn as_str(self) -> &'static str {
        match self {
            NetworkScheme::Smb => "smb",
            NetworkScheme::Sftp => "sftp",
            NetworkScheme::Webdav => "webdav",
            NetworkScheme::Gvfs => "gvfs",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "smb" | "cifs" => Some(NetworkScheme::Smb),
            "sftp" | "ssh" => Some(NetworkScheme::Sftp),
            "webdav" | "dav" | "davs" => Some(NetworkScheme::Webdav),
            "gvfs" => Some(NetworkScheme::Gvfs),
            _ => None,
        }
    }
}

/// A remote location. Modelled fully so a tab, a bookmark, and a history entry
/// can hold one; listing it is not implemented and reports
/// [`LocationError::BackendUnavailable`].
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct NetworkLocation {
    pub scheme: NetworkScheme,
    /// Host, optionally with a user and port, exactly as the user entered it.
    pub authority: String,
    /// Path on the remote, always starting with `/`.
    pub path: String,
}

impl NetworkLocation {
    pub fn new(
        scheme: NetworkScheme,
        authority: impl Into<String>,
        path: impl Into<String>,
    ) -> Result<Self, LocationError> {
        let authority = authority.into();
        if authority.trim().is_empty() {
            return Err(LocationError::MissingAuthority(scheme.as_str()));
        }
        let mut path = path.into();
        if !path.starts_with('/') {
            path.insert(0, '/');
        }
        Ok(Self {
            scheme,
            authority,
            path,
        })
    }
}

/// A location this build recognizes as a location but will not interpret.
///
/// This is the closed enum's honest arm. A URI with an unknown scheme becomes
/// one of these rather than being coerced into a local path, so a restored
/// session that names something this version does not understand reports that
/// instead of opening the wrong directory.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct UnsupportedLocation {
    /// What was recorded, kept verbatim so a later version can interpret it.
    pub raw: String,
    /// Why this build refused it, as a stable machine key.
    pub reason: UnsupportedReason,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum UnsupportedReason {
    /// The scheme is not one this version models.
    UnknownScheme,
    /// The scheme is modelled but the text after it did not parse.
    Malformed,
}

/// Everywhere Better Files can be.
///
/// Adding a variant is a deliberate, compiler-enforced change: every match on
/// a `Location` has to say what the new kind does.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Location {
    /// A directory in the session's own filesystem.
    Local(LocalPath),
    /// The freedesktop trash.
    Trash(TrashLocation),
    /// Recently used files, aggregated rather than stored in a directory.
    Recent,
    /// Installed applications, backed by the shared catalog. It is a view, not
    /// a directory, and has no filesystem path at all.
    Applications,
    /// A path on a specific storage device.
    ///
    /// Boxed because a device identity carries every identifier the platform
    /// reported, and an unboxed variant would make every `Location` — a tab, a
    /// history entry, a bookmark — as large as the biggest one.
    Device(Box<DeviceLocation>),
    /// A remote location. Representable, not implemented.
    Network(NetworkLocation),
    /// Recognized as a location, refused by this build.
    Unsupported(UnsupportedLocation),
}

/// What a location is, without its payload. Useful for grouping in a sidebar
/// and for asserting a kind in a test without matching on the contents.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum LocationKind {
    Local,
    Trash,
    Recent,
    Applications,
    Device,
    Network,
    Unsupported,
}

impl Location {
    /// Convenience for the common case, still going through the absolute-path
    /// check.
    pub fn local(path: impl Into<PathBuf>) -> Result<Self, LocationError> {
        Ok(Location::Local(LocalPath::new(path)?))
    }

    pub fn kind(&self) -> LocationKind {
        match self {
            Location::Local(_) => LocationKind::Local,
            Location::Trash(_) => LocationKind::Trash,
            Location::Recent => LocationKind::Recent,
            Location::Applications => LocationKind::Applications,
            Location::Device(_) => LocationKind::Device,
            Location::Network(_) => LocationKind::Network,
            Location::Unsupported(_) => LocationKind::Unsupported,
        }
    }

    /// The filesystem path for this location, when it has one.
    ///
    /// This is the whole point of the type. `Applications`, `Recent`, a
    /// network location, and an unmounted device all answer `None`, and the
    /// caller has to handle it; there is no fallback that invents a path.
    pub fn as_local_path(&self) -> Option<&LocalPath> {
        match self {
            Location::Local(path) => Some(path),
            _ => None,
        }
    }

    /// Whether this location's contents come from reading a directory.
    pub fn is_filesystem_backed(&self) -> bool {
        matches!(self, Location::Local(_) | Location::Device(_))
    }

    /// Whether this build can list the location at all.
    pub fn is_listable(&self) -> bool {
        !matches!(
            self,
            Location::Network(_) | Location::Unsupported(_) | Location::Recent
        )
    }

    /// The location one level up, or `None` when there is none.
    ///
    /// Virtual roots have no parent: Applications is not "inside" anything, and
    /// saying its parent is Home would be an invented hierarchy.
    pub fn parent(&self) -> Option<Location> {
        match self {
            Location::Local(path) => path.parent().map(Location::Local),
            Location::Trash(TrashLocation::Inside { item, relative }) => {
                match relative.parent() {
                    Some(parent) if !parent.as_os_str().is_empty() => {
                        Some(Location::Trash(TrashLocation::Inside {
                            item: item.clone(),
                            relative: parent.to_path_buf(),
                        }))
                    }
                    // The parent of the top of a trashed folder is the Trash
                    // listing, not the folder's original parent directory,
                    // which may no longer exist.
                    _ => Some(Location::Trash(TrashLocation::Root)),
                }
            }
            Location::Device(device) => match device.relative.parent() {
                Some(parent) if !parent.as_os_str().is_empty() => Some(Location::Device(Box::new(
                    DeviceLocation::new(device.identity.clone(), parent.to_path_buf()),
                ))),
                _ => None,
            },
            _ => None,
        }
    }

    /// A stable identifier for persistence: tabs, history, and bookmarks are
    /// stored as these strings.
    ///
    /// Round-tripping is exact for every variant this build understands, and
    /// an unsupported location keeps whatever it was given, so a session
    /// written by a later version survives being read by this one.
    pub fn to_uri(&self) -> String {
        match self {
            Location::Local(path) => format!("file://{path}"),
            Location::Trash(TrashLocation::Root) => "trash:///".to_string(),
            Location::Trash(TrashLocation::Inside { item, relative }) => {
                format!("trash:///{item}/{}", relative.to_string_lossy())
            }
            Location::Recent => "recent:///".to_string(),
            Location::Applications => "applications:///".to_string(),
            Location::Device(device) => format!(
                "device://{}/{}",
                device.identity.key(),
                device.relative.to_string_lossy()
            ),
            Location::Network(network) => format!(
                "{}://{}{}",
                network.scheme.as_str(),
                network.authority,
                network.path
            ),
            Location::Unsupported(unsupported) => unsupported.raw.clone(),
        }
    }

    /// Reads a location back from [`Location::to_uri`].
    ///
    /// A device URI cannot be rebuilt into a `Location::Device` here, because
    /// the identity's evidence is not in the string; the caller resolves the
    /// key against the connected devices. Everything unrecognized becomes
    /// [`Location::Unsupported`] rather than an error, so one bad saved tab
    /// does not fail a whole session restore.
    pub fn parse_uri(raw: &str) -> Location {
        let Some((scheme, rest)) = raw.split_once("://") else {
            return Location::Unsupported(UnsupportedLocation {
                raw: raw.to_string(),
                reason: UnsupportedReason::UnknownScheme,
            });
        };
        match scheme.to_ascii_lowercase().as_str() {
            "file" => match LocalPath::new(rest) {
                Ok(path) => Location::Local(path),
                Err(_) => Location::Unsupported(UnsupportedLocation {
                    raw: raw.to_string(),
                    reason: UnsupportedReason::Malformed,
                }),
            },
            "recent" => Location::Recent,
            "applications" => Location::Applications,
            "trash" => {
                let trimmed = rest.trim_start_matches('/');
                if trimmed.is_empty() {
                    Location::Trash(TrashLocation::Root)
                } else {
                    match trimmed.split_once('/') {
                        Some((item, relative)) => Location::Trash(TrashLocation::Inside {
                            item: item.to_string(),
                            relative: PathBuf::from(relative),
                        }),
                        None => Location::Trash(TrashLocation::Inside {
                            item: trimmed.to_string(),
                            relative: PathBuf::new(),
                        }),
                    }
                }
            }
            other => match NetworkScheme::parse(other) {
                Some(network_scheme) => {
                    let (authority, path) = match rest.find('/') {
                        Some(index) => rest.split_at(index),
                        None => (rest, "/"),
                    };
                    match NetworkLocation::new(network_scheme, authority, path) {
                        Ok(network) => Location::Network(network),
                        Err(_) => Location::Unsupported(UnsupportedLocation {
                            raw: raw.to_string(),
                            reason: UnsupportedReason::Malformed,
                        }),
                    }
                }
                None => Location::Unsupported(UnsupportedLocation {
                    raw: raw.to_string(),
                    reason: UnsupportedReason::UnknownScheme,
                }),
            },
        }
    }

    /// The label a sidebar or tab shows.
    pub fn display_name(&self) -> String {
        match self {
            Location::Local(path) => path.file_name(),
            Location::Trash(TrashLocation::Root) => "Trash".to_string(),
            Location::Trash(TrashLocation::Inside { item, relative }) => {
                match relative.file_name() {
                    Some(name) => name.to_string_lossy().into_owned(),
                    None => item.clone(),
                }
            }
            Location::Recent => "Recent".to_string(),
            Location::Applications => "Applications".to_string(),
            Location::Device(device) => match device.relative.file_name() {
                Some(name) => name.to_string_lossy().into_owned(),
                None => device.identity.display_name(),
            },
            Location::Network(network) => network.authority.clone(),
            Location::Unsupported(unsupported) => unsupported.raw.clone(),
        }
    }
}

impl fmt::Display for Location {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.to_uri())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_relative_path_is_not_a_location() {
        assert!(matches!(
            LocalPath::new("relative/dir"),
            Err(LocationError::RelativePath(_))
        ));
    }

    #[test]
    fn applications_has_no_filesystem_path() {
        assert_eq!(Location::Applications.as_local_path(), None);
        assert_eq!(Location::Recent.as_local_path(), None);
        assert_eq!(Location::Trash(TrashLocation::Root).as_local_path(), None);
        assert!(!Location::Applications.is_filesystem_backed());
    }

    #[test]
    fn a_current_directory_component_is_collapsed_but_dotdot_is_kept() {
        let path = LocalPath::new("/home/./user/../user").unwrap();
        assert_eq!(path.as_path(), Path::new("/home/user/../user"));
    }

    #[test]
    fn a_child_name_cannot_escape_its_directory() {
        let path = LocalPath::new("/home/user").unwrap();
        assert!(path.join_name("..").is_err());
        assert!(path.join_name("a/b").is_err());
        assert!(path.join_name("").is_err());
        assert_eq!(
            path.join_name("file.txt").unwrap().as_path(),
            Path::new("/home/user/file.txt")
        );
    }

    #[test]
    fn uris_round_trip_for_every_understood_kind() {
        let cases = [
            Location::local("/home/user/Documents").unwrap(),
            Location::Trash(TrashLocation::Root),
            Location::Trash(TrashLocation::Inside {
                item: "report.txt".to_string(),
                relative: PathBuf::from("nested/deep"),
            }),
            Location::Recent,
            Location::Applications,
            Location::Network(
                NetworkLocation::new(NetworkScheme::Smb, "fileserver", "/share").unwrap(),
            ),
        ];
        for location in cases {
            assert_eq!(Location::parse_uri(&location.to_uri()), location);
        }
    }

    #[test]
    fn an_unknown_scheme_is_kept_verbatim_rather_than_guessed() {
        let parsed = Location::parse_uri("mtp://phone/DCIM");
        assert_eq!(
            parsed,
            Location::Unsupported(UnsupportedLocation {
                raw: "mtp://phone/DCIM".to_string(),
                reason: UnsupportedReason::UnknownScheme,
            })
        );
        assert_eq!(parsed.to_uri(), "mtp://phone/DCIM");
        assert!(!parsed.is_listable());
    }

    #[test]
    fn virtual_roots_have_no_parent_and_trash_folders_return_to_trash() {
        assert_eq!(Location::Applications.parent(), None);
        assert_eq!(Location::Recent.parent(), None);
        assert_eq!(Location::Trash(TrashLocation::Root).parent(), None);
        assert_eq!(
            Location::Trash(TrashLocation::Inside {
                item: "folder".to_string(),
                relative: PathBuf::from("one"),
            })
            .parent(),
            Some(Location::Trash(TrashLocation::Root))
        );
        assert_eq!(
            Location::local("/home/user").unwrap().parent(),
            Some(Location::local("/home").unwrap())
        );
        assert_eq!(Location::local("/").unwrap().parent(), None);
    }

    #[test]
    fn a_network_location_needs_an_authority() {
        assert!(NetworkLocation::new(NetworkScheme::Sftp, "  ", "/home").is_err());
        let location = NetworkLocation::new(NetworkScheme::Sftp, "user@host:2222", "home").unwrap();
        assert_eq!(location.path, "/home");
        assert!(!Location::Network(location).is_listable());
    }
}
