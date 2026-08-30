//! How many bytes the kernel still owes a device.
//!
//! Two sources, and they are not equally good:
//!
//! * `/sys/kernel/debug/bdi/<major:minor>/stats` accounts per backing device,
//!   which is exactly the question being asked. It lives in debugfs, which is
//!   root-only on a normal desktop, so an unprivileged session almost never
//!   gets it.
//! * `/proc/meminfo`'s `Dirty` and `Writeback` are machine-wide. They cannot
//!   prove anything about one device, so they are reported with
//!   [`WritebackScope::SystemWide`] and the readiness model treats them as
//!   corroboration only.
//!
//! When neither is readable the answer is "unavailable", never zero.

use crate::model::PlatformDevice;
use crate::roots::Roots;
use crate::traits::WritebackInspector;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use storage_core::{PendingWriteback, SignalStatus, WritebackScope};

#[derive(Clone, Debug, Default)]
pub struct LinuxWriteback {
    roots: Roots,
}

impl LinuxWriteback {
    pub fn new(roots: Roots) -> Self {
        Self { roots }
    }
}

/// Splits a `dev_t` the way glibc's `major`/`minor` macros do.
pub(crate) fn device_numbers(rdev: u64) -> (u32, u32) {
    let major = ((rdev >> 8) & 0x0000_0fff) | ((rdev >> 32) & !0x0000_0fff_u64);
    let minor = (rdev & 0xff) | ((rdev >> 12) & !0xff_u64);
    (major as u32, minor as u32)
}

/// Reads `key: value kB` lines and returns the sum of the requested keys in
/// bytes. Returns `None` if any requested key is absent, because a partial sum
/// would understate what is pending.
fn sum_kilobyte_fields(contents: &str, keys: &[&str]) -> Option<u64> {
    let mut total = 0_u64;
    for key in keys {
        let line = contents
            .lines()
            .find(|line| line.split(':').next().map(str::trim) == Some(*key))?;
        let value = line.split(':').nth(1)?;
        let number: u64 = value.split_whitespace().next()?.parse().ok()?;
        total = total.checked_add(number.checked_mul(1024)?)?;
    }
    Some(total)
}

/// Reads whitespace-separated `key value` lines, as debugfs bdi stats use, and
/// sums the requested keys in bytes.
fn sum_bdi_fields(contents: &str, keys: &[&str]) -> Option<u64> {
    let mut total = 0_u64;
    for key in keys {
        let line = contents
            .lines()
            .find(|line| line.split_whitespace().next() == Some(*key))?;
        let number: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
        total = total.checked_add(number.checked_mul(1024)?)?;
    }
    Some(total)
}

impl LinuxWriteback {
    fn per_device(&self, device_path: &Path) -> Option<PendingWriteback> {
        let metadata = std::fs::metadata(device_path).ok()?;
        let (major, minor) = device_numbers(metadata.rdev());
        let stats = self
            .roots
            .sys
            .join("kernel")
            .join("debug")
            .join("bdi")
            .join(format!("{major}:{minor}"))
            .join("stats");
        let contents = std::fs::read_to_string(stats).ok()?;
        let bytes = sum_bdi_fields(&contents, &["BdiWriteback:", "BdiReclaimable:"])
            .or_else(|| sum_bdi_fields(&contents, &["BdiWriteback:"]))?;
        Some(PendingWriteback {
            bytes,
            scope: WritebackScope::Device,
        })
    }

    fn system_wide(&self) -> Result<PendingWriteback, String> {
        let path = self.roots.proc.join("meminfo");
        let contents = std::fs::read_to_string(&path)
            .map_err(|error| format!("{}: {error}", path.display()))?;
        let bytes = sum_kilobyte_fields(&contents, &["Dirty", "Writeback"])
            .ok_or_else(|| format!("{} has no Dirty and Writeback fields", path.display()))?;
        Ok(PendingWriteback {
            bytes,
            scope: WritebackScope::SystemWide,
        })
    }
}

impl WritebackInspector for LinuxWriteback {
    fn pending(&self, device: &PlatformDevice) -> SignalStatus<PendingWriteback> {
        if let Some(pending) = self.per_device(Path::new(&device.block.device_path)) {
            return SignalStatus::Observed(pending);
        }
        match self.system_wide() {
            Ok(pending) => SignalStatus::Observed(pending),
            Err(detail) => SignalStatus::Unavailable { detail },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{BlockInfo, DeviceAddress};

    fn device(path: &str) -> PlatformDevice {
        PlatformDevice {
            address: DeviceAddress {
                object_path: "/org/freedesktop/UDisks2/block_devices/sdb1".to_string(),
                device_path: path.to_string(),
            },
            block: BlockInfo {
                device_path: path.to_string(),
                ..BlockInfo::default()
            },
            drive: None,
            mount_point: None,
        }
    }

    #[test]
    fn a_dev_t_splits_the_way_the_kernel_encodes_it() {
        // 8:17 is /dev/sdb1 on a normal machine.
        assert_eq!(device_numbers(0x0811), (8, 17));
        // A major above 255 uses the extended encoding.
        assert_eq!(device_numbers((259 << 8) | 3), (259, 3));
    }

    #[test]
    fn meminfo_fields_are_summed_in_bytes_and_a_missing_field_is_not_treated_as_zero() {
        let meminfo = "MemTotal:       32000000 kB\nDirty:               128 kB\nWriteback:            64 kB\n";
        assert_eq!(
            sum_kilobyte_fields(meminfo, &["Dirty", "Writeback"]),
            Some(192 * 1024)
        );
        assert_eq!(sum_kilobyte_fields(meminfo, &["Dirty", "Missing"]), None);
    }

    #[test]
    fn a_measured_zero_is_reported_as_a_measurement() {
        let meminfo = "Dirty:                 0 kB\nWriteback:             0 kB\n";
        assert_eq!(
            sum_kilobyte_fields(meminfo, &["Dirty", "Writeback"]),
            Some(0)
        );
    }

    #[test]
    fn bdi_stats_are_summed_in_bytes() {
        let stats = "BdiWriteback:                8 kB\nBdiReclaimable:             16 kB\nBdiDirtyThresh:              0 kB\n";
        assert_eq!(
            sum_bdi_fields(stats, &["BdiWriteback:", "BdiReclaimable:"]),
            Some(24 * 1024)
        );
    }

    #[test]
    fn a_fixture_without_either_source_reports_unavailable_rather_than_idle() {
        let temporary =
            std::env::temp_dir().join(format!("better-os-writeback-empty-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temporary);
        std::fs::create_dir_all(temporary.join("proc")).unwrap();
        let inspector = LinuxWriteback::new(Roots::at(&temporary));
        assert!(matches!(
            inspector.pending(&device("/dev/does-not-exist")),
            SignalStatus::Unavailable { .. }
        ));
        let _ = std::fs::remove_dir_all(&temporary);
    }

    #[test]
    fn the_running_host_reports_at_least_a_machine_wide_figure() {
        let inspector = LinuxWriteback::new(Roots::system());
        let status = inspector.pending(&device("/dev/does-not-exist"));
        let observed = status
            .observed()
            .expect("/proc/meminfo is readable on any Linux host");
        assert_eq!(observed.scope, WritebackScope::SystemWide);
    }
}
