//! Telling the storage layer when a Better Files job writes to an external disk.
//!
//! Issue #5's readiness rule rests on the storage service knowing which writes
//! are still running. Other applications' writes it sees through the kernel;
//! Better Files' own it is told about, as a tracked operation, so a copy to a
//! USB stick holds the device out of "Ready to unplug" from before its first
//! byte to after its last.
//!
//! Three parts, and only the last one touches a link:
//!
//! - [`written_paths`] says which paths a job changes. A copy writes its
//!   destination; a move writes its destination *and* removes from every
//!   source, which is why a move between two disks involves both.
//! - [`resolve_path`] and [`device_for`] map a path to the device it is on:
//!   symbolic links and `..` are resolved first, then the device whose mount
//!   point is the longest whole-component prefix wins.
//! - [`StorageTracker`] is the engine's [`JobObserver`]. It runs on the job's
//!   worker thread, so the filesystem lookups above never happen on the render
//!   thread, and the job waits for the started notice before it writes.
//!
//! `files-operations` knows none of this. It promises the observer's ordering
//! and nothing about devices, so the job engine has no storage dependency.

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};

use files_operations::{JobId, JobObserver, JobSpec, JobState, Operation, TrashItemRef};

use crate::devices::{DeviceLink, MountedDevice};

/// Resolves symbolic links and `..` in a path that may not exist yet.
///
/// The deepest ancestor that exists is canonicalized by the kernel; whatever
/// is below it is appended with `.` dropped and `..` applied to what has been
/// built so far, since a component that does not exist cannot be a link.
pub fn resolve_path(path: &Path) -> PathBuf {
    let mut existing = path;
    let mut rest: Vec<Component<'_>> = Vec::new();
    let mut resolved = loop {
        if let Ok(canonical) = existing.canonicalize() {
            break canonical;
        }
        match (existing.parent(), existing.components().next_back()) {
            (Some(parent), Some(last)) => {
                rest.push(last);
                existing = parent;
            }
            // Nothing along the path exists, not even `/`. There is nothing
            // to resolve against; the path is matched as it was given.
            _ => return path.to_path_buf(),
        }
    };
    for component in rest.into_iter().rev() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                resolved.pop();
            }
            other => resolved.push(other.as_os_str()),
        }
    }
    resolved
}

/// The device a resolved path is on: the one whose mount point is the longest
/// prefix of it, compared whole component by whole component.
pub fn device_for<'a>(path: &Path, devices: &'a [MountedDevice]) -> Option<&'a MountedDevice> {
    // `Path::starts_with` compares components, so `/media/a` is not a prefix
    // of `/media/ab`.
    devices
        .iter()
        .filter(|device| path.starts_with(&device.mount_point))
        .max_by_key(|device| device.mount_point.components().count())
}

/// Every path a job changes. Reading is not writing, so a checksum names none.
///
/// `home_trash` is where a trash with no explicit root goes, and
/// `original_of` answers where a trashed item will be restored to, which only
/// its `.trashinfo` records.
pub fn written_paths(
    operation: &Operation,
    home_trash: Option<&Path>,
    original_of: &dyn Fn(&TrashItemRef) -> Option<PathBuf>,
) -> Vec<PathBuf> {
    let paths = |list: &[files_core::LocalPath]| -> Vec<PathBuf> {
        list.iter()
            .map(|path| path.as_path().to_path_buf())
            .collect()
    };
    match operation {
        Operation::CreateFile { parent, .. } | Operation::CreateFolder { parent, .. } => {
            vec![parent.as_path().to_path_buf()]
        }
        Operation::Rename { path, .. } => vec![path.as_path().to_path_buf()],
        Operation::BulkRename { targets, .. } => paths(targets),
        Operation::Copy { destination, .. } => vec![destination.as_path().to_path_buf()],
        // A move removes from every source as well as writing the
        // destination, even within one filesystem: a rename is a write to
        // the directory it leaves.
        Operation::Move {
            sources,
            destination,
        } => {
            let mut written = vec![destination.as_path().to_path_buf()];
            written.extend(paths(sources));
            written
        }
        // A duplicate is written beside its source.
        Operation::Duplicate { sources } => paths(sources),
        // An item on another device goes to that device's own trash, which is
        // on the same device as the item, so the sources already name it. The
        // home trash is named as well, because an item whose device has no
        // usable trash is copied there instead.
        Operation::Trash {
            sources,
            trash_root,
        } => {
            let mut written = paths(sources);
            written.extend(
                trash_root
                    .clone()
                    .or_else(|| home_trash.map(Path::to_path_buf)),
            );
            written
        }
        Operation::RestoreFromTrash { items } => items
            .iter()
            .flat_map(|item| std::iter::once(item.trash_root.clone()).chain(original_of(item)))
            .collect(),
        Operation::PermanentDelete { targets, .. } => targets
            .iter()
            .map(|target| match target {
                files_operations::DeleteTarget::Path(path) => path.as_path().to_path_buf(),
                files_operations::DeleteTarget::TrashItem(item) => item.trash_root.clone(),
            })
            .collect(),
        Operation::Checksum { .. } => Vec::new(),
        // The archive file, which is created where it is named; its sources
        // are only read.
        Operation::Archive { destination, .. } => vec![destination.as_path().to_path_buf()],
        // The folder each archive is extracted into is created inside
        // `destination`; the archives are only read.
        Operation::Extract { destination, .. } => vec![destination.as_path().to_path_buf()],
    }
}

/// The object paths of the devices a job writes to, each once.
pub fn devices_written(
    operation: &Operation,
    home_trash: Option<&Path>,
    original_of: &dyn Fn(&TrashItemRef) -> Option<PathBuf>,
    devices: &[MountedDevice],
) -> Vec<String> {
    let mut objects: Vec<String> = Vec::new();
    for path in written_paths(operation, home_trash, original_of) {
        if let Some(device) = device_for(&resolve_path(&path), devices)
            && !objects.contains(&device.object_path)
        {
            objects.push(device.object_path.clone());
        }
    }
    objects
}

/// The identifier a job is registered under. Unique across processes, because
/// the service hears from all of them and a job number alone is not: it
/// continues from the job store, but two processes running at once can still
/// pick the same one.
pub fn operation_id(process: u32, job: JobId) -> String {
    format!("better-files:{process}:{job}")
}

/// The engine's observer: maps each job to devices and tells the link.
pub struct StorageTracker {
    link: RwLock<Option<Arc<dyn DeviceLink>>>,
    home_trash: Option<PathBuf>,
    process: u32,
    /// The devices each running job was announced to, so completion goes to
    /// exactly those even if a mount changed while the job ran.
    running: Mutex<HashMap<JobId, Vec<String>>>,
}

impl StorageTracker {
    pub fn new(home_trash: Option<PathBuf>) -> Self {
        Self {
            link: RwLock::new(None),
            home_trash,
            process: std::process::id(),
            running: Mutex::new(HashMap::new()),
        }
    }

    /// The tracker the real engine uses, with the session's home trash.
    pub fn from_env() -> Self {
        Self::new(files_platform::TrashDirectory::home_from_env().map(|trash| trash.root().into()))
    }

    /// Connects the tracker to the window's link. Until this is called — and
    /// while the link has no mounted devices — a job is registered nowhere,
    /// which is correct: there is nothing to hold ready.
    pub fn attach(&self, link: Arc<dyn DeviceLink>) {
        *self.link.write().expect("tracker link") = Some(link);
    }

    fn operation(&self, id: JobId) -> String {
        operation_id(self.process, id)
    }
}

impl JobObserver for StorageTracker {
    fn starting(&self, id: JobId, spec: &JobSpec) {
        let Some(link) = self.link.read().expect("tracker link").clone() else {
            return;
        };
        let devices = link.mounted_devices();
        if devices.is_empty() {
            return;
        }
        let original_of = |item: &TrashItemRef| {
            files_platform::original_path_of(
                &files_platform::TrashDirectory::new(&item.trash_root),
                &item.item,
            )
            .ok()
        };
        let objects = devices_written(
            &spec.operation,
            self.home_trash.as_deref(),
            &original_of,
            &devices,
        );
        if objects.is_empty() {
            return;
        }
        let operation = self.operation(id);
        for object_path in &objects {
            // A notice that fails is not a reason to stop the user's copy:
            // the device may simply read as ready early, which is the state
            // this ticket improves on rather than one it makes worse.
            if let Err(detail) = link.operation_started(object_path, &operation) {
                eprintln!(
                    "better-files: the storage layer was not told that {operation} is writing to {object_path}: {detail}"
                );
            }
        }
        self.running
            .lock()
            .expect("tracker jobs")
            .insert(id, objects);
    }

    fn finished(&self, id: JobId, _spec: &JobSpec, _state: JobState) {
        // Succeeded, failed, cancelled, or rolled back: in every case the job
        // has stopped writing, and every device it was announced to is
        // released, including one whose started notice failed — the service
        // may have recorded it even though the answer never arrived.
        let Some(objects) = self.running.lock().expect("tracker jobs").remove(&id) else {
            return;
        };
        let Some(link) = self.link.read().expect("tracker link").clone() else {
            return;
        };
        let operation = self.operation(id);
        for object_path in &objects {
            if let Err(detail) = link.operation_completed(object_path, &operation) {
                eprintln!(
                    "better-files: the storage layer was not told that {operation} finished writing to {object_path}: {detail}"
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use files_core::LocalPath;
    use files_operations::{DeleteConfirmation, DeleteTarget};

    fn mounted(object_path: &str, mount_point: &str) -> MountedDevice {
        MountedDevice {
            object_path: object_path.to_string(),
            mount_point: PathBuf::from(mount_point),
        }
    }

    fn local(path: &str) -> LocalPath {
        LocalPath::new(path).unwrap()
    }

    fn no_trash_info(_: &TrashItemRef) -> Option<PathBuf> {
        None
    }

    #[test]
    fn the_longest_mount_point_wins_and_a_prefix_must_be_whole_components() {
        let devices = vec![
            mounted("/a", "/media/tim/a"),
            mounted("/nested", "/media/tim/a/inner"),
            mounted("/ab", "/media/tim/ab"),
        ];
        let object = |path: &str| {
            device_for(Path::new(path), &devices).map(|device| device.object_path.as_str())
        };
        assert_eq!(object("/media/tim/a/photos/x.jpg"), Some("/a"));
        assert_eq!(object("/media/tim/a"), Some("/a"));
        assert_eq!(object("/media/tim/a/inner/deep"), Some("/nested"));
        assert_eq!(object("/media/tim/ab/file"), Some("/ab"));
        assert_eq!(object("/media/tim/abc/file"), None);
        assert_eq!(object("/media/tim"), None);
        assert_eq!(object("/home/tim/Documents"), None);
    }

    #[test]
    fn dotdot_and_symbolic_links_are_resolved_before_matching() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        let usb = root.join("media/usb");
        std::fs::create_dir_all(&usb).unwrap();
        std::fs::create_dir_all(root.join("home/tim")).unwrap();
        std::os::unix::fs::symlink(&usb, root.join("home/tim/stick")).unwrap();

        // Through a link, into a folder that does not exist yet.
        assert_eq!(
            resolve_path(&root.join("home/tim/stick/new/file.txt")),
            usb.join("new/file.txt")
        );
        // `..` out of home and into the device.
        assert_eq!(
            resolve_path(&root.join("home/tim/../../media/usb/x")),
            usb.join("x")
        );
        // `..` below the part that exists is applied lexically.
        assert_eq!(
            resolve_path(&root.join("media/usb/new/../other")),
            usb.join("other")
        );
        // `/home/tim/stick` does not start with the mount point as text; it
        // does once resolved.
        let devices = vec![mounted("/usb", usb.to_str().unwrap())];
        let resolved = resolve_path(&root.join("home/tim/stick/file"));
        assert_eq!(
            device_for(&resolved, &devices).map(|d| d.object_path.as_str()),
            Some("/usb")
        );
    }

    #[test]
    fn each_operation_names_the_paths_it_changes() {
        let trash = Path::new("/home/tim/.local/share/Trash");
        let paths = |operation: Operation| written_paths(&operation, Some(trash), &no_trash_info);

        assert_eq!(
            paths(Operation::Copy {
                sources: vec![local("/home/tim/a.txt")],
                destination: local("/media/usb"),
            }),
            vec![PathBuf::from("/media/usb")]
        );
        assert_eq!(
            paths(Operation::Move {
                sources: vec![local("/media/one/a.txt"), local("/media/one/b.txt")],
                destination: local("/media/two"),
            }),
            vec![
                PathBuf::from("/media/two"),
                PathBuf::from("/media/one/a.txt"),
                PathBuf::from("/media/one/b.txt"),
            ]
        );
        assert_eq!(
            paths(Operation::CreateFolder {
                parent: local("/media/usb"),
                name: "new".into(),
            }),
            vec![PathBuf::from("/media/usb")]
        );
        assert_eq!(
            paths(Operation::Trash {
                sources: vec![local("/media/usb/old.txt")],
                trash_root: None,
            }),
            vec![PathBuf::from("/media/usb/old.txt"), trash.to_path_buf()]
        );
        assert_eq!(
            paths(Operation::PermanentDelete {
                targets: vec![DeleteTarget::Path(local("/media/usb/old.txt"))],
                confirmation: DeleteConfirmation::explicit(),
            }),
            vec![PathBuf::from("/media/usb/old.txt")]
        );
        assert_eq!(
            paths(Operation::Checksum {
                targets: vec![local("/media/usb/big.iso")],
                algorithm: Default::default(),
            }),
            Vec::<PathBuf>::new()
        );
        // Compressing a folder on the internal disk onto a stick writes the
        // stick; extracting from the stick onto the internal disk writes the
        // internal disk, and only reads the stick.
        assert_eq!(
            paths(Operation::Archive {
                sources: vec![local("/home/tim/photos")],
                destination: local("/media/usb/photos.zip"),
                format: files_operations::ArchiveFormat::Zip,
            }),
            vec![PathBuf::from("/media/usb/photos.zip")]
        );
        assert_eq!(
            paths(Operation::Extract {
                archives: vec![local("/media/usb/photos.tar.zst")],
                destination: local("/home/tim/Downloads"),
            }),
            vec![PathBuf::from("/home/tim/Downloads")]
        );
    }

    #[test]
    fn a_restore_writes_where_the_trash_record_says_the_item_came_from() {
        let item = TrashItemRef::new("/home/tim/.local/share/Trash", "photo.jpg");
        let original = |reference: &TrashItemRef| {
            (reference.item == "photo.jpg").then(|| PathBuf::from("/media/usb/photo.jpg"))
        };
        assert_eq!(
            written_paths(
                &Operation::RestoreFromTrash { items: vec![item] },
                None,
                &original
            ),
            vec![
                PathBuf::from("/home/tim/.local/share/Trash"),
                PathBuf::from("/media/usb/photo.jpg"),
            ]
        );
    }

    #[test]
    fn trashing_on_a_usb_disk_holds_that_disk_where_its_own_trash_is_written() {
        // The item goes to `/media/usb/.Trash-1000`, on the disk itself, and
        // only when the disk has no usable trash to the home trash.
        let devices = vec![mounted("/usb", "/media/usb"), mounted("/home", "/home")];
        let trash = Operation::Trash {
            sources: vec![local("/media/usb/videos/clip.mkv")],
            trash_root: None,
        };
        assert_eq!(
            devices_written(
                &trash,
                Some(Path::new("/home/tim/.local/share/Trash")),
                &no_trash_info,
                &devices
            ),
            vec!["/usb".to_string(), "/home".to_string()]
        );
        // Restoring from, and emptying, the disk's own trash write only there.
        let item = TrashItemRef::new("/media/usb/.Trash-1000", "clip.mkv");
        let original = |_: &TrashItemRef| Some(PathBuf::from("/media/usb/videos/clip.mkv"));
        assert_eq!(
            devices_written(
                &Operation::RestoreFromTrash {
                    items: vec![item.clone()]
                },
                None,
                &original,
                &devices
            ),
            vec!["/usb".to_string()]
        );
        assert_eq!(
            devices_written(
                &Operation::PermanentDelete {
                    targets: vec![DeleteTarget::TrashItem(item)],
                    confirmation: DeleteConfirmation::explicit(),
                },
                None,
                &no_trash_info,
                &devices
            ),
            vec!["/usb".to_string()]
        );
    }

    #[test]
    fn a_move_between_two_devices_involves_both_once_each() {
        let devices = vec![mounted("/one", "/media/one"), mounted("/two", "/media/two")];
        let operation = Operation::Move {
            sources: vec![local("/media/one/a.txt"), local("/media/one/b.txt")],
            destination: local("/media/two/in"),
        };
        assert_eq!(
            devices_written(&operation, None, &no_trash_info, &devices),
            vec!["/two".to_string(), "/one".to_string()]
        );
        let home_only = Operation::Copy {
            sources: vec![local("/media/one/a.txt")],
            destination: local("/home/tim"),
        };
        assert!(devices_written(&home_only, None, &no_trash_info, &devices).is_empty());
    }
}
