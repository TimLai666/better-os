//! Turning a spec into a list of items, before a single byte moves.
//!
//! Planning walks the sources up front. That costs a directory traversal
//! before the copy starts, and it buys the only honest progress bar there is:
//! a job that has not counted its work cannot say what fraction of it is done,
//! and a percentage that recalculates its own denominator as it goes is worse
//! than no percentage.
//!
//! The walk carries a visited set of `(device, inode)` pairs for every
//! directory it enters. That is how a symlink loop terminates: not with a depth
//! cap, which would refuse a legitimately deep tree, but by noticing the walk
//! has arrived somewhere it has already been. The set is maintained even when
//! the policy copies symlinks as links and cannot loop, because a bind mount
//! pointing at an ancestor produces the same cycle without a symlink in sight.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::error::OperationError;
use crate::fsops::FileSnapshot;
use crate::policy::{CopyPolicy, SymlinkPolicy};

/// What one planned item is.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ItemKind {
    /// A regular file: copied, moved, hashed, or deleted.
    File,
    /// A symbolic link, handled as a link rather than as what it points at.
    Symlink,
    /// A directory that has to exist at the destination before its children
    /// are written.
    Directory,
    /// The second visit to a directory, after its children. Where the
    /// directory's real mode and timestamps go back on, and where the source
    /// directory is removed for a move or a delete.
    DirectoryEpilogue,
    /// Something that is neither a file, a directory, nor a symlink: a socket,
    /// a fifo, a device node. Recorded so it is reported as skipped rather
    /// than silently missing from the destination.
    Other,
}

impl ItemKind {
    /// Whether this counts towards the item total the user sees. A directory
    /// epilogue is bookkeeping, not work the user asked for.
    pub fn counts_as_item(self) -> bool {
        !matches!(self, ItemKind::DirectoryEpilogue)
    }
}

/// One unit of work.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlanItem {
    pub kind: ItemKind,
    /// The item being acted on. For a create, the thing being created.
    pub source: PathBuf,
    /// Where it is going, when it is going anywhere.
    pub destination: Option<PathBuf>,
    /// Bytes this item is expected to cost.
    pub bytes: u64,
    /// What the source looked like at plan time, re-checked before anything
    /// destructive happens to it.
    pub snapshot: Option<FileSnapshot>,
}

impl PlanItem {
    pub fn new(kind: ItemKind, source: PathBuf, destination: Option<PathBuf>) -> Self {
        Self {
            kind,
            source,
            destination,
            bytes: 0,
            snapshot: None,
        }
    }
}

/// The whole plan.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Plan {
    pub items: Vec<PlanItem>,
    /// Sources the walk could not read. They are failures, recorded here so
    /// they appear in the job's log rather than shrinking the total silently.
    pub unreadable: Vec<(PathBuf, OperationError)>,
}

impl Plan {
    pub fn total_items(&self) -> u64 {
        self.items
            .iter()
            .filter(|item| item.kind.counts_as_item())
            .count() as u64
            + self.unreadable.len() as u64
    }

    pub fn total_bytes(&self) -> u64 {
        self.items.iter().map(|item| item.bytes).sum()
    }
}

/// Where the directory entry goes relative to its children.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WalkOrder {
    /// Directory first, then children, then the epilogue. What a copy needs:
    /// the destination directory has to exist before anything goes in it.
    Prologue,
    /// Children first, then the directory. What a delete needs: a directory
    /// cannot be removed until it is empty.
    PostOrder,
}

/// Expands one source into its items.
///
/// `destination_root` is where this source lands; `None` for an operation with
/// no destination, such as a delete or a checksum.
pub fn walk_source(
    source: &Path,
    destination_root: Option<&Path>,
    order: WalkOrder,
    policy: &CopyPolicy,
    plan: &mut Plan,
) {
    let mut visited = HashSet::new();
    walk_inner(source, destination_root, order, policy, plan, &mut visited);
}

fn walk_inner(
    source: &Path,
    destination: Option<&Path>,
    order: WalkOrder,
    policy: &CopyPolicy,
    plan: &mut Plan,
    visited: &mut HashSet<(u64, u64)>,
) {
    let metadata = match fs::symlink_metadata(source) {
        Ok(metadata) => metadata,
        Err(error) => {
            plan.unreadable.push((
                source.to_path_buf(),
                OperationError::from_io(source, &error),
            ));
            return;
        }
    };
    let snapshot = FileSnapshot::from_metadata(&metadata);

    // A symlink is an item in its own right unless the policy says to follow
    // it, in which case it is whatever it points at and the visited set has to
    // notice when that is somewhere the walk has been.
    if snapshot.is_symlink && policy.symlinks == SymlinkPolicy::CopyAsLink {
        let mut item = PlanItem::new(
            ItemKind::Symlink,
            source.to_path_buf(),
            destination.map(Path::to_path_buf),
        );
        item.snapshot = Some(snapshot);
        plan.items.push(item);
        return;
    }

    let followed = if snapshot.is_symlink {
        match fs::metadata(source) {
            Ok(metadata) => metadata,
            Err(error) => {
                plan.unreadable.push((
                    source.to_path_buf(),
                    OperationError::from_io(source, &error),
                ));
                return;
            }
        }
    } else {
        metadata
    };

    if followed.is_dir() {
        use std::os::unix::fs::MetadataExt;
        // The loop guard. A directory reached twice is a cycle, whether a
        // symlink or a bind mount made it.
        if !visited.insert((followed.dev(), followed.ino())) {
            plan.unreadable.push((
                source.to_path_buf(),
                OperationError::SymlinkLoop {
                    path: source.to_path_buf(),
                },
            ));
            return;
        }
        let mut prologue = PlanItem::new(
            ItemKind::Directory,
            source.to_path_buf(),
            destination.map(Path::to_path_buf),
        );
        prologue.snapshot = Some(FileSnapshot::from_metadata(&followed));
        if order == WalkOrder::Prologue {
            plan.items.push(prologue);
        }

        match fs::read_dir(source) {
            Ok(entries) => {
                // Sorted by name so a job's order is reproducible: the same
                // copy run twice reports the same item at the same point, which
                // is what makes a benchmark and a failure report comparable.
                let mut children: Vec<PathBuf> =
                    entries.flatten().map(|entry| entry.path()).collect();
                children.sort();
                for child in children {
                    let child_destination = destination.map(|root| {
                        root.join(child.file_name().unwrap_or_else(|| child.as_os_str()))
                    });
                    walk_inner(
                        &child,
                        child_destination.as_deref(),
                        order,
                        policy,
                        plan,
                        visited,
                    );
                }
            }
            Err(error) => {
                plan.unreadable.push((
                    source.to_path_buf(),
                    OperationError::from_io(source, &error),
                ));
            }
        }

        let mut epilogue = PlanItem::new(
            ItemKind::DirectoryEpilogue,
            source.to_path_buf(),
            destination.map(Path::to_path_buf),
        );
        epilogue.snapshot = Some(FileSnapshot::from_metadata(&followed));
        if order == WalkOrder::PostOrder {
            // In post-order there is no prologue, so this entry is the
            // directory itself rather than bookkeeping after it, and it counts
            // towards the item total the user sees.
            let mut only = epilogue;
            only.kind = ItemKind::Directory;
            plan.items.push(only);
        } else {
            plan.items.push(epilogue);
        }
        // The directory is left in the visited set: a second arrival at the
        // same directory through a different path is still a cycle for this
        // walk, and copying it twice would double the destination.
        return;
    }

    let kind = if followed.is_file() {
        ItemKind::File
    } else {
        ItemKind::Other
    };
    let mut item = PlanItem::new(
        kind,
        source.to_path_buf(),
        destination.map(Path::to_path_buf),
    );
    item.bytes = if kind == ItemKind::File {
        followed.len()
    } else {
        0
    };
    item.snapshot = Some(snapshot);
    plan.items.push(item);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn policy() -> CopyPolicy {
        CopyPolicy::default()
    }

    #[test]
    fn a_tree_is_planned_with_directories_before_their_children() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("a/b")).unwrap();
        fs::write(root.path().join("a/one.txt"), b"12345").unwrap();
        fs::write(root.path().join("a/b/two.txt"), b"67").unwrap();

        let mut plan = Plan::default();
        walk_source(
            &root.path().join("a"),
            Some(Path::new("/dst/a")),
            WalkOrder::Prologue,
            &policy(),
            &mut plan,
        );
        let kinds: Vec<ItemKind> = plan.items.iter().map(|item| item.kind).collect();
        assert_eq!(
            kinds,
            vec![
                ItemKind::Directory, // a
                ItemKind::Directory, // a/b
                ItemKind::File,      // a/b/two.txt
                ItemKind::DirectoryEpilogue,
                ItemKind::File, // a/one.txt
                ItemKind::DirectoryEpilogue,
            ]
        );
        assert_eq!(plan.total_bytes(), 7);
        // Two files and two directories; the epilogues are bookkeeping.
        assert_eq!(plan.total_items(), 4);
    }

    #[test]
    fn a_symlink_is_planned_as_a_link_and_its_target_is_not_walked() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("real")).unwrap();
        fs::write(root.path().join("real/big.bin"), vec![0u8; 4096]).unwrap();
        fs::create_dir(root.path().join("tree")).unwrap();
        std::os::unix::fs::symlink(root.path().join("real"), root.path().join("tree/link"))
            .unwrap();

        let mut plan = Plan::default();
        walk_source(
            &root.path().join("tree"),
            Some(Path::new("/dst")),
            WalkOrder::Prologue,
            &policy(),
            &mut plan,
        );
        assert!(plan.items.iter().any(|item| item.kind == ItemKind::Symlink));
        assert_eq!(plan.total_bytes(), 0, "the link's target was not copied");
    }

    #[test]
    fn a_symlink_loop_terminates_and_is_reported_rather_than_hanging() {
        let root = tempfile::tempdir().unwrap();
        let tree = root.path().join("tree");
        fs::create_dir_all(tree.join("inner")).unwrap();
        // inner/back points at tree, so following links walks tree -> inner ->
        // tree -> ... forever without the visited set.
        std::os::unix::fs::symlink(&tree, tree.join("inner/back")).unwrap();

        let mut policy = policy();
        policy.symlinks = SymlinkPolicy::FollowAndCopyTarget;
        let mut plan = Plan::default();
        walk_source(
            &tree,
            Some(Path::new("/dst")),
            WalkOrder::Prologue,
            &policy,
            &mut plan,
        );
        assert!(
            plan.unreadable
                .iter()
                .any(|(_, error)| matches!(error, OperationError::SymlinkLoop { .. })),
            "expected a reported loop, got {:?}",
            plan.unreadable
        );
    }

    #[test]
    fn a_removal_plan_puts_children_before_their_directory() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("a/b")).unwrap();
        fs::write(root.path().join("a/b/one.txt"), b"x").unwrap();

        let mut plan = Plan::default();
        walk_source(
            &root.path().join("a"),
            None,
            WalkOrder::PostOrder,
            &policy(),
            &mut plan,
        );
        let kinds: Vec<ItemKind> = plan.items.iter().map(|item| item.kind).collect();
        assert_eq!(
            kinds,
            vec![
                ItemKind::File,      // a/b/one.txt
                ItemKind::Directory, // a/b
                ItemKind::Directory, // a
            ]
        );
    }

    #[test]
    fn a_source_that_cannot_be_read_is_recorded_not_dropped() {
        let root = tempfile::tempdir().unwrap();
        let mut plan = Plan::default();
        walk_source(
            &root.path().join("absent"),
            Some(Path::new("/dst")),
            WalkOrder::Prologue,
            &policy(),
            &mut plan,
        );
        assert!(plan.items.is_empty());
        assert_eq!(plan.unreadable.len(), 1);
        assert!(matches!(
            plan.unreadable[0].1,
            OperationError::NotFound { .. }
        ));
        // And it still counts, so the total does not quietly shrink.
        assert_eq!(plan.total_items(), 1);
    }

    #[test]
    fn a_name_that_is_not_utf8_is_planned_like_any_other() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("tree");
        fs::create_dir(&directory).unwrap();
        let name = OsStr::from_bytes(b"caf\xe9.txt");
        fs::write(directory.join(name), b"abc").unwrap();

        let mut plan = Plan::default();
        walk_source(
            &directory,
            Some(Path::new("/dst")),
            WalkOrder::Prologue,
            &policy(),
            &mut plan,
        );
        let file = plan
            .items
            .iter()
            .find(|item| item.kind == ItemKind::File)
            .unwrap();
        assert_eq!(file.source.file_name().unwrap(), name);
        assert_eq!(
            file.destination.as_ref().unwrap(),
            &PathBuf::from("/dst").join(name)
        );
    }
}
