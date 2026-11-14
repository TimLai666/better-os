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

use git_core::{
    Commit, CommitFile, EdgeKind, GraphRow, GraphRows, LaneLabel, LaneLayout, LaneRefs, RefKind,
    RefLabel, RepositoryStatus,
};
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
    /// The open lanes' nearest labelled commits ([`LaneRefs::carry`]),
    /// for the next page's lane names to carry on from.
    lane_refs: HashMap<u32, (u32, u32)>,
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
    /// What each lane of the page's rows is named after, laid out with
    /// them.
    pub lanes: LaneRefs,
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
                lanes: LaneRefs::starting_at(feed.read, feed.lane_refs.clone()),
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
                lane_refs,
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
                        let row = layout.push(&commit.sha, &commit.parents);
                        page.rows.push(&row);
                        page.lanes.push(&row, lane_label(commit, &self.query));
                    }
                    *read += commits.len();
                    page.commits.extend(commits);
                }
                *exhausted = source.is_done();
                *lane_refs = page.lanes.carry().clone();
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
    /// What each lane is named after when the pointer rests on it.
    pub lanes: LaneRefs,
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
            let old_lanes = std::mem::replace(&mut self.lanes, page.lanes);
            let index = page.index.unwrap_or_else(|| {
                self.commits
                    .iter()
                    .enumerate()
                    .map(|(i, c)| (c.sha.clone(), i))
                    .collect()
            });
            let old_index = std::mem::replace(&mut self.index, index);
            if old_commits.len() >= DROP_ELSEWHERE {
                std::thread::spawn(move || drop((old_commits, old_rows, old_lanes, old_index)));
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
            self.lanes.append(page.lanes);
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

/// What a commit's labels can name a lane by, among those the filter
/// shows: a branch before a tag. A detached `HEAD` names nothing.
pub fn lane_label(commit: &Commit, query: &GraphQuery) -> LaneLabel {
    labels_for(commit, query)
        .iter()
        .map(|l| match l.kind {
            RefKind::Local | RefKind::Remote => LaneLabel::Branch,
            RefKind::Tag => LaneLabel::Tag,
            RefKind::Head => LaneLabel::None,
        })
        .max()
        .unwrap_or_default()
}

/// What the pointer rests on in a row's graph cell.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum GraphSpot {
    /// The commit's dot.
    Dot { lane: u32 },
    /// A line of `lane`: straight through the row, or into or out of the
    /// dot in the dot's own column.
    Line { lane: u32 },
    /// The curve from a merge commit to a parent other than its first: the
    /// parent's lane `from` merged into the commit's lane `into`.
    Merge { from: u32, into: u32 },
    /// The curve of a branch ending in the commit it branched from: `branch`
    /// branched from the commit's lane `from`.
    Fork { branch: u32, from: u32 },
}

impl GraphSpot {
    /// The lane highlighted while the pointer rests here: the line's own,
    /// the merged branch's, the branch that forked, or the dot's.
    pub fn lane(self) -> u32 {
        match self {
            GraphSpot::Dot { lane } | GraphSpot::Line { lane } => lane,
            GraphSpot::Merge { from, .. } => from,
            GraphSpot::Fork { branch, .. } => branch,
        }
    }
}

/// How far from a line's centre the pointer still rests on it: the hit
/// area is twice this wide, well over the 1.5px stroke.
pub const LINE_REACH: f32 = 5.0;
// The ticket asks for a hit area at least 8px across.
const _: () = assert!(LINE_REACH * 2.0 >= 8.0);
/// How far from the dot's centre the pointer still rests on the dot.
pub const DOT_REACH: f32 = better_ui::fluent::lanes::NODE_RADIUS + 3.0;

fn line_kind(kind: EdgeKind) -> better_ui::fluent::lanes::LineKind {
    use better_ui::fluent::lanes::LineKind;
    match kind {
        EdgeKind::Pass => LineKind::Pass,
        EdgeKind::In => LineKind::In,
        EdgeKind::Out => LineKind::Out,
    }
}

/// The distance from `p` to the segment from `a` to `b`.
fn to_segment(p: (f32, f32), a: (f32, f32), b: (f32, f32)) -> f32 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let length = dx * dx + dy * dy;
    let t = if length == 0.0 {
        0.0
    } else {
        (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / length).clamp(0.0, 1.0)
    };
    let (x, y) = (a.0 + t * dx, a.1 + t * dy);
    ((p.0 - x).powi(2) + (p.1 - y).powi(2)).sqrt()
}

/// The points a line is drawn through, as
/// [`better_ui::fluent::lanes::segment`] gives its path: two for a
/// straight line, 25 along a curve.
fn line_points(segment: better_ui::fluent::lanes::Segment) -> Vec<(f32, f32)> {
    let Some((c1, c2)) = segment.controls else {
        return vec![segment.from, segment.to];
    };
    let (p0, p3) = (segment.from, segment.to);
    (0..=24)
        .map(|i| {
            let t = i as f32 / 24.0;
            let u = 1.0 - t;
            let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
            (
                a * p0.0 + b * c1.0 + c * c2.0 + d * p3.0,
                a * p0.1 + b * c1.1 + c * c2.1 + d * p3.1,
            )
        })
        .collect()
}

/// What lies under the point `(x, y)` of a row's graph cell, drawn
/// `lane_width` per column and `height` tall with its first `visible`
/// columns shown, exactly as `LaneCell` draws it.
///
/// Expected results:
/// - Within [`DOT_REACH`] of the dot (where it is drawn, the overflow
///   column for a hidden one) is the dot.
/// - Otherwise the nearest line within [`LINE_REACH`] of the pointer, in
///   the visible columns only: an `Out` line to another column is a merge
///   curve, an `In` line from another column a fork curve, and any other
///   line the lane's own line.
/// - Anywhere else is nothing.
pub fn spot_at(
    row: &GraphRow,
    (x, y): (f32, f32),
    lane_width: f32,
    height: f32,
    visible: u16,
) -> Option<GraphSpot> {
    use better_ui::fluent::lanes::{LaneLine, clipping, column_x, segment};
    let lines: Vec<LaneLine> = row
        .edges
        .iter()
        .map(|e| LaneLine {
            kind: line_kind(e.kind),
            column: e.column,
            lane: e.lane,
        })
        .collect();
    let clip = clipping(row.column, &lines, visible);
    let dot = (column_x(clip.node_column, lane_width), height / 2.0);
    if ((x - dot.0).powi(2) + (y - dot.1).powi(2)).sqrt() <= DOT_REACH {
        return Some(GraphSpot::Dot { lane: row.lane });
    }
    let edge_of_lanes = lane_width * visible.max(1) as f32;
    if x >= edge_of_lanes {
        return None;
    }
    let mut nearest: Option<(f32, GraphSpot)> = None;
    for (edge, line) in row.edges.iter().zip(&lines) {
        let points = line_points(segment(*line, row.column, lane_width, height));
        // Only what is drawn counts: the lanes are clipped at the edge of
        // the visible columns.
        let distance = points
            .windows(2)
            .filter(|pair| pair[0].0 < edge_of_lanes || pair[1].0 < edge_of_lanes)
            .map(|pair| to_segment((x, y), pair[0], pair[1]))
            .fold(f32::INFINITY, f32::min);
        if distance > LINE_REACH || nearest.is_some_and(|(d, _)| d <= distance) {
            continue;
        }
        let spot = match edge.kind {
            EdgeKind::Out if edge.column != row.column => GraphSpot::Merge {
                from: edge.lane,
                into: row.lane,
            },
            EdgeKind::In if edge.column != row.column => GraphSpot::Fork {
                branch: edge.lane,
                from: row.lane,
            },
            _ => GraphSpot::Line { lane: edge.lane },
        };
        nearest = Some((distance, spot));
    }
    nearest.map(|(_, spot)| spot)
}

/// How long ago, in words: "3 days ago".
pub fn ago(g: &GraphCopy, now: i64, then: i64) -> String {
    let (n, unit) = crate::history::age(now, then);
    let (one, many) = match unit {
        "s" => return g.ago_now.to_string(),
        "m" => (g.ago_minute, g.ago_minutes),
        "h" => (g.ago_hour, g.ago_hours),
        "d" => (g.ago_day, g.ago_days),
        "mo" => (g.ago_month, g.ago_months),
        _ => (g.ago_year, g.ago_years),
    };
    if n == 1 {
        one.to_string()
    } else {
        crate::i18n::fill(many, &[("n", &n.to_string())])
    }
}

/// How many names a line's tooltip lists before counting the rest.
const NAMES_SHOWN: usize = 3;

impl GraphView {
    /// What names `lane` at `row`: the branches on the nearest commit at or
    /// above it on the lane that has any, or else the tags on the nearest
    /// tagged one, as the filter shows them; `None` when no label points
    /// into the lane.
    fn lane_names(&self, lane: u32, row: usize, g: &GraphCopy) -> Option<String> {
        let commit = self.commits.get(self.lanes.named_row(lane, row)?)?;
        let labels = labels_for(commit, &self.query);
        let branch = |l: &RefLabel| matches!(l.kind, RefKind::Local | RefKind::Remote);
        let branches = labels.iter().any(branch);
        let names: Vec<String> = labels
            .into_iter()
            .filter(|l| {
                if branches {
                    branch(l)
                } else {
                    l.kind == RefKind::Tag
                }
            })
            .map(|l| l.name)
            .collect();
        if names.is_empty() {
            return None;
        }
        let mut text = names[..names.len().min(NAMES_SHOWN)].join(g.name_separator);
        if names.len() > NAMES_SHOWN {
            text.push_str(&crate::i18n::fill(
                g.names_more,
                &[("n", &(names.len() - NAMES_SHOWN).to_string())],
            ));
        }
        Some(text)
    }

    /// A commit in a sentence: its short hash and subject.
    fn commit_words(&self, row: Option<usize>, g: &GraphCopy) -> String {
        match row.and_then(|i| self.commits.get(i)) {
            Some(c) => format!("{} {}", c.short_sha(), c.summary),
            None => g.unread_commit.to_string(),
        }
    }

    /// What the tooltip says about `spot` in the row at `index`; `now` is
    /// the time for the dot's "how long ago".
    ///
    /// Expected results:
    /// - A line names the branches on the nearest commit of its lane at or
    ///   above it that has any, or failing that the tags on the nearest
    ///   tagged one; with neither, the commit the lane leads up to, by short
    ///   hash and subject.
    /// - A merge curve says which branch was merged into which, a fork
    ///   curve which branch branched from which; a side with no label is the
    ///   commit itself (the merged parent, or the commit forked from).
    /// - A dot gives the commit's short hash and subject, then its author
    ///   and how long ago.
    pub fn describe(
        &self,
        index: usize,
        spot: GraphSpot,
        g: &GraphCopy,
        now: i64,
    ) -> Option<String> {
        use crate::i18n::fill;
        let commit = self.commits.get(index)?;
        let text = match spot {
            GraphSpot::Dot { .. } => format!(
                "{} {}\n{} · {}",
                commit.short_sha(),
                commit.summary,
                commit.author_name,
                ago(g, now, commit.author_time)
            ),
            GraphSpot::Line { lane } => match self.lane_names(lane, index, g) {
                Some(names) => fill(g.line_of, &[("names", &names)]),
                None => fill(
                    g.line_leads_to,
                    &[("commit", &self.commit_words(self.lanes.tip(lane), g))],
                ),
            },
            GraphSpot::Merge { from, into } => {
                let from = self.lane_names(from, index, g).unwrap_or_else(|| {
                    self.commit_words(self.lanes.node_at_or_below(from, index + 1), g)
                });
                let into = self
                    .lane_names(into, index, g)
                    .unwrap_or_else(|| self.commit_words(Some(index), g));
                fill(g.merged_into, &[("from", &from), ("into", &into)])
            }
            GraphSpot::Fork { branch, from } => {
                let branch = self
                    .lane_names(branch, index, g)
                    .unwrap_or_else(|| self.commit_words(self.lanes.tip(branch), g));
                let from = self
                    .lane_names(from, index, g)
                    .unwrap_or_else(|| self.commit_words(Some(index), g));
                fill(g.branched_from, &[("branch", &branch), ("from", &from)])
            }
        };
        Some(text)
    }
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
    line_of: "Line of {names}", "{names} 的分支線";
    line_leads_to: "Leads to {commit}", "通往 {commit}";
    merged_into: "{from} merged into {into}", "{from} 合併到 {into}";
    branched_from: "{branch} branched from {from}", "{branch} 從 {from} 分出";
    unread_commit: "older commits not read yet", "還沒讀取的較舊提交";
    name_separator: ", ", "、";
    names_more: " and {n} more", " 等另外 {n} 個";
    ago_now: "just now", "剛剛";
    ago_minute: "1 minute ago", "1 分鐘前";
    ago_minutes: "{n} minutes ago", "{n} 分鐘前";
    ago_hour: "1 hour ago", "1 小時前";
    ago_hours: "{n} hours ago", "{n} 小時前";
    ago_day: "1 day ago", "1 天前";
    ago_days: "{n} days ago", "{n} 天前";
    ago_month: "1 month ago", "1 個月前";
    ago_months: "{n} months ago", "{n} 個月前";
    ago_year: "1 year ago", "1 年前";
    ago_years: "{n} years ago", "{n} 年前";
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
        GraphPage, GraphSpot, GraphView, fingerprint, labels_for, query_from, spot_at,
    };
    use git_core::{
        Branch, BranchHead, Commit, Edge, EdgeKind, GraphRow, GraphRows, RefKind, RepositoryStatus,
    };
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

    /// A graph read by the background job from `commits`, as the window
    /// gets it. Hashes are turned round so each short hash is its own:
    /// commit 1 is `1000000`.
    fn read_graph(mut commits: Vec<Commit>, query: GraphQuery) -> GraphView {
        let turn = |sha: &mut String| *sha = sha.chars().rev().collect();
        for c in &mut commits {
            turn(&mut c.sha);
            c.parents.iter_mut().for_each(turn);
        }
        let git = fake(commits);
        let mut view = GraphView::new(query);
        let job = view.start(1, PathBuf::from("/r"), true);
        run(&mut view, job, &git);
        while let Some(job) = view.more(1, PathBuf::from("/r")) {
            run(&mut view, job, &git);
        }
        view
    }

    /// The spot of the line of `kind` in `column` of the row at `index`.
    fn spot_of(view: &GraphView, index: usize, kind: EdgeKind, column: u16) -> GraphSpot {
        let row = &view.rows.rows(index..index + 1)[0];
        let edge = row
            .edges
            .iter()
            .find(|e| e.kind == kind && e.column == column)
            .expect("the line");
        match kind {
            EdgeKind::Out if column != row.column => GraphSpot::Merge {
                from: edge.lane,
                into: row.lane,
            },
            EdgeKind::In if column != row.column => GraphSpot::Fork {
                branch: edge.lane,
                from: row.lane,
            },
            _ => GraphSpot::Line { lane: edge.lane },
        }
    }

    const DAY: i64 = 86_400;

    #[test]
    fn the_pointer_finds_the_dot_a_line_or_a_curve_within_reach() {
        // Node in column 0; main comes in and goes on in column 0, a lane
        // passes in column 1, and a merge curve leaves for column 2.
        let edge = |kind, column, lane| Edge { kind, column, lane };
        let row = GraphRow {
            column: 0,
            lane: 0,
            edges: vec![
                edge(EdgeKind::Pass, 1, 5),
                edge(EdgeKind::In, 0, 0),
                edge(EdgeKind::Out, 0, 0),
                edge(EdgeKind::Out, 2, 7),
            ],
            width: 3,
        };
        let at = |x: f32, y: f32| spot_at(&row, (x, y), 16.0, 36.0, 12);
        assert_eq!(at(8.0, 18.0), Some(GraphSpot::Dot { lane: 0 }));
        assert_eq!(at(14.0, 18.0), Some(GraphSpot::Dot { lane: 0 }));
        assert_eq!(at(8.0, 2.0), Some(GraphSpot::Line { lane: 0 }));
        assert_eq!(at(8.0, 33.0), Some(GraphSpot::Line { lane: 0 }));
        // The hit area is wider than 8px around a 1.5px line.
        for dx in -4..=4 {
            assert_eq!(
                at(24.0 + dx as f32, 6.0),
                Some(GraphSpot::Line { lane: 5 }),
                "{dx}px off the line"
            );
        }
        assert_eq!(at(31.0, 6.0), None, "between lanes is nothing");
        assert_eq!(
            at(40.0, 35.0),
            Some(GraphSpot::Merge { from: 7, into: 0 }),
            "the curve's foot"
        );
        assert_eq!(at(60.0, 18.0), None, "past the lanes");
        // With two lanes shown, the curve's foot is cut off and not hit.
        assert_eq!(spot_at(&row, (40.0, 35.0), 16.0, 36.0, 2), None);
        // A node in a hidden column is drawn, and hit, in the overflow
        // column.
        let hidden = GraphRow {
            column: 4,
            lane: 9,
            edges: vec![edge(EdgeKind::In, 4, 9)],
            width: 5,
        };
        assert_eq!(
            spot_at(&hidden, (40.0, 18.0), 16.0, 36.0, 2),
            Some(GraphSpot::Dot { lane: 9 })
        );
        // A line ending in a node in another column is a fork.
        let fork = GraphRow {
            column: 0,
            lane: 0,
            edges: vec![edge(EdgeKind::In, 0, 0), edge(EdgeKind::In, 1, 3)],
            width: 2,
        };
        assert_eq!(
            spot_at(&fork, (24.0, 1.0), 16.0, 36.0, 12),
            Some(GraphSpot::Fork { branch: 3, from: 0 })
        );
        assert_eq!(GraphSpot::Fork { branch: 3, from: 0 }.lane(), 3);
        assert_eq!(GraphSpot::Merge { from: 7, into: 0 }.lane(), 7);
    }

    /// main (with origin/main) merged feature; v1 tags the base.
    ///
    /// ```text
    /// 0 M    main, origin/main
    /// | \
    /// | 1 f1 feature
    /// 2 |  m1
    /// 3 /  base v1
    /// ```
    fn merged() -> Vec<Commit> {
        let mut commits = vec![
            commit(
                0,
                &[2, 1],
                &["HEAD -> refs/heads/main", "refs/remotes/origin/main"],
                "Merge feature",
            ),
            commit(1, &[3], &["refs/heads/feature"], "Add feature"),
            commit(2, &[3], &[], "Fix main"),
            commit(3, &[], &["tag: refs/tags/v1"], "Start"),
        ];
        commits[1].author_time = 10 * DAY;
        commits
    }

    #[test]
    fn a_line_a_curve_and_a_dot_say_what_they_are() {
        let view = read_graph(merged(), GraphQuery::default());
        let g = &GRAPH_EN_US;
        let say = |index, spot| view.describe(index, spot, g, 12 * DAY).unwrap();
        assert_eq!(view.lanes.len(), 4, "named with the layout");
        assert_eq!(
            say(2, spot_of(&view, 2, EdgeKind::Pass, 1)),
            "Line of feature"
        );
        assert_eq!(
            say(2, spot_of(&view, 2, EdgeKind::Out, 0)),
            "Line of main, origin/main"
        );
        assert_eq!(
            say(0, spot_of(&view, 0, EdgeKind::Out, 1)),
            "feature merged into main, origin/main"
        );
        assert_eq!(
            say(3, spot_of(&view, 3, EdgeKind::In, 1)),
            "feature branched from main, origin/main",
            "a branch names main's lane before the tag on the base"
        );
        assert_eq!(
            say(1, GraphSpot::Dot { lane: 1 }),
            "1000000 Add feature\nAuthor 1 · 2 days ago"
        );
        let zh = view
            .describe(0, spot_of(&view, 0, EdgeKind::Out, 1), &GRAPH_ZH_TW, 0)
            .unwrap();
        assert_eq!(zh, "feature 合併到 main、origin/main");
        let zh = view
            .describe(3, spot_of(&view, 3, EdgeKind::In, 1), &GRAPH_ZH_TW, 0)
            .unwrap();
        assert_eq!(zh, "feature 從 main、origin/main 分出");
        assert_eq!(
            view.describe(2, spot_of(&view, 2, EdgeKind::Pass, 1), &GRAPH_ZH_TW, 0)
                .unwrap(),
            "feature 的分支線"
        );
    }

    #[test]
    fn lane_names_follow_the_filter() {
        // Without remote branches and tags, only local branches name lanes.
        let query = GraphQuery {
            remotes: false,
            tags: false,
            ..GraphQuery::default()
        };
        let view = read_graph(merged(), query);
        let say = |index, spot| view.describe(index, spot, &GRAPH_EN_US, 0).unwrap();
        assert_eq!(
            say(3, spot_of(&view, 3, EdgeKind::In, 1)),
            "feature branched from main"
        );
        assert_eq!(
            say(0, spot_of(&view, 0, EdgeKind::Out, 1)),
            "feature merged into main"
        );
    }

    #[test]
    fn a_lane_no_label_points_into_is_described_by_its_commits() {
        // A merged branch whose name was deleted, under a detached HEAD.
        let commits = vec![
            commit(0, &[3, 1], &["HEAD"], "Merge"),
            commit(1, &[2], &[], "Second step"),
            commit(2, &[3], &[], "First step"),
            commit(3, &[], &[], "Start"),
        ];
        let view = read_graph(commits, GraphQuery::default());
        let say = |index, spot| view.describe(index, spot, &GRAPH_EN_US, 0).unwrap();
        assert_eq!(
            say(2, spot_of(&view, 2, EdgeKind::Out, 1)),
            "Leads to 1000000 Second step"
        );
        assert_eq!(
            say(0, spot_of(&view, 0, EdgeKind::Out, 1)),
            "1000000 Second step merged into 0000000 Merge"
        );
        assert_eq!(
            say(3, spot_of(&view, 3, EdgeKind::In, 1)),
            "1000000 Second step branched from 3000000 Start"
        );
        assert_eq!(
            say(3, spot_of(&view, 3, EdgeKind::In, 0)),
            "Leads to 0000000 Merge"
        );
    }

    #[test]
    fn a_tag_names_a_lane_no_branch_points_into() {
        let commits = vec![
            commit(0, &[1], &["tag: refs/tags/v2"], "Release"),
            commit(1, &[], &[], "Start"),
        ];
        let view = read_graph(commits, GraphQuery::default());
        assert_eq!(
            view.describe(1, spot_of(&view, 1, EdgeKind::In, 0), &GRAPH_EN_US, 0)
                .unwrap(),
            "Line of v2"
        );
    }

    #[test]
    fn many_names_on_one_commit_are_counted_after_the_first_three() {
        let commits = vec![commit(
            0,
            &[],
            &[
                "refs/heads/a",
                "refs/heads/b",
                "refs/heads/c",
                "refs/heads/d",
                "refs/remotes/origin/e",
                "tag: refs/tags/f",
            ],
            "Start",
        )];
        let view = read_graph(commits, GraphQuery::default());
        let lane = view.rows.rows(0..1)[0].lane;
        assert_eq!(
            view.describe(0, GraphSpot::Line { lane }, &GRAPH_EN_US, 0)
                .unwrap(),
            "Line of a, b, c and 2 more"
        );
        assert_eq!(
            view.describe(0, GraphSpot::Line { lane }, &GRAPH_ZH_TW, 0)
                .unwrap(),
            "a、b、c 等另外 2 個 的分支線"
        );
    }

    #[test]
    fn lane_names_carry_across_pages() {
        // A branch tip at the top and nothing else named: a line deep in
        // the third page still names it.
        let mut commits = line(GRAPH_PAGE * 2 + 50);
        commits[0].refs = vec!["HEAD -> refs/heads/main".into()];
        let view = read_graph(commits, GraphQuery::default());
        assert!(view.end);
        assert_eq!(view.lanes.len(), GRAPH_PAGE * 2 + 50);
        let index = GRAPH_PAGE * 2 + 20;
        assert_eq!(
            view.describe(
                index,
                spot_of(&view, index, EdgeKind::In, 0),
                &GRAPH_EN_US,
                0
            )
            .unwrap(),
            "Line of main"
        );
    }

    #[test]
    fn how_long_ago_reads_as_words_in_both_languages() {
        use super::ago;
        assert_eq!(ago(&GRAPH_EN_US, 30, 0), "just now");
        assert_eq!(ago(&GRAPH_EN_US, 60, 0), "1 minute ago");
        assert_eq!(ago(&GRAPH_EN_US, 3 * 3600, 0), "3 hours ago");
        assert_eq!(ago(&GRAPH_EN_US, 400 * DAY, 0), "1 year ago");
        assert_eq!(ago(&GRAPH_ZH_TW, 5 * DAY, 0), "5 天前");
        assert_eq!(ago(&GRAPH_ZH_TW, 60 * DAY, 0), "2 個月前");
    }

    #[test]
    fn every_graph_string_exists_in_both_languages() {
        for (en, zh) in GRAPH_EN_US.all().into_iter().zip(GRAPH_ZH_TW.all()) {
            assert!(!en.trim().is_empty() && !zh.trim().is_empty());
            assert_eq!(en.contains("{n}"), zh.contains("{n}"), "{en}");
        }
    }
}
