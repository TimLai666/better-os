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
use std::path::PathBuf;

use files_core::{Entry, EntryBody, EntryKind, LocalPath, Location, TrashLocation};
use files_operations::{
    ArchiveFormat, DeleteConfirmation, DeleteTarget, JobSpec, Operation, TrashItemRef,
    spec::is_usable_name,
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
    /// Extract was asked of something that is not an archive this build
    /// reads.
    NotAnArchive,
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
///
/// Each item names the trash it is actually in. The Trash view merges the home
/// trash with every device's own, so one selection can span several, and the
/// only thing that says which is where the item's bytes are stored.
pub fn selected_trash_items(entries: &[&Entry]) -> Vec<TrashItemRef> {
    entries
        .iter()
        .filter_map(|entry| match &entry.body {
            EntryBody::Trashed(facts) => Some(TrashItemRef::new(
                files_platform::trash_root_of(facts.stored_path.as_path())?,
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
/// viewed, each emptied from the trash it is in, and plain paths everywhere
/// else.
pub fn delete_targets(location: &Location, entries: &[&Entry]) -> Vec<DeleteTarget> {
    if matches!(location, Location::Trash(_)) {
        return selected_trash_items(entries)
            .into_iter()
            .map(DeleteTarget::TrashItem)
            .collect();
    }
    selected_paths(entries)
        .into_iter()
        .map(DeleteTarget::Path)
        .collect()
}

/// Which archive commands the selection offers in this location.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ArchiveActions {
    /// Compress the selection into a new archive beside it.
    pub compress: bool,
    /// Extract every selected archive into a folder of its own beside it.
    pub extract: bool,
}

/// Decides [`ArchiveActions`] for a selection.
///
/// Both need a folder to write into and a selection made entirely of real
/// files and folders: an application row or a trashed item has no path to
/// read. Extract is offered only when every selected entry is a file whose
/// name says it is one of the archives [`ArchiveFormat`] reads.
pub fn archive_actions(location: &Location, entries: &[&Entry]) -> ArchiveActions {
    let writable = location.as_local_path().is_some();
    let all_local = !entries.is_empty() && selected_paths(entries).len() == entries.len();
    ArchiveActions {
        compress: writable && all_local,
        extract: writable && all_local && entries.iter().all(|entry| is_archive(entry)),
    }
}

fn is_archive(entry: &Entry) -> bool {
    entry.kind == EntryKind::File
        && entry
            .as_local_path()
            .and_then(|path| path.as_path().file_name())
            .and_then(ArchiveFormat::from_file_name)
            .is_some()
}

/// A compress the window has been asked for and is waiting on a format for.
///
/// The folder and the sources are fixed when it is asked, so the format
/// chooser cannot be answered for a different selection than the one it
/// was opened for.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompressRequest {
    pub parent: LocalPath,
    pub sources: Vec<LocalPath>,
    /// The archive's name without its extension.
    pub stem: OsString,
}

impl CompressRequest {
    /// The archive's file name in `format`: `photos.zip`, `photos.tar.gz`.
    pub fn file_name(&self, format: ArchiveFormat) -> OsString {
        let mut name = self.stem.clone();
        name.push(".");
        name.push(format.extension());
        name
    }

    /// The job, once a format is chosen. An archive already there is the
    /// engine's conflict to raise, like any other destination.
    pub fn spec(&self, format: ArchiveFormat) -> Result<JobSpec, CommandRefusal> {
        let name = self.file_name(format);
        if !is_usable_name(&name) {
            return Err(CommandRefusal::UnusableName);
        }
        let destination = LocalPath::new(self.parent.as_path().join(name))
            .map_err(|_| CommandRefusal::NotAFilesystemLocation)?;
        Ok(JobSpec::new(Operation::Archive {
            sources: self.sources.clone(),
            destination,
            format,
        }))
    }
}

/// The format chooser for a [`CompressRequest`]: which format the keyboard
/// is on. It opens on the first of [`ArchiveFormat::ALL`], `.zip`, the one any
/// recipient can open.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompressChooser {
    pub request: CompressRequest,
    focused: usize,
}

impl CompressChooser {
    pub fn new(request: CompressRequest) -> Self {
        Self {
            request,
            focused: 0,
        }
    }

    pub fn focused(&self) -> ArchiveFormat {
        ArchiveFormat::ALL[self.focused % ArchiveFormat::ALL.len()]
    }

    pub fn next(&mut self) {
        self.focused = (self.focused + 1) % ArchiveFormat::ALL.len();
    }

    pub fn previous(&mut self) {
        let count = ArchiveFormat::ALL.len();
        self.focused = (self.focused + count - 1) % count;
    }
}

/// Asks to compress the selection. The archive goes beside it, named after
/// the one item selected — a file without its extension, `report.pdf` giving
/// `report.zip` — or after the folder they are in when there are several.
pub fn compress_request(
    location: &Location,
    entries: &[&Entry],
) -> Result<CompressRequest, CommandRefusal> {
    let parent = writable_parent(location)?;
    let sources = selected_paths(entries);
    if sources.is_empty() || sources.len() != entries.len() {
        return Err(CommandRefusal::NothingToActOn);
    }
    let stem = match entries {
        [only] => {
            use std::os::unix::ffi::{OsStrExt, OsStringExt};
            let name = sources[0]
                .as_path()
                .file_name()
                .map(|name| name.as_bytes().to_vec())
                .unwrap_or_default();
            // The extension split ignores a leading dot, so `.bashrc` stays
            // `.bashrc`, the rule `files_operations::conflict` names by too.
            let stem = match name.iter().rposition(|byte| *byte == b'.') {
                Some(dot) if dot > 0 && only.kind != EntryKind::Directory => name[..dot].to_vec(),
                _ => name,
            };
            OsString::from_vec(stem)
        }
        _ => parent
            .as_path()
            .file_name()
            .map(|name| name.to_os_string())
            .unwrap_or_default(),
    };
    let stem = if stem.is_empty() {
        OsString::from("archive")
    } else {
        stem
    };
    Ok(CompressRequest {
        parent,
        sources,
        stem,
    })
}

/// Extract: each selected archive into a new folder named after it, in the
/// folder being viewed.
pub fn extract(location: &Location, entries: &[&Entry]) -> Result<JobSpec, CommandRefusal> {
    let destination = writable_parent(location)?;
    let archives = selected_paths(entries);
    if archives.is_empty() {
        return Err(CommandRefusal::NothingToActOn);
    }
    if archives.len() != entries.len() || !entries.iter().all(|entry| is_archive(entry)) {
        return Err(CommandRefusal::NotAnArchive);
    }
    Ok(JobSpec::new(Operation::Extract {
        archives,
        destination,
    }))
}

fn usable(name: &str) -> Result<OsString, CommandRefusal> {
    let name = OsString::from(name.trim());
    if !is_usable_name(&name) {
        return Err(CommandRefusal::UnusableName);
    }
    Ok(name)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use files_core::{EntryKind, TrashedFacts};

    use super::*;

    fn in_trash(root: &str, stem: &str) -> Entry {
        let mut entry = Entry::file(
            stem,
            LocalPath::new(Path::new(root).join("files").join(stem)).unwrap(),
            EntryKind::File,
        );
        entry.body = EntryBody::Trashed(TrashedFacts {
            item: stem.to_string(),
            original_path: PathBuf::from("/somewhere").join(stem),
            deleted_at: None,
            stored_path: LocalPath::new(Path::new(root).join("files").join(stem)).unwrap(),
        });
        entry
    }

    fn local_entry(path: &str, kind: EntryKind) -> Entry {
        let local = LocalPath::new(path).unwrap();
        Entry::file(local.file_name(), local, kind)
    }

    #[test]
    fn compress_is_offered_for_real_files_and_extract_only_for_archives() {
        let here = Location::local("/home/user/Downloads").unwrap();
        let report = local_entry("/home/user/Downloads/report.pdf", EntryKind::File);
        let photos = local_entry("/home/user/Downloads/photos.tar.gz", EntryKind::File);
        let zipped = local_entry("/home/user/Downloads/old.ZIP", EntryKind::File);
        let folder = local_entry("/home/user/Downloads/music.zip", EntryKind::Directory);

        assert_eq!(archive_actions(&here, &[]), ArchiveActions::default());
        assert_eq!(
            archive_actions(&here, &[&report]),
            ArchiveActions {
                compress: true,
                extract: false
            }
        );
        assert_eq!(
            archive_actions(&here, &[&photos, &zipped]),
            ArchiveActions {
                compress: true,
                extract: true
            }
        );
        // One non-archive in the selection takes Extract away.
        assert!(!archive_actions(&here, &[&photos, &report]).extract);
        // A folder that happens to end in `.zip` is not an archive.
        assert!(!archive_actions(&here, &[&folder]).extract);
        // Nowhere to write: neither.
        assert_eq!(
            archive_actions(&Location::Trash(TrashLocation::Root), &[&photos]),
            ArchiveActions::default()
        );
        let trashed = in_trash("/home/user/.local/share/Trash", "a.zip");
        assert_eq!(
            archive_actions(&here, &[&trashed]),
            ArchiveActions::default()
        );
        assert_eq!(
            extract(&here, &[&photos, &report]),
            Err(CommandRefusal::NotAnArchive)
        );
    }

    #[test]
    fn the_archive_is_named_after_the_selection_and_goes_beside_it() {
        let here = Location::local("/home/user/Downloads").unwrap();
        let report = local_entry("/home/user/Downloads/report.pdf", EntryKind::File);
        let folder = local_entry("/home/user/Downloads/v1.2", EntryKind::Directory);
        let dotfile = local_entry("/home/user/Downloads/.bashrc", EntryKind::File);

        let one = compress_request(&here, &[&report]).unwrap();
        assert_eq!(one.file_name(ArchiveFormat::Zip), "report.zip");
        assert_eq!(one.file_name(ArchiveFormat::TarZst), "report.tar.zst");
        // A folder's dots are part of its name.
        let dir = compress_request(&here, &[&folder]).unwrap();
        assert_eq!(dir.file_name(ArchiveFormat::TarGz), "v1.2.tar.gz");
        let dot = compress_request(&here, &[&dotfile]).unwrap();
        assert_eq!(dot.file_name(ArchiveFormat::Tar), ".bashrc.tar");
        let several = compress_request(&here, &[&report, &folder]).unwrap();
        assert_eq!(several.file_name(ArchiveFormat::Zip), "Downloads.zip");
        let at_root =
            compress_request(&Location::local("/").unwrap(), &[&report, &folder]).unwrap();
        assert_eq!(at_root.file_name(ArchiveFormat::Zip), "archive.zip");

        let spec = several.spec(ArchiveFormat::Zip).unwrap();
        assert_eq!(
            spec.operation,
            Operation::Archive {
                sources: vec![
                    LocalPath::new("/home/user/Downloads/report.pdf").unwrap(),
                    LocalPath::new("/home/user/Downloads/v1.2").unwrap(),
                ],
                destination: LocalPath::new("/home/user/Downloads/Downloads.zip").unwrap(),
                format: ArchiveFormat::Zip,
            }
        );
        assert_eq!(
            compress_request(&here, &[]),
            Err(CommandRefusal::NothingToActOn)
        );
    }

    #[test]
    fn extract_builds_one_job_for_every_selected_archive_into_the_current_folder() {
        let here = Location::local("/home/user/Downloads").unwrap();
        let a = local_entry("/home/user/Downloads/a.tar.zst", EntryKind::File);
        let b = local_entry("/home/user/Downloads/b.tgz", EntryKind::File);
        let spec = extract(&here, &[&a, &b]).unwrap();
        assert_eq!(
            spec.operation,
            Operation::Extract {
                archives: vec![
                    LocalPath::new("/home/user/Downloads/a.tar.zst").unwrap(),
                    LocalPath::new("/home/user/Downloads/b.tgz").unwrap(),
                ],
                destination: LocalPath::new("/home/user/Downloads").unwrap(),
            }
        );
    }

    #[test]
    fn each_trashed_entry_is_acted_on_in_the_trash_it_is_in() {
        let home = in_trash("/home/user/.local/share/Trash", "report.txt");
        let stick = in_trash("/media/user/STICK/.Trash-1000", "report.txt");
        let entries = [&home, &stick];

        assert_eq!(
            selected_trash_items(&entries),
            vec![
                TrashItemRef::new("/home/user/.local/share/Trash", "report.txt"),
                TrashItemRef::new("/media/user/STICK/.Trash-1000", "report.txt"),
            ]
        );
        let targets = delete_targets(&Location::Trash(TrashLocation::Root), &entries);
        assert_eq!(
            targets,
            vec![
                DeleteTarget::TrashItem(TrashItemRef::new(
                    "/home/user/.local/share/Trash",
                    "report.txt"
                )),
                DeleteTarget::TrashItem(TrashItemRef::new(
                    "/media/user/STICK/.Trash-1000",
                    "report.txt"
                )),
            ]
        );
    }
}
