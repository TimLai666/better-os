//! The branch graph's lane layout, case by case, with every result checked
//! for continuity: what leaves the bottom of one row is exactly what enters
//! the top of the next, in the same columns and lanes.

use git_core::{Edge, EdgeKind, GraphRow, LaneLayout};

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
