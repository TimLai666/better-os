//! Seeing changes made outside Better Git while its window stays open.
//!
//! The status read itself is complete; what was missing is something that
//! asks for it. The open repository's working tree and its git directory
//! (HEAD, the index, refs, and the files of a merge, rebase, cherry-pick,
//! or revert in progress) are watched with `notify`, one non-recursive
//! watch per folder so that folders Git ignores (`node_modules`, `target`,
//! and the rest of `git ls-files --ignored`) are never watched: a large
//! project does not use up the inotify watches, and churn in a build folder
//! asks for nothing. A change asks for a status refresh about 300 ms after
//! things go quiet (at most a second after the first change while they do
//! not). When a folder cannot be watched (the watch limit, a network
//! filesystem), the window refreshes every 3 seconds instead and says so
//! once. The refresh goes through the session's queue, so it never runs
//! beside a Git operation and never two at a time.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

use git_platform::Git;
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};

/// How long the folders must be quiet before the refresh.
pub const QUIET_MS: u64 = 300;
/// The longest a refresh waits while changes keep coming.
pub const MAX_WAIT_MS: u64 = 1_000;
/// How often the window refreshes when it cannot watch.
pub const POLL_MS: u64 = 3_000;

/// The files in a git directory whose change is a change the window shows.
/// Lock files, objects, and logs are not: they change while Git works, and
/// the result arrives through one of these.
const GIT_FILES: [&str; 9] = [
    "HEAD",
    "index",
    "MERGE_HEAD",
    "CHERRY_PICK_HEAD",
    "REVERT_HEAD",
    "REBASE_HEAD",
    "ORIG_HEAD",
    "packed-refs",
    "AUTO_MERGE",
];
/// Folders in a git directory whose whole content matters.
const GIT_FOLDERS: [&str; 4] = ["refs", "rebase-merge", "rebase-apply", "sequencer"];

/// Where a repository's changes show up.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WatchPlan {
    pub root: PathBuf,
    /// This worktree's git directory.
    pub git_dir: PathBuf,
    /// The git directory every worktree shares (refs, `packed-refs`).
    pub common_dir: PathBuf,
    /// Ignored folders, absolute.
    pub ignored: Vec<PathBuf>,
}

impl WatchPlan {
    pub fn new(root: &Path, git_dir: PathBuf, common_dir: PathBuf, ignored: &[PathBuf]) -> Self {
        WatchPlan {
            root: root.to_path_buf(),
            ignored: ignored.iter().map(|dir| root.join(dir)).collect(),
            git_dir,
            common_dir,
        }
    }

    fn in_git_dir(&self, path: &Path) -> Option<PathBuf> {
        [&self.git_dir, &self.common_dir]
            .iter()
            .find_map(|dir| path.strip_prefix(dir).ok().map(Path::to_path_buf))
    }

    fn ignored(&self, path: &Path) -> bool {
        self.ignored.iter().any(|dir| path.starts_with(dir))
    }

    /// Whether a change at `path` changes what the window shows.
    pub fn matters(&self, path: &Path) -> bool {
        if let Some(inside) = self.in_git_dir(path) {
            if inside
                .extension()
                .is_some_and(|extension| extension == "lock")
            {
                return false;
            }
            let mut parts = inside.components();
            let first = parts
                .next()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .unwrap_or_default();
            let only = parts.next().is_none();
            return (only && GIT_FILES.contains(&first.as_str()))
                || GIT_FOLDERS.contains(&first.as_str());
        }
        // A `.git` that is not this repository's own git directory: a
        // nested repository's, or a linked worktree's pointer file.
        if path.components().any(|c| c.as_os_str() == ".git") {
            return false;
        }
        path.starts_with(&self.root) && !self.ignored(path)
    }

    /// The folders to watch: every folder of the working tree that Git does
    /// not ignore, one watch each; the git directories themselves; and the
    /// refs, recursively, since branches nest (`feature/x`).
    pub fn folders(&self) -> Vec<(PathBuf, RecursiveMode)> {
        let mut folders = Vec::new();
        let mut stack = vec![self.root.clone()];
        while let Some(dir) = stack.pop() {
            if self.ignored(&dir) || self.in_git_dir(&dir).is_some() {
                continue;
            }
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            folders.push((dir.clone(), RecursiveMode::NonRecursive));
            for entry in entries.flatten() {
                let Ok(kind) = entry.file_type() else {
                    continue;
                };
                // Not through links: they can lead out of the repository or
                // round in a circle.
                if kind.is_dir() && entry.file_name() != ".git" {
                    stack.push(entry.path());
                }
            }
        }
        folders.push((self.git_dir.clone(), RecursiveMode::NonRecursive));
        if self.common_dir != self.git_dir {
            folders.push((self.common_dir.clone(), RecursiveMode::NonRecursive));
        }
        let refs = self.common_dir.join("refs");
        if refs.is_dir() {
            folders.push((refs, RecursiveMode::Recursive));
        }
        // `info/exclude` is ignore rules too.
        let info = self.common_dir.join("info");
        if info.is_dir() {
            folders.push((info, RecursiveMode::NonRecursive));
        }
        folders
    }
}

/// When to refresh, from when changes arrived: once `QUIET_MS` passed with
/// none, or `MAX_WAIT_MS` after the first while they keep coming.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Debounce {
    first: Option<u64>,
    last: u64,
}

impl Debounce {
    pub fn changed(&mut self, now: u64) {
        if self.first.is_none() {
            self.first = Some(now);
        }
        self.last = now;
    }

    /// Whether the refresh is due now; answering yes starts over.
    pub fn due(&mut self, now: u64) -> bool {
        let Some(first) = self.first else {
            return false;
        };
        let due =
            now.saturating_sub(self.last) >= QUIET_MS || now.saturating_sub(first) >= MAX_WAIT_MS;
        if due {
            *self = Debounce::default();
        }
        due
    }

    pub fn waiting(&self) -> bool {
        self.first.is_some()
    }
}

/// What came out of the watcher since the last look.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Seen {
    /// Something that matters changed.
    pub changed: bool,
    /// A folder appeared in the working tree; the plan is read again so it
    /// is watched (or, if Git ignores it, left alone).
    pub new_folder: bool,
    /// A `.gitignore` or `info/exclude` changed: what is ignored is read
    /// again.
    pub ignore_rules_changed: bool,
    /// The kernel dropped events; refresh to be sure.
    pub overflowed: bool,
}

impl Seen {
    fn merge(&mut self, other: Seen) {
        self.changed |= other.changed;
        self.new_folder |= other.new_folder;
        self.ignore_rules_changed |= other.ignore_rules_changed;
        self.overflowed |= other.overflowed;
    }

    /// Whether the plan of what to watch must be read again.
    pub fn replan(&self) -> bool {
        self.new_folder || self.ignore_rules_changed
    }
}

/// One change, sorted before Git is asked about ignored files.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Batch {
    pub seen: Seen,
    /// Working-tree paths that changed, for `git check-ignore`.
    pub candidates: Vec<PathBuf>,
    /// The folders among them that are new.
    pub new_folders: Vec<PathBuf>,
}

impl WatchPlan {
    /// Sorts one event's paths into the batch.
    pub fn sort(
        &self,
        kind: &EventKind,
        paths: &[PathBuf],
        is_dir: impl Fn(&Path) -> bool,
        batch: &mut Batch,
    ) {
        if matches!(kind, EventKind::Access(_)) {
            return;
        }
        for path in paths {
            if let Some(inside) = self.in_git_dir(path)
                && inside == Path::new("info/exclude")
            {
                batch.seen.ignore_rules_changed = true;
                batch.seen.changed = true;
                continue;
            }
            if !self.matters(path) {
                continue;
            }
            if self.in_git_dir(path).is_some() {
                batch.seen.changed = true;
                continue;
            }
            if path.file_name().is_some_and(|name| name == ".gitignore") {
                batch.seen.ignore_rules_changed = true;
                batch.seen.changed = true;
                continue;
            }
            if matches!(kind, EventKind::Create(_)) && is_dir(path) {
                batch.new_folders.push(path.clone());
            }
            batch.candidates.push(path.clone());
        }
    }
}

/// Settles a batch once Git said which of its paths it ignores: only a
/// change to a file Git does not ignore asks for a refresh.
pub fn settle(mut batch: Batch, ignored: &[PathBuf]) -> Seen {
    let kept = |path: &PathBuf| !ignored.iter().any(|i| path.starts_with(i));
    if batch.candidates.iter().any(kept) {
        batch.seen.changed = true;
    }
    if batch.new_folders.iter().any(kept) {
        batch.seen.new_folder = true;
    }
    batch.seen
}

/// A running watch of one repository. Dropping it stops the watch and the
/// thread that sorts its events.
pub struct Watching {
    pub plan: WatchPlan,
    _watcher: RecommendedWatcher,
    seen: Receiver<Seen>,
}

impl Watching {
    /// Reads what arrived, without waiting.
    pub fn take(&self) -> Seen {
        let mut seen = Seen::default();
        for batch in self.seen.try_iter() {
            seen.merge(batch);
        }
        seen
    }
}

/// Why a repository is not being watched.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NotWatched {
    /// Git could not say where the repository's folders are.
    Git(String),
    /// A folder could not be watched: the limit, or a filesystem with no
    /// change events.
    Watch(String),
}

/// How long the sorting thread gathers events before it asks Git about
/// the files among them, once the first one arrived.
const GATHER: Duration = Duration::from_millis(40);

/// Sets up the watch. Runs off the painting thread: it asks Git twice and
/// walks the working tree. A thread sorts the events: it gathers them
/// briefly, asks `git check-ignore` once for the files among them, and
/// passes on only what matters.
pub fn start(git: Arc<dyn Git>, root: &Path) -> Result<Watching, NotWatched> {
    let (git_dir, common_dir) = git
        .git_dirs(root)
        .map_err(|error| NotWatched::Git(error.to_string()))?;
    let ignored = git
        .ignored_directories(root)
        .map_err(|error| NotWatched::Git(error.to_string()))?;
    let plan = WatchPlan::new(root, git_dir, common_dir, &ignored);
    let (raw_sender, raw) = channel::<notify::Result<notify::Event>>();
    let mut watcher = notify::recommended_watcher(move |event| {
        let _ = raw_sender.send(event);
    })
    .map_err(|error| NotWatched::Watch(error.to_string()))?;
    for (folder, mode) in plan.folders() {
        watcher
            .watch(&folder, mode)
            .map_err(|error| NotWatched::Watch(format!("{}: {error}", folder.display())))?;
    }
    let (seen_sender, seen) = channel();
    let sorter_plan = plan.clone();
    let root = root.to_path_buf();
    std::thread::Builder::new()
        .name("better-git-watch".into())
        .spawn(move || {
            // Ends when the watcher (and with it the sender) is dropped.
            while let Ok(first) = raw.recv() {
                let mut batch = Batch::default();
                let mut next = Some(first);
                while let Some(event) = next.take() {
                    match event {
                        Ok(event) => {
                            if event.need_rescan() {
                                batch.seen.overflowed = true;
                            }
                            sorter_plan.sort(&event.kind, &event.paths, Path::is_dir, &mut batch);
                        }
                        Err(_) => batch.seen.overflowed = true,
                    }
                    next = raw.recv_timeout(GATHER).ok();
                }
                let ignored = git
                    .ignored_paths(&root, &batch.candidates)
                    .unwrap_or_default();
                let seen = settle(batch, &ignored);
                if seen != Seen::default() && seen_sender.send(seen).is_err() {
                    return;
                }
            }
        })
        .map_err(|error| NotWatched::Watch(error.to_string()))?;
    Ok(Watching {
        plan,
        _watcher: watcher,
        seen,
    })
}

/// The window's side: which repository is watched, how, and when to
/// refresh. The window owns one and drives it from a short timer.
#[derive(Default)]
pub struct Live {
    /// The repository the watch is for, set when its setup starts.
    pub root: Option<PathBuf>,
    pub watching: Option<Watching>,
    /// The watch could not be set up: refresh on a timer instead.
    pub polling: bool,
    /// When the last timed refresh ran.
    pub last_poll: u64,
    /// The fallback has been mentioned for this window.
    pub told: bool,
    /// A setup is running in the background.
    pub starting: bool,
    pub debounce: Debounce,
    /// Set up again (after a new folder) once the refresh is done.
    pub restart: bool,
    /// The timer that reads the watch is running.
    pub ticking: bool,
}

impl Live {
    /// What the watch decides this tick: whether to refresh, and whether to
    /// set the watch up again.
    pub fn tick(&mut self, now: u64) -> (bool, bool) {
        if let Some(watching) = self.watching.as_ref() {
            let seen = watching.take();
            if seen.changed || seen.overflowed {
                self.debounce.changed(now);
            }
            if seen.replan() {
                self.restart = true;
            }
        }
        let mut refresh = self.debounce.due(now);
        if self.polling && now.saturating_sub(self.last_poll) >= POLL_MS {
            self.last_poll = now;
            refresh = true;
        }
        let restart = refresh && std::mem::take(&mut self.restart);
        (refresh, restart)
    }
}
