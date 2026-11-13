//! The seams.
//!
//! Every host interaction is behind one of these, so the coordinating service
//! can be driven entirely by fakes. The split is deliberate: enumeration,
//! mounting, and eject are asynchronous and live on D-Bus, while flushing and
//! the two inspection signals are ordinary file I/O that runs on a blocking
//! thread.

use crate::model::{DeviceAddress, PlatformDevice};
use std::future::Future;
use std::path::Path;
use storage_core::{
    FlushOutcome, FlushScope, FlushVerification, OpenWriters, PendingWriteback, SignalStatus,
    Timestamp,
};
use thiserror::Error;

/// What a flush attempt did, without a session timestamp: the platform has no
/// session clock, so the caller stamps completion.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FlushReport {
    Completed { scope: FlushScope },
    Failed { detail: String },
    Unsupported { detail: String },
}

impl FlushReport {
    pub fn into_outcome(self, at: Timestamp) -> FlushOutcome {
        match self {
            FlushReport::Completed { scope } => FlushOutcome::Completed(FlushVerification {
                scope,
                completed_at: at,
            }),
            FlushReport::Failed { detail } => FlushOutcome::Failed { detail },
            FlushReport::Unsupported { detail } => FlushOutcome::Unsupported { detail },
        }
    }
}

#[derive(Debug, Error)]
pub enum PlatformError {
    #[error("the storage service could not reach {service}: {detail}")]
    Unreachable { service: String, detail: String },
    #[error("{operation} was refused for {device}: {detail}")]
    Refused {
        operation: String,
        device: String,
        detail: String,
    },
    #[error("no device is known at {0}")]
    UnknownDevice(String),
    #[error("{operation} is not supported for {device}: {detail}")]
    Unsupported {
        operation: String,
        device: String,
        detail: String,
    },
}

/// What `Eject` actually managed to do.
///
/// Powering the drive off is the part that makes the light go out, and plenty
/// of enclosures do not support it. Reporting an unmount that succeeded and a
/// power-off that was unavailable is the honest answer; claiming a clean eject
/// is not.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EjectOutcome {
    pub unmounted: bool,
    pub powered_off: bool,
    pub detail: String,
}

/// The filesystems Format offers, as folio's Format dialog does.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FilesystemKind {
    /// Readable by Windows, macOS and Linux, and any size of file.
    Exfat,
    /// Readable by almost anything, files up to 4 GB.
    Fat32,
    /// Linux only.
    Ext4,
}

impl FilesystemKind {
    pub const ALL: [FilesystemKind; 3] = [
        FilesystemKind::Exfat,
        FilesystemKind::Fat32,
        FilesystemKind::Ext4,
    ];

    /// The type UDisks2's `Format` is asked for.
    pub fn udisks_type(self) -> &'static str {
        match self {
            FilesystemKind::Exfat => "exfat",
            FilesystemKind::Fat32 => "vfat",
            FilesystemKind::Ext4 => "ext4",
        }
    }

    /// The longest volume label each filesystem stores.
    pub fn label_limit(self) -> usize {
        match self {
            FilesystemKind::Exfat => 15,
            FilesystemKind::Fat32 => 11,
            FilesystemKind::Ext4 => 16,
        }
    }
}

/// One Format: the filesystem and the new name.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FormatRequest {
    pub kind: FilesystemKind,
    pub label: String,
}

impl FormatRequest {
    /// Whether the label fits the filesystem: not too long, no control
    /// characters, no `/`, and (for FAT32) only the characters FAT allows.
    pub fn validate(&self) -> Result<(), String> {
        let label = self.label.trim();
        if label.chars().count() > self.kind.label_limit() {
            return Err(format!(
                "the label is longer than {} characters",
                self.kind.label_limit()
            ));
        }
        if label.chars().any(|c| c.is_control() || c == '/') {
            return Err("the label has a character the filesystem cannot store".to_string());
        }
        if self.kind == FilesystemKind::Fat32
            && label
                .chars()
                .any(|c| !c.is_ascii() || "\"*+,.:;<=>?[\\]|".contains(c))
        {
            return Err("FAT32 labels take plain letters, digits and spaces".to_string());
        }
        Ok(())
    }
}

/// Enumerating and controlling devices. Implemented over UDisks2 in production
/// and by a fake in tests.
pub trait DeviceControl: Send + Sync {
    fn enumerate(&self) -> impl Future<Output = Result<Vec<PlatformDevice>, PlatformError>> + Send;

    fn read(
        &self,
        address: &DeviceAddress,
    ) -> impl Future<Output = Result<PlatformDevice, PlatformError>> + Send;

    /// Mounts on open, the way the desktop already behaves. Returns the mount
    /// point.
    fn mount(
        &self,
        address: &DeviceAddress,
    ) -> impl Future<Output = Result<std::path::PathBuf, PlatformError>> + Send;

    fn unmount(
        &self,
        address: &DeviceAddress,
    ) -> impl Future<Output = Result<(), PlatformError>> + Send;

    /// Unmount plus power-off where the drive supports it.
    fn eject(
        &self,
        address: &DeviceAddress,
    ) -> impl Future<Output = Result<EjectOutcome, PlatformError>> + Send;

    /// Erases the volume and makes a new filesystem on it, through UDisks2's
    /// own `Format` (which unmounts first and asks polkit). Validated by the
    /// caller.
    fn format(
        &self,
        address: &DeviceAddress,
        request: &FormatRequest,
    ) -> impl Future<Output = Result<(), PlatformError>> + Send;

    /// Writes a disk image (`.iso`, `.img`) to the whole drive the volume is
    /// on, through UDisks2's `OpenForRestore` (polkit asks). Returns the
    /// bytes written.
    fn write_image(
        &self,
        address: &DeviceAddress,
        image: &std::path::Path,
    ) -> impl Future<Output = Result<u64, PlatformError>> + Send;
}

/// Flushing. Always the narrowest scope that applies: one filesystem, or one
/// device. Nothing in this crate calls a machine-wide `sync`.
pub trait FlushBackend: Send + Sync {
    fn flush_filesystem(&self, mount_point: &Path) -> FlushReport;

    /// A device-level cache flush, where the platform exposes one this process
    /// is allowed to issue. Reports `Unsupported` rather than pretending.
    fn flush_device(&self, device_path: &Path) -> FlushReport;
}

/// Bytes the kernel still owes a device.
pub trait WritebackInspector: Send + Sync {
    fn pending(&self, device: &PlatformDevice) -> SignalStatus<PendingWriteback>;
}

/// Processes holding files on a mount open for writing.
pub trait OpenUseInspector: Send + Sync {
    fn open_writers(&self, mount_point: &Path) -> SignalStatus<OpenWriters>;
}
