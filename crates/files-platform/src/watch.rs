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

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::time::Duration;

use files_core::entry::{EntryId, HiddenState};
use files_core::hidden::HiddenRules;
use files_core::location::LocalPath;
use files_core::model::RefreshEvent;
use notify::{
    Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher, WatcherKind,
    event::{ModifyKind, RenameMode},
};

use crate::PlatformError;
use crate::local::ReaderConfig;

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
    /// The watched directory itself was deleted or moved away: nothing
    /// that follows is about the folder the view shows.
    Gone,
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
        let watched = directory.to_path_buf();
        let mut watcher =
            notify::recommended_watcher(move |event| forward(&sender, &watched, event))
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

fn forward(sender: &Sender<WatchEvent>, directory: &Path, event: notify::Result<Event>) {
    for resolved in classify(directory, event) {
        if sender.send(resolved).is_err() {
            return;
        }
    }
}

/// What one `notify` event means for a watch on `directory`.
///
/// A kernel queue overflow arrives as an event flagged `Rescan` (notify
/// 7's `Q_OVERFLOW`), not as an error, and is the one thing that must never
/// be dropped: the list would quietly miss rows. The directory itself
/// deleted (`DELETE_SELF`) or moved (`MOVE_SELF`, reported as a rename
/// "from" of its own path) is [`WatchEvent::Gone`].
pub fn classify(directory: &Path, event: notify::Result<Event>) -> Vec<WatchEvent> {
    let Ok(event) = event else {
        // A watcher error is usually a dropped-event queue. Reported as a
        // resynchronization rather than dropped, because the alternative is a
        // list that is silently missing rows.
        return vec![WatchEvent::Overflowed];
    };
    if event.need_rescan() {
        return vec![WatchEvent::Overflowed];
    }
    if event.paths.iter().any(|path| path == directory)
        && matches!(
            event.kind,
            EventKind::Remove(_) | EventKind::Modify(ModifyKind::Name(_))
        )
    {
        return vec![WatchEvent::Gone];
    }
    let kind = match event.kind {
        EventKind::Create(_) => Some(Change::Created),
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
        return Vec::new();
    };
    event
        .paths
        .iter()
        .enumerate()
        .map(|(index, path)| match kind {
            Change::Created => WatchEvent::Created(path.clone()),
            Change::Removed => WatchEvent::Removed(path.clone()),
            Change::Modified => WatchEvent::Modified(path.clone()),
            // `RenameMode::Both` carries the old path then the new one.
            Change::Renamed if index == 0 => WatchEvent::Removed(path.clone()),
            Change::Renamed => WatchEvent::Created(path.clone()),
        })
        .collect()
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
    refresh_with(directory, rules, event, &ReaderConfig::new())
}

/// [`refresh_for`], with the row built by the listing's own configuration
/// ([`crate::local::read_entry`]): its type detector and mount table, so a
/// file that appears through an event has the icon and type a re-listing
/// would give it.
pub fn refresh_with(
    directory: &LocalPath,
    rules: &HiddenRules,
    event: &WatchEvent,
    config: &ReaderConfig,
) -> Option<RefreshEvent> {
    let path = match event {
        WatchEvent::Overflowed | WatchEvent::Gone => return Some(RefreshEvent::Resynchronize),
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
    let Some(entry) = crate::local::read_entry(directory, &name, config, rules) else {
        // It was created and is already gone. Removal is the honest update.
        return Some(RefreshEvent::Removed(id));
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

// --- One watch for the whole process ----------------------------------------

/// What a view hears about the folder it subscribed to.
#[derive(Clone, Debug, PartialEq)]
pub enum HubUpdate {
    /// The kernel watch is in place (`polling` false), or this folder can only
    /// be read again now and then (`polling` true: a network or FUSE-daemon
    /// filesystem, or no watch could be made). What changed before this moment
    /// is unknown, so a view reads the folder once more when it arrives.
    Ready { polling: bool },
    /// Rows that changed, already read (`lstat`) off the window's thread.
    Rows(Vec<RefreshEvent>),
    /// Read the whole folder again: events were lost, too many came at once,
    /// or its `.hidden` file changed.
    Resynchronize,
    /// The folder was deleted or moved away. `nearest` is the closest folder
    /// above it that is still there.
    Gone { nearest: Option<PathBuf> },
    /// The watch was let go of so a device can be unmounted; `Ready` follows
    /// if the folder comes back.
    Suspended,
}

/// More distinct paths than this in one batch and the folder is read again
/// instead: one listing on the reader's thread is cheaper than ten thousand
/// rows built one by one.
pub const HUB_BURST: usize = 4096;

/// How long the hub gathers events before it reads the rows they name, so a
/// file being written is read once per batch rather than once per write.
pub const HUB_BATCH: Duration = Duration::from_millis(30);

enum HubMessage {
    Subscribe {
        id: u64,
        directory: PathBuf,
        config: ReaderConfig,
        updates: Sender<HubUpdate>,
    },
    Unsubscribe(u64),
    Suspend(PathBuf),
    Resume(PathBuf),
    Raw(notify::Result<Event>),
    Watching(PathBuf, Sender<bool>),
}

/// The one inotify instance every view in the process shares (the kernel
/// allows a user 128 instances; one per tab would run out), with one thread
/// that makes and drops watches, reads `.hidden`, and `lstat`s changed rows,
/// so a folder on a stalled filesystem can never freeze a window.
pub struct WatchHub {
    messages: Sender<HubMessage>,
    next: std::sync::atomic::AtomicU64,
}

/// A view's subscription to one folder. Dropping it lets the watch go.
pub struct Subscription {
    id: u64,
    directory: PathBuf,
    updates: Receiver<HubUpdate>,
    messages: Sender<HubMessage>,
}

impl Subscription {
    pub fn directory(&self) -> &Path {
        &self.directory
    }

    /// Everything that has arrived, without waiting.
    pub fn poll(&self) -> Vec<HubUpdate> {
        self.updates.try_iter().collect()
    }
}

impl Drop for Subscription {
    fn drop(&mut self) {
        let _ = self.messages.send(HubMessage::Unsubscribe(self.id));
    }
}

impl WatchHub {
    /// The process's hub, started on first use.
    pub fn global() -> &'static WatchHub {
        static HUB: std::sync::OnceLock<WatchHub> = std::sync::OnceLock::new();
        HUB.get_or_init(WatchHub::start)
    }

    fn start() -> WatchHub {
        let (messages, inbox) = channel::<HubMessage>();
        let raw = messages.clone();
        let _ = std::thread::Builder::new()
            .name("files-watch-hub".to_string())
            .spawn(move || {
                let watcher = notify::recommended_watcher(move |event| {
                    let _ = raw.send(HubMessage::Raw(event));
                })
                .ok();
                HubState {
                    watcher,
                    subs: std::collections::HashMap::new(),
                    watched: std::collections::HashMap::new(),
                }
                .run(inbox);
            });
        WatchHub {
            messages,
            next: std::sync::atomic::AtomicU64::new(1),
        }
    }

    /// Starts following `directory`. Returns at once; [`HubUpdate::Ready`]
    /// says when the watch is in place.
    pub fn subscribe(&self, directory: &Path, config: ReaderConfig) -> Subscription {
        let id = self.next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let (updates, receiver) = channel();
        let _ = self.messages.send(HubMessage::Subscribe {
            id,
            directory: directory.to_path_buf(),
            config,
            updates,
        });
        Subscription {
            id,
            directory: directory.to_path_buf(),
            updates: receiver,
            messages: self.messages.clone(),
        }
    }

    /// Lets go of every watch on a folder under `mount_point`, in every
    /// window, before a device there is unmounted.
    pub fn suspend_under(&self, mount_point: &Path) {
        let _ = self
            .messages
            .send(HubMessage::Suspend(mount_point.to_path_buf()));
    }

    /// Watches again what [`Self::suspend_under`] let go of (an eject that
    /// failed, or a device that came back).
    pub fn resume_under(&self, mount_point: &Path) {
        let _ = self
            .messages
            .send(HubMessage::Resume(mount_point.to_path_buf()));
    }

    /// Whether a kernel watch is held on `directory` now. Waits for the hub;
    /// for tests.
    pub fn is_watching(&self, directory: &Path) -> bool {
        let (reply, answer) = channel();
        let _ = self
            .messages
            .send(HubMessage::Watching(directory.to_path_buf(), reply));
        answer.recv_timeout(Duration::from_secs(5)).unwrap_or(false)
    }
}

struct HubSub {
    directory: PathBuf,
    config: ReaderConfig,
    rules: HiddenRules,
    updates: Sender<HubUpdate>,
    /// Holds a kernel watch on `directory`.
    watching: bool,
    suspended: bool,
}

struct HubState {
    watcher: Option<RecommendedWatcher>,
    subs: std::collections::HashMap<u64, HubSub>,
    /// How many subscriptions hold the kernel watch on each folder.
    watched: std::collections::HashMap<PathBuf, usize>,
}

impl HubState {
    fn run(mut self, inbox: Receiver<HubMessage>) {
        while let Ok(message) = inbox.recv() {
            let mut batch = Vec::new();
            let mut lost = false;
            let mut deadline = None;
            let mut next = Some(message);
            loop {
                if let Some(message) = next.take() {
                    match message {
                        HubMessage::Raw(Ok(event)) => {
                            batch.push(event);
                            deadline.get_or_insert_with(|| std::time::Instant::now() + HUB_BATCH);
                        }
                        HubMessage::Raw(Err(_)) => {
                            lost = true;
                            deadline.get_or_insert_with(|| std::time::Instant::now() + HUB_BATCH);
                        }
                        other => self.command(other),
                    }
                }
                let Some(until) = deadline else { break };
                let left = until.saturating_duration_since(std::time::Instant::now());
                if left.is_zero() {
                    break;
                }
                match inbox.recv_timeout(left) {
                    Ok(message) => next = Some(message),
                    Err(RecvTimeoutError::Timeout) => break,
                    Err(RecvTimeoutError::Disconnected) => return,
                }
            }
            if !batch.is_empty() || lost {
                self.deliver(&batch, lost);
            }
        }
    }

    fn command(&mut self, message: HubMessage) {
        match message {
            HubMessage::Subscribe {
                id,
                directory,
                config,
                updates,
            } => {
                let mut sub = HubSub {
                    directory,
                    config,
                    rules: HiddenRules::dotfiles_only(),
                    updates,
                    watching: false,
                    suspended: false,
                };
                self.attach(&mut sub);
                self.subs.insert(id, sub);
            }
            HubMessage::Unsubscribe(id) => {
                if let Some(mut sub) = self.subs.remove(&id) {
                    self.release(&mut sub);
                }
            }
            HubMessage::Suspend(mount_point) => {
                let ids: Vec<u64> = self
                    .subs
                    .iter()
                    .filter(|(_, sub)| !sub.suspended && sub.directory.starts_with(&mount_point))
                    .map(|(id, _)| *id)
                    .collect();
                for id in ids {
                    let mut sub = self.subs.remove(&id).expect("listed");
                    self.release(&mut sub);
                    sub.suspended = true;
                    let _ = sub.updates.send(HubUpdate::Suspended);
                    self.subs.insert(id, sub);
                }
            }
            HubMessage::Resume(mount_point) => {
                let ids: Vec<u64> = self
                    .subs
                    .iter()
                    .filter(|(_, sub)| sub.suspended && sub.directory.starts_with(&mount_point))
                    .map(|(id, _)| *id)
                    .collect();
                for id in ids {
                    let mut sub = self.subs.remove(&id).expect("listed");
                    sub.suspended = false;
                    if sub.directory.is_dir() {
                        self.attach(&mut sub);
                    } else {
                        let _ = sub.updates.send(HubUpdate::Gone {
                            nearest: nearest_folder(&sub.directory),
                        });
                    }
                    self.subs.insert(id, sub);
                }
            }
            HubMessage::Watching(directory, reply) => {
                let _ = reply.send(self.watched.get(&directory).is_some_and(|count| *count > 0));
            }
            HubMessage::Raw(_) => {}
        }
    }

    /// Makes the kernel watch for `sub`, or says it will be polled.
    fn attach(&mut self, sub: &mut HubSub) {
        // Read now, not at window start, so a share mounted since is known.
        let mounts = crate::mounts::MountTable::from_env();
        let unannounced = mounts.changes_may_go_unannounced(&sub.directory);
        if !unannounced {
            let count = self.watched.get(&sub.directory).copied().unwrap_or(0);
            let made = count > 0
                || self.watcher.as_mut().is_some_and(|watcher| {
                    watcher
                        .watch(&sub.directory, RecursiveMode::NonRecursive)
                        .is_ok()
                });
            if made {
                self.watched.insert(sub.directory.clone(), count + 1);
                sub.watching = true;
                sub.rules = crate::hidden::read_hidden_rules(&sub.directory);
            }
        }
        let _ = sub.updates.send(HubUpdate::Ready {
            polling: !sub.watching,
        });
    }

    fn release(&mut self, sub: &mut HubSub) {
        if !std::mem::take(&mut sub.watching) {
            return;
        }
        let count = self.watched.get(&sub.directory).copied().unwrap_or(1);
        if count <= 1 {
            self.watched.remove(&sub.directory);
            if let Some(watcher) = self.watcher.as_mut() {
                let _ = watcher.unwatch(&sub.directory);
            }
        } else {
            self.watched.insert(sub.directory.clone(), count - 1);
        }
    }

    fn deliver(&mut self, batch: &[Event], lost: bool) {
        let ids: Vec<u64> = self
            .subs
            .iter()
            .filter(|(_, sub)| sub.watching)
            .map(|(id, _)| *id)
            .collect();
        for id in ids {
            let mut sub = self.subs.remove(&id).expect("listed");
            if let Some(update) = self.update_for(&mut sub, batch, lost) {
                let _ = sub.updates.send(update);
            }
            self.subs.insert(id, sub);
        }
    }

    /// What one batch means for one subscription, its rows read here.
    fn update_for(&mut self, sub: &mut HubSub, batch: &[Event], lost: bool) -> Option<HubUpdate> {
        if lost {
            return Some(HubUpdate::Resynchronize);
        }
        // The last word on each path, in order of arrival.
        let mut latest: Vec<WatchEvent> = Vec::new();
        for event in batch {
            for change in classify(&sub.directory, Ok(event.clone())) {
                match change {
                    WatchEvent::Overflowed => return Some(HubUpdate::Resynchronize),
                    WatchEvent::Gone => {
                        self.release(sub);
                        return Some(HubUpdate::Gone {
                            nearest: nearest_folder(&sub.directory),
                        });
                    }
                    WatchEvent::Created(ref path)
                    | WatchEvent::Modified(ref path)
                    | WatchEvent::Removed(ref path) => {
                        if path.parent() != Some(sub.directory.as_path()) {
                            continue;
                        }
                        latest.retain(|seen| watch_path(seen) != Some(path.as_path()));
                        latest.push(change);
                    }
                }
            }
        }
        if latest.is_empty() {
            return None;
        }
        if latest
            .iter()
            .any(|event| watch_path(event).and_then(Path::file_name) == Some(".hidden".as_ref()))
        {
            sub.rules = crate::hidden::read_hidden_rules(&sub.directory);
            return Some(HubUpdate::Resynchronize);
        }
        if latest.len() > HUB_BURST {
            return Some(HubUpdate::Resynchronize);
        }
        let directory = LocalPath::new(&sub.directory).ok()?;
        let rows: Vec<RefreshEvent> = latest
            .iter()
            .filter_map(|event| refresh_with(&directory, &sub.rules, event, &sub.config))
            .collect();
        (!rows.is_empty()).then_some(HubUpdate::Rows(rows))
    }
}

fn watch_path(event: &WatchEvent) -> Option<&Path> {
    match event {
        WatchEvent::Created(path) | WatchEvent::Modified(path) | WatchEvent::Removed(path) => {
            Some(path)
        }
        WatchEvent::Overflowed | WatchEvent::Gone => None,
    }
}

/// The closest folder above `path` that is still there.
fn nearest_folder(path: &Path) -> Option<PathBuf> {
    path.ancestors()
        .skip(1)
        .find(|ancestor| ancestor.is_dir())
        .map(Path::to_path_buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use files_core::entry::{EntryKind, EntrySize};
    use std::fs;
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
    fn a_kernel_queue_overflow_asks_for_a_relisting() {
        // notify 7 reports `Q_OVERFLOW` as an `Other` event flagged `Rescan`.
        let overflow = Event::new(EventKind::Other).set_flag(notify::event::Flag::Rescan);
        assert_eq!(
            classify(Path::new("/d"), Ok(overflow)),
            vec![WatchEvent::Overflowed]
        );
    }

    #[test]
    fn the_folder_itself_deleted_or_moved_is_gone() {
        let deleted = Event::new(EventKind::Remove(notify::event::RemoveKind::Folder))
            .add_path(PathBuf::from("/d"));
        assert_eq!(
            classify(Path::new("/d"), Ok(deleted)),
            vec![WatchEvent::Gone]
        );
        let moved = Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::From)))
            .add_path(PathBuf::from("/d"));
        assert_eq!(classify(Path::new("/d"), Ok(moved)), vec![WatchEvent::Gone]);
        let child = Event::new(EventKind::Remove(notify::event::RemoveKind::File))
            .add_path(PathBuf::from("/d/x"));
        assert_eq!(
            classify(Path::new("/d"), Ok(child)),
            vec![WatchEvent::Removed(PathBuf::from("/d/x"))]
        );
    }

    /// Waits up to five seconds for an update `accept` takes.
    fn wait_update(
        subscription: &Subscription,
        accept: impl Fn(&HubUpdate) -> bool,
    ) -> Option<HubUpdate> {
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(5) {
            if let Some(found) = subscription
                .poll()
                .into_iter()
                .find(|update| accept(update))
            {
                return Some(found);
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        None
    }

    #[test]
    fn the_hub_reads_changed_rows_and_says_when_the_folder_goes() {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("watched");
        fs::create_dir(&folder).unwrap();
        let hub = WatchHub::global();
        let subscription = hub.subscribe(&folder, ReaderConfig::new());
        assert_eq!(
            wait_update(&subscription, |u| matches!(u, HubUpdate::Ready { .. })),
            Some(HubUpdate::Ready { polling: false })
        );
        assert!(hub.is_watching(&folder));

        fs::write(folder.join("new.txt"), b"abc").unwrap();
        let rows = wait_update(&subscription, |u| matches!(u, HubUpdate::Rows(_)));
        let Some(HubUpdate::Rows(rows)) = rows else {
            panic!("no rows: {rows:?}");
        };
        assert!(
            rows.iter().any(|row| matches!(row,
            RefreshEvent::Added(entry) | RefreshEvent::Modified(entry) if entry.name == "new.txt"))
        );

        fs::write(folder.join(".hidden"), b"new.txt\n").unwrap();
        assert!(wait_update(&subscription, |u| *u == HubUpdate::Resynchronize).is_some());

        hub.suspend_under(root.path());
        assert!(wait_update(&subscription, |u| *u == HubUpdate::Suspended).is_some());
        assert!(!hub.is_watching(&folder));
        hub.resume_under(root.path());
        assert!(wait_update(&subscription, |u| matches!(u, HubUpdate::Ready { .. })).is_some());
        assert!(hub.is_watching(&folder));

        fs::remove_dir_all(&folder).unwrap();
        assert_eq!(
            wait_update(&subscription, |u| matches!(u, HubUpdate::Gone { .. })),
            Some(HubUpdate::Gone {
                nearest: Some(root.path().to_path_buf())
            })
        );
        drop(subscription);
        assert!(!hub.is_watching(&folder));
    }

    #[test]
    fn many_writes_to_one_file_are_one_row() {
        let root = tempfile::tempdir().unwrap();
        let hub = WatchHub::global();
        let subscription = hub.subscribe(root.path(), ReaderConfig::new());
        wait_update(&subscription, |u| matches!(u, HubUpdate::Ready { .. })).unwrap();
        let path = root.path().join("growing.log");
        let mut file = fs::File::create(&path).unwrap();
        for _ in 0..500 {
            use std::io::Write;
            file.write_all(&[b'x'; 64]).unwrap();
            file.flush().unwrap();
        }
        let Some(HubUpdate::Rows(rows)) =
            wait_update(&subscription, |u| matches!(u, HubUpdate::Rows(_)))
        else {
            panic!("no rows");
        };
        assert_eq!(rows.len(), 1, "one row for one path, however many writes");
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
