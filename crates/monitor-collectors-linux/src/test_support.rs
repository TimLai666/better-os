//! Fixture helpers shared by the collector tests.
//!
//! The fixtures under `tests/fixtures/` are captured `/proc` and `/sys` trees
//! from a real machine plus hand-authored edge cases, so a test drives exactly
//! the production read path rather than a parser in isolation.

use crate::roots::Roots;
use monitor_core::Timestamp;
use std::path::{Path, PathBuf};

/// The path of a fixture tree. Missing trees are allowed: a test that wants to
/// prove the collectors survive an absent `/proc` names one on purpose.
pub fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

/// A timestamp at a chosen offset, so a test controls the interval a rate is
/// divided by instead of depending on how fast the machine ran the test.
pub fn at(milliseconds: u64) -> Timestamp {
    Timestamp {
        unix_ms: 1_700_000_000_000 + milliseconds,
        monotonic_ns: milliseconds * 1_000_000,
    }
}

/// A writable copy of a fixture tree, for the cases that need to remove a file
/// or take away a permission. Removed on drop.
pub struct TempTree {
    root: PathBuf,
}

impl TempTree {
    pub fn copy_of(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "better-monitor-fixture-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        copy_tree(&fixture(name), &root);
        Self { root }
    }

    pub fn path(&self) -> &Path {
        &self.root
    }

    pub fn roots(&self) -> Roots {
        Roots::at(&self.root)
    }
}

impl Drop for TempTree {
    fn drop(&mut self) {
        // Best effort: a test that already failed should report its own
        // failure, not a cleanup error.
        let _ = restore_permissions(&self.root);
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("create fixture copy");
    for entry in std::fs::read_dir(from).expect("read fixture") {
        let entry = entry.expect("fixture entry");
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).expect("copy fixture file");
        }
    }
}

/// Make every directory traversable again so the copy can be removed.
fn restore_permissions(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    if path.is_dir() {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))?;
        for entry in std::fs::read_dir(path)? {
            restore_permissions(&entry?.path())?;
        }
    }
    Ok(())
}

/// Whether the test process is root, in which case a `chmod 000` proves
/// nothing because root ignores it.
pub fn running_as_root() -> bool {
    // Reading a mode-000 file only fails for a non-root caller, so this asks
    // the question by experiment rather than by linking libc for `geteuid`.
    let probe =
        std::env::temp_dir().join(format!("better-monitor-root-probe-{}", std::process::id()));
    let _ = std::fs::remove_file(&probe);
    if std::fs::write(&probe, b"probe").is_err() {
        return false;
    }
    use std::os::unix::fs::PermissionsExt;
    let denied = std::fs::set_permissions(&probe, std::fs::Permissions::from_mode(0o000)).is_ok()
        && std::fs::read(&probe).is_ok();
    let _ = std::fs::set_permissions(&probe, std::fs::Permissions::from_mode(0o644));
    let _ = std::fs::remove_file(&probe);
    denied
}
