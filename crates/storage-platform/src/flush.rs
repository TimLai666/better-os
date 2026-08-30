//! Flushing, at the narrowest scope that applies.
//!
//! `syncfs(2)` on a descriptor for the mount writes back that one filesystem
//! and returns when it is done. That is the operation this component builds its
//! readiness claim on, and it is deliberately not `sync(2)`: a machine-wide
//! flush after every small file operation would charge every other filesystem
//! for one USB stick's safety.
//!
//! `BLKFLSBUF` on the block device is the second half, and on most desktops it
//! is not available to an unprivileged session — opening `/dev/sdb` for writing
//! needs privilege this component deliberately does not take. That case is
//! reported as unsupported, never as a flush that happened.

use crate::traits::{FlushBackend, FlushReport};
use std::fs::{File, OpenOptions};
use std::os::fd::AsRawFd;
use std::path::Path;
use storage_core::FlushScope;

/// `BLKFLSBUF` from `include/uapi/linux/fs.h`: flush the buffer cache for a
/// block device.
const BLKFLSBUF: libc::c_ulong = 0x1261;

#[derive(Clone, Copy, Debug, Default)]
pub struct LinuxFlush;

impl FlushBackend for LinuxFlush {
    fn flush_filesystem(&self, mount_point: &Path) -> FlushReport {
        let directory = match File::open(mount_point) {
            Ok(file) => file,
            Err(error) => {
                return FlushReport::Failed {
                    detail: format!("could not open {}: {error}", mount_point.display()),
                };
            }
        };
        // SAFETY: the descriptor is owned by `directory` and stays open for the
        // duration of the call.
        let result = unsafe { libc::syncfs(directory.as_raw_fd()) };
        if result == 0 {
            FlushReport::Completed {
                scope: FlushScope::Filesystem,
            }
        } else {
            let error = std::io::Error::last_os_error();
            match error.raw_os_error() {
                // The filesystem does not implement writeback this way.
                Some(libc::ENOSYS) | Some(libc::ENOTSUP) => FlushReport::Unsupported {
                    detail: format!(
                        "syncfs is not implemented for {}: {error}",
                        mount_point.display()
                    ),
                },
                _ => FlushReport::Failed {
                    detail: format!("syncfs on {} failed: {error}", mount_point.display()),
                },
            }
        }
    }

    fn flush_device(&self, device_path: &Path) -> FlushReport {
        // BLKFLSBUF needs the device open for writing, which an unprivileged
        // session does not get. This is one of the two places a privileged
        // helper would change the answer; see docs/storage-safety-signals.md.
        let device = match OpenOptions::new().read(true).write(true).open(device_path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                return FlushReport::Unsupported {
                    detail: format!(
                        "a device-level flush of {} needs privileges this service does not hold",
                        device_path.display()
                    ),
                };
            }
            Err(error) => {
                return FlushReport::Failed {
                    detail: format!("could not open {}: {error}", device_path.display()),
                };
            }
        };
        // SAFETY: the descriptor is owned by `device` and BLKFLSBUF takes no
        // argument.
        let result = unsafe { libc::ioctl(device.as_raw_fd(), BLKFLSBUF) };
        if result == 0 {
            FlushReport::Completed {
                scope: FlushScope::Device,
            }
        } else {
            let error = std::io::Error::last_os_error();
            FlushReport::Failed {
                detail: format!("BLKFLSBUF on {} failed: {error}", device_path.display()),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flushing_a_real_directory_reports_a_filesystem_scoped_completion() {
        // This runs against whatever filesystem the build directory is on. It
        // proves the call path and the scope, not anything about a USB stick.
        let report = LinuxFlush.flush_filesystem(Path::new(env!("CARGO_MANIFEST_DIR")));
        assert_eq!(
            report,
            FlushReport::Completed {
                scope: FlushScope::Filesystem
            }
        );
    }

    #[test]
    fn flushing_a_path_that_is_not_there_fails_rather_than_reporting_success() {
        let report = LinuxFlush.flush_filesystem(Path::new("/nonexistent-better-os-mount"));
        assert!(matches!(report, FlushReport::Failed { .. }));
    }

    #[test]
    fn a_device_flush_this_session_may_not_issue_is_reported_as_unsupported() {
        // As an unprivileged user this is the expected answer for any real
        // block device. If the tests are ever run as root the call may reach
        // the ioctl instead, so both honest answers are accepted — what must
        // never happen is a silent success for a device that was never opened.
        let report = LinuxFlush.flush_device(Path::new("/dev/mem"));
        assert!(
            matches!(
                report,
                FlushReport::Unsupported { .. } | FlushReport::Failed { .. }
            ),
            "unexpected report: {report:?}"
        );
    }
}
