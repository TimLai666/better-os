//! UDisks2 over zbus.
//!
//! Everything here talks to `org.freedesktop.UDisks2` on the system bus. The
//! service itself stays unprivileged: UDisks2 is the thing that holds the
//! privilege, and it applies its own polkit rules to mount, unmount, and
//! power-off. Nothing in this file needs the storage service to run as root,
//! which is the whole point of routing through UDisks2 instead of calling
//! `mount(2)`.
//!
//! Observation is event-driven. `InterfacesAdded` and `InterfacesRemoved` from
//! the object manager cover plug and unplug, and `PropertiesChanged` covers
//! mount, unmount, and media changes. There is no polling loop: an idle session
//! with a stick plugged in costs one open D-Bus connection and nothing else.

use crate::model::{BlockInfo, DeviceAddress, DriveInfo, PlatformDevice, PlatformEvent};
use crate::traits::{DeviceControl, EjectOutcome, PlatformError};
use futures_util::StreamExt;
use std::collections::HashMap;
use std::path::PathBuf;
use tokio::sync::mpsc::UnboundedSender;
use zbus::zvariant::{OwnedObjectPath, Value};

pub const UDISKS2_SERVICE: &str = "org.freedesktop.UDisks2";
pub const UDISKS2_OBJECT_PATH: &str = "/org/freedesktop/UDisks2";
pub const BLOCK_INTERFACE: &str = "org.freedesktop.UDisks2.Block";
pub const FILESYSTEM_INTERFACE: &str = "org.freedesktop.UDisks2.Filesystem";

#[zbus::proxy(
    interface = "org.freedesktop.UDisks2.Block",
    default_service = "org.freedesktop.UDisks2"
)]
pub trait Block {
    #[zbus(property)]
    fn device(&self) -> zbus::Result<Vec<u8>>;
    #[zbus(property)]
    fn preferred_device(&self) -> zbus::Result<Vec<u8>>;
    #[zbus(property)]
    fn symlinks(&self) -> zbus::Result<Vec<Vec<u8>>>;
    #[zbus(property)]
    fn drive(&self) -> zbus::Result<OwnedObjectPath>;
    #[zbus(property, name = "IdUUID")]
    fn id_uuid(&self) -> zbus::Result<String>;
    #[zbus(property, name = "IdLabel")]
    fn id_label(&self) -> zbus::Result<String>;
    #[zbus(property, name = "IdType")]
    fn id_type(&self) -> zbus::Result<String>;
    #[zbus(property)]
    fn hint_system(&self) -> zbus::Result<bool>;
    #[zbus(property)]
    fn read_only(&self) -> zbus::Result<bool>;
    #[zbus(property)]
    fn size(&self) -> zbus::Result<u64>;
}

#[zbus::proxy(
    interface = "org.freedesktop.UDisks2.Partition",
    default_service = "org.freedesktop.UDisks2"
)]
pub trait Partition {
    #[zbus(property, name = "UUID")]
    fn uuid(&self) -> zbus::Result<String>;
    #[zbus(property)]
    fn number(&self) -> zbus::Result<u32>;
}

#[zbus::proxy(
    interface = "org.freedesktop.UDisks2.Filesystem",
    default_service = "org.freedesktop.UDisks2"
)]
pub trait Filesystem {
    #[zbus(property)]
    fn mount_points(&self) -> zbus::Result<Vec<Vec<u8>>>;
    fn mount(&self, options: HashMap<&str, Value<'_>>) -> zbus::Result<String>;
    fn unmount(&self, options: HashMap<&str, Value<'_>>) -> zbus::Result<()>;
}

#[zbus::proxy(
    interface = "org.freedesktop.UDisks2.Drive",
    default_service = "org.freedesktop.UDisks2"
)]
pub trait Drive {
    #[zbus(property)]
    fn removable(&self) -> zbus::Result<bool>;
    #[zbus(property)]
    fn media_removable(&self) -> zbus::Result<bool>;
    #[zbus(property)]
    fn connection_bus(&self) -> zbus::Result<String>;
    #[zbus(property)]
    fn serial(&self) -> zbus::Result<String>;
    #[zbus(property)]
    fn vendor(&self) -> zbus::Result<String>;
    #[zbus(property)]
    fn model(&self) -> zbus::Result<String>;
    #[zbus(property, name = "WWN")]
    fn wwn(&self) -> zbus::Result<String>;
    #[zbus(property)]
    fn ejectable(&self) -> zbus::Result<bool>;
    #[zbus(property)]
    fn can_power_off(&self) -> zbus::Result<bool>;
    fn power_off(&self, options: HashMap<&str, Value<'_>>) -> zbus::Result<()>;
}

/// UDisks2 reports paths as NUL-terminated byte arrays.
fn decode_path(raw: &[u8]) -> String {
    String::from_utf8_lossy(raw)
        .trim_end_matches('\0')
        .to_string()
}

/// An empty string from UDisks2 means "not reported", not "the empty value".
fn optional(value: String) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn unreachable(detail: impl std::fmt::Display) -> PlatformError {
    PlatformError::Unreachable {
        service: UDISKS2_SERVICE.to_string(),
        detail: detail.to_string(),
    }
}

/// A live connection to UDisks2.
#[derive(Clone, Debug)]
pub struct UDisks2 {
    connection: zbus::Connection,
}

impl UDisks2 {
    /// Connects to the system bus. Fails honestly when UDisks2 is not there:
    /// the caller reports the devices it cannot see rather than reporting none.
    pub async fn connect() -> Result<Self, PlatformError> {
        let connection = zbus::Connection::system().await.map_err(unreachable)?;
        Ok(Self { connection })
    }

    pub fn from_connection(connection: zbus::Connection) -> Self {
        Self { connection }
    }

    async fn object_manager(&self) -> Result<zbus::fdo::ObjectManagerProxy<'_>, PlatformError> {
        zbus::fdo::ObjectManagerProxy::builder(&self.connection)
            .destination(UDISKS2_SERVICE)
            .map_err(unreachable)?
            .path(UDISKS2_OBJECT_PATH)
            .map_err(unreachable)?
            .build()
            .await
            .map_err(unreachable)
    }

    async fn read_object(&self, object_path: &str) -> Result<PlatformDevice, PlatformError> {
        let block = BlockProxy::builder(&self.connection)
            .path(object_path.to_string())
            .map_err(unreachable)?
            .build()
            .await
            .map_err(unreachable)?;

        let device_path = decode_path(&block.device().await.map_err(unreachable)?);

        let partition = PartitionProxy::builder(&self.connection)
            .path(object_path.to_string())
            .map_err(unreachable)?
            .build()
            .await
            .ok();
        let (partition_uuid, partition_number) = match &partition {
            Some(partition) => (
                partition.uuid().await.ok().and_then(optional),
                partition.number().await.ok(),
            ),
            None => (None, None),
        };

        let block_info = BlockInfo {
            device_path: device_path.clone(),
            id_uuid: block.id_uuid().await.ok().and_then(optional),
            id_label: block.id_label().await.ok().and_then(optional),
            id_type: block.id_type().await.ok().and_then(optional),
            hint_system: block.hint_system().await.unwrap_or(false),
            read_only: block.read_only().await.unwrap_or(false),
            size: block.size().await.unwrap_or(0),
            partition_uuid,
            partition_number,
            symlinks: block
                .symlinks()
                .await
                .unwrap_or_default()
                .iter()
                .map(|raw| decode_path(raw))
                .collect(),
        };

        let drive_path = block.drive().await.ok();
        let drive = match drive_path {
            Some(path) if path.as_str() != "/" => {
                match DriveProxy::builder(&self.connection)
                    .path(path.as_str().to_string())
                    .map_err(unreachable)?
                    .build()
                    .await
                {
                    Ok(drive) => Some(DriveInfo {
                        removable: drive.removable().await.unwrap_or(false),
                        media_removable: drive.media_removable().await.unwrap_or(false),
                        connection_bus: drive.connection_bus().await.unwrap_or_default(),
                        serial: drive.serial().await.ok().and_then(optional),
                        vendor: drive.vendor().await.ok().and_then(optional),
                        model: drive.model().await.ok().and_then(optional),
                        wwn: drive.wwn().await.ok().and_then(optional),
                        ejectable: drive.ejectable().await.unwrap_or(false),
                        can_power_off: drive.can_power_off().await.unwrap_or(false),
                    }),
                    Err(_) => None,
                }
            }
            _ => None,
        };

        let mount_point = self.mount_point(object_path).await;

        Ok(PlatformDevice {
            address: DeviceAddress {
                object_path: object_path.to_string(),
                device_path,
            },
            block: block_info,
            drive,
            mount_point,
        })
    }

    async fn mount_point(&self, object_path: &str) -> Option<PathBuf> {
        let filesystem = FilesystemProxy::builder(&self.connection)
            .path(object_path.to_string())
            .ok()?
            .build()
            .await
            .ok()?;
        let mounts = filesystem.mount_points().await.ok()?;
        mounts.first().map(|raw| PathBuf::from(decode_path(raw)))
    }

    async fn filesystem(&self, object_path: &str) -> Result<FilesystemProxy<'_>, PlatformError> {
        FilesystemProxy::builder(&self.connection)
            .path(object_path.to_string())
            .map_err(unreachable)?
            .build()
            .await
            .map_err(|error| PlatformError::Unsupported {
                operation: "filesystem access".to_string(),
                device: object_path.to_string(),
                detail: error.to_string(),
            })
    }

    /// Starts watching for device and property changes, sending each into
    /// `sender`. Returns once the streams are established; the watching itself
    /// runs on spawned tasks that end when the sender is dropped.
    pub async fn watch(&self, sender: UnboundedSender<PlatformEvent>) -> Result<(), PlatformError> {
        let manager = self.object_manager().await?;
        let mut added = manager
            .receive_interfaces_added()
            .await
            .map_err(unreachable)?;
        let mut removed = manager
            .receive_interfaces_removed()
            .await
            .map_err(unreachable)?;

        let properties = zbus::MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .sender(UDISKS2_SERVICE)
            .map_err(unreachable)?
            .interface("org.freedesktop.DBus.Properties")
            .map_err(unreachable)?
            .member("PropertiesChanged")
            .map_err(unreachable)?
            .build();
        let mut changed = zbus::MessageStream::for_match_rule(properties, &self.connection, None)
            .await
            .map_err(unreachable)?;

        let client = self.clone();
        let added_sender = sender.clone();
        tokio::spawn(async move {
            while let Some(signal) = added.next().await {
                let Ok(args) = signal.args() else { continue };
                if !args.interfaces_and_properties.contains_key(BLOCK_INTERFACE) {
                    continue;
                }
                let path = args.object_path.as_str().to_string();
                match client.read_object(&path).await {
                    Ok(device) => {
                        if added_sender
                            .send(PlatformEvent::Added(Box::new(device)))
                            .is_err()
                        {
                            return;
                        }
                    }
                    // The device went away between the signal and the read.
                    // Nothing to report: a Removed signal is on its way.
                    Err(_) => continue,
                }
            }
        });

        let removed_sender = sender.clone();
        tokio::spawn(async move {
            while let Some(signal) = removed.next().await {
                let Ok(args) = signal.args() else { continue };
                let path = args.object_path.as_str().to_string();
                if removed_sender
                    .send(PlatformEvent::Removed {
                        address: DeviceAddress {
                            object_path: path,
                            device_path: String::new(),
                        },
                    })
                    .is_err()
                {
                    return;
                }
            }
        });

        tokio::spawn(async move {
            while let Some(message) = changed.next().await {
                let Ok(message) = message else { continue };
                let Some(path) = message
                    .header()
                    .path()
                    .map(|path| path.as_str().to_string())
                else {
                    continue;
                };
                if !path.starts_with("/org/freedesktop/UDisks2/block_devices/") {
                    continue;
                }
                if sender
                    .send(PlatformEvent::Changed {
                        address: DeviceAddress {
                            object_path: path,
                            device_path: String::new(),
                        },
                    })
                    .is_err()
                {
                    return;
                }
            }
        });

        Ok(())
    }
}

impl DeviceControl for UDisks2 {
    async fn enumerate(&self) -> Result<Vec<PlatformDevice>, PlatformError> {
        let manager = self.object_manager().await?;
        let objects = manager.get_managed_objects().await.map_err(unreachable)?;

        let mut devices = Vec::new();
        for (path, interfaces) in objects {
            if !interfaces.contains_key(BLOCK_INTERFACE) {
                continue;
            }
            if let Ok(device) = self.read_object(path.as_str()).await {
                devices.push(device);
            }
        }
        devices.sort_by(|left, right| left.address.cmp(&right.address));
        Ok(devices)
    }

    async fn read(&self, address: &DeviceAddress) -> Result<PlatformDevice, PlatformError> {
        self.read_object(&address.object_path).await
    }

    async fn mount(&self, address: &DeviceAddress) -> Result<PathBuf, PlatformError> {
        let filesystem = self.filesystem(&address.object_path).await?;
        // No options: the mount options per filesystem are a deferred decision
        // in issue #5, and inventing one here would be exactly the undocumented
        // assumption that decision exists to prevent.
        match filesystem.mount(HashMap::new()).await {
            Ok(mount_point) => Ok(PathBuf::from(mount_point)),
            Err(error) => Err(PlatformError::Refused {
                operation: "mount".to_string(),
                device: address.device_path.clone(),
                detail: error.to_string(),
            }),
        }
    }

    async fn unmount(&self, address: &DeviceAddress) -> Result<(), PlatformError> {
        let filesystem = self.filesystem(&address.object_path).await?;
        filesystem
            .unmount(HashMap::new())
            .await
            .map_err(|error| PlatformError::Refused {
                operation: "unmount".to_string(),
                device: address.device_path.clone(),
                detail: error.to_string(),
            })
    }

    async fn eject(&self, address: &DeviceAddress) -> Result<EjectOutcome, PlatformError> {
        let unmounted = match self.filesystem(&address.object_path).await {
            Ok(filesystem) => filesystem.unmount(HashMap::new()).await.is_ok(),
            // No filesystem interface means nothing to unmount, which is not a
            // failure of the eject.
            Err(_) => true,
        };

        let block = BlockProxy::builder(&self.connection)
            .path(address.object_path.clone())
            .map_err(unreachable)?
            .build()
            .await
            .map_err(unreachable)?;
        let drive_path = block.drive().await.ok();

        let mut powered_off = false;
        let mut detail = "the drive does not support power-off".to_string();
        if let Some(path) = drive_path
            && path.as_str() != "/"
            && let Ok(drive) = DriveProxy::builder(&self.connection)
                .path(path.as_str().to_string())
                .map_err(unreachable)?
                .build()
                .await
            && drive.can_power_off().await.unwrap_or(false)
        {
            match drive.power_off(HashMap::new()).await {
                Ok(()) => {
                    powered_off = true;
                    detail = "unmounted and powered off".to_string();
                }
                Err(error) => detail = format!("power-off was refused: {error}"),
            }
        }

        Ok(EjectOutcome {
            unmounted,
            powered_off,
            detail,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_nul_terminated_device_path_decodes_without_its_terminator() {
        assert_eq!(decode_path(b"/dev/sdb1\0"), "/dev/sdb1");
        assert_eq!(decode_path(b"/dev/sdb1"), "/dev/sdb1");
    }

    #[test]
    fn an_empty_property_is_absent_rather_than_an_empty_identifier() {
        assert_eq!(optional(String::new()), None);
        assert_eq!(optional("   ".to_string()), None);
        assert_eq!(optional(" 0123 ".to_string()), Some("0123".to_string()));
    }
}
