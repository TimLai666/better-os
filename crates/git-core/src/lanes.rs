//! The branch graph's lanes: which column each commit sits in and which
//! lines run through its row, the way `git log --graph`, VS Code's Git
//! Graph, and Fork draw them.
//!
//! Nothing here reads git. [`LaneLayout`] is fed commits in topological order
//! (every child before any of its parents, as `git log --topo-order` prints
//! them), one at a time, and answers each with a [`GraphRow`]. The layout
//! keeps its state between calls, so a long history is laid out a page at a
//! time and a page carries on exactly where the one before stopped.
//!
//! The rules:
//! - A lane is a column waiting for one commit, the parent of the commit
//!   above it. A branch stays in its column for as long as it lives.
//! - A commit takes the leftmost lane waiting for it; every other lane
//!   waiting for the same commit joins it in that row (a fork, drawn as a
//!   curve) and its column becomes free.
//! - A commit nothing waits for (a branch tip) takes the leftmost free
//!   column, with a new colour.
//! - The first parent continues in the commit's own column and colour. Any
//!   other parent (a merge) joins the lane already waiting for it, or opens
//!   a new lane in the leftmost free column with a new colour.
//! - A commit with no parents (a root, which may sit in the middle of the
//!   list when histories are unrelated) ends its lane.
//! - Colours are lane numbers, counted up as lanes open; the window turns a
//!   number into a colour of its palette, so a lane keeps one colour for its
//!   whole life.

/// How one line crosses a row.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum EdgeKind {
    /// From the top of the row to the bottom, straight down its column: a
    /// lane passing a commit that is not its own.
    Pass,
    /// From the top of the row at its column into the commit's node: a
    /// child's line ending here. A column other than the node's is a fork.
    In,
    /// From the commit's node to the bottom of the row at its column: the
    /// line to a parent. A column other than the node's is a merge or a new
    /// branch line.
    Out,
}

/// One line in a row.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Edge {
    pub kind: EdgeKind,
    /// The column at the top (`Pass`, `In`) or the bottom (`Out`) of the row.
    pub column: u16,
    /// The lane the line belongs to, for its colour.
    pub lane: u32,
}

/// One commit's row.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct GraphRow {
    /// Where the commit's node sits.
    pub column: u16,
    /// The node's lane, for its colour.
    pub lane: u32,
    /// Every line in the row: passing lanes and the lines ending in the
    /// node in column order, then the lines to the parents in parent order.
    pub edges: Vec<Edge>,
    /// How many columns the row uses: one more than the rightmost column any
    /// of its lines or its node touches.
    pub width: u16,
}

impl GraphRow {
    /// Columns at the top edge of the row, with their lanes, left to right.
    pub fn top(&self) -> Vec<(u16, u32)> {
        let mut top: Vec<(u16, u32)> = self
            .edges
            .iter()
            .filter(|e| matches!(e.kind, EdgeKind::Pass | EdgeKind::In))
            .map(|e| (e.column, e.lane))
            .collect();
        top.sort_unstable();
        top.dedup();
        top
    }

    /// Columns at the bottom edge of the row, with their lanes, left to
    /// right. A merge into a lane that passes this row meets it at the
    /// bottom edge; the pair counts once, with the lane that continues.
    pub fn bottom(&self) -> Vec<(u16, u32)> {
        let passing: Vec<u16> = self
            .edges
            .iter()
            .filter(|e| e.kind == EdgeKind::Pass)
            .map(|e| e.column)
            .collect();
        let mut bottom: Vec<(u16, u32)> = self
            .edges
            .iter()
            .filter(|e| {
                e.kind == EdgeKind::Pass
                    || (e.kind == EdgeKind::Out && !passing.contains(&e.column))
            })
            .map(|e| (e.column, e.lane))
            .collect();
        bottom.sort_unstable();
        bottom.dedup();
        bottom
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Lane {
    /// The commit this column waits for.
    wants: String,
    lane: u32,
}

/// The lanes open after the last commit laid out.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LaneLayout {
    columns: Vec<Option<Lane>>,
    next_lane: u32,
}

impl LaneLayout {
    pub fn new() -> Self {
        Self::default()
    }

    /// How many lanes are open, waiting for a commit still to come.
    pub fn open_lanes(&self) -> usize {
        self.columns.iter().flatten().count()
    }

    /// The open lanes as `(column, lane)`, left to right: what enters the
    /// top of the next row.
    pub fn tops(&self) -> Vec<(u16, u32)> {
        self.columns
            .iter()
            .enumerate()
            .filter_map(|(i, c)| c.as_ref().map(|l| (i as u16, l.lane)))
            .collect()
    }

    fn new_lane(&mut self) -> u32 {
        let lane = self.next_lane;
        self.next_lane = self.next_lane.wrapping_add(1);
        lane
    }

    fn free_column(&mut self, not: Option<usize>) -> usize {
        let free = self
            .columns
            .iter()
            .enumerate()
            .position(|(i, c)| c.is_none() && Some(i) != not);
        match free {
            Some(column) => column,
            None => {
                self.columns.push(None);
                self.columns.len() - 1
            }
        }
    }

    /// Lays out the next commit, given its parents in git's order (first
    /// parent first).
    ///
    /// Expected results:
    /// - A commit no lane waits for takes the leftmost free column and a new
    ///   lane; its row has no `In` line.
    /// - Every lane waiting for the commit ends in its row with an `In`
    ///   line, and the node takes the leftmost of them.
    /// - Every other open lane passes the row with a `Pass` line.
    /// - The first parent continues in the node's column and lane (`Out` at
    ///   the node's column). Each further parent joins the lane already
    ///   waiting for it, or opens a new one in the leftmost free column; a
    ///   parent named twice gets one line.
    /// - A root commit has no `Out` line, and its column is free from the
    ///   next row on.
    pub fn push(&mut self, sha: &str, parents: &[String]) -> GraphRow {
        let waiting: Vec<usize> = self
            .columns
            .iter()
            .enumerate()
            .filter(|(_, c)| c.as_ref().is_some_and(|l| l.wants == sha))
            .map(|(i, _)| i)
            .collect();
        let mut edges = Vec::new();
        for (i, slot) in self.columns.iter().enumerate() {
            if let Some(l) = slot
                && !waiting.contains(&i)
            {
                edges.push(Edge {
                    kind: EdgeKind::Pass,
                    column: i as u16,
                    lane: l.lane,
                });
            }
        }
        let (column, lane) = match waiting.first() {
            Some(&first) => {
                let lane = self.columns[first].as_ref().map(|l| l.lane).unwrap_or(0);
                for &i in &waiting {
                    let l = self.columns[i].take().expect("a waiting lane");
                    edges.push(Edge {
                        kind: EdgeKind::In,
                        column: i as u16,
                        lane: l.lane,
                    });
                }
                (first, lane)
            }
            None => {
                let column = self.free_column(None);
                (column, self.new_lane())
            }
        };
        let mut seen: Vec<&str> = Vec::new();
        for (index, parent) in parents.iter().enumerate() {
            if seen.contains(&parent.as_str()) {
                continue;
            }
            seen.push(parent);
            if index == 0 {
                self.columns[column] = Some(Lane {
                    wants: parent.clone(),
                    lane,
                });
                edges.push(Edge {
                    kind: EdgeKind::Out,
                    column: column as u16,
                    lane,
                });
                continue;
            }
            let existing = self
                .columns
                .iter()
                .enumerate()
                .find(|(i, c)| *i != column && c.as_ref().is_some_and(|l| &l.wants == parent))
                .map(|(i, c)| (i, c.as_ref().map(|l| l.lane).unwrap_or(0)));
            let (target, target_lane) = match existing {
                Some(found) => found,
                None => {
                    let target = self.free_column(Some(column));
                    let new_lane = self.new_lane();
                    self.columns[target] = Some(Lane {
                        wants: parent.clone(),
                        lane: new_lane,
                    });
                    (target, new_lane)
                }
            };
            edges.push(Edge {
                kind: EdgeKind::Out,
                column: target as u16,
                lane: target_lane,
            });
        }
        while matches!(self.columns.last(), Some(None)) {
            self.columns.pop();
        }
        let width = edges
            .iter()
            .map(|e| e.column as usize + 1)
            .chain([column + 1])
            .max()
            .unwrap_or(1);
        GraphRow {
            column: column as u16,
            lane,
            edges,
            width: width as u16,
        }
    }
}

/// Rows between two stored states of the open lanes in [`GraphRows`].
pub const CHECKPOINT: usize = 128;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct RowHead {
    start: u32,
    len: u16,
    column: u16,
    lane: u32,
    width: u16,
}

/// Laid-out rows kept small: only the lines that start or end at a node
/// (`In` and `Out`) are stored, and the open lanes every [`CHECKPOINT`]
/// rows. The passing lines of a row are worked out again when it is asked
/// for, from the nearest stored state. A history with 200 long-lived
/// branches would otherwise hold 200 `Pass` lines in every row.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct GraphRows {
    /// The index of the first row, for a segment laid out to be appended.
    base: usize,
    heads: Vec<RowHead>,
    ends: Vec<Edge>,
    /// The open lanes at the top of every row whose index is a multiple of
    /// [`CHECKPOINT`], from `base` on.
    checkpoints: Vec<Vec<(u16, u32)>>,
    /// The open lanes at the top of the next row.
    top: Vec<(u16, u32)>,
    /// The open lanes at the top of row `base`.
    start_top: Vec<(u16, u32)>,
    widest: u16,
}

/// The open lanes at the bottom of a row, given those at its top.
fn advance(top: &mut Vec<(u16, u32)>, ends: &[Edge]) {
    top.retain(|(column, _)| {
        !ends
            .iter()
            .any(|e| e.kind == EdgeKind::In && e.column == *column)
    });
    for e in ends.iter().filter(|e| e.kind == EdgeKind::Out) {
        if let Err(at) = top.binary_search_by_key(&e.column, |(c, _)| *c) {
            top.insert(at, (e.column, e.lane));
        }
    }
}

impl GraphRows {
    pub fn new() -> Self {
        Self::default()
    }

    /// An empty segment that carries on after `base` rows, whose next row
    /// starts with the open lanes `top` ([`LaneLayout::tops`]). Appending it
    /// to the rows before it with [`GraphRows::append`] gives the same rows
    /// as laying all of them out in one.
    pub fn starting_at(base: usize, top: Vec<(u16, u32)>) -> Self {
        Self {
            base,
            start_top: top.clone(),
            top,
            ..Self::default()
        }
    }

    pub fn len(&self) -> usize {
        self.heads.len()
    }

    pub fn is_empty(&self) -> bool {
        self.heads.is_empty()
    }

    /// The widest row, in columns.
    pub fn widest(&self) -> u16 {
        self.widest
    }

    /// How many lines are stored, for keeping an eye on memory.
    pub fn stored_edges(&self) -> usize {
        self.ends.len() + self.checkpoints.iter().map(Vec::len).sum::<usize>()
    }

    /// Adds the next row, as [`LaneLayout::push`] returned it.
    pub fn push(&mut self, row: &GraphRow) {
        if (self.base + self.heads.len()).is_multiple_of(CHECKPOINT) {
            self.checkpoints.push(self.top.clone());
        }
        let start = self.ends.len() as u32;
        self.ends.extend(
            row.edges
                .iter()
                .filter(|e| e.kind != EdgeKind::Pass)
                .copied(),
        );
        let ends = &self.ends[start as usize..];
        self.heads.push(RowHead {
            start,
            len: ends.len() as u16,
            column: row.column,
            lane: row.lane,
            width: row.width,
        });
        advance(&mut self.top, ends);
        self.widest = self.widest.max(row.width);
    }

    /// Adds a segment made with [`GraphRows::starting_at`] for exactly the
    /// rows after these.
    pub fn append(&mut self, mut next: GraphRows) {
        debug_assert_eq!(next.base, self.base + self.heads.len());
        let shift = self.ends.len() as u32;
        self.heads.extend(next.heads.iter().map(|h| RowHead {
            start: h.start + shift,
            ..*h
        }));
        self.ends.append(&mut next.ends);
        self.checkpoints.append(&mut next.checkpoints);
        self.top = next.top;
        self.widest = self.widest.max(next.widest);
    }

    /// The rows in `range` whole, passing lines included, exactly as
    /// [`LaneLayout::push`] returned them. Rows outside what is stored are
    /// left out.
    pub fn rows(&self, range: std::ops::Range<usize>) -> Vec<GraphRow> {
        let end = range.end.min(self.base + self.heads.len());
        if range.start >= end || range.start < self.base {
            return Vec::new();
        }
        let first_checkpoint = self.base.div_ceil(CHECKPOINT);
        let checkpoint = range.start / CHECKPOINT;
        let (mut top, mut at) = if checkpoint >= first_checkpoint {
            (
                self.checkpoints[checkpoint - first_checkpoint].clone(),
                checkpoint * CHECKPOINT,
            )
        } else {
            // A segment's rows before its first checkpoint start from what
            // it was made with.
            (self.start_top.clone(), self.base)
        };
        while at < range.start {
            advance(&mut top, self.ends_of(at));
            at += 1;
        }
        let mut rows = Vec::with_capacity(end - range.start);
        for index in range.start..end {
            let head = self.heads[index - self.base];
            let ends = self.ends_of(index);
            let mut edges: Vec<Edge> = top
                .iter()
                .filter(|(column, _)| {
                    !ends
                        .iter()
                        .any(|e| e.kind == EdgeKind::In && e.column == *column)
                })
                .map(|&(column, lane)| Edge {
                    kind: EdgeKind::Pass,
                    column,
                    lane,
                })
                .collect();
            edges.extend_from_slice(ends);
            rows.push(GraphRow {
                column: head.column,
                lane: head.lane,
                edges,
                width: head.width,
            });
            advance(&mut top, ends);
        }
        rows
    }

    fn ends_of(&self, index: usize) -> &[Edge] {
        let head = self.heads[index - self.base];
        &self.ends[head.start as usize..head.start as usize + head.len as usize]
    }
}
