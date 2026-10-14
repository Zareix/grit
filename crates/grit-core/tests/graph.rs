//! Pure lane-layout tests: no repos, just ordered `Vec<Commit>` —
//! the layout's contract is children-before-parents ordering.

use grit_core::Oid;
use grit_core::commit::{Commit, Signature};
use grit_core::graph::{GraphData, Segment};

/// Commit `id` (as u8 → oid, encoded in the last byte) with the given
/// parent ids.
fn oid(id: u8) -> Oid {
    let mut bytes = [0u8; 20];
    bytes[19] = id;
    Oid::from_bytes(&bytes).expect("oid")
}

fn c(id: u8, parents: &[u8]) -> Commit {
    let sig = |name: &str| Signature {
        name: name.to_owned(),
        email: "test@example.com".to_owned(),
        time: git2::Time::new(0, 0),
    };
    Commit {
        id: oid(id),
        summary: format!("commit {id}"),
        message: None,
        author: sig("Test User"),
        committer: sig("Test User"),
        parents: parents.iter().map(|p| oid(*p)).collect(),
    }
}

#[test]
fn linear_history_is_one_straight_lane() {
    let graph = GraphData::from_commits(&[c(2, &[1]), c(1, &[0]), c(0, &[])]);

    // Every node in the leftmost column, same (first) color.
    assert!(graph.lanes.iter().all(|lane| lane.column == 0));
    assert_eq!(graph.lanes[0], graph.lanes[1]);
    assert_eq!(graph.max_lanes, 1);

    // One edge per consecutive pair: straight, child→parent, spanning
    // exactly both rows.
    assert_eq!(graph.lines.len(), 2);
    for (ix, edge) in graph.lines.iter().enumerate() {
        assert_eq!(edge.child_row, ix);
        assert_eq!(edge.child_column, 0);
        assert_eq!(edge.first_row, ix);
        assert_eq!(edge.last_row, ix + 1);
        assert_eq!(edge.segments, vec![Segment::Straight { to_row: ix + 1 }]);
    }
}

#[test]
fn fork_and_merge_open_and_close_a_second_lane() {
    // m merges a and b; both are children of root.
    let graph = GraphData::from_commits(&[c(3, &[1, 2]), c(2, &[0]), c(1, &[0]), c(0, &[])]);

    // m (row0) in column 0, b (row1) forks into column 1, a (row2) back
    // to column 0, root (row3) column 0.
    assert_eq!(graph.lanes[0].column, 0);
    assert_eq!(graph.lanes[1].column, 1);
    assert_eq!(graph.lanes[2].column, 0);
    assert_eq!(graph.lanes[3].column, 0);
    assert_eq!(graph.max_lanes, 2);

    // Four edges: m→a (straight), m→b (fork curve then straight), b→root
    // (straight), a→root (straight).
    assert_eq!(graph.lines.len(), 4);

    // The fork edge curves into the fresh column, landing at row 1's
    // center, after the straight run to the bottom.
    let fork = graph
        .lines
        .iter()
        .find(|edge| edge.segments.len() == 2)
        .expect("exactly one edge carries the fork curve");
    assert_eq!(fork.child_row, 0);
    assert_eq!(fork.segments[0], Segment::Curve { to_column: 1, on_row: 1 });
    assert_eq!(fork.segments[1], Segment::Straight { to_row: 1 });
    assert_eq!(fork.first_row, 0);
    assert_eq!(fork.last_row, 1);
}

#[test]
fn merge_fork_edge_departs_from_the_merge_node() {
    // The fork edge must START at the merge node (the child_column is
    // the merge's own column), then curve into the fresh lane — not
    // appear as a detached vertical in the fork column.
    let graph = GraphData::from_commits(&[c(3, &[1, 2]), c(2, &[0]), c(1, &[0]), c(0, &[])]);
    let fork = graph.lines.iter().find(|edge| edge.segments.len() == 2).expect("the fork edge");
    assert_eq!(fork.child_row, 0);
    assert_eq!(fork.child_column, 0, "the fork edge departs from the merge node in column 0");
    assert_eq!(fork.segments[0], Segment::Curve { to_column: 1, on_row: 1 });
}

#[test]
fn colors_round_robin_and_are_stable_per_column() {
    let graph = GraphData::from_commits(&[c(3, &[1, 2]), c(2, &[0]), c(1, &[0]), c(0, &[])]);
    // Column 0 took the first color, column 1 the second; both rows back
    // in column 0 keep the first color.
    assert_eq!(graph.lanes[0].color, 0);
    assert_eq!(graph.lanes[1].color, 1);
    assert_eq!(graph.lanes[2].color, 0);
    assert_eq!(graph.lanes[3].color, 0);
}

#[test]
fn edges_open_at_the_window_bottom_run_to_the_last_row() {
    // Parent 1 is outside the window: the edge runs to the bottom edge.
    let graph = GraphData::from_commits(&[c(1, &[0]), c(0, &[9])]);
    assert_eq!(graph.lines.len(), 2);
    let last = graph.lines.last().unwrap();
    assert_eq!(last.last_row, 1);
    assert_eq!(last.segments, vec![Segment::Straight { to_row: 1 }]);
}

#[test]
fn merge_commit_reuses_its_first_childs_column() {
    // Two children (rows 0,1) of the same parent, then the parent (row2):
    // the parent takes the smallest waiting column; the other edge bends.
    let graph = GraphData::from_commits(&[c(2, &[0]), c(1, &[0]), c(0, &[])]);
    assert_eq!(graph.lanes[0].column, 0);
    assert_eq!(graph.lanes[1].column, 1);
    assert_eq!(graph.lanes[2].column, 0);

    // Edge from row1 bends left into the parent's column at row2.
    let bent = graph.lines.iter().find(|edge| edge.child_row == 1).unwrap();
    assert_eq!(bent.segments.last(), Some(&Segment::Curve { to_column: 0, on_row: 2 }));
}
