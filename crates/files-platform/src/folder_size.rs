//! How much a folder holds: the walk behind a folder's Size cell.
//!
//! The total is the sum of the regular files beneath a folder, each counted
//! once however many hard links reach it, as `du --apparent-size` counts. A
//! symbolic link is never followed (it adds nothing of its own), and the walk
//! never enters another filesystem, so a disk mounted somewhere inside a home
//! folder is not added to it.
//!
//! The walk is bounded three ways, and a bound that is reached gives an
//! answer that says so rather than the number reached so far: a folder with
//! more entries than [`FolderSizeLimits::max_items`], or one still being
//! walked at its deadline, is [`FolderSize::TooMany`]. A [`SizeBudget`] shared
//! by every folder of one visit caps the whole visit's work, and its
//! cancellation token stops a walk between two entries when the person leaves.
//!
//! Nothing here decides which folders are walked. Whether a network share, a
//! FUSE mount, or the Trash is looked at is the window's decision, made with
//! [`MountTable::reach`](crate::MountTable::reach) before a walk is asked for.

use std::collections::HashSet;
use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use files_core::CancellationToken;

/// The bounds on one folder's walk.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FolderSizeLimits {
    /// More entries than this (files and folders together) and the folder is
    /// [`FolderSize::TooMany`].
    pub max_items: u64,
    /// A walk still running this long after it started stops and reports
    /// [`FolderSize::TooMany`] with what it had counted.
    pub deadline: Duration,
}

impl Default for FolderSizeLimits {
    fn default() -> Self {
        Self {
            max_items: 1_000_000,
            deadline: Duration::from_secs(20),
        }
    }
}

/// What every walk of one folder visit may spend together, and the flag that
/// stops them all.
#[derive(Debug)]
pub struct SizeBudget {
    remaining: AtomicU64,
    until: Instant,
    token: CancellationToken,
}

impl SizeBudget {
    pub fn new(items: u64, time: Duration, token: CancellationToken) -> Self {
        Self {
            remaining: AtomicU64::new(items),
            until: Instant::now() + time,
            token,
        }
    }

    /// A budget nothing will run out of, for a caller measuring one folder.
    pub fn unlimited() -> Self {
        Self::new(
            u64::MAX,
            Duration::from_secs(u64::MAX / 4),
            CancellationToken::new(),
        )
    }

    pub fn token(&self) -> &CancellationToken {
        &self.token
    }

    /// Whether this budget has nothing left, so a walk not yet started need
    /// not start.
    pub fn is_spent(&self) -> bool {
        self.remaining.load(Ordering::Relaxed) == 0 || Instant::now() >= self.until
    }

    /// Takes one item. False when there was none left.
    fn take(&self) -> bool {
        // A compare-and-swap loop rather than `fetch_update`, which newer
        // toolchains deprecate and `rust-version` does not yet have the
        // replacement for.
        let mut left = self.remaining.load(Ordering::Relaxed);
        loop {
            let Some(next) = left.checked_sub(1) else {
                return false;
            };
            match self.remaining.compare_exchange_weak(
                left,
                next,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => return true,
                Err(actual) => left = actual,
            }
        }
    }
}

/// The answer for one folder.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FolderSize {
    /// The walk reached everything it could. `unreadable` counts the folders
    /// inside that could not be listed; when it is not zero, `bytes` is what
    /// the readable part holds and the real total is at least that.
    Total {
        bytes: u64,
        files: u64,
        folders: u64,
        unreadable: u64,
    },
    /// The walk stopped at a bound: the folder holds more than `items`
    /// entries, or more than could be counted in time.
    TooMany { items: u64 },
    /// The folder itself could not be listed.
    Unreadable,
    /// The person left the folder, or the visit's budget ran out before this
    /// walk could finish.
    Cancelled,
}

/// Works out how much `root` holds. Runs on the caller's thread and blocks
/// for as long as the walk takes, up to the limits.
pub fn measure(root: &Path, limits: FolderSizeLimits, budget: &SizeBudget) -> FolderSize {
    let started = Instant::now();
    let root_device = match std::fs::symlink_metadata(root) {
        Ok(metadata) if metadata.is_dir() => metadata.dev(),
        _ => return FolderSize::Unreadable,
    };
    let mut pending: Vec<PathBuf> = vec![root.to_path_buf()];
    let mut linked: HashSet<(u64, u64)> = HashSet::new();
    let (mut bytes, mut files, mut folders, mut unreadable, mut items) =
        (0u64, 0u64, 0u64, 0u64, 0u64);
    let mut first = true;
    while let Some(directory) = pending.pop() {
        let reader = match std::fs::read_dir(&directory) {
            Ok(reader) => reader,
            Err(error) if first && error.kind() != io::ErrorKind::Interrupted => {
                return FolderSize::Unreadable;
            }
            Err(_) => {
                unreadable += 1;
                continue;
            }
        };
        first = false;
        for child in reader {
            if budget.token.is_cancelled() {
                return FolderSize::Cancelled;
            }
            items += 1;
            if items > limits.max_items || started.elapsed() >= limits.deadline {
                return FolderSize::TooMany {
                    items: items.saturating_sub(1),
                };
            }
            if !budget.take() || Instant::now() >= budget.until {
                return FolderSize::TooMany {
                    items: items.saturating_sub(1),
                };
            }
            let Ok(child) = child else {
                unreadable += 1;
                continue;
            };
            // `DirEntry::metadata` is `fstatat(AT_SYMLINK_NOFOLLOW)`: a link
            // is looked at, never through.
            let Ok(metadata) = child.metadata() else {
                unreadable += 1;
                continue;
            };
            let kind = metadata.file_type();
            if kind.is_dir() {
                folders += 1;
                if metadata.dev() == root_device {
                    pending.push(child.path());
                }
            } else if kind.is_file() {
                files += 1;
                if metadata.nlink() > 1 && !linked.insert((metadata.dev(), metadata.ino())) {
                    continue;
                }
                bytes = bytes.saturating_add(metadata.len());
            }
        }
    }
    FolderSize::Total {
        bytes,
        files,
        folders,
        unreadable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    fn total(path: &Path) -> FolderSize {
        measure(path, FolderSizeLimits::default(), &SizeBudget::unlimited())
    }

    #[test]
    fn sums_every_file_beneath_and_counts_a_hard_link_once() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("root");
        fs::create_dir_all(root.join("a/b")).unwrap();
        fs::write(root.join("one"), vec![0u8; 100]).unwrap();
        fs::write(root.join("a/two"), vec![0u8; 20]).unwrap();
        fs::write(root.join("a/b/three"), vec![0u8; 3]).unwrap();
        fs::hard_link(root.join("one"), root.join("a/b/one-again")).unwrap();
        assert_eq!(
            total(&root),
            FolderSize::Total {
                bytes: 123,
                files: 4,
                folders: 2,
                unreadable: 0
            }
        );
    }

    #[test]
    fn a_symbolic_link_is_never_followed() {
        let dir = tempfile::tempdir().unwrap();
        let outside = dir.path().join("outside");
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("big"), vec![0u8; 5000]).unwrap();
        let root = dir.path().join("root");
        fs::create_dir(&root).unwrap();
        fs::write(root.join("small"), vec![0u8; 7]).unwrap();
        std::os::unix::fs::symlink(&outside, root.join("to-folder")).unwrap();
        std::os::unix::fs::symlink(outside.join("big"), root.join("to-file")).unwrap();
        // A loop back to the root would never end if links were followed.
        std::os::unix::fs::symlink(&root, root.join("loop")).unwrap();
        let FolderSize::Total { bytes, files, .. } = total(&root) else {
            panic!("expected a total");
        };
        assert_eq!((bytes, files), (7, 1));
    }

    #[test]
    fn another_filesystem_inside_is_not_entered() {
        // /proc is its own filesystem under / on every Linux host; a walk of a
        // folder holding a bind of it is not available without privilege, so
        // the device rule is checked directly on a folder whose only child
        // lives on another device: /dev/shm under /dev (devtmpfs vs tmpfs).
        let dev = fs::symlink_metadata("/dev").unwrap().dev();
        let shm = fs::symlink_metadata("/dev/shm");
        let Ok(shm) = shm else { return };
        assert_ne!(dev, shm.dev(), "the host keeps /dev/shm on its own mount");
        // Walking /dev must not descend into /dev/shm; its entries would be
        // counted as folders otherwise. Count what /dev/shm alone holds and
        // what /dev holds, and check the second does not include the first's
        // files.
        let marker = tempfile::Builder::new()
            .prefix("better-files-size-")
            .tempfile_in("/dev/shm");
        let Ok(mut marker) = marker else { return };
        std::io::Write::write_all(&mut marker, &vec![0u8; 1 << 20]).unwrap();
        let in_dev = measure(
            Path::new("/dev"),
            FolderSizeLimits::default(),
            &SizeBudget::unlimited(),
        );
        if let FolderSize::Total { bytes, .. } = in_dev {
            assert!(bytes < 1 << 20, "/dev counted a file on /dev/shm: {bytes}");
        }
    }

    #[test]
    fn an_unreadable_folder_is_unreadable_and_an_unreadable_inner_folder_is_counted() {
        if unsafe_is_root() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("root");
        fs::create_dir_all(root.join("locked")).unwrap();
        fs::write(root.join("seen"), vec![0u8; 9]).unwrap();
        fs::write(root.join("locked/hidden"), vec![0u8; 50]).unwrap();
        fs::set_permissions(root.join("locked"), fs::Permissions::from_mode(0o000)).unwrap();
        let inner = total(&root);
        let locked = total(&root.join("locked"));
        fs::set_permissions(root.join("locked"), fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(
            inner,
            FolderSize::Total {
                bytes: 9,
                files: 1,
                folders: 1,
                unreadable: 1
            }
        );
        assert_eq!(locked, FolderSize::Unreadable);
        assert_eq!(total(&root.join("missing")), FolderSize::Unreadable);
    }

    #[test]
    fn the_item_limit_and_the_deadline_say_too_many_rather_than_a_number() {
        let dir = tempfile::tempdir().unwrap();
        for index in 0..10 {
            fs::write(dir.path().join(index.to_string()), b"x").unwrap();
        }
        let limits = FolderSizeLimits {
            max_items: 4,
            deadline: Duration::from_secs(60),
        };
        assert_eq!(
            measure(dir.path(), limits, &SizeBudget::unlimited()),
            FolderSize::TooMany { items: 4 }
        );
        let limits = FolderSizeLimits {
            max_items: 100,
            deadline: Duration::ZERO,
        };
        assert_eq!(
            measure(dir.path(), limits, &SizeBudget::unlimited()),
            FolderSize::TooMany { items: 0 }
        );
    }

    #[test]
    fn the_shared_budget_caps_every_walk_of_a_visit_together() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["a", "b"] {
            fs::create_dir(dir.path().join(name)).unwrap();
            for index in 0..5 {
                fs::write(dir.path().join(name).join(index.to_string()), b"x").unwrap();
            }
        }
        let budget = SizeBudget::new(7, Duration::from_secs(60), CancellationToken::new());
        let limits = FolderSizeLimits::default();
        assert!(matches!(
            measure(&dir.path().join("a"), limits, &budget),
            FolderSize::Total { files: 5, .. }
        ));
        assert_eq!(
            measure(&dir.path().join("b"), limits, &budget),
            FolderSize::TooMany { items: 2 }
        );
        assert!(budget.is_spent());
    }

    #[test]
    fn a_cancelled_walk_stops() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a"), b"x").unwrap();
        let token = CancellationToken::new();
        let budget = SizeBudget::new(100, Duration::from_secs(60), token.clone());
        token.cancel();
        assert_eq!(
            measure(dir.path(), FolderSizeLimits::default(), &budget),
            FolderSize::Cancelled
        );
    }

    fn unsafe_is_root() -> bool {
        crate::current_uid() == Some(0)
    }
}
