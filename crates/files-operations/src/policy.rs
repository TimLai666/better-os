//! The copy correctness policy, written down rather than assumed.
//!
//! Issue #6 requires "a documented policy for timestamps, permissions, ACLs,
//! xattrs, sparse-file behavior, and links". This module is that document, and
//! the tests in `tests/metadata_policy.rs` are the enforcement. What Better
//! Files does, by default:
//!
//! | Property | Default | Why |
//! | --- | --- | --- |
//! | Modification time | preserved to nanosecond resolution | a copied photo keeps its date, which is what every sort by date depends on |
//! | Access time | preserved | it costs nothing to carry, and dropping it makes a backup look freshly read |
//! | Creation time | not carried | Linux exposes `statx(STATX_BTIME)` for reading and has no interface for setting it |
//! | Permission bits | preserved, masked by the destination's mount options | an executable script stays executable |
//! | Ownership | not carried | changing an owner needs `CAP_CHOWN`, and this crate never runs privileged. A copy is owned by whoever made it |
//! | POSIX ACLs | carried only where the filesystem exposes them as `system.posix_acl_*` extended attributes | there is no portable unprivileged interface beyond that, and a silently dropped ACL is reported in the operation log rather than assumed away |
//! | Extended attributes | copied where the destination supports them, per-attribute failures logged and not fatal | a destination filesystem that has no xattrs must not fail a copy |
//! | Symbolic links | copied as links, pointing at the same target text | following them would silently duplicate the target and turn one 4 GB link farm into forty |
//! | Hard links | preserved within one job: the first path to an inode is copied and every later path is linked to that copy, when the destination filesystem allows `link(2)` | a backup tree of hard-linked snapshots must not come out several times its size. A refused link, or a first copy that changed since the job wrote it, is copied instead and logged |
//! | Sparse regions | preserved through `SEEK_HOLE`/`SEEK_DATA` where the filesystem answers, dense copy where it does not | a 100 GB sparse image must not become 100 GB of zeroes on the destination |
//! | Durability | `fsync` on each file then on its parent directory when the destination is removable | the flush that makes an external disk safe to unplug is `storage-service`'s job; this is the file-level half of it |
//!
//! ## Partial copy and move behaviour
//!
//! Every destination file is written to a temporary name in the destination
//! directory (`.<name>.betteros-part-<job>-<n>`) and renamed into place only
//! once its bytes, metadata, and verification are complete. `rename(2)` within
//! a directory is atomic, so an interrupted, cancelled, or failed copy leaves
//! either the previous content or nothing — never a truncated file under the
//! real name. The temporary is removed on every exit path.
//!
//! An archive is written the same way, under a temporary name beside it, and
//! an extraction into a temporary directory renamed to its folder once every
//! entry is in; [`ExtractLimits`] bounds what one extraction may write.
//!
//! A move is a rename when source and destination share a filesystem, and a
//! copy, a verification, then a source delete when they do not. The source is
//! deleted only after the destination verifies, and only after a metadata
//! re-check confirms the source did not change while the copy ran. An
//! interrupted cross-device move therefore leaves the source intact and the
//! partially copied destination absent.

use serde::{Deserialize, Serialize};

/// How symbolic links are treated.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SymlinkPolicy {
    /// Recreate the link with the same target text. The default.
    #[default]
    CopyAsLink,
    /// Copy what the link points at. Only safe with the walker's visited set,
    /// which is why loop detection is unconditional.
    FollowAndCopyTarget,
}

/// How sparse files are handled.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SparsePolicy {
    /// Ask the filesystem where the holes are with `SEEK_HOLE`/`SEEK_DATA` and
    /// reproduce them, falling back to a dense copy when the filesystem does
    /// not answer. The default.
    #[default]
    Auto,
    /// Copy every byte, holes included. Kept because a destination that
    /// reports hole support and then mishandles it is a real failure mode, and
    /// this is the escape hatch.
    Dense,
}

/// When to force data to the platter.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FsyncPolicy {
    /// `fsync` the file then its parent directory. Correct and slow; this is
    /// what a copy to a removable device uses, because the user will pull it.
    PerFileAndDirectory,
    /// Leave durability to the page cache. The default for a copy within the
    /// internal disk, where a power loss loses the whole session anyway.
    #[default]
    Deferred,
}

/// Where the destination lives, as far as durability is concerned.
///
/// This is the typed hook `storage-core` plugs into. A caller that knows the
/// destination sits on a removable device says so, and the copy switches to
/// per-file durability without this crate having to enumerate devices or talk
/// to UDisks2. The device-level flush that follows — the one that turns
/// "written" into "safe to unplug" — stays `storage-service`'s.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DestinationDurability {
    /// Nothing is known; the policy's own default applies.
    #[default]
    Unknown,
    /// An internal disk that will not be removed mid-session.
    Internal,
    /// A removable device. Forces [`FsyncPolicy::PerFileAndDirectory`].
    Removable,
}

/// What to do when an item fails.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailurePolicy {
    /// Record the failure, keep going, finish in [`crate::JobState::Failed`]
    /// with the failed items retryable. The default: one unreadable file in a
    /// 40,000-file copy must not abandon the other 39,999.
    #[default]
    Continue,
    /// Stop at the first failure.
    Stop,
    /// Stop at the first failure and undo what the job created. Only
    /// operations with a safe compensating action honour this; see
    /// [`crate::spec::OperationKind::supports_rollback`].
    StopAndRollback,
}

/// Whether a move may take the rename fast path.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MoveStrategy {
    /// Try `rename(2)` first and fall back to copy-verify-delete on `EXDEV`.
    /// The default, and the only sane one in production.
    #[default]
    RenameWhenPossible,
    /// Always take the copy-verify-delete path.
    ///
    /// This exists so the cross-filesystem move can be tested without a second
    /// filesystem. Mounting one needs privilege the test suite does not have
    /// and must not ask for, and a code path that is only exercised on a
    /// developer's machine with a spare USB stick is a code path that is not
    /// tested at all.
    AlwaysCopyThenDelete,
}

/// The most an extraction writes, in bytes of unpacked content, before it
/// stops: 64 GiB.
///
/// An archive is untrusted input and its compressed size says nothing about
/// what it unpacks to — a few kilobytes of zip can declare terabytes of
/// zeroes. The limit is on what is actually written, counted as it is
/// written, so an entry that understates its own size is stopped too. 64 GiB
/// is larger than any archive a desktop user plausibly extracts through a file
/// manager and smaller than the free space on most disks the extraction could
/// fill.
pub const MAX_EXTRACTED_BYTES: u64 = 64 * 1024 * 1024 * 1024;

/// The most entries an extraction creates before it stops: one million.
///
/// A million-entry archive is a source tree or a package mirror, not a
/// download a person unpacks by hand, and an archive of empty entries costs
/// inodes and directory scans that no byte limit sees.
pub const MAX_EXTRACTED_ENTRIES: u64 = 1_000_000;

/// The largest central directory a zip may declare before it is read: 256 MiB.
///
/// The zip reader holds every directory entry in memory while an archive is
/// open, and it reads the whole directory before a single entry can be
/// counted. A directory entry is 46 bytes plus its name, extra field, and
/// comment, so a million entries — [`MAX_EXTRACTED_ENTRIES`] — with names and
/// fields averaging more than 200 bytes still fit. Fixed rather than carried
/// in [`ExtractLimits`], because it bounds the reader's memory, not what the
/// extraction writes.
pub const MAX_ZIP_CENTRAL_DIRECTORY_BYTES: u64 = 256 * 1024 * 1024;

/// The ceilings one extraction is held to.
///
/// The defaults are [`MAX_EXTRACTED_BYTES`] and [`MAX_EXTRACTED_ENTRIES`].
/// Carried in the policy rather than read from the constants directly so the
/// suite can prove the stop with a small archive instead of writing 64 GiB.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ExtractLimits {
    pub max_bytes: u64,
    pub max_entries: u64,
}

impl Default for ExtractLimits {
    fn default() -> Self {
        Self {
            max_bytes: MAX_EXTRACTED_BYTES,
            max_entries: MAX_EXTRACTED_ENTRIES,
        }
    }
}

/// The whole policy for one job.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CopyPolicy {
    pub preserve_timestamps: bool,
    pub preserve_permissions: bool,
    pub preserve_xattrs: bool,
    pub symlinks: SymlinkPolicy,
    pub sparse: SparsePolicy,
    pub fsync: FsyncPolicy,
    pub durability: DestinationDurability,
    pub on_failure: FailurePolicy,
    pub moves: MoveStrategy,
    /// How much is read and written between two cancellation checks.
    ///
    /// This is the pause and cancel granularity for a single large file: a
    /// 4 GB copy stops within one chunk of the request rather than at the end
    /// of the file. Bigger is faster and less responsive; 1 MiB keeps the stop
    /// under a few milliseconds on any device worth copying to.
    pub chunk_bytes: usize,
    /// Whether the destination is re-read and compared after each item.
    pub verify: bool,
    /// How much an extraction may unpack before it stops.
    pub extract_limits: ExtractLimits,
}

impl Default for CopyPolicy {
    fn default() -> Self {
        Self {
            preserve_timestamps: true,
            preserve_permissions: true,
            preserve_xattrs: true,
            symlinks: SymlinkPolicy::CopyAsLink,
            sparse: SparsePolicy::Auto,
            fsync: FsyncPolicy::Deferred,
            durability: DestinationDurability::Unknown,
            on_failure: FailurePolicy::Continue,
            moves: MoveStrategy::RenameWhenPossible,
            chunk_bytes: 1024 * 1024,
            verify: true,
            extract_limits: ExtractLimits::default(),
        }
    }
}

impl CopyPolicy {
    /// The policy with the durability the destination actually needs.
    ///
    /// A removable destination is forced to per-file `fsync` here rather than
    /// at every call site, so a caller cannot accidentally copy to a USB stick
    /// with the deferred policy.
    pub fn for_destination(mut self, durability: DestinationDurability) -> Self {
        self.durability = durability;
        if durability == DestinationDurability::Removable {
            self.fsync = FsyncPolicy::PerFileAndDirectory;
        }
        self
    }

    /// Whether this job must `fsync` each file and its parent directory.
    pub fn wants_fsync(&self) -> bool {
        self.fsync == FsyncPolicy::PerFileAndDirectory
            || self.durability == DestinationDurability::Removable
    }

    pub fn chunk_bytes(&self) -> usize {
        self.chunk_bytes.clamp(4096, 64 * 1024 * 1024)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_are_the_documented_ones() {
        let policy = CopyPolicy::default();
        assert!(policy.preserve_timestamps);
        assert!(policy.preserve_permissions);
        assert!(policy.preserve_xattrs);
        assert_eq!(policy.symlinks, SymlinkPolicy::CopyAsLink);
        assert_eq!(policy.sparse, SparsePolicy::Auto);
        assert_eq!(policy.moves, MoveStrategy::RenameWhenPossible);
        assert!(policy.verify);
    }

    #[test]
    fn a_removable_destination_cannot_be_copied_to_with_deferred_durability() {
        let policy = CopyPolicy::default().for_destination(DestinationDurability::Removable);
        assert_eq!(policy.fsync, FsyncPolicy::PerFileAndDirectory);
        assert!(policy.wants_fsync());
        let internal = CopyPolicy::default().for_destination(DestinationDurability::Internal);
        assert!(!internal.wants_fsync());
    }

    #[test]
    fn the_chunk_size_is_clamped_so_a_zero_cannot_stall_a_copy() {
        let zero = CopyPolicy {
            chunk_bytes: 0,
            ..CopyPolicy::default()
        };
        assert_eq!(zero.chunk_bytes(), 4096);
        let huge = CopyPolicy {
            chunk_bytes: usize::MAX,
            ..CopyPolicy::default()
        };
        assert_eq!(huge.chunk_bytes(), 64 * 1024 * 1024);
    }
}
