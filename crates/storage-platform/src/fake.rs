//! Fakes for every seam in this crate.
//!
//! Behind the `test-support` feature, so nothing in a shipped binary can build
//! one — the same rule `manager-daemon` follows for its privileged fakes. With
//! these, the whole coordination path runs with no D-Bus, no `/proc`, and no
//! disk, which is the only way the failure cases get exercised on every run.

use crate::model::{DeviceAddress, PlatformDevice};
use crate::traits::{
    DeviceControl, EjectOutcome, FlushBackend, FlushReport, FormatRequest, ImageProgress,
    OpenUseInspector, PlatformError, WritebackInspector,
};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use storage_core::{FlushScope, OpenWriters, PendingWriteback, ScanCoverage, SignalStatus};

/// An in-memory set of devices that can be mounted, unmounted, and ejected.
#[derive(Clone, Debug, Default)]
pub struct FakeDeviceControl {
    devices: Arc<Mutex<BTreeMap<String, PlatformDevice>>>,
    calls: Arc<Mutex<Vec<String>>>,
    /// Object paths whose drive cannot be powered off.
    no_power_off: Arc<Mutex<Vec<String>>>,
    /// What each fake drive holds after an image was written to it. Nothing
    /// here ever touches a real device.
    written: Arc<Mutex<BTreeMap<String, Vec<u8>>>>,
    /// Object paths whose unmount is refused, as a busy volume's is.
    no_unmount: Arc<Mutex<Vec<String>>>,
    /// While set, an image write waits here after its first chunk, so a
    /// test can try other operations on the drive meanwhile.
    image_gate: Arc<Mutex<Option<std::sync::mpsc::Receiver<()>>>>,
}

impl FakeDeviceControl {
    pub fn new(devices: impl IntoIterator<Item = PlatformDevice>) -> Self {
        let control = Self::default();
        for device in devices {
            control.attach(device);
        }
        control
    }

    pub fn attach(&self, device: PlatformDevice) {
        self.devices
            .lock()
            .expect("device lock")
            .insert(device.address.object_path.clone(), device);
    }

    pub fn detach(&self, address: &DeviceAddress) {
        self.devices
            .lock()
            .expect("device lock")
            .remove(&address.object_path);
    }

    pub fn refuse_power_off(&self, address: &DeviceAddress) {
        self.no_power_off
            .lock()
            .expect("power lock")
            .push(address.object_path.clone());
    }

    /// Makes unmounting this volume fail, as a busy one does.
    pub fn refuse_unmount(&self, address: &DeviceAddress) {
        self.no_unmount
            .lock()
            .expect("unmount lock")
            .push(address.object_path.clone());
    }

    /// Holds the next image write after its first chunk until the returned
    /// sender is used (or dropped).
    pub fn hold_image_write(&self) -> std::sync::mpsc::Sender<()> {
        let (release, gate) = std::sync::mpsc::channel();
        *self.image_gate.lock().expect("gate lock") = Some(gate);
        release
    }

    /// The bytes an image write left on a fake drive.
    pub fn written(&self, address: &DeviceAddress) -> Option<Vec<u8>> {
        self.written
            .lock()
            .expect("written lock")
            .get(&address.object_path)
            .cloned()
    }

    /// The operations that reached this control, in order.
    pub fn calls(&self) -> Vec<String> {
        self.calls.lock().expect("call lock").clone()
    }

    fn record(&self, call: impl Into<String>) {
        self.calls.lock().expect("call lock").push(call.into());
    }

    fn lookup(&self, address: &DeviceAddress) -> Result<PlatformDevice, PlatformError> {
        self.devices
            .lock()
            .expect("device lock")
            .get(&address.object_path)
            .cloned()
            .ok_or_else(|| PlatformError::UnknownDevice(address.object_path.clone()))
    }
}

impl DeviceControl for FakeDeviceControl {
    async fn enumerate(&self) -> Result<Vec<PlatformDevice>, PlatformError> {
        self.record("enumerate");
        Ok(self
            .devices
            .lock()
            .expect("device lock")
            .values()
            .cloned()
            .collect())
    }

    async fn read(&self, address: &DeviceAddress) -> Result<PlatformDevice, PlatformError> {
        self.lookup(address)
    }

    async fn mount(&self, address: &DeviceAddress) -> Result<PathBuf, PlatformError> {
        self.record(format!("mount {}", address.object_path));
        let mut devices = self.devices.lock().expect("device lock");
        let device = devices
            .get_mut(&address.object_path)
            .ok_or_else(|| PlatformError::UnknownDevice(address.object_path.clone()))?;
        let mount_point = device.mount_point.clone().unwrap_or_else(|| {
            PathBuf::from("/run/media/user")
                .join(device.block.device_path.trim_start_matches("/dev/"))
        });
        device.mount_point = Some(mount_point.clone());
        Ok(mount_point)
    }

    async fn unmount(&self, address: &DeviceAddress) -> Result<(), PlatformError> {
        self.record(format!("unmount {}", address.object_path));
        if self
            .no_unmount
            .lock()
            .expect("unmount lock")
            .contains(&address.object_path)
        {
            return Err(PlatformError::Refused {
                operation: "unmount".to_string(),
                device: address.object_path.clone(),
                detail: "target is busy".to_string(),
            });
        }
        let mut devices = self.devices.lock().expect("device lock");
        let device = devices
            .get_mut(&address.object_path)
            .ok_or_else(|| PlatformError::UnknownDevice(address.object_path.clone()))?;
        device.mount_point = None;
        Ok(())
    }

    async fn eject(&self, address: &DeviceAddress) -> Result<EjectOutcome, PlatformError> {
        self.record(format!("eject {}", address.object_path));
        let refuses_power_off = self
            .no_power_off
            .lock()
            .expect("power lock")
            .contains(&address.object_path);
        let mut devices = self.devices.lock().expect("device lock");
        let device = devices
            .get_mut(&address.object_path)
            .ok_or_else(|| PlatformError::UnknownDevice(address.object_path.clone()))?;
        device.mount_point = None;
        Ok(EjectOutcome {
            unmounted: true,
            powered_off: !refuses_power_off,
            detail: if refuses_power_off {
                "the drive does not support power-off".to_string()
            } else {
                "unmounted and powered off".to_string()
            },
        })
    }

    async fn format(
        &self,
        address: &DeviceAddress,
        request: &FormatRequest,
    ) -> Result<(), PlatformError> {
        request
            .validate()
            .map_err(|detail| PlatformError::Refused {
                operation: "format".to_string(),
                device: address.object_path.clone(),
                detail,
            })?;
        self.record(format!(
            "format {} {} {}",
            address.object_path,
            request.kind.udisks_type(),
            request.label.trim()
        ));
        let mut devices = self.devices.lock().expect("device lock");
        let device = devices
            .get_mut(&address.object_path)
            .ok_or_else(|| PlatformError::UnknownDevice(address.object_path.clone()))?;
        device.mount_point = None;
        device.block.id_type = Some(request.kind.udisks_type().to_string());
        device.block.id_label = Some(request.label.trim().to_string());
        Ok(())
    }

    async fn write_image(
        &self,
        address: &DeviceAddress,
        image: &Path,
        progress: ImageProgress,
    ) -> Result<u64, PlatformError> {
        let bytes = std::fs::read(image).map_err(|error| PlatformError::Refused {
            operation: "write image".to_string(),
            device: address.object_path.clone(),
            detail: error.to_string(),
        })?;
        {
            let mut devices = self.devices.lock().expect("device lock");
            let device = devices
                .get_mut(&address.object_path)
                .ok_or_else(|| PlatformError::UnknownDevice(address.object_path.clone()))?;
            if device.block.partition_number.is_some() {
                return Err(PlatformError::Refused {
                    operation: "write image".to_string(),
                    device: address.object_path.clone(),
                    detail: "it is a partition, not a whole drive".to_string(),
                });
            }
            if device.block.size > 0 && bytes.len() as u64 > device.block.size {
                return Err(PlatformError::Refused {
                    operation: "write image".to_string(),
                    device: address.object_path.clone(),
                    detail: "the image is larger than the drive".to_string(),
                });
            }
            device.mount_point = None;
        }
        self.record(format!(
            "write image {} {}",
            address.object_path,
            image.display()
        ));
        let length = bytes.len() as u64;
        // Two chunks, so progress is heard before the end; a held write
        // waits between them.
        progress(length / 2);
        let gate = self.image_gate.lock().expect("gate lock").take();
        if let Some(gate) = gate {
            let (done, released) = tokio::sync::oneshot::channel();
            std::thread::spawn(move || {
                let _ = gate.recv();
                let _ = done.send(());
            });
            let _ = released.await;
        }
        progress(length);
        self.written
            .lock()
            .expect("written lock")
            .insert(address.object_path.clone(), bytes);
        Ok(length)
    }
}

/// A flush backend whose answer is set by the test.
#[derive(Clone, Debug)]
pub struct FakeFlush {
    report: Arc<Mutex<FlushReport>>,
    flushed: Arc<Mutex<Vec<PathBuf>>>,
}

impl Default for FakeFlush {
    fn default() -> Self {
        Self {
            report: Arc::new(Mutex::new(FlushReport::Completed {
                scope: FlushScope::Filesystem,
            })),
            flushed: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

impl FakeFlush {
    pub fn failing(detail: &str) -> Self {
        let fake = Self::default();
        fake.set(FlushReport::Failed {
            detail: detail.to_string(),
        });
        fake
    }

    pub fn unsupported(detail: &str) -> Self {
        let fake = Self::default();
        fake.set(FlushReport::Unsupported {
            detail: detail.to_string(),
        });
        fake
    }

    pub fn set(&self, report: FlushReport) {
        *self.report.lock().expect("flush lock") = report;
    }

    /// Every path that was flushed, in order. A test asserts on this to prove
    /// the flush was filesystem-scoped and not machine-wide.
    pub fn flushed(&self) -> Vec<PathBuf> {
        self.flushed.lock().expect("flushed lock").clone()
    }
}

impl FlushBackend for FakeFlush {
    fn flush_filesystem(&self, mount_point: &Path) -> FlushReport {
        self.flushed
            .lock()
            .expect("flushed lock")
            .push(mount_point.to_path_buf());
        self.report.lock().expect("flush lock").clone()
    }

    fn flush_device(&self, device_path: &Path) -> FlushReport {
        self.flushed
            .lock()
            .expect("flushed lock")
            .push(device_path.to_path_buf());
        FlushReport::Unsupported {
            detail: format!("no device flush is available for {}", device_path.display()),
        }
    }
}

/// Writeback answers set by the test.
#[derive(Clone, Debug)]
pub struct FakeWriteback {
    status: Arc<Mutex<SignalStatus<PendingWriteback>>>,
}

impl Default for FakeWriteback {
    fn default() -> Self {
        Self::idle()
    }
}

impl FakeWriteback {
    pub fn idle() -> Self {
        Self {
            status: Arc::new(Mutex::new(SignalStatus::Observed(PendingWriteback {
                bytes: 0,
                scope: storage_core::WritebackScope::Device,
            }))),
        }
    }

    pub fn set(&self, status: SignalStatus<PendingWriteback>) {
        *self.status.lock().expect("writeback lock") = status;
    }

    pub fn pending_bytes(&self, bytes: u64) {
        self.set(SignalStatus::Observed(PendingWriteback {
            bytes,
            scope: storage_core::WritebackScope::Device,
        }));
    }
}

impl WritebackInspector for FakeWriteback {
    fn pending(&self, _device: &PlatformDevice) -> SignalStatus<PendingWriteback> {
        self.status.lock().expect("writeback lock").clone()
    }
}

/// Open-writer answers set by the test.
#[derive(Clone, Debug)]
pub struct FakeOpenUse {
    status: Arc<Mutex<SignalStatus<OpenWriters>>>,
}

impl Default for FakeOpenUse {
    fn default() -> Self {
        Self::idle()
    }
}

impl FakeOpenUse {
    pub fn idle() -> Self {
        Self {
            status: Arc::new(Mutex::new(SignalStatus::Observed(OpenWriters {
                writers: Vec::new(),
                coverage: ScanCoverage::Complete,
            }))),
        }
    }

    pub fn set(&self, status: SignalStatus<OpenWriters>) {
        *self.status.lock().expect("open-use lock") = status;
    }
}

impl OpenUseInspector for FakeOpenUse {
    fn open_writers(&self, _mount_point: &Path) -> SignalStatus<OpenWriters> {
        self.status.lock().expect("open-use lock").clone()
    }
}

/// A USB stick, the shape of device most of these tests want.
pub fn usb_stick(object_path: &str, device_path: &str, uuid: &str) -> PlatformDevice {
    use crate::model::{BlockInfo, DriveInfo};
    PlatformDevice {
        address: DeviceAddress {
            object_path: object_path.to_string(),
            device_path: device_path.to_string(),
        },
        block: BlockInfo {
            device_path: device_path.to_string(),
            id_uuid: Some(uuid.to_string()),
            id_label: Some("FIELD DATA".to_string()),
            id_type: Some("exfat".to_string()),
            partition_number: Some(1),
            size: 32 * 1024 * 1024 * 1024,
            symlinks: vec![format!("/dev/disk/by-uuid/{uuid}")],
            ..BlockInfo::default()
        },
        drive: Some(DriveInfo {
            object_path: format!("/org/freedesktop/UDisks2/drives/SN_{uuid}"),
            removable: true,
            connection_bus: "usb".to_string(),
            serial: Some(format!("SN-{uuid}")),
            vendor: Some("Generic".to_string()),
            model: Some("Flash Disk".to_string()),
            ejectable: true,
            can_power_off: true,
            ..DriveInfo::default()
        }),
        mount_point: None,
    }
}

/// A USB drive's own block device (the whole disk, no filesystem of its
/// own), on the drive `drive_object`.
pub fn usb_disk(object_path: &str, device_path: &str, drive_object: &str) -> PlatformDevice {
    let mut disk = usb_stick(object_path, device_path, "disk");
    disk.block.id_uuid = None;
    disk.block.id_label = None;
    disk.block.id_type = None;
    disk.block.partition_number = None;
    disk.block.symlinks = Vec::new();
    if let Some(drive) = disk.drive.as_mut() {
        drive.object_path = drive_object.to_string();
        drive.serial = Some(drive_object.rsplit('/').next().unwrap_or("").to_string());
    }
    disk
}

/// A partition with a filesystem on the drive `drive_object`.
pub fn usb_partition(
    object_path: &str,
    device_path: &str,
    uuid: &str,
    drive_object: &str,
) -> PlatformDevice {
    let mut partition = usb_stick(object_path, device_path, uuid);
    if let Some(drive) = partition.drive.as_mut() {
        drive.object_path = drive_object.to_string();
        drive.serial = Some(drive_object.rsplit('/').next().unwrap_or("").to_string());
    }
    partition
}

/// An internal system disk, for asserting that it is excluded.
pub fn internal_disk(object_path: &str, device_path: &str) -> PlatformDevice {
    use crate::model::{BlockInfo, DriveInfo};
    PlatformDevice {
        address: DeviceAddress {
            object_path: object_path.to_string(),
            device_path: device_path.to_string(),
        },
        block: BlockInfo {
            device_path: device_path.to_string(),
            id_uuid: Some("11111111-2222-3333-4444-555555555555".to_string()),
            id_type: Some("ext4".to_string()),
            hint_system: true,
            ..BlockInfo::default()
        },
        drive: Some(DriveInfo {
            connection_bus: String::new(),
            serial: Some("NVME-0001".to_string()),
            model: Some("Internal SSD".to_string()),
            ..DriveInfo::default()
        }),
        mount_point: Some(PathBuf::from("/")),
    }
}
