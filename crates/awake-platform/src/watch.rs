//! Watched file and directory activity, through `inotify`.
//!
//! This is the one provider that is genuinely event-driven, which is why it has
//! no poll interval at all: `notify` gives us a kernel watch, and between events
//! the cost is one blocked thread and nothing else. A rule that says "stay awake
//! while my download folder is changing" therefore costs nothing at all while
//! the folder is quiet, which is exactly when it matters.
//!
//! What is recorded per path is one timestamp: when it last changed. Not the
//! file names, not the sizes, not the event kinds. A rule asks "did something
//! happen here within N seconds", and one number answers that; anything more
//! would be a record of what the user was working on, which ticket 26 forbids.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use awake_core::{Observations, ProviderKind, WatchActivity, WatchedPath};
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};

/// The last-change time of every watched path.
///
/// Shared with the watcher's own thread, which is why it is behind a mutex; the
/// map is tiny and written once per filesystem event, so contention is not a
/// consideration.
#[derive(Clone, Debug, Default)]
pub struct WatchLog {
    /// Only ever holds paths a rule asked for, so it is bounded by the rule set.
    last_change: Arc<Mutex<BTreeMap<PathBuf, u64>>>,
}

impl WatchLog {
    pub fn new() -> Self {
        Self::default()
    }

    /// Records that a watched path changed. Called from the watcher thread.
    pub fn record(&self, path: &Path, now_unix_seconds: u64) {
        if let Ok(mut log) = self.last_change.lock() {
            log.insert(path.to_path_buf(), now_unix_seconds);
        }
    }

    /// Starts tracking a path, with no activity recorded yet.
    ///
    /// The initial timestamp is zero rather than "now": a rule must not fire the
    /// instant the service starts just because it began watching. It fires when
    /// something actually happens.
    pub fn begin_tracking(&self, path: &Path) {
        if let Ok(mut log) = self.last_change.lock() {
            log.entry(path.to_path_buf()).or_insert(0);
        }
    }

    pub fn stop_tracking(&self, path: &Path) {
        if let Ok(mut log) = self.last_change.lock() {
            log.remove(path);
        }
    }

    pub fn tracked(&self) -> Vec<PathBuf> {
        self.last_change
            .lock()
            .map(|log| log.keys().cloned().collect())
            .unwrap_or_default()
    }

    pub fn activity(&self) -> Vec<WatchActivity> {
        self.last_change
            .lock()
            .map(|log| {
                log.iter()
                    .map(|(path, last)| WatchActivity {
                        path: path.clone(),
                        last_change_unix_seconds: *last,
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// Watches the paths the rules name.
pub struct WatchProvider {
    log: WatchLog,
    watcher: Option<RecommendedWatcher>,
    /// Paths currently registered with the kernel watch.
    watched: Vec<PathBuf>,
    /// Why the watcher could not be created, when it could not.
    failure: Option<String>,
    /// Paths a rule asked for that could not be watched, each with its reason.
    /// Kept so the rule editor can say which path is the problem rather than
    /// reporting the whole provider as broken.
    rejected: BTreeMap<PathBuf, String>,
}

impl WatchProvider {
    /// Creates a watcher, or records why one could not be created.
    ///
    /// A missing `inotify` is a real state in a container with a low
    /// `max_user_instances`, and it is reported rather than retried in a loop.
    pub fn new(clock: impl Fn() -> u64 + Send + 'static) -> Self {
        let log = WatchLog::new();
        let sink = log.clone();
        let watcher = notify::recommended_watcher(move |event: notify::Result<Event>| {
            let Ok(event) = event else { return };
            // Access events are not activity: reading a file does not mean a
            // download is still running, and treating them as such would keep
            // the machine awake for a backup scan.
            if !(event.kind.is_create() || event.kind.is_modify() || event.kind.is_remove()) {
                return;
            }
            let now = clock();
            for path in &event.paths {
                sink.record(path, now);
                // The rule names a directory; the event names a file inside it.
                // Both are recorded so a condition on the directory matches.
                if let Some(parent) = path.parent() {
                    sink.record(parent, now);
                }
            }
        });

        match watcher {
            Ok(watcher) => Self {
                log,
                watcher: Some(watcher),
                watched: Vec::new(),
                failure: None,
                rejected: BTreeMap::new(),
            },
            Err(error) => Self {
                log,
                watcher: None,
                watched: Vec::new(),
                failure: Some(format!("awake.provider.watch_unavailable:{error}")),
                rejected: BTreeMap::new(),
            },
        }
    }

    pub fn log(&self) -> &WatchLog {
        &self.log
    }

    /// Whether a watcher exists at all.
    pub fn is_available(&self) -> bool {
        self.watcher.is_some()
    }

    /// Paths a rule asked for that could not be watched, and why.
    pub fn rejected(&self) -> &BTreeMap<PathBuf, String> {
        &self.rejected
    }

    /// Brings the kernel watches in line with the paths the rules name.
    ///
    /// Called whenever the rule set changes, so a path nobody watches any more
    /// stops costing a watch descriptor.
    pub fn watch_only(&mut self, wanted: &[WatchedPath]) {
        let wanted: Vec<PathBuf> = wanted
            .iter()
            .map(|path| path.as_path().to_path_buf())
            .collect();

        let Some(watcher) = self.watcher.as_mut() else {
            return;
        };

        for existing in self.watched.clone() {
            if !wanted.contains(&existing) {
                let _ = watcher.unwatch(&existing);
                self.watched.retain(|path| path != &existing);
                self.log.stop_tracking(&existing);
                self.rejected.remove(&existing);
            }
        }

        for path in wanted {
            if self.watched.contains(&path) {
                continue;
            }
            // A recursive watch on a home directory would be a watch descriptor
            // per subdirectory, so this is deliberately non-recursive: a rule
            // watches a folder, not a tree.
            match watcher.watch(&path, RecursiveMode::NonRecursive) {
                Ok(()) => {
                    self.watched.push(path.clone());
                    self.log.begin_tracking(&path);
                    self.rejected.remove(&path);
                }
                Err(error) => {
                    // One unwatchable path — a folder that was deleted, a
                    // permission problem — must not take the other watches down
                    // with it.
                    self.rejected
                        .insert(path, format!("awake.provider.path_unwatchable:{error}"));
                }
            }
        }
    }

    pub fn kind(&self) -> ProviderKind {
        ProviderKind::WatchedPath
    }

    pub fn cadence(&self) -> crate::provider::Cadence {
        crate::provider::Cadence::EventDriven
    }

    /// Copies the recorded activity into a sample.
    pub fn sample(&mut self, _now_unix_seconds: u64, into: &mut Observations) {
        match &self.failure {
            Some(failure) => into.mark_unavailable(ProviderKind::WatchedPath, failure.clone()),
            None => {
                into.watch_activity = Some(self.log.activity());
                into.mark_available(ProviderKind::WatchedPath);
            }
        }
    }
}

impl std::fmt::Debug for WatchProvider {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `RecommendedWatcher` is not `Debug`, and its internals are not
        // something a log line should carry anyway.
        formatter
            .debug_struct("WatchProvider")
            .field("watched", &self.watched)
            .field("available", &self.is_available())
            .field("rejected", &self.rejected)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn provider_at(now: Arc<AtomicU64>) -> WatchProvider {
        WatchProvider::new(move || now.load(Ordering::SeqCst))
    }

    #[test]
    fn a_watched_path_starts_quiet_rather_than_active() {
        let directory = tempfile::tempdir().unwrap();
        let clock = Arc::new(AtomicU64::new(1_000));
        let mut provider = provider_at(clock);
        provider.watch_only(&[WatchedPath::new(directory.path()).unwrap()]);

        let mut observations = Observations::at(1_000);
        provider.sample(1_000, &mut observations);

        let activity = observations.watch_activity.unwrap();
        assert_eq!(activity.len(), 1);
        assert_eq!(
            activity[0].last_change_unix_seconds, 0,
            "a rule must not fire just because the service started watching"
        );
    }

    #[test]
    fn a_file_written_in_a_watched_directory_is_recorded_as_activity() {
        let directory = tempfile::tempdir().unwrap();
        let clock = Arc::new(AtomicU64::new(1_000));
        let mut provider = provider_at(clock.clone());
        provider.watch_only(&[WatchedPath::new(directory.path()).unwrap()]);

        clock.store(2_000, Ordering::SeqCst);
        std::fs::write(directory.path().join("download.part"), b"data").unwrap();

        // inotify delivery is asynchronous; wait for it rather than assuming.
        let mut activity = Vec::new();
        for _ in 0..100 {
            std::thread::sleep(std::time::Duration::from_millis(20));
            activity = provider.log().activity();
            if activity
                .iter()
                .any(|entry| entry.last_change_unix_seconds > 0)
            {
                break;
            }
        }

        let entry = activity
            .iter()
            .find(|entry| entry.path == directory.path())
            .expect("the watched directory must be reported");
        assert_eq!(
            entry.last_change_unix_seconds, 2_000,
            "the directory a rule names must be what the activity is recorded against"
        );
    }

    #[test]
    fn only_the_last_change_time_is_kept_and_never_what_changed() {
        let log = WatchLog::new();
        log.record(Path::new("/home/user/Documents/tax-return.pdf"), 5_000);
        let activity = log.activity();
        assert_eq!(activity.len(), 1);
        assert_eq!(activity[0].last_change_unix_seconds, 5_000);
        // The struct has exactly two fields, so there is nowhere for a file
        // name to be stored beyond the path the rule itself named.
        assert_eq!(
            activity[0],
            WatchActivity {
                path: PathBuf::from("/home/user/Documents/tax-return.pdf"),
                last_change_unix_seconds: 5_000,
            }
        );
    }

    #[test]
    fn a_path_no_rule_names_any_more_stops_being_watched() {
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        let clock = Arc::new(AtomicU64::new(1_000));
        let mut provider = provider_at(clock);

        provider.watch_only(&[
            WatchedPath::new(first.path()).unwrap(),
            WatchedPath::new(second.path()).unwrap(),
        ]);
        assert_eq!(provider.log().tracked().len(), 2);

        provider.watch_only(&[WatchedPath::new(second.path()).unwrap()]);
        assert_eq!(provider.log().tracked(), vec![second.path().to_path_buf()]);
    }

    #[test]
    fn one_unwatchable_path_does_not_take_the_others_down_with_it() {
        let good = tempfile::tempdir().unwrap();
        let clock = Arc::new(AtomicU64::new(1_000));
        let mut provider = provider_at(clock);

        let missing = WatchedPath::new("/nonexistent-better-awake-watch-target").unwrap();
        provider.watch_only(&[WatchedPath::new(good.path()).unwrap(), missing]);

        assert_eq!(
            provider.log().tracked(),
            vec![good.path().to_path_buf()],
            "the folder that exists is still watched"
        );
        assert_eq!(provider.rejected().len(), 1);
        let (path, reason) = provider.rejected().iter().next().unwrap();
        assert_eq!(path, Path::new("/nonexistent-better-awake-watch-target"));
        assert!(reason.starts_with("awake.provider.path_unwatchable"));
    }

    #[test]
    fn watching_the_same_path_twice_registers_one_watch() {
        let directory = tempfile::tempdir().unwrap();
        let clock = Arc::new(AtomicU64::new(1_000));
        let mut provider = provider_at(clock);
        let path = WatchedPath::new(directory.path()).unwrap();

        provider.watch_only(std::slice::from_ref(&path));
        provider.watch_only(&[path]);
        assert_eq!(provider.log().tracked().len(), 1);
    }

    #[test]
    fn the_watch_provider_polls_nothing() {
        let provider = provider_at(Arc::new(AtomicU64::new(0)));
        assert_eq!(provider.cadence().poll_seconds(), None);
    }
}
