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

use files_platform::trash::{self, TrashDirectory};

use crate::checksum::{Sha256, to_hex};
use crate::conflict::{Conflict, ConflictKind, Resolution, next_available_name};
use crate::error::OperationError;
use crate::fsops::{self, FileSnapshot};
use crate::log::{LogEvent, SkipReason};
use crate::plan::{ItemKind, Plan, PlanItem, WalkOrder, walk_source};
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
                if let Ok(existing) = fs::symlink_metadata(&destination) {
                    if existing.is_dir() {
                        return Ok(ItemOutcome::Failed(OperationError::IsADirectory {
                            path: destination.clone(),
                        }));
                    }
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
    if is_move {
        if let Some(expected) = &item.snapshot {
            if let Err(error) = fsops::ensure_unchanged(&item.source, expected) {
                return Ok(ItemOutcome::Failed(error));
            }
        }
    }

    let verified = if policy.verify {
        match fsops::verify_copy(&item.source, &destination, policy) {
            Ok(()) => true,
            Err(error) => return Ok(ItemOutcome::Failed(error)),
        }
    } else {
        false
    };

    if is_move {
        if let Err(error) = finish_move(&item.source, item.snapshot.as_ref()) {
            return Ok(ItemOutcome::Failed(error));
        }
    }

    Ok(ItemOutcome::Done {
        bytes: report.bytes,
        verified,
    })
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
    match trash::move_to_trash(&home, source) {
        Ok(item) => {
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
        Err(trash::TrashError::NotFound { .. }) => ItemOutcome::Skipped(SkipReason::SourceGone),
        Err(trash::TrashError::CrossDevice { .. }) => {
            // The specification's fallback. A device with no usable
            // `.Trash-$uid` still has to be able to delete something, and the
            // home trash is the answer every desktop uses.
            control.log(Some(source.to_path_buf()), LogEvent::CrossDeviceFallback);
            match copy_into_trash(&home, source, control) {
                Ok(outcome) => outcome,
                Err(error) => ItemOutcome::Failed(error),
            }
        }
        Err(error) => ItemOutcome::Failed(OperationError::TrashUnavailable {
            reason: error.to_string(),
        }),
    }
}

/// The cross-filesystem trash fallback: copy the item into the home trash,
/// then delete the source.
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
