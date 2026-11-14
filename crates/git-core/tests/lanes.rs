//! The branch graph's lane layout, case by case, with every result checked
//! for continuity: what leaves the bottom of one row is exactly what enters
//! the top of the next, in the same columns and lanes.

use git_core::{Edge, EdgeKind, GraphRow, LaneLabel, LaneLayout, LaneRefs};

/// Lays out `(sha, parents)` in the order given (topological, children
/// first).
fn layout(commits: &[(&str, &[&str])]) -> Vec<GraphRow> {
    let mut lanes = LaneLayout::new();
    commits
        .iter()
        .map(|(sha, parents)| {
            let parents: Vec<String> = parents.iter().map(|p| p.to_string()).collect();
            lanes.push(sha, &parents)
        })
        .collect()
}

/// The invariants every layout keeps.
fn check(rows: &[GraphRow]) {
    if let Some(first) = rows.first() {
        assert!(first.top().is_empty(), "nothing enters the first row");
    }
    for pair in rows.windows(2) {
        assert_eq!(
            pair[0].bottom(),
            pair[1].top(),
            "the lines leaving one row enter the next"
        );
    }
    for row in rows {
        // The node sits where a line ending in it came from, or in a column
        // nothing passes.
        let passing: Vec<u16> = edges(row, EdgeKind::Pass);
        assert!(!passing.contains(&row.column), "a lane passes over a node");
        let ins = edges(row, EdgeKind::In);
        if let Some(&first) = ins.iter().min() {
            assert_eq!(row.column, first, "the node takes the leftmost lane");
        }
        let widest = row
            .edges
            .iter()
            .map(|e| e.column + 1)
            .chain([row.column + 1])
            .max()
            .unwrap();
        assert_eq!(row.width, widest);
    }
}

/// The columns of one kind of line, left to right.
fn edges(row: &GraphRow, kind: EdgeKind) -> Vec<u16> {
    let mut columns: Vec<u16> = row
        .edges
        .iter()
        .filter(|e| e.kind == kind)
        .map(|e| e.column)
        .collect();
    columns.sort_unstable();
    columns
}

fn out(row: &GraphRow) -> Vec<u16> {
    edges(row, EdgeKind::Out)
}

#[test]
fn a_straight_line_stays_in_the_first_column_in_one_colour() {
    let rows = layout(&[("c", &["b"]), ("b", &["a"]), ("a", &[])]);
    check(&rows);
    assert!(rows.iter().all(|r| r.column == 0 && r.width == 1));
    assert!(rows.iter().all(|r| r.lane == rows[0].lane));
    assert_eq!(out(&rows[0]), [0]);
    assert_eq!(edges(&rows[1], EdgeKind::In), [0]);
    // The root ends the line.
    assert!(out(&rows[2]).is_empty());
}

#[test]
fn a_fork_draws_the_side_branch_into_the_parent() {
    // main: m2 -> base; feature: f1 -> base. Both tips wait for `base`.
    let rows = layout(&[("m2", &["base"]), ("f1", &["base"]), ("base", &[])]);
    check(&rows);
    assert_eq!(rows[0].column, 0);
    assert_eq!(rows[1].column, 1, "the second tip opens a second column");
    assert_ne!(rows[0].lane, rows[1].lane, "a new branch gets a new colour");
    // At the fork point both lanes end in the node, which takes column 0.
    assert_eq!(rows[2].column, 0);
    assert_eq!(edges(&rows[2], EdgeKind::In), [0, 1]);
    assert_eq!(rows[2].lane, rows[0].lane);
}

#[test]
fn a_merge_opens_a_lane_for_its_second_parent_and_closes_it_at_the_fork() {
    //   M (main, merge of m1 and f1)
    //   | \
    //   m1  f1
    //   | /
    //   base
    let rows = layout(&[
        ("M", &["m1", "f1"]),
        ("f1", &["base"]),
        ("m1", &["base"]),
        ("base", &[]),
    ]);
    check(&rows);
    assert_eq!(
        out(&rows[0]),
        [0, 1],
        "the merge draws a line to each parent"
    );
    assert_eq!(rows[1].column, 1, "the merged branch keeps its own column");
    assert_eq!(rows[2].column, 0);
    assert_eq!(edges(&rows[2], EdgeKind::Pass), [1]);
    assert_eq!(edges(&rows[3], EdgeKind::In), [0, 1]);
    // The merged lane kept one colour from the merge to the fork.
    let merged_lane = rows[0]
        .edges
        .iter()
        .find(|e| e.kind == EdgeKind::Out && e.column == 1)
        .unwrap()
        .lane;
    assert_eq!(rows[1].lane, merged_lane);
    assert!(rows[2].edges.contains(&Edge {
        kind: EdgeKind::Pass,
        column: 1,
        lane: merged_lane
    }));
}

#[test]
fn a_merge_into_a_lane_already_open_joins_it_instead_of_opening_another() {
    // Two merges of the same branch tip `f1`: the second joins the lane the
    // first opened.
    let rows = layout(&[
        ("M2", &["M1", "f1"]),
        ("M1", &["m0", "f1"]),
        ("f1", &["m0"]),
        ("m0", &[]),
    ]);
    check(&rows);
    assert_eq!(out(&rows[0]), [0, 1]);
    assert_eq!(
        out(&rows[1]),
        [0, 1],
        "the second merge meets the open lane"
    );
    assert!(rows.iter().all(|r| r.width <= 2));
}

#[test]
fn an_octopus_merge_draws_a_line_to_every_parent() {
    let rows = layout(&[
        ("O", &["a", "b", "c", "d"]),
        ("d", &["root"]),
        ("c", &["root"]),
        ("b", &["root"]),
        ("a", &["root"]),
        ("root", &[]),
    ]);
    check(&rows);
    assert_eq!(out(&rows[0]), [0, 1, 2, 3]);
    let lanes: Vec<u32> = rows[0]
        .edges
        .iter()
        .filter(|e| e.kind == EdgeKind::Out)
        .map(|e| e.lane)
        .collect();
    let mut distinct = lanes.clone();
    distinct.dedup();
    assert_eq!(distinct.len(), 4, "each parent's line has its own colour");
    assert_eq!(rows[1].column, 3);
    assert_eq!(rows[4].column, 0);
    assert_eq!(edges(&rows[5], EdgeKind::In), [0, 1, 2, 3]);
    assert_eq!(rows[5].column, 0);
}

#[test]
fn a_parent_named_twice_gets_one_line() {
    let rows = layout(&[("M", &["a", "a"]), ("a", &[])]);
    check(&rows);
    assert_eq!(out(&rows[0]), [0]);
}

#[test]
fn a_branch_that_ends_frees_its_column_for_the_next_branch() {
    // `old` is a separate root history that ends after one commit; the
    // branch tip that comes after it reuses column 1.
    let rows = layout(&[
        ("m3", &["m2"]),
        ("old", &[]),
        ("new", &["m1"]),
        ("m2", &["m1"]),
        ("m1", &[]),
    ]);
    check(&rows);
    assert_eq!(rows[1].column, 1);
    assert!(out(&rows[1]).is_empty(), "a root ends its lane");
    assert_eq!(rows[2].column, 1, "the freed column is used again");
    assert_ne!(
        rows[2].lane, rows[1].lane,
        "a new branch in an old column gets a new colour"
    );
    assert!(rows.iter().all(|r| r.width <= 2));
}

#[test]
fn a_merged_branch_ends_where_it_joins_and_its_column_is_reused() {
    // feature merges into main and is gone; a later branch from below
    // takes the column again.
    let rows = layout(&[
        ("x1", &["m2"]),
        ("M", &["m1", "f1"]),
        ("f1", &["m1"]),
        ("m1", &["m0"]),
        ("m0", &[]),
    ]);
    check(&rows);
    // x1 is a tip waiting for m2, which never comes: its lane stays open.
    assert_eq!(rows[0].column, 0);
    assert_eq!(rows[1].column, 1);
    // M merges f1 in column 2; at m1 the lanes for m1 (1, 2) end.
    assert_eq!(out(&rows[1]), [1, 2]);
    assert_eq!(edges(&rows[3], EdgeKind::In), [1, 2]);
    assert_eq!(rows[3].column, 1);
    assert_eq!(edges(&rows[3], EdgeKind::Pass), [0]);
    assert!(
        rows[4].width <= 2,
        "the merged branch's column is free again"
    );
}

#[test]
fn criss_cross_merges_keep_every_line_continuous() {
    // a1 and b1 each merge the other's parent:
    //   A2 = merge(a1, b1), B2 = merge(b1, a1)
    //   a1 -> base, b1 -> base
    let rows = layout(&[
        ("A2", &["a1", "b1"]),
        ("B2", &["b1", "a1"]),
        ("a1", &["base"]),
        ("b1", &["base"]),
        ("base", &[]),
    ]);
    check(&rows);
    assert_eq!(rows[0].column, 0);
    assert_eq!(out(&rows[0]), [0, 1]);
    // B2 is a second tip: its own column continues to b1, and its merge
    // line to a1 joins the lane A2 already opened for a1.
    assert_eq!(rows[1].column, 2);
    assert_eq!(edges(&rows[1], EdgeKind::Pass), [0, 1]);
    assert_eq!(out(&rows[1]), [0, 2]);
    assert_eq!(rows[2].column, 0);
    assert_eq!(edges(&rows[2], EdgeKind::In), [0]);
    assert_eq!(rows[3].column, 1);
    assert_eq!(edges(&rows[3], EdgeKind::In), [1, 2]);
    assert_eq!(edges(&rows[4], EdgeKind::In), [0, 1]);
}

#[test]
fn a_root_in_the_middle_starts_an_unrelated_history_beside_the_first() {
    // Two unrelated histories merged later (`git merge
    // --allow-unrelated-histories`): `r2` is a second root.
    let rows = layout(&[
        ("M", &["a2", "o2"]),
        ("o2", &["r2"]),
        ("r2", &[]),
        ("a2", &["a1"]),
        ("a1", &[]),
    ]);
    check(&rows);
    assert_eq!(rows[2].column, 1);
    assert!(out(&rows[2]).is_empty(), "the second root ends its lane");
    assert_eq!(rows[3].column, 0);
    assert_eq!(rows[3].width, 1, "only the first history is left");
    assert!(out(&rows[4]).is_empty());
}

#[test]
fn a_page_carries_on_exactly_where_the_one_before_stopped() {
    let commits: Vec<(String, Vec<String>)> = (0..40)
        .map(|i| {
            let sha = format!("c{i}");
            let mut parents = vec![format!("c{}", i + 1)];
            if i % 7 == 0 {
                parents.push(format!("c{}", i + 3));
            }
            if i == 39 {
                parents.clear();
            }
            (sha, parents)
        })
        .collect();
    let mut whole = LaneLayout::new();
    let all: Vec<GraphRow> = commits.iter().map(|(s, p)| whole.push(s, p)).collect();
    check(&all);
    let mut paged = LaneLayout::new();
    let mut rows = Vec::new();
    for page in commits.chunks(9) {
        // Each page is laid out by a copy handed on from the one before, as
        // a background job does it.
        let mut carried = paged.clone();
        rows.extend(page.iter().map(|(s, p)| carried.push(s, p)));
        paged = carried;
    }
    assert_eq!(rows, all);
    assert_eq!(paged, whole);
    assert_eq!(paged.open_lanes(), 0);
}

#[test]
fn many_branches_stay_in_their_columns_while_they_live() {
    // Ten tips off one base: each keeps its column to the end.
    let mut commits: Vec<(String, Vec<String>)> = Vec::new();
    for round in 0..3 {
        for branch in 0..10 {
            let next = if round == 2 {
                "base".to_string()
            } else {
                format!("b{branch}r{}", round + 1)
            };
            commits.push((format!("b{branch}r{round}"), vec![next]));
        }
    }
    commits.push(("base".into(), vec![]));
    let mut lanes = LaneLayout::new();
    let rows: Vec<GraphRow> = commits.iter().map(|(s, p)| lanes.push(s, p)).collect();
    check(&rows);
    for (i, row) in rows[..30].iter().enumerate() {
        assert_eq!(row.column as usize, i % 10);
        assert_eq!(row.lane, rows[i % 10].lane, "a branch keeps its colour");
    }
    assert_eq!(edges(&rows[30], EdgeKind::In).len(), 10);
}

/// A history shaped by a seed: tips, merges, octopuses, and roots mixed.
fn shuffled(n: usize, seed: u64) -> Vec<(String, Vec<String>)> {
    let mut state = seed;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    (0..n)
        .map(|i| {
            let mut parents = Vec::new();
            if i + 1 < n && next() % 11 != 0 {
                let near = (i + 1 + (next() % 5) as usize).min(n - 1);
                parents.push(format!("c{near}"));
                if next() % 4 == 0 {
                    let far = (i + 2 + (next() % 40) as usize).min(n - 1);
                    parents.push(format!("c{far}"));
                }
            }
            (format!("c{i}"), parents)
        })
        .collect()
}

#[test]
fn stored_rows_give_back_exactly_what_was_laid_out_for_any_range() {
    use git_core::GraphRows;
    let commits = shuffled(1_000, 0x9e37_79b9);
    let mut lanes = LaneLayout::new();
    let all: Vec<GraphRow> = commits.iter().map(|(s, p)| lanes.push(s, p)).collect();
    // Laid out in one go.
    let mut whole = GraphRows::new();
    for row in &all {
        whole.push(row);
    }
    // And in pages of 97 rows, each laid out on its own and appended.
    let mut lanes = LaneLayout::new();
    let mut paged = GraphRows::new();
    for page in commits.chunks(97) {
        let mut segment = GraphRows::starting_at(paged.len(), lanes.tops());
        for (s, p) in page {
            segment.push(&lanes.push(s, p));
        }
        paged.append(segment);
    }
    assert_eq!(paged, whole);
    assert_eq!(whole.rows(0..all.len()), all);
    for start in [0, 1, 127, 128, 129, 500, 999] {
        for len in [1, 7, 40] {
            let end = (start + len).min(all.len());
            assert_eq!(whole.rows(start..end), all[start..end], "{start}..{end}");
        }
    }
    assert!(
        whole.rows(990..2_000).len() == 10,
        "rows past the end are left out"
    );
    assert_eq!(whole.widest(), all.iter().map(|r| r.width).max().unwrap());
}

#[test]
fn two_hundred_long_branches_store_a_few_lines_a_row_not_two_hundred() {
    use git_core::GraphRows;
    // 200 branches of 250 commits each, all alive the whole way.
    let mut commits: Vec<(String, Vec<String>)> = Vec::new();
    for round in 0..250 {
        for branch in 0..200 {
            let next = if round == 249 {
                "base".to_string()
            } else {
                format!("b{branch}r{}", round + 1)
            };
            commits.push((format!("b{branch}r{round}"), vec![next]));
        }
    }
    commits.push(("base".into(), vec![]));
    let mut lanes = LaneLayout::new();
    let mut rows = GraphRows::new();
    let mut full = 0;
    for (s, p) in &commits {
        let row = lanes.push(s, p);
        full += row.edges.len();
        rows.push(&row);
    }
    assert_eq!(rows.widest(), 200);
    assert!(full > 9_000_000, "{full} lines laid out in all");
    // Two lines a row plus the open lanes every 128 rows.
    assert!(
        rows.stored_edges() < 500_000,
        "{} stored",
        rows.stored_edges()
    );
    let middle = rows.rows(25_000..25_003);
    assert_eq!(middle[0].edges.len(), 201);
}

const B: LaneLabel = LaneLabel::Branch;
const T: LaneLabel = LaneLabel::Tag;
const N: LaneLabel = LaneLabel::None;

/// Lays out `(sha, parents, label)` and records what each commit's labels
/// can name a lane by, as the graph's background job does.
fn named(commits: &[(&str, &[&str], LaneLabel)]) -> (Vec<GraphRow>, LaneRefs) {
    let mut lanes = LaneLayout::new();
    let mut refs = LaneRefs::new();
    let rows = commits
        .iter()
        .map(|(sha, parents, label)| {
            let parents: Vec<String> = parents.iter().map(|p| p.to_string()).collect();
            let row = lanes.push(sha, &parents);
            refs.push(&row, *label);
            row
        })
        .collect();
    (rows, refs)
}

/// The lane of the line of `kind` in `column` of `row`.
fn lane_of(row: &GraphRow, kind: EdgeKind, column: u16) -> u32 {
    row.edges
        .iter()
        .find(|e| e.kind == kind && e.column == column)
        .unwrap_or_else(|| panic!("no {kind:?} line in column {column}"))
        .lane
}

#[test]
fn a_straight_line_is_named_after_the_nearest_label_above_it() {
    let (rows, refs) = named(&[("c", &["b"], N), ("b", &["a"], B), ("a", &[], N)]);
    let lane = rows[0].lane;
    assert_eq!(refs.named_row(lane, 0), None, "nothing named at or above");
    assert_eq!(
        refs.named_row(lane, 1),
        Some(1),
        "a named commit names itself"
    );
    assert_eq!(refs.named_row(lane, 2), Some(1));
    assert_eq!(refs.tip(lane), Some(0));
    assert_eq!(refs.len(), 3);
}

#[test]
fn a_fork_curve_belongs_to_the_branch_that_ends_in_it() {
    // main: m2 -> base; feature: f1 -> base.
    let (rows, refs) = named(&[("m2", &["base"], B), ("f1", &["base"], B), ("base", &[], N)]);
    let feature = lane_of(&rows[2], EdgeKind::In, 1);
    assert_eq!(feature, rows[1].lane);
    assert_eq!(
        refs.named_row(feature, 2),
        Some(1),
        "the curve names feature"
    );
    assert_eq!(
        refs.named_row(rows[2].lane, 2),
        Some(0),
        "the commit it branched from is on main"
    );
    // The feature lane ended at the fork: nothing carries it on.
    assert!(!refs.carry().contains_key(&feature));
}

#[test]
fn a_merge_curve_belongs_to_the_merged_branch_named_below_it() {
    //   M (main, merge of m1 and f1)
    //   | \
    //   m1  f1 (feature)
    //   | /
    //   base
    let (rows, refs) = named(&[
        ("M", &["m1", "f1"], B),
        ("f1", &["base"], B),
        ("m1", &["base"], N),
        ("base", &[], N),
    ]);
    let merged = lane_of(&rows[0], EdgeKind::Out, 1);
    assert_eq!(
        refs.named_row(merged, 0),
        Some(1),
        "the curve opens a lane whose first commit is feature"
    );
    assert_eq!(refs.named_row(merged, 2), Some(1), "the line passing m1");
    assert_eq!(refs.named_row(rows[0].lane, 2), Some(0), "m1 is main's");
    assert_eq!(refs.node_at_or_below(merged, 0), Some(1));
}

#[test]
fn every_line_of_an_octopus_merge_is_named_after_its_own_branch() {
    let (rows, refs) = named(&[
        ("O", &["a", "b", "c", "d"], B),
        ("d", &["root"], B),
        ("c", &["root"], B),
        ("b", &["root"], N),
        ("a", &["root"], N),
        ("root", &[], N),
    ]);
    // Column 1 waits for b, 2 for c, 3 for d.
    let b = lane_of(&rows[0], EdgeKind::Out, 1);
    let c = lane_of(&rows[0], EdgeKind::Out, 2);
    let d = lane_of(&rows[0], EdgeKind::Out, 3);
    assert_eq!(refs.named_row(d, 0), Some(1));
    assert_eq!(refs.named_row(c, 0), Some(2));
    assert_eq!(refs.named_row(b, 0), None, "b carries no label");
    assert_eq!(
        refs.tip(b),
        Some(3),
        "so the line is described by its commit"
    );
    // The first parent continues main's lane.
    assert_eq!(refs.named_row(rows[0].lane, 4), Some(0));
}

#[test]
fn a_lane_with_no_label_has_no_name_but_a_tip() {
    // A branch merged and deleted: its commits carry nothing.
    let (rows, refs) = named(&[
        ("M", &["m1", "f2"], B),
        ("f2", &["f1"], N),
        ("f1", &["m1"], N),
        ("m1", &[], N),
    ]);
    let merged = lane_of(&rows[0], EdgeKind::Out, 1);
    for row in 0..4 {
        assert_eq!(refs.named_row(merged, row), None);
    }
    assert_eq!(refs.tip(merged), Some(1));
    assert_eq!(refs.named_row(rows[3].lane, 3), Some(0));
}

#[test]
fn two_labels_on_one_lane_each_name_the_lines_below_them() {
    // main on c3, release on c1.
    let (rows, refs) = named(&[
        ("c3", &["c2"], B),
        ("c2", &["c1"], N),
        ("c1", &["c0"], B),
        ("c0", &[], N),
    ]);
    let lane = rows[0].lane;
    assert_eq!(refs.named_row(lane, 1), Some(0));
    assert_eq!(refs.named_row(lane, 2), Some(2), "the nearer branch wins");
    assert_eq!(refs.named_row(lane, 3), Some(2));
}

#[test]
fn a_branch_names_a_lane_before_a_nearer_tag_and_a_tag_names_one_without() {
    // main on c3, a tag v1 on c1; an old line with only a tag, v0, on o1.
    let (rows, refs) = named(&[
        ("c3", &["c2"], B),
        ("c2", &["c1"], N),
        ("c1", &["c0"], T),
        ("o1", &["o0"], T),
        ("c0", &[], N),
        ("o0", &[], N),
    ]);
    let lane = rows[0].lane;
    assert_eq!(
        refs.named_row(lane, 2),
        Some(0),
        "main, not the tag below it"
    );
    assert_eq!(refs.named_row(lane, 4), Some(0));
    let old = rows[3].lane;
    assert_ne!(old, lane);
    assert_eq!(refs.named_row(old, 5), Some(3), "with no branch, the tag");
}

#[test]
fn a_line_that_moves_to_another_column_keeps_its_name_in_both() {
    // M merges f2 into main; f2's line curves from column 0 to column 1
    // in M's row and runs straight down column 1 past m1.
    let (rows, refs) = named(&[
        ("M", &["m1", "f2"], N),
        ("m1", &["base"], B),
        ("f2", &["base"], B),
        ("base", &[], N),
    ]);
    let curve = lane_of(&rows[0], EdgeKind::Out, 1);
    let straight = lane_of(&rows[1], EdgeKind::Pass, 1);
    assert_eq!(curve, straight, "the curve and the line are one lane");
    assert_eq!(refs.named_row(curve, 0), Some(2));
    assert_eq!(refs.named_row(straight, 1), Some(2));
    // main's own line, at M, has no label at or above it.
    assert_eq!(refs.named_row(rows[0].lane, 0), None);
    assert_eq!(refs.named_row(rows[0].lane, 1), Some(1));
}

#[test]
fn lane_names_laid_out_page_by_page_match_one_layout_of_everything() {
    let commits = shuffled(2_000, 0x5eed_1234);
    let label = |i: usize| match (i.is_multiple_of(37), i % 101 == 5) {
        (true, _) => LaneLabel::Branch,
        (false, true) => LaneLabel::Tag,
        _ => LaneLabel::None,
    };
    let mut whole_layout = LaneLayout::new();
    let mut whole = LaneRefs::new();
    let rows: Vec<GraphRow> = commits
        .iter()
        .enumerate()
        .map(|(i, (s, p))| {
            let row = whole_layout.push(s, p);
            whole.push(&row, label(i));
            row
        })
        .collect();
    let mut paged_layout = LaneLayout::new();
    let mut paged = LaneRefs::new();
    for (n, page) in commits.chunks(300).enumerate() {
        // Each page is built from what the one before carried, as the
        // background job does, and appended on the window's thread.
        let mut segment = LaneRefs::starting_at(paged.len(), paged.carry().clone());
        for (k, (s, p)) in page.iter().enumerate() {
            let row = paged_layout.push(s, p);
            segment.push(&row, label(n * 300 + k));
        }
        paged.append(segment);
    }
    assert_eq!(paged, whole);
    for (index, row) in rows.iter().enumerate() {
        for edge in &row.edges {
            assert_eq!(
                paged.named_row(edge.lane, index),
                whole.named_row(edge.lane, index)
            );
            assert_eq!(
                paged.named_above(edge.lane, index),
                whole.named_above(edge.lane, index)
            );
        }
    }
}

/// What names `lane` at `row`, worked out the slow way from the rows and
/// the commits alone, by the rule: among the lane's commits at or above
/// `row` (strictly above for the line into the commit at `row`), walk up
/// from the nearest for the nearest with a branch, else the nearest with a
/// tag; a lane with none above is named by its first commit's own labels,
/// and a lane that never held a commit by the commit it ended in.
fn oracle(
    rows: &[GraphRow],
    labels: &[LaneLabel],
    lane: u32,
    row: usize,
    inclusive: bool,
) -> Option<usize> {
    let own: Vec<usize> = (0..rows.len()).filter(|&i| rows[i].lane == lane).collect();
    let above: Vec<usize> = own
        .iter()
        .copied()
        .filter(|&i| if inclusive { i <= row } else { i < row })
        .collect();
    let mine = |i: usize| -> Option<usize> { (labels[i] != LaneLabel::None).then_some(i) };
    if above.is_empty() {
        let first = match own.first() {
            Some(&first) => first,
            None => (0..rows.len()).find(|&i| {
                rows[i]
                    .edges
                    .iter()
                    .any(|e| e.kind == EdgeKind::In && e.lane == lane && e.column != rows[i].column)
            })?,
        };
        return mine(first);
    }
    above
        .iter()
        .rev()
        .find(|&&i| labels[i] == LaneLabel::Branch)
        .or_else(|| above.iter().rev().find(|&&i| labels[i] == LaneLabel::Tag))
        .copied()
}

#[test]
fn lane_names_agree_with_the_rule_worked_out_the_slow_way() {
    let commits = shuffled(1_200, 0xfeed_0042);
    let labels: Vec<LaneLabel> = (0..commits.len())
        .map(|i| match (i.is_multiple_of(53), i % 29 == 3) {
            (true, _) => LaneLabel::Branch,
            (false, true) => LaneLabel::Tag,
            _ => LaneLabel::None,
        })
        .collect();
    let mut layout = LaneLayout::new();
    let mut refs = LaneRefs::new();
    let rows: Vec<GraphRow> = commits
        .iter()
        .zip(&labels)
        .map(|((s, p), label)| {
            let row = layout.push(s, p);
            refs.push(&row, *label);
            row
        })
        .collect();
    let at: std::collections::HashMap<&str, usize> = commits
        .iter()
        .enumerate()
        .map(|(i, (s, _))| (s.as_str(), i))
        .collect();
    let mut checked = 0;
    for (index, row) in rows.iter().enumerate() {
        for edge in &row.edges {
            let into_node = edge.kind == EdgeKind::In && edge.column == row.column;
            let found = if into_node {
                refs.named_above(edge.lane, index)
            } else {
                refs.named_row(edge.lane, index)
            };
            assert_eq!(
                found,
                oracle(&rows, &labels, edge.lane, index, !into_node),
                "{edge:?} in row {index}"
            );
            // A name found above the line is the line's own history: the
            // nearest commit of the lane at the line is reached from it by
            // first parents.
            if let Some(found) = found
                && found <= index
                && rows[found].lane == edge.lane
            {
                // (The line into a lane's first commit is named by that
                // commit itself: there is nothing to walk.)
                let Some(nearest) = (found..=index)
                    .rev()
                    .find(|&i| rows[i].lane == edge.lane && (i < index || !into_node))
                else {
                    continue;
                };
                let mut walk = found;
                while walk != nearest {
                    let first = commits[walk].1.first().expect("a first parent");
                    walk = at[first.as_str()];
                    assert!(walk <= nearest, "{found} does not lead to {nearest}");
                }
                checked += 1;
            }
        }
    }
    assert!(checked > 500, "only {checked} names checked");
}

#[test]
fn the_line_into_a_labelled_commit_is_named_from_above_it() {
    // main on m2, release on c: the line from m2 down into c is main's.
    let (rows, refs) = named(&[("m2", &["c"], B), ("c", &["base"], B), ("base", &[], N)]);
    let lane = rows[0].lane;
    assert_eq!(refs.named_above(lane, 1), Some(0), "the edge m2 -> c");
    assert_eq!(refs.named_row(lane, 1), Some(1), "the edge c -> base");
    // With nothing labelled above, nothing names it; the tip describes it.
    let (rows, refs) = named(&[("m2", &["c"], N), ("c", &["base"], B), ("base", &[], N)]);
    assert_eq!(refs.named_above(rows[0].lane, 1), None);
    assert_eq!(refs.tip(rows[0].lane), Some(0));
}

#[test]
fn a_lane_a_merge_opened_that_ends_without_a_commit_is_named_by_its_end() {
    // develop: C2 -> C1 -> P; main: M merges P (opening a lane in column
    // 2 while column 1 holds main), then main1 -> Q. P is taken by
    // develop's lane in column 0, so the merge's lane ends in P without a
    // commit of its own.
    let graph = |p: LaneLabel| {
        named(&[
            ("C2", &["C1"], B),
            ("M", &["main1", "P"], B),
            ("C1", &["P"], N),
            ("main1", &["Q"], N),
            ("P", &["Q"], p),
            ("Q", &[], N),
        ])
    };
    let (rows, refs) = graph(T);
    let merged = lane_of(&rows[1], EdgeKind::Out, 2);
    assert!(!refs.has_commits(merged));
    assert_eq!(lane_of(&rows[4], EdgeKind::In, 2), merged, "it ends in P");
    assert_eq!(refs.tip(merged), Some(4));
    assert_eq!(refs.named_row(merged, 1), Some(4), "P's own tag");
    assert_eq!(refs.named_row(merged, 3), Some(4));
    let (rows, refs) = graph(N);
    let merged = lane_of(&rows[1], EdgeKind::Out, 2);
    assert_eq!(
        refs.named_row(merged, 2),
        None,
        "develop's name above P is not this line's"
    );
    assert_eq!(refs.tip(merged), Some(4));
}
