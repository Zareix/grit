//! The graph column: git's own ASCII art re-painted as lanes.
//!
//! `git log --graph` computes the lane layout; we only translate its art
//! into pixels. Every art line of a row owns a vertical *slat* of the
//! cell (n art lines → n slats), and git's continuity guarantees the
//! pixels line up across rows: a column is a logical lane, `\` bends one
//! column right at the next slat boundary, swaths meet exactly.
//!
//! Colors come from the theme's semantic tokens, cycled by column so a
//! lane keeps its color while it survives (no raw hex, per repo rule).

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::*;
use grit_core::graph::GraphRow;

use super::GRAPH_W;

/// Pixel width of one lane column (5 lanes fit in `GRAPH_W` = 96).
const LANE_W: f32 = 18.;
/// Node dot radius, painted on top of the passing lanes.
const DOT_R: f32 = 3.5;

/// Does `line` carry lane `lane` crossing this slat vertically? A node
/// or vertical at the lane's own char column, or a bend arriving at the
/// boundary into it (`/` from the right, `\` from the left).
fn lane_passes(line: Option<&String>, lane: usize) -> bool {
    let Some(line) = line else { return false };
    let chars: Vec<char> = line.chars().collect();
    let own = lane * 2;
    matches!(chars.get(own), Some('|') | Some('*'))
        || chars.get(own + 1) == Some(&'/')
        || (own > 0 && chars.get(own - 1) == Some(&'\\'))
}

/// Does the NEXT slat take this lane away on a curve instead? True when
/// a bend touches the lane on either boundary: the curve departs from the
/// node itself and the straight stub would double it.
fn lane_departs(line: Option<&String>, lane: usize) -> bool {
    let Some(line) = line else { return false };
    let chars: Vec<char> = line.chars().collect();
    let own = lane * 2;
    chars.get(own + 1) == Some(&'\\') || (own > 0 && chars.get(own - 1) == Some(&'/'))
}

/// The graph cell for one log row. Missing art (`None` — first frame
/// before the snapshot lands) falls back to the plain muted dot.
pub(super) fn graph_lane_cell(
    row: Option<&GraphRow>,
    prev: Option<&GraphRow>,
    next: Option<&GraphRow>,
    cx: &App,
) -> AnyElement {
    let Some(row) = row else {
        return fallback_dot(cx).into_any_element();
    };
    let mut art = row.art.clone();
    art.retain(|line| !line.is_empty());
    if art.is_empty() {
        return fallback_dot(cx).into_any_element();
    }
    let palette = lane_palette(cx);
    let ring = cx.theme().background;
    // Neighbour rows' art drive the node stubs: a lane that enters from
    // nowhere (branch tip) gets no half-line above, a lane that starts
    // here (first divergent node) gets none below.
    let prev_art = prev.and_then(|p| {
        let mut a = p.art.clone();
        a.retain(|l| !l.is_empty());
        a.into_iter().next_back()
    });
    let next_art = next.and_then(|n| {
        let mut a = n.art.clone();
        a.retain(|l| !l.is_empty());
        a.into_iter().next_back()
    });

    div()
        .flex_none()
        .w(px(GRAPH_W))
        .h_full()
        .relative()
        .child(
            canvas(
                move |bounds, _, _| {
                    (bounds, art.clone(), palette.clone(), ring, prev_art.clone(), next_art.clone())
                },
                |_, (bounds, art, palette, ring, prev_art, next_art), window, _| {
                    let height = bounds.size.height.as_f32();
                    let slots = art.len() as f32;

                    // git's text grid is TWO chars per lane: nodes and
                    // verticals sit on even columns, bends (`\` `/`) on the
                    // odd boundary between two lanes. Char → lane mapping:
                    // an even column `c` is lane `c/2`; an odd bend column
                    // `c` joins lanes `(c-1)/2` and `(c+1)/2`.
                    let lane_of_char = |char_col: usize| char_col as f32 / 2.;
                    let lane_left_of = |char_col: usize| (char_col as f32 - 1.) / 2.;
                    let lane_right_of = |char_col: usize| (char_col as f32 + 1.) / 2.;
                    // Lane x centers; clamp so a very deep lane does not
                    // paint outside the cell (git rarely produces >5 wide).
                    let x =
                        |lane: f32| bounds.left() + px((LANE_W * lane + LANE_W / 2.).min(GRAPH_W));
                    let slat = |li: usize| {
                        let top = bounds.top() + px(height * li as f32 / slots);
                        let bottom = bounds.top() + px(height * (li + 1) as f32 / slots);
                        let mid = (top + bottom) / 2.;
                        (top, bottom, mid)
                    };
                    // A bend starts from the node above when the slat above
                    // carries the lane's own node — full-height curves
                    // instead of a stubby hook inside the continuation slat.
                    // The node sits on the bend's ORIGIN lane: for `\` that
                    // is the lane left of the boundary (char `cc-1`), for
                    // `/` the lane right of it (char `cc+1`).
                    let node_origin =
                        |art: &[String], li: usize, origin_char: usize| -> (Pixels, Pixels) {
                            if li > 0 && art[li - 1].chars().nth(origin_char) == Some('*') {
                                (x(lane_of_char(origin_char)), slat(li - 1).2)
                            } else {
                                (x(lane_of_char(origin_char)), slat(li).0)
                            }
                        };

                    // Pass one: the passing lanes. Pass two: the nodes, so
                    // a dot always paints over the lines it sits on.
                    for (li, line) in art.iter().enumerate() {
                        let (top, bottom, mid) = slat(li);
                        for (cc, glyph) in line.chars().enumerate() {
                            let color = lane_color(&palette, lane_of_char(cc) as usize);
                            match glyph {
                                '|' => {
                                    paint_stroke(
                                        point(x(lane_of_char(cc)), top),
                                        point(x(lane_of_char(cc)), bottom),
                                        color,
                                        window,
                                    );
                                }
                                // A `*` implies the lane passes through: git
                                // prints no continuation line for it on
                                // straight history — draw it here so
                                // consecutive nodes stay connected. Each
                                // half is drawn only when the lane really
                                // passes on that side: a lane tip (nothing
                                // above) gets no stub, a first divergent
                                // node gets none below.
                                '*' => {
                                    let lane_index = lane_of_char(cc) as usize;
                                    let passes_above =
                                        li > 0 || lane_passes(prev_art.as_ref(), lane_index);
                                    if passes_above {
                                        paint_stroke(
                                            point(x(lane_index as f32), top),
                                            point(x(lane_index as f32), mid),
                                            color,
                                            window,
                                        );
                                    }
                                    // Below the node: connect straight ONLY
                                    // when the lane keeps running vertically.
                                    // When a bend takes over right under the
                                    // node (a `\` or `/` on either boundary
                                    // touching this lane), the curve already
                                    // leaves the node horizontally — the
                                    // stub would double it.
                                    let departs = lane_departs(next_art.as_ref(), lane_index);
                                    if !departs {
                                        paint_stroke(
                                            point(x(lane_index as f32), mid),
                                            point(x(lane_index as f32), bottom),
                                            color,
                                            window,
                                        );
                                    }
                                }
                                '\\' => {
                                    let (from_x, from_y) = if cc > 0 {
                                        node_origin(&art, li, cc - 1)
                                    } else {
                                        (x(0.), top)
                                    };
                                    // The curve belongs to the ORIGIN lane —
                                    // the branch line it continues (GitKraken
                                    // keeps a branch's color through its
                                    // merge curve), not the destination.
                                    let from_lane = lane_of_char(cc - 1) as usize;
                                    paint_bend(
                                        point(from_x, from_y),
                                        x(lane_right_of(cc)),
                                        bottom,
                                        lane_color(&palette, from_lane),
                                        window,
                                    );
                                }
                                '/' => {
                                    let (from_x, from_y) = node_origin(&art, li, cc + 1);
                                    // Same rule, mirrored: origin lane on the
                                    // right of the boundary.
                                    let from_lane = lane_of_char(cc + 1) as usize;
                                    paint_bend(
                                        point(from_x, from_y),
                                        x(lane_left_of(cc)),
                                        bottom,
                                        lane_color(&palette, from_lane),
                                        window,
                                    );
                                }
                                '-' | '_' => {
                                    paint_stroke(
                                        point(x(lane_left_of(cc)), mid),
                                        point(x(lane_right_of(cc)), mid),
                                        color,
                                        window,
                                    );
                                }
                                _ => {}
                            }
                        }
                    }
                    for (li, line) in art.iter().enumerate() {
                        let (_, _, mid) = slat(li);
                        for (cc, glyph) in line.chars().enumerate() {
                            if glyph == '*' {
                                paint_dot(
                                    x(lane_of_char(cc)),
                                    mid,
                                    lane_color(&palette, lane_of_char(cc) as usize),
                                    ring,
                                    window,
                                );
                            }
                        }
                    }
                },
            )
            .absolute()
            .size_full(),
        )
        .into_any_element()
}

/// Plain muted dot for rows without art.
fn fallback_dot(cx: &App) -> Div {
    div()
        .flex_none()
        .w(px(GRAPH_W))
        .h_full()
        .flex()
        .justify_center()
        .child(div().size_2().rounded_full().bg(cx.theme().muted.opacity(0.6)))
}

/// Semantic lane palette (cycled by column index).
fn lane_palette(cx: &App) -> Vec<Hsla> {
    vec![
        cx.theme().info,
        cx.theme().success,
        cx.theme().warning,
        cx.theme().warning,
        cx.theme().accent,
    ]
}

fn lane_color(palette: &[Hsla], col: usize) -> Hsla {
    palette[col % palette.len().max(1)]
}

/// Straight segment between two points.
fn paint_stroke(from: Point<Pixels>, to: Point<Pixels>, color: Hsla, window: &mut Window) {
    let mut builder = PathBuilder::stroke(px(2.));
    builder.move_to(from);
    builder.line_to(to);
    if let Ok(path) = builder.build() {
        window.paint_path(path, color);
    }
}

/// S-curve from `from_x / top` to `to_x / bottom`: control points sit at
/// the horizontal midpoint with the endpoints' y, so both ends leave and
/// land horizontally — the GitKraken-looking bend for `\` and `/`.
fn paint_bend(from: Point<Pixels>, to_x: Pixels, bottom: Pixels, color: Hsla, window: &mut Window) {
    let mid_x = (from.x + to_x) / 2.;
    let mut builder = PathBuilder::stroke(px(2.));
    builder.move_to(from);
    // Doc note: `cubic_bezier_to` takes the END point first, then controls.
    builder.cubic_bezier_to(point(to_x, bottom), point(mid_x, from.y), point(mid_x, bottom));
    if let Ok(path) = builder.build() {
        window.paint_path(path, color);
    }
}

/// Filled dot with a background-colored ring, so it separates from any
/// lane passing beneath it.
fn paint_dot(cx_dot: Pixels, cy: Pixels, color: Hsla, ring: Hsla, window: &mut Window) {
    let halo = px(DOT_R + 0.6);
    // Ring first (outsized, background color), then the dot over it.
    let mut halo_builder = PathBuilder::fill();
    halo_builder.move_to(point(cx_dot + halo, cy));
    halo_builder.arc_to(point(halo, halo), px(0.), false, true, point(cx_dot - halo, cy));
    halo_builder.arc_to(point(halo, halo), px(0.), false, true, point(cx_dot + halo, cy));
    halo_builder.close();
    if let Ok(path) = halo_builder.build() {
        window.paint_path(path, ring);
    }

    let radius = px(DOT_R);
    let mut builder = PathBuilder::fill();
    builder.move_to(point(cx_dot + radius, cy));
    // `arc_to(radii, rotation, large_arc, sweep, end)` — SVG `A` order.
    builder.arc_to(point(radius, radius), px(0.), false, true, point(cx_dot - radius, cy));
    builder.arc_to(point(radius, radius), px(0.), false, true, point(cx_dot + radius, cy));
    builder.close();
    if let Ok(path) = builder.build() {
        window.paint_path(path, color);
    }
}
