//! Which block devices are drives a person would call one (ticket 122), shared
//! by Better Files' sidebar and Better Manager's Drives section (ticket 127).
//! Nothing here reads the machine but `names_device`'s link resolution and
//! `running_system_name`, which asks `better_core::host`.

use std::path::{Path, PathBuf};

use storage_platform::model::PlatformDevice;
pub use storage_platform::system_use::SystemUse;
use storage_platform::system_use::{SystemDrives, SystemSource, SystemSources};

use crate::choice::AutoMount;

/// What kind of drive, for its icon.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum DriveKind {
    /// The drive the running system is on (`/`).
    System,
    /// Another internal partition.
    Internal,
    /// A USB stick, an external disk, a memory card.
    Removable,
    /// A CD, DVD or Blu-ray with media in it.
    Optical,
    /// A phone or a camera, through GVfs.
    Phone,
    /// A network share or a server GVfs has mounted.
    Network,
}

/// One drive.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Drive {
    /// Stable within a scan: the UDisks2 object path, or the mount point.
    pub key: String,
    /// Its label, when it has one; otherwise it is named by its size.
    pub label: Option<String>,
    pub kind: DriveKind,
    pub mount_point: Option<PathBuf>,
    /// The UDisks2 block object, for mounting and ejecting.
    pub object_path: Option<String>,
    /// Its device node (`/dev/sdb1`).
    pub device_path: Option<String>,
    /// The UDisks2 drive object the volume is on, for powering it off.
    pub drive_object: Option<String>,
    /// For an unlocked encrypted volume: the locked container under it,
    /// locked again when the drive is ejected.
    pub backing: Option<String>,
    /// An encrypted volume that is still locked: it asks for its passphrase
    /// before it mounts.
    pub locked: bool,
    /// The filesystem's UUID: a btrfs filesystem on several devices is
    /// one drive.
    pub uuid: Option<String>,
    /// The size UDisks2 reports, for a drive that is not mounted.
    pub size: u64,
    /// Whether it can be ejected (a removable or optical drive that holds
    /// nothing of the running system).
    pub ejectable: bool,
    /// What of the running system is on its drive (ticket 171): Eject,
    /// Format and unmounting are offered only for `Free`.
    pub system_use: SystemUse,
    /// How full it is: `None` until read, and for a drive that is not
    /// mounted or did not answer.
    pub capacity: Option<Capacity>,
    /// Whether it mounts automatically (ticket 127): `None` for a drive the
    /// choice does not apply to (a phone, a share, one with no UUID) and
    /// until the system's files have been read.
    pub auto_mount: Option<AutoMount>,
    /// For a removable drive: a USB stick, a memory card, or an external
    /// disk, which the sidebar draws differently. `None` for other kinds.
    pub form: Option<RemovableForm>,
}

/// What a drive is called, before a window puts it into its own words
/// (ticket 138). Better Files and Better Manager both name drives through
/// [`Drive::naming`], so a drive never has two names.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DriveName<'a> {
    /// The running system's drive, by the running system's name ("Zorin
    /// OS"), whatever its filesystem label says.
    System(&'a str),
    /// The running system's drive when its os-release names no system: the
    /// window's fixed wording ("System drive"), never a guessed name.
    UnnamedSystem,
    /// Its filesystem label.
    Label(&'a str),
    /// No label: named by its size, in bytes.
    Size(u64),
}

impl<'a> DriveName<'a> {
    /// The name of the drive the running system is on, when the system
    /// calls itself `system`: also what a path bar's root crumb says.
    pub fn of_system(system: Option<&'a str>) -> Self {
        match system {
            Some(name) => DriveName::System(name),
            None => DriveName::UnnamedSystem,
        }
    }
}

/// The running system's name, read once through `better_core::host`, the one
/// place that reads os-release.
pub fn running_system_name() -> Option<&'static str> {
    static NAME: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    NAME.get_or_init(better_core::host::running_system_name)
        .as_deref()
}

impl Drive {
    /// What this drive is called on this machine.
    pub fn naming(&self) -> DriveName<'_> {
        self.naming_with(running_system_name())
    }

    /// What this drive is called when the running system calls itself
    /// `system`. Only the drive the running system is on takes that name;
    /// another Linux install on another partition keeps its label or its size
    /// (its own os-release is not read).
    pub fn naming_with<'a>(&'a self, system: Option<&'a str>) -> DriveName<'a> {
        if self.kind == DriveKind::System {
            return DriveName::of_system(system);
        }
        match &self.label {
            Some(label) => DriveName::Label(label),
            None => DriveName::Size(
                self.capacity
                    .map(|capacity| capacity.total)
                    .unwrap_or(self.size),
            ),
        }
    }
}

/// Buses a drive is plugged into and pulled out of. A USB hard disk often
/// reports neither `Removable` nor `MediaRemovable`, and is still a drive
/// that comes and goes (ticket 127 review).
pub const HOT_PLUGGABLE_BUSES: &[&str] = &["usb", "ieee1394", "thunderbolt", "sdio", "mmc"];

/// Whether a drive is removable, from what UDisks2 says of it. The daemon
/// and both windows decide it here, so a drive is never internal to one
/// and removable to another.
pub fn hot_pluggable(removable: bool, media_removable: bool, connection_bus: &str) -> bool {
    removable || media_removable || HOT_PLUGGABLE_BUSES.contains(&connection_bus)
}

/// What a removable drive is, by what UDisks2 says it takes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RemovableForm {
    /// UDisks2 reports `thumb` media: a USB flash drive.
    Stick,
    /// A memory card (`flash_*` media, or a card reader on `sdio`/`mmc`).
    Card,
    /// Anything else: an external hard disk or SSD. UDisks2 marks sticks it
    /// recognizes, so an unmarked drive is drawn as a disk rather than
    /// guessed to be a stick.
    Disk,
}

/// A removable drive's form from its UDisks2 drive record.
pub fn removable_form(drive: &storage_platform::model::DriveInfo) -> RemovableForm {
    let media = |prefix: &str| drive.media.iter().any(|entry| entry.starts_with(prefix));
    if media("thumb") {
        RemovableForm::Stick
    } else if media("flash") || matches!(drive.connection_bus.as_str(), "sdio" | "mmc") {
        RemovableForm::Card
    } else {
        RemovableForm::Disk
    }
}

/// A mounted filesystem's size and free space, in bytes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Capacity {
    pub total: u64,
    /// What an ordinary user can still write (`f_bavail`).
    pub free: u64,
}

/// The share of a drive in use at which its bar turns to the caution
/// colour: 90%, the point at which Windows 11 File Explorer turns a drive's
/// bar red.
pub const NEARLY_FULL: f64 = 0.9;

impl Capacity {
    /// The share in use, 0.0 to 1.0.
    pub fn used_fraction(&self) -> f64 {
        if self.total == 0 {
            return 0.0;
        }
        (self.total.saturating_sub(self.free) as f64 / self.total as f64).clamp(0.0, 1.0)
    }

    pub fn is_nearly_full(&self) -> bool {
        self.total > 0 && self.used_fraction() >= NEARLY_FULL
    }
}

/// Filesystems a person keeps files on. Anything else on a block device
/// (swap, an unformatted partition, a RAID member) is not shown as a drive,
/// except an encrypted container ([`ENCRYPTED`]) while it is locked.
pub const DRIVE_FILESYSTEMS: &[&str] = &[
    "ext2", "ext3", "ext4", "btrfs", "xfs", "f2fs", "vfat", "exfat", "ntfs", "ntfs3", "hfsplus",
    "iso9660", "udf",
];

/// Mount points the system keeps for itself.
pub fn is_system_mount(mount_point: &Path) -> bool {
    const SYSTEM: &[&str] = &[
        "/boot",
        "/efi",
        "/recovery",
        "/snap",
        "/var",
        "/usr",
        "/opt",
        "/proc",
        "/sys",
        "/dev",
        "/run",
    ];
    if mount_point == Path::new("/") {
        return false;
    }
    // Removable media under `/run/media` is a person's drive.
    if mount_point.starts_with("/run/media") {
        return false;
    }
    SYSTEM.iter().any(|prefix| mount_point.starts_with(prefix))
}

/// A partition named as the firmware's or the vendor's: EFI, recovery,
/// Microsoft's reserved one. Only on an internal disk: a Raspberry Pi card's
/// `boot` partition or a stick someone labelled EFI is still a drive.
pub fn is_system_label(label: &str) -> bool {
    let label = label.to_ascii_lowercase();
    ["efi", "esp", "recovery", "system reserved", "msr", "boot"]
        .iter()
        .any(|name| label == *name || label.starts_with(&format!("{name} ")))
}

/// Partition types a desktop never shows, as UDisks2's own udev rules mark
/// them `UDISKS_IGNORE` (`80-udisks2.rules`), for a UDisks2 that did not:
/// on GPT the EFI system partition, BIOS boot, Microsoft reserved, Windows
/// recovery, LVM and RAID, the Linux extended boot partition and Apple's
/// boot; on MBR the hidden, recovery, diagnostic, hibernation, EFI and
/// vendor-tool types.
pub const HIDDEN_PARTITION_TYPES: &[&str] = &[
    "c12a7328-f81f-11d2-ba4b-00a0c93ec93b",
    "21686148-6449-6e6f-744e-656564454649",
    "e3c9e316-0b5c-4db8-817d-f92df00215ae",
    "de94bba4-06d1-4d40-a16a-bfd50179d6ac",
    "a19d880f-05fc-4d3b-a006-743f0f84911e",
    "e6d6d379-f507-44c2-a23c-238f2a3df928",
    "bc13c2ff-59e6-4262-a352-b275fd6f7172",
    "426f6f74-0000-11aa-aa11-00306543ecac",
    "0x0",
    "0x11",
    "0x12",
    "0x14",
    "0x16",
    "0x17",
    "0x1b",
    "0x1c",
    "0x1e",
    "0x27",
    "0x3d",
    "0x84",
    "0x8d",
    "0x8e",
    "0x90",
    "0x91",
    "0x92",
    "0x93",
    "0x97",
    "0x98",
    "0x9a",
    "0x9b",
    "0xab",
    "0xbb",
    "0xc2",
    "0xc3",
    "0xdd",
    "0xde",
    "0xef",
    "0xfd",
];

/// Whether a partition's type is one a desktop never shows.
pub fn is_hidden_partition_type(kind: &str) -> bool {
    let kind = kind.to_ascii_lowercase();
    HIDDEN_PARTITION_TYPES.contains(&kind.as_str())
}

/// Encrypted containers UDisks2 can unlock: LUKS and BitLocker.
pub const ENCRYPTED: &[&str] = &["crypto_LUKS", "crypto_BITLK"];

/// Every place a device is mounted: UDisks2's whole list when the storage
/// layer has it, else the one it named.
pub fn mount_points_of(device: &PlatformDevice) -> Vec<PathBuf> {
    device.mount_points()
}

/// Whether `source` (a mount table's `/dev/mapper/luks-…`, say) names
/// `device`: its node, one of its symlinks, or the same node once links
/// are followed.
pub fn names_device(source: &str, device: &PlatformDevice) -> bool {
    if source == device.address.device_path
        || device.block.symlinks.iter().any(|link| link == source)
    {
        return true;
    }
    let (Ok(source), Ok(node)) = (
        std::fs::canonicalize(source),
        std::fs::canonicalize(&device.address.device_path),
    ) else {
        return false;
    };
    source == node
}

/// The drives among UDisks2's block devices. `root_source` is the device the
/// mount table shows at `/`, which UDisks2 may name differently (a btrfs
/// volume it lists at `/home`, `/dev/mapper/luks-…` for its `/dev/dm-0`).
/// What else the running system uses is not known here: see
/// [`drives_with_system`].
pub fn drives_from_devices(devices: &[PlatformDevice], root_source: Option<&str>) -> Vec<Drive> {
    let sources = SystemSources {
        sources: root_source
            .map(|source| SystemSource::node(SystemUse::RunningSystem, source))
            .into_iter()
            .collect(),
        ..SystemSources::default()
    };
    drives_with_system(
        devices,
        root_source,
        &SystemDrives::find(devices, Some(&sources)),
    )
}

/// The drives among UDisks2's block devices, each with what `system` says
/// its drive holds of the running system (ticket 171): a drive that holds
/// anything, or that cannot be told apart from one, is never ejectable.
pub fn drives_with_system(
    devices: &[PlatformDevice],
    root_source: Option<&str>,
    system: &SystemDrives,
) -> Vec<Drive> {
    let mut drives: Vec<Drive> = Vec::new();
    // A container that is unlocked is shown as the volume inside it.
    let unlocked: Vec<&str> = devices
        .iter()
        .filter_map(|device| device.block.crypto_backing.as_deref())
        .collect();
    // The drive object each volume is on: an unlocked volume is on its
    // container's.
    fn drive_of<'a>(
        device: &'a PlatformDevice,
        devices: &'a [PlatformDevice],
    ) -> Option<&'a storage_platform::model::DriveInfo> {
        device.drive.as_ref().or_else(|| {
            let backing = device.block.crypto_backing.as_deref()?;
            devices
                .iter()
                .find(|other| other.address.object_path == backing)?
                .drive
                .as_ref()
        })
    }
    let is_root = |device: &PlatformDevice| {
        mount_points_of(device)
            .iter()
            .any(|point| point == Path::new("/"))
            || root_source.is_some_and(|source| names_device(source, device))
    };
    for device in devices {
        let path = device.address.device_path.as_str();
        if ["/dev/loop", "/dev/ram", "/dev/zram"]
            .iter()
            .any(|prefix| path.starts_with(prefix))
        {
            continue;
        }
        if device.block.hint_ignore
            || device
                .block
                .partition_type
                .as_deref()
                .is_some_and(is_hidden_partition_type)
        {
            continue;
        }
        // A whole disk that holds partitions is listed through them (an
        // ISO image written to a stick shows a filesystem on both).
        if device.block.partition_table && device.block.partition_number.is_none() {
            continue;
        }
        let Some(filesystem) = device.block.id_type.as_deref() else {
            continue;
        };
        let locked = ENCRYPTED.contains(&filesystem);
        if locked && unlocked.contains(&device.address.object_path.as_str()) {
            continue;
        }
        if !locked && !DRIVE_FILESYSTEMS.contains(&filesystem) {
            continue;
        }
        let is_system = is_root(device);
        let points = mount_points_of(device);
        // Shown at `/` when it is the system's, else at the first place a
        // person keeps files; a volume mounted only where the system keeps
        // itself (`/boot/efi`) is not a drive.
        let mount_point = if is_system {
            Some(PathBuf::from("/"))
        } else {
            match points.iter().find(|point| !is_system_mount(point)) {
                Some(point) => Some(point.clone()),
                None if points.is_empty() => None,
                None => continue,
            }
        };
        let drive_info = drive_of(device, devices);
        let removable = drive_info.is_some_and(|drive| {
            hot_pluggable(
                drive.removable,
                drive.media_removable,
                &drive.connection_bus,
            )
        });
        let label = device
            .block
            .id_label
            .as_deref()
            .map(str::trim)
            .filter(|label| !label.is_empty())
            .map(str::to_string);
        if !removable && label.as_deref().is_some_and(is_system_label) {
            continue;
        }
        let optical = drive_info.is_some_and(|drive| drive.optical);
        // An unmounted small FAT partition on an internal disk is the EFI
        // system partition whatever it is called.
        if filesystem == "vfat"
            && !removable
            && mount_point.is_none()
            && device.block.size <= 1 << 30
        {
            continue;
        }
        let drive_object = drive_info
            .map(|drive| drive.object_path.clone())
            .filter(|path| !path.is_empty() && path != "/");
        // The running system's own volume holds it whatever the drive
        // lookup says; every other volume is what its drive holds.
        let system_use = if is_system {
            SystemUse::RunningSystem.strongest(system.of_device(device, devices))
        } else {
            system.of_device(device, devices)
        };
        let kind = if is_system {
            DriveKind::System
        } else if optical {
            DriveKind::Optical
        } else if removable {
            DriveKind::Removable
        } else {
            DriveKind::Internal
        };
        let uuid = device.block.id_uuid.clone();
        // A btrfs filesystem on several devices is one drive: the mounted
        // device stands for it.
        if filesystem == "btrfs"
            && let Some(uuid) = &uuid
            && let Some(known) = drives
                .iter_mut()
                .find(|known| known.uuid.as_ref() == Some(uuid) && !known.locked)
        {
            if known.mount_point.is_none() && mount_point.is_some() {
                known.mount_point = mount_point;
                known.kind = kind;
                known.system_use = known.system_use.clone().strongest(system_use.clone());
                known.ejectable = known.ejectable && known.system_use.permits_removal();
                known.key = device.address.object_path.clone();
                known.object_path = Some(device.address.object_path.clone());
                known.device_path = Some(device.address.device_path.clone());
            }
            continue;
        }
        drives.push(Drive {
            key: device.address.object_path.clone(),
            label,
            kind,
            mount_point,
            object_path: Some(device.address.object_path.clone()),
            device_path: Some(device.address.device_path.clone()),
            drive_object,
            backing: device.block.crypto_backing.clone(),
            locked,
            uuid,
            size: device.block.size,
            // Linux on an external SSD or a live USB, `/home` or swap on
            // a USB disk: that drive is never offered for ejecting.
            ejectable: system_use.permits_removal()
                && (removable || optical || drive_info.is_some_and(|drive| drive.ejectable)),
            system_use,
            capacity: None,
            auto_mount: None,
            form: (kind == DriveKind::Removable)
                .then(|| drive_info.map_or(RemovableForm::Disk, removable_form)),
        });
    }
    drives
}

/// What the drive of the UDisks2 block `object_path` holds of the running
/// system (ticket 171), asked of UDisks2 over `system` and of the kernel's
/// mount table now. Blocking: for a worker thread such as the daemon's.
pub fn system_use_now(system: &zbus::blocking::Connection, object_path: &str) -> SystemUse {
    use storage_platform::traits::DeviceControl;
    let udisks = storage_platform::UDisks2::from_connection(system.inner().clone());
    let Ok(devices) = zbus::block_on(udisks.enumerate()) else {
        return SystemUse::Unknown;
    };
    let sources = udisks.system_sources();
    let found = SystemDrives::find(&devices, sources.as_ref());
    devices
        .iter()
        .find(|device| device.address.object_path == object_path)
        .map_or(SystemUse::Unknown, |device| {
            found.of_device(device, &devices)
        })
}

/// Names a JSON file of drives a sandbox shows instead of the machine's
/// own, in Better Files and Better Manager alike (`[{"label", "kind", "mount_point", "size", "total", "free",
/// "ejectable", "locked"}]`), so screenshots never show a real drive.
/// Honoured in debug builds only.
pub const FIXTURE_VARIABLE: &str = "BETTER_FILES_TEST_DRIVES";

#[derive(serde::Deserialize)]
struct FixtureDrive {
    label: Option<String>,
    kind: String,
    mount_point: Option<PathBuf>,
    #[serde(default)]
    size: u64,
    total: Option<u64>,
    free: Option<u64>,
    #[serde(default)]
    ejectable: bool,
    #[serde(default)]
    locked: bool,
    /// For a removable fixture: `stick`, `card` or `disk`.
    #[serde(default)]
    form: Option<String>,
    uuid: Option<String>,
    /// Why it did not mount, as systemd would say (`timeout`).
    failure: Option<String>,
    /// What of the running system its drive holds: `free`, `running_system`,
    /// `swap`, `unknown`, or a system folder's path (`/home`). By default a
    /// `system` fixture holds the running system and any other is free.
    system_use: Option<String>,
}

/// The drives a fixture file names, unsorted.
pub fn fixture_drives(path: &Path) -> Vec<Drive> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let Ok(listed) = serde_json::from_str::<Vec<FixtureDrive>>(&text) else {
        return Vec::new();
    };
    let drives: Vec<Drive> = listed
        .into_iter()
        .enumerate()
        .map(|(index, fixture)| Drive {
            system_use: match fixture.system_use.as_deref() {
                Some("free") => SystemUse::Free,
                Some("running_system") => SystemUse::RunningSystem,
                Some("swap") => SystemUse::Swap,
                Some("unknown") => SystemUse::Unknown,
                Some(path) if path.starts_with('/') => SystemUse::SystemFolder {
                    path: PathBuf::from(path),
                },
                _ if fixture.kind == "system" => SystemUse::RunningSystem,
                _ => SystemUse::Free,
            },
            key: format!("fixture-{index}"),
            label: fixture.label,
            kind: match fixture.kind.as_str() {
                "system" => DriveKind::System,
                "removable" => DriveKind::Removable,
                "optical" => DriveKind::Optical,
                "phone" => DriveKind::Phone,
                "network" => DriveKind::Network,
                _ => DriveKind::Internal,
            },
            mount_point: fixture.mount_point,
            object_path: None,
            device_path: None,
            // A fixture's internal drive is on a drive of its own, as a real one is.
            drive_object: (fixture.kind == "internal").then(|| format!("fixture-drive-{index}")),
            backing: None,
            locked: fixture.locked,
            uuid: fixture.uuid,
            size: fixture.size,
            ejectable: fixture.ejectable,
            capacity: fixture
                .total
                .zip(fixture.free)
                .map(|(total, free)| Capacity { total, free }),
            form: match fixture.form.as_deref() {
                Some("stick") => Some(RemovableForm::Stick),
                Some("card") => Some(RemovableForm::Card),
                Some("disk") => Some(RemovableForm::Disk),
                _ => None,
            },
            auto_mount: fixture.failure.map(|failure| crate::choice::AutoMount {
                on: true,
                locked: None,
                failure: Some(failure),
            }),
        })
        .collect();
    drives
}

#[cfg(test)]
mod tests {
    use super::{Capacity, Drive, DriveKind, DriveName};

    fn drive(kind: DriveKind, label: Option<&str>) -> Drive {
        Drive {
            key: "key".into(),
            label: label.map(str::to_string),
            kind,
            mount_point: None,
            object_path: None,
            device_path: None,
            drive_object: None,
            backing: None,
            locked: false,
            uuid: None,
            size: 263_000_000_000,
            ejectable: false,
            system_use: super::SystemUse::Free,
            capacity: None,
            auto_mount: None,
            form: None,
        }
    }

    #[test]
    fn the_running_systems_drive_is_named_after_it_with_or_without_a_label() {
        let unlabelled = drive(DriveKind::System, None);
        let labelled = drive(DriveKind::System, Some("rootfs"));
        for system in [&unlabelled, &labelled] {
            assert_eq!(
                system.naming_with(Some("Zorin OS")),
                DriveName::System("Zorin OS")
            );
        }
    }

    #[test]
    fn a_system_with_no_name_gets_the_fixed_wording_and_not_its_label() {
        assert_eq!(
            drive(DriveKind::System, Some("rootfs")).naming_with(None),
            DriveName::UnnamedSystem
        );
        assert_eq!(
            drive(DriveKind::System, None).naming_with(None),
            DriveName::UnnamedSystem
        );
    }

    #[test]
    fn another_linux_partition_keeps_its_label_or_its_size() {
        assert_eq!(
            drive(DriveKind::Internal, Some("Fedora")).naming_with(Some("Zorin OS")),
            DriveName::Label("Fedora")
        );
        let mut unlabelled = drive(DriveKind::Internal, None);
        assert_eq!(
            unlabelled.naming_with(Some("Zorin OS")),
            DriveName::Size(263_000_000_000)
        );
        // A mounted one is named by the size its filesystem reports.
        unlabelled.capacity = Some(Capacity {
            total: 250_000_000_000,
            free: 1,
        });
        assert_eq!(
            unlabelled.naming_with(Some("Zorin OS")),
            DriveName::Size(250_000_000_000)
        );
    }
}
