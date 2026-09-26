//! The directory reader the window hands to every pane.
//!
//! `files-platform` has one reader per source: a local directory, and the
//! freedesktop trash. The Trash location is the home trash and every mounted
//! device's own trash read into one listing, so an item deleted on a USB disk
//! is found where every other deleted item is. A pane takes one
//! `DirectoryReader`, so this is the
//! dispatcher that picks the right one for a typed location — and refuses the
//! locations this build cannot list with the same [`ListingError::NotListable`]
//! the model already knows how to draw, rather than with an empty folder that
//! looks like a folder with nothing in it.
//!
//! Nothing here reads a directory on the calling thread. Every branch either
//! spawns a thread or hands the request to a reader that does.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

use files_core::listing::{DirectoryReader, ListingRequest, ListingSink};
use files_core::{ListingError, Location, TrashLocation, list_applications};
use files_platform::{LocalDirectoryReader, ReaderConfig, TrashDirectory, read_trash};

use crate::apps::CatalogHandle;

/// Finds the device trashes to list beside the home trash. Called on the
/// listing thread each time the Trash is opened, because devices come and go.
type VolumeTrashes = Arc<dyn Fn() -> Vec<TrashDirectory> + Send + Sync>;

/// Reads whichever kind of location it is handed.
pub struct FilesReader {
    local: LocalDirectoryReader,
    /// The home trash, when the session has one. `None` leaves only the
    /// device trashes, and a session with neither lists an empty Trash rather
    /// than failing, which is what one with no `$XDG_DATA_HOME` and no `$HOME`
    /// honestly has.
    trash: Option<TrashDirectory>,
    /// Every mounted device's own trash. A reader built with [`Self::new`]
    /// looks at none, so a test never reads the host's disks; the window's
    /// reader reads the mount table.
    volume_trashes: VolumeTrashes,
    /// The shared application catalog, for the Applications location.
    ///
    /// A snapshot is taken per listing and the rows are produced from it. No
    /// desktop file is read here, no path is invented, and nothing is written
    /// anywhere — the location is a view over records, which is the whole of
    /// Issue #4's "not a directory, not a symlink farm, not a FUSE mount".
    catalog: CatalogHandle,
    /// Whether an Applications listing includes entries the catalog excluded.
    /// Follows the window's hidden-entry preference, because a `NoDisplay`
    /// application is hidden in the same sense a dotfile is.
    include_hidden_applications: AtomicBool,
}

impl FilesReader {
    pub fn new(
        config: ReaderConfig,
        trash: Option<TrashDirectory>,
        catalog: CatalogHandle,
    ) -> Self {
        Self {
            local: LocalDirectoryReader::with_config(config),
            trash,
            volume_trashes: Arc::new(Vec::new),
            catalog,
            include_hidden_applications: AtomicBool::new(false),
        }
    }

    /// The reader a running window uses: MIME detection from the session's
    /// shared MIME database, the home trash and every mounted device's trash,
    /// and an empty catalog that the window fills from a background thread.
    pub fn from_env() -> Self {
        Self::new(
            ReaderConfig::new().with_mime(files_platform::detector_from_env()),
            TrashDirectory::home_from_env(),
            CatalogHandle::empty(app_catalog_platform::SessionEnvironment::from_env()),
        )
        .with_volume_trashes(|| match files_platform::current_uid() {
            Some(uid) => {
                files_platform::volume_trashes(&files_platform::MountTable::from_env(), uid)
            }
            None => Vec::new(),
        })
    }

    /// Where the device trashes listed beside the home trash come from.
    pub fn with_volume_trashes(
        mut self,
        find: impl Fn() -> Vec<TrashDirectory> + Send + Sync + 'static,
    ) -> Self {
        self.volume_trashes = Arc::new(find);
        self
    }

    pub fn catalog(&self) -> &CatalogHandle {
        &self.catalog
    }

    /// Follows `Ctrl+H` into the Applications location.
    pub fn set_include_hidden_applications(&self, include: bool) {
        self.include_hidden_applications
            .store(include, Ordering::Relaxed);
    }
}

impl DirectoryReader for FilesReader {
    fn start(&self, request: ListingRequest, sink: ListingSink) {
        match &request.location {
            Location::Local(_) => self.local.start(request, sink),
            Location::Trash(TrashLocation::Root) => {
                let home = self.trash.clone();
                let volume_trashes = Arc::clone(&self.volume_trashes);
                let _ = thread::Builder::new()
                    .name("files-trash-listing".to_string())
                    .spawn(move || {
                        let mut sink = sink;
                        // Home first, then each device. A trash that is absent
                        // or unreadable lists as empty, so a session with none
                        // at all finishes with nothing rather than failing:
                        // "nothing I can see" is the honest answer.
                        let trashes = home.into_iter().chain(volume_trashes());
                        for trash in trashes {
                            if read_trash(&trash, &mut sink).is_err() {
                                // A cancelled read drops the sink, whose
                                // `Drop` reports the cancellation.
                                return;
                            }
                        }
                        let _ = sink.finish();
                    });
            }
            Location::Applications => {
                // A snapshot, not the lock: a reload while this listing runs
                // replaces the handle's catalog and leaves this listing
                // finishing the one it started with, rather than blocking
                // either of them.
                let catalog = self.catalog.snapshot();
                let view = self
                    .catalog
                    .view(self.include_hidden_applications.load(Ordering::Relaxed));
                let _ = thread::Builder::new()
                    .name("files-applications-listing".to_string())
                    .spawn(move || {
                        let mut sink = sink;
                        if list_applications(&catalog, &view, &mut sink).is_ok() {
                            let _ = sink.finish();
                        }
                        // A cancelled listing drops the sink, whose `Drop`
                        // reports the cancellation.
                    });
            }
            other => {
                let kind = other.kind();
                sink.fail(ListingError::NotListable(kind));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{Duration, Instant};

    use files_core::listing::{ListingEvent, ListingSession};
    use files_core::{Entry, EntryBody};

    use super::*;

    fn trashed(trash: &TrashDirectory, stem: &str, original: &str) {
        fs::create_dir_all(trash.info_dir()).unwrap();
        fs::create_dir_all(trash.files_dir()).unwrap();
        fs::write(
            trash.info_dir().join(format!("{stem}.trashinfo")),
            format!("[Trash Info]\nPath={original}\nDeletionDate=2024-01-01T00:00:00\n"),
        )
        .unwrap();
        fs::write(trash.files_dir().join(stem), b"x").unwrap();
    }

    fn list_trash(reader: &FilesReader) -> Vec<Entry> {
        let request = ListingRequest::new(Location::Trash(TrashLocation::Root));
        let (mut session, sink) = ListingSession::start(&request);
        reader.start(request, sink);
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut entries = Vec::new();
        loop {
            let mut done = false;
            for event in session.drain() {
                match event {
                    ListingEvent::Batch(batch) => entries.extend(batch.entries),
                    event if event.is_terminal() => done = true,
                    _ => {}
                }
            }
            if done {
                return entries;
            }
            assert!(
                Instant::now() < deadline,
                "the trash listing never finished"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    fn stored(entry: &Entry) -> PathBuf {
        match &entry.body {
            EntryBody::Trashed(facts) => facts.stored_path.as_path().to_path_buf(),
            other => panic!("expected a trashed entry, got {other:?}"),
        }
    }

    #[test]
    fn the_trash_lists_the_home_trash_and_every_device_trash_together() {
        let root = tempfile::tempdir().unwrap();
        let home = TrashDirectory::new(root.path().join("home/.local/share/Trash"));
        let stick = root.path().join("stick");
        let device = TrashDirectory::new(stick.join(".Trash-1000"));
        // The same name in both: two items, not one.
        trashed(&home, "report.txt", "/home/user/report.txt");
        trashed(&device, "report.txt", "docs/report.txt");

        let volumes = vec![device.clone()];
        let reader = FilesReader::new(
            ReaderConfig::new(),
            Some(home.clone()),
            CatalogHandle::empty(Default::default()),
        )
        .with_volume_trashes(move || volumes.clone());
        let mut entries = list_trash(&reader);
        entries.sort_by_key(stored);

        assert_eq!(entries.len(), 2);
        assert_ne!(
            entries[0].id(),
            entries[1].id(),
            "selection tells them apart"
        );
        let originals: Vec<&Path> = entries
            .iter()
            .map(|entry| match &entry.body {
                EntryBody::Trashed(facts) => facts.original_path.as_path(),
                _ => unreachable!(),
            })
            .collect();
        assert!(originals.contains(&Path::new("/home/user/report.txt")));
        assert!(originals.contains(&stick.join("docs/report.txt").as_path()));
    }

    #[test]
    fn a_session_with_no_home_trash_still_lists_device_trashes() {
        let root = tempfile::tempdir().unwrap();
        let device = TrashDirectory::new(root.path().join("stick/.Trash-1000"));
        trashed(&device, "clip.mov", "clip.mov");
        let volumes = vec![device.clone()];
        let reader = FilesReader::new(
            ReaderConfig::new(),
            None,
            CatalogHandle::empty(Default::default()),
        )
        .with_volume_trashes(move || volumes.clone());
        let entries = list_trash(&reader);
        assert_eq!(entries.len(), 1);
        assert_eq!(stored(&entries[0]), device.files_dir().join("clip.mov"));
    }

    #[test]
    fn a_reader_built_for_tests_looks_at_no_device_trash() {
        let root = tempfile::tempdir().unwrap();
        let home = TrashDirectory::new(root.path().join("Trash"));
        trashed(&home, "a.txt", "/a.txt");
        let reader = FilesReader::new(
            ReaderConfig::new(),
            Some(home),
            CatalogHandle::empty(Default::default()),
        );
        assert_eq!(list_trash(&reader).len(), 1);
    }
}
