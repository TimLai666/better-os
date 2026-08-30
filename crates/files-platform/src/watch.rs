//! Watching one directory, and turning kernel events into incremental
//! refreshes.
//!
//! Issue #6 requires file watching to produce incremental refreshes rather
//! than full re-listings, so this crate does the `lstat` for the one path that
//! changed and emits a [`files_core::RefreshEvent`] the model applies to a
//! single row. A thousand files appearing produces a thousand row insertions,
//! not a thousand directory reads.
//!
//! The dependency and its shape are `app-catalog-platform`'s: `notify`'s
//! recommended backend, with the backend reported so a caller can prove it is
//! event-driven rather than assume it. The difference is what comes out — the
//! catalog reloads wholesale because desktop-entry precedence means one file
//! can change a different application, while a directory listing has no such
//! coupling and can be updated one row at a time.
//!
//! When the kernel drops events, that is reported as
//! [`files_core::RefreshEvent::Resynchronize`] instead of being papered over.
//! A list that is quietly wrong is worse than one that says it needs
//! rereading.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::time::Duration;

use files_core::entry::{
    Entry, EntryBody, EntryId, EntryKind, EntrySize, FileFacts, FileTime, HiddenState,
    PermissionsSummary, SymlinkStatus,
};
use files_core::hidden::HiddenRules;
use files_core::location::LocalPath;
use files_core::model::RefreshEvent;
use notify::{
    Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher, WatcherKind,
    event::{CreateKind, ModifyKind, RemoveKind, RenameMode},
};

use crate::PlatformError;

/// How the watcher learns about changes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WatchBackend {
    /// The kernel delivers events. Nothing runs while the directory is idle.
    EventDriven,
    /// No event source was available and the backend re-reads on a timer.
    /// Reported, never chosen silently.
    Polling,
}

/// A raw change, before it is turned into a model update.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WatchEvent {
    Created(PathBuf),
    Modified(PathBuf),
    Removed(PathBuf),
    /// Events were lost. The directory has to be listed again.
    Overflowed,
}

/// Watches one directory, non-recursively.
///
/// Non-recursive on purpose: a file list shows one directory, and watching a
/// tree would deliver events for entries no view is showing while costing an
/// inotify watch per subdirectory.
pub struct DirectoryWatcher {
    watcher: RecommendedWatcher,
    receiver: Receiver<WatchEvent>,
    directory: PathBuf,
}

impl DirectoryWatcher {
    pub fn new(directory: &Path) -> Result<Self, PlatformError> {
        if !directory.is_dir() {
            return Err(PlatformError::NotADirectory(
                directory.to_string_lossy().into_owned(),
            ));
        }
        let (sender, receiver) = channel();
        let mut watcher = notify::recommended_watcher(move |event| forward(&sender, event))
            .map_err(|error| PlatformError::WatchFailed(error.to_string()))?;
        watcher
            .watch(directory, RecursiveMode::NonRecursive)
            .map_err(|error| PlatformError::WatchFailed(error.to_string()))?;
        Ok(Self {
            watcher,
            receiver,
            directory: directory.to_path_buf(),
        })
    }

    /// The backend `notify` selected on this host.
    pub fn backend(&self) -> WatchBackend {
        match RecommendedWatcher::kind() {
            WatcherKind::PollWatcher => WatchBackend::Polling,
            _ => WatchBackend::EventDriven,
        }
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }

    /// Takes everything that has arrived without waiting.
    pub fn poll(&self) -> Vec<WatchEvent> {
        self.receiver.try_iter().collect()
    }

    /// Waits for one event. For tests; nothing on a render thread calls this.
    pub fn recv_timeout(&self, timeout: Duration) -> Option<WatchEvent> {
        match self.receiver.recv_timeout(timeout) {
            Ok(event) => Some(event),
            Err(RecvTimeoutError::Timeout) | Err(RecvTimeoutError::Disconnected) => None,
        }
    }

    /// Stops watching. Dropping the watcher does the same; this is here for a
    /// caller that wants the watch released at a known point.
    pub fn stop(mut self) {
        let _ = self.watcher.unwatch(&self.directory);
    }
}

fn forward(sender: &Sender<WatchEvent>, event: notify::Result<Event>) {
    let Ok(event) = event else {
        // A watcher error is usually a dropped-event queue. Reported as a
        // resynchronization rather than dropped, because the alternative is a
        // list that is silently missing rows.
        let _ = sender.send(WatchEvent::Overflowed);
        return;
    };
    let kind = match event.kind {
        EventKind::Create(CreateKind::Any | CreateKind::File | CreateKind::Folder) => {
            Some(Change::Created)
        }
        EventKind::Create(_) => Some(Change::Created),
        EventKind::Remove(RemoveKind::Any | RemoveKind::File | RemoveKind::Folder) => {
            Some(Change::Removed)
        }
        EventKind::Remove(_) => Some(Change::Removed),
        // A rename arrives as two paths: the old name goes, the new one
        // appears. Turning it into one "modified" would leave the old row on
        // screen under a name that no longer exists.
        EventKind::Modify(ModifyKind::Name(RenameMode::From)) => Some(Change::Removed),
        EventKind::Modify(ModifyKind::Name(RenameMode::To)) => Some(Change::Created),
        EventKind::Modify(ModifyKind::Name(_)) => Some(Change::Renamed),
        EventKind::Modify(_) => Some(Change::Modified),
        _ => None,
    };
    let Some(kind) = kind else {
        return;
    };
    for (index, path) in event.paths.iter().enumerate() {
        let resolved = match kind {
            Change::Created => WatchEvent::Created(path.clone()),
            Change::Removed => WatchEvent::Removed(path.clone()),
            Change::Modified => WatchEvent::Modified(path.clone()),
            // `RenameMode::Both` carries the old path then the new one.
            Change::Renamed if index == 0 => WatchEvent::Removed(path.clone()),
            Change::Renamed => WatchEvent::Created(path.clone()),
        };
        if sender.send(resolved).is_err() {
            return;
        }
    }
}

#[derive(Clone, Copy)]
enum Change {
    Created,
    Removed,
    Modified,
    Renamed,
}

/// Turns a raw watch event into the model update for one row.
///
/// The `lstat` happens here, for the one path that changed. A create or modify
/// whose path has already disappeared again becomes a removal, which is the
/// correct answer for a temporary file that was written and deleted between
/// the event and this call.
pub fn refresh_for(
    directory: &LocalPath,
    rules: &HiddenRules,
    event: &WatchEvent,
) -> Option<RefreshEvent> {
    let path = match event {
        WatchEvent::Overflowed => return Some(RefreshEvent::Resynchronize),
        WatchEvent::Created(path) | WatchEvent::Modified(path) | WatchEvent::Removed(path) => path,
    };
    // An event for something that is not directly in this directory is not
    // this listing's business.
    if path.parent() != Some(directory.as_path()) {
        return None;
    }
    let name = path.file_name()?.to_string_lossy().into_owned();
    let id = EntryId::Name(name.clone());
    if matches!(event, WatchEvent::Removed(_)) {
        return Some(RefreshEvent::Removed(id));
    }
    let entry_path = directory.join_name(&name).ok()?;
    let Ok(link_metadata) = fs::symlink_metadata(entry_path.as_path()) else {
        // It was created and is already gone. Removal is the honest update.
        return Some(RefreshEvent::Removed(id));
    };
    let is_symlink = link_metadata.file_type().is_symlink();
    let (symlink, metadata) = if is_symlink {
        let target = fs::read_link(entry_path.as_path()).unwrap_or_default();
        match fs::metadata(entry_path.as_path()) {
            Ok(resolved) => (SymlinkStatus::Resolved { target }, Some(resolved)),
            Err(error) if error.raw_os_error() == Some(40) => {
                (SymlinkStatus::Loop { target }, None)
            }
            Err(_) => (SymlinkStatus::Broken { target }, None),
        }
    } else {
        (SymlinkStatus::None, Some(link_metadata))
    };

    let kind = match &metadata {
        Some(metadata) if metadata.is_dir() => EntryKind::Directory,
        Some(metadata) if metadata.is_file() => EntryKind::File,
        Some(_) => EntryKind::Special,
        None => EntryKind::Unknown,
    };
    let entry = Entry {
        name: name.clone(),
        kind,
        size: match &metadata {
            Some(metadata) if metadata.is_file() => EntrySize::Bytes(metadata.len()),
            _ => EntrySize::Unknown,
        },
        modified: metadata
            .as_ref()
            .and_then(|metadata| metadata.modified().ok())
            .map(FileTime::from_system_time),
        permissions: PermissionsSummary::UNKNOWN,
        hidden: rules.classify(&name),
        mime: None,
        body: EntryBody::File(FileFacts {
            path: entry_path,
            symlink,
            device: None,
        }),
    };
    let entry = Box::new(entry);
    match event {
        WatchEvent::Created(_) => Some(RefreshEvent::Added(entry)),
        _ => Some(RefreshEvent::Modified(entry)),
    }
}

/// Whether the process is running where an unmodified `HiddenState` matters.
/// Kept as a helper so a caller can build the same default the listing does.
pub fn default_hidden_state(rules: &HiddenRules, name: &str) -> HiddenState {
    rules.classify(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    /// Waits for the watcher to deliver at least one event, or gives up.
    ///
    /// A fixed sleep would either be flaky or slow; this polls for up to five
    /// seconds, which is long enough for inotify on a loaded machine and
    /// returns as soon as the event arrives.
    fn wait_for(watcher: &DirectoryWatcher, deadline: Duration) -> Vec<WatchEvent> {
        let started = Instant::now();
        let mut collected = Vec::new();
        while started.elapsed() < deadline {
            collected.extend(watcher.poll());
            if !collected.is_empty() {
                // Give the backend a moment to deliver the rest of a burst.
                std::thread::sleep(Duration::from_millis(50));
                collected.extend(watcher.poll());
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        collected
    }

    #[test]
    fn a_created_file_produces_an_insertion_for_one_row() {
        let root = tempfile::tempdir().unwrap();
        let watcher = DirectoryWatcher::new(root.path()).unwrap();
        fs::write(root.path().join("new.txt"), b"hello").unwrap();

        let events = wait_for(&watcher, Duration::from_secs(5));
        assert!(!events.is_empty(), "the watcher delivered no events");

        let directory = LocalPath::new(root.path()).unwrap();
        let rules = HiddenRules::dotfiles_only();
        let refreshes: Vec<RefreshEvent> = events
            .iter()
            .filter_map(|event| refresh_for(&directory, &rules, event))
            .collect();
        let added = refreshes.iter().find_map(|refresh| match refresh {
            RefreshEvent::Added(entry) | RefreshEvent::Modified(entry) => Some(entry),
            _ => None,
        });
        let added = added.expect("no insertion for the created file");
        assert_eq!(added.name, "new.txt");
        assert_eq!(added.kind, EntryKind::File);
        assert_eq!(added.size, EntrySize::Bytes(5));
    }

    #[test]
    fn a_removed_file_produces_a_removal_by_identity() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("doomed.txt");
        fs::write(&path, b"x").unwrap();
        let watcher = DirectoryWatcher::new(root.path()).unwrap();
        fs::remove_file(&path).unwrap();

        let events = wait_for(&watcher, Duration::from_secs(5));
        let directory = LocalPath::new(root.path()).unwrap();
        let rules = HiddenRules::dotfiles_only();
        let removed = events
            .iter()
            .filter_map(|event| refresh_for(&directory, &rules, event))
            .any(|refresh| refresh == RefreshEvent::Removed(EntryId::Name("doomed.txt".into())));
        assert!(removed, "no removal event for the deleted file: {events:?}");
    }

    #[test]
    fn an_event_for_a_nested_path_is_not_this_listings_business() {
        let root = tempfile::tempdir().unwrap();
        let directory = LocalPath::new(root.path()).unwrap();
        let rules = HiddenRules::dotfiles_only();
        let nested = root.path().join("sub/deep.txt");
        assert_eq!(
            refresh_for(&directory, &rules, &WatchEvent::Created(nested)),
            None
        );
    }

    #[test]
    fn a_lost_event_queue_asks_for_a_relisting_rather_than_pretending() {
        let root = tempfile::tempdir().unwrap();
        let directory = LocalPath::new(root.path()).unwrap();
        let rules = HiddenRules::dotfiles_only();
        assert_eq!(
            refresh_for(&directory, &rules, &WatchEvent::Overflowed),
            Some(RefreshEvent::Resynchronize)
        );
    }

    #[test]
    fn a_created_file_that_is_already_gone_becomes_a_removal() {
        let root = tempfile::tempdir().unwrap();
        let directory = LocalPath::new(root.path()).unwrap();
        let rules = HiddenRules::dotfiles_only();
        let event = WatchEvent::Created(root.path().join("transient.tmp"));
        assert_eq!(
            refresh_for(&directory, &rules, &event),
            Some(RefreshEvent::Removed(EntryId::Name("transient.tmp".into())))
        );
    }

    #[test]
    fn a_created_dotfile_arrives_already_marked_hidden() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join(".secret"), b"x").unwrap();
        let directory = LocalPath::new(root.path()).unwrap();
        let rules = HiddenRules::dotfiles_only();
        let event = WatchEvent::Created(root.path().join(".secret"));
        match refresh_for(&directory, &rules, &event) {
            Some(RefreshEvent::Added(entry)) => assert!(entry.hidden.is_hidden()),
            other => panic!("expected an addition, got {other:?}"),
        }
    }

    #[test]
    fn watching_something_that_is_not_a_directory_is_refused() {
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("plain.txt");
        fs::write(&file, b"x").unwrap();
        assert!(matches!(
            DirectoryWatcher::new(&file),
            Err(PlatformError::NotADirectory(_))
        ));
    }
}
