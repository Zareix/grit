//! The commit graph: lane layout computed in-house (the approach Zed
//! proved in production — one ordered log pass, each edge child→parent
//! laid out as straight runs and quarter curves).
//!
//! Input is nothing but an ordered `&[Commit]` (children before parents,
//! as any log traversal produces): no shell-out, no ASCII art to parse.
//!
//! Output: the lane column + palette color of every commit, and the edge
//! lines to draw — each a list of segments spanning row intervals, so a
//! single overlay canvas can cull by viewport and paint full-height
//! bends (no per-row compression).
//!
//! Render contract (the overlay painter relies on it):
//! - a row's node sits at (column center, row center),
//! - `Straight { to_row }` runs the vertical to that row's center — or
//!   to the row's top when the next segment bends into the same row,
//! - `Curve { to_column, on_row }` is a full S-curve from the current
//!   point to (to_column, on_row)'s center.

use std::collections::HashMap;

use crate::Oid;
use crate::commit::Commit;

/// Row index meaning "still open" — the edge hasn't met its parent yet.
const OPEN: usize = usize::MAX;
/// Round-robin color slots; must match the painter's palette length.
pub(crate) const PALETTE_LEN: usize = 13;

/// One drawable piece of an edge line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Segment {
    /// Vertical run down to `to_row` (its center) in the current column.
    Straight { to_row: usize },
    /// S-curve to `to_column`, landing at row `on_row`'s center.
    Curve { to_column: usize, on_row: usize },
}

/// One edge line: child node → parent node (or the window's bottom when
/// the parent fell out of the log window).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EdgeLine {
    pub child_row: usize,
    pub child_column: usize,
    pub color: usize,
    /// Inclusive row interval the line paints across (viewport culling).
    pub first_row: usize,
    pub last_row: usize,
    pub segments: Vec<Segment>,
}

/// Lane column + palette color for one commit (indexes parallel to the
/// log).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommitLane {
    pub column: usize,
    pub color: usize,
}

/// The whole graph for an ordered log.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GraphData {
    /// One per commit, same indexes as the log.
    pub lanes: Vec<CommitLane>,
    pub lines: Vec<EdgeLine>,
    /// Number of columns the graph ever used (gutter width).
    pub max_lanes: usize,
}

/// An edge still being drawn, parked in its column.
struct OpenEdge {
    child_row: usize,
    child_column: usize,
    color: usize,
    segments: Vec<Segment>,
}

impl GraphData {
    /// Lay out the graph over an ordered log (children before parents).
    pub fn from_commits(commits: &[Commit]) -> GraphData {
        let mut lanes: Vec<CommitLane> = Vec::with_capacity(commits.len());
        let mut lines: Vec<EdgeLine> = Vec::new();

        // Columns occupied by open edges, indexed by column (`None` =
        // free). Grows with the topology; shrinks back when edges land.
        let mut open: Vec<Option<OpenEdge>> = Vec::new();
        // Parent oid → columns of edges that will land on it.
        let mut waiting: HashMap<Oid, Vec<usize>> = HashMap::new();
        // Color memoized per column: a column keeps its color for its
        // whole life, so a lane never changes color mid-history.
        let mut column_colors: Vec<usize> = Vec::new();
        let mut next_color: usize = 0;
        let mut max_lanes: usize = 0;

        for (row, commit) in commits.iter().enumerate() {
            // This commit's node column: the smallest column waiting for
            // it (its first child's edge keeps the straight track), else
            // the first free column.
            let arrivals = waiting.remove(&commit.id).unwrap_or_default();
            let column = arrivals.iter().copied().min().unwrap_or_else(|| {
                open.iter().position(|slot| slot.is_none()).unwrap_or(open.len())
            });

            while open.len() <= column {
                open.push(None);
                column_colors.push(next_color);
                next_color = (next_color + 1) % PALETTE_LEN;
            }
            let color = column_colors[column];
            lanes.push(CommitLane { column, color });
            max_lanes = max_lanes.max(column + 1);

            // Close every edge that landed on this commit: the one living
            // in the node's column ends straight at the node, the others
            // curve in (and their straight run stops one row higher).
            for arrival in arrivals {
                let mut edge = open[arrival].take().expect("arriving lane is open");
                if let Some(Segment::Straight { to_row: OPEN }) = edge.segments.last_mut() {
                    let to_row = if arrival == column { row } else { row - 1 };
                    *edge.segments.last_mut().unwrap() = Segment::Straight { to_row };
                }
                // A same-column landing ends at the node directly; the
                // bent ones curve in from the row above.
                if arrival != column {
                    edge.segments.push(Segment::Curve { to_column: column, on_row: row });
                }
                lines.push(EdgeLine {
                    child_row: edge.child_row,
                    child_column: edge.child_column,
                    color: edge.color,
                    first_row: edge.child_row,
                    last_row: row,
                    segments: edge.segments,
                });
            }

            // Open the commit's own edges: the first parent continues in
            // place; every additional parent (merge) forks a fresh column
            // starting with a curve.
            let mut parents = commit.parents.iter().copied();
            if let Some(first) = parents.next() {
                open[column] = Some(OpenEdge {
                    child_row: row,
                    child_column: column,
                    color,
                    segments: vec![Segment::Straight { to_row: OPEN }],
                });
                waiting.entry(first).or_default().push(column);

                for extra in parents {
                    let fork_column =
                        open.iter().position(|slot| slot.is_none()).unwrap_or(open.len());
                    while open.len() <= fork_column {
                        open.push(None);
                        column_colors.push(next_color);
                        next_color = (next_color + 1) % PALETTE_LEN;
                    }
                    open[fork_column] = Some(OpenEdge {
                        child_row: row,
                        // The edge DEPARTS from the merge node: the painter
                        // walks segments from (child_row, child_column), so
                        // the opening curve must start in the merge's own
                        // column and land in the fresh fork column at the
                        // next row's node.
                        child_column: column,
                        color: column_colors[fork_column],
                        segments: vec![
                            Segment::Curve { to_column: fork_column, on_row: row + 1 },
                            Segment::Straight { to_row: OPEN },
                        ],
                    });
                    waiting.entry(extra).or_default().push(fork_column);
                    max_lanes = max_lanes.max(fork_column + 1);
                }
            }
        }

        // Flush edges still open at the window's bottom: their parents
        // fell out of the limit, so they run to the last row.
        let last_row = commits.len().saturating_sub(1);
        for slot in open.iter_mut() {
            if let Some(mut edge) = slot.take() {
                if let Some(Segment::Straight { to_row: OPEN }) = edge.segments.last_mut() {
                    *edge.segments.last_mut().unwrap() = Segment::Straight { to_row: last_row };
                }
                lines.push(EdgeLine {
                    child_row: edge.child_row,
                    child_column: edge.child_column,
                    color: edge.color,
                    first_row: edge.child_row,
                    last_row,
                    segments: edge.segments,
                });
            }
        }

        GraphData { lanes, lines, max_lanes }
    }
}
