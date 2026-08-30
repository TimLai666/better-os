//! Where the kernel interfaces live.
//!
//! Nothing in this crate opens a hardcoded `/proc` or `/sys` path. Every read
//! goes through a `Roots`, so a test can point a collector at a captured
//! snapshot and get exactly the code path production takes.

use std::path::{Path, PathBuf};

/// The filesystem roots a collector reads from.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Roots {
    proc_dir: PathBuf,
    sys_dir: PathBuf,
    passwd_path: PathBuf,
}

impl Roots {
    /// The real machine.
    pub fn system() -> Self {
        Self {
            proc_dir: PathBuf::from("/proc"),
            sys_dir: PathBuf::from("/sys"),
            passwd_path: PathBuf::from("/etc/passwd"),
        }
    }

    /// A captured snapshot laid out as `<root>/proc`, `<root>/sys`, and
    /// `<root>/etc/passwd`.
    pub fn at(root: impl AsRef<Path>) -> Self {
        let root = root.as_ref();
        Self {
            proc_dir: root.join("proc"),
            sys_dir: root.join("sys"),
            passwd_path: root.join("etc").join("passwd"),
        }
    }

    pub fn new(
        proc_dir: impl Into<PathBuf>,
        sys_dir: impl Into<PathBuf>,
        passwd_path: impl Into<PathBuf>,
    ) -> Self {
        Self {
            proc_dir: proc_dir.into(),
            sys_dir: sys_dir.into(),
            passwd_path: passwd_path.into(),
        }
    }

    /// A path under the proc root, given the same relative form a
    /// `MetricSource::Proc` uses.
    pub fn proc(&self, relative: &str) -> PathBuf {
        self.proc_dir.join(relative)
    }

    pub fn proc_dir(&self) -> &Path {
        &self.proc_dir
    }

    /// A path under the sys root, given the same relative form a
    /// `MetricSource::Sys` uses.
    pub fn sys(&self, relative: &str) -> PathBuf {
        self.sys_dir.join(relative)
    }

    pub fn sys_dir(&self) -> &Path {
        &self.sys_dir
    }

    pub fn passwd_path(&self) -> &Path {
        &self.passwd_path
    }
}

impl Default for Roots {
    fn default() -> Self {
        Self::system()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_system_roots_are_the_real_kernel_interfaces() {
        let roots = Roots::system();
        assert_eq!(roots.proc("stat"), PathBuf::from("/proc/stat"));
        assert_eq!(
            roots.sys("class/net/eth0/speed"),
            PathBuf::from("/sys/class/net/eth0/speed")
        );
        assert_eq!(roots.passwd_path(), Path::new("/etc/passwd"));
    }

    #[test]
    fn a_snapshot_root_redirects_every_interface_the_collectors_read() {
        let roots = Roots::at("/fixtures/snapshot-a");
        assert_eq!(
            roots.proc("pressure/cpu"),
            PathBuf::from("/fixtures/snapshot-a/proc/pressure/cpu")
        );
        assert_eq!(
            roots.sys("block/nvme0n1/queue/rotational"),
            PathBuf::from("/fixtures/snapshot-a/sys/block/nvme0n1/queue/rotational")
        );
        assert_eq!(
            roots.passwd_path(),
            Path::new("/fixtures/snapshot-a/etc/passwd")
        );
    }
}
