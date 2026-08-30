//! Fakes for every seam in this crate.
//!
//! Behind the `test-support` feature, so nothing in a shipped binary can build
//! one — the same rule `manager-daemon` follows for its privileged fakes. With
//! these, the whole coordination path runs with no D-Bus, no `/proc`, and no
//! disk, which is the only way the failure cases get exercised on every run.

use crate::model::{DeviceAddress, PlatformDevice};
use crate::traits::{
    DeviceControl, EjectOutcome, FlushBackend, FlushReport, OpenUseInspector, PlatformError,
    WritebackInspector,
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
