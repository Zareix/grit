//! The graph column: ONE overlay canvas painting the whole commit list
//! (the Zed-proved layout — not one canvas per row).
//!
//! The overlay reads the list's scroll offset every frame, culls edge
//! lines whose row interval misses the viewport, and paints:
//! 1. edges, grouped per color in their own paint layer (crossing lines
//!    of different colors never tint each other),
//! 2. commit nodes last: a three-fill circle (background halo, colored
//!    ring, background hole) so they read as rings over the lanes.

use std::collections::BTreeMap;

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::*;
use grit_core::graph::{EdgeLine, GraphData, Segment};

use crate::workspace::Workspace;

/// Lane palette (token layer): 13 mid-tone colors that read on both
/// appearances. Round-robin per lane column, memoized there.
const PALETTE_HEX: [u32; 13] = [
    0x0090ff, 0xff9d33, 0xff66c6, 0x99d52a, 0x7a5cff, 0xffb224, 0x2db398, 0xe05555, 0x2bc7bd,
    0xccae15, 0x43a865, 0x5d68ff, 0x6a5cf0,
];

/// Pixel width of one lane column.
pub(super) const LANE_W: f32 = 18.;

/// The graph gutter width for a log: one slot per lane the layout ever
/// used, clamped to [3, 8] lanes — wide enough to keep the whole main
/// line visible in skewed topologies (rebased tips sharing a base far
/// below), narrow enough to leave the title column room. Deeper lanes
/// are simply cut off at the gutter edge (the overlay clips its paint).
/// Shared by the header, every row's empty slot and the overlay.
pub(super) fn gutter_width(graph: Option<&GraphData>) -> Pixels {
    let lanes = graph.map_or(3, |g| g.max_lanes.clamp(3, 8));
    px(lanes as f32 * LANE_W)
}

/// Geometry (log rows are `h_7` = 1.75rem tall).
const LINE_W: f32 = 2.;
const DOT_R: f32 = 4.5;
const DOT_HALO_W: f32 = 2.;

/// The overlay canvas: paint-only, no hitbox — clicks pass through to
/// the list underneath. Anchored over the log's graph column (see
/// `GRAPH_COL_LEFT` there), `gutter_w` wide, so lane centers align with
/// the empty slots every row keeps.
pub(super) fn graph_overlay(
    workspace: &WeakEntity<Workspace>,
    gutter_w: Pixels,
) -> impl IntoElement {
    let workspace = workspace.clone();
    canvas(
        move |_, _, _| workspace,
        move |bounds, workspace, window, cx| {
            let Some(entity) = workspace.upgrade() else { return };
            let ws = entity.read(cx);
            let Some(graph) = &ws.graph else { return };
            let row_h = row_height(window);
            if row_h <= px(0.) {
                return;
            }
            let has_wip = usize::from(!ws.status.is_empty());
            let lane_w = px(LANE_W);

            let scroll = (-ws.log_scroll.0.borrow().base_handle.offset().y).max(px(0.));
            let first_visible = (scroll / row_h).floor() as usize;
            // Rows visible = commit rows + the WIP row (painted above).
            let last_visible = (((scroll + bounds.size.height) / row_h).ceil() as usize)
                .min(graph.lanes.len() + has_wip);

            let x = |column: usize| bounds.left() + lane_w * column as f32 + lane_w / 2.;
            // Row Y in CANVAS coordinates: the list's logical position
            // minus the scroll offset (the overlay never moves; the rows
            // under it do).
            let rc = |row: usize| bounds.top() + row_h * row as f32 + row_h / 2. - scroll;

            // Everything clips to the canvas bounds: a topology deeper
            // than the gutter's lane cap paints up to the edge and stops
            // — never over the title column.
            window.with_content_mask(Some(ContentMask { bounds }), |window| {
                // 1. Edges, grouped per color, culled by row interval.
                let mut by_color: BTreeMap<usize, Vec<PathBuilder>> = BTreeMap::new();
                for line in &graph.lines {
                    let first = line.first_row + has_wip;
                    let last = line.last_row + has_wip;
                    if last < first_visible || first > last_visible {
                        continue;
                    }
                    by_color
                        .entry(line.color)
                        .or_default()
                        .push(build_line(line, &x, &rc, has_wip));
                }
                for (color, builders) in by_color {
                    let color_hsla = palette_color(color);
                    window.paint_layer(bounds, |window| {
                        for builder in builders {
                            if let Ok(path) = builder.build() {
                                window.paint_path(path, color_hsla);
                            }
                        }
                    });
                }

                // 2. Nodes, painted last so they sit over the lanes.
                for row in first_visible..last_visible {
                    let commit_ix = row.checked_sub(has_wip);
                    let Some(commit_ix) = commit_ix else { continue };
                    let Some(lane) = graph.lanes.get(commit_ix) else { continue };
                    draw_commit_circle(
                        x(lane.column),
                        rc(row),
                        palette_color(lane.color),
                        cx.theme().background,
                        window,
                    );
                }
            });
        },
    )
    .absolute()
    .top_0()
    .bottom_0()
    .left(px(super::GRAPH_COL_LEFT))
    .w(gutter_w)
}

fn build_line(
    line: &EdgeLine,
    x: &dyn Fn(usize) -> Pixels,
    rc: &dyn Fn(usize) -> Pixels,
    has_wip: usize,
) -> PathBuilder {
    let mut builder = PathBuilder::stroke(px(LINE_W));
    builder.move_to(point(x(line.child_column), rc(line.child_row + has_wip)));

    let mut column = line.child_column;
    for segment in line.segments.iter() {
        match segment {
            Segment::Straight { to_row } => {
                builder.line_to(point(x(column), rc(*to_row + has_wip)));
            }
            Segment::Curve { to_column, on_row } => {
                // Departure = the row above's center (the straight run
                // before us always ends there); the S lands at this row's
                // node. Full-height, never compressed.
                let from_y = rc(on_row - 1 + has_wip);
                let to_y = rc(*on_row + has_wip);
                let mid_y = (from_y + to_y) / 2.;
                // Doc note: `cubic_bezier_to` takes the END first.
                builder.cubic_bezier_to(
                    point(x(*to_column), to_y),
                    point(x(column), mid_y),
                    point(x(*to_column), mid_y),
                );
                column = *to_column;
            }
        }
    }
    builder
}

/// Row height of the log rows (`h_7`), snapped to the physical pixel so
/// the canvas math agrees with `uniform_list`'s device-pixel snapping.
fn row_height(window: &Window) -> Pixels {
    let raw = window.rem_size() * 1.75;
    let scale = window.scale_factor();
    (raw * scale).round() / scale
}

fn palette_color(index: usize) -> Hsla {
    rgb(PALETTE_HEX[index % PALETTE_HEX.len()]).into()
}

/// Ring-style commit node: background halo (masks crossing lanes),
/// colored disc, background hole.
fn draw_commit_circle(
    cx_dot: Pixels,
    cy: Pixels,
    color: Hsla,
    background: Hsla,
    window: &mut Window,
) {
    fill_circle(cx_dot, cy, px(DOT_R + DOT_HALO_W), background, window);
    fill_circle(cx_dot, cy, px(DOT_R), color, window);
    fill_circle(cx_dot, cy, px((DOT_R - LINE_W).max(0.)), background, window);
}

/// Filled circle via two `arc_to` halves (SVG `A` semantics).
fn fill_circle(cx_dot: Pixels, cy: Pixels, radius: Pixels, color: Hsla, window: &mut Window) {
    let mut builder = PathBuilder::fill();
    builder.move_to(point(cx_dot + radius, cy));
    builder.arc_to(point(radius, radius), px(0.), false, true, point(cx_dot - radius, cy));
    builder.arc_to(point(radius, radius), px(0.), false, true, point(cx_dot + radius, cy));
    builder.close();
    if let Ok(path) = builder.build() {
        window.paint_path(path, color);
    }
}
