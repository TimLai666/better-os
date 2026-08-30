//! Turning a selection and a keystroke into a `files-operations` job spec.
//!
//! Every file operation the window offers ends here, and every one of them
//! becomes a `JobSpec` handed to the shared engine. There is no filesystem
//! call in the GUI, no `std::process::Command`, and no path assembled into a
//! string — the specs carry `LocalPath` and `OsString` end to end, which is
//! how a file named `caf\xe9 \xff report.txt` survives being copied from a
//! window as intact as it survives being copied from the engine's own tests.
//!
//! The clipboard is here rather than in the window because cut-and-paste is a
//! decision, not a widget: a cut followed by a paste is a move job, a copy
//! followed by a paste is a copy job, and both are refused rather than guessed
//! at when the destination cannot take them.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use files_core::{Entry, EntryBody, LocalPath, Location, TrashLocation};
use files_operations::{
    DeleteConfirmation, DeleteTarget, JobSpec, Operation, TrashItemRef, spec::is_usable_name,
};

/// What a cut or copy put on the clipboard.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum Clipboard {
    #[default]
    Empty,
    Copy(Vec<LocalPath>),
    Cut(Vec<LocalPath>),
}

impl Clipboard {
    pub fn is_empty(&self) -> bool {
        matches!(self, Clipboard::Empty)
    }

    pub fn len(&self) -> usize {
        match self {
            Clipboard::Empty => 0,
            Clipboard::Copy(paths) | Clipboard::Cut(paths) => paths.len(),
        }
    }
}

/// Why a command could not be built.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommandRefusal {
    /// Nothing was selected, or the clipboard was empty.
    NothingToActOn,
    /// The current location has no filesystem path to operate in — the
    /// Applications view, a network location, Recent.
    NotAFilesystemLocation,
    /// The name typed cannot be used: empty, `.`, `..`, or containing a
    /// separator.
    UnusableName,
    /// The selection is not in the trash, so there is nothing to put back.
    NotInTrash,
}

/// The local paths of the selected entries, in visible order.
///
/// An entry with no filesystem path — an application row — is skipped rather
/// than turned into one, which is the rule `files-core` states and the reason
/// there is no `EntryBody::Application` to `PathBuf` conversion anywhere.
pub fn selected_paths(entries: &[&Entry]) -> Vec<LocalPath> {
    entries
        .iter()
        .filter_map(|entry| entry.as_local_path().cloned())
        .collect()
}

/// The trash items among the selection, for restore and for emptying.
pub fn selected_trash_items(entries: &[&Entry], trash_root: &Path) -> Vec<TrashItemRef> {
    entries
        .iter()
        .filter_map(|entry| match &entry.body {
            EntryBody::Trashed(facts) => Some(TrashItemRef::new(
                trash_root.to_path_buf(),
                facts.item.clone(),
            )),
            _ => None,
        })
        .collect()
}

/// The directory a new item is created in, or a refusal when the location has
/// no directory.
pub fn writable_parent(location: &Location) -> Result<LocalPath, CommandRefusal> {
    location
        .as_local_path()
        .cloned()
        .ok_or(CommandRefusal::NotAFilesystemLocation)
}

pub fn new_folder(location: &Location, name: &str) -> Result<JobSpec, CommandRefusal> {
    let parent = writable_parent(location)?;
    let name = usable(name)?;
    Ok(JobSpec::new(Operation::CreateFolder { parent, name }))
}

pub fn new_file(location: &Location, name: &str) -> Result<JobSpec, CommandRefusal> {
    let parent = writable_parent(location)?;
    let name = usable(name)?;
    Ok(JobSpec::new(Operation::CreateFile { parent, name }))
}

pub fn rename(path: &LocalPath, new_name: &str) -> Result<JobSpec, CommandRefusal> {
    let new_name = usable(new_name)?;
    Ok(JobSpec::new(Operation::Rename {
        path: path.clone(),
        new_name,
    }))
}

pub fn duplicate(sources: Vec<LocalPath>) -> Result<JobSpec, CommandRefusal> {
    if sources.is_empty() {
        return Err(CommandRefusal::NothingToActOn);
    }
    Ok(JobSpec::new(Operation::Duplicate { sources }))
}

/// The job a paste builds: a copy for a copied clipboard, a move for a cut one.
pub fn paste(clipboard: &Clipboard, destination: &Location) -> Result<JobSpec, CommandRefusal> {
    let destination = writable_parent(destination)?;
    match clipboard {
        Clipboard::Empty => Err(CommandRefusal::NothingToActOn),
        Clipboard::Copy(sources) if !sources.is_empty() => Ok(JobSpec::new(Operation::Copy {
            sources: sources.clone(),
            destination,
        })),
        Clipboard::Cut(sources) if !sources.is_empty() => Ok(JobSpec::new(Operation::Move {
            sources: sources.clone(),
            destination,
        })),
        _ => Err(CommandRefusal::NothingToActOn),
    }
}

/// Move to trash. `trash_root` is `None` for the home trash, which is what a
/// desktop wants; naming one explicitly is how a device's own trash is reached.
pub fn move_to_trash(
    sources: Vec<LocalPath>,
    trash_root: Option<PathBuf>,
) -> Result<JobSpec, CommandRefusal> {
    if sources.is_empty() {
        return Err(CommandRefusal::NothingToActOn);
    }
    Ok(JobSpec::new(Operation::Trash {
        sources,
        trash_root,
    }))
}

/// Permanent delete.
///
/// The confirmation is a value the caller has to construct, and it can only be
/// constructed by [`DeleteConfirmation::explicit`] — there is no `Default` and
/// no `Deserialize`. That is why this function takes one rather than a `bool`:
/// a confirmed delete cannot be assembled from a config file or a replayed
/// event, only from a person answering the dialog.
pub fn delete_permanently(
    targets: Vec<DeleteTarget>,
    confirmation: DeleteConfirmation,
) -> Result<JobSpec, CommandRefusal> {
    if targets.is_empty() {
        return Err(CommandRefusal::NothingToActOn);
    }
    Ok(JobSpec::new(Operation::PermanentDelete {
        targets,
        confirmation,
    }))
}

/// Put back, which is only offered while the Trash is what is being viewed.
pub fn restore_from_trash(
    location: &Location,
    items: Vec<TrashItemRef>,
) -> Result<JobSpec, CommandRefusal> {
    if !matches!(location, Location::Trash(TrashLocation::Root)) {
        return Err(CommandRefusal::NotInTrash);
    }
    if items.is_empty() {
        return Err(CommandRefusal::NothingToActOn);
    }
    Ok(JobSpec::new(Operation::RestoreFromTrash { items }))
}

/// The delete targets for a selection: trash items when the Trash is being
/// viewed, plain paths everywhere else.
///
/// The trash root is optional because deleting a file by path does not need
/// one. Only emptying an item out of the trash does, and a session with no
/// trash directory produces no such targets rather than refusing every delete.
pub fn delete_targets(
    location: &Location,
    entries: &[&Entry],
    trash_root: Option<&Path>,
) -> Vec<DeleteTarget> {
    if matches!(location, Location::Trash(_)) {
        let Some(trash_root) = trash_root else {
            return Vec::new();
        };
        return selected_trash_items(entries, trash_root)
            .into_iter()
            .map(DeleteTarget::TrashItem)
            .collect();
    }
    selected_paths(entries)
        .into_iter()
        .map(DeleteTarget::Path)
        .collect()
}

fn usable(name: &str) -> Result<OsString, CommandRefusal> {
    let name = OsString::from(name.trim());
    if !is_usable_name(&name) {
        return Err(CommandRefusal::UnusableName);
    }
    Ok(name)
}
