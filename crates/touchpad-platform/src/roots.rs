//! Where the kernel interfaces live.
//!
//! Nothing in this crate opens a hardcoded `/proc` or `/sys` path. Every read
//! goes through a `Roots`, so a test can point device enumeration at a captured
//! snapshot and get exactly the code path production takes. This is the same
//! seam `monitor-collectors-linux` uses, for the same reason: a live host
//! proves nothing repeatable about a parser.

use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Roots {
    proc_dir: PathBuf,
    sys_dir: PathBuf,
}

impl Roots {
    /// The real machine.
    pub fn system() -> Self {
        Self {
            proc_dir: PathBuf::from("/proc"),
            sys_dir: PathBuf::from("/sys"),
        }
    }

    /// A captured snapshot laid out as `<root>/proc` and `<root>/sys`.
    pub fn at(root: impl AsRef<Path>) -> Self {
        let root = root.as_ref();
        Self {
            proc_dir: root.join("proc"),
            sys_dir: root.join("sys"),
        }
    }

    pub fn new(proc_dir: impl Into<PathBuf>, sys_dir: impl Into<PathBuf>) -> Self {
        Self {
            proc_dir: proc_dir.into(),
            sys_dir: sys_dir.into(),
        }
    }

    pub fn proc(&self, relative: &str) -> PathBuf {
        self.proc_dir.join(relative)
    }

    pub fn sys(&self, relative: &str) -> PathBuf {
        self.sys_dir.join(relative)
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
        assert_eq!(
            roots.proc("bus/input/devices"),
            PathBuf::from("/proc/bus/input/devices")
        );
        assert_eq!(
            roots.sys("class/input/event5/device/name"),
            PathBuf::from("/sys/class/input/event5/device/name")
        );
    }

    #[test]
    fn a_snapshot_root_redirects_both_interfaces() {
        let roots = Roots::at("/fixtures/one-touchpad");
        assert_eq!(
            roots.proc("bus/input/devices"),
            PathBuf::from("/fixtures/one-touchpad/proc/bus/input/devices")
        );
        assert_eq!(
            roots.sys("class/input/event5"),
            PathBuf::from("/fixtures/one-touchpad/sys/class/input/event5")
        );
    }
}
