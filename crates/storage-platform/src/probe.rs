//! The doctor probe: what this host actually exposes.
//!
//! Run against the live system it answers the questions the safety model
//! depends on — is UDisks2 there, which volumes are external, which identity
//! each one gets, and which of the three signals this session can actually
//! read. It is read-only by default: it never mounts, unmounts, or flushes
//! anything unless asked to.

use crate::model::{DeviceClass, PlatformDevice};
use crate::openuse::ProcOpenUse;
use crate::roots::Roots;
use crate::traits::{DeviceControl, FlushBackend, OpenUseInspector, WritebackInspector};
use crate::writeback::LinuxWriteback;
use std::fmt;
use storage_core::{DeviceIdentity, SignalStatus};

fn describe<T: fmt::Debug>(status: &SignalStatus<T>) -> String {
    match status {
        SignalStatus::Observed(value) => format!("observed: {value:?}"),
        SignalStatus::Unsupported { detail } => format!("unsupported ({detail})"),
        SignalStatus::Unavailable { detail } => format!("unavailable ({detail})"),
        SignalStatus::PermissionDenied { detail } => format!("permission denied ({detail})"),
    }
}

pub struct DeviceReport {
    pub device: PlatformDevice,
    pub class: DeviceClass,
    pub identity: DeviceIdentity,
    pub writeback: String,
    pub open_writers: String,
    pub flush: Option<String>,
}

pub struct DoctorReport {
    pub devices: Vec<DeviceReport>,
}

impl fmt::Display for DoctorReport {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(formatter, "block devices reported: {}", self.devices.len())?;
        let external = self
            .devices
            .iter()
            .filter(|report| report.class.is_external())
            .count();
        writeln!(formatter, "external hot-pluggable:  {external}")?;
        for report in &self.devices {
            writeln!(formatter)?;
            writeln!(formatter, "{}", report.device.address.device_path)?;
            writeln!(
                formatter,
                "  object path:  {}",
                report.device.address.object_path
            )?;
            writeln!(formatter, "  class:        {:?}", report.class)?;
            writeln!(
                formatter,
                "  filesystem:   {}",
                report.device.block.id_type.as_deref().unwrap_or("(none)")
            )?;
            writeln!(
                formatter,
                "  mount point:  {}",
                report
                    .device
                    .mount_point
                    .as_ref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|| "(not mounted)".to_string())
            )?;
            writeln!(formatter, "  identity:     {}", report.identity.key())?;
            writeln!(
                formatter,
                "  confidence:   {:?}",
                report.identity.confidence()
            )?;
            writeln!(formatter, "  writeback:    {}", report.writeback)?;
            writeln!(formatter, "  open writers: {}", report.open_writers)?;
            if let Some(flush) = &report.flush {
                writeln!(formatter, "  flush:        {flush}")?;
            }
        }
        Ok(())
    }
}

/// Probes every block device the control reports.
///
/// `flush` makes the probe issue a real filesystem-scoped flush on each mounted
/// external volume. It is off by default because a probe should not change
/// anything about the machine it is describing.
pub async fn probe<C: DeviceControl, F: FlushBackend>(
    control: &C,
    flush_backend: &F,
    roots: Roots,
    flush: bool,
) -> Result<DoctorReport, crate::traits::PlatformError> {
    let writeback = LinuxWriteback::new(roots.clone());
    let open_use = ProcOpenUse::new(roots);

    let mut devices = Vec::new();
    for device in control.enumerate().await? {
        let class = device.classify();
        let identity = DeviceIdentity::from_evidence(device.identity_evidence());
        let writeback_status = describe(&writeback.pending(&device));
        let open_status = match &device.mount_point {
            Some(mount) => describe(&open_use.open_writers(mount)),
            None => "not applicable (not mounted)".to_string(),
        };
        let flush_status = match (&device.mount_point, flush && class.is_external()) {
            (Some(mount), true) => Some(format!("{:?}", flush_backend.flush_filesystem(mount))),
            _ => None,
        };
        devices.push(DeviceReport {
            device,
            class,
            identity,
            writeback: writeback_status,
            open_writers: open_status,
            flush: flush_status,
        });
    }
    Ok(DoctorReport { devices })
}

#[cfg(all(test, feature = "test-support"))]
mod tests {
    use super::*;
    use crate::fake::{FakeDeviceControl, FakeFlush, internal_disk, usb_stick};

    #[tokio::test]
    async fn the_report_separates_external_devices_from_internal_ones() {
        let control = FakeDeviceControl::new([
            usb_stick("/objects/sdb1", "/dev/sdb1", "A1B2-C3D4"),
            internal_disk("/objects/nvme0n1p2", "/dev/nvme0n1p2"),
        ]);
        let report = probe(&control, &FakeFlush::default(), Roots::system(), false)
            .await
            .unwrap();
        assert_eq!(report.devices.len(), 2);
        assert_eq!(
            report
                .devices
                .iter()
                .filter(|device| device.class.is_external())
                .count(),
            1
        );
        let rendered = report.to_string();
        assert!(rendered.contains("external hot-pluggable:  1"));
    }

    #[tokio::test]
    async fn a_read_only_probe_never_flushes_anything() {
        let control =
            FakeDeviceControl::new([usb_stick("/objects/sdb1", "/dev/sdb1", "A1B2-C3D4")]);
        control
            .mount(&control.enumerate().await.unwrap()[0].address.clone())
            .await
            .unwrap();
        let flush = FakeFlush::default();
        probe(&control, &flush, Roots::system(), false)
            .await
            .unwrap();
        assert!(flush.flushed().is_empty());
    }
}
