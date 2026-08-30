//! Where the kernel interfaces live.
//!
//! Production reads `/proc` and `/sys`. Tests point the same code at a fixture
//! tree, which is the seam Better Monitor's collectors already use: a parser
//! tested against the running host proves nothing repeatable.

use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Roots {
    pub proc: PathBuf,
    pub sys: PathBuf,
}

impl Roots {
    pub fn system() -> Self {
        Self {
            proc: PathBuf::from("/proc"),
            sys: PathBuf::from("/sys"),
        }
    }

    /// A fixture tree containing `proc/` and `sys/` directories.
    pub fn at(root: impl AsRef<Path>) -> Self {
        let root = root.as_ref();
        Self {
            proc: root.join("proc"),
            sys: root.join("sys"),
        }
    }
}

impl Default for Roots {
    fn default() -> Self {
        Self::system()
    }
}
