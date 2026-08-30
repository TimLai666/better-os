//! Who still has a file on the mount open for writing.
//!
//! The scan walks `/proc/<pid>/fd`, resolves each descriptor, and keeps the
//! ones that land inside the mount and were opened for writing. A session
//! process cannot read another user's `/proc` entries, so the scan counts what
//! it could not inspect and reports [`ScanCoverage::Partial`] rather than
//! implying it saw everything. That distinction is the whole reason this
//! returns a coverage value at all.

use crate::roots::Roots;
use crate::traits::OpenUseInspector;
use std::path::Path;
use storage_core::{OpenWriters, ScanCoverage, SignalStatus, WriterIdentity};

#[derive(Clone, Debug, Default)]
pub struct ProcOpenUse {
    roots: Roots,
}

impl ProcOpenUse {
    pub fn new(roots: Roots) -> Self {
        Self { roots }
    }
}

/// Whether an fdinfo `flags` value was opened for writing. The value is octal,
/// and the low two bits are the access mode: 0 read-only, 1 write-only, 2
/// read-write.
fn opened_for_writing(fdinfo: &str) -> Option<bool> {
    let line = fdinfo.lines().find(|line| line.starts_with("flags:"))?;
    let raw = line.split_whitespace().nth(1)?;
    let flags = u32::from_str_radix(raw, 8).ok()?;
    Some(flags & 0b11 != 0)
}

fn process_name(proc_root: &Path, pid: i32) -> Option<String> {
    std::fs::read_to_string(proc_root.join(pid.to_string()).join("comm"))
        .ok()
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
}

fn is_inside(target: &Path, mount_point: &Path) -> bool {
    target != mount_point && target.starts_with(mount_point)
}

impl OpenUseInspector for ProcOpenUse {
    fn open_writers(&self, mount_point: &Path) -> SignalStatus<OpenWriters> {
        let entries = match std::fs::read_dir(&self.roots.proc) {
            Ok(entries) => entries,
            Err(error) => {
                return SignalStatus::Unsupported {
                    detail: format!("{} is not readable: {error}", self.roots.proc.display()),
                };
            }
        };

        let mut writers: Vec<WriterIdentity> = Vec::new();
        let mut unreadable_processes = 0_u32;

        for entry in entries.flatten() {
            let Ok(pid) = entry.file_name().to_string_lossy().parse::<i32>() else {
                continue;
            };
            let fd_directory = entry.path().join("fd");
            let descriptors = match std::fs::read_dir(&fd_directory) {
                Ok(descriptors) => descriptors,
                // A process that belongs to someone else, or one that exited
                // between listing and reading. Either way this scan did not see
                // it, and that has to be visible in the result.
                Err(_) => {
                    if fd_directory.exists() {
                        unreadable_processes += 1;
                    }
                    continue;
                }
            };

            let mut holds_write = false;
            for descriptor in descriptors.flatten() {
                let Ok(target) = std::fs::read_link(descriptor.path()) else {
                    continue;
                };
                if !is_inside(&target, mount_point) {
                    continue;
                }
                let fdinfo = self
                    .roots
                    .proc
                    .join(pid.to_string())
                    .join("fdinfo")
                    .join(descriptor.file_name());
                match std::fs::read_to_string(&fdinfo)
                    .ok()
                    .and_then(|contents| opened_for_writing(&contents))
                {
                    Some(true) => {
                        holds_write = true;
                        break;
                    }
                    Some(false) => {}
                    // The descriptor points into the mount but its mode could
                    // not be read. Counting it as a reader would be a guess in
                    // the reassuring direction, so it counts as a blocker.
                    None => {
                        holds_write = true;
                        break;
                    }
                }
            }

            if holds_write {
                writers.push(WriterIdentity {
                    pid,
                    name: process_name(&self.roots.proc, pid),
                });
            }
        }

        writers.sort();
        SignalStatus::Observed(OpenWriters {
            writers,
            coverage: if unreadable_processes == 0 {
                ScanCoverage::Complete
            } else {
                ScanCoverage::Partial {
                    unreadable_processes,
                }
            },
        })
    }
}

/// Builds a `/proc`-shaped fixture tree, used by the tests here and available
/// to the service's tests through the `test-support` feature.
#[cfg(any(test, feature = "test-support"))]
pub mod fixture {
    use std::path::{Path, PathBuf};

    /// One process to represent in the fixture.
    pub struct FakeProcess {
        pub pid: i32,
        pub name: &'static str,
        /// Open descriptors as (target path, octal flags).
        pub descriptors: Vec<(PathBuf, &'static str)>,
        /// Whether the fd directory should be unreadable, standing in for
        /// another user's process.
        pub readable: bool,
    }

    /// Writes the tree and returns the `proc` root.
    pub fn write_proc_tree(root: &Path, processes: &[FakeProcess]) -> std::io::Result<PathBuf> {
        let proc_root = root.join("proc");
        std::fs::create_dir_all(&proc_root)?;
        for process in processes {
            let directory = proc_root.join(process.pid.to_string());
            std::fs::create_dir_all(&directory)?;
            std::fs::write(directory.join("comm"), format!("{}\n", process.name))?;
            if !process.readable {
                // An entry that exists but whose fd directory cannot be listed.
                std::fs::write(directory.join("fd"), b"")?;
                continue;
            }
            let fd_directory = directory.join("fd");
            let fdinfo_directory = directory.join("fdinfo");
            std::fs::create_dir_all(&fd_directory)?;
            std::fs::create_dir_all(&fdinfo_directory)?;
            for (index, (target, flags)) in process.descriptors.iter().enumerate() {
                std::os::unix::fs::symlink(target, fd_directory.join(index.to_string()))?;
                std::fs::write(
                    fdinfo_directory.join(index.to_string()),
                    format!("pos:\t0\nflags:\t{flags}\nmnt_id:\t1\n"),
                )?;
            }
        }
        Ok(proc_root)
    }
}

#[cfg(test)]
mod tests {
    use super::fixture::{FakeProcess, write_proc_tree};
    use super::*;
    use std::path::PathBuf;

    struct Fixture {
        root: PathBuf,
    }

    impl Fixture {
        fn new(label: &str) -> Self {
            let root = std::env::temp_dir()
                .join(format!("better-os-openuse-{label}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(&root).unwrap();
            Self { root }
        }

        fn roots(&self) -> Roots {
            Roots {
                proc: self.root.join("proc"),
                sys: self.root.join("sys"),
            }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    const MOUNT: &str = "/run/media/user/FIELD DATA";

    #[test]
    fn a_process_writing_into_the_mount_is_found_and_named() {
        let fixture = Fixture::new("writer");
        write_proc_tree(
            &fixture.root,
            &[FakeProcess {
                pid: 4242,
                name: "libreoffice",
                descriptors: vec![(PathBuf::from(MOUNT).join("report.odt"), "100002")],
                readable: true,
            }],
        )
        .unwrap();

        let status = ProcOpenUse::new(fixture.roots()).open_writers(Path::new(MOUNT));
        let observed = status.observed().expect("an observation");
        assert_eq!(observed.coverage, ScanCoverage::Complete);
        assert_eq!(observed.writers.len(), 1);
        assert_eq!(observed.writers[0].pid, 4242);
        assert_eq!(observed.writers[0].name.as_deref(), Some("libreoffice"));
    }

    #[test]
    fn a_reader_does_not_count_as_a_writer() {
        let fixture = Fixture::new("reader");
        write_proc_tree(
            &fixture.root,
            &[FakeProcess {
                pid: 4243,
                name: "eog",
                descriptors: vec![(PathBuf::from(MOUNT).join("photo.jpg"), "100000")],
                readable: true,
            }],
        )
        .unwrap();

        let status = ProcOpenUse::new(fixture.roots()).open_writers(Path::new(MOUNT));
        assert!(
            status
                .observed()
                .expect("an observation")
                .writers
                .is_empty()
        );
    }

    #[test]
    fn a_write_to_another_filesystem_is_not_attributed_to_this_mount() {
        let fixture = Fixture::new("elsewhere");
        write_proc_tree(
            &fixture.root,
            &[FakeProcess {
                pid: 4244,
                name: "journald",
                descriptors: vec![(PathBuf::from("/var/log/journal/system.journal"), "100002")],
                readable: true,
            }],
        )
        .unwrap();

        let status = ProcOpenUse::new(fixture.roots()).open_writers(Path::new(MOUNT));
        assert!(
            status
                .observed()
                .expect("an observation")
                .writers
                .is_empty()
        );
    }

    #[test]
    fn a_process_this_session_cannot_inspect_is_counted_rather_than_ignored() {
        let fixture = Fixture::new("foreign");
        write_proc_tree(
            &fixture.root,
            &[
                FakeProcess {
                    pid: 1,
                    name: "systemd",
                    descriptors: Vec::new(),
                    readable: false,
                },
                FakeProcess {
                    pid: 4245,
                    name: "bash",
                    descriptors: Vec::new(),
                    readable: true,
                },
            ],
        )
        .unwrap();

        let status = ProcOpenUse::new(fixture.roots()).open_writers(Path::new(MOUNT));
        let observed = status.observed().expect("an observation");
        assert_eq!(
            observed.coverage,
            ScanCoverage::Partial {
                unreadable_processes: 1
            }
        );
        assert!(observed.writers.is_empty());
    }

    #[test]
    fn a_host_without_procfs_reports_unsupported_rather_than_an_empty_answer() {
        let fixture = Fixture::new("noproc");
        let status = ProcOpenUse::new(fixture.roots()).open_writers(Path::new(MOUNT));
        assert!(matches!(status, SignalStatus::Unsupported { .. }));
    }

    #[test]
    fn the_mount_point_itself_being_open_is_not_a_write_into_it() {
        let fixture = Fixture::new("mountopen");
        write_proc_tree(
            &fixture.root,
            &[FakeProcess {
                pid: 4246,
                name: "nautilus",
                descriptors: vec![(PathBuf::from(MOUNT), "100000")],
                readable: true,
            }],
        )
        .unwrap();
        let status = ProcOpenUse::new(fixture.roots()).open_writers(Path::new(MOUNT));
        assert!(
            status
                .observed()
                .expect("an observation")
                .writers
                .is_empty()
        );
    }

    #[test]
    fn octal_flags_decide_the_access_mode() {
        assert_eq!(opened_for_writing("flags:\t0100000\n"), Some(false));
        assert_eq!(opened_for_writing("flags:\t0100001\n"), Some(true));
        assert_eq!(opened_for_writing("flags:\t0100002\n"), Some(true));
        assert_eq!(opened_for_writing("pos:\t0\n"), None);
    }
}
