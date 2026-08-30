//! What the platform reports about a block device, and how that becomes an
//! identity and a classification.
//!
//! Everything here is plain data. The UDisks2 code fills it in, the fakes fill
//! it in, and the classification rules are the same either way — which is what
//! makes "is this an external hot-pluggable volume" testable without hardware.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use storage_core::{IdentityEvidence, Transport};

/// How to reach one volume again. The object path is stable for the lifetime of
/// the connection; the device path is not, and is carried for diagnostics and
/// for the ioctls that need a node to open.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct DeviceAddress {
    pub object_path: String,
    pub device_path: String,
}

/// `org.freedesktop.UDisks2.Drive` properties this crate reads.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct DriveInfo {
    pub removable: bool,
    pub media_removable: bool,
    /// The raw `ConnectionBus` string, kept raw so an unrecognized bus stays
    /// distinguishable from a missing one.
    pub connection_bus: String,
    pub serial: Option<String>,
    pub vendor: Option<String>,
    pub model: Option<String>,
    pub wwn: Option<String>,
    pub ejectable: bool,
    pub can_power_off: bool,
}

/// `org.freedesktop.UDisks2.Block` and `.Partition` properties this crate reads.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct BlockInfo {
    pub device_path: String,
    pub id_uuid: Option<String>,
    pub id_label: Option<String>,
    /// The filesystem type UDisks2 detected, such as `vfat`, `exfat`, `ntfs`,
    /// or `ext4`. Empty for an unformatted or unrecognized volume.
    pub id_type: Option<String>,
    /// UDisks2's own hint that this belongs to the running system.
    pub hint_system: bool,
    pub read_only: bool,
    pub size: u64,
    pub partition_uuid: Option<String>,
    pub partition_number: Option<u32>,
    /// `/dev/disk/by-*` symlinks. The `by-path` entry is the closest thing to
    /// stable topology UDisks2 offers.
    pub symlinks: Vec<String>,
}

impl BlockInfo {
    /// The bus path this volume is attached through, taken from the `by-path`
    /// symlink UDisks2 reports.
    pub fn topology(&self) -> Option<String> {
        self.symlinks
            .iter()
            .find(|link| link.contains("/by-path/"))
            .map(|link| link.rsplit('/').next().unwrap_or(link.as_str()).to_string())
    }
}

/// Why a device is or is not in scope for direct-removal policy.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceClass {
    /// A hot-pluggable external volume: this component's whole subject.
    ExternalHotPluggable,
    /// Part of the running system, or attached through a bus that is not
    /// hot-pluggable in the sense this component means.
    Internal { reason: String },
    /// In an external enclosure, but nothing this component can promise
    /// anything about — no filesystem it recognizes, or a device whose
    /// removability the platform never stated.
    Unsupported { reason: String },
}

impl DeviceClass {
    pub fn is_external(&self) -> bool {
        matches!(self, DeviceClass::ExternalHotPluggable)
    }
}

/// One volume as the platform sees it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PlatformDevice {
    pub address: DeviceAddress,
    pub block: BlockInfo,
    /// `None` when the block device has no drive object, which is what a loop
    /// or device-mapper node looks like.
    pub drive: Option<DriveInfo>,
    pub mount_point: Option<PathBuf>,
}

impl PlatformDevice {
    /// Whether this volume is in scope, and why.
    ///
    /// The rule is positive: a device is external only when a drive object says
    /// it is removable and names a hot-pluggable bus. Nothing is assumed from
    /// the absence of a system hint, because that absence is also what an
    /// unusual internal controller looks like.
    pub fn classify(&self) -> DeviceClass {
        let Some(drive) = &self.drive else {
            return DeviceClass::Unsupported {
                reason: "the block device has no drive object, so its removability is unknown"
                    .to_string(),
            };
        };
        if self.block.hint_system {
            return DeviceClass::Internal {
                reason: "the platform marks this device as part of the running system".to_string(),
            };
        }
        let transport = Transport::from_connection_bus(&drive.connection_bus);
        let hot_pluggable_bus = matches!(
            transport,
            Transport::Usb
                | Transport::Sdio
                | Transport::Mmc
                | Transport::Thunderbolt
                | Transport::Ieee1394
        );
        if !hot_pluggable_bus {
            return DeviceClass::Internal {
                reason: format!(
                    "connection bus {:?} is not a hot-pluggable external bus",
                    drive.connection_bus
                ),
            };
        }
        if !(drive.removable || drive.media_removable) {
            return DeviceClass::Internal {
                reason: "the drive reports neither removable nor removable media".to_string(),
            };
        }
        DeviceClass::ExternalHotPluggable
    }

    pub fn transport(&self) -> Transport {
        self.drive
            .as_ref()
            .map(|drive| Transport::from_connection_bus(&drive.connection_bus))
            .unwrap_or(Transport::Unknown)
    }

    pub fn identity_evidence(&self) -> IdentityEvidence {
        let drive = self.drive.clone().unwrap_or_default();
        IdentityEvidence {
            filesystem_uuid: self.block.id_uuid.clone(),
            partition_uuid: self.block.partition_uuid.clone(),
            drive_serial: drive.serial,
            wwn: drive.wwn,
            vendor: drive.vendor,
            model: drive.model,
            transport: self.transport(),
            topology: self.block.topology(),
            partition_number: self.block.partition_number,
            device_path: self.block.device_path.clone(),
            label: self.block.id_label.clone(),
        }
    }
}

/// Something the platform saw happen. Delivered through a channel rather than
/// polled, which is what keeps idle cost at zero.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PlatformEvent {
    Added(Box<PlatformDevice>),
    Removed {
        address: DeviceAddress,
    },
    /// A mount appeared or disappeared for a device already known.
    MountChanged {
        address: DeviceAddress,
        mount_point: Option<PathBuf>,
    },
    /// Some other property changed and the device is worth re-reading. Carries
    /// no payload on purpose: the reader re-reads rather than trusting a diff.
    Changed {
        address: DeviceAddress,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usb_stick() -> PlatformDevice {
        PlatformDevice {
            address: DeviceAddress {
                object_path: "/org/freedesktop/UDisks2/block_devices/sdb1".to_string(),
                device_path: "/dev/sdb1".to_string(),
            },
            block: BlockInfo {
                device_path: "/dev/sdb1".to_string(),
                id_uuid: Some("A1B2-C3D4".to_string()),
                id_label: Some("FIELD DATA".to_string()),
                id_type: Some("exfat".to_string()),
                partition_number: Some(1),
                symlinks: vec![
                    "/dev/disk/by-uuid/A1B2-C3D4".to_string(),
                    "/dev/disk/by-path/pci-0000:00:14.0-usb-0:2:1.0-scsi-0:0:0:0-part1".to_string(),
                ],
                ..BlockInfo::default()
            },
            drive: Some(DriveInfo {
                removable: true,
                media_removable: false,
                connection_bus: "usb".to_string(),
                serial: Some("0123456789".to_string()),
                vendor: Some("Generic".to_string()),
                model: Some("Flash Disk".to_string()),
                ejectable: true,
                can_power_off: true,
                ..DriveInfo::default()
            }),
            mount_point: Some(PathBuf::from("/run/media/user/FIELD DATA")),
        }
    }

    #[test]
    fn a_removable_usb_volume_is_the_case_this_component_is_for() {
        assert_eq!(usb_stick().classify(), DeviceClass::ExternalHotPluggable);
    }

    #[test]
    fn the_system_disk_is_never_treated_as_external_however_it_is_attached() {
        let mut device = usb_stick();
        device.block.hint_system = true;
        assert!(matches!(device.classify(), DeviceClass::Internal { .. }));
    }

    #[test]
    fn an_internal_sata_disk_is_excluded_by_its_bus() {
        let mut device = usb_stick();
        device.drive.as_mut().unwrap().connection_bus = String::new();
        assert!(matches!(device.classify(), DeviceClass::Internal { .. }));

        let mut device = usb_stick();
        device.drive.as_mut().unwrap().connection_bus = "sata".to_string();
        assert!(matches!(device.classify(), DeviceClass::Internal { .. }));
    }

    #[test]
    fn a_usb_enclosure_that_says_it_is_not_removable_is_not_assumed_to_be() {
        let mut device = usb_stick();
        let drive = device.drive.as_mut().unwrap();
        drive.removable = false;
        drive.media_removable = false;
        assert!(matches!(device.classify(), DeviceClass::Internal { .. }));
    }

    #[test]
    fn an_sd_card_reader_counts_through_its_removable_media() {
        let mut device = usb_stick();
        let drive = device.drive.as_mut().unwrap();
        drive.connection_bus = "sdio".to_string();
        drive.removable = false;
        drive.media_removable = true;
        assert_eq!(device.classify(), DeviceClass::ExternalHotPluggable);
    }

    #[test]
    fn a_block_device_with_no_drive_is_unsupported_rather_than_internal() {
        let mut device = usb_stick();
        device.drive = None;
        assert!(matches!(device.classify(), DeviceClass::Unsupported { .. }));
        // And it is not silently treated as in scope.
        assert!(!device.classify().is_external());
    }

    #[test]
    fn identity_evidence_carries_the_port_path_but_never_only_the_kernel_name() {
        let evidence = usb_stick().identity_evidence();
        assert_eq!(evidence.transport, Transport::Usb);
        assert_eq!(
            evidence.topology.as_deref(),
            Some("pci-0000:00:14.0-usb-0:2:1.0-scsi-0:0:0:0-part1")
        );
        assert_eq!(evidence.filesystem_uuid.as_deref(), Some("A1B2-C3D4"));
        assert_eq!(evidence.device_path, "/dev/sdb1");
    }
}
