//! The Graph tab's rules, with no GPUI in them: the commits and lanes
//! loaded so far, reading the next page off the window's thread, the
//! filter, the labels a row shows, the selection, finding a commit, and
//! keeping the place across a refresh.
//!
//! The graph reads one `git log --topo-order` for as long as it is shown
//! ([`git_platform::graph`]) and lays its commits out with
//! [`git_core::LaneLayout`]. Both live in a [`Feed`] that a [`GraphJob`]
//! takes to a background thread, reads a page from, and hands back; the
//! layout's state is in the feed, so each page carries on from the one
//! before. A refresh opens a new feed and reads as many commits as were
//! shown, then swaps the list in one step, so the rows never empty out
//! under the person.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use git_core::{Commit, CommitFile, GraphRows, LaneLayout, RefKind, RefLabel, RepositoryStatus};
use git_platform::{Git, GitError, GraphQuery, GraphScope, GraphSource, RepositoryInfo};

use crate::i18n::Locale;

/// How many commits one page reads.
pub const GRAPH_PAGE: usize = 3_000;
/// How close to the last loaded row the list may come before the next page
/// is asked for.
pub const PREFETCH_ROWS: usize = 600;
/// How many lanes the graph column shows until the person drags its edge.
pub const AUTO_LANES: u16 = 12;

/// How many lanes the graph column shows: what the person dragged it to,
/// or [`AUTO_LANES`], and never more than the widest row needs or fewer
/// than one. Lanes past it are marked at the column's edge, not lost.
pub fn shown_lanes(columns: u16, chosen: Option<u16>) -> u16 {
    chosen.unwrap_or(AUTO_LANES).min(columns).max(1)
}

/// The lane count a drag of the column's edge to `offset` pixels from the
/// column's left edge asks for, rounded to the nearest lane and kept
/// between one and `columns`.
pub fn lanes_at(offset: f32, lane_width: f32, columns: u16) -> u16 {
    let lanes = (offset / lane_width.max(1.0)).round();
    (lanes.max(1.0) as u16).min(columns.max(1))
}

/// How many labels of `widths` (in order) fit in `room` pixels with `gap`
/// between them. When not all of them fit, room is kept for a "+N" chip
/// `more` wide after the ones shown; a label is either shown whole or not
/// at all.
pub fn fit_labels(widths: &[f32], room: f32, more: f32, gap: f32) -> usize {
    let total: f32 = widths.iter().sum::<f32>() + gap * widths.len().saturating_sub(1) as f32;
    if total <= room {
        return widths.len();
    }
    let room = room - more - gap;
    let mut used = 0.0;
    let mut shown = 0;
    for width in widths {
        let next = used + width + if shown > 0 { gap } else { 0.0 };
        if next > room {
            break;
        }
        used = next;
        shown += 1;
    }
    shown
}

/// A replaced list this long is freed on a thread of its own, so dropping
/// 100,000 commits never holds up a frame.
const DROP_ELSEWHERE: usize = 10_000;

/// A `git log` being read and the lane layout of what it printed so far.
#[derive(Default)]
pub struct Feed {
    source: Option<Box<dyn GraphSource>>,
    layout: LaneLayout,
    /// Commits read so far: where a stopped `git log` picks up again.
    read: usize,
    /// Every commit has been read.
    exhausted: bool,
    /// The commits the walk started from ([`Git::graph_tips`]), pinned at
    /// the first page so a walk started again after a pause is the same walk.
    tips: Option<Vec<String>>,
}

#[derive(Default)]
struct Shared {
    feed: Mutex<Feed>,
    /// The tab was hidden: `git log` is stopped after the read in progress,
    /// and started again from `read` when the next page is asked for.
    paused: AtomicBool,
}

/// A feed shared between the session and the job reading it. Equal only to
/// itself, so a job knows which feed it was asked for.
#[derive(Clone, Default)]
pub struct FeedHandle(Arc<Shared>);

impl FeedHandle {
    pub fn new() -> Self {
        Self::default()
    }

    /// Stops `git log` now when no read is running, or after the one that
    /// is. What was read, and the lanes, are kept.
    pub fn pause(&self) {
        self.0.paused.store(true, Ordering::SeqCst);
        if let Ok(mut feed) = self.0.feed.try_lock() {
            feed.source = None;
        }
    }

    pub fn resume(&self) {
        self.0.paused.store(false, Ordering::SeqCst);
    }

    /// Whether a `git log` is running for this feed, for tests.
    pub fn is_reading(&self) -> bool {
        self.0
            .feed
            .lock()
            .map(|f| f.source.is_some())
            .unwrap_or(false)
    }
}

impl PartialEq for FeedHandle {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for FeedHandle {}

impl std::fmt::Debug for FeedHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("FeedHandle")
    }
}

/// Reading the graph's next commits.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GraphJob {
    pub generation: u64,
    pub stream: u64,
    pub repository: PathBuf,
    pub query: GraphQuery,
    pub feed: FeedHandle,
    /// Read at least this many commits (fewer only at the end).
    pub want: usize,
    /// The answer replaces the list rather than adding to it.
    pub replace: bool,
}

/// What a page read, laid out and ready to add.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct GraphPage {
    pub commits: Vec<Commit>,
    pub rows: GraphRows,
    /// The page's part of the hash-to-row index, built off the window's
    /// thread: the whole index for a replacing page, the entries to add for
    /// any other.
    pub index: Option<HashMap<String, usize>>,
    pub end: bool,
    pub replace: bool,
    /// The walk was to start again from where it stopped, but a branch,
    /// tag, or HEAD has moved since it began: carrying on would repeat or
    /// skip commits, so the graph is read again from the top instead.
    pub moved: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GraphOutcome {
    pub generation: u64,
    pub stream: u64,
    pub result: Result<GraphPage, GitError>,
}

impl GraphJob {
    /// Starts the feed's `git log` when none is running (the first read, or
    /// after the tab was hidden, from where it stopped), then reads and lays
    /// out pages until `want` commits are read or the history ends. Safe on
    /// any thread; the feed's lock is held for the whole read, so two jobs
    /// on one feed take turns.
    pub fn run(self, git: &dyn Git) -> GraphOutcome {
        let result = (|| {
            let mut feed = self.feed.0.feed.lock().unwrap_or_else(|p| p.into_inner());
            let mut page = GraphPage {
                replace: self.replace,
                rows: GraphRows::starting_at(feed.read, feed.layout.tops()),
                ..GraphPage::default()
            };
            if feed.source.is_none() && !feed.exhausted {
                let tips = git.graph_tips(&self.repository, &self.query)?;
                match &feed.tips {
                    Some(pinned) if *pinned != tips => {
                        page.moved = true;
                        return Ok(page);
                    }
                    Some(_) => {}
                    None => feed.tips = Some(tips),
                }
                let tips = feed.tips.clone().unwrap_or_default();
                feed.source = Some(git.graph(&self.repository, &self.query, &tips, feed.read)?);
            }
            let Feed {
                source,
                layout,
                read,
                exhausted,
                ..
            } = &mut *feed;
            if let Some(source) = source.as_mut() {
                while page.commits.len() < self.want && !source.is_done() {
                    let asked = (self.want - page.commits.len()).min(GRAPH_PAGE);
                    let commits = source.next_page(asked)?;
                    if commits.is_empty() {
                        break;
                    }
                    for commit in &commits {
                        page.rows.push(&layout.push(&commit.sha, &commit.parents));
                    }
                    *read += commits.len();
                    page.commits.extend(commits);
                }
                *exhausted = source.is_done();
            }
            page.end = feed.exhausted;
            if page.end || self.feed.0.paused.load(Ordering::SeqCst) {
                // git has exited, or the tab is hidden: nothing to hold on to.
                feed.source = None;
            }
            let first = if self.replace {
                0
            } else {
                feed.read - page.commits.len()
            };
            page.index = Some(
                page.commits
                    .iter()
                    .enumerate()
                    .map(|(i, c)| (c.sha.clone(), first + i))
                    .collect(),
            );
            Ok(page)
        })();
        GraphOutcome {
            generation: self.generation,
            stream: self.stream,
            result,
        }
    }
}

/// Which way "find" steps.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Direction {
    Next,
    Previous,
}

/// Why a drag that would reorder or squash was refused.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Refusal {
    /// One of the two commits is not on the current branch's own line.
    NotOnBranch,
    /// The commits are pushed already: rewriting them rewrites what others
    /// have.
    Pushed,
    /// A merge commit lies between them.
    Merge,
}

/// Whether History's rules allow reordering or squashing the commits at
/// `from` and `to` in History's list (newest first), and why not.
/// `ahead` is how many commits the branch has that its upstream does not;
/// `None` when it has no upstream. The rule is
/// [`crate::history::can_rewrite`] for the older of the two.
pub fn rewrite_refusal(
    history: &[Commit],
    from: Option<usize>,
    to: Option<usize>,
    ahead: Option<u32>,
) -> Option<Refusal> {
    let (Some(from), Some(to)) = (from, to) else {
        return Some(Refusal::NotOnBranch);
    };
    let oldest = from.max(to);
    if oldest >= history.len() {
        return Some(Refusal::NotOnBranch);
    }
    if history[..=oldest].iter().any(Commit::is_merge) {
        return Some(Refusal::Merge);
    }
    if ahead.is_some_and(|ahead| oldest >= ahead as usize) {
        return Some(Refusal::Pushed);
    }
    None
}

/// What a find came to.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Found {
    At(usize),
    /// Nothing more in what is loaded; the next page may have it.
    NeedMore,
    Nothing,
}

/// The Graph tab.
#[derive(Clone, Debug, Default)]
pub struct GraphView {
    pub query: GraphQuery,
    pub commits: Vec<Commit>,
    pub rows: GraphRows,
    /// The first page has arrived.
    pub loaded: bool,
    /// Every commit has been read.
    pub end: bool,
    /// A page is being read.
    pub loading: bool,
    /// The selected commit, by hash, so it survives a refresh.
    pub selected: Option<String>,
    pub files: Vec<CommitFile>,
    pub file: Option<usize>,
    /// The widest row loaded, in columns.
    pub columns: u16,
    /// Counts the times the list was replaced, for the window to find its
    /// place again.
    pub replaced: u64,
    /// Repository state the list was read at ([`fingerprint`]); a refresh
    /// that changes it reads the graph again.
    pub fingerprint: Option<u64>,
    /// Read again the next time the tab is shown.
    pub stale: bool,
    /// The find field.
    pub search: String,
    pub search_open: bool,
    /// A find waiting for the next page.
    pub search_pending: Option<Direction>,
    /// The commit the window should scroll to, taken once.
    pub scroll_to: Option<usize>,
    /// A find that came to nothing, for the field to say so.
    pub not_found: bool,
    /// The last drag refused, for the pane to explain.
    pub refusal: Option<Refusal>,
    /// What was read is no longer the history to carry on from: read it
    /// again ([`GraphPage::moved`]). The session clears it.
    pub moved: bool,
    stream: u64,
    feed: Option<FeedHandle>,
    index: HashMap<String, usize>,
}

impl GraphView {
    pub fn new(query: GraphQuery) -> Self {
        Self {
            query,
            ..Self::default()
        }
    }

    /// A new `git log` for the current filter, to replace the list when it
    /// arrives. With `keep`, it reads at least as many commits as are shown
    /// now (all of them when the whole history was shown), so a refresh
    /// leaves the person where they were; without, one page.
    pub fn start(&mut self, generation: u64, repository: PathBuf, keep: bool) -> GraphJob {
        let want = match (keep, self.end) {
            (true, true) => usize::MAX,
            (true, false) => self.commits.len().max(GRAPH_PAGE),
            (false, _) => GRAPH_PAGE,
        };
        self.stream += 1;
        let feed = FeedHandle::new();
        self.feed = Some(feed.clone());
        self.loading = true;
        self.stale = false;
        GraphJob {
            generation,
            stream: self.stream,
            repository,
            query: self.query,
            feed,
            want,
            replace: true,
        }
    }

    /// The next page, when there is more and none is being read.
    pub fn more(&mut self, generation: u64, repository: PathBuf) -> Option<GraphJob> {
        if self.end || self.loading || !self.loaded {
            return None;
        }
        let feed = self.feed.clone()?;
        self.loading = true;
        Some(GraphJob {
            generation,
            stream: self.stream,
            repository,
            query: self.query,
            feed,
            want: GRAPH_PAGE,
            replace: false,
        })
    }

    /// Whether the rows shown up to `last_visible` are close enough to the
    /// end of what is loaded to read the next page.
    pub fn wants_more(&self, last_visible: usize) -> bool {
        self.loaded
            && !self.end
            && !self.loading
            && last_visible + PREFETCH_ROWS >= self.commits.len()
    }

    /// A page arrived. Pages of a stream that was replaced meanwhile are
    /// dropped; `false` then.
    ///
    /// Expected results:
    /// - A replacing page swaps the whole list in, keeps the selection when
    ///   its commit is still there (and clears it and its files when not),
    ///   and counts one more replacement.
    /// - Any other page is added at the end.
    /// - A failure ends the stream: what is loaded stays, and nothing more
    ///   is asked for until the graph is read again.
    pub fn apply(&mut self, outcome: GraphOutcome) -> Result<bool, GitError> {
        if outcome.stream != self.stream {
            return Ok(false);
        }
        self.loading = false;
        let page = match outcome.result {
            Ok(page) if page.moved => {
                self.moved = true;
                return Ok(true);
            }
            Ok(page) => page,
            Err(error) => {
                self.end = true;
                self.feed = None;
                self.loaded = true;
                return Err(error);
            }
        };
        if page.replace {
            let old_commits = std::mem::replace(&mut self.commits, page.commits);
            let old_rows = std::mem::replace(&mut self.rows, page.rows);
            let index = page.index.unwrap_or_else(|| {
                self.commits
                    .iter()
                    .enumerate()
                    .map(|(i, c)| (c.sha.clone(), i))
                    .collect()
            });
            let old_index = std::mem::replace(&mut self.index, index);
            if old_commits.len() >= DROP_ELSEWHERE {
                std::thread::spawn(move || drop((old_commits, old_rows, old_index)));
            }
            self.replaced += 1;
            if self
                .selected
                .as_ref()
                .is_some_and(|sha| !self.index.contains_key(sha))
            {
                self.selected = None;
                self.files.clear();
                self.file = None;
            }
        } else {
            let start = self.commits.len();
            match page.index {
                Some(entries) => self.index.extend(entries),
                None => {
                    for (i, commit) in page.commits.iter().enumerate() {
                        self.index.insert(commit.sha.clone(), start + i);
                    }
                }
            }
            self.commits.extend(page.commits);
            self.rows.append(page.rows);
        }
        self.columns = self.rows.widest().max(1);
        self.end = page.end;
        if self.end {
            self.feed = None;
        }
        self.loaded = true;
        Ok(true)
    }

    /// Drops the list and its `git log`; the next show reads it again.
    pub fn clear(&mut self) {
        let query = self.query;
        let stream = self.stream + 1;
        *self = GraphView::new(query);
        self.stream = stream;
    }

    /// The tab was hidden: stop `git log` (after a read in progress).
    pub fn pause(&self) {
        if let Some(feed) = &self.feed {
            feed.pause();
        }
    }

    /// The tab is shown again: the next page starts `git log` again from
    /// where it stopped.
    pub fn resume(&self) {
        if let Some(feed) = &self.feed {
            feed.resume();
        }
    }

    /// The feed being read, for tests.
    pub fn feed(&self) -> Option<&FeedHandle> {
        self.feed.as_ref()
    }

    /// Whether `sha` is HEAD or one of its ancestors: the commits a reset
    /// of the current branch or a revert applies to. `head` is HEAD's
    /// commit. Every descendant of a loaded commit is loaded before it
    /// (topological order), so walking parents from HEAD through what is
    /// loaded reaches `sha` exactly when it is an ancestor.
    pub fn is_ancestor_of(&self, sha: &str, head: &str) -> bool {
        let (Some(target), Some(start)) = (self.index_of(sha), self.index_of(head)) else {
            return false;
        };
        if target < start {
            return false;
        }
        let mut seen = vec![false; target - start + 1];
        let mut stack = vec![start];
        while let Some(at) = stack.pop() {
            if at == target {
                return true;
            }
            if at > target || seen[at - start] {
                continue;
            }
            seen[at - start] = true;
            for parent in &self.commits[at].parents {
                if let Some(i) = self.index_of(parent)
                    && i <= target
                {
                    stack.push(i);
                }
            }
        }
        false
    }

    pub fn index_of(&self, sha: &str) -> Option<usize> {
        self.index.get(sha).copied()
    }

    pub fn selected_index(&self) -> Option<usize> {
        self.selected.as_deref().and_then(|sha| self.index_of(sha))
    }

    pub fn selected_commit(&self) -> Option<&Commit> {
        self.selected_index().and_then(|i| self.commits.get(i))
    }

    /// The labels the row at `index` shows under the filter.
    pub fn labels(&self, index: usize) -> Vec<RefLabel> {
        let Some(commit) = self.commits.get(index) else {
            return Vec::new();
        };
        labels_for(commit, &self.query)
    }

    /// Steps the find from the selected commit.
    ///
    /// Expected results:
    /// - Forward, a match below the selection is found first; with nothing
    ///   below it and more to read, the answer is `NeedMore`, and only at
    ///   the end of the history does the find wrap to the top.
    /// - Backward, a match above the selection is found first, then the
    ///   find wraps to the bottom of what is loaded.
    /// - A blank query finds nothing.
    pub fn find(&self, direction: Direction) -> Found {
        if self.search.trim().is_empty() {
            return Found::Nothing;
        }
        let from = self.selected_index();
        match direction {
            Direction::Next => {
                let start = from.map_or(0, |i| i + 1);
                if let Some(i) = (start..self.commits.len())
                    .find(|&i| git_core::graph::matches(&self.commits[i], &self.search))
                {
                    return Found::At(i);
                }
                if !self.end {
                    return Found::NeedMore;
                }
                git_core::graph::find_next(&self.commits, &self.search, from)
                    .map_or(Found::Nothing, Found::At)
            }
            Direction::Previous => {
                git_core::graph::find_previous(&self.commits, &self.search, from)
                    .map_or(Found::Nothing, Found::At)
            }
        }
    }

    /// Where the list's top row is after a replacement: the row of the
    /// commit that was at the top before, or the nearest row above it that
    /// survived, or the top.
    pub fn place_of(&self, previous_top: &[String]) -> usize {
        previous_top
            .iter()
            .find_map(|sha| self.index_of(sha))
            .unwrap_or(0)
    }
}

/// A commit's labels, as the filter shows them: remote branches and tags
/// only when they are shown.
pub fn labels_for(commit: &Commit, query: &GraphQuery) -> Vec<RefLabel> {
    git_core::ref_labels(&commit.refs)
        .into_iter()
        .filter(|l| match l.kind {
            RefKind::Remote => query.remotes,
            RefKind::Tag => query.tags,
            RefKind::Head | RefKind::Local => true,
        })
        .collect()
}

/// What the graph depends on: HEAD, and every branch's and tag's commit.
/// A refresh that leaves it unchanged leaves the graph alone.
pub fn fingerprint(status: &RepositoryStatus, info: &RepositoryInfo) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    status.head_oid.hash(&mut hasher);
    format!("{:?}", status.head).hash(&mut hasher);
    for branch in &info.branches {
        branch.full_ref.hash(&mut hasher);
        branch.sha.hash(&mut hasher);
    }
    for tag in &info.tags {
        tag.name.hash(&mut hasher);
        tag.commit.hash(&mut hasher);
    }
    hasher.finish()
}

/// Reads the stored filter.
pub fn query_from(scope: &str, remotes: bool, tags: bool) -> GraphQuery {
    GraphQuery {
        scope: GraphScope::from_name(scope).unwrap_or_default(),
        remotes,
        tags,
    }
}

macro_rules! graph_strings {
    ($($name:ident: $en:expr, $zh:expr;)*) => {
        /// Every word the Graph tab shows.
        pub struct GraphCopy {
            $(pub $name: &'static str,)*
        }

        pub static GRAPH_EN_US: GraphCopy = GraphCopy { $($name: $en,)* };
        pub static GRAPH_ZH_TW: GraphCopy = GraphCopy { $($name: $zh,)* };

        impl GraphCopy {
            /// Every string, for the test that no field is empty.
            pub fn all(&self) -> Vec<&'static str> {
                vec![$(self.$name,)*]
            }
        }
    };
}

graph_strings! {
    tab: "Graph", "分支圖";
    scope: "Show", "顯示";
    scope_all: "All branches", "所有分支";
    scope_local: "Local branches", "本機分支";
    scope_current: "Current branch", "目前的分支";
    remotes: "Remote branches", "遠端分支";
    tags: "Tags", "標籤";
    find: "Find", "尋找";
    find_placeholder: "Subject, author, or hash", "主旨、作者或雜湊值";
    find_next: "Next match", "下一個";
    find_previous: "Previous match", "上一個";
    find_close: "Close find", "關閉尋找";
    not_found: "No commit matches", "沒有相符的提交";
    searching: "Searching older commits…", "正在搜尋較舊的提交…";
    loading: "Reading the history…", "正在讀取歷程記錄…";
    loading_more: "Reading more commits…", "正在讀取更多提交…";
    count: "{n} commits", "{n} 個提交";
    count_more: "{n} commits so far", "目前 {n} 個提交";
    empty: "No commits to show", "沒有可以顯示的提交";
    detached: "HEAD", "HEAD";
    drag_hint: "Drag a commit onto a branch to cherry-pick it. Your current branch's own commits that are not pushed yet, with no merge between, can be dragged to the top edge of another to reorder or onto it to squash.", "把提交拖到分支上可摘取。目前分支上還沒推送、中間沒有合併的提交，拖到另一個提交的上緣可重新排序，拖到它上面可壓縮。";
    refused_title: "Can't reorder or squash these commits", "無法重新排序或壓縮這些提交";
    refused_branch: "Only commits on the current branch's own line can be reordered or squashed.", "只有目前分支本身那條線上的提交可以重新排序或壓縮。";
    refused_pushed: "They are already pushed. Reordering or squashing them would rewrite history others already have.", "這些提交已經推送了，重新排序或壓縮會改寫別人已經拿到的歷程記錄。";
    refused_merge: "A merge commit lies between them, and a rewrite cannot go across a merge.", "它們之間有合併提交，改寫無法跨過合併。";
    find_done: "Done", "完成";
    lanes_hint: "Drag to show more or fewer lanes", "拖曳來顯示更多或更少的分支線";
    select_hint: "Select a commit to see its changes", "選取一個提交來查看變更";
    shortcut_find: "Find in graph", "在分支圖中尋找";
}

/// The graph's words in the language of the window's other words.
pub fn graph_copy_for(c: &crate::i18n::Copy) -> &'static GraphCopy {
    if std::ptr::eq(c, &crate::i18n::ZH_TW) {
        &GRAPH_ZH_TW
    } else {
        &GRAPH_EN_US
    }
}

impl GraphCopy {
    /// Why a drag was refused, in a sentence.
    pub fn refusal(&self, refusal: Refusal) -> &'static str {
        match refusal {
            Refusal::NotOnBranch => self.refused_branch,
            Refusal::Pushed => self.refused_pushed,
            Refusal::Merge => self.refused_merge,
        }
    }
}

pub fn graph_copy(locale: Locale) -> &'static GraphCopy {
    match locale {
        Locale::ZhTw => &GRAPH_ZH_TW,
        _ => &GRAPH_EN_US,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Direction, FeedHandle, Found, GRAPH_EN_US, GRAPH_PAGE, GRAPH_ZH_TW, GraphJob, GraphOutcome,
        GraphPage, GraphView, fingerprint, labels_for, query_from,
    };
    use git_core::{Branch, BranchHead, Commit, GraphRow, GraphRows, RefKind, RepositoryStatus};
    use git_platform::{
        Git, GitError, GraphQuery, GraphScope, GraphSource, MemorySource, RepositoryInfo,
    };
    use std::path::{Path, PathBuf};

    fn commit(i: usize, parents: &[usize], refs: &[&str], summary: &str) -> Commit {
        Commit {
            sha: format!("{i:040x}"),
            parents: parents.iter().map(|p| format!("{p:040x}")).collect(),
            author_name: format!("Author {}", i % 3),
            author_email: String::new(),
            author_time: 0,
            refs: refs.iter().map(|r| r.to_string()).collect(),
            summary: summary.to_string(),
            body: String::new(),
        }
    }

    /// A line of `n` commits, newest (0) first.
    fn line(n: usize) -> Vec<Commit> {
        (0..n)
            .map(|i| {
                let parents: Vec<usize> = if i + 1 < n { vec![i + 1] } else { vec![] };
                commit(i, &parents, &[], &format!("commit {i}"))
            })
            .collect()
    }

    struct Fake(Vec<Commit>, std::sync::Mutex<Vec<String>>);

    fn fake(commits: Vec<Commit>) -> Fake {
        Fake(commits, std::sync::Mutex::new(vec!["tip".into()]))
    }

    impl Git for Fake {
        fn repository_root(&self, path: &Path) -> Result<PathBuf, GitError> {
            Ok(path.to_path_buf())
        }
        fn status(&self, _: &Path) -> Result<RepositoryStatus, GitError> {
            Err(GitError::Unsupported("status"))
        }
        fn diff(
            &self,
            _: &Path,
            _: &Path,
            _: git_platform::DiffSide,
        ) -> Result<git_core::FileDiff, GitError> {
            Err(GitError::Unsupported("diff"))
        }
        fn stage(&self, _: &Path, _: &[PathBuf]) -> Result<(), GitError> {
            Err(GitError::Unsupported("stage"))
        }
        fn unstage(&self, _: &Path, _: &[PathBuf]) -> Result<(), GitError> {
            Err(GitError::Unsupported("unstage"))
        }
        fn commit(&self, _: &Path, _: &git_platform::CommitMessage) -> Result<(), GitError> {
            Err(GitError::Unsupported("commit"))
        }
        fn graph_tips(&self, _: &Path, _: &GraphQuery) -> Result<Vec<String>, GitError> {
            Ok(self.1.lock().unwrap().clone())
        }
        fn graph(
            &self,
            _: &Path,
            _: &GraphQuery,
            _: &[String],
            skip: usize,
        ) -> Result<Box<dyn GraphSource>, GitError> {
            Ok(Box::new(MemorySource::new(
                self.0.iter().skip(skip).cloned().collect(),
            )))
        }
    }

    fn rows_of(rows: Vec<GraphRow>) -> GraphRows {
        let mut stored = GraphRows::new();
        for row in &rows {
            stored.push(row);
        }
        stored
    }

    fn run(view: &mut GraphView, job: GraphJob, git: &dyn Git) {
        let outcome = job.run(git);
        view.apply(outcome).unwrap();
    }

    #[test]
    fn the_first_page_then_the_next_carry_the_lanes_on() {
        let git = fake(line(GRAPH_PAGE * 2 + 10));
        let mut view = GraphView::default();
        let job = view.start(1, PathBuf::from("/r"), true);
        assert!(view.loading);
        assert!(
            view.more(1, PathBuf::from("/r")).is_none(),
            "one page at a time"
        );
        run(&mut view, job, &git);
        assert_eq!(view.commits.len(), GRAPH_PAGE);
        assert!(!view.end && view.loaded && !view.loading);
        assert!(view.wants_more(GRAPH_PAGE - 1));
        assert!(!view.wants_more(0));
        let job = view.more(1, PathBuf::from("/r")).unwrap();
        run(&mut view, job, &git);
        let job = view.more(1, PathBuf::from("/r")).unwrap();
        run(&mut view, job, &git);
        assert_eq!(view.commits.len(), GRAPH_PAGE * 2 + 10);
        assert!(view.end);
        assert!(view.more(1, PathBuf::from("/r")).is_none());
        // One straight line across all three pages: the layout was carried.
        assert!(
            view.rows
                .rows(0..view.commits.len())
                .iter()
                .all(|r| r.column == 0 && r.lane == 0)
        );
        assert_eq!(
            view.index_of(&format!("{:040x}", GRAPH_PAGE + 5)),
            Some(GRAPH_PAGE + 5)
        );
    }

    #[test]
    fn a_refresh_reads_as_many_as_were_shown_and_keeps_the_selection() {
        let git = fake(line(GRAPH_PAGE + 500));
        let mut view = GraphView::default();
        let job = view.start(1, PathBuf::from("/r"), true);
        run(&mut view, job, &git);
        let job = view.more(1, PathBuf::from("/r")).unwrap();
        run(&mut view, job, &git);
        view.selected = Some(format!("{:040x}", GRAPH_PAGE + 100));
        let replaced = view.replaced;
        let job = view.start(1, PathBuf::from("/r"), true);
        assert_eq!(job.want, usize::MAX, "the whole history was shown");
        // The old list stays until the new one is whole.
        assert_eq!(view.commits.len(), GRAPH_PAGE + 500);
        run(&mut view, job, &git);
        assert_eq!(view.commits.len(), GRAPH_PAGE + 500);
        assert_eq!(view.replaced, replaced + 1);
        assert_eq!(view.selected_index(), Some(GRAPH_PAGE + 100));
        // A commit that is gone takes the selection and its files with it.
        view.files = vec![];
        view.file = Some(0);
        let git = fake(line(10));
        let job = view.start(1, PathBuf::from("/r"), true);
        run(&mut view, job, &git);
        assert!(view.selected.is_none());
        assert!(view.file.is_none());
    }

    #[test]
    fn a_refresh_of_part_of_a_history_reads_that_part_and_a_new_filter_one_page() {
        let git = fake(line(GRAPH_PAGE * 3));
        let mut view = GraphView::default();
        let job = view.start(1, PathBuf::from("/r"), true);
        run(&mut view, job, &git);
        let job = view.more(1, PathBuf::from("/r")).unwrap();
        run(&mut view, job, &git);
        assert_eq!(
            view.start(1, PathBuf::from("/r"), true).want,
            GRAPH_PAGE * 2
        );
        assert_eq!(view.start(1, PathBuf::from("/r"), false).want, GRAPH_PAGE);
    }

    #[test]
    fn a_page_of_a_replaced_stream_is_dropped() {
        let git = fake(line(10));
        let mut view = GraphView::default();
        let old = view.start(1, PathBuf::from("/r"), true);
        let new = view.start(1, PathBuf::from("/r"), true);
        assert!(!view.apply(old.run(&git)).unwrap());
        assert!(view.loading, "still waiting for the new stream");
        assert!(view.apply(new.run(&git)).unwrap());
        assert_eq!(view.commits.len(), 10);
    }

    #[test]
    fn a_failure_keeps_what_was_loaded_and_stops_asking() {
        let mut view = GraphView::default();
        let job = view.start(1, PathBuf::from("/r"), true);
        let outcome = GraphOutcome {
            generation: 1,
            stream: job.stream,
            result: Err(GitError::NotInstalled),
        };
        assert!(view.apply(outcome).is_err());
        assert!(view.end && view.loaded && !view.loading);
        assert!(view.more(1, PathBuf::from("/r")).is_none());
    }

    #[test]
    fn a_feed_is_equal_only_to_itself() {
        let a = FeedHandle::new();
        assert_eq!(a, a.clone());
        assert_ne!(a, FeedHandle::new());
    }

    #[test]
    fn the_graph_column_is_as_wide_as_the_widest_row() {
        let mut view = GraphView::default();
        let job = view.start(1, PathBuf::from("/r"), true);
        let wide = GraphRow {
            width: 40,
            ..GraphRow::default()
        };
        let page = GraphPage {
            commits: vec![commit(1, &[], &[], "x"), commit(2, &[], &[], "y")],
            rows: rows_of(vec![
                GraphRow {
                    width: 3,
                    ..GraphRow::default()
                },
                wide,
            ]),
            end: true,
            replace: true,
            ..GraphPage::default()
        };
        view.apply(GraphOutcome {
            generation: 1,
            stream: job.stream,
            result: Ok(page),
        })
        .unwrap();
        assert_eq!(view.columns, 40, "no lane is cut off");
    }

    #[test]
    fn find_looks_below_the_selection_then_asks_for_more_then_wraps() {
        let mut view = GraphView::default();
        let job = view.start(1, PathBuf::from("/r"), true);
        let mut commits = line(6);
        commits[1].summary = "fix parser".into();
        commits[4].summary = "Fix lexer".into();
        let page = GraphPage {
            rows: rows_of(vec![GraphRow::default(); 6]),
            commits,
            end: false,
            replace: true,
            ..GraphPage::default()
        };
        view.apply(GraphOutcome {
            generation: 1,
            stream: job.stream,
            result: Ok(page),
        })
        .unwrap();
        view.search = "fix".into();
        assert_eq!(view.find(Direction::Next), Found::At(1));
        view.selected = Some(view.commits[1].sha.clone());
        assert_eq!(view.find(Direction::Next), Found::At(4));
        view.selected = Some(view.commits[4].sha.clone());
        assert_eq!(
            view.find(Direction::Next),
            Found::NeedMore,
            "older pages first"
        );
        view.end = true;
        assert_eq!(view.find(Direction::Next), Found::At(1), "then wraps");
        assert_eq!(view.find(Direction::Previous), Found::At(1));
        view.selected = None;
        view.search = format!("{:040x}", 3);
        assert_eq!(view.find(Direction::Next), Found::At(3), "a whole hash");
        view.search = "nothing like it".into();
        assert_eq!(view.find(Direction::Next), Found::Nothing);
        view.search = "  ".into();
        assert_eq!(view.find(Direction::Next), Found::Nothing);
    }

    #[test]
    fn the_place_after_a_refresh_is_the_first_surviving_top_row() {
        let mut view = GraphView::default();
        let job = view.start(1, PathBuf::from("/r"), true);
        let page = GraphPage {
            commits: line(5),
            rows: rows_of(vec![GraphRow::default(); 5]),
            end: true,
            replace: true,
            ..GraphPage::default()
        };
        view.apply(GraphOutcome {
            generation: 1,
            stream: job.stream,
            result: Ok(page),
        })
        .unwrap();
        let gone = "f".repeat(40);
        assert_eq!(
            view.place_of(&[gone.clone(), view.commits[3].sha.clone()]),
            3
        );
        assert_eq!(view.place_of(&[gone]), 0);
        assert_eq!(view.place_of(&[]), 0);
    }

    #[test]
    fn labels_follow_the_filter() {
        let c = commit(
            1,
            &[],
            &[
                "HEAD -> refs/heads/main",
                "refs/remotes/origin/main",
                "tag: refs/tags/v1",
            ],
            "x",
        );
        let kinds = |q: GraphQuery| -> Vec<RefKind> {
            labels_for(&c, &q).into_iter().map(|l| l.kind).collect()
        };
        assert_eq!(
            kinds(GraphQuery::default()),
            [RefKind::Local, RefKind::Remote, RefKind::Tag]
        );
        assert_eq!(
            kinds(GraphQuery {
                scope: GraphScope::Local,
                remotes: false,
                tags: false
            }),
            [RefKind::Local]
        );
    }

    #[test]
    fn the_fingerprint_changes_with_heads_branches_and_tags_only() {
        let status = RepositoryStatus {
            head: BranchHead::Branch("main".into()),
            unborn: false,
            head_oid: Some("a".into()),
            upstream: None,
            ahead_behind: None,
            changes: vec![],
            stash_count: 0,
        };
        let mut info = RepositoryInfo::default();
        let base = fingerprint(&status, &info);
        let mut dirty = status.clone();
        dirty.stash_count = 3;
        assert_eq!(
            fingerprint(&dirty, &info),
            base,
            "working-tree state is not the graph's"
        );
        let mut moved = status.clone();
        moved.head_oid = Some("b".into());
        assert_ne!(fingerprint(&moved, &info), base);
        info.branches.push(Branch {
            name: "x".into(),
            full_ref: "refs/heads/x".into(),
            remote: false,
            sha: "c".into(),
            upstream: None,
            ahead: 0,
            behind: 0,
            upstream_gone: false,
            current: false,
            date: 0,
            subject: String::new(),
        });
        assert_ne!(fingerprint(&status, &info), base);
    }

    #[test]
    fn the_stored_filter_reads_back_and_falls_back_to_all() {
        assert_eq!(query_from("local", false, true).scope, GraphScope::Local);
        assert_eq!(query_from("current", true, true).scope, GraphScope::Current);
        assert_eq!(query_from("nonsense", true, false).scope, GraphScope::All);
        assert!(!query_from("all", true, false).tags);
    }

    #[test]
    fn the_lane_column_shows_what_was_chosen_within_what_exists() {
        use super::{AUTO_LANES, lanes_at, shown_lanes};
        assert_eq!(shown_lanes(3, None), 3);
        assert_eq!(shown_lanes(200, None), AUTO_LANES);
        assert_eq!(shown_lanes(200, Some(40)), 40);
        assert_eq!(shown_lanes(5, Some(40)), 5);
        assert_eq!(shown_lanes(0, Some(0)), 1);
        assert_eq!(lanes_at(16.0 * 7.4, 16.0, 50), 7);
        assert_eq!(lanes_at(16.0 * 7.6, 16.0, 50), 8);
        assert_eq!(lanes_at(-30.0, 16.0, 50), 1);
        assert_eq!(lanes_at(10_000.0, 16.0, 50), 50);
    }

    #[test]
    fn labels_that_do_not_fit_give_way_to_a_count() {
        use super::fit_labels;
        let widths = [60.0, 50.0, 40.0];
        assert_eq!(fit_labels(&widths, 158.0, 30.0, 4.0), 3, "all fit");
        // Not all: room is kept for "+N".
        assert_eq!(fit_labels(&widths, 150.0, 30.0, 4.0), 2);
        assert_eq!(fit_labels(&widths, 100.0, 30.0, 4.0), 1);
        assert_eq!(fit_labels(&widths, 40.0, 30.0, 4.0), 0, "only the count");
        assert_eq!(fit_labels(&[], 0.0, 30.0, 4.0), 0);
    }

    #[test]
    fn a_refused_rewrite_says_why() {
        use super::{Refusal, rewrite_refusal};
        let mut history = line(5);
        assert_eq!(rewrite_refusal(&history, Some(0), Some(2), None), None);
        assert_eq!(rewrite_refusal(&history, Some(0), Some(2), Some(3)), None);
        assert_eq!(
            rewrite_refusal(&history, Some(0), Some(3), Some(3)),
            Some(Refusal::Pushed)
        );
        assert_eq!(
            rewrite_refusal(&history, None, Some(1), None),
            Some(Refusal::NotOnBranch)
        );
        history[1].parents.push("other".into());
        assert_eq!(
            rewrite_refusal(&history, Some(0), Some(2), None),
            Some(Refusal::Merge)
        );
        for refusal in [Refusal::NotOnBranch, Refusal::Pushed, Refusal::Merge] {
            assert!(!GRAPH_ZH_TW.refusal(refusal).is_empty());
        }
    }

    #[test]
    fn ancestors_of_head_are_found_through_loaded_parents_only() {
        // 0 merges 1 and 2; 1 -> 3; 2 -> 3; 4 is a separate root.
        let commits = vec![
            commit(0, &[1, 2], &[], "M"),
            commit(1, &[3], &[], "a"),
            commit(2, &[3], &[], "b"),
            commit(3, &[], &[], "base"),
            commit(4, &[], &[], "other root"),
        ];
        let mut view = GraphView::default();
        let job = view.start(1, PathBuf::from("/r"), true);
        view.apply(GraphOutcome {
            generation: 1,
            stream: job.stream,
            result: Ok(GraphPage {
                rows: rows_of(vec![GraphRow::default(); 5]),
                commits,
                end: true,
                replace: true,
                ..GraphPage::default()
            }),
        })
        .unwrap();
        let sha = |i: usize| format!("{i:040x}");
        for i in 0..4 {
            assert!(view.is_ancestor_of(&sha(i), &sha(0)), "{i}");
        }
        assert!(!view.is_ancestor_of(&sha(4), &sha(0)));
        // From `a`, its sibling `b` and the merge above are not ancestors.
        assert!(view.is_ancestor_of(&sha(3), &sha(1)));
        assert!(!view.is_ancestor_of(&sha(2), &sha(1)));
        assert!(!view.is_ancestor_of(&sha(0), &sha(1)));
        assert!(!view.is_ancestor_of("nothing", &sha(0)));
    }

    #[test]
    fn a_hidden_tab_stops_git_and_picks_up_from_the_next_page() {
        let git = fake(line(GRAPH_PAGE * 2 + 7));
        let mut view = GraphView::default();
        let job = view.start(1, PathBuf::from("/r"), true);
        run(&mut view, job, &git);
        let feed = view.feed().unwrap().clone();
        assert!(feed.is_reading());
        view.pause();
        assert!(!feed.is_reading(), "git is stopped while the tab is hidden");
        view.resume();
        let job = view.more(1, PathBuf::from("/r")).unwrap();
        run(&mut view, job, &git);
        let job = view.more(1, PathBuf::from("/r")).unwrap();
        run(&mut view, job, &git);
        assert!(view.end);
        assert_eq!(view.commits.len(), GRAPH_PAGE * 2 + 7);
        // The same commits in the same order, and the lanes carried on.
        for (i, c) in view.commits.iter().enumerate() {
            assert_eq!(c.sha, format!("{i:040x}"));
        }
        let rows = view.rows.rows(0..view.commits.len());
        assert!(rows.iter().all(|r| r.column == 0 && r.lane == 0));
        // Paused while a read runs: git stops once that read is done.
        let mut view = GraphView::default();
        let job = view.start(1, PathBuf::from("/r"), true);
        view.pause();
        run(&mut view, job, &git);
        assert!(!view.feed().unwrap().is_reading());
        assert_eq!(view.commits.len(), GRAPH_PAGE);
    }

    #[test]
    fn a_walk_whose_tips_moved_while_paused_is_read_again_not_carried_on() {
        let git = fake(line(GRAPH_PAGE * 2));
        let mut view = GraphView::default();
        let job = view.start(1, PathBuf::from("/r"), true);
        run(&mut view, job, &git);
        view.pause();
        // A commit lands on a branch while the tab is hidden.
        *git.1.lock().unwrap() = vec!["new tip".into()];
        view.resume();
        let job = view.more(1, PathBuf::from("/r")).unwrap();
        let outcome = job.run(&git);
        assert!(matches!(&outcome.result, Ok(page) if page.moved && page.commits.is_empty()));
        view.apply(outcome).unwrap();
        assert!(view.moved, "the session reads the graph again");
        assert_eq!(view.commits.len(), GRAPH_PAGE, "nothing was added on top");
        // Unmoved tips carry on as before.
        let git = fake(line(GRAPH_PAGE * 2));
        let mut view = GraphView::default();
        let job = view.start(1, PathBuf::from("/r"), true);
        run(&mut view, job, &git);
        view.pause();
        view.resume();
        let job = view.more(1, PathBuf::from("/r")).unwrap();
        run(&mut view, job, &git);
        assert!(!view.moved);
        assert_eq!(view.commits.len(), GRAPH_PAGE * 2);
    }

    #[test]
    fn every_graph_string_exists_in_both_languages() {
        for (en, zh) in GRAPH_EN_US.all().into_iter().zip(GRAPH_ZH_TW.all()) {
            assert!(!en.trim().is_empty() && !zh.trim().is_empty());
            assert_eq!(en.contains("{n}"), zh.contains("{n}"), "{en}");
        }
    }
}
