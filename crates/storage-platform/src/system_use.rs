//! Whether a drive holds something the running system is using (ticket 171).
//!
//! UDisks2 marks the drive the system runs from with `HintSystem` only when
//! it is internal. A system installed on a USB drive, a live USB, or `/home`
//! on an external disk is a hot-pluggable removable drive like any stick, and
//! nothing in UDisks2's answer says otherwise. Ejecting or formatting it
//! takes the running system down with it.
//!
//! So the answer is worked out here, once, from the kernel's own record of
//! what is mounted where and which swap is in use, matched against UDisks2's
//! devices: Better Files' menu, the storage service, and `manager-daemon`
//! all ask [`SystemDrives`], so a drive is never safe to one of them and the
//! running system to another.
//!
//! **Unknown is never Free.** A drive is [`SystemUse::Free`] only when the
//! mount table could be read, every system mount and swap on a block device
//! was traced to a drive, and this drive is not one of them. Anything short
//! of that is [`SystemUse::Unknown`], and Unknown is treated as held: nothing
//! is ejected or formatted on a guess.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::model::PlatformDevice;
use crate::roots::Roots;

/// Mount points the running system cannot lose: its root, its boot and
/// firmware partitions, the people's home folders and the system's own
/// trees, and a live session's medium (Ubuntu's `/cdrom`, Debian live's,
/// dracut's and Arch's), whose squashfs the whole session runs from.
pub const SYSTEM_PATHS: &[&str] = &[
    "/",
    "/boot",
    "/boot/efi",
    "/efi",
    "/home",
    "/usr",
    "/var",
    "/cdrom",
    "/run/live/medium",
    "/lib/live/mount/medium",
    "/run/initramfs/live",
    "/run/archiso/bootmnt",
];

/// Whether a mount point is one of [`SYSTEM_PATHS`] exactly. `/home/me/usb`
/// is not: a stick mounted under a home folder is the person's.
pub fn is_system_path(point: &Path) -> bool {
    SYSTEM_PATHS.iter().any(|path| point == Path::new(path))
}

/// What, if anything, of the running system a drive holds.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SystemUse {
    /// Nothing the running system uses is on it.
    Free,
    /// The running system's root (`/`) is on it.
    RunningSystem,
    /// Another mounted system path is on it (`/home`, `/boot/efi`, a live
    /// session's medium).
    SystemFolder { path: PathBuf },
    /// Swap the system is using is on it.
    Swap,
    /// It could not be told: treated as held.
    #[default]
    Unknown,
}

impl SystemUse {
    /// Whether ejecting, unmounting, powering off or formatting it may be
    /// offered and carried out.
    pub fn permits_removal(&self) -> bool {
        matches!(self, SystemUse::Free)
    }

    /// The typed reason a refused request carries
    /// (`storage.system.running_system`, …); `None` for a free drive.
    pub fn refusal(&self) -> Option<&'static str> {
        match self {
            SystemUse::Free => None,
            SystemUse::RunningSystem => Some("storage.system.running_system"),
            SystemUse::SystemFolder { .. } => Some("storage.system.folder"),
            SystemUse::Swap => Some("storage.system.swap"),
            SystemUse::Unknown => Some("storage.system.unknown"),
        }
    }

    /// The reason a refusal names, back from its code.
    pub fn from_refusal(code: &str) -> Option<SystemUse> {
        match code {
            "storage.system.running_system" => Some(SystemUse::RunningSystem),
            "storage.system.swap" => Some(SystemUse::Swap),
            "storage.system.unknown" => Some(SystemUse::Unknown),
            _ => code
                .strip_prefix("storage.system.folder")
                .map(|rest| SystemUse::SystemFolder {
                    path: PathBuf::from(rest.strip_prefix(':').unwrap_or("")),
                }),
        }
    }

    /// The refusal with its path, for a folder: `storage.system.folder:/home`.
    pub fn refusal_detail(&self) -> Option<String> {
        let code = self.refusal()?;
        Some(match self {
            SystemUse::SystemFolder { path } => format!("{code}:{}", path.display()),
            _ => code.to_string(),
        })
    }

    /// How strongly it holds a drive, for keeping the strongest of several.
    fn rank(&self) -> u8 {
        match self {
            SystemUse::RunningSystem => 4,
            SystemUse::SystemFolder { .. } => 3,
            SystemUse::Swap => 2,
            SystemUse::Unknown => 1,
            SystemUse::Free => 0,
        }
    }

    /// The stronger of two answers about the same drive.
    pub fn strongest(self, other: SystemUse) -> SystemUse {
        if other.rank() > self.rank() {
            other
        } else {
            self
        }
    }
}

/// One thing the running system uses that lives on a block device: a system
/// mount or an active swap partition, with every node it goes by.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SystemSource {
    pub held: SystemUse,
    /// The node the kernel names (`/dev/mapper/vg-root`), the node it
    /// resolves to (`/dev/dm-0`), and every node it is built on, down to the
    /// partitions (`/dev/sda3`).
    pub nodes: Vec<String>,
}

/// What the kernel says the running system uses, read from `/proc` and
/// `/sys`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SystemSources {
    pub sources: Vec<SystemSource>,
}

/// Filesystems on block devices the mount table does not name.
const UNTRACEABLE_FILESYSTEMS: &[&str] = &["zfs", "bcachefs"];

/// Block devices that are no drive: memory, compressed swap, loop files (a
/// live medium's own mount names the drive under its loop).
fn is_virtual_node(node: &str) -> bool {
    ["/dev/zram", "/dev/ram", "/dev/loop"]
        .iter()
        .any(|prefix| node.starts_with(prefix))
}

/// A mount table field, with the kernel's octal escapes (`\040` for a
/// space) undone.
fn unescape(field: &str) -> String {
    let bytes = field.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'\\'
            && let Some(octal) = field.get(index + 1..index + 4)
            && let Ok(value) = u8::from_str_radix(octal, 8)
        {
            out.push(value);
            index += 4;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

impl SystemSources {
    /// The system mounts and swap partitions in `/proc/self/mounts` and
    /// `/proc/swaps` text, each with its own node only. Mounts that are no
    /// block device (an overlay `/`, `tmpfs`, ZFS datasets) are left out:
    /// nothing a drive's eject could take away is named by them.
    pub fn parse(mounts: &str, swaps: &str) -> Self {
        let mut sources = Vec::new();
        for line in mounts.lines() {
            let mut fields = line.split_whitespace();
            let (Some(source), Some(point)) = (fields.next(), fields.next()) else {
                continue;
            };
            let filesystem = fields.next().unwrap_or_default();
            let (source, point) = (unescape(source), PathBuf::from(unescape(point)));
            if !is_system_path(&point) {
                continue;
            }
            let held = if point == Path::new("/") {
                SystemUse::RunningSystem
            } else {
                SystemUse::SystemFolder { path: point }
            };
            // A ZFS dataset lives on a pool whose drives the mount table
            // does not name: kept with no node, so it is never traced and
            // every drive it could be on stays unknown.
            if UNTRACEABLE_FILESYSTEMS.contains(&filesystem) {
                sources.push(SystemSource {
                    held,
                    nodes: Vec::new(),
                });
                continue;
            }
            if !source.starts_with("/dev/") || is_virtual_node(&source) {
                continue;
            }
            sources.push(SystemSource {
                held,
                nodes: vec![source],
            });
        }
        // `Filename Type Size Used Priority`, after a header line. A swap
        // file lives on a filesystem that is mounted, and an eject of its
        // drive is refused as busy by the kernel; only partitions name a
        // drive here.
        for line in swaps.lines().skip(1) {
            let mut fields = line.split_whitespace();
            let (Some(name), Some(kind)) = (fields.next(), fields.next()) else {
                continue;
            };
            let name = unescape(name);
            if kind != "partition" || is_virtual_node(&name) {
                continue;
            }
            sources.push(SystemSource {
                held: SystemUse::Swap,
                nodes: vec![name],
            });
        }
        Self { sources }
    }

    /// Reads the running system's mounts and swap under `roots`, and follows
    /// each node through `/dev`'s links and `/sys/class/block/*/slaves` to
    /// the partitions it is built on (LVM, LUKS, RAID). `None` when the mount
    /// table cannot be read: then nothing about any drive is known.
    pub fn read(roots: &Roots) -> Option<Self> {
        let mounts = std::fs::read_to_string(roots.proc.join("self/mounts"))
            .or_else(|_| std::fs::read_to_string(roots.proc.join("mounts")))
            .ok()?;
        // No swap file is no swap.
        let swaps = std::fs::read_to_string(roots.proc.join("swaps")).unwrap_or_default();
        let mut found = Self::parse(&mounts, &swaps);
        for source in &mut found.sources {
            let mut nodes = Vec::new();
            for node in &source.nodes {
                underlying(roots, node, &mut nodes, 0);
            }
            source.nodes = nodes;
        }
        Some(found)
    }
}

/// `node`, the node its link resolves to, and every node under it, in that
/// order and without repeats.
fn underlying(roots: &Roots, node: &str, into: &mut Vec<String>, depth: usize) {
    if depth > 8 || into.iter().any(|known| known == node) {
        return;
    }
    into.push(node.to_string());
    let real = std::fs::canonicalize(node)
        .ok()
        .map(|path| path.to_string_lossy().into_owned());
    let real = real.unwrap_or_else(|| node.to_string());
    if real != node && !into.contains(&real) {
        into.push(real.clone());
    }
    let Some(name) = Path::new(&real).file_name() else {
        return;
    };
    let slaves = roots.sys.join("class/block").join(name).join("slaves");
    let Ok(entries) = std::fs::read_dir(slaves) else {
        return;
    };
    let mut names: Vec<String> = entries
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    for name in names {
        underlying(roots, &format!("/dev/{name}"), into, depth + 1);
    }
}

/// Whether `node` names `device`: its node or one of its links. No I/O:
/// [`SystemSources::read`] has already added what each node resolves to.
pub fn names_node(node: &str, device: &PlatformDevice) -> bool {
    node == device.address.device_path
        || node == device.block.device_path
        || device.block.symlinks.iter().any(|link| link == node)
}

/// The UDisks2 drive object a volume is on: its own, or, for an unlocked
/// encrypted volume, its container's (following a chain of them).
pub fn drive_object_of<'a>(
    device: &'a PlatformDevice,
    devices: &'a [PlatformDevice],
) -> Option<&'a str> {
    let mut current = device;
    for _ in 0..8 {
        if let Some(drive) = &current.drive
            && !drive.object_path.is_empty()
            && drive.object_path != "/"
        {
            return Some(drive.object_path.as_str());
        }
        let backing = current.block.crypto_backing.as_deref()?;
        current = devices
            .iter()
            .find(|other| other.address.object_path == backing)?;
    }
    None
}

/// Which drives hold something of the running system: the one answer every
/// caller asks.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SystemDrives {
    /// Drive object → what it holds.
    held: BTreeMap<String, SystemUse>,
    /// The mount table was read and every system source was traced to a
    /// drive: a drive not in `held` is free.
    complete: bool,
}

impl SystemDrives {
    /// Nothing known: every drive is [`SystemUse::Unknown`].
    pub fn unknown() -> Self {
        Self::default()
    }

    /// Works out which of `devices`' drives hold what `sources` names, and
    /// what UDisks2 itself reports mounted at a system path. `None` sources
    /// (the mount table could not be read) know nothing.
    pub fn find(devices: &[PlatformDevice], sources: Option<&SystemSources>) -> Self {
        let Some(sources) = sources else {
            return Self::unknown();
        };
        let mut found = Self {
            held: BTreeMap::new(),
            complete: true,
        };
        for source in &sources.sources {
            let mut traced = false;
            for device in devices
                .iter()
                .filter(|device| source.nodes.iter().any(|node| names_node(node, device)))
            {
                traced |= found.hold(device, devices, source.held.clone());
            }
            if !traced {
                found.complete = false;
            }
        }
        // UDisks2's own list of where each volume is mounted, for a system
        // mount the kernel names in a way no link matches.
        for device in devices {
            if let Some(point) = device
                .mount_points()
                .into_iter()
                .find(|point| is_system_path(point))
            {
                let held = if point == Path::new("/") {
                    SystemUse::RunningSystem
                } else {
                    SystemUse::SystemFolder { path: point }
                };
                found.hold(device, devices, held);
            }
        }
        // A btrfs filesystem across several devices is held on all of them.
        let held_uuids: Vec<(String, SystemUse)> = devices
            .iter()
            .filter(|device| device.block.id_type.as_deref() == Some("btrfs"))
            .filter_map(|device| {
                let held = found.of_device(device, devices);
                let uuid = device.block.id_uuid.clone()?;
                (!held.permits_removal() && held != SystemUse::Unknown).then_some((uuid, held))
            })
            .collect();
        for (uuid, held) in held_uuids {
            for device in devices
                .iter()
                .filter(|device| device.block.id_uuid.as_deref() == Some(uuid.as_str()))
            {
                found.hold(device, devices, held.clone());
            }
        }
        found
    }

    /// Marks the drive `device` is on; false when it is on none.
    fn hold(
        &mut self,
        device: &PlatformDevice,
        devices: &[PlatformDevice],
        held: SystemUse,
    ) -> bool {
        let Some(drive) = drive_object_of(device, devices) else {
            return false;
        };
        let entry = self
            .held
            .entry(drive.to_string())
            .or_insert(SystemUse::Free);
        *entry = std::mem::replace(entry, SystemUse::Free).strongest(held);
        true
    }

    /// What the drive with this UDisks2 object holds. A drive that is held
    /// says what holds it even when something else was not traced; any
    /// other is Free only when everything was.
    pub fn of_drive(&self, drive_object: Option<&str>) -> SystemUse {
        let Some(drive) = drive_object.filter(|drive| !drive.is_empty() && *drive != "/") else {
            return SystemUse::Unknown;
        };
        match self.held.get(drive) {
            Some(held) => held.clone(),
            None if self.complete => SystemUse::Free,
            None => SystemUse::Unknown,
        }
    }

    /// What the drive a volume is on holds.
    pub fn of_device(&self, device: &PlatformDevice, devices: &[PlatformDevice]) -> SystemUse {
        self.of_drive(drive_object_of(device, devices))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{BlockInfo, DeviceAddress, DriveInfo};

    fn volume(object: &str, node: &str, drive: Option<&str>, mounted: &[&str]) -> PlatformDevice {
        PlatformDevice {
            address: DeviceAddress {
                object_path: object.into(),
                device_path: node.into(),
            },
            block: BlockInfo {
                device_path: node.into(),
                id_type: Some("ext4".into()),
                mount_points: mounted.iter().map(PathBuf::from).collect(),
                partition_number: Some(1),
                ..BlockInfo::default()
            },
            drive: drive.map(|drive| DriveInfo {
                object_path: drive.into(),
                removable: true,
                connection_bus: "usb".into(),
                ..DriveInfo::default()
            }),
            mount_point: mounted.first().map(PathBuf::from),
        }
    }

    const MOUNTS: &str = "\
/dev/sdb2 / ext4 rw,relatime 0 0
proc /proc proc rw 0 0
/dev/sdb1 /boot/efi vfat rw 0 0
/dev/nvme0n1p3 /home ext4 rw 0 0
/dev/sdc1 /media/me/STICK vfat rw 0 0
/dev/loop3 /snap/core/1 squashfs ro 0 0
tmpfs /run tmpfs rw 0 0
";
    const SWAPS: &str = "\
Filename\t\t\t\tType\t\tSize\t\tUsed\t\tPriority
/dev/sdd2                               partition\t8388604\t\t0\t\t-2
/swap.img                               file\t\t2097148\t\t0\t\t-3
/dev/zram0                              partition\t4096\t\t0\t\t100
";

    #[test]
    fn the_mount_table_names_the_system_paths_and_swap_partitions_only() {
        let sources = SystemSources::parse(MOUNTS, SWAPS);
        let held: Vec<(&SystemUse, &str)> = sources
            .sources
            .iter()
            .map(|source| (&source.held, source.nodes[0].as_str()))
            .collect();
        assert_eq!(
            held,
            [
                (&SystemUse::RunningSystem, "/dev/sdb2"),
                (
                    &SystemUse::SystemFolder {
                        path: "/boot/efi".into()
                    },
                    "/dev/sdb1"
                ),
                (
                    &SystemUse::SystemFolder {
                        path: "/home".into()
                    },
                    "/dev/nvme0n1p3"
                ),
                (&SystemUse::Swap, "/dev/sdd2"),
            ]
        );
    }

    #[test]
    fn an_escaped_mount_point_is_read_as_the_kernel_meant_it() {
        let sources = SystemSources::parse(
            "/dev/sdb1 /run/live/medium vfat ro 0 0\n/dev/sdc1 /media/me/My\\040Stick vfat rw 0 0\n",
            "",
        );
        assert_eq!(sources.sources.len(), 1);
        assert_eq!(unescape("My\\040Stick"), "My Stick");
    }

    #[test]
    fn the_running_system_on_a_usb_drive_holds_every_volume_on_that_drive() {
        let devices = [
            volume("/b/sdb1", "/dev/sdb1", Some("/d/usb"), &["/boot/efi"]),
            volume("/b/sdb2", "/dev/sdb2", Some("/d/usb"), &["/"]),
            volume("/b/sdb3", "/dev/sdb3", Some("/d/usb"), &["/media/me/DATA"]),
            volume(
                "/b/sdc1",
                "/dev/sdc1",
                Some("/d/stick"),
                &["/media/me/STICK"],
            ),
        ];
        let sources = SystemSources::parse(
            "/dev/sdb2 / ext4 rw 0 0\n/dev/sdb1 /boot/efi vfat rw 0 0\n",
            "",
        );
        let system = SystemDrives::find(&devices, Some(&sources));
        // The second partition of the system disk is on the same drive.
        assert_eq!(
            system.of_device(&devices[2], &devices),
            SystemUse::RunningSystem
        );
        assert_eq!(system.of_device(&devices[3], &devices), SystemUse::Free);
        assert!(!system.of_device(&devices[0], &devices).permits_removal());
    }

    #[test]
    fn home_and_swap_on_an_external_disk_hold_it() {
        let devices = [
            volume("/b/nvme", "/dev/nvme0n1p2", Some("/d/nvme"), &["/"]),
            volume("/b/sdb1", "/dev/sdb1", Some("/d/ext"), &[]),
            volume("/b/sdc1", "/dev/sdc1", Some("/d/swap"), &[]),
        ];
        let sources = SystemSources::parse(
            "/dev/nvme0n1p2 / ext4 rw 0 0\n/dev/sdb1 /home ext4 rw 0 0\n",
            "Filename Type Size Used Priority\n/dev/sdc1 partition 1 0 -2\n",
        );
        let system = SystemDrives::find(&devices, Some(&sources));
        assert_eq!(
            system.of_drive(Some("/d/ext")),
            SystemUse::SystemFolder {
                path: "/home".into()
            }
        );
        assert_eq!(system.of_drive(Some("/d/swap")), SystemUse::Swap);
        assert_eq!(system.of_drive(Some("/d/nvme")), SystemUse::RunningSystem);
    }

    #[test]
    fn an_unlocked_root_is_traced_through_its_container_and_a_link() {
        let container = volume("/b/sdb2", "/dev/sdb2", Some("/d/usb"), &[]);
        let mut cleartext = volume("/b/dm0", "/dev/dm-0", None, &[]);
        cleartext.block.crypto_backing = Some("/b/sdb2".into());
        cleartext.block.symlinks = vec!["/dev/mapper/luks-1234".into()];
        let devices = [container, cleartext];
        let sources = SystemSources::parse("/dev/mapper/luks-1234 / ext4 rw 0 0\n", "");
        let system = SystemDrives::find(&devices, Some(&sources));
        assert_eq!(system.of_drive(Some("/d/usb")), SystemUse::RunningSystem);
    }

    #[test]
    fn a_root_traced_through_its_slaves_holds_the_physical_volume() {
        let pv = volume("/b/sdb3", "/dev/sdb3", Some("/d/usb"), &[]);
        let lv = volume("/b/dm1", "/dev/dm-1", None, &[]);
        let devices = [pv, lv];
        let sources = SystemSources {
            sources: vec![SystemSource {
                held: SystemUse::RunningSystem,
                nodes: vec![
                    "/dev/mapper/vg-root".into(),
                    "/dev/dm-1".into(),
                    "/dev/sdb3".into(),
                ],
            }],
        };
        let system = SystemDrives::find(&devices, Some(&sources));
        assert_eq!(system.of_drive(Some("/d/usb")), SystemUse::RunningSystem);
        assert_eq!(system.of_drive(Some("/d/other")), SystemUse::Free);
    }

    #[test]
    fn what_cannot_be_told_is_unknown_and_never_free() {
        let stick = volume("/b/sdc1", "/dev/sdc1", Some("/d/stick"), &["/media/me/S"]);
        let devices = [stick];
        // No mount table at all.
        assert_eq!(
            SystemDrives::find(&devices, None).of_drive(Some("/d/stick")),
            SystemUse::Unknown
        );
        // A ZFS root: its pool's drives are not named anywhere.
        let zfs = SystemSources::parse("rpool/ROOT/ubuntu / zfs rw 0 0\n", "");
        assert_eq!(
            SystemDrives::find(&devices, Some(&zfs)).of_drive(Some("/d/stick")),
            SystemUse::Unknown
        );
        // An overlay root (a live session) names no drive and holds none.
        let live = SystemSources::parse("overlay / overlay rw 0 0\n", "");
        assert_eq!(
            SystemDrives::find(&devices, Some(&live)).of_drive(Some("/d/stick")),
            SystemUse::Free
        );
        // A root UDisks2 does not list: any drive might hold it.
        let sources = SystemSources::parse("/dev/sda2 / ext4 rw 0 0\n", "");
        let system = SystemDrives::find(&devices, Some(&sources));
        assert_eq!(system.of_drive(Some("/d/stick")), SystemUse::Unknown);
        // A volume with no drive object.
        let complete = SystemDrives::find(&devices, Some(&SystemSources::default()));
        assert_eq!(complete.of_drive(None), SystemUse::Unknown);
        assert_eq!(complete.of_drive(Some("/d/stick")), SystemUse::Free);
        assert!(!SystemUse::Unknown.permits_removal());
    }

    #[test]
    fn a_btrfs_root_across_two_drives_holds_both() {
        let mut first = volume("/b/sdb1", "/dev/sdb1", Some("/d/one"), &["/"]);
        let mut second = volume("/b/sdc1", "/dev/sdc1", Some("/d/two"), &[]);
        for device in [&mut first, &mut second] {
            device.block.id_type = Some("btrfs".into());
            device.block.id_uuid = Some("same".into());
        }
        let devices = [first, second];
        let system = SystemDrives::find(&devices, Some(&SystemSources::default()));
        assert_eq!(system.of_drive(Some("/d/two")), SystemUse::RunningSystem);
    }

    #[test]
    fn a_refusal_names_its_reason_and_reads_back() {
        let home = SystemUse::SystemFolder {
            path: "/home".into(),
        };
        assert_eq!(
            home.refusal_detail().as_deref(),
            Some("storage.system.folder:/home")
        );
        assert_eq!(
            SystemUse::from_refusal("storage.system.folder:/home"),
            Some(home)
        );
        assert_eq!(
            SystemUse::from_refusal("storage.system.running_system"),
            Some(SystemUse::RunningSystem)
        );
        assert_eq!(SystemUse::from_refusal("storage.erase.busy"), None);
        assert_eq!(SystemUse::Free.refusal(), None);
    }

    #[test]
    fn slaves_are_followed_in_a_fixture_tree() {
        let root = tempfile::tempdir().unwrap();
        let roots = Roots::at(root.path());
        std::fs::create_dir_all(roots.proc.join("self")).unwrap();
        std::fs::write(
            roots.proc.join("self/mounts"),
            "/dev/dm-7 / ext4 rw 0 0\n/dev/dm-8 /mnt/data ext4 rw 0 0\n",
        )
        .unwrap();
        let slaves = roots.sys.join("class/block/dm-7/slaves");
        std::fs::create_dir_all(slaves.join("dm-6")).unwrap();
        std::fs::create_dir_all(roots.sys.join("class/block/dm-6/slaves/sdz3")).unwrap();
        let read = SystemSources::read(&roots).unwrap();
        assert_eq!(read.sources.len(), 1);
        assert_eq!(
            read.sources[0].nodes,
            ["/dev/dm-7", "/dev/dm-6", "/dev/sdz3"]
        );
        // No mount table: nothing is known.
        assert_eq!(
            SystemSources::read(&Roots::at(root.path().join("none"))),
            None
        );
    }
}
