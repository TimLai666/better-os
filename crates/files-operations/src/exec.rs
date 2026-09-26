//! Running a plan.
//!
//! The executor knows nothing about threads, channels, or windows. It walks a
//! list of items, calls into [`crate::fsops`], and reports what happened
//! through [`JobControl`]. Everything that makes a job durable — the worker
//! pool, the pause condition variable, the persisted record — lives in
//! [`crate::engine`] on the other side of that trait.
//!
//! That split is what makes the operations testable without an engine at all:
//! a test implements `JobControl` in twenty lines, drives a copy, and asserts
//! on the exact sequence of decisions the executor made.

use std::ffi::OsStr;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use files_platform::trash::{self, DeviceProbe, HostDevices, SharedTrash, TrashDirectory};

use crate::checksum::{Sha256, to_hex};
use crate::conflict::{Conflict, ConflictKind, Resolution, next_available_name};
use crate::error::OperationError;
use crate::fsops::{self, FileSnapshot};
use crate::log::{LogEvent, SkipReason};
use crate::plan::{InodeKey, ItemKind, Plan, PlanItem, WalkOrder, walk_source};
use crate::policy::{CopyPolicy, MoveStrategy};
use crate::spec::{DeleteTarget, Operation, RenamePattern, TrashItemRef, is_usable_name};

/// The executor's window onto the job that owns it.
pub trait JobControl {
    /// Called between items and between chunks of a large copy.
    ///
    /// The implementation blocks while the job is paused and returns an error
    /// once it is cancelled. Everything else in the executor treats it as a
    /// plain `?`, which is why cancellation lands at a chunk boundary rather
    /// than wherever a flag happened to be checked.
    fn checkpoint(&mut self) -> Result<(), OperationError>;

    /// Bytes done for the item in progress, cumulative.
    fn item_bytes(&mut self, done: u64);

    /// Asks for a decision. Blocks until one arrives.
    fn resolve(&mut self, conflict: Conflict) -> Result<Resolution, OperationError>;

    /// Adds a line to the operation log.
    fn log(&mut self, path: Option<PathBuf>, event: LogEvent);

    /// Records a computed digest.
    fn checksum(&mut self, path: PathBuf, digest: String);

    /// Where this job already copied a file with this inode to, if it has.
    ///
    /// The answer lives as long as the job does — across a retry, not across a
    /// process — so a job only ever links to a destination it wrote itself.
    fn copied_inode(&mut self, key: InodeKey) -> Option<CopiedInode>;

    /// Records that this job copied a file with this inode to a destination.
    fn record_copied_inode(&mut self, key: InodeKey, copied: CopiedInode);
}

/// A destination this job copied a hard-linked file to, and what it looked
/// like right after.
///
/// The snapshot is how a later link proves the destination still holds the
/// first copy's content before linking to it: something may have rewritten
/// or replaced it since, and a link to that would copy somebody else's file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CopiedInode {
    pub destination: PathBuf,
    pub snapshot: FileSnapshot,
}

/// What one item did.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ItemOutcome {
    /// Done, with the bytes it moved.
    Done {
        bytes: u64,
        verified: bool,
    },
    Skipped(SkipReason),
    Failed(OperationError),
}

/// Builds the plan for an operation.
///
/// Recursive operations walk here; everything else produces one item per
/// target. A walk before execution is what gives the job an honest total.
pub fn build_plan(operation: &Operation, policy: &CopyPolicy) -> Plan {
    let mut plan = Plan::default();
    match operation {
        Operation::CreateFile { parent, name } | Operation::CreateFolder { parent, name } => {
            plan.items.push(PlanItem::new(
                ItemKind::File,
                parent.as_path().to_path_buf(),
                Some(parent.as_path().join(name)),
            ));
        }
        Operation::Rename { path, new_name } => {
            let destination = path
                .as_path()
                .parent()
                .unwrap_or(Path::new("/"))
                .join(new_name);
            plan.items.push(PlanItem::new(
                ItemKind::File,
                path.as_path().to_path_buf(),
                Some(destination),
            ));
        }
        Operation::BulkRename { targets, pattern } => {
            for (index, target) in targets.iter().enumerate() {
                let current = target
                    .as_path()
                    .file_name()
                    .unwrap_or_else(|| OsStr::new(""));
                match pattern.apply(current, index as u64) {
                    Some(name) => plan.items.push(PlanItem::new(
                        ItemKind::File,
                        target.as_path().to_path_buf(),
                        Some(
                            target
                                .as_path()
                                .parent()
                                .unwrap_or(Path::new("/"))
                                .join(name),
                        ),
                    )),
                    None => plan.unreadable.push((
                        target.as_path().to_path_buf(),
                        OperationError::InvalidName {
                            name: target.as_path().to_path_buf(),
                        },
                    )),
                }
            }
        }
        Operation::Copy {
            sources,
            destination,
        }
        | Operation::Move {
            sources,
            destination,
        } => {
            for source in sources {
                let name = source
                    .as_path()
                    .file_name()
                    .unwrap_or_else(|| OsStr::new("unnamed"));
                let target = destination.as_path().join(name);
                walk_source(
                    source.as_path(),
                    Some(&target),
                    WalkOrder::Prologue,
                    policy,
                    &mut plan,
                );
            }
            plan.count_linked_bytes_once();
        }
        Operation::Duplicate { sources } => {
            for source in sources {
                let parent = source.as_path().parent().unwrap_or(Path::new("/"));
                let name = source
                    .as_path()
                    .file_name()
                    .unwrap_or_else(|| OsStr::new("unnamed"));
                let target = next_available_name(parent, name);
                walk_source(
                    source.as_path(),
                    Some(&target),
                    WalkOrder::Prologue,
                    policy,
                    &mut plan,
                );
            }
            plan.count_linked_bytes_once();
        }
        Operation::Trash { sources, .. } => {
            // No walk. Trashing moves a whole tree with one `rename`, so the
            // tree's shape is not work this job has to enumerate.
            for source in sources {
                let mut item = PlanItem::new(ItemKind::File, source.as_path().to_path_buf(), None);
                item.snapshot = FileSnapshot::read(source.as_path()).ok();
                plan.items.push(item);
            }
        }
        Operation::RestoreFromTrash { items } => {
            for reference in items {
                plan.items.push(PlanItem::new(
                    ItemKind::File,
                    reference.trash_root.join("files").join(&reference.item),
                    None,
                ));
            }
        }
        Operation::PermanentDelete { targets, .. } => {
            for target in targets {
                match target {
                    DeleteTarget::Path(path) => walk_source(
                        path.as_path(),
                        None,
                        WalkOrder::PostOrder,
                        policy,
                        &mut plan,
                    ),
                    DeleteTarget::TrashItem(reference) => {
                        plan.items.push(PlanItem::new(
                            ItemKind::File,
                            reference.trash_root.join("files").join(&reference.item),
                            None,
                        ));
                    }
                }
            }
        }
        Operation::Checksum { targets, .. } => {
            for target in targets {
                let mut item = PlanItem::new(ItemKind::File, target.as_path().to_path_buf(), None);
                item.bytes = fs::symlink_metadata(target.as_path())
                    .map(|metadata| metadata.len())
                    .unwrap_or(0);
                item.snapshot = FileSnapshot::read(target.as_path()).ok();
                plan.items.push(item);
            }
        }
    }
    plan
}

/// Runs one item.
///
/// Every path through this returns an [`ItemOutcome`] rather than propagating,
/// except cancellation, which is the one thing that ends the job rather than
/// the item.
pub fn execute_item(
    operation: &Operation,
    item: &PlanItem,
    policy: &CopyPolicy,
    control: &mut dyn JobControl,
) -> Result<ItemOutcome, OperationError> {
    control.checkpoint()?;
    match operation {
        Operation::CreateFile { name, .. } => {
            let destination = item.destination.clone().unwrap_or_default();
            Ok(create_file(&destination, name, control))
        }
        Operation::CreateFolder { name, .. } => {
            let destination = item.destination.clone().unwrap_or_default();
            Ok(create_folder(&destination, name, control))
        }
        Operation::Rename { .. } | Operation::BulkRename { .. } => {
            let destination = item.destination.clone().unwrap_or_default();
            Ok(rename_one(&item.source, &destination, control))
        }
        Operation::Copy { .. } | Operation::Duplicate { .. } => {
            transfer(item, policy, false, control)
        }
        Operation::Move { .. } => transfer(item, policy, true, control),
        Operation::Trash { trash_root, .. } => {
            Ok(trash_one(&item.source, trash_root.as_deref(), control))
        }
        Operation::RestoreFromTrash { items } => match items.iter().find(|reference| {
            reference.trash_root.join("files").join(&reference.item) == item.source
        }) {
            Some(reference) => restore_one(reference, control),
            None => Ok(ItemOutcome::Failed(OperationError::TrashUnavailable {
                reason: "no_record".to_string(),
            })),
        },
        Operation::PermanentDelete { targets, .. } => Ok(permanent_delete(item, targets, control)),
        Operation::Checksum { .. } => checksum_one(item, policy, control),
    }
}

// --- Create, rename ------------------------------------------------------

fn create_file(destination: &Path, name: &OsStr, control: &mut dyn JobControl) -> ItemOutcome {
    if !is_usable_name(name) {
        return ItemOutcome::Failed(OperationError::InvalidName {
            name: PathBuf::from(name),
        });
    }
    match fs::File::options()
        .write(true)
        .create_new(true)
        .open(destination)
    {
        Ok(_) => {
            control.log(Some(destination.to_path_buf()), LogEvent::Created);
            ItemOutcome::Done {
                bytes: 0,
                verified: destination.is_file(),
            }
        }
        Err(error) => ItemOutcome::Failed(OperationError::from_io(destination, &error)),
    }
}

fn create_folder(destination: &Path, name: &OsStr, control: &mut dyn JobControl) -> ItemOutcome {
    if !is_usable_name(name) {
        return ItemOutcome::Failed(OperationError::InvalidName {
            name: PathBuf::from(name),
        });
    }
    match fs::create_dir(destination) {
        Ok(()) => {
            control.log(Some(destination.to_path_buf()), LogEvent::Created);
            ItemOutcome::Done {
                bytes: 0,
                verified: destination.is_dir(),
            }
        }
        Err(error) => ItemOutcome::Failed(OperationError::from_io(destination, &error)),
    }
}

fn rename_one(source: &Path, destination: &Path, control: &mut dyn JobControl) -> ItemOutcome {
    if source == destination {
        return ItemOutcome::Skipped(SkipReason::AlreadyDone);
    }
    // `rename(2)` would silently replace the destination. The check is a
    // separate syscall and therefore racy, which is why the job also records
    // what it found: the alternative is a rename that destroys a file the user
    // never saw.
    if fs::symlink_metadata(destination).is_ok() {
        return ItemOutcome::Failed(OperationError::AlreadyExists {
            path: destination.to_path_buf(),
        });
    }
    match fs::rename(source, destination) {
        Ok(()) => {
            control.log(Some(destination.to_path_buf()), LogEvent::RenameFastPath);
            ItemOutcome::Done {
                bytes: 0,
                verified: fs::symlink_metadata(destination).is_ok(),
            }
        }
        Err(error) => ItemOutcome::Failed(OperationError::from_io(source, &error)),
    }
}

// --- Copy, move, duplicate ----------------------------------------------

fn transfer(
    item: &PlanItem,
    policy: &CopyPolicy,
    is_move: bool,
    control: &mut dyn JobControl,
) -> Result<ItemOutcome, OperationError> {
    let Some(destination) = item.destination.clone() else {
        return Ok(ItemOutcome::Failed(OperationError::Io {
            path: item.source.clone(),
            reason: "no_destination".to_string(),
            errno: None,
        }));
    };

    match item.kind {
        ItemKind::Directory => {
            let source_metadata = fs::symlink_metadata(&item.source).ok();
            Ok(
                match fsops::create_directory(&destination, source_metadata.as_ref(), policy) {
                    Ok(()) => {
                        control.log(Some(destination.clone()), LogEvent::Created);
                        ItemOutcome::Done {
                            bytes: 0,
                            verified: destination.is_dir(),
                        }
                    }
                    Err(error) => ItemOutcome::Failed(error),
                },
            )
        }
        ItemKind::DirectoryEpilogue => {
            if let Ok(metadata) = fs::symlink_metadata(&item.source) {
                let _ = fsops::finalize_directory(&destination, &metadata, policy);
            }
            if is_move {
                // The source directory goes only once its children are gone,
                // which the post-order of the plan guarantees.
                let _ = fsops::remove_directory(&item.source);
            }
            Ok(ItemOutcome::Done {
                bytes: 0,
                verified: true,
            })
        }
        ItemKind::Other => Ok(ItemOutcome::Skipped(SkipReason::SourceGone)),
        ItemKind::File | ItemKind::Symlink => {
            transfer_leaf(item, &destination, policy, is_move, control)
        }
    }
}

fn transfer_leaf(
    item: &PlanItem,
    destination: &Path,
    policy: &CopyPolicy,
    is_move: bool,
    control: &mut dyn JobControl,
) -> Result<ItemOutcome, OperationError> {
    let mut destination = destination.to_path_buf();

    // The source may have gone between planning and now.
    if fs::symlink_metadata(&item.source).is_err() {
        return Ok(ItemOutcome::Skipped(SkipReason::SourceGone));
    }

    if let Some(conflict) = detect_conflict(&item.source, &destination) {
        let kind = conflict.kind;
        let resolution = control.resolve(conflict)?;
        match resolution {
            Resolution::Skip => return Ok(ItemOutcome::Skipped(SkipReason::ConflictSkipped)),
            Resolution::Cancel => {
                return Err(OperationError::Cancelled {
                    path: item.source.clone(),
                });
            }
            Resolution::Rename => {
                let parent = destination.parent().unwrap_or(Path::new("/")).to_path_buf();
                let name = destination
                    .file_name()
                    .unwrap_or_else(|| OsStr::new("unnamed"))
                    .to_os_string();
                destination = next_available_name(&parent, &name);
            }
            Resolution::Overwrite => {
                if !kind.accepts_overwrite() {
                    return Ok(ItemOutcome::Failed(OperationError::ConflictUnresolved {
                        path: destination.clone(),
                    }));
                }
                // An existing directory cannot be replaced by a file with a
                // rename, and removing it would delete its contents without
                // being asked. That is a failure, not an overwrite.
                if let Ok(existing) = fs::symlink_metadata(&destination)
                    && existing.is_dir()
                {
                    return Ok(ItemOutcome::Failed(OperationError::IsADirectory {
                        path: destination.clone(),
                    }));
                }
            }
        }
    }

    // A move within one filesystem is a rename, which costs nothing and
    // preserves everything by definition.
    if is_move && policy.moves == MoveStrategy::RenameWhenPossible {
        match fsops::same_filesystem(&item.source, &destination) {
            Ok(true) => {
                if let Some(expected) = &item.snapshot {
                    fsops::ensure_unchanged(&item.source, expected)?;
                }
                return Ok(match fs::rename(&item.source, &destination) {
                    Ok(()) => {
                        control.log(Some(destination.clone()), LogEvent::RenameFastPath);
                        ItemOutcome::Done {
                            bytes: 0,
                            verified: fs::symlink_metadata(&destination).is_ok(),
                        }
                    }
                    Err(error) => {
                        ItemOutcome::Failed(OperationError::from_io(&item.source, &error))
                    }
                });
            }
            Ok(false) => control.log(Some(item.source.clone()), LogEvent::CrossDeviceFallback),
            Err(error) => return Ok(ItemOutcome::Failed(error)),
        }
    } else if is_move {
        control.log(Some(item.source.clone()), LogEvent::CrossDeviceFallback);
    }

    if item.kind == ItemKind::Symlink {
        return Ok(
            match fsops::copy_symlink(&item.source, &destination, policy) {
                Ok(()) => {
                    control.log(Some(destination.clone()), LogEvent::Created);
                    if is_move {
                        match finish_move(&item.source, item.snapshot.as_ref()) {
                            Ok(()) => {}
                            Err(error) => return Ok(ItemOutcome::Failed(error)),
                        }
                    }
                    ItemOutcome::Done {
                        bytes: 0,
                        verified: true,
                    }
                }
                Err(error) => ItemOutcome::Failed(error),
            },
        );
    }

    if let Some(key) = item.hard_link
        && let Some(outcome) = link_to_first_copy(item, key, &destination, policy, is_move, control)
    {
        return Ok(outcome);
    }

    let mut hook = |written: u64| {
        control.item_bytes(written);
        control.checkpoint()
    };
    let report = match fsops::copy_file(&item.source, &destination, policy, &mut hook) {
        Ok(report) => report,
        Err(error) => {
            return match error {
                OperationError::Cancelled { .. } => Err(error),
                other => Ok(ItemOutcome::Failed(other)),
            };
        }
    };
    control.log(Some(destination.clone()), LogEvent::Created);
    if report.holes > 0 {
        control.log(
            Some(destination.clone()),
            LogEvent::SparseRegionsPreserved {
                holes: report.holes,
            },
        );
    }
    for property in &report.metadata_gaps {
        control.log(
            Some(destination.clone()),
            LogEvent::MetadataNotCarried {
                property: *property,
            },
        );
    }

    // The source is re-checked before the destination is compared to it, so a
    // file somebody else rewrote mid-copy is reported as what it is rather
    // than as a copy that came out the wrong size.
    if is_move
        && let Some(expected) = &item.snapshot
        && let Err(error) = fsops::ensure_unchanged(&item.source, expected)
    {
        return Ok(ItemOutcome::Failed(error));
    }

    let verified = if policy.verify {
        match fsops::verify_copy(&item.source, &destination, policy) {
            Ok(()) => true,
            Err(error) => return Ok(ItemOutcome::Failed(error)),
        }
    } else {
        false
    };

    // The copy is the one later paths to the same inode link to. A copy made
    // because a link could not be is recorded too, replacing a first copy that
    // is no longer fit to link to.
    if let Some(key) = item.hard_link
        && let Ok(snapshot) = FileSnapshot::read(&destination)
    {
        control.record_copied_inode(
            key,
            CopiedInode {
                destination: destination.clone(),
                snapshot,
            },
        );
    }

    if is_move && let Err(error) = finish_move(&item.source, item.snapshot.as_ref()) {
        return Ok(ItemOutcome::Failed(error));
    }

    Ok(ItemOutcome::Done {
        bytes: report.bytes,
        verified,
    })
}

/// Creates `destination` as a hard link to the job's earlier copy of the same
/// inode.
///
/// Returns `None` when the file should be copied instead, having logged why:
/// the job has not copied this inode, the first copy has changed or gone since
/// the job wrote it, or `link(2)` refused — another filesystem, one without
/// hard links, a permission. The link is made under a temporary name and
/// renamed into place, the same way a copy is, so an existing destination the
/// user chose to overwrite is replaced atomically.
fn link_to_first_copy(
    item: &PlanItem,
    key: InodeKey,
    destination: &Path,
    policy: &CopyPolicy,
    is_move: bool,
    control: &mut dyn JobControl,
) -> Option<ItemOutcome> {
    let first = control.copied_inode(key)?;
    let not_preserved = |control: &mut dyn JobControl, reason: &str| {
        control.log(
            Some(destination.to_path_buf()),
            LogEvent::HardLinkNotPreserved {
                reason: reason.to_string(),
            },
        );
    };
    match FileSnapshot::read(&first.destination) {
        Ok(current) if current.matches(&first.snapshot) => {}
        Ok(_) => {
            not_preserved(control, "files.operation.hard_link.first_copy_changed");
            return None;
        }
        Err(_) => {
            not_preserved(control, "files.operation.hard_link.first_copy_gone");
            return None;
        }
    }

    let temporary = fsops::temporary_name_for(destination);
    if let Err(error) = fs::hard_link(&first.destination, &temporary) {
        not_preserved(control, OperationError::from_io(&temporary, &error).key());
        return None;
    }
    if let Err(error) = fs::rename(&temporary, destination) {
        let _ = fs::remove_file(&temporary);
        return Some(ItemOutcome::Failed(OperationError::from_io(
            destination,
            &error,
        )));
    }
    // `rename(2)` between two links to one inode succeeds and does nothing, so
    // the temporary can still be there. Its name is this job's own.
    let _ = fs::remove_file(&temporary);
    control.log(Some(destination.to_path_buf()), LogEvent::Created);
    control.log(
        Some(destination.to_path_buf()),
        LogEvent::HardLinked {
            first: first.destination.clone(),
        },
    );

    if is_move
        && let Some(expected) = &item.snapshot
        && let Err(error) = fsops::ensure_unchanged(&item.source, expected)
    {
        return Some(ItemOutcome::Failed(error));
    }
    let verified = if policy.verify {
        match fsops::verify_copy(&item.source, destination, policy) {
            Ok(()) => true,
            Err(error) => return Some(ItemOutcome::Failed(error)),
        }
    } else {
        false
    };
    if is_move && let Err(error) = finish_move(&item.source, item.snapshot.as_ref()) {
        return Some(ItemOutcome::Failed(error));
    }
    Some(ItemOutcome::Done { bytes: 0, verified })
}

/// Deletes a move's source, but only after proving it is still the file the
/// job copied.
fn finish_move(source: &Path, expected: Option<&FileSnapshot>) -> Result<(), OperationError> {
    if let Some(expected) = expected {
        fsops::ensure_unchanged(source, expected)?;
    }
    fsops::remove_file(source)
}

/// Classifies what is at the destination, if anything.
fn detect_conflict(source: &Path, destination: &Path) -> Option<Conflict> {
    let existing = fs::symlink_metadata(destination).ok()?;
    let _ = existing;
    let kind = match actual_name_of(destination) {
        // The destination resolved to an entry spelled differently: the
        // filesystem folded the case. Saying "overwrite" here overwrites a
        // file with a different name, and the user has to be told which.
        Some(actual) if actual != destination.file_name().unwrap_or_default() => {
            ConflictKind::CaseConflict
        }
        _ => ConflictKind::Exists,
    };
    Some(Conflict {
        kind,
        source: Some(source.to_path_buf()),
        destination: destination.to_path_buf(),
        existing: actual_name_of(destination).map(PathBuf::from),
    })
}

/// The name the directory really holds for this path.
///
/// On a case-sensitive filesystem this is always the name that was asked for.
/// On a case-insensitive one it is the spelling that is actually stored, which
/// is the only way to tell a case conflict from a plain overwrite.
fn actual_name_of(path: &Path) -> Option<std::ffi::OsString> {
    let parent = path.parent()?;
    let wanted = path.file_name()?;
    for entry in fs::read_dir(parent).ok()?.flatten() {
        let name = entry.file_name();
        if name == wanted {
            return Some(name);
        }
        if name
            .as_encoded_bytes()
            .eq_ignore_ascii_case(wanted.as_encoded_bytes())
        {
            return Some(name);
        }
    }
    None
}

// --- Trash, restore, delete ---------------------------------------------

fn trash_one(
    source: &Path,
    trash_root: Option<&Path>,
    control: &mut dyn JobControl,
) -> ItemOutcome {
    trash_one_with(
        source,
        trash_root,
        &HostDevices,
        trash::current_uid(),
        control,
    )
}

/// Trashes one item into the trash on its own device.
///
/// `trash_root` is the job's home trash. An item on the same device goes
/// there with a `rename`. An item on another device goes to that device's own
/// trash, so deleting a large file on a USB disk does not copy it onto the
/// home partition. Only when the device has no usable trash — the private
/// directory is not ours, the medium is read-only, no uid is known — is the
/// item copied into the home trash and the source deleted, and the job records
/// why.
///
/// `devices` is the seam that lets a test say which device a path is on.
fn trash_one_with(
    source: &Path,
    trash_root: Option<&Path>,
    devices: &dyn DeviceProbe,
    uid: Option<u32>,
    control: &mut dyn JobControl,
) -> ItemOutcome {
    let home = match trash_root {
        Some(root) => TrashDirectory::new(root),
        None => match TrashDirectory::home_from_env() {
            Some(home) => home,
            None => {
                return ItemOutcome::Failed(OperationError::TrashUnavailable {
                    reason: "no_home_trash".to_string(),
                });
            }
        },
    };
    // An item that cannot be located falls through to the plain move, which
    // reports a missing source as the skip it is.
    if matches!(trash::shares_device(&home, source, devices), Ok(false)) {
        return trash_on_own_device(&home, source, devices, uid, control);
    }
    match trash::move_to_trash(&home, source) {
        Ok(item) => trashed(item, control),
        Err(trash::TrashError::NotFound { .. }) => ItemOutcome::Skipped(SkipReason::SourceGone),
        // The device numbers agreed and the kernel still said `EXDEV`: a bind
        // mount does that. The item's own device decides, as above.
        Err(trash::TrashError::CrossDevice { .. }) => {
            trash_on_own_device(&home, source, devices, uid, control)
        }
        Err(error) => ItemOutcome::Failed(OperationError::TrashUnavailable {
            reason: error.to_string(),
        }),
    }
}

fn trashed(item: trash::TrashedItem, control: &mut dyn JobControl) -> ItemOutcome {
    control.log(
        Some(item.stored_path.clone()),
        LogEvent::Note {
            text: format!("trashed as {}", item.item),
        },
    );
    ItemOutcome::Done {
        bytes: 0,
        verified: item.stored_path.symlink_metadata().is_ok(),
    }
}

/// The per-volume half: the device's own trash, or the home-trash copy when
/// the device has none that may be used.
fn trash_on_own_device(
    home: &TrashDirectory,
    source: &Path,
    devices: &dyn DeviceProbe,
    uid: Option<u32>,
    control: &mut dyn JobControl,
) -> ItemOutcome {
    let volume = match uid {
        Some(uid) => {
            trash::volume_trash_for(source, uid, devices).map_err(|error| error.to_string())
        }
        None => Err("files.trash.error.no_uid".to_string()),
    };
    match volume {
        Ok(volume) => {
            if !matches!(volume.shared, SharedTrash::Usable | SharedTrash::Missing) {
                // The specification asks for a failed `.Trash` check to be
                // reported. The item still goes to the private trash.
                control.log(
                    Some(source.to_path_buf()),
                    LogEvent::DeviceTrashUnavailable {
                        reason: volume.shared.key().to_string(),
                    },
                );
            }
            match trash::move_to_trash(&volume.directory, source) {
                Ok(item) => {
                    control.log(Some(item.stored_path.clone()), LogEvent::TrashedOnDevice);
                    trashed(item, control)
                }
                Err(trash::TrashError::NotFound { .. }) => {
                    ItemOutcome::Skipped(SkipReason::SourceGone)
                }
                Err(error) => home_copy_fallback(home, source, error.to_string(), control),
            }
        }
        Err(reason) => home_copy_fallback(home, source, reason, control),
    }
}

fn home_copy_fallback(
    home: &TrashDirectory,
    source: &Path,
    reason: String,
    control: &mut dyn JobControl,
) -> ItemOutcome {
    control.log(
        Some(source.to_path_buf()),
        LogEvent::DeviceTrashUnavailable { reason },
    );
    control.log(Some(source.to_path_buf()), LogEvent::CrossDeviceFallback);
    match copy_into_trash(home, source, control) {
        Ok(outcome) => outcome,
        Err(error) => ItemOutcome::Failed(error),
    }
}

/// The last resort for an item on a device with no usable trash: copy it into
/// the home trash, then delete the source.
///
/// The order matters. The source goes only after the copy verified, so a
/// failure loses nothing.
fn copy_into_trash(
    home: &TrashDirectory,
    source: &Path,
    control: &mut dyn JobControl,
) -> Result<ItemOutcome, OperationError> {
    let staging = home.root().join("betteros-staging");
    fs::create_dir_all(&staging).map_err(|error| OperationError::from_io(&staging, &error))?;
    let name = source
        .file_name()
        .unwrap_or_else(|| OsStr::new("unnamed"))
        .to_os_string();
    let staged = staging.join(&name);
    let _ = fs::remove_file(&staged);

    let policy = CopyPolicy::default();
    let mut plan = Plan::default();
    walk_source(
        source,
        Some(&staged),
        WalkOrder::Prologue,
        &policy,
        &mut plan,
    );
    for item in &plan.items {
        match transfer(item, &policy, true, control)? {
            ItemOutcome::Failed(error) => {
                let _ = fs::remove_dir_all(&staged);
                return Ok(ItemOutcome::Failed(error));
            }
            _ => continue,
        }
    }
    // The staged copy is now on the trash's own filesystem, so the real
    // trashing is a rename again.
    let result = trash::move_to_trash(home, &staged);
    let _ = fs::remove_dir(&staging);
    match result {
        Ok(item) => Ok(ItemOutcome::Done {
            bytes: 0,
            verified: item.stored_path.symlink_metadata().is_ok(),
        }),
        Err(error) => Ok(ItemOutcome::Failed(OperationError::TrashUnavailable {
            reason: error.to_string(),
        })),
    }
}

fn restore_one(
    reference: &TrashItemRef,
    control: &mut dyn JobControl,
) -> Result<ItemOutcome, OperationError> {
    let directory = TrashDirectory::new(&reference.trash_root);
    let original = match trash::original_path_of(&directory, &reference.item) {
        Ok(path) => path,
        Err(error) => {
            return Ok(ItemOutcome::Failed(OperationError::TrashUnavailable {
                reason: error.to_string(),
            }));
        }
    };
    let mut destination = original.clone();
    if fs::symlink_metadata(&destination).is_ok() {
        let conflict = Conflict::exists(None, destination.clone());
        match control.resolve(conflict)? {
            Resolution::Skip => return Ok(ItemOutcome::Skipped(SkipReason::ConflictSkipped)),
            Resolution::Cancel => {
                return Err(OperationError::Cancelled { path: destination });
            }
            Resolution::Rename => {
                let parent = destination.parent().unwrap_or(Path::new("/")).to_path_buf();
                let name = destination
                    .file_name()
                    .unwrap_or_else(|| OsStr::new("unnamed"))
                    .to_os_string();
                destination = next_available_name(&parent, &name);
            }
            Resolution::Overwrite => {
                if let Err(error) = fs::remove_file(&destination) {
                    return Ok(ItemOutcome::Failed(OperationError::from_io(
                        &destination,
                        &error,
                    )));
                }
            }
        }
    }
    Ok(
        match trash::restore_to(&directory, &reference.item, &destination) {
            Ok(path) => {
                control.log(
                    Some(path.clone()),
                    LogEvent::Note {
                        text: "restored".to_string(),
                    },
                );
                ItemOutcome::Done {
                    bytes: 0,
                    verified: path.symlink_metadata().is_ok(),
                }
            }
            Err(error) => ItemOutcome::Failed(OperationError::TrashUnavailable {
                reason: error.to_string(),
            }),
        },
    )
}

fn permanent_delete(
    item: &PlanItem,
    targets: &[DeleteTarget],
    control: &mut dyn JobControl,
) -> ItemOutcome {
    // A trash item is deleted through the trash so its record goes with it.
    if let Some(reference) = targets.iter().find_map(|target| match target {
        DeleteTarget::TrashItem(reference)
            if reference.trash_root.join("files").join(&reference.item) == item.source =>
        {
            Some(reference)
        }
        _ => None,
    }) {
        let directory = TrashDirectory::new(&reference.trash_root);
        return match trash::purge(&directory, &reference.item) {
            Ok(()) => ItemOutcome::Done {
                bytes: 0,
                verified: !item.source.exists(),
            },
            Err(error) => ItemOutcome::Failed(OperationError::TrashUnavailable {
                reason: error.to_string(),
            }),
        };
    }

    let result = match item.kind {
        ItemKind::Directory | ItemKind::DirectoryEpilogue => fsops::remove_directory(&item.source),
        _ => fsops::remove_file(&item.source),
    };
    match result {
        Ok(()) => {
            control.log(
                Some(item.source.clone()),
                LogEvent::Note {
                    text: "deleted".to_string(),
                },
            );
            ItemOutcome::Done {
                bytes: 0,
                verified: fs::symlink_metadata(&item.source).is_err(),
            }
        }
        Err(OperationError::NotFound { .. }) => ItemOutcome::Skipped(SkipReason::SourceGone),
        Err(error) => ItemOutcome::Failed(error),
    }
}

// --- Checksum ------------------------------------------------------------

fn checksum_one(
    item: &PlanItem,
    policy: &CopyPolicy,
    control: &mut dyn JobControl,
) -> Result<ItemOutcome, OperationError> {
    let mut file = match fs::File::open(&item.source) {
        Ok(file) => file,
        Err(error) => {
            return Ok(ItemOutcome::Failed(OperationError::from_io(
                &item.source,
                &error,
            )));
        }
    };
    let mut digest = Sha256::new();
    let mut buffer = vec![0u8; policy.chunk_bytes()];
    let mut read_total = 0u64;
    loop {
        let read = match file.read(&mut buffer) {
            Ok(read) => read,
            Err(error) => {
                return Ok(ItemOutcome::Failed(OperationError::from_io(
                    &item.source,
                    &error,
                )));
            }
        };
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
        read_total += read as u64;
        control.item_bytes(read_total);
        control.checkpoint()?;
    }
    let hex = to_hex(&digest.finish());
    control.checksum(item.source.clone(), hex);
    Ok(ItemOutcome::Done {
        bytes: read_total,
        verified: true,
    })
}

/// Undoes what a job created, newest first.
///
/// Only ever removes paths the job's own log recorded as created, so a
/// rollback cannot remove something that was already there.
pub fn rollback(created: &[PathBuf], control: &mut dyn JobControl) {
    for path in created.iter().rev() {
        let removed = match fs::symlink_metadata(path) {
            Ok(metadata) if metadata.is_dir() => fs::remove_dir(path).is_ok(),
            Ok(_) => fs::remove_file(path).is_ok(),
            Err(_) => false,
        };
        if removed {
            control.log(Some(path.clone()), LogEvent::RollbackRemoved);
        }
    }
}

/// A bulk rename's plan, exposed so a caller can preview the new names before
/// submitting the job.
pub fn preview_bulk_rename(
    targets: &[files_core::location::LocalPath],
    pattern: &RenamePattern,
) -> Vec<(PathBuf, Option<PathBuf>)> {
    targets
        .iter()
        .enumerate()
        .map(|(index, target)| {
            let current = target
                .as_path()
                .file_name()
                .unwrap_or_else(|| OsStr::new(""));
            let renamed = pattern.apply(current, index as u64).map(|name| {
                target
                    .as_path()
                    .parent()
                    .unwrap_or(Path::new("/"))
                    .join(name)
            });
            (target.as_path().to_path_buf(), renamed)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// A job control that answers everything and remembers the log.
    #[derive(Default)]
    struct RecordingControl {
        log: Vec<(Option<PathBuf>, LogEvent)>,
        copied: HashMap<InodeKey, CopiedInode>,
    }

    impl JobControl for RecordingControl {
        fn checkpoint(&mut self) -> Result<(), OperationError> {
            Ok(())
        }
        fn item_bytes(&mut self, _done: u64) {}
        fn resolve(&mut self, _conflict: Conflict) -> Result<Resolution, OperationError> {
            Ok(Resolution::Skip)
        }
        fn log(&mut self, path: Option<PathBuf>, event: LogEvent) {
            self.log.push((path, event));
        }
        fn checksum(&mut self, _path: PathBuf, _digest: String) {}
        fn copied_inode(&mut self, key: InodeKey) -> Option<CopiedInode> {
            self.copied.get(&key).cloned()
        }
        fn record_copied_inode(&mut self, key: InodeKey, copied: CopiedInode) {
            self.copied.insert(key, copied);
        }
    }

    /// Devices by longest matching prefix: the mount table the suite cannot
    /// build for real.
    struct FakeDevices(HashMap<PathBuf, u64>);

    impl FakeDevices {
        fn new(entries: &[(&Path, u64)]) -> Self {
            Self(
                entries
                    .iter()
                    .map(|(path, device)| (fs::canonicalize(path).unwrap(), *device))
                    .collect(),
            )
        }
    }

    impl DeviceProbe for FakeDevices {
        fn device_of(&self, path: &Path) -> std::io::Result<u64> {
            self.0
                .iter()
                .filter(|(prefix, _)| path.starts_with(prefix))
                .max_by_key(|(prefix, _)| prefix.as_os_str().len())
                .map(|(_, device)| *device)
                .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound))
        }
    }

    struct Layout {
        _root: tempfile::TempDir,
        home_trash: PathBuf,
        stick: PathBuf,
        file: PathBuf,
        devices: FakeDevices,
    }

    /// A home on device 1 and a "stick" on device 2, both really inside one
    /// temporary directory, so every rename the executor makes succeeds while
    /// the device numbers say what a real second disk would.
    fn layout() -> Layout {
        let root = tempfile::tempdir().unwrap();
        let home_trash = root.path().join("home/.local/share/Trash");
        fs::create_dir_all(root.path().join("home")).unwrap();
        let stick = root.path().join("stick");
        fs::create_dir_all(stick.join("photos")).unwrap();
        let file = stick.join("photos/beach.jpg");
        fs::write(&file, b"jpeg").unwrap();
        let devices = FakeDevices::new(&[(root.path(), 1), (&stick, 2)]);
        Layout {
            _root: root,
            home_trash,
            stick: fs::canonicalize(stick).unwrap(),
            file,
            devices,
        }
    }

    fn uid() -> Option<u32> {
        trash::current_uid()
    }

    #[test]
    fn a_link_the_kernel_refuses_is_copied_instead_and_the_job_says_why() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("a.bin");
        fs::write(&source, b"payload").unwrap();
        let destination = root.path().join("out.bin");
        let key = InodeKey {
            device: 1,
            inode: 1,
        };
        // A "first copy" on another filesystem. `/proc` is never the device a
        // temporary directory is on, so `link(2)` answers `EXDEV` for real.
        let foreign = Path::new("/proc/self/status");
        let mut control = RecordingControl::default();
        control.copied.insert(
            key,
            CopiedInode {
                destination: foreign.to_path_buf(),
                snapshot: FileSnapshot::read(foreign).unwrap(),
            },
        );
        let mut item = PlanItem::new(ItemKind::File, source.clone(), Some(destination.clone()));
        item.bytes = 7;
        item.snapshot = FileSnapshot::read(&source).ok();
        item.hard_link = Some(key);

        let outcome = transfer(&item, &CopyPolicy::default(), false, &mut control).unwrap();
        assert!(matches!(
            outcome,
            ItemOutcome::Done {
                bytes: 7,
                verified: true
            }
        ));
        assert_eq!(fs::read(&destination).unwrap(), b"payload");
        assert!(control.log.iter().any(|(_, event)| matches!(
            event,
            LogEvent::HardLinkNotPreserved { reason } if reason == "files.operation.error.cross_device"
        )));
        // The copy is what the next path to this inode links to.
        assert_eq!(control.copied[&key].destination, destination);
    }

    #[test]
    fn an_item_on_another_device_goes_to_that_devices_own_trash() {
        let layout = layout();
        let uid = uid().unwrap();
        let mut control = RecordingControl::default();
        let outcome = trash_one_with(
            &layout.file,
            Some(&layout.home_trash),
            &layout.devices,
            Some(uid),
            &mut control,
        );
        assert!(matches!(outcome, ItemOutcome::Done { verified: true, .. }));
        assert!(!layout.file.exists());
        let volume = layout.stick.join(format!(".Trash-{uid}"));
        assert_eq!(fs::read(volume.join("files/beach.jpg")).unwrap(), b"jpeg");
        let record = fs::read_to_string(volume.join("info/beach.jpg.trashinfo")).unwrap();
        assert!(record.contains("\nPath=photos/beach.jpg\n"), "{record}");
        assert!(
            !layout.home_trash.join("files").exists(),
            "nothing reached the home trash"
        );
        assert!(
            control
                .log
                .iter()
                .any(|(_, event)| matches!(event, LogEvent::TrashedOnDevice)),
            "{:?}",
            control.log
        );
        assert!(
            !control
                .log
                .iter()
                .any(|(_, event)| matches!(event, LogEvent::CrossDeviceFallback))
        );
    }

    #[test]
    fn a_shared_trash_that_fails_its_checks_is_reported_and_the_private_one_used() {
        let layout = layout();
        let uid = uid().unwrap();
        let shared = layout.stick.join(".Trash");
        fs::create_dir(&shared).unwrap();
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&shared, fs::Permissions::from_mode(0o777)).unwrap();

        let mut control = RecordingControl::default();
        let outcome = trash_one_with(
            &layout.file,
            Some(&layout.home_trash),
            &layout.devices,
            Some(uid),
            &mut control,
        );
        assert!(matches!(outcome, ItemOutcome::Done { .. }));
        assert!(
            layout
                .stick
                .join(format!(".Trash-{uid}/files/beach.jpg"))
                .exists()
        );
        assert!(control.log.iter().any(|(_, event)| matches!(
            event,
            LogEvent::DeviceTrashUnavailable { reason } if reason == "files.trash.shared.not_sticky"
        )));
    }

    #[test]
    fn an_item_whose_device_has_no_usable_trash_is_copied_to_the_home_trash() {
        let layout = layout();
        let uid = uid().unwrap();
        // A file where the private trash would go: unusable, and not ours to
        // replace.
        fs::write(layout.stick.join(format!(".Trash-{uid}")), b"in the way").unwrap();

        let mut control = RecordingControl::default();
        let outcome = trash_one_with(
            &layout.file,
            Some(&layout.home_trash),
            &layout.devices,
            Some(uid),
            &mut control,
        );
        assert!(matches!(outcome, ItemOutcome::Done { .. }));
        assert!(!layout.file.exists());
        assert_eq!(
            fs::read(layout.home_trash.join("files/beach.jpg")).unwrap(),
            b"jpeg"
        );
        let unavailable = control.log.iter().find_map(|(_, event)| match event {
            LogEvent::DeviceTrashUnavailable { reason } => Some(reason.clone()),
            _ => None,
        });
        assert!(
            unavailable.is_some_and(|reason| reason.starts_with("files.trash.error.unusable")),
            "{:?}",
            control.log
        );
        assert!(
            control
                .log
                .iter()
                .any(|(_, event)| matches!(event, LogEvent::CrossDeviceFallback))
        );
    }

    #[test]
    fn an_item_on_the_home_device_still_goes_to_the_home_trash() {
        let layout = layout();
        let uid = uid().unwrap();
        let file = layout.home_trash.parent().unwrap().join("notes.txt");
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, b"n").unwrap();
        let mut control = RecordingControl::default();
        let outcome = trash_one_with(
            &file,
            Some(&layout.home_trash),
            &layout.devices,
            Some(uid),
            &mut control,
        );
        assert!(matches!(outcome, ItemOutcome::Done { .. }));
        assert!(layout.home_trash.join("files/notes.txt").exists());
        assert!(!layout.stick.join(format!(".Trash-{uid}")).exists());
    }
}
