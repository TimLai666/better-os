//! Mount-point enumeration, so an entry can be attributed to the device it is
//! actually on.
//!
//! A file list needs this for two honest answers: which entries live on an
//! external device — so removing it can say what is affected, and so the view
//! can mark them — and which entries are on a read-only medium, where a
//! writable-looking mode is still not writable.
//!
//! Identity comes from `storage-core`. What this crate reads is a mount table,
//! which carries far less than UDisks2 does, so the identity built here is
//! honestly weak or volatile. It never claims to be stable: filling
//! `storage-core`'s stable identifiers from a mount table would be inventing
//! them. `storage-service` is the component that knows a device's real
//! identity, and a consumer that has one should prefer it.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use storage_core::{DeviceIdentity, IdentityEvidence, IdentityKey, Transport};

/// Where the kernel publishes the mount table.
pub const MOUNTINFO_PATH: &str = "/proc/self/mountinfo";

/// One mounted filesystem.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MountPoint {
    pub mount_point: PathBuf,
    /// The kernel device name, as the mount table gives it.
    pub source: String,
    pub filesystem: String,
    pub read_only: bool,
    /// The identity entries on this mount are tagged with.
    pub identity: DeviceIdentity,
}

impl MountPoint {
    /// Whether this looks like removable or external storage.
    ///
    /// A heuristic, and named as one. The mount point's location under the
    /// conventional media directories is all a mount table supports;
    /// `storage-service` is where the real answer comes from.
    pub fn is_probably_external(&self) -> bool {
        let path = self.mount_point.to_string_lossy();
        path.starts_with("/media/") || path.starts_with("/run/media/") || path.starts_with("/mnt/")
    }
}

/// Every mount, ordered so the longest mount point wins a lookup.
#[derive(Clone, Debug, Default)]
pub struct MountTable {
    mounts: Vec<MountPoint>,
}

impl MountTable {
    pub fn new(mut mounts: Vec<MountPoint>) -> Self {
        // Longest first, so `/media/stick` beats `/` for a path under it.
        mounts.sort_by_key(|mount| std::cmp::Reverse(mount.mount_point.as_os_str().len()));
        Self { mounts }
    }

    /// Reads the table from the running kernel.
    pub fn from_env() -> Self {
        read_mount_table(Path::new(MOUNTINFO_PATH))
    }

    pub fn mounts(&self) -> &[MountPoint] {
        &self.mounts
    }

    pub fn is_empty(&self) -> bool {
        self.mounts.is_empty()
    }

    /// The mount a path sits on, or `None` when no mount contains it — which
    /// happens when the table could not be read at all.
    pub fn mount_for(&self, path: &Path) -> Option<&MountPoint> {
        self.mounts
            .iter()
            .find(|mount| path.starts_with(&mount.mount_point))
    }

    /// The device identity to tag entries under this path with.
    pub fn owner_of(&self, path: &Path) -> Option<IdentityKey> {
        self.mount_for(path)
            .map(|mount| mount.identity.key().clone())
    }

    pub fn is_read_only(&self, path: &Path) -> bool {
        self.mount_for(path)
            .map(|mount| mount.read_only)
            .unwrap_or(false)
    }

    /// Every mount that looks external, for a sidebar's Devices section.
    pub fn external_mounts(&self) -> impl Iterator<Item = &MountPoint> {
        self.mounts
            .iter()
            .filter(|mount| mount.is_probably_external())
    }
}

/// Reads a mount table from a `mountinfo`-format file.
///
/// Taking the path makes this testable against a fixture instead of against
/// whatever the host happens to have mounted.
pub fn read_mount_table(path: &Path) -> MountTable {
    let Ok(contents) = fs::read_to_string(path) else {
        return MountTable::default();
    };
    MountTable::new(parse_mountinfo(&contents))
}

/// Parses `mountinfo`, whose lines are
/// `id parent major:minor root mount-point options... - fstype source
/// super-options`.
fn parse_mountinfo(contents: &str) -> Vec<MountPoint> {
    let mut mounts = Vec::new();
    for line in contents.lines() {
        let Some((before, after)) = line.split_once(" - ") else {
            continue;
        };
        let head: Vec<&str> = before.split_whitespace().collect();
        let tail: Vec<&str> = after.split_whitespace().collect();
        if head.len() < 6 || tail.len() < 2 {
            continue;
        }
        let mount_point = unescape(head[4]);
        let options = head[5];
        let filesystem = tail[0].to_string();
        let source = unescape(tail[1]);
        let read_only = options.split(',').any(|option| option == "ro")
            || tail
                .get(2)
                .is_some_and(|super_options| super_options.split(',').any(|option| option == "ro"));
        let identity = identity_for(&source, head[2]);
        mounts.push(MountPoint {
            mount_point: PathBuf::from(mount_point),
            source,
            filesystem,
            read_only,
            identity,
        });
    }
    mounts
}

/// Builds the weakest honest identity a mount table supports.
///
/// The device name and the major:minor pair are all that is available, so the
/// evidence carries them and nothing else. `storage-core` reads that as a
/// volatile identity, which is correct: it identifies the device for this
/// connection only and can never hold a stored preference.
fn identity_for(source: &str, major_minor: &str) -> DeviceIdentity {
    DeviceIdentity::from_evidence(IdentityEvidence {
        device_path: if source.starts_with('/') {
            source.to_string()
        } else {
            // A pseudo-filesystem such as `tmpfs` has no device path. The
            // major:minor pair still distinguishes two of them.
            format!("{source}#{major_minor}")
        },
        transport: Transport::Unknown,
        ..IdentityEvidence::default()
    })
}

/// `mountinfo` escapes space, tab, newline, and backslash as octal.
fn unescape(field: &str) -> String {
    if !field.contains('\\') {
        return field.to_string();
    }
    let bytes = field.as_bytes();
    let mut out = String::with_capacity(field.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'\\' && index + 3 < bytes.len() {
            let octal = &field[index + 1..index + 4];
            if let Ok(value) = u8::from_str_radix(octal, 8) {
                out.push(value as char);
                index += 4;
                continue;
            }
        }
        out.push(bytes[index] as char);
        index += 1;
    }
    out
}

/// The mount points that would be shown as devices, keyed by identity, for a
/// caller assembling a sidebar.
pub fn external_devices(table: &MountTable) -> HashMap<IdentityKey, PathBuf> {
    table
        .external_mounts()
        .map(|mount| (mount.identity.key().clone(), mount.mount_point.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = concat!(
        "23 28 0:21 / /proc rw,nosuid,relatime shared:12 - proc proc rw\n",
        "28 1 8:2 / / rw,relatime shared:1 - ext4 /dev/sda2 rw,errors=remount-ro\n",
        "76 28 8:17 / /media/user/USB\\040STICK rw,nosuid,relatime shared:40 - vfat /dev/sdb1 rw,uid=1000\n",
        "88 28 8:33 / /mnt/archive ro,relatime shared:50 - ext4 /dev/sdc1 ro\n",
        "malformed line without a separator\n",
    );

    fn table() -> MountTable {
        MountTable::new(parse_mountinfo(FIXTURE))
    }

    #[test]
    fn the_longest_matching_mount_owns_a_path() {
        let table = table();
        let root = table.mount_for(Path::new("/home/user/notes.txt")).unwrap();
        assert_eq!(root.source, "/dev/sda2");
        let stick = table
            .mount_for(Path::new("/media/user/USB STICK/photos"))
            .unwrap();
        assert_eq!(stick.source, "/dev/sdb1");
        assert_eq!(stick.filesystem, "vfat");
    }

    #[test]
    fn an_escaped_mount_point_is_decoded() {
        let table = table();
        assert!(
            table
                .mounts()
                .iter()
                .any(|mount| mount.mount_point == Path::new("/media/user/USB STICK"))
        );
    }

    #[test]
    fn a_read_only_mount_is_reported_so_a_writable_mode_is_not_believed() {
        let table = table();
        assert!(table.is_read_only(Path::new("/mnt/archive/old")));
        assert!(!table.is_read_only(Path::new("/home/user")));
    }

    #[test]
    fn entries_on_different_mounts_get_different_identities() {
        let table = table();
        let root = table.owner_of(Path::new("/home/user")).unwrap();
        let stick = table
            .owner_of(Path::new("/media/user/USB STICK/a"))
            .unwrap();
        assert_ne!(root, stick);
    }

    #[test]
    fn an_identity_from_a_mount_table_is_never_claimed_to_be_stable() {
        let table = table();
        let mount = table.mount_for(Path::new("/home/user")).unwrap();
        assert_eq!(
            mount.identity.confidence(),
            storage_core::IdentityConfidence::Volatile,
            "a mount table has no stable identifier, and must not pretend otherwise"
        );
        assert!(!mount.identity.confidence().persistable());
    }

    #[test]
    fn the_external_devices_are_the_ones_under_the_media_directories() {
        let table = table();
        let external: Vec<&Path> = table
            .external_mounts()
            .map(|mount| mount.mount_point.as_path())
            .collect();
        assert_eq!(external.len(), 2);
        assert!(external.contains(&Path::new("/mnt/archive")));
        assert!(external.contains(&Path::new("/media/user/USB STICK")));
        assert_eq!(external_devices(&table).len(), 2);
    }

    #[test]
    fn an_unreadable_mount_table_yields_no_mounts_rather_than_failing() {
        let table = read_mount_table(Path::new("/nonexistent/mountinfo"));
        assert!(table.is_empty());
        assert_eq!(table.owner_of(Path::new("/home")), None);
        assert!(!table.is_read_only(Path::new("/home")));
    }
}
