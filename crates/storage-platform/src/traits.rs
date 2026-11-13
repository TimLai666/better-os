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

    /// The longest volume label each filesystem stores, in its own units:
    /// UTF-16 code units for exFAT, bytes for FAT32 and ext4.
    pub fn label_limit(self) -> usize {
        match self {
            FilesystemKind::Exfat => 15,
            FilesystemKind::Fat32 => 11,
            FilesystemKind::Ext4 => 16,
        }
    }

    /// A label's length in the units [`FilesystemKind::label_limit`] counts:
    /// exFAT stores UTF-16, ext4 stores UTF-8 bytes, FAT stores bytes.
    pub fn label_length(self, label: &str) -> usize {
        match self {
            FilesystemKind::Exfat => label.encode_utf16().count(),
            FilesystemKind::Fat32 | FilesystemKind::Ext4 => label.len(),
        }
    }
}

/// Told how many bytes of an image have reached the drive so far.
pub type ImageProgress = std::sync::Arc<dyn Fn(u64) + Send + Sync>;

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
        if self.kind.label_length(label) > self.kind.label_limit() {
            return Err(match self.kind {
                FilesystemKind::Exfat => format!(
                    "the label is longer than the {} UTF-16 units exFAT stores",
                    self.kind.label_limit()
                ),
                _ => format!(
                    "the label is longer than the {} bytes the filesystem stores",
                    self.kind.label_limit()
                ),
            });
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

    /// Writes a disk image (`.iso`, `.img`) to a whole drive, through
    /// UDisks2's `OpenForRestore` (polkit asks). `drive` is the drive's own
    /// block device, never a partition: a partition is refused. The caller
    /// unmounts every volume on the drive first. `progress` hears the bytes
    /// written as they go. Returns the bytes written.
    fn write_image(
        &self,
        drive: &DeviceAddress,
        image: &std::path::Path,
        progress: ImageProgress,
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

#[cfg(test)]
mod tests {
    use super::{FilesystemKind, FormatRequest};

    fn fits(kind: FilesystemKind, label: &str) -> bool {
        FormatRequest {
            kind,
            label: label.to_string(),
        }
        .validate()
        .is_ok()
    }

    #[test]
    fn a_label_is_measured_in_the_units_its_filesystem_stores() {
        // ext4 stores 16 bytes: six CJK characters are 18 of them.
        assert!(fits(FilesystemKind::Ext4, "相片相片相"));
        assert!(!fits(FilesystemKind::Ext4, "相片相片相片"));
        assert!(fits(FilesystemKind::Ext4, "sixteen-bytes-ok"));
        // exFAT stores 15 UTF-16 units: an emoji is two of them.
        assert!(fits(
            FilesystemKind::Exfat,
            "相片相片相片相片相片相片相片相"
        ));
        assert!(!fits(FilesystemKind::Exfat, "fourteen chars😀"));
        assert!(fits(FilesystemKind::Exfat, "thirteen char😀"));
        // FAT32: 11 bytes of plain characters.
        assert!(fits(FilesystemKind::Fat32, "ELEVEN CHAR"));
        assert!(!fits(FilesystemKind::Fat32, "TWELVE CHARS"));
    }
}
