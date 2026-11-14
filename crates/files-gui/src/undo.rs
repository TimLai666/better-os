//! Undo and redo of the eight kinds of change folio undoes: rename, move,
//! copy, duplicate, new folder (and new file), compress, extract, and delete
//! (move to the trash).
//!
//! Every step is a move: an item renamed or moved is moved back; something a
//! change made (a copy, an archive, a new folder) goes to the trash, never
//! away, and redo brings it back; something a change sent to the trash comes
//! back out. What a Replace sent to the trash comes back too.
//!
//! Before a step runs, the item is checked to be the one the change left
//! there: the same drive and inode. Something renamed since, or a different
//! file put in its place, stops the undo with "Can't undo that: … isn't
//! where it was left." rather than moving somebody else's file. Every step of
//! a change is checked before any runs, so an undo that cannot finish does
//! not start.
//!
//! Changes are built from what the job itself recorded (its log), so a step
//! the job did not do is never undone; for a job whose log overflowed, the
//! dropped part is simply not undoable.

use std::collections::BTreeSet;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use files_operations::{JobSnapshot, JobState, LogEvent, Operation};
use files_platform::trash::{self, TrashDirectory};

use crate::i18n::Copy;

/// The kind of change, which is what the undo command is named after.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChangeKind {
    Rename,
    Move,
    Copy,
    Duplicate,
    NewFolder,
    NewFile,
    Compress,
    Extract,
    Trash,
}

/// Which file a step is about, so undo never moves another one put there
/// since.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Identity {
    pub device: u64,
    pub inode: u64,
}

impl Identity {
    pub fn of(path: &Path) -> Option<Identity> {
        fs::symlink_metadata(path).ok().map(|meta| Identity {
            device: meta.dev(),
            inode: meta.ino(),
        })
    }
}

/// One step, described as it stands after the change was made.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Step {
    /// The item is at `now` and was at `was`.
    Moved {
        now: PathBuf,
        was: PathBuf,
        id: Option<Identity>,
    },
    /// The change made the item at `path`. Undone, it is in the trash at
    /// `stored`.
    Made {
        path: PathBuf,
        id: Option<Identity>,
        stored: Option<PathBuf>,
    },
    /// The change sent the item at `original` to the trash, where it is at
    /// `stored`.
    Trashed { original: PathBuf, stored: PathBuf },
    /// A move into a folder that was already there (Merge) made the folder
    /// `now` for the folder `was`, item by item. Undone, `was` is there
    /// again and `now` goes once it is empty.
    Folder { now: PathBuf, was: PathBuf },
}

/// One change: its kind, what to call it, and its steps in the order they
/// were made.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Change {
    pub kind: ChangeKind,
    /// The item's name, or empty when there were several.
    pub name: String,
    pub count: usize,
    pub steps: Vec<Step>,
}

/// Why an undo or redo did not run.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Refusal {
    Nothing,
    /// The item is not where the change left it, or another one is there.
    NotWhereLeft {
        name: String,
    },
    /// Something else is already where the item would go back to.
    Occupied {
        name: String,
        folder: String,
    },
    /// The move itself failed.
    Failed {
        name: String,
        reason: String,
    },
}

impl Refusal {
    pub fn message(&self, undoing: bool, c: &'static Copy) -> String {
        let (gone, taken) = if undoing {
            (c.undo_not_where_left, c.undo_occupied)
        } else {
            (c.redo_not_where_left, c.redo_occupied)
        };
        match self {
            Refusal::Nothing => {
                if undoing {
                    c.nothing_to_undo.to_string()
                } else {
                    c.nothing_to_redo.to_string()
                }
            }
            Refusal::NotWhereLeft { name } => gone.replace("{name}", name),
            Refusal::Occupied { name, folder } => {
                taken.replace("{name}", name).replace("{folder}", folder)
            }
            Refusal::Failed { name, reason } => {
                format!("{} ({reason})", gone.replace("{name}", name))
            }
        }
    }
}

/// The undo and redo stacks.
#[derive(Debug, Default)]
pub struct History {
    undo: Vec<Change>,
    redo: Vec<Change>,
    /// The home trash that what undo removes goes to; `None` is the
    /// session's own.
    trash: Option<PathBuf>,
}

/// How many changes are kept.
pub const LIMIT: usize = 100;

impl History {
    pub fn with_trash(trash: Option<PathBuf>) -> Self {
        Self {
            trash,
            ..Self::default()
        }
    }

    /// Remembers a change, forgetting what was undone before it.
    pub fn record(&mut self, change: Change) {
        if change.steps.is_empty() {
            return;
        }
        self.redo.clear();
        self.undo.push(change);
        if self.undo.len() > LIMIT {
            self.undo.remove(0);
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// What the next undo is called: "Undo Rename".
    pub fn undo_name(&self, c: &'static Copy) -> Option<String> {
        self.undo
            .last()
            .map(|change| c.undo_named.replace("{kind}", kind_label(change.kind, c)))
    }

    pub fn redo_name(&self, c: &'static Copy) -> Option<String> {
        self.redo
            .last()
            .map(|change| c.redo_named.replace("{kind}", kind_label(change.kind, c)))
    }

    /// Undoes the latest change. On success returns the change's kind.
    ///
    /// A change that cannot be undone (its item was moved or replaced
    /// since) is said so and forgotten, so the next Undo reaches the change
    /// before it rather than refusing the same way forever.
    ///
    /// One that stops part-way (a move fails after others ran) is kept as it
    /// stands: what ran can be redone, and what did not is still there to
    /// undo once the cause is gone.
    pub fn undo(&mut self) -> Result<ChangeKind, Refusal> {
        let change = self.undo.pop().ok_or(Refusal::Nothing)?;
        match flip(&change, true, self.trash.as_deref()) {
            Ok(flipped) => {
                let kind = flipped.kind;
                self.redo.push(flipped);
                Ok(kind)
            }
            Err(stopped) => {
                self.redo.extend(stopped.ran);
                self.undo.extend(stopped.remaining);
                Err(stopped.refusal)
            }
        }
    }

    /// Redoes the latest undone change; one that cannot be redone is
    /// forgotten, as for undo.
    pub fn redo(&mut self) -> Result<ChangeKind, Refusal> {
        let change = self.redo.pop().ok_or(Refusal::Nothing)?;
        let flipped = match flip(&change, false, self.trash.as_deref()) {
            Ok(flipped) => flipped,
            Err(stopped) => {
                self.undo.extend(stopped.ran);
                self.redo.extend(stopped.remaining);
                return Err(stopped.refusal);
            }
        };
        let kind = flipped.kind;
        self.undo.push(flipped);
        Ok(kind)
    }
}

pub fn kind_label(kind: ChangeKind, c: &'static Copy) -> &'static str {
    match kind {
        ChangeKind::Rename => c.rename,
        ChangeKind::Move => c.change_move,
        ChangeKind::Copy => c.copy_items,
        ChangeKind::Duplicate => c.duplicate,
        ChangeKind::NewFolder => c.new_folder,
        ChangeKind::NewFile => c.new_file,
        ChangeKind::Compress => c.compress_plain,
        ChangeKind::Extract => c.extract,
        ChangeKind::Trash => c.change_delete,
    }
}

fn name_of(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

fn folder_of(path: &Path) -> String {
    path.parent()
        .map(|parent| parent.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Whether `path` holds the item `id` names (any item when `id` is unknown).
fn is_there(path: &Path, id: Option<Identity>) -> bool {
    match (Identity::of(path), id) {
        (Some(now), Some(id)) => now == id,
        (Some(_), None) => true,
        (None, _) => false,
    }
}

fn occupied(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok()
}

/// Checks every step, then runs them backwards (undo) or forwards (redo),
/// returning the change as it stands afterwards.
/// Checks every step of a change before any runs.
fn check(change: &Change, undoing: bool) -> Result<(), Refusal> {
    // Check first, all of it. A path an earlier-running step of this same
    // change empties is not "occupied": undoing a Replace puts the new file
    // away before the old one comes back.
    let vacated: BTreeSet<PathBuf> = change
        .steps
        .iter()
        .filter_map(|step| match (step, undoing) {
            (Step::Made { path, .. }, true) => Some(path.clone()),
            (Step::Moved { now, .. }, true) => Some(now.clone()),
            (Step::Trashed { original, .. }, false) => Some(original.clone()),
            (Step::Moved { was, .. }, false) => Some(was.clone()),
            _ => None,
        })
        .collect();
    // Whether something is in the way where an item is going.
    let blocked = |path: &Path| occupied(path) && !vacated.contains(path);
    for step in &change.steps {
        match (step, undoing) {
            (Step::Moved { now, was, id }, true) => {
                if !is_there(now, *id) {
                    return Err(Refusal::NotWhereLeft { name: name_of(now) });
                }
                if blocked(was) {
                    return Err(Refusal::Occupied {
                        name: name_of(was),
                        folder: folder_of(was),
                    });
                }
            }
            (Step::Moved { now, was, id }, false) => {
                if !is_there(was, *id) {
                    return Err(Refusal::NotWhereLeft { name: name_of(was) });
                }
                if blocked(now) {
                    return Err(Refusal::Occupied {
                        name: name_of(now),
                        folder: folder_of(now),
                    });
                }
            }
            (Step::Made { path, id, .. }, true) => {
                if !is_there(path, *id) {
                    return Err(Refusal::NotWhereLeft {
                        name: name_of(path),
                    });
                }
            }
            (Step::Made { path, stored, .. }, false) => {
                let Some(stored) = stored else {
                    return Err(Refusal::NotWhereLeft {
                        name: name_of(path),
                    });
                };
                if !occupied(stored) {
                    return Err(Refusal::NotWhereLeft {
                        name: name_of(path),
                    });
                }
                if blocked(path) {
                    return Err(Refusal::Occupied {
                        name: name_of(path),
                        folder: folder_of(path),
                    });
                }
            }
            (Step::Trashed { original, stored }, true) => {
                if !occupied(stored) {
                    return Err(Refusal::NotWhereLeft {
                        name: name_of(original),
                    });
                }
                if blocked(original) {
                    return Err(Refusal::Occupied {
                        name: name_of(original),
                        folder: folder_of(original),
                    });
                }
            }
            (Step::Trashed { original, .. }, false) => {
                if !occupied(original) {
                    return Err(Refusal::NotWhereLeft {
                        name: name_of(original),
                    });
                }
            }
            // A folder made or emptied by the items around it: nothing of
            // its own to check.
            (Step::Folder { .. }, _) => {}
        }
    }

    Ok(())
}

/// Why a flip did not finish, and, when it stopped after some steps ran,
/// the change split there: the steps that ran (flipped) and the ones that
/// did not (as they were).
struct Stopped {
    refusal: Refusal,
    ran: Option<Change>,
    remaining: Option<Change>,
}

impl From<Refusal> for Box<Stopped> {
    fn from(refusal: Refusal) -> Self {
        Box::new(Stopped {
            refusal,
            ran: None,
            remaining: None,
        })
    }
}

fn flip(change: &Change, undoing: bool, trash: Option<&Path>) -> Result<Change, Box<Stopped>> {
    check(change, undoing)?;

    // Then run. Undo goes last step first, so a folder made by the change is
    // emptied of what came back before it is itself put away.
    let mut steps = change.steps.clone();
    let order: Vec<usize> = if undoing {
        (0..steps.len()).rev().collect()
    } else {
        (0..steps.len()).collect()
    };
    let mut ran: Vec<usize> = Vec::new();
    for index in order {
        match run_step(&steps[index], undoing, trash) {
            Ok(next) => {
                steps[index] = next;
                ran.push(index);
            }
            Err(refusal) => {
                let part = |indices: Vec<usize>, from: &[Step]| {
                    let mut indices = indices;
                    indices.sort_unstable();
                    let steps: Vec<Step> = indices.into_iter().map(|at| from[at].clone()).collect();
                    (!steps.is_empty()).then(|| Change {
                        count: steps.len(),
                        steps,
                        ..change.clone()
                    })
                };
                let remaining: Vec<usize> =
                    (0..steps.len()).filter(|at| !ran.contains(at)).collect();
                return Err(Box::new(Stopped {
                    refusal,
                    ran: part(ran, &steps),
                    remaining: part(remaining, &change.steps),
                }));
            }
        }
    }
    Ok(Change {
        steps,
        ..change.clone()
    })
}

fn failed(path: &Path, reason: impl ToString) -> Refusal {
    Refusal::Failed {
        name: name_of(path),
        reason: reason.to_string(),
    }
}

/// Makes the folder an item is going back into, when the change took it
/// away (a merged move removes each source folder once it is empty).
fn ensure_parent(path: &Path) -> Result<(), Refusal> {
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => {
            fs::create_dir_all(parent).map_err(|error| failed(path, error))
        }
        _ => Ok(()),
    }
}

fn run_step(step: &Step, undoing: bool, trash: Option<&Path>) -> Result<Step, Refusal> {
    Ok(match (step, undoing) {
        (Step::Folder { now, was }, true) => {
            fs::create_dir_all(was).map_err(|error| failed(was, error))?;
            // Only once empty: something put in it since stays, and so does
            // the folder.
            let _ = fs::remove_dir(now);
            step.clone()
        }
        (Step::Folder { now, .. }, false) => {
            fs::create_dir_all(now).map_err(|error| failed(now, error))?;
            step.clone()
        }
        (Step::Moved { now, was, id }, true) => {
            ensure_parent(was)?;
            fs::rename(now, was).map_err(|error| failed(now, error))?;
            Step::Moved {
                now: now.clone(),
                was: was.clone(),
                id: *id,
            }
        }
        (Step::Moved { now, was, id }, false) => {
            ensure_parent(now)?;
            fs::rename(was, now).map_err(|error| failed(was, error))?;
            Step::Moved {
                now: now.clone(),
                was: was.clone(),
                id: *id,
            }
        }
        (Step::Made { path, id, .. }, true) => {
            let stored = send_to_trash(path, trash)?;
            Step::Made {
                path: path.clone(),
                id: *id,
                stored: Some(stored),
            }
        }
        (Step::Made { path, id, stored }, false) => {
            let stored = stored.clone().ok_or_else(|| failed(path, "no_record"))?;
            take_out_of_trash(&stored, path)?;
            Step::Made {
                path: path.clone(),
                id: *id,
                stored: None,
            }
        }
        (Step::Trashed { original, stored }, true) => {
            take_out_of_trash(stored, original)?;
            Step::Trashed {
                original: original.clone(),
                stored: stored.clone(),
            }
        }
        (Step::Trashed { original, .. }, false) => {
            let stored = send_to_trash(original, trash)?;
            Step::Trashed {
                original: original.clone(),
                stored,
            }
        }
    })
}

/// Sends `path` to the trash on its own device (the home trash when it is
/// on the home's), returning where it is stored.
fn send_to_trash(path: &Path, home: Option<&Path>) -> Result<PathBuf, Refusal> {
    let home = match home {
        Some(root) => TrashDirectory::new(root),
        None => TrashDirectory::home_from_env().ok_or_else(|| failed(path, "no_home_trash"))?,
    };
    match trash::move_to_trash(&home, path) {
        Ok(item) => Ok(item.stored_path),
        Err(trash::TrashError::CrossDevice { .. }) => {
            let uid = trash::current_uid().ok_or_else(|| failed(path, "no_uid"))?;
            let volume = trash::volume_trash_for(path, uid, &trash::HostDevices)
                .map_err(|error| failed(path, error))?;
            trash::move_to_trash(&volume.directory, path)
                .map(|item| item.stored_path)
                .map_err(|error| failed(path, error))
        }
        Err(error) => Err(failed(path, error)),
    }
}

/// Puts a trashed item back at `destination`, removing its trash record.
fn take_out_of_trash(stored: &Path, destination: &Path) -> Result<(), Refusal> {
    let item = stored
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .ok_or_else(|| failed(destination, "no_record"))?;
    let root = stored
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| failed(destination, "no_record"))?;
    trash::restore_to(&TrashDirectory::new(root), &item, destination)
        .map(|_| ())
        .map_err(|error| failed(destination, error))
}

// --- Building a change from a finished job ------------------------------

/// The change a finished job made, or `None` for a job that made none or is
/// not one undo covers (a permanent delete is not undoable; a restore from
/// the trash is undone by deleting again, which folio does not offer either).
pub fn change_from_job(operation: &Operation, snapshot: &JobSnapshot) -> Option<Change> {
    if !matches!(snapshot.state, JobState::Completed | JobState::Failed) {
        return None;
    }
    let records = snapshot.log.records();
    // Each item's source, from the record that started it, paired with what
    // it became.
    let mut pairs: Vec<(PathBuf, PathBuf, &'static str)> = Vec::new();
    let mut started: Option<PathBuf> = None;
    let mut created: Vec<PathBuf> = Vec::new();
    let mut trashed: Vec<(Option<PathBuf>, PathBuf)> = Vec::new();
    for record in &records {
        match &record.event {
            LogEvent::ItemStarted { .. } => started = record.path.clone(),
            LogEvent::RenameFastPath => {
                if let (Some(source), Some(path)) = (&started, &record.path) {
                    pairs.push((source.clone(), path.clone(), "moved"));
                }
            }
            LogEvent::Created => {
                if let Some(path) = &record.path {
                    created.push(path.clone());
                    if let Some(source) = &started {
                        pairs.push((source.clone(), path.clone(), "made"));
                    }
                }
            }
            LogEvent::Note { text } if text.starts_with("trashed as ") => {
                if let Some(stored) = &record.path {
                    trashed.push((started.clone(), stored.clone()));
                }
            }
            _ => {}
        }
    }

    let (kind, steps) = match operation {
        Operation::Rename { .. } | Operation::BulkRename { .. } => (
            ChangeKind::Rename,
            pairs
                .iter()
                .filter(|(_, _, how)| *how == "moved")
                .map(|(source, now, _)| moved(now, source))
                .collect(),
        ),
        Operation::Move { sources, .. } => {
            let tops: BTreeSet<PathBuf> = sources
                .iter()
                .map(|source| source.as_path().to_path_buf())
                .collect();
            let mut steps: Vec<Step> = Vec::new();
            for (source, now, how) in &pairs {
                // Already part of an item moved whole.
                let covered = steps
                    .iter()
                    .any(|step| matches!(step, Step::Moved { was, .. } if source.starts_with(was)));
                if covered {
                    continue;
                }
                if tops.contains(source) {
                    steps.push(moved(now, source));
                } else if tops.iter().any(|top| source.starts_with(top)) {
                    // Inside a folder merged into one already there: the
                    // move went item by item, and so does its undo.
                    steps.push(if *how == "made" && now.is_dir() {
                        Step::Folder {
                            now: now.clone(),
                            was: source.clone(),
                        }
                    } else {
                        moved(now, source)
                    });
                }
            }
            (ChangeKind::Move, with_displaced(steps, &trashed, operation))
        }
        Operation::Copy { .. } => (
            ChangeKind::Copy,
            with_displaced(made_roots(&created), &trashed, operation),
        ),
        Operation::Duplicate { .. } => (ChangeKind::Duplicate, made_roots(&created)),
        Operation::CreateFolder { .. } => (ChangeKind::NewFolder, made_roots(&created)),
        Operation::CreateFile { .. } => (ChangeKind::NewFile, made_roots(&created)),
        Operation::Archive { .. } => (ChangeKind::Compress, made_roots(&created)),
        Operation::Extract { .. } => (ChangeKind::Extract, made_roots(&created)),
        Operation::Trash { .. } => (
            ChangeKind::Trash,
            trashed
                .iter()
                .filter_map(|(original, stored)| {
                    original.as_ref().map(|original| Step::Trashed {
                        original: original.clone(),
                        stored: stored.clone(),
                    })
                })
                .collect(),
        ),
        _ => return None,
    };
    if steps.is_empty() {
        return None;
    }
    let count = steps.len();
    let name = if count == 1 {
        match &steps[0] {
            Step::Moved { now, .. } => name_of(now),
            Step::Made { path, .. } => name_of(path),
            Step::Trashed { original, .. } => name_of(original),
            Step::Folder { now, .. } => name_of(now),
        }
    } else {
        String::new()
    };
    Some(Change {
        kind,
        name,
        count,
        steps,
    })
}

fn moved(now: &Path, was: &Path) -> Step {
    Step::Moved {
        now: now.to_path_buf(),
        was: was.to_path_buf(),
        id: Identity::of(now),
    }
}

/// What a job made, reduced to the outermost items: a copied folder is one
/// step, not one per file inside it.
fn made_roots(created: &[PathBuf]) -> Vec<Step> {
    let all: BTreeSet<&PathBuf> = created.iter().collect();
    let mut steps = Vec::new();
    let mut seen = BTreeSet::new();
    for path in created {
        let inside_another = path
            .ancestors()
            .skip(1)
            .any(|ancestor| all.contains(&ancestor.to_path_buf()));
        if !inside_another && seen.insert(path.clone()) && occupied(path) {
            steps.push(Step::Made {
                path: path.clone(),
                id: Identity::of(path),
                stored: None,
            });
        }
    }
    steps
}

/// For a copy or move that replaced things: each one the Replace sent to the
/// trash comes back when the change is undone. Its step comes first, so it
/// is undone last, after what replaced it has gone.
fn with_displaced(
    steps: Vec<Step>,
    trashed: &[(Option<PathBuf>, PathBuf)],
    operation: &Operation,
) -> Vec<Step> {
    let mut out: Vec<Step> = Vec::new();
    for (_, stored) in trashed {
        if let Ok(original) = displaced_original(stored)
            && original_belongs(&original, operation)
        {
            out.push(Step::Trashed {
                original,
                stored: stored.clone(),
            });
        }
    }
    out.extend(steps);
    out
}

/// Where a trashed item came from, from its `.trashinfo`.
fn displaced_original(stored: &Path) -> Result<PathBuf, ()> {
    let item = stored.file_name().ok_or(())?.to_string_lossy().into_owned();
    let root = stored.parent().and_then(Path::parent).ok_or(())?;
    trash::original_path_of(&TrashDirectory::new(root), &item).map_err(|_| ())
}

/// Whether a displaced item was in the job's destination.
fn original_belongs(original: &Path, operation: &Operation) -> bool {
    match operation {
        Operation::Copy { destination, .. } | Operation::Move { destination, .. } => {
            original.starts_with(destination.as_path())
        }
        _ => false,
    }
}
