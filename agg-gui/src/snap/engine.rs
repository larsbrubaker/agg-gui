//! Pure snap engine — no widget / event dependencies.
//!
//! Implements the same two-tier algorithm as Node Designer's
//! `smart-snap.js`:
//!
//! 1. **Edge / centre alignment**.  Six snap points per rect (left,
//!    right, centre-x, top, bottom, centre-y).  For each moving edge
//!    we look at every same-axis target point within `threshold`; the
//!    smallest delta wins per axis.
//!
//! 2. **Equal-spacing**.  Run only for [`SnapMode::Move`].  Find a
//!    left + right neighbour (horizontally) or top + bottom
//!    neighbour (vertically), compute the symmetric centre, and snap
//!    if the moving rect lands inside `threshold` of that centre.
//!
//! Resize mode skips spacing detection and restricts alignment to
//! the edges the active handle actually controls (a resize from the
//! right edge can't snap the moving rect's LEFT side).
//!
//! The engine is stateless — escape-accumulator logic lives at the
//! call site (each frame's drag delta is passed in already
//! integrated by the drag handler).  That keeps `compute_snap` a
//! pure function and easy to test.

use crate::geometry::Rect;

use super::model::{ResizeEdge, SnapGuide, SnapId, SnapMode, SnapResult};

mod spacing;
pub(super) use spacing::{horizontal_equal_spacing, vertical_equal_spacing};
use spacing::{horizontal_resize_spacing, vertical_resize_spacing};

/// Snap a candidate rect against a set of stationary targets.
///
/// - `moving`: the rect produced by the raw drag (un-snapped).
/// - `moving_id`: identity used to skip self-matches inside `targets`.
/// - `targets`: every other snappable rect in the scene.
/// - `threshold`: maximum pixel distance for a snap to engage.
/// - `mode`: [`SnapMode::Move`] vs [`SnapMode::Resize`].
///
/// Returns the (possibly adjusted) rect plus visual guides.
pub fn compute_snap(
    moving: Rect,
    moving_id: SnapId,
    targets: &[(SnapId, Rect)],
    threshold: f64,
    mode: SnapMode,
) -> SnapResult {
    if targets.is_empty() || threshold <= 0.0 {
        return SnapResult {
            rect: moving,
            guides: Vec::new(),
        };
    }

    // Filter once and reuse — skipping self every iteration would
    // dominate the inner loops for large scenes.
    let neighbours: Vec<Rect> = targets
        .iter()
        .filter_map(|(id, r)| (id != &moving_id).then_some(*r))
        .collect();

    if neighbours.is_empty() {
        return SnapResult {
            rect: moving,
            guides: Vec::new(),
        };
    }

    let mut rect = moving;
    let mut guides: Vec<SnapGuide> = Vec::new();

    // ── Phase 1: edge alignment ──────────────────────────────────
    let allow_left = match mode {
        SnapMode::Move => true,
        SnapMode::Resize(e) => e.affects_left(),
    };
    let allow_right = match mode {
        SnapMode::Move => true,
        SnapMode::Resize(e) => e.affects_right(),
    };
    let allow_top = match mode {
        SnapMode::Move => true,
        SnapMode::Resize(e) => e.affects_top(),
    };
    let allow_bottom = match mode {
        SnapMode::Move => true,
        SnapMode::Resize(e) => e.affects_bottom(),
    };
    // Centre snaps only make sense when BOTH edges on that axis are
    // free — otherwise sliding the centre would have to move an edge
    // that the resize handle isn't supposed to touch.
    let allow_cx = allow_left && allow_right;
    let allow_cy = allow_top && allow_bottom;

    // X axis: collect candidate offsets, keep the smallest |delta|.
    let mut x_edge_engaged = false;
    let mut y_edge_engaged = false;
    if let Some(snap) = best_x_alignment(
        rect,
        &neighbours,
        threshold,
        allow_left,
        allow_right,
        allow_cx,
    ) {
        if matches!(mode, SnapMode::Move) {
            // Translate the whole rect — both edges move by `delta`.
            rect.x += snap.delta;
        } else if let SnapMode::Resize(e) = mode {
            apply_resize_x(&mut rect, snap.edge, snap.delta, e);
        }
        guides.push(SnapGuide::VLine {
            x: snap.x,
            y0: y_span(rect, snap.target_rect).0,
            y1: y_span(rect, snap.target_rect).1,
        });
        x_edge_engaged = true;
    }

    // Y axis: same shape, swap horizontal ↔ vertical.
    if let Some(snap) = best_y_alignment(
        rect,
        &neighbours,
        threshold,
        allow_top,
        allow_bottom,
        allow_cy,
    ) {
        if matches!(mode, SnapMode::Move) {
            rect.y += snap.delta;
        } else if let SnapMode::Resize(e) = mode {
            apply_resize_y(&mut rect, snap.edge, snap.delta, e);
        }
        guides.push(SnapGuide::HLine {
            y: snap.y,
            x0: x_span(rect, snap.target_rect).0,
            x1: x_span(rect, snap.target_rect).1,
        });
        y_edge_engaged = true;
    }

    // ── Phase 2: equal-spacing.
    //
    // Precedence is PER AXIS: edge alignment on the X axis
    // suppresses horizontal spacing only — vertical spacing stays
    // free to engage independently, and vice versa.  An X-axis edge
    // snap and a Y-axis spacing snap are not competing explanations,
    // so they can co-exist.
    match mode {
        SnapMode::Move => {
            if !x_edge_engaged {
                if let Some(spacing) = horizontal_equal_spacing(rect, &neighbours, threshold) {
                    rect.x += spacing.delta;
                    // Each helper picks the right number of dimension
                    // lines for the case it matched (two for sandwich
                    // flanking the moving rect, two for a chain
                    // extension showing both reference + matched gap).
                    guides.extend(spacing.guides);
                }
            }
            if !y_edge_engaged {
                if let Some(spacing) = vertical_equal_spacing(rect, &neighbours, threshold) {
                    rect.y += spacing.delta;
                    guides.extend(spacing.guides);
                }
            }
        }
        SnapMode::Resize(e) => {
            // Resize spacing — match the dragged edge's gap to a
            // reference gap somewhere in the scene.  Only the edge
            // the active handle controls is allowed to move.
            if !x_edge_engaged {
                if let Some(s) = horizontal_resize_spacing(rect, &neighbours, threshold, e) {
                    apply_resize_x_spacing(&mut rect, e, s.delta);
                    guides.extend(s.guides);
                }
            }
            if !y_edge_engaged {
                if let Some(s) = vertical_resize_spacing(rect, &neighbours, threshold, e) {
                    apply_resize_y_spacing(&mut rect, e, s.delta);
                    guides.extend(s.guides);
                }
            }
        }
    }

    SnapResult { rect, guides }
}

// ── Alignment internals ──────────────────────────────────────────

/// Which edge of the MOVING rect produced the alignment match.  Used
/// when `SnapMode::Resize` so we apply `delta` to the correct edge
/// instead of translating the whole rect.
#[derive(Clone, Copy, Debug)]
enum MovingEdge {
    Left,
    Right,
    CenterX,
    Top,
    Bottom,
    CenterY,
}

#[derive(Clone, Copy, Debug)]
struct AlignmentX {
    delta: f64,
    x: f64,
    edge: MovingEdge,
    target_rect: Rect,
}

#[derive(Clone, Copy, Debug)]
struct AlignmentY {
    delta: f64,
    y: f64,
    edge: MovingEdge,
    target_rect: Rect,
}

fn best_x_alignment(
    moving: Rect,
    targets: &[Rect],
    threshold: f64,
    allow_left: bool,
    allow_right: bool,
    allow_cx: bool,
) -> Option<AlignmentX> {
    let mut best: Option<AlignmentX> = None;
    let m_left = moving.x;
    let m_right = moving.x + moving.width;
    let m_cx = moving.x + moving.width * 0.5;
    for t in targets {
        let t_left = t.x;
        let t_right = t.x + t.width;
        let t_cx = t.x + t.width * 0.5;
        let moving_edges: [(MovingEdge, f64, bool); 3] = [
            (MovingEdge::Left, m_left, allow_left),
            (MovingEdge::Right, m_right, allow_right),
            (MovingEdge::CenterX, m_cx, allow_cx),
        ];
        let target_points = [t_left, t_right, t_cx];
        for (edge, mv, enabled) in moving_edges.iter().copied() {
            if !enabled {
                continue;
            }
            for tp in target_points {
                let delta = tp - mv;
                if delta.abs() <= threshold {
                    let cand = AlignmentX {
                        delta,
                        x: tp,
                        edge,
                        target_rect: *t,
                    };
                    best = match best {
                        None => Some(cand),
                        Some(prev) if delta.abs() < prev.delta.abs() => Some(cand),
                        Some(prev) => Some(prev),
                    };
                }
            }
        }
    }
    best
}

fn best_y_alignment(
    moving: Rect,
    targets: &[Rect],
    threshold: f64,
    allow_top: bool,
    allow_bottom: bool,
    allow_cy: bool,
) -> Option<AlignmentY> {
    let mut best: Option<AlignmentY> = None;
    // Y-up: rect.y is the BOTTOM edge.  "Top" in user terms = y + height.
    let m_bottom = moving.y;
    let m_top = moving.y + moving.height;
    let m_cy = moving.y + moving.height * 0.5;
    for t in targets {
        let t_bottom = t.y;
        let t_top = t.y + t.height;
        let t_cy = t.y + t.height * 0.5;
        let moving_edges: [(MovingEdge, f64, bool); 3] = [
            (MovingEdge::Bottom, m_bottom, allow_bottom),
            (MovingEdge::Top, m_top, allow_top),
            (MovingEdge::CenterY, m_cy, allow_cy),
        ];
        let target_points = [t_bottom, t_top, t_cy];
        for (edge, mv, enabled) in moving_edges.iter().copied() {
            if !enabled {
                continue;
            }
            for tp in target_points {
                let delta = tp - mv;
                if delta.abs() <= threshold {
                    let cand = AlignmentY {
                        delta,
                        y: tp,
                        edge,
                        target_rect: *t,
                    };
                    best = match best {
                        None => Some(cand),
                        Some(prev) if delta.abs() < prev.delta.abs() => Some(cand),
                        Some(prev) => Some(prev),
                    };
                }
            }
        }
    }
    best
}

fn apply_resize_x(rect: &mut Rect, edge: MovingEdge, delta: f64, active: ResizeEdge) {
    match edge {
        MovingEdge::Left if active.affects_left() => {
            rect.x += delta;
            rect.width -= delta;
        }
        MovingEdge::Right if active.affects_right() => {
            rect.width += delta;
        }
        _ => {}
    }
}

fn apply_resize_y(rect: &mut Rect, edge: MovingEdge, delta: f64, active: ResizeEdge) {
    match edge {
        MovingEdge::Bottom if active.affects_bottom() => {
            rect.y += delta;
            rect.height -= delta;
        }
        MovingEdge::Top if active.affects_top() => {
            rect.height += delta;
        }
        _ => {}
    }
}

/// Apply a horizontal resize-spacing delta to `rect`.  East-side
/// resizes grow `width`; west-side resizes shift `x` and shrink
/// `width` by the same amount so the right edge stays put.
fn apply_resize_x_spacing(rect: &mut Rect, edge: ResizeEdge, delta: f64) {
    if edge.affects_right() {
        rect.width += delta;
    } else if edge.affects_left() {
        rect.x += delta;
        rect.width -= delta;
    }
}

/// Apply a vertical resize-spacing delta to `rect`.  Mirrors
/// [`apply_resize_x_spacing`] on the Y axis.
fn apply_resize_y_spacing(rect: &mut Rect, edge: ResizeEdge, delta: f64) {
    if edge.affects_top() {
        rect.height += delta;
    } else if edge.affects_bottom() {
        rect.y += delta;
        rect.height -= delta;
    }
}

// ── Span helpers for guide line endpoints ────────────────────────

/// Y range that spans both the moving rect and a vertical-line
/// target — guide lines run end-to-end across both so the user can
/// see what aligned.
fn y_span(a: Rect, b: Rect) -> (f64, f64) {
    let y0 = a.y.min(b.y);
    let y1 = (a.y + a.height).max(b.y + b.height);
    (y0, y1)
}

fn x_span(a: Rect, b: Rect) -> (f64, f64) {
    let x0 = a.x.min(b.x);
    let x1 = (a.x + a.width).max(b.x + b.width);
    (x0, x1)
}
