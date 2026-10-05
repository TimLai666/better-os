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
//!
//! ## A share that does not answer
//!
//! The Trash listing reads the home trash and sends its rows before it looks
//! for any device. Trashes on local disks come next, on the listing thread. A
//! network or FUSE filesystem is never touched by the listing thread: each one
//! gets a probe thread of its own that finds and reads its trash, and the
//! listing waits [`PROBE_DEADLINE`] for the probes together. A share whose
//! probe answered in time is known to be reachable and its rows are listed. One
//! that did not is named as skipped, and its probe is left to finish or hang on
//! its own — a thread stuck in an NFS `stat` cannot be interrupted. Until that
//! probe returns the share is known not to answer: a later listing does not
//! start a second probe of it and does not wait for it.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use files_core::listing::{
    DirectoryReader, ListingEvent, ListingRequest, ListingSession, ListingSink,
};
use files_core::{Entry, ListingError, Location, SkippedEntry, TrashLocation, list_applications};
use files_platform::{LocalDirectoryReader, ReaderConfig, TrashDirectory, read_trash};

use crate::apps::CatalogHandle;

/// How long a Trash listing waits for the probes of network and FUSE
/// filesystems, all together, after the home trash is on screen. A share that
/// answers at all answers a directory read in milliseconds; one that has not
/// answered in a second is treated as gone for this listing.
pub(crate) const PROBE_DEADLINE: Duration = Duration::from_secs(1);

/// How often a listing waiting on probes checks whether it was cancelled.
const CANCEL_POLL: Duration = Duration::from_millis(50);

/// The key a share that did not answer is reported as skipped with.
const NOT_ANSWERING: &str = "files.trash.error.filesystem_not_answering";

/// Finds the device trashes on local storage to list beside the home trash.
/// Called on the listing thread each time the Trash is opened, because devices
/// come and go.
type VolumeTrashes = Arc<dyn Fn() -> Vec<TrashDirectory> + Send + Sync>;

/// Names the network and FUSE filesystems whose trash is looked for off the
/// listing thread. Called on the listing thread each time the Trash is opened;
/// it must not touch the filesystems themselves.
type ProbedMounts = Arc<dyn Fn() -> Vec<ProbedMount> + Send + Sync>;

/// What one probe finds on a network or FUSE filesystem: each trash with the
/// identity that tells one volume seen through two mount points apart from two
/// volumes.
type Found = Vec<(TrashDirectory, (u64, u64))>;

/// A network or FUSE filesystem, and how to find its trashes. `find` runs on a
/// probe thread and may never return.
pub struct ProbedMount {
    mount_point: PathBuf,
    find: Box<dyn FnOnce() -> Found + Send>,
}

impl ProbedMount {
    pub fn new(
        mount_point: impl Into<PathBuf>,
        find: impl FnOnce() -> Found + Send + 'static,
    ) -> Self {
        Self {
            mount_point: mount_point.into(),
            find: Box::new(find),
        }
    }
}

/// Reads whichever kind of location it is handed.
pub struct FilesReader {
    local: LocalDirectoryReader,
    /// The home trash, when the session has one. `None` leaves only the
    /// device trashes, and a session with neither lists an empty Trash rather
    /// than failing, which is what one with no `$XDG_DATA_HOME` and no `$HOME`
    /// honestly has.
    trash: Option<TrashDirectory>,
    /// Every local device's own trash. A reader built with [`Self::new`]
    /// looks at none, so a test never reads the host's disks; the window's
    /// reader reads the mount table.
    volume_trashes: VolumeTrashes,
    /// The network and FUSE filesystems, probed rather than read. None for a
    /// reader built with [`Self::new`].
    probed_mounts: ProbedMounts,
    /// Mount points whose probe has not returned yet, across listings.
    probing: Arc<Mutex<HashSet<PathBuf>>>,
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
            probed_mounts: Arc::new(Vec::new),
            probing: Arc::new(Mutex::new(HashSet::new())),
            catalog,
            include_hidden_applications: AtomicBool::new(false),
        }
    }

    /// The reader a running window uses: MIME detection from the session's
    /// shared MIME database, the home trash, every local device's trash, a
    /// probe of every network and FUSE filesystem, and an empty catalog that
    /// the window fills from a background thread.
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
        .with_probed_mounts(|| {
            let Some(uid) = files_platform::current_uid() else {
                return Vec::new();
            };
            files_platform::network_or_fuse_mounts(&files_platform::MountTable::from_env())
                .into_iter()
                .map(|mount_point| {
                    let topdir = mount_point.clone();
                    ProbedMount::new(mount_point, move || {
                        files_platform::trashes_on_mount(&topdir, uid)
                    })
                })
                .collect()
        })
    }

    /// Where the local device trashes listed beside the home trash come from.
    pub fn with_volume_trashes(
        mut self,
        find: impl Fn() -> Vec<TrashDirectory> + Send + Sync + 'static,
    ) -> Self {
        self.volume_trashes = Arc::new(find);
        self
    }

    /// Where the network and FUSE filesystems to probe come from.
    pub fn with_probed_mounts(
        mut self,
        find: impl Fn() -> Vec<ProbedMount> + Send + Sync + 'static,
    ) -> Self {
        self.probed_mounts = Arc::new(find);
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

/// What a probe sends back: its mount point, and each trash it found with that
/// trash's rows and skipped names.
type Answer = (PathBuf, Vec<((u64, u64), Vec<Entry>, Vec<SkippedEntry>)>);

/// The Trash listing, on its own thread. A trash that is absent or unreadable
/// lists as empty, so a session with none at all finishes with nothing rather
/// than failing: "nothing I can see" is the honest answer. A cancelled read
/// returns and drops the sink, whose `Drop` reports the cancellation.
fn list_trashes(
    mut sink: ListingSink,
    home: Option<TrashDirectory>,
    volume_trashes: VolumeTrashes,
    probed_mounts: ProbedMounts,
    probing: Arc<Mutex<HashSet<PathBuf>>>,
) {
    // The home trash reaches the screen before any device is looked for.
    if let Some(home) = home
        && (read_trash(&home, &mut sink).is_err() || sink.flush().is_err())
    {
        return;
    }

    let (answers, receiver) = mpsc::channel::<Answer>();
    let mut waiting = Vec::new();
    let mut not_answering = Vec::new();
    for mount in probed_mounts() {
        let mount_point = mount.mount_point.clone();
        if !probing
            .lock()
            .expect("probe set")
            .insert(mount_point.clone())
        {
            // Its last probe has not come back: known not to answer.
            not_answering.push(mount_point);
            continue;
        }
        let answers = answers.clone();
        let done = Arc::clone(&probing);
        let spawned = thread::Builder::new()
            .name("files-trash-probe".to_string())
            .spawn(move || {
                let read: Vec<_> = (mount.find)()
                    .into_iter()
                    .map(|(trash, identity)| {
                        let (entries, skipped) = read_detached(&trash);
                        (identity, entries, skipped)
                    })
                    .collect();
                done.lock().expect("probe set").remove(&mount.mount_point);
                let _ = answers.send((mount.mount_point, read));
            });
        if spawned.is_ok() {
            waiting.push(mount_point);
        } else {
            probing.lock().expect("probe set").remove(&mount_point);
            not_answering.push(mount_point);
        }
    }
    drop(answers);
    let deadline = Instant::now() + PROBE_DEADLINE;

    for trash in volume_trashes() {
        if read_trash(&trash, &mut sink).is_err() {
            return;
        }
    }

    let mut seen = HashSet::new();
    while !waiting.is_empty() {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let (mount_point, found) = match receiver.recv_timeout(remaining.min(CANCEL_POLL)) {
            Ok(answer) => answer,
            Err(RecvTimeoutError::Timeout) if sink.is_cancelled() => return,
            Err(RecvTimeoutError::Timeout) if remaining.is_zero() => break,
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => break,
        };
        waiting.retain(|waited| *waited != mount_point);
        for (identity, entries, skipped) in found {
            // One volume through two mount points is listed once.
            if !seen.insert(identity) {
                continue;
            }
            for entry in entries {
                if sink.push(entry).is_err() {
                    return;
                }
            }
            for skipped in skipped {
                if sink.skip(skipped.name, skipped.error).is_err() {
                    return;
                }
            }
        }
    }
    not_answering.extend(waiting);
    for mount_point in not_answering {
        if sink
            .skip(display(&mount_point), not_answering_error(&mount_point))
            .is_err()
        {
            return;
        }
    }
    let _ = sink.finish();
}

/// Reads a trash into rows without a listing to send them to, on a probe
/// thread.
fn read_detached(trash: &TrashDirectory) -> (Vec<Entry>, Vec<SkippedEntry>) {
    let request = ListingRequest::new(Location::Trash(TrashLocation::Root));
    let (mut session, mut sink) = ListingSession::start(&request);
    let mut entries = Vec::new();
    let mut skipped = Vec::new();
    if read_trash(trash, &mut sink).is_ok() {
        let _ = sink.finish();
    } else {
        drop(sink);
    }
    for event in session.drain() {
        match event {
            ListingEvent::Batch(batch) => entries.extend(batch.entries),
            ListingEvent::Complete(summary) => skipped = summary.skipped,
            _ => {}
        }
    }
    (entries, skipped)
}

fn display(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn not_answering_error(mount_point: &Path) -> ListingError {
    ListingError::Io {
        path: display(mount_point),
        reason: NOT_ANSWERING.to_string(),
    }
}

impl DirectoryReader for FilesReader {
    fn start(&self, request: ListingRequest, sink: ListingSink) {
        match &request.location {
            Location::Local(_) => self.local.start(request, sink),
            Location::Trash(TrashLocation::Root) => {
                let home = self.trash.clone();
                let volume_trashes = Arc::clone(&self.volume_trashes);
                let probed_mounts = Arc::clone(&self.probed_mounts);
                let probing = Arc::clone(&self.probing);
                let _ = thread::Builder::new()
                    .name("files-trash-listing".to_string())
                    .spawn(move || {
                        list_trashes(sink, home, volume_trashes, probed_mounts, probing);
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
    use std::sync::Mutex;
    use std::sync::atomic::AtomicUsize;
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
        list_trash_fully(reader).0
    }

    /// A finished Trash listing: its rows, and the names it reported as
    /// skipped.
    fn list_trash_fully(reader: &FilesReader) -> (Vec<Entry>, Vec<String>) {
        let request = ListingRequest::new(Location::Trash(TrashLocation::Root));
        let (mut session, sink) = ListingSession::start(&request);
        reader.start(request, sink);
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut entries = Vec::new();
        let mut skipped = Vec::new();
        loop {
            let mut done = false;
            for event in session.drain() {
                match event {
                    ListingEvent::Batch(batch) => entries.extend(batch.entries),
                    ListingEvent::Complete(summary) => {
                        skipped.extend(summary.skipped.into_iter().map(|entry| entry.name));
                        done = true;
                    }
                    event if event.is_terminal() => done = true,
                    _ => {}
                }
            }
            if done {
                return (entries, skipped);
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

    /// A probe that does not return until the test lets go of `gate`, the way
    /// `stat` on a share whose server has gone away does not return.
    fn stuck_share(
        mount_point: &Path,
        gate: &Arc<Mutex<()>>,
        calls: &Arc<AtomicUsize>,
    ) -> impl Fn() -> Vec<ProbedMount> + Send + Sync + 'static {
        let mount_point = mount_point.to_path_buf();
        let gate = Arc::clone(gate);
        let calls = Arc::clone(calls);
        move || {
            let gate = Arc::clone(&gate);
            let calls = Arc::clone(&calls);
            vec![ProbedMount::new(mount_point.clone(), move || {
                calls.fetch_add(1, Ordering::SeqCst);
                let _held = gate.lock();
                Vec::new()
            })]
        }
    }

    #[test]
    fn a_share_that_never_answers_does_not_hold_up_the_trash() {
        let root = tempfile::tempdir().unwrap();
        let home = TrashDirectory::new(root.path().join("Trash"));
        trashed(&home, "a.txt", "/a.txt");
        let gate = Arc::new(Mutex::new(()));
        let held = gate.lock().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let share = PathBuf::from("/mnt/server-gone");
        let reader = FilesReader::new(
            ReaderConfig::new(),
            Some(home),
            CatalogHandle::empty(Default::default()),
        )
        .with_probed_mounts(stuck_share(&share, &gate, &calls));

        let started = Instant::now();
        let (entries, skipped) = list_trash_fully(&reader);
        assert_eq!(entries.len(), 1, "the home trash is listed");
        assert_eq!(
            skipped,
            vec![share.display().to_string()],
            "and the share is named as not read"
        );
        assert!(started.elapsed() < PROBE_DEADLINE + Duration::from_secs(1));

        // Opened again while that probe is still stuck: the share is not
        // asked a second time and nothing waits for it.
        let started = Instant::now();
        let (entries, skipped) = list_trash_fully(&reader);
        assert_eq!(entries.len(), 1);
        assert_eq!(skipped, vec![share.display().to_string()]);
        assert!(
            started.elapsed() < PROBE_DEADLINE / 2,
            "a share already known not to answer is not waited for: {:?}",
            started.elapsed()
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        drop(held);
    }

    #[test]
    fn a_share_that_answers_has_its_trash_listed_once() {
        let root = tempfile::tempdir().unwrap();
        let home = TrashDirectory::new(root.path().join("home/Trash"));
        trashed(&home, "a.txt", "/a.txt");
        let share = root.path().join("share");
        let remote = TrashDirectory::new(share.join(".Trash-1000"));
        trashed(&remote, "b.txt", "b.txt");
        let found = remote.clone();
        let reader = FilesReader::new(
            ReaderConfig::new(),
            Some(home),
            CatalogHandle::empty(Default::default()),
        )
        .with_probed_mounts(move || {
            // The same share mounted in two places: one trash, listed once.
            ["/mnt/share", "/mnt/share-again"]
                .into_iter()
                .map(|mount_point| {
                    let found = found.clone();
                    ProbedMount::new(mount_point, move || vec![(found, (9, 9))])
                })
                .collect()
        });
        let (entries, skipped) = list_trash_fully(&reader);
        assert!(skipped.is_empty(), "{skipped:?}");
        let mut stored: Vec<PathBuf> = entries.iter().map(stored).collect();
        stored.sort();
        assert_eq!(stored.len(), 2, "{stored:?}");
        assert_eq!(stored[1], remote.files_dir().join("b.txt"));
    }

    #[test]
    fn the_home_trash_is_on_screen_before_any_device_is_looked_for() {
        let root = tempfile::tempdir().unwrap();
        let home = TrashDirectory::new(root.path().join("Trash"));
        trashed(&home, "a.txt", "/a.txt");
        let gate = Arc::new(Mutex::new(()));
        let held = gate.lock().unwrap();
        let finder_gate = Arc::clone(&gate);
        let reader = FilesReader::new(
            ReaderConfig::new(),
            Some(home),
            CatalogHandle::empty(Default::default()),
        )
        .with_volume_trashes(move || {
            let _held = finder_gate.lock();
            Vec::new()
        });

        let request = ListingRequest::new(Location::Trash(TrashLocation::Root));
        let (mut session, sink) = ListingSession::start(&request);
        reader.start(request, sink);
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut rows = 0;
        while rows == 0 && Instant::now() < deadline {
            for event in session.drain() {
                if let ListingEvent::Batch(batch) = event {
                    rows += batch.entries.len();
                }
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(
            rows, 1,
            "the home trash's row arrives while devices are still being found"
        );
        drop(held);
    }
}
